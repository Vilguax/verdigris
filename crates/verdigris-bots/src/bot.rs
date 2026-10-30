use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

use crate::codec;
use crate::conn::{Conn, RecvError, connect};
use crate::metrics::Metrics;

pub const HANDSHAKE_LOGIN_TIMEOUT: Duration = Duration::from_secs(15);
pub const CONFIG_TIMEOUT: Duration = Duration::from_secs(15);
pub const PLAY_FIRST_POSITION_TIMEOUT: Duration = Duration::from_secs(20);
pub const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
pub const TICK_PERIOD: Duration = Duration::from_millis(50);

pub const WALK_STEP_PER_TICK: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCause {
    Online,
    Timeout,
    Disconnected,
    ConnectionReset,
    Cancelled,
    Panicked,
}

#[derive(Debug, Clone)]
pub struct BotOutcome {
    pub reached_login_finished: Option<Duration>,
    pub reached_play: Option<Duration>,
    pub first_chunk: Option<Duration>,
    pub chunks: u64,
    pub bytes: u64,
    pub failure: Option<FailureCause>,
    pub disconnect_reason: Option<String>,
}

impl BotOutcome {
    fn empty() -> BotOutcome {
        BotOutcome {
            reached_login_finished: None,
            reached_play: None,
            first_chunk: None,
            chunks: 0,
            bytes: 0,
            failure: None,
            disconnect_reason: None,
        }
    }

    pub fn panicked() -> BotOutcome {
        let mut outcome = BotOutcome::empty();
        outcome.failure = Some(FailureCause::Panicked);
        outcome
    }

