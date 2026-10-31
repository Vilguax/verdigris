mod generator;
mod interaction;
mod level;
mod placement;
mod player;
mod portal;
mod raycast;
mod registry;
mod storage;

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use generator::{
    DEFAULT_SEED, GENERATOR_ENV, GENERATOR_FLAT, GENERATOR_NOISE, GenMetrics, GeneratorKind,
    GeneratorSetup, NoiseGenerator, SEED_ENV, parse_generator_kind, parse_seed,
    read_generator_kind, read_seed,
};
pub use level::{LEVEL_FILE, LevelConfig, LevelError, ensure as ensure_level, level_path};
pub use player::{GAMEMODE_ENV, PlayerState, read_gamemode, resolve_gamemode};
pub use registry::PlayerRegistry;
use registry::{EntitySpawn, OutboundSender, allocate_entity_id};
pub use storage::{
    CHUNKS_SUBDIR, ChunkStore, DEFAULT_WORLD_DIR, DiskLoader, LoadOutcome, REJECTED_SUFFIX,
    WORLD_ENV, world_dir,
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time::{Duration, Instant, MissedTickBehavior, interval, timeout, timeout_at};
use tracing::{Instrument, debug, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

use verdigris_protocol::play::packet_ids::clientbound as play_cb;
use verdigris_protocol::{
    COMPRESSION_THRESHOLD, EncodeError, FlatGenerator, GAME_EVENT_WAIT_FOR_CHUNKS, Intent,
    ListedPlayer, MovePlayerPos, MovePlayerPosRot, MovePlayerRot, MovePlayerStatus,
    PLAYER_ENTITY_TYPE, PROTOCOL_VERSION, SET_COMPRESSION_ID, Serverbound, State, StatusInfo,
    World, add_entity_payload, block_changed_ack_payload, block_update_payload,
    build_compressed_packet, build_packet, bundle_delimiter_payload, chunk_batch_finished_payload,
    chunk_batch_start_payload, chunk_coord, chunks_to_forget, chunks_to_load, client_has_core,
    config_disconnect_payload, container_set_slot_payload, decode_compressed_frame, decode_frame,
    decode_known_packs, decode_serverbound, forget_level_chunk_payload, game_event_payload,
    keep_alive_payload, known_packs_payload, login_disconnect_payload, login_play_payload,
    login_success_payload, network_data, offline_uuid, packet_ids, player_abilities_payload,
    player_info_remove_payload, player_info_update_gamemode_payload,
    player_info_update_init_payload, player_info_update_latency_payload, player_position_payload,
    pong_payload, random_session_id, read_var_int, registry_data_payload, remove_entities_payload,
    server_known_pack, set_chunk_cache_center_payload, set_compression_payload,
    set_entity_data_health_payload, set_held_slot_payload, status_json, status_response_payload,
    tab_list_payload, update_tags_payload, validate_name, write_chunk_payload,
};
use verdigris_protocol::{GAME_VERSION, WORLD_MAX_Y, WORLD_MIN_Y, is_air, is_fluid};

pub const DEFAULT_BIND: &str = "0.0.0.0:25565";
pub const DEFAULT_MOTD: &str = "Verdigris";
pub const DEFAULT_MAX_PLAYERS: i32 = 20;

pub const REASON_VERSION_INCOMPATIBLE: &str =
    "Version incompatible: ce serveur attend Minecraft 26.3 (protocole 777)";
pub const REASON_TRANSFER_UNSUPPORTED: &str = "Transfert non supporté";
pub const REASON_LOGIN_INVALID_NAME: &str = "login nom invalide";
pub const REASON_LOGIN_UNEXPECTED_PACKET: &str = "login paquet inattendu";
pub const REASON_LOGIN_TIMEOUT: &str = "login délai dépassé";
pub const REASON_CONFIG_UNEXPECTED: &str = "paquet de configuration inattendu";
pub const REASON_CONFIG_NOT_VANILLA: &str = "un client vanilla 26.3 est requis";
pub const REASON_CONFIG_TIMEOUT: &str = "délai de configuration dépassé";
const LOGIN_SUCCESS_ID: i32 = 0x02;

const PLAY_CHUNK_RADIUS: i32 = 8;
const PLAY_SIMULATION_DISTANCE: i32 = 8;
const PLAY_MAX_PLAYERS: i32 = 20;
const PLAY_TELEPORT_ID: i32 = 1;
const PLAY_SPAWN_XZ: f64 = 0.5;
const PLAY_SPAWN_FALLBACK: [f64; 3] = [PLAY_SPAWN_XZ, 0.0, PLAY_SPAWN_XZ];
const CHUNK_SEND_YIELD_STRIDE: usize = 8;
pub const REASON_PROTOCOL_ERROR_PREFIX: &str = "Paquet invalide: ";
pub const REASON_CONNECTION_LOST: &str = "Connexion interrompue";
pub const REASON_INVALID_SEQUENCE: &str = "séquence d'interaction négative";
pub const REASON_READ_TIMEOUT: &str = "Délai de lecture dépassé";
pub const REASON_KEEP_ALIVE_TIMEOUT: &str = "keep alive délai dépassé";

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const CONFIG_ACK_TIMEOUT: Duration = Duration::from_secs(30);
const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const ENTITY_TICK_INTERVAL: Duration = Duration::from_millis(50);
const LATENCY_BROADCAST_INTERVAL: Duration = Duration::from_secs(30);
const GEN_METRICS_INTERVAL: Duration = Duration::from_secs(30);
const PLAY_THRESHOLD: usize = COMPRESSION_THRESHOLD as usize;
const READ_CHUNK: usize = 4096;
const CLOSE_BUDGET: Duration = Duration::from_secs(1);
const DRAIN_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct KeepAliveConfig {
    pub interval: Duration,
    pub timeout: Duration,
    pub check_period: Duration,
}

impl Default for KeepAliveConfig {
    fn default() -> Self {
        KeepAliveConfig {
            interval: Duration::from_secs(3600),
            timeout: Duration::from_secs(7200),
            check_period: Duration::from_secs(1),
        }
    }
}

pub const PRODUCTION_KEEP_ALIVE: KeepAliveConfig = KeepAliveConfig {
    interval: Duration::from_secs(10),
    timeout: Duration::from_secs(15),
    check_period: Duration::from_secs(1),
};

struct Pending {
    id: i64,
    sent_at: Instant,
}

struct KeepAliveState {
    pending: Option<Pending>,
    last_sent: Instant,
    next_id: i64,
}

impl KeepAliveState {
    fn new(now: Instant) -> Self {
        KeepAliveState {
            pending: None,
            last_sent: now,
            next_id: 1,
        }
    }

    fn take_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(1);
        id
    }
}

enum KeepAliveAction {
    None,
    Send(i64),
    Timeout,
}

fn evaluate_keep_alive(
    state: &mut KeepAliveState,
    config: &KeepAliveConfig,
    now: Instant,
) -> KeepAliveAction {
    if let Some(pending) = &state.pending {
        if now.duration_since(pending.sent_at) >= config.timeout {
            return KeepAliveAction::Timeout;
        }
        return KeepAliveAction::None;
    }
    if now.duration_since(state.last_sent) >= config.interval {
        let id = state.take_id();
        state.pending = Some(Pending { id, sent_at: now });
        state.last_sent = now;
        return KeepAliveAction::Send(id);
    }
    KeepAliveAction::None
}

fn ack_keep_alive(state: &mut KeepAliveState, id: i64, now: Instant) -> Option<Duration> {
    match &state.pending {
        Some(pending) if pending.id == id => {
            let elapsed = now.duration_since(pending.sent_at);
            debug!(
                id,
                response_ms = elapsed.as_millis() as u64,
                "keep alive confirmé"
            );
            state.pending = None;
            Some(elapsed)
        }
        _ => {
            warn!(id, "keep alive inattendu ignoré");
            None
        }
    }
}

fn latency_ms_from(elapsed: Duration) -> i32 {
    elapsed.as_millis().min(i32::MAX as u128) as i32
}

fn frame_arc(id: i32, payload: &[u8]) -> Option<Arc<[u8]>> {
    match build_compressed_packet(id, payload, PLAY_THRESHOLD) {
        Ok(frame) => Some(Arc::from(frame.into_boxed_slice())),
        Err(err) => {
            warn!("diffusion impossible: {err}");
            None
        }
    }
}

fn push_frame(senders: &[OutboundSender], frame: &Arc<[u8]>) {
    for sender in senders {
        let _ = sender.send(Arc::clone(frame));
    }
}

fn spawn_bundle(spawn: &EntitySpawn) -> Vec<Arc<[u8]>> {
    let mut frames = Vec::with_capacity(4);
    if let Some(frame) = frame_arc(play_cb::BUNDLE_DELIMITER, &bundle_delimiter_payload()) {
        frames.push(frame);
    }
    let add_entity = add_entity_payload(
        spawn.entity_id,
        spawn.uuid,
        PLAYER_ENTITY_TYPE,
        spawn.x,
        spawn.y,
        spawn.z,
        spawn.pitch_byte,
        spawn.yaw_byte,
        spawn.yaw_byte,
        0,
    );
    if let Some(frame) = frame_arc(play_cb::ADD_ENTITY, &add_entity) {
        frames.push(frame);
    }
    if let Some(frame) = frame_arc(
        play_cb::SET_ENTITY_DATA,
        &set_entity_data_health_payload(spawn.entity_id),
    ) {
        frames.push(frame);
    }
    if let Some(frame) = frame_arc(play_cb::BUNDLE_DELIMITER, &bundle_delimiter_payload()) {
        frames.push(frame);
    }
    frames
}

fn push_bundle(sender: &OutboundSender, frames: &[Arc<[u8]>]) {
    for frame in frames {
        let _ = sender.send(Arc::clone(frame));
    }
}

pub fn broadcast_gamemode_change(registry: &Arc<PlayerRegistry>, uuid: u128, mode: i32) {
    let senders = registry.set_gamemode(uuid, mode);
    if senders.is_empty() {
        return;
    }
    if let Some(frame) = frame_arc(
        play_cb::PLAYER_INFO_UPDATE,
        &player_info_update_gamemode_payload(uuid, mode),
    ) {
        push_frame(&senders, &frame);
    }
}

fn run_entity_tick(registry: &Arc<PlayerRegistry>, threshold: usize) {
    for actions in registry.tick(threshold) {
        if !actions.removed.is_empty()
            && let Some(frame) = frame_arc(
                play_cb::REMOVE_ENTITIES,
                &remove_entities_payload(&actions.removed),
            )
        {
            let _ = actions.sender.send(frame);
        }
        for spawn in &actions.entered {
            push_bundle(&actions.sender, &spawn_bundle(spawn));
        }
        for frame in &actions.moved {
            let _ = actions.sender.send(Arc::clone(frame));
        }
    }
}

fn broadcast_latency(registry: &Arc<PlayerRegistry>) {
    let (all_entries, senders) = registry.latency_broadcast();
    let entries: Vec<(u128, i32)> = all_entries
        .into_iter()
        .filter(|&(_, latency_ms)| latency_ms > 0)
        .collect();
    if entries.is_empty() || senders.is_empty() {
        return;
    }
    if let Some(frame) = frame_arc(
        play_cb::PLAYER_INFO_UPDATE,
        &player_info_update_latency_payload(&entries),
    ) {
        push_frame(&senders, &frame);
    }
    debug!(joueurs = senders.len(), "latence diffusée");
}

struct PlayGuard {
    registry: Arc<PlayerRegistry>,
    uuid: u128,
}

impl Drop for PlayGuard {
    fn drop(&mut self) {
        let departure = self.registry.depart(self.uuid);
        if !departure.observers.is_empty()
            && let Some(frame) = frame_arc(
                play_cb::REMOVE_ENTITIES,
                &remove_entities_payload(&[departure.entity_id]),
            )
        {
            push_frame(&departure.observers, &frame);
        }
        if departure.remaining.is_empty() {
            return;
        }
        let remove_frame = frame_arc(
            play_cb::PLAYER_INFO_REMOVE,
            &player_info_remove_payload(&[self.uuid]),
        );
        let (header, footer) = self.registry.tab_strings();
        let tab_frame = frame_arc(play_cb::TAB_LIST, &tab_list_payload(&header, &footer));
        if let Some(frame) = &remove_frame {
            push_frame(&departure.remaining, frame);
        }
        if let Some(frame) = &tab_frame {
            push_frame(&departure.remaining, frame);
        }
    }
}

enum Next {
    Continue,
    EnterConfiguration,
    Terminate(String),
}

enum ReadOutcome {
    Packet(Vec<u8>),
    Eof,
    Protocol(String),
    Timeout,
}

enum Framing {
    Uncompressed,
    Compressed(usize),
}

enum WriteError {
    Encode(EncodeError),
    Io(std::io::Error),
}

impl core::fmt::Display for WriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WriteError::Encode(err) => write!(f, "encode: {err}"),
            WriteError::Io(err) => write!(f, "io: {err}"),
        }
    }
}

