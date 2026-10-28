use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, Instant, timeout, timeout_at};

use verdigris_protocol::{
    EncodeError, Intent, PROTOCOL_VERSION, Serverbound, State, StatusInfo, build_packet,
    config_disconnect_payload, decode_frame, decode_serverbound, login_disconnect_payload,
    pong_payload, status_json, status_response_payload,
};

pub const DEFAULT_BIND: &str = "0.0.0.0:25565";
pub const DEFAULT_MOTD: &str = "Verdigris";
pub const DEFAULT_MAX_PLAYERS: i32 = 20;

pub const REASON_VERSION_INCOMPATIBLE: &str =
    "Version incompatible: ce serveur attend Minecraft 26.3 (protocole 777)";
pub const REASON_LOGIN_UNAVAILABLE: &str = "La connexion des joueurs n'est pas encore disponible";
pub const REASON_TRANSFER_UNSUPPORTED: &str = "Transfert non supporté";
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

pub async fn serve(bind: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    run(listener).await
}

pub async fn run(listener: TcpListener) -> std::io::Result<()> {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(async move {
                    handle_connection(stream, peer).await;
                });
            }
            Err(err) => {
                eprintln!("[debug] accept error: {err}");
            }
        }
    }
}

async fn handle_connection(mut stream: TcpStream, _peer: SocketAddr) {
    let mut buf: Vec<u8> = Vec::new();
    let mut state = State::Handshaking;
    let mut chunk = [0u8; READ_CHUNK];

    loop {
        loop {
            let (body, consumed) = match decode_frame(&buf) {
                Ok(Some((body, consumed))) => (body.to_vec(), consumed),
                Ok(None) => break,
                Err(err) => {
                    let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                    disconnect(&mut stream, state, &reason).await;
                    return;
                }
            };
            buf.drain(..consumed);
            match dispatch(&mut stream, &mut state, &body).await {
                Next::Continue => {}
                Next::Terminate(reason) => {
                    disconnect(&mut stream, state, &reason).await;
                    return;
                }
            }
        }

        let read = timeout(READ_TIMEOUT, stream.read(&mut chunk)).await;
        let n = match read {
            Ok(Ok(0)) => {
                disconnect(&mut stream, state, REASON_CONNECTION_LOST).await;
                return;
            }
            Ok(Ok(n)) => n,
            Ok(Err(err)) => {
                let reason = format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}");
                disconnect(&mut stream, state, &reason).await;
                return;
            }
            Err(_) => {
                disconnect(&mut stream, state, REASON_READ_TIMEOUT).await;
                return;
            }
        };
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn dispatch(stream: &mut TcpStream, state: &mut State, body: &[u8]) -> Next {
    let packet = match decode_serverbound(*state, body) {
        Ok(packet) => packet,
        Err(err) => return Next::Terminate(format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}")),
    };

    match packet {
        Serverbound::Handshake(handshake) => match handshake.intent {
            Intent::Status => {
                *state = State::Status;
                Next::Continue
            }
            Intent::Login => {
                *state = State::Login;
                if handshake.protocol_version != PROTOCOL_VERSION {
                    Next::Terminate(REASON_VERSION_INCOMPATIBLE.to_owned())
                } else {
                    Next::Continue
                }
            }
            Intent::Transfer => {
                *state = State::Login;
                Next::Terminate(REASON_TRANSFER_UNSUPPORTED.to_owned())
            }
            Intent::Unknown(_) => Next::Terminate(String::new()),
        },
        Serverbound::StatusRequest => {
            let info = StatusInfo {
                motd: DEFAULT_MOTD,
                max_players: DEFAULT_MAX_PLAYERS,
                ..StatusInfo::default()
            };
            let json = status_json(&info);
            match status_response_payload(&json) {
                Ok(payload) => match write_packet(stream, 0x00, &payload).await {
                    Ok(()) => Next::Continue,
                    Err(_) => Next::Terminate(String::new()),
                },
                Err(err) => Next::Terminate(format!("{REASON_PROTOCOL_ERROR_PREFIX}{err}")),
            }
        }
        Serverbound::PingRequest { payload } => {
            let _ = write_packet(stream, 0x01, &pong_payload(payload)).await;
            Next::Terminate(String::new())
        }
        Serverbound::LoginStart { .. } => Next::Terminate(REASON_LOGIN_UNAVAILABLE.to_owned()),
    }
}

async fn write_packet(stream: &mut TcpStream, id: i32, payload: &[u8]) -> Result<(), WriteError> {
    let framed = build_packet(id, payload).map_err(WriteError::Encode)?;
    stream.write_all(&framed).await.map_err(WriteError::Io)?;
    Ok(())
}

async fn disconnect(stream: &mut TcpStream, state: State, reason: &str) {
    let packet = match state {
        State::Login => Some((0x00, login_disconnect_payload(reason))),
        State::Configuration => Some((0x02, config_disconnect_payload(reason))),
        State::Handshaking | State::Status => None,
    };

    let deadline = Instant::now() + CLOSE_BUDGET;
    let graceful = match packet {
        Some((id, payload)) => {
            let sent = timeout_at(deadline, async {
                write_packet(stream, id, &payload).await?;
                stream.flush().await.map_err(WriteError::Io)?;
                Ok::<(), WriteError>(())
            })
            .await;
            match sent {
                Ok(Ok(())) => true,
                Ok(Err(err)) => {
                    eprintln!("[debug] disconnect write failed: {err}");
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