    pub fn is_in_play(&self) -> bool {
        self.failure.is_none() && self.reached_play.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct BotConfig {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub view_distance: u8,
    pub walk: f64,
}

struct Rng(u64);

impl Rng {
    fn seeded(index: usize) -> Rng {
        let state = RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_usize(index);
        hasher.write_u8(0x5B);
        Rng(hasher.finish() | 1)
    }

    fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        ((x >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

pub async fn run_bot(
    cfg: BotConfig,
    index: usize,
    token: CancellationToken,
    metrics: Arc<Metrics>,
) -> BotOutcome {
    let mut conn = match connect(&cfg.host, cfg.port, &token).await {
        Ok(conn) => conn,
        Err(err) => {
            let cause = connect_cause(err);
            let mut outcome = BotOutcome::empty();
            outcome.failure = Some(cause);
            if cause == FailureCause::Timeout || cause == FailureCause::ConnectionReset {
                metrics.record_failure();
            }
            return outcome;
        }
    };
    let start = Instant::now();
    let outcome = run_session(&mut conn, &cfg, index, &token, start, &metrics).await;
    if let Some(cause) = outcome.failure
        && (cause != FailureCause::Cancelled || outcome.reached_play.is_none())
    {
        metrics.record_failure();
    }
    outcome
}

fn connect_cause(err: RecvError) -> FailureCause {
    match err {
        RecvError::Cancelled => FailureCause::Cancelled,
        RecvError::Timeout => FailureCause::Timeout,
        RecvError::Reset => FailureCause::ConnectionReset,
    }
}

fn recv_cause(err: RecvError, in_play: bool) -> Option<FailureCause> {
    match err {
        RecvError::Cancelled => {
            if in_play {
                None
            } else {
                Some(FailureCause::Cancelled)
            }
        }
        RecvError::Timeout => Some(FailureCause::Timeout),
        RecvError::Reset => Some(FailureCause::ConnectionReset),
    }
}

fn sync_bytes<S>(conn: &Conn<S>, last: &mut u64, metrics: &Metrics)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let total = conn.bytes_read();
    if total > *last {
        metrics.add_bytes(total - *last);
        *last = total;
    }
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

pub async fn run_session<S>(
    conn: &mut Conn<S>,
    cfg: &BotConfig,
    index: usize,
    token: &CancellationToken,
    start: Instant,
    metrics: &Arc<Metrics>,
) -> BotOutcome
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut outcome = BotOutcome::empty();
    let mut last_bytes = 0u64;

    if let Err(err) = conn
        .send(
            codec::handshake::HANDSHAKE,
            &codec::handshake_payload(&cfg.host, cfg.port, codec::INTENT_LOGIN),
            token,
        )
        .await
    {
        outcome.failure = recv_cause(err, false);
        outcome.bytes = conn.bytes_read();
        return outcome;
    }

    let uuid = codec::offline_uuid_bytes(&cfg.name);
    if let Err(err) = conn
        .send(
            codec::login_sb::HELLO,
            &codec::login_hello_payload(&cfg.name, uuid),
            token,
        )
        .await
    {
        outcome.failure = recv_cause(err, false);
        outcome.bytes = conn.bytes_read();
        return outcome;
    }

    let login_deadline = Instant::now() + HANDSHAKE_LOGIN_TIMEOUT;
    loop {
        let (id, body) = match conn.recv(remaining(login_deadline), token).await {
            Ok(frame) => frame,
            Err(err) => {
                outcome.failure = recv_cause(err, false);
                outcome.bytes = conn.bytes_read();
                return outcome;
            }
        };
        sync_bytes(conn, &mut last_bytes, metrics);
        if id == codec::login_cb::SET_COMPRESSION {
            if let Some(threshold) = codec::decode_set_compression(&body)
                && threshold >= 0
            {
                conn.enable_compression(threshold as usize);
            }
        } else if id == codec::login_cb::HELLO {
            outcome.failure = Some(FailureCause::Online);
            outcome.bytes = conn.bytes_read();
            return outcome;
        } else if id == codec::login_cb::FINISHED {
            outcome.reached_login_finished = Some(start.elapsed());
            if let Err(err) = conn
                .send(
                    codec::login_sb::ACKNOWLEDGED,
                    &codec::login_acknowledged_payload(),
                    token,
                )
                .await
            {
                outcome.failure = recv_cause(err, false);
                outcome.bytes = conn.bytes_read();
                return outcome;
            }
            break;
        } else if id == codec::login_cb::DISCONNECT {
            outcome.failure = Some(FailureCause::Disconnected);
            outcome.disconnect_reason = codec::parse_disconnect_reason(&body);
            outcome.bytes = conn.bytes_read();
            return outcome;
        }
    }

    if let Err(err) = conn
        .send(
            codec::config_sb::CLIENT_INFORMATION,
            &codec::client_information_payload(cfg.view_distance),
            token,
        )
        .await
    {
        outcome.failure = recv_cause(err, false);
        outcome.bytes = conn.bytes_read();
        return outcome;
    }

    let config_deadline = Instant::now() + CONFIG_TIMEOUT;
    loop {
        let (id, body) = match conn.recv(remaining(config_deadline), token).await {
            Ok(frame) => frame,
            Err(err) => {
                outcome.failure = recv_cause(err, false);
                outcome.bytes = conn.bytes_read();
                return outcome;
            }
        };
        sync_bytes(conn, &mut last_bytes, metrics);
        if id == codec::config_cb::SELECT_KNOWN_PACKS {
            if let Err(err) = conn
                .send(
                    codec::config_sb::SELECT_KNOWN_PACKS,
                    &codec::select_known_packs_reply(),
                    token,
                )
                .await
            {
                outcome.failure = recv_cause(err, false);
                outcome.bytes = conn.bytes_read();
                return outcome;
            }
        } else if id == codec::config_cb::KEEP_ALIVE {
            if let Some(ka) = codec::decode_keep_alive(&body)
                && let Err(err) = conn
                    .send(
                        codec::config_sb::KEEP_ALIVE,
                        &codec::keep_alive_payload(ka),
                        token,
                    )
                    .await
            {
                outcome.failure = recv_cause(err, false);
                outcome.bytes = conn.bytes_read();
                return outcome;
            }
        } else if id == codec::config_cb::PING {
            if let Some(ping) = codec::decode_ping(&body)
                && let Err(err) = conn
                    .send(
                        codec::config_sb::PONG,
                        &codec::pong_config_payload(ping),
                        token,
                    )
                    .await
            {
                outcome.failure = recv_cause(err, false);
                outcome.bytes = conn.bytes_read();
                return outcome;
            }
        } else if id == codec::config_cb::FINISH_CONFIGURATION {
            if let Err(err) = conn
                .send(
                    codec::config_sb::FINISH_CONFIGURATION,
                    &codec::finish_configuration_payload(),
                    token,
                )
                .await
            {
                outcome.failure = recv_cause(err, false);
                outcome.bytes = conn.bytes_read();
                return outcome;
            }
            break;
        } else if id == codec::config_cb::DISCONNECT {
            outcome.failure = Some(FailureCause::Disconnected);
            outcome.disconnect_reason = codec::parse_disconnect_reason(&body);
            outcome.bytes = conn.bytes_read();
            return outcome;
        }
    }

    outcome.reached_play = Some(start.elapsed());
    metrics.enter_play();
    let play = play_loop(conn, cfg, index, token, start, metrics, &mut last_bytes).await;
    metrics.leave_play();

    outcome.chunks = play.chunks;
    outcome.first_chunk = play.first_chunk;
    outcome.failure = play.failure;
    if outcome.disconnect_reason.is_none() {
        outcome.disconnect_reason = play.disconnect_reason;
    }
    outcome.bytes = conn.bytes_read();
    outcome
}

struct PlayResult {
    failure: Option<FailureCause>,
    chunks: u64,
    first_chunk: Option<Duration>,
    disconnect_reason: Option<String>,
}

#[allow(clippy::too_many_arguments)]
async fn play_loop<S>(
    conn: &mut Conn<S>,
    cfg: &BotConfig,
    index: usize,
    token: &CancellationToken,
    start: Instant,
    metrics: &Arc<Metrics>,
    last_bytes: &mut u64,
) -> PlayResult
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut result = PlayResult {
        failure: None,
        chunks: 0,
        first_chunk: None,
        disconnect_reason: None,
    };
    let mut pos: Option<[f64; 3]> = None;
    let mut player_loaded_sent = false;
    let mut rng = Rng::seeded(index);

    let first_position_deadline = Instant::now() + PLAY_FIRST_POSITION_TIMEOUT;

    while pos.is_none() {
        let (id, body) = match conn.recv(remaining(first_position_deadline), token).await {
            Ok(frame) => frame,
            Err(err) => {
                result.failure = recv_cause(err, true);
                return result;
            }
        };
        sync_bytes(conn, last_bytes, metrics);
        match handle_play_packet(conn, id, &body, start, metrics, &mut result, token).await {
            PacketAction::Continue => {}
            PacketAction::Position(x, y, z) => {
                pos = Some([x, y, z]);
                if !player_loaded_sent {
                    if let Err(err) = conn
                        .send(
                            codec::play_sb::PLAYER_LOADED,
                            &codec::player_loaded_payload(),
                            token,
                        )
                        .await
                    {
                        result.failure = recv_cause(err, true);
                        return result;
                    }
                    player_loaded_sent = true;
                }
            }
            PacketAction::Disconnected(reason) => {
                result.failure = Some(FailureCause::Disconnected);
                result.disconnect_reason = reason;
                return result;
            }
            PacketAction::Failed(cause) => {
                result.failure = Some(cause);
                return result;
            }
        }
    }

    let mut current = match pos {
        Some(value) => value,
        None => return result,
    };
    let mut last_activity = Instant::now();
    let mut next_tick = Instant::now() + TICK_PERIOD;

    loop {
        tokio::task::yield_now().await;
        if token.is_cancelled() {
            result.failure = None;
            return result;
        }

        let now = Instant::now();
        if now >= next_tick {
            if cfg.walk > 0.0 {
                let angle = rng.next_f64() * std::f64::consts::TAU;
                let step = cfg.walk * WALK_STEP_PER_TICK;
                current[0] += angle.cos() * step;
                current[2] += angle.sin() * step;
                if let Err(err) = conn
                    .send(
                        codec::play_sb::MOVE_PLAYER_POS,
                        &codec::move_player_pos_payload(
                            current[0],
                            current[1],
                            current[2],
                            codec::MOVE_FLAG_ON_GROUND,
                        ),
                        token,
                    )
                    .await
                {
                    result.failure = recv_cause(err, true);
                    return result;
                }
            }
            if let Err(err) = conn
                .send(
                    codec::play_sb::CLIENT_TICK_END,
                    &codec::client_tick_end_payload(),
                    token,
                )
                .await
            {
                result.failure = recv_cause(err, true);
                return result;
            }
            next_tick += TICK_PERIOD;
            if next_tick <= Instant::now() {
                next_tick = Instant::now() + TICK_PERIOD;
            }
            continue;
        }

        let wait = next_tick.saturating_duration_since(now);
        match conn.recv(wait, token).await {
            Ok((id, body)) => {
                last_activity = Instant::now();
                sync_bytes(conn, last_bytes, metrics);
                match handle_play_packet(conn, id, &body, start, metrics, &mut result, token).await
                {
                    PacketAction::Continue => {}
                    PacketAction::Position(x, y, z) => {
                        current = [x, y, z];
                    }
                    PacketAction::Disconnected(reason) => {
                        result.failure = Some(FailureCause::Disconnected);
                        result.disconnect_reason = reason;
                        return result;
                    }
                    PacketAction::Failed(cause) => {
                        result.failure = Some(cause);
                        return result;
                    }
                }
            }
            Err(RecvError::Cancelled) => {
                result.failure = None;
                return result;
            }
            Err(RecvError::Reset) => {
                result.failure = Some(FailureCause::ConnectionReset);
                return result;
            }
            Err(RecvError::Timeout) => {
                if Instant::now().duration_since(last_activity) >= READ_IDLE_TIMEOUT {
                    result.failure = Some(FailureCause::Timeout);
                    return result;
                }
            }
        }
    }
}

enum PacketAction {
    Continue,
    Position(f64, f64, f64),
    Disconnected(Option<String>),
    Failed(FailureCause),
}

async fn handle_play_packet<S>(
    conn: &mut Conn<S>,
    id: i32,
    body: &[u8],
    start: Instant,
    metrics: &Arc<Metrics>,
    result: &mut PlayResult,
    token: &CancellationToken,
) -> PacketAction
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if id == codec::play_cb::KEEP_ALIVE {
        if let Some(ka) = codec::decode_keep_alive(body)
            && conn
                .send(
                    codec::play_sb::KEEP_ALIVE,
                    &codec::keep_alive_payload(ka),
                    token,
                )
                .await
                .is_err()
        {
            return PacketAction::Failed(FailureCause::ConnectionReset);
        }
        PacketAction::Continue
    } else if id == codec::play_cb::PING {
        if let Some(ping) = codec::decode_ping(body)
            && conn
                .send(codec::play_sb::PONG, &codec::pong_play_payload(ping), token)
                .await
                .is_err()
        {
            return PacketAction::Failed(FailureCause::ConnectionReset);
        }
        PacketAction::Continue
    } else if id == codec::play_cb::PLAYER_POSITION {
        match codec::decode_player_position(body) {
            Some(position) => {
                if conn
                    .send(
                        codec::play_sb::ACCEPT_TELEPORTATION,
                        &codec::accept_teleportation_payload(&position),
                        token,
                    )
                    .await
                    .is_err()
                {
                    return PacketAction::Failed(FailureCause::ConnectionReset);
                }
                PacketAction::Position(position.x, position.y, position.z)
            }
            None => PacketAction::Continue,
        }
    } else if id == codec::play_cb::CHUNK_BATCH_FINISHED {
        if conn
            .send(
                codec::play_sb::CHUNK_BATCH_RECEIVED,
                &codec::chunk_batch_received_payload(codec::CHUNK_BATCH_DESIRED_PER_TICK),
                token,
            )
            .await
            .is_err()
        {
            return PacketAction::Failed(FailureCause::ConnectionReset);
        }
        PacketAction::Continue
    } else if id == codec::play_cb::LEVEL_CHUNK_WITH_LIGHT {
        result.chunks += 1;
        metrics.add_chunks(1);
        if result.first_chunk.is_none() {
            result.first_chunk = Some(start.elapsed());
        }
        PacketAction::Continue
    } else if id == codec::play_cb::DISCONNECT {
        PacketAction::Disconnected(codec::parse_disconnect_reason(body))
    } else {
        PacketAction::Continue
    }
}
