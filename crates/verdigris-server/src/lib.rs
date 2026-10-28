use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, Instant, timeout, timeout_at};
use tracing::{Instrument, debug, error, info, info_span, warn};
use tracing_subscriber::EnvFilter;

use verdigris_protocol::{
    COMPRESSION_THRESHOLD, EncodeError, Intent, PROTOCOL_VERSION, SET_COMPRESSION_ID, Serverbound,
    State, StatusInfo, build_compressed_packet, build_packet, config_disconnect_payload,
    decode_compressed_frame, decode_frame, decode_serverbound, login_disconnect_payload,
    login_success_payload, offline_uuid, pong_payload, random_session_id, read_var_int,
    set_compression_payload, status_json, status_response_payload, validate_name,
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
pub const REASON_CONFIG_UNAVAILABLE: &str =
    "la configuration du serveur n'est pas encore disponible";
const LOGIN_SUCCESS_ID: i32 = 0x02;
pub const REASON_PROTOCOL_ERROR_PREFIX: &str = "Paquet invalide: ";
pub const REASON_CONNECTION_LOST: &str = "Connexion interrompue";
pub const REASON_READ_TIMEOUT: &str = "Délai de lecture dépassé";

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const READ_CHUNK: usize = 4096;
const CLOSE_BUDGET: Duration = Duration::from_secs(1);
const DRAIN_LIMIT: usize = 64 * 1024;

enum Next {
    Continue,
    Terminate(String),
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
    let counter = AtomicU64::new(0);
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let conn_id = counter.fetch_add(1, Ordering::Relaxed);
                let span = info_span!("conn", conn_id, peer = %peer);
                tokio::spawn(
                    async move {
                        handle_connection(stream).await;
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

async fn handle_connection(stream: TcpStream) {
    let started = Instant::now();
    info!("connexion acceptée");
    serve_connection(stream).await;
    info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        "fin de connexion"
    );
}

async fn serve_connection(mut stream: TcpStream) {
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
        return finish_login(state, body);
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

fn finish_login(state: &mut State, body: &[u8]) -> Next {
    match decode_serverbound(State::Login, body) {
        Ok(Serverbound::LoginAcknowledged) => {
            set_state(state, State::Configuration);
            Next::Terminate(REASON_CONFIG_UNAVAILABLE.to_owned())
        }
        _ => Next::Terminate(REASON_LOGIN_UNEXPECTED_PACKET.to_owned()),
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
        State::Configuration => Some((0x02, config_disconnect_payload(reason))),
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