pub fn build_env_filter(value: Option<&str>) -> EnvFilter {
    match value {
        Some(raw) if !raw.trim().is_empty() => {
            EnvFilter::try_new(raw).unwrap_or_else(|_| EnvFilter::new("info"))
        }
        _ => EnvFilter::new("info"),
    }
}

pub fn init_logging() {
    let filter = build_env_filter(std::env::var("VERDIGRIS_LOG").ok().as_deref());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn peek_id(body: &[u8]) -> i32 {
    match read_var_int(body) {
        Ok(Some((id, _))) => id,
        _ => -1,
    }
}

fn trace_packet(message: &'static str, state: State, id: i32, size: usize) {
    debug!(state = ?state, id = %format!("{id:#x}"), size, "{message}");
}

fn set_state(state: &mut State, next: State) {
    *state = next;
    info!(status = ?next, "transition d'état");
}

pub async fn serve(bind: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let store = Arc::new(ChunkStore::from_env());
    let kind = read_generator_kind().map_err(std::io::Error::other)?;
    let seed = read_seed().map_err(std::io::Error::other)?;

    let requested = LevelConfig {
        generator: kind.as_str().to_owned(),
        seed,
        game_version: GAME_VERSION.to_owned(),
    };
    ensure_level(store.base(), &requested).map_err(std::io::Error::other)?;

    let setup = GeneratorSetup::build(kind, seed).map_err(std::io::Error::other)?;
    let metrics = setup.metrics();
    let generator = setup.into_generator().map_err(std::io::Error::other)?;
    let loader = DiskLoader::new(Arc::clone(&store));
    let world = Arc::new(World::with_loader(generator, Box::new(loader)));
    info!(
        monde = %store.base().display(),
        generateur = kind.as_str(),
        seed,
        "monde partagé prêt"
    );

    run_core(
        listener,
        PRODUCTION_KEEP_ALIVE,
        world,
        store,
        Arc::new(PlayerRegistry::new(DEFAULT_MOTD, DEFAULT_MAX_PLAYERS)),
        metrics,
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    )
    .await
}

pub async fn run(listener: TcpListener) -> std::io::Result<()> {
    run_with_config(listener, KeepAliveConfig::default()).await
}

pub async fn run_with_config(
    listener: TcpListener,
    keep_alive: KeepAliveConfig,
) -> std::io::Result<()> {
    run_with_shutdown(listener, keep_alive, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

pub async fn run_with_shutdown<F>(
    listener: TcpListener,
    keep_alive: KeepAliveConfig,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()>,
{
    run_with_store(
        listener,
        keep_alive,
        Arc::new(ChunkStore::from_env()),
        shutdown,
    )
    .await
}

pub async fn run_with_store<F>(
    listener: TcpListener,
    keep_alive: KeepAliveConfig,
    store: Arc<ChunkStore>,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()>,
{
    let players = Arc::new(PlayerRegistry::new(DEFAULT_MOTD, DEFAULT_MAX_PLAYERS));
    let generator = FlatGenerator::new().map_err(std::io::Error::other)?;
    let loader = DiskLoader::new(Arc::clone(&store));
    let world = Arc::new(World::with_loader(Box::new(generator), Box::new(loader)));
    info!(monde = %store.base().display(), "monde plat partagé prêt");
    run_core(listener, keep_alive, world, store, players, None, shutdown).await
}

pub async fn run_with_registry<F>(
    listener: TcpListener,
    keep_alive: KeepAliveConfig,
    players: Arc<PlayerRegistry>,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()>,
{
    let store = Arc::new(ChunkStore::from_env());
    let generator = FlatGenerator::new().map_err(std::io::Error::other)?;
    let loader = DiskLoader::new(Arc::clone(&store));
    let world = Arc::new(World::with_loader(Box::new(generator), Box::new(loader)));
    info!(monde = %store.base().display(), "monde plat partagé prêt");
    run_core(listener, keep_alive, world, store, players, None, shutdown).await
}

pub fn flush_dirty(world: &World, store: &ChunkStore) -> usize {
    let pending = world.take_dirty();
    let mut written = 0usize;
    for (x, z, column) in pending {
        match store.save_chunk(x, z, &column) {
            Ok(()) => written += 1,
            Err(err) => {
                error!(x, z, erreur = %err, "écriture de chunk échouée, remis en attente");
                world.mark_dirty(x, z);
            }
        }
    }
    info!(chunks_ecrits = written, "flush");
    written
}

async fn run_core<F>(
    listener: TcpListener,
    keep_alive: KeepAliveConfig,
    world: Arc<World>,
    store: Arc<ChunkStore>,
    players: Arc<PlayerRegistry>,
    gen_metrics: Option<Arc<GenMetrics>>,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()>,
{
    let counter = AtomicU64::new(0);
    let mut ticker = interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ticker.tick().await;
    let mut latency_ticker = interval(LATENCY_BROADCAST_INTERVAL);
    latency_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    latency_ticker.tick().await;
    let mut gen_ticker = interval(GEN_METRICS_INTERVAL);
    gen_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    gen_ticker.tick().await;
    let mut entity_ticker = interval(ENTITY_TICK_INTERVAL);
    entity_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    entity_ticker.tick().await;
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    if let Err(error) = stream.set_nodelay(true) {
                        warn!(%peer, %error, "TCP_NODELAY non applique");
                    }
                    let conn_id = counter.fetch_add(1, Ordering::Relaxed);
                    let span = info_span!("conn", conn_id, peer = %peer);
                    let world = Arc::clone(&world);
                    let players = Arc::clone(&players);
                    tokio::spawn(
                        async move {
                            handle_connection(stream, keep_alive, world, players).await;
                        }
                        .instrument(span),
                    );
                }
                Err(err) => {
                    error!("accept error: {err}");
                }
            },
            _ = ticker.tick() => {
                flush_dirty(&world, &store);
            }
            _ = latency_ticker.tick() => {
                broadcast_latency(&players);
            }
            _ = gen_ticker.tick() => {
                log_gen_metrics(gen_metrics.as_deref());
            }
            _ = entity_ticker.tick() => {
                run_entity_tick(&players, PLAY_THRESHOLD);
            }
            _ = &mut shutdown => break,
        }
    }

    info!("arrêt demandé, flush final");
    flush_dirty(&world, &store);
    Ok(())
}

fn log_gen_metrics(metrics: Option<&GenMetrics>) {
    let Some(metrics) = metrics else {
        return;
    };
    let (count, nanos) = metrics.take();
    if count == 0 {
        info!(
            fenetre_s = GEN_METRICS_INTERVAL.as_secs(),
            "génération: aucun chunk sur l'intervalle"
        );
        return;
    }
    let avg_us = (nanos / count) as f64 / 1000.0;
    info!(
        chunks = count,
        moyenne_ms = avg_us / 1000.0,
        fenetre_s = GEN_METRICS_INTERVAL.as_secs(),
        "génération de chunks"
    );
}

async fn handle_connection(
    stream: TcpStream,
    keep_alive: KeepAliveConfig,
    world: Arc<World>,
    players: Arc<PlayerRegistry>,
) {
    let started = Instant::now();
    info!("connexion acceptée");
    serve_connection(stream, keep_alive, world, players).await;
    info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        "fin de connexion"
    );
}

async fn serve_connection(
    mut stream: TcpStream,
    keep_alive: KeepAliveConfig,
    world: Arc<World>,
    players: Arc<PlayerRegistry>,
) {
    let mut buf: Vec<u8> = Vec::new();
    let mut state = State::Handshaking;
    let mut framing = Framing::Uncompressed;
    let mut awaiting_ack = false;
    let mut player: Option<PlayerState> = None;
    let mut chunk = [0u8; READ_CHUNK];

    loop {
        loop {
            let (body, consumed) = match decode_any(&framing, &buf) {
                Ok(Some((body, consumed))) => (body, consumed),
                Ok(None) => break,
                Err(err) => {
                    let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                    disconnect(&mut stream, &framing, state, &reason).await;
                    return;
                }
            };
            buf.drain(..consumed);
            trace_packet("paquet reçu", state, peek_id(&body), body.len());
            match dispatch(
                &mut stream,
                &mut state,
                &mut framing,
                &mut awaiting_ack,
                &mut player,
                &body,
            )
            .await
            {
                Next::Continue => {}
                Next::EnterConfiguration => {
                    let Some(player) = player.take() else {
                        disconnect(&mut stream, &framing, state, REASON_LOGIN_UNEXPECTED_PACKET)
                            .await;
                        return;
                    };
                    let handoff = PlayHandoff {
                        keep_alive,
                        world: &world,
                        player,
                        registry: &players,
                    };
                    run_configuration(
                        &mut stream,
                        &framing,
                        &mut state,
                        &mut buf,
                        &mut chunk,
                        handoff,
                    )
                    .await;
                    return;
                }
                Next::Terminate(reason) => {
                    disconnect(&mut stream, &framing, state, &reason).await;
                    return;
                }
            }
        }

        let read = timeout(READ_TIMEOUT, stream.read(&mut chunk)).await;
        let n = match read {
            Ok(Ok(0)) => {
                disconnect(&mut stream, &framing, state, REASON_CONNECTION_LOST).await;
                return;
            }
            Ok(Ok(n)) => n,
            Ok(Err(err)) => {
                let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                disconnect(&mut stream, &framing, state, &reason).await;
                return;
            }
            Err(_) => {
                let reason = if awaiting_ack {
                    REASON_LOGIN_TIMEOUT
                } else {
                    REASON_READ_TIMEOUT
                };
                disconnect(&mut stream, &framing, state, reason).await;
                return;
            }
        };
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn decode_any(
    framing: &Framing,
    buf: &[u8],
) -> Result<Option<(Vec<u8>, usize)>, verdigris_protocol::ProtocolError> {
    match framing {
        Framing::Uncompressed => match decode_frame(buf)? {
            Some((body, consumed)) => Ok(Some((body.to_vec(), consumed))),
            None => Ok(None),
        },
        Framing::Compressed(threshold) => decode_compressed_frame(buf, *threshold),
    }
}

async fn dispatch(
    stream: &mut TcpStream,
    state: &mut State,
    framing: &mut Framing,
    awaiting_ack: &mut bool,
    player: &mut Option<PlayerState>,
    body: &[u8],
) -> Next {
    if *awaiting_ack {
        return finish_login(state, awaiting_ack, body);
    }

    let packet = match decode_serverbound(*state, body) {
        Ok(packet) => packet,
        Err(err) => return Next::Terminate(format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}")),
    };

    match packet {
        Serverbound::Handshake(handshake) => {
            info!(
                protocol = handshake.protocol_version,
                intent = ?handshake.intent,
                "handshake"
            );
            match handshake.intent {
                Intent::Status => {
                    set_state(state, State::Status);
                    Next::Continue
                }
                Intent::Login => {
                    set_state(state, State::Login);
                    if handshake.protocol_version != PROTOCOL_VERSION {
                        Next::Terminate(REASON_VERSION_INCOMPATIBLE.to_owned())
                    } else {
                        Next::Continue
                    }
                }
                Intent::Transfer => {
                    set_state(state, State::Login);
                    Next::Terminate(REASON_TRANSFER_UNSUPPORTED.to_owned())
                }
                Intent::Unknown(_) => Next::Terminate(String::new()),
            }
        }
        Serverbound::StatusRequest => {
            let info = StatusInfo {
                motd: DEFAULT_MOTD,
                max_players: DEFAULT_MAX_PLAYERS,
                ..StatusInfo::default()
            };
            let json = status_json(&info);
            match status_response_payload(&json) {
                Ok(payload) => match write_packet(stream, framing, *state, 0x00, &payload).await {
                    Ok(()) => Next::Continue,
                    Err(_) => Next::Terminate(String::new()),
                },
                Err(err) => Next::Terminate(format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}")),
            }
        }
        Serverbound::PingRequest { payload } => {
            let _ = write_packet(stream, framing, *state, 0x01, &pong_payload(payload)).await;
            Next::Terminate(String::new())
        }
        Serverbound::LoginStart { name, .. } => {
            begin_login(stream, framing, *state, awaiting_ack, player, &name).await
        }
        Serverbound::LoginAcknowledged => {
            Next::Terminate(REASON_LOGIN_UNEXPECTED_PACKET.to_owned())
        }
        Serverbound::PlayAcceptTeleportation(_)
        | Serverbound::PlayerLoaded
        | Serverbound::PlayChunkBatchReceived(_)
        | Serverbound::PlayKeepAlive(_)
        | Serverbound::PlayMovePlayerPos(_)
        | Serverbound::PlayMovePlayerPosRot(_)
        | Serverbound::PlayMovePlayerRot(_)
        | Serverbound::PlayMovePlayerStatus(_)
        | Serverbound::PlayPlayerAbilities { .. }
        | Serverbound::PlayPlayerAction(_)
        | Serverbound::PlayPlayerCommand(_)
        | Serverbound::PlayPlayerInput(_)
        | Serverbound::PlayUseItemOn(_)
        | Serverbound::PlayUseItem(_)
        | Serverbound::PlaySetCarriedItem(_)
        | Serverbound::PlaySetCreativeModeSlot(_)
        | Serverbound::PlayPickItemFromBlock(_)
        | Serverbound::PlayIgnored { .. } => {
            Next::Terminate(REASON_LOGIN_UNEXPECTED_PACKET.to_owned())
        }
    }
}

async fn begin_login(
    stream: &mut TcpStream,
    framing: &mut Framing,
    state: State,
    awaiting_ack: &mut bool,
    player: &mut Option<PlayerState>,
    name: &str,
) -> Next {
    info!(player = name, "login start");
    if validate_name(name).is_err() {
        return Next::Terminate(REASON_LOGIN_INVALID_NAME.to_owned());
    }
    let set_compression = set_compression_payload(COMPRESSION_THRESHOLD);
    if write_packet(stream, framing, state, SET_COMPRESSION_ID, &set_compression)
        .await
        .is_err()
    {
        return Next::Terminate(String::new());
    }
    *framing = Framing::Compressed(COMPRESSION_THRESHOLD as usize);
    info!(threshold = COMPRESSION_THRESHOLD, "compression activée");
    let uuid = offline_uuid(name);
    let session_id = random_session_id();
    let payload = login_success_payload(uuid, name, session_id);
    match write_packet(stream, framing, state, LOGIN_SUCCESS_ID, &payload).await {
        Ok(()) => {
            *player = Some(PlayerState::connect(name));
            *awaiting_ack = true;
            Next::Continue
        }
        Err(_) => Next::Terminate(String::new()),
    }
}

fn finish_login(state: &mut State, awaiting_ack: &mut bool, body: &[u8]) -> Next {
    match decode_serverbound(State::Login, body) {
        Ok(Serverbound::LoginAcknowledged) => {
            *awaiting_ack = false;
            set_state(state, State::Configuration);
            info!("entrée en configuration");
            Next::EnterConfiguration
        }
        _ => Next::Terminate(REASON_LOGIN_UNEXPECTED_PACKET.to_owned()),
    }
}

fn is_ignorable_config(id: i32) -> bool {
    id == packet_ids::serverbound::CLIENT_INFORMATION
        || id == packet_ids::serverbound::CUSTOM_PAYLOAD
        || id == packet_ids::serverbound::KEEP_ALIVE
}

fn split_packet(body: &[u8]) -> Option<(i32, &[u8])> {
    match read_var_int(body) {
        Ok(Some((id, header))) => Some((id, &body[header..])),
        _ => None,
    }
}

async fn read_config_packet(
    stream: &mut TcpStream,
    threshold: usize,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    deadline: Instant,
) -> ReadOutcome {
    loop {
        match decode_compressed_frame(buf, threshold) {
            Ok(Some((body, consumed))) => {
                buf.drain(..consumed);
                return ReadOutcome::Packet(body);
            }
            Ok(None) => {}
            Err(err) => {
                return ReadOutcome::Protocol(format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}"));
            }
        }
        match timeout_at(deadline, stream.read(chunk)).await {
            Ok(Ok(0)) => return ReadOutcome::Eof,
            Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
            Ok(Err(err)) => {
                return ReadOutcome::Protocol(format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}"));
            }
            Err(_) => return ReadOutcome::Timeout,
        }
    }
}

struct PlayHandoff<'a> {
    keep_alive: KeepAliveConfig,
    world: &'a Arc<World>,
    player: PlayerState,
    registry: &'a Arc<PlayerRegistry>,
}

async fn run_configuration(
    stream: &mut TcpStream,
    framing: &Framing,
    state: &mut State,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    handoff: PlayHandoff<'_>,
) {
    let threshold = match framing {
        Framing::Compressed(threshold) => *threshold,
        Framing::Uncompressed => {
            disconnect(stream, framing, *state, REASON_CONFIG_UNEXPECTED).await;
            return;
        }
    };

    let packs = [server_known_pack()];
    let payload = known_packs_payload(&packs);
    if write_packet(
        stream,
        framing,
        *state,
        packet_ids::clientbound::SELECT_KNOWN_PACKS,
        &payload,
    )
    .await
    .is_err()
    {
        return;
    }

    let client_packs = loop {
        let deadline = Instant::now() + READ_TIMEOUT;
        match read_config_packet(stream, threshold, buf, chunk, deadline).await {
            ReadOutcome::Packet(body) => {
                let Some((id, rest)) = split_packet(&body) else {
                    disconnect(stream, framing, *state, REASON_CONFIG_UNEXPECTED).await;
                    return;
                };
                if is_ignorable_config(id) {
                    continue;
                }
                if id == packet_ids::serverbound::SELECT_KNOWN_PACKS {
                    match decode_known_packs(rest) {
                        Ok(packs) => break packs,
                        Err(err) => {
                            let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                            disconnect(stream, framing, *state, &reason).await;
                            return;
                        }
                    }
                }
                disconnect(stream, framing, *state, REASON_CONFIG_UNEXPECTED).await;
                return;
            }
            ReadOutcome::Eof => {
                disconnect(stream, framing, *state, REASON_CONNECTION_LOST).await;
                return;
            }
            ReadOutcome::Protocol(reason) => {
                disconnect(stream, framing, *state, &reason).await;
                return;
            }
            ReadOutcome::Timeout => {
                disconnect(stream, framing, *state, REASON_READ_TIMEOUT).await;
                return;
            }
        }
    };

    info!(packs = ?client_packs, "known packs reçus");
    if !client_has_core(&client_packs) {
        disconnect(stream, framing, *state, REASON_CONFIG_NOT_VANILLA).await;
        return;
    }

    let data = network_data();
    for registry in &data.registries {
        let payload = registry_data_payload(registry);
        if write_packet(
            stream,
            framing,
            *state,
            packet_ids::clientbound::REGISTRY_DATA,
            &payload,
        )
        .await
        .is_err()
        {
            return;
        }
    }
    info!(registres = data.registries.len(), "registry data envoyés");

    let tags_payload = update_tags_payload(&data.tags);
    if write_packet(
        stream,
        framing,
        *state,
        packet_ids::clientbound::UPDATE_TAGS,
        &tags_payload,
    )
    .await
    .is_err()
    {
        return;
    }
    let tag_count: usize = data.tags.iter().map(|registry| registry.tags.len()).sum();
    info!(tags = tag_count, "update tags envoyé");

    if write_packet(
        stream,
        framing,
        *state,
        packet_ids::clientbound::FINISH_CONFIGURATION,
        &[],
    )
    .await
    .is_err()
    {
        return;
    }

    let ack_deadline = Instant::now() + CONFIG_ACK_TIMEOUT;
    loop {
        match read_config_packet(stream, threshold, buf, chunk, ack_deadline).await {
            ReadOutcome::Packet(body) => {
                let id = peek_id(&body);
                if is_ignorable_config(id) {
                    continue;
                }
                if id == packet_ids::serverbound::FINISH_CONFIGURATION {
                    break;
                }
                disconnect(stream, framing, *state, REASON_CONFIG_UNEXPECTED).await;
                return;
            }
            ReadOutcome::Eof => {
                disconnect(stream, framing, *state, REASON_CONNECTION_LOST).await;
                return;
            }
            ReadOutcome::Protocol(reason) => {
                disconnect(stream, framing, *state, &reason).await;
                return;
            }
            ReadOutcome::Timeout => {
                disconnect(stream, framing, *state, REASON_CONFIG_TIMEOUT).await;
                return;
            }
        }
    }

    set_state(state, State::Play);
    info!("entrée en jeu");
    enter_play(stream, framing, state, buf, chunk, handoff).await;
}

struct PlayerView {
    x: f64,
    y: f64,
    z: f64,
    flags: u8,
    center: (i32, i32),
    sent: HashSet<(i32, i32)>,
}

impl PlayerView {
    fn new(spawn: [f64; 3]) -> Self {
        PlayerView {
            x: spawn[0],
            y: spawn[1],
            z: spawn[2],
            flags: 0,
            center: (chunk_coord(spawn[0]), chunk_coord(spawn[2])),
            sent: HashSet::new(),
        }
    }

    fn chunk_changed(&self) -> bool {
        (chunk_coord(self.x), chunk_coord(self.z)) != self.center
    }

    fn log_position(&self, kind: &'static str) {
        debug!(
            x = self.x,
            y = self.y,
            z = self.z,
            flags = self.flags,
            "{kind}"
        );
    }

    fn apply_pos(&mut self, m: &MovePlayerPos) {
        self.x = m.x;
        self.y = m.y;
        self.z = m.z;
        self.flags = m.flags;
    }

    fn apply_pos_rot(&mut self, m: &MovePlayerPosRot) {
        self.x = m.x;
        self.y = m.y;
        self.z = m.z;
        self.flags = m.flags;
    }

    fn apply_rot(&mut self, m: &MovePlayerRot) {
        self.flags = m.flags;
    }

    fn apply_status(&mut self, m: &MovePlayerStatus) {
        self.flags = m.flags;
    }
}

struct Session<'a> {
    view: PlayerView,
    world: &'a Arc<World>,
    player: PlayerState,
    uuid: u128,
    registry: Arc<PlayerRegistry>,
}

async fn enter_play(
    stream: &mut TcpStream,
    framing: &Framing,
    state: &mut State,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    handoff: PlayHandoff<'_>,
) {
    let PlayHandoff {
        keep_alive,
        world,
        mut player,
        registry,
    } = handoff;
    let spawn = {
        let world = Arc::clone(world);
        tokio::task::spawn_blocking(move || spawn_position(&world))
            .await
            .unwrap_or(PLAY_SPAWN_FALLBACK)
    };
    player.pos = (spawn[0], spawn[1], spawn[2]);
    let entity_id = allocate_entity_id();
    info!(player = %player.name, entity_id, mode = %player.mode, "entrée en jeu du joueur");

    let login = match login_play_payload(
        entity_id,
        PLAY_CHUNK_RADIUS,
        PLAY_SIMULATION_DISTANCE,
        player.mode.game_type(),
        PLAY_MAX_PLAYERS,
    ) {
        Ok(payload) => payload,
        Err(err) => {
            let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
            disconnect(stream, framing, *state, &reason).await;
            return;
        }
    };
    if write_packet(stream, framing, *state, play_cb::LOGIN, &login)
        .await
        .is_err()
    {
        return;
    }

    let abilities = player_abilities_payload(player.mode.abilities_flags());
    if write_packet(
        stream,
        framing,
        *state,
        play_cb::PLAYER_ABILITIES,
        &abilities,
    )
    .await
    .is_err()
    {
        return;
    }

    let position = player_position_payload(PLAY_TELEPORT_ID, spawn, [0.0; 3], 0.0, 0.0, 0);
    if write_packet(stream, framing, *state, play_cb::PLAYER_POSITION, &position)
        .await
        .is_err()
    {
        return;
    }
    info!(
        x = spawn[0],
        y = spawn[1],
        z = spawn[2],
        teleport_id = PLAY_TELEPORT_ID,
        "position de spawn"
    );

    let game_event = game_event_payload(GAME_EVENT_WAIT_FOR_CHUNKS, 0.0);
    if write_packet(stream, framing, *state, play_cb::GAME_EVENT, &game_event)
        .await
        .is_err()
    {
        return;
    }

    let mut view = PlayerView::new(spawn);
    if update_chunks(stream, framing, *state, &mut view, world).await {
        return;
    }

    let uuid = offline_uuid(&player.name);
    let (tx, rx) = unbounded_channel::<Arc<[u8]>>();
    let snapshot = registry.register(
        uuid,
        entity_id,
        (spawn[0], spawn[1], spawn[2]),
        player.name.clone(),
        player.mode.game_type(),
        0,
        tx,
    );
    registry.update_sent_chunks(uuid, &view.sent);
    let _guard = PlayGuard {
        registry: Arc::clone(registry),
        uuid,
    };

    let own = ListedPlayer {
        uuid,
        name: player.name.clone(),
        game_mode: player.mode.game_type(),
        latency_ms: 0,
        listed: true,
    };

    let mut everyone = snapshot.existing;
    everyone.push(own.clone());
    let init_all = match player_info_update_init_payload(&everyone) {
        Ok(payload) => payload,
        Err(err) => {
            let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
            disconnect(stream, framing, *state, &reason).await;
            return;
        }
    };
    if write_packet(
        stream,
        framing,
        *state,
        play_cb::PLAYER_INFO_UPDATE,
        &init_all,
    )
    .await
    .is_err()
    {
        return;
    }

    let (header, footer) = registry.tab_strings();
    let tab = tab_list_payload(&header, &footer);
    if write_packet(stream, framing, *state, play_cb::TAB_LIST, &tab)
        .await
        .is_err()
    {
        return;
    }

    if !snapshot.other_senders.is_empty() {
        let init_self = match player_info_update_init_payload(std::slice::from_ref(&own)) {
            Ok(payload) => payload,
            Err(err) => {
                let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                disconnect(stream, framing, *state, &reason).await;
                return;
            }
        };
        if let Some(frame) = frame_arc(play_cb::PLAYER_INFO_UPDATE, &init_self) {
            push_frame(&snapshot.other_senders, &frame);
        }
        if let Some(frame) = frame_arc(play_cb::TAB_LIST, &tab) {
            push_frame(&snapshot.other_senders, &frame);
        }
    }

    registry.activate(uuid);

    let session = Session {
        view,
        world,
        player,
        uuid,
        registry: Arc::clone(registry),
    };
    run_play(stream, framing, state, buf, chunk, keep_alive, rx, session).await;
}

fn spawn_position(world: &World) -> [f64; 3] {
    let mut top = WORLD_MIN_Y - 1;
    for y in (WORLD_MIN_Y..=WORLD_MAX_Y).rev() {
        let state = world.state_at(0, y, 0) as i32;
        if !is_air(state) && !is_fluid(state) {
            top = y;
            break;
        }
    }
    [PLAY_SPAWN_XZ, f64::from(top + 1), PLAY_SPAWN_XZ]
}

async fn update_chunks(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    view: &mut PlayerView,
    world: &Arc<World>,
) -> bool {
    let center = (chunk_coord(view.x), chunk_coord(view.z));
    view.center = center;

    if write_packet(
        stream,
        framing,
        state,
        play_cb::SET_CHUNK_CACHE_CENTER,
        &set_chunk_cache_center_payload(center.0, center.1),
    )
    .await
    .is_err()
    {
        return true;
    }

    let forget = chunks_to_forget(center, &view.sent);
    for &(x, z) in &forget {
        if write_packet(
            stream,
            framing,
            state,
            play_cb::FORGET_LEVEL_CHUNK,
            &forget_level_chunk_payload(x, z),
        )
        .await
        .is_err()
        {
            return true;
        }
        view.sent.remove(&(x, z));
    }

    let load = chunks_to_load(center, &view.sent);
    if write_packet(
        stream,
        framing,
        state,
        play_cb::CHUNK_BATCH_START,
        &chunk_batch_start_payload(),
    )
    .await
    .is_err()
    {
        return true;
    }

    for (sent_count, &(x, z)) in load.iter().enumerate() {
        let world = Arc::clone(world);
        let payload = match tokio::task::spawn_blocking(move || {
            let column = world.chunk(x, z);
            let mut payload = Vec::new();
            let column = column.lock().unwrap();
            write_chunk_payload(x, z, &column, &mut payload);
            payload
        })
        .await
        {
            Ok(payload) => payload,
            Err(err) => {
                error!(x, z, erreur = %err, "génération de chunk interrompue");
                return true;
            }
        };
        if write_packet(
            stream,
            framing,
            state,
            play_cb::LEVEL_CHUNK_WITH_LIGHT,
            &payload,
        )
        .await
        .is_err()
        {
            return true;
        }
        view.sent.insert((x, z));
        if sent_count % CHUNK_SEND_YIELD_STRIDE == CHUNK_SEND_YIELD_STRIDE - 1 {
            tokio::task::yield_now().await;
        }
    }

    if write_packet(
        stream,
        framing,
        state,
        play_cb::CHUNK_BATCH_FINISHED,
        &chunk_batch_finished_payload(load.len() as i32),
    )
    .await
    .is_err()
    {
        return true;
    }

    info!(
        center_x = center.0,
        center_z = center.1,
        forgotten = forget.len(),
        loaded = load.len(),
        "chunks mis à jour"
    );
    false
}

enum PlayStep {
    Timer,
    Outbound(Option<Arc<[u8]>>),
    Read(std::io::Result<usize>),
}

#[allow(clippy::too_many_arguments)]
async fn run_play(
    stream: &mut TcpStream,
    framing: &Framing,
    state: &mut State,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    keep_alive: KeepAliveConfig,
    mut rx: UnboundedReceiver<Arc<[u8]>>,
    mut session: Session<'_>,
) {
    let threshold = match framing {
        Framing::Compressed(threshold) => *threshold,
        Framing::Uncompressed => {
            disconnect(stream, framing, *state, REASON_CONFIG_UNEXPECTED).await;
            return;
        }
    };

    let mut ka = KeepAliveState::new(Instant::now());
    let mut ticker = interval(keep_alive.check_period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        loop {
            match decode_compressed_frame(buf, threshold) {
                Ok(Some((body, consumed))) => {
                    buf.drain(..consumed);
                    match decode_serverbound(State::Play, &body) {
                        Ok(packet) => {
                            if handle_play_packet(
                                stream,
                                framing,
                                *state,
                                packet,
                                &mut ka,
                                &mut session,
                                Instant::now(),
                            )
                            .await
                            {
                                return;
                            }
                        }
                        Err(err) => {
                            let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                            disconnect(stream, framing, *state, &reason).await;
                            return;
                        }
                    }
                    if run_keep_alive_action(stream, framing, *state, &mut ka, &keep_alive).await {
                        return;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                    disconnect(stream, framing, *state, &reason).await;
                    return;
                }
            }
        }

        let step = tokio::select! {
            _ = ticker.tick() => PlayStep::Timer,
            outbound = rx.recv() => PlayStep::Outbound(outbound),
            read = stream.read(chunk) => PlayStep::Read(read),
        };

        match step {
            PlayStep::Timer => {
                if run_keep_alive_action(stream, framing, *state, &mut ka, &keep_alive).await {
                    return;
                }
            }
            PlayStep::Outbound(Some(bytes)) => {
                if stream.write_all(&bytes).await.is_err() {
                    disconnect(stream, framing, *state, REASON_CONNECTION_LOST).await;
                    return;
                }
            }
            PlayStep::Outbound(None) => return,
            PlayStep::Read(Ok(0)) => {
                disconnect(stream, framing, *state, REASON_CONNECTION_LOST).await;
                return;
            }
            PlayStep::Read(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
            PlayStep::Read(Err(err)) => {
                let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                disconnect(stream, framing, *state, &reason).await;
                return;
            }
        }
    }
}

async fn run_keep_alive_action(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    ka: &mut KeepAliveState,
    config: &KeepAliveConfig,
) -> bool {
    match evaluate_keep_alive(ka, config, Instant::now()) {
        KeepAliveAction::None => false,
        KeepAliveAction::Send(id) => {
            let payload = keep_alive_payload(id);
            write_packet(stream, framing, state, play_cb::KEEP_ALIVE, &payload)
                .await
                .is_err()
        }
        KeepAliveAction::Timeout => {
            disconnect(stream, framing, state, REASON_KEEP_ALIVE_TIMEOUT).await;
            true
        }
    }
}

fn publish_orientation(session: &Session<'_>) {
    let (yaw, pitch, sneaking) = session.player.orientation();
    session
        .registry
        .set_orientation(session.uuid, yaw, pitch, sneaking);
}

async fn handle_play_packet(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    packet: Serverbound,
    ka: &mut KeepAliveState,
    session: &mut Session<'_>,
    now: Instant,
) -> bool {
    match packet {
        Serverbound::PlayAcceptTeleportation(accept) => {
            if accept.teleport_id == PLAY_TELEPORT_ID {
                info!(teleport_id = accept.teleport_id, "téléportation confirmée");
            } else {
                warn!(
                    recu = accept.teleport_id,
                    attendu = PLAY_TELEPORT_ID,
                    "téléportation inattendue"
                );
            }
            false
        }
        Serverbound::PlayerLoaded => {
            info!("joueur chargé");
            false
        }
        Serverbound::PlayChunkBatchReceived(received) => {
            info!(
                chunks_par_tick = received.desired_chunks_per_tick as f64,
                "lot de chunks reçu"
            );
            false
        }
        Serverbound::PlayKeepAlive(keep_alive) => {
            if let Some(elapsed) = ack_keep_alive(ka, keep_alive.id, now) {
                session
                    .registry
                    .set_latency(session.uuid, latency_ms_from(elapsed));
            }
            false
        }
        Serverbound::PlayMovePlayerPos(m) => {
            session.view.apply_pos(&m);
            session.player.pos = (m.x, m.y, m.z);
            session
                .registry
                .set_position(session.uuid, m.x, m.y, m.z, on_ground_flag(m.flags));
            session.view.log_position("déplacement");
            follow_player(stream, framing, state, session).await
        }
        Serverbound::PlayMovePlayerPosRot(m) => {
            if !(m.yaw.is_finite() && m.pitch.is_finite()) {
                debug!("rotation non finie ignorée");
                return false;
            }
            session.view.apply_pos_rot(&m);
            session.player.pos = (m.x, m.y, m.z);
            session.player.yaw = m.yaw;
            session.player.pitch = m.pitch;
            publish_orientation(session);
            session
                .registry
                .set_position(session.uuid, m.x, m.y, m.z, on_ground_flag(m.flags));
            session.view.log_position("déplacement et rotation");
            follow_player(stream, framing, state, session).await
        }
        Serverbound::PlayMovePlayerRot(m) => {
            if !(m.yaw.is_finite() && m.pitch.is_finite()) {
                debug!("rotation non finie ignorée");
                return false;
            }
            session.view.apply_rot(&m);
            session.player.yaw = m.yaw;
            session.player.pitch = m.pitch;
            publish_orientation(session);
            session
                .registry
                .set_ground(session.uuid, on_ground_flag(m.flags));
            session.view.log_position("rotation");
            false
        }
        Serverbound::PlayMovePlayerStatus(m) => {
            session.view.apply_status(&m);
            session
                .registry
                .set_ground(session.uuid, on_ground_flag(m.flags));
            session.view.log_position("statut au sol");
            false
        }
        Serverbound::PlayPlayerAbilities { flying } => {
            if session.player.mode.can_fly() {
                session.player.flying = flying;
                debug!(flying, mode = %session.player.mode, "vol mis à jour");
            } else {
                debug!(
                    flying,
                    mode = %session.player.mode,
                    "vol ignoré pour un mode sans vol"
                );
            }
            false
        }
        Serverbound::PlayPlayerAction(action) => {
            let result = session.player.handle_player_action(session.world, action);
            emit_interaction(stream, framing, state, result).await
        }
        Serverbound::PlayUseItemOn(use_item_on) => {
            let result = session
                .player
                .handle_use_item_on(session.world, use_item_on);
            emit_interaction(stream, framing, state, result).await
        }
        Serverbound::PlayUseItem(use_item) => {
            let result = session.player.handle_use_item(session.world, use_item);
            emit_use_item(stream, framing, state, &mut session.player, result).await
        }
        Serverbound::PlaySetCarriedItem(set) => {
            session.player.inventory.set_carried(set.slot);
            debug!(slot = set.slot, "case active mise à jour");
            false
        }
        Serverbound::PlaySetCreativeModeSlot(set) => {
            let mode = session.player.mode;
            session
                .player
                .inventory
                .set_creative_slot(mode, set.slot, set.stack);
            debug!(slot = set.slot, "case créative mise à jour");
            false
        }
        Serverbound::PlayPickItemFromBlock(pick) => {
            let result = session
                .player
                .handle_pick_item_from_block(session.world, pick);
            emit_pick(stream, framing, state, &mut session.player, result).await
        }
        Serverbound::PlayPlayerInput(input) => {
            session.player.sneaking = input.sneaking();
            publish_orientation(session);
            debug!(sneaking = session.player.sneaking, "entrée joueur");
            false
        }
        Serverbound::PlayPlayerCommand(command) => {
            debug!(
                entity = command.entity_id,
                action = command.action,
                "commande joueur ignorée"
            );
            false
        }
        Serverbound::PlayIgnored { id } => {
            trace_packet("paquet play ignoré", State::Play, id, 0);
            false
        }
        other => {
            debug!(packet = ?other, "paquet play inattendu ignoré");
            false
        }
    }
}

async fn emit_interaction(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    result: player::ActionResult,
) -> bool {
    match result {
        player::ActionResult::ProtocolError => {
            disconnect(stream, framing, state, REASON_INVALID_SEQUENCE).await;
            true
        }
        player::ActionResult::Ignored => false,
        player::ActionResult::Respond { updates, ack } => {
            for (pos, block_state) in updates {
                if write_packet(
                    stream,
                    framing,
                    state,
                    play_cb::BLOCK_UPDATE,
                    &block_update_payload(pos, block_state),
                )
                .await
                .is_err()
                {
                    return true;
                }
            }
            write_packet(
                stream,
                framing,
                state,
                play_cb::BLOCK_CHANGED_ACK,
                &block_changed_ack_payload(ack),
            )
            .await
            .is_err()
        }
    }
}

async fn emit_pick(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    player_state: &mut PlayerState,
    result: player::PickResult,
) -> bool {
    match result {
        player::PickResult::Ignored => false,
        player::PickResult::Respond { slots, selected } => {
            for slot in slots {
                let state_id = player_state.next_container_state_id();
                if write_packet(
                    stream,
                    framing,
                    state,
                    play_cb::CONTAINER_SET_SLOT,
                    &container_set_slot_payload(0, state_id, slot.menu_slot, slot.stack),
                )
                .await
                .is_err()
                {
                    return true;
                }
            }
            write_packet(
                stream,
                framing,
                state,
                play_cb::SET_HELD_SLOT,
                &set_held_slot_payload(selected as i32),
            )
            .await
            .is_err()
        }
    }
}

async fn emit_use_item(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    player_state: &mut PlayerState,
    result: player::UseItemResult,
) -> bool {
    match result {
        player::UseItemResult::ProtocolError => {
            disconnect(stream, framing, state, REASON_INVALID_SEQUENCE).await;
            true
        }
        player::UseItemResult::Respond(response) => {
            for (pos, block_state) in response.updates {
                if write_packet(
                    stream,
                    framing,
                    state,
                    play_cb::BLOCK_UPDATE,
                    &block_update_payload(pos, block_state),
                )
                .await
                .is_err()
                {
                    return true;
                }
            }
            for slot in response.slots {
                let state_id = player_state.next_container_state_id();
                if write_packet(
                    stream,
                    framing,
                    state,
                    play_cb::CONTAINER_SET_SLOT,
                    &container_set_slot_payload(0, state_id, slot.menu_slot, slot.stack),
                )
                .await
                .is_err()
                {
                    return true;
                }
            }
            write_packet(
                stream,
                framing,
                state,
                play_cb::BLOCK_CHANGED_ACK,
                &block_changed_ack_payload(response.ack),
            )
            .await
            .is_err()
        }
    }
}

fn on_ground_flag(flags: u8) -> bool {
    flags & 0x01 != 0
}

async fn follow_player(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    session: &mut Session<'_>,
) -> bool {
    if !session.view.chunk_changed() {
        return false;
    }
    let closed = update_chunks(stream, framing, state, &mut session.view, session.world).await;
    if !closed {
        session
            .registry
            .update_sent_chunks(session.uuid, &session.view.sent);
    }
    closed
}

async fn write_packet(
    stream: &mut TcpStream,
    framing: &Framing,
    state: State,
    id: i32,
    payload: &[u8],
) -> Result<(), WriteError> {
    trace_packet("paquet émis", state, id, payload.len());
    let framed = match framing {
        Framing::Uncompressed => build_packet(id, payload).map_err(WriteError::Encode)?,
        Framing::Compressed(threshold) => {
            build_compressed_packet(id, payload, *threshold).map_err(WriteError::Encode)?
        }
    };
    stream.write_all(&framed).await.map_err(WriteError::Io)?;
    Ok(())
}

async fn disconnect(stream: &mut TcpStream, framing: &Framing, state: State, reason: &str) {
    info!(reason, "déconnexion");
    let packet = match state {
        State::Login => Some((0x00, login_disconnect_payload(reason))),
        State::Configuration => Some((
            packet_ids::clientbound::DISCONNECT,
            config_disconnect_payload(reason),
        )),
        State::Play => Some((
            packet_ids::play::DISCONNECT,
            config_disconnect_payload(reason),
        )),
        State::Handshaking | State::Status => None,
    };

    let deadline = Instant::now() + CLOSE_BUDGET;
    let graceful = match packet {
        Some((id, payload)) => {
            let sent = timeout_at(deadline, async {
                write_packet(stream, framing, state, id, &payload).await?;
                stream.flush().await.map_err(WriteError::Io)?;
                Ok::<(), WriteError>(())
            })
            .await;
            match sent {
                Ok(Ok(())) => true,
                Ok(Err(err)) => {
                    warn!("disconnect write failed: {err}");
                    false
                }
                Err(_) => false,
            }
        }
        None => true,
    };

    if graceful {
        let _ = timeout_at(deadline, stream.shutdown()).await;
        drain(stream, deadline).await;
    }
}

async fn drain(stream: &mut TcpStream, deadline: Instant) {
    let mut scratch = [0u8; READ_CHUNK];
    let mut total = 0usize;
    while total < DRAIN_LIMIT {
        match timeout_at(deadline, stream.read(&mut scratch)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => total += n,
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Arc, PLAY_THRESHOLD, PlayerRegistry, World, broadcast_gamemode_change, broadcast_latency,
        build_env_filter, spawn_position,
    };
    use tokio::sync::mpsc::unbounded_channel;
    use verdigris_protocol::{
        decode_compressed_frame, play::packet_ids::clientbound as play_cb, read_var_int,
    };

    fn action_byte(frame: &Arc<[u8]>) -> (i32, u8) {
        let (body, _) = decode_compressed_frame(&frame[..], PLAY_THRESHOLD)
            .unwrap()
            .unwrap();
        let (id, header) = read_var_int(&body).unwrap().unwrap();
        (id, body[header])
    }

    #[test]
    fn spawn_sits_on_the_flat_ground() {
        let world = World::flat().unwrap();
        assert_eq!(spawn_position(&world), [0.5, -60.0, 0.5]);
    }

    #[test]
    fn spawn_sits_on_solid_not_on_fluid() {
        use verdigris_protocol::{
            ChunkColumn, ChunkGenerator, block_index, default_state_of_name, plains_biome_id,
        };

        struct OceanGenerator {
            stone: u16,
            water: u16,
            biome: u16,
        }

        impl ChunkGenerator for OceanGenerator {
            fn generate(&self, _chunk_x: i32, _chunk_z: i32) -> ChunkColumn {
                let mut column = ChunkColumn::empty(self.biome);
                for lz in 0..16usize {
                    for lx in 0..16usize {
                        for j in 0..=24usize {
                            column.sections[j / 16].states[block_index(lx, j % 16, lz)] =
                                self.stone;
                        }
                        for j in 25..=126usize {
                            column.sections[j / 16].states[block_index(lx, j % 16, lz)] =
                                self.water;
                        }
                    }
                }
                column
            }
        }

        let generator = OceanGenerator {
            stone: default_state_of_name("minecraft:stone").unwrap() as u16,
            water: default_state_of_name("minecraft:water").unwrap() as u16,
            biome: plains_biome_id().unwrap() as u16,
        };
        let world = World::new(Box::new(generator));
        assert_eq!(spawn_position(&world), [0.5, -39.0, 0.5]);
    }

    #[test]
    fn gamemode_change_broadcasts_only_its_action_to_all() {
        let registry = Arc::new(PlayerRegistry::new("Verdigris", 20));
        let (tx_a, mut rx_a) = unbounded_channel::<Arc<[u8]>>();
        let (tx_b, mut rx_b) = unbounded_channel::<Arc<[u8]>>();
        registry.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        registry.register(2, 11, (1.5, -60.0, 2.5), "Bob".to_owned(), 0, 0, tx_b);

        broadcast_gamemode_change(&registry, 2, 3);

        let frame_a = rx_a.try_recv().expect("Alice reçoit la diffusion");
        let frame_b = rx_b.try_recv().expect("Bob reçoit la diffusion");
        assert_eq!(action_byte(&frame_a), (play_cb::PLAYER_INFO_UPDATE, 0x04));
        assert_eq!(action_byte(&frame_b), (play_cb::PLAYER_INFO_UPDATE, 0x04));
    }

    #[test]
    fn latency_broadcast_uses_the_latency_action() {
        let registry = Arc::new(PlayerRegistry::new("Verdigris", 20));
        let (tx_a, mut rx_a) = unbounded_channel::<Arc<[u8]>>();
        registry.register(1, 10, (0.5, -60.0, 0.5), "Alice".to_owned(), 1, 0, tx_a);
        registry.set_latency(1, 42);

        broadcast_latency(&registry);

        let frame = rx_a.try_recv().expect("latence diffusée au joueur");
        assert_eq!(action_byte(&frame), (play_cb::PLAYER_INFO_UPDATE, 0x10));
    }

    #[test]
    fn invalid_filter_falls_back_to_info() {
        let invalid = build_env_filter(Some("!!!nope"));
        let default = build_env_filter(None);
        assert_eq!(invalid.to_string(), default.to_string());
    }

    #[test]
    fn valid_filter_is_preserved() {
        assert_eq!(build_env_filter(Some("debug")).to_string(), "debug");
    }
}
