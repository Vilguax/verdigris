use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, timeout};

use verdigris_protocol::{
    Intent, Serverbound, State, StatusInfo, decode_frame, decode_serverbound, encode_pong,
    encode_status_response, status_json,
};

pub const DEFAULT_BIND: &str = "0.0.0.0:25565";
pub const DEFAULT_MOTD: &str = "Verdigris";
pub const DEFAULT_MAX_PLAYERS: i32 = 20;

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const READ_CHUNK: usize = 4096;

enum Flow {
    Continue,
    Close,
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

async fn handle_connection(mut stream: TcpStream, peer: SocketAddr) {
    let mut buf: Vec<u8> = Vec::new();
    let mut state = State::Handshaking;
    let mut chunk = [0u8; READ_CHUNK];

    loop {
        loop {
            let (body, consumed) = match decode_frame(&buf) {
                Ok(Some((body, consumed))) => (body.to_vec(), consumed),
                Ok(None) => break,
                Err(err) => {
                    eprintln!("[debug] {peer}: closing on frame error: {err}");
                    return;
                }
            };
            buf.drain(..consumed);
            match dispatch(&mut stream, &mut state, &body, peer).await {
                Flow::Continue => {}
                Flow::Close => return,
            }
        }

        let read = timeout(READ_TIMEOUT, stream.read(&mut chunk)).await;
        let n = match read {
            Ok(Ok(0)) => return,
            Ok(Ok(n)) => n,
            Ok(Err(err)) => {
                eprintln!("[debug] {peer}: read error: {err}");
                return;
            }
            Err(_) => {
                eprintln!("[debug] {peer}: read timeout");
                return;
            }
        };
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn dispatch(
    stream: &mut TcpStream,
    state: &mut State,
    body: &[u8],
    peer: SocketAddr,
) -> Flow {
    let packet = match decode_serverbound(*state, body) {
        Ok(packet) => packet,
        Err(err) => {
            eprintln!("[debug] {peer}: closing on packet error: {err}");
            return Flow::Close;
        }
    };

    match packet {
        Serverbound::Handshake(handshake) => {
            eprintln!(
                "[debug] {peer}: handshake protocol={} intent={:?}",
                handshake.protocol_version, handshake.intent
            );
            match handshake.intent {
                Intent::Status => {
                    *state = State::Status;
                    Flow::Continue
                }
                Intent::Login | Intent::Transfer | Intent::Unknown(_) => {
                    eprintln!(
                        "[debug] {peer}: intent {:?} out of scope, closing",
                        handshake.intent
                    );
                    Flow::Close
                }
            }
        }
        Serverbound::StatusRequest => {
            let info = StatusInfo {
                motd: DEFAULT_MOTD,
                max_players: DEFAULT_MAX_PLAYERS,
                ..StatusInfo::default()
            };
            let json = status_json(&info);
            match encode_status_response(&json) {
                Ok(bytes) => {
                    if stream.write_all(&bytes).await.is_err() {
                        return Flow::Close;
                    }
                    Flow::Continue
                }
                Err(err) => {
                    eprintln!("[debug] {peer}: status encode error: {err}");
                    Flow::Close
                }
            }
        }
        Serverbound::PingRequest { payload } => {
            let bytes = encode_pong(payload);
            let _ = stream.write_all(&bytes).await;
            Flow::Close
        }
    }
}
