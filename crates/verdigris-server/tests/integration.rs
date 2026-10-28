use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, timeout};

use verdigris_protocol::{
    MAX_STRING_LEN, decode_frame, encode_frame, read_i64, read_string, read_var_int, write_i64,
    write_string, write_u16, write_var_int,
};
use verdigris_server::run;

async fn spawn_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = run(listener).await;
    });
    addr
}

fn handshake_frame(intent: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x00, &mut body);
    write_var_int(777, &mut body);
    write_string("127.0.0.1", &mut body);
    write_u16(25565, &mut body);
    write_var_int(intent, &mut body);
    encode_frame(&body)
}

fn status_request_frame() -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x00, &mut body);
    encode_frame(&body)
}

fn ping_frame(payload: i64) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x01, &mut body);
    write_i64(payload, &mut body);
    encode_frame(&body)
}

async fn read_one_frame(stream: &mut TcpStream) -> (i32, Vec<u8>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some((body, _consumed)) = decode_frame(&buf).unwrap() {
            let (id, header) = read_var_int(body).unwrap().unwrap();
            return (id, body[header..].to_vec());
        }
        let n = timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .expect("read did not time out")
            .expect("read ok");
        assert!(n > 0, "server closed before sending a full frame");
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[tokio::test]
async fn status_and_ping_round_trip() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    client.write_all(&handshake_frame(1)).await.unwrap();
    client.write_all(&status_request_frame()).await.unwrap();

    let (status_id, status_body) = read_one_frame(&mut client).await;
    assert_eq!(status_id, 0x00);
    let (json, _) = read_string(&status_body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.contains("\"name\":\"26.3\""));
    assert!(json.contains("\"protocol\":777"));
    assert!(json.contains("\"players\""));
    assert!(json.contains("\"description\""));

    let payload: i64 = 0x0123456789ABCDEF;
    client.write_all(&ping_frame(payload)).await.unwrap();
    let (pong_id, pong_body) = read_one_frame(&mut client).await;
    assert_eq!(pong_id, 0x01);
    let (echoed, _) = read_i64(&pong_body).unwrap().unwrap();
    assert_eq!(echoed, payload);
}

#[tokio::test]
async fn partial_handshake_is_buffered() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let frame = handshake_frame(1);
    let (first, second) = frame.split_at(3);
    client.write_all(first).await.unwrap();
    client.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    client.write_all(second).await.unwrap();
    client.write_all(&status_request_frame()).await.unwrap();

    let (status_id, status_body) = read_one_frame(&mut client).await;
    assert_eq!(status_id, 0x00);
    let (json, _) = read_string(&status_body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.contains("\"protocol\":777"));
}

#[tokio::test]
async fn malformed_connection_is_isolated() {
    let addr = spawn_server().await;

    let mut bad = TcpStream::connect(addr).await.unwrap();
    bad.write_all(&handshake_frame(2)).await.unwrap();
    bad.write_all(&status_request_frame()).await.unwrap();
    let mut scratch = [0u8; 16];
    let closed = bad.read(&mut scratch).await.unwrap();
    assert_eq!(closed, 0, "login intent must close the connection");

    let mut good = TcpStream::connect(addr).await.unwrap();
    good.write_all(&handshake_frame(1)).await.unwrap();
    good.write_all(&status_request_frame()).await.unwrap();
    let (status_id, status_body) = read_one_frame(&mut good).await;
    assert_eq!(status_id, 0x00);
    let (json, _) = read_string(&status_body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.contains("\"name\":\"26.3\""));
}

#[tokio::test]
async fn unknown_packet_id_closes() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut body = Vec::new();
    write_var_int(0x7E, &mut body);
    client.write_all(&encode_frame(&body)).await.unwrap();

    let mut scratch = [0u8; 16];
    let closed = client.read(&mut scratch).await.unwrap();
    assert_eq!(closed, 0, "unknown handshake id must close the connection");
}
