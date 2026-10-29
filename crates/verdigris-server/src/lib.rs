use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, Instant, MissedTickBehavior, interval, timeout, timeout_at};
use tracing::{Instrument, debug, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

use verdigris_protocol::play::packet_ids::clientbound as play_cb;
use verdigris_protocol::{
    COMPRESSION_THRESHOLD, EncodeError, GAME_EVENT_WAIT_FOR_CHUNKS, Intent, PROTOCOL_VERSION,
    SET_COMPRESSION_ID, Serverbound, State, StatusInfo, build_compressed_packet, build_packet,
    chunk_batch_finished_payload, chunk_batch_start_payload, client_has_core,
    config_disconnect_payload, decode_compressed_frame, decode_frame, decode_known_packs,
    decode_serverbound, game_event_payload, keep_alive_payload, known_packs_payload,
    level_chunk_with_light_payload, login_disconnect_payload, login_play_payload,
    login_success_payload, network_data, offline_uuid, packet_ids, player_position_payload,
    pong_payload, random_session_id, read_var_int, registry_data_payload, server_known_pack,
    set_chunk_cache_center_payload, set_compression_payload, status_json, status_response_payload,
    update_tags_payload, validate_name,
};

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

const PLAY_PLAYER_ID: i32 = 1;
const PLAY_GAME_TYPE_SPECTATOR: i32 = 3;
const PLAY_VIEW_DISTANCE: i32 = 4;
const PLAY_MAX_PLAYERS: i32 = 20;
const PLAY_TELEPORT_ID: i32 = 1;
const PLAY_SPAWN: [f64; 3] = [0.5, 100.0, 0.5];
const PLAY_CHUNK_REACH: i32 = 4;
pub const REASON_PROTOCOL_ERROR_PREFIX: &str = "Paquet invalide: ";
pub const REASON_CONNECTION_LOST: &str = "Connexion interrompue";
pub const REASON_READ_TIMEOUT: &str = "Délai de lecture dépassé";
pub const REASON_KEEP_ALIVE_TIMEOUT: &str = "keep alive délai dépassé";

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const CONFIG_ACK_TIMEOUT: Duration = Duration::from_secs(30);
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
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(15),
            check_period: Duration::from_secs(1),
        }
    }
}

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

fn ack_keep_alive(state: &mut KeepAliveState, id: i64, now: Instant) {
    match &state.pending {
        Some(pending) if pending.id == id => {
            let elapsed = now.duration_since(pending.sent_at);
            debug!(
                id,
                response_ms = elapsed.as_millis() as u64,
                "keep alive confirmé"
            );
            state.pending = None;
        }
        _ => {
            warn!(id, "keep alive inattendu ignoré");
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
    run(listener).await
}

pub async fn run(listener: TcpListener) -> std::io::Result<()> {
    run_with_config(listener, KeepAliveConfig::default()).await
}

pub async fn run_with_config(
    listener: TcpListener,
    keep_alive: KeepAliveConfig,
) -> std::io::Result<()> {
    let counter = AtomicU64::new(0);
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let conn_id = counter.fetch_add(1, Ordering::Relaxed);
                let span = info_span!("conn", conn_id, peer = %peer);
                tokio::spawn(
                    async move {
                        handle_connection(stream, keep_alive).await;
                    }
                    .instrument(span),
                );
            }
            Err(err) => {
                error!("accept error: {err}");
            }
        }
    }
}

async fn handle_connection(stream: TcpStream, keep_alive: KeepAliveConfig) {
    let started = Instant::now();
    info!("connexion acceptée");
    serve_connection(stream, keep_alive).await;
    info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        "fin de connexion"
    );
}

async fn serve_connection(mut stream: TcpStream, keep_alive: KeepAliveConfig) {
    let mut buf: Vec<u8> = Vec::new();
    let mut state = State::Handshaking;
    let mut framing = Framing::Uncompressed;
    let mut awaiting_ack = false;
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
                &body,
            )
            .await
            {
                Next::Continue => {}
                Next::EnterConfiguration => {
                    run_configuration(
                        &mut stream,
                        &framing,
                        &mut state,
                        &mut buf,
                        &mut chunk,
                        keep_alive,
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
            begin_login(stream, framing, *state, awaiting_ack, &name).await
        }
        Serverbound::LoginAcknowledged => {
            Next::Terminate(REASON_LOGIN_UNEXPECTED_PACKET.to_owned())
        }
        Serverbound::PlayAcceptTeleportation(_)
        | Serverbound::PlayerLoaded
        | Serverbound::PlayChunkBatchReceived(_)
        | Serverbound::PlayKeepAlive(_)
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

async fn run_configuration(
    stream: &mut TcpStream,
    framing: &Framing,
    state: &mut State,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    keep_alive: KeepAliveConfig,
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
    enter_play(stream, framing, state, buf, chunk, keep_alive).await;
}

async fn enter_play(
    stream: &mut TcpStream,
    framing: &Framing,
    state: &mut State,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    keep_alive: KeepAliveConfig,
) {
    let login = match login_play_payload(
        PLAY_PLAYER_ID,
        PLAY_VIEW_DISTANCE,
        PLAY_VIEW_DISTANCE,
        PLAY_GAME_TYPE_SPECTATOR,
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

    let position = player_position_payload(PLAY_TELEPORT_ID, PLAY_SPAWN, [0.0; 3], 0.0, 0.0, 0);
    if write_packet(stream, framing, *state, play_cb::PLAYER_POSITION, &position)
        .await
        .is_err()
    {
        return;
    }
    info!(
        x = PLAY_SPAWN[0],
        y = PLAY_SPAWN[1],
        z = PLAY_SPAWN[2],
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

    let center = set_chunk_cache_center_payload(0, 0);
    if write_packet(
        stream,
        framing,
        *state,
        play_cb::SET_CHUNK_CACHE_CENTER,
        &center,
    )
    .await
    .is_err()
    {
        return;
    }

    if write_packet(
        stream,
        framing,
        *state,
        play_cb::CHUNK_BATCH_START,
        &chunk_batch_start_payload(),
    )
    .await
    .is_err()
    {
        return;
    }

    let mut sent: i32 = 0;
    for x in -PLAY_CHUNK_REACH..=PLAY_CHUNK_REACH {
        for z in -PLAY_CHUNK_REACH..=PLAY_CHUNK_REACH {
            let payload = match level_chunk_with_light_payload(x, z) {
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
                play_cb::LEVEL_CHUNK_WITH_LIGHT,
                &payload,
            )
            .await
            .is_err()
            {
                return;
            }
            sent += 1;
        }
    }

    if write_packet(
        stream,
        framing,
        *state,
        play_cb::CHUNK_BATCH_FINISHED,
        &chunk_batch_finished_payload(sent),
    )
    .await
    .is_err()
    {
        return;
    }
    info!(chunks = sent, "monde vide envoyé");

    run_play(stream, framing, state, buf, chunk, keep_alive).await;
}

enum PlayStep {
    Timer,
    Read(std::io::Result<usize>),
}

async fn run_play(
    stream: &mut TcpStream,
    framing: &Framing,
    state: &mut State,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    keep_alive: KeepAliveConfig,
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
                        Ok(packet) => handle_play_packet(packet, &mut ka, Instant::now()),
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
            read = stream.read(chunk) => PlayStep::Read(read),
        };

        match step {
            PlayStep::Timer => {
                if run_keep_alive_action(stream, framing, *state, &mut ka, &keep_alive).await {
                    return;
                }
            }
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

fn handle_play_packet(packet: Serverbound, ka: &mut KeepAliveState, now: Instant) {
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
        }
        Serverbound::PlayerLoaded => info!("joueur chargé"),
        Serverbound::PlayChunkBatchReceived(received) => {
            info!(
                chunks_par_tick = received.desired_chunks_per_tick as f64,
                "lot de chunks reçu"
            );
        }
        Serverbound::PlayKeepAlive(keep_alive) => {
            ack_keep_alive(ka, keep_alive.id, now);
        }
        Serverbound::PlayIgnored { id } => {
            trace_packet("paquet play ignoré", State::Play, id, 0);
        }
        other => {
            debug!(packet = ?other, "paquet play inattendu ignoré");
        }
    }
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
    use super::build_env_filter;

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
