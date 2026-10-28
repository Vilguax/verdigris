use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, timeout};

use verdigris_protocol::{
    MAX_STRING_LEN, MAX_USERNAME_LEN, decode_compressed_frame, decode_frame,
    encode_compressed_frame, encode_frame, offline_uuid, read_i64, read_string, read_var_int,
    write_i64, write_string, write_u16, write_var_int,
};
use verdigris_server::{
    REASON_CONFIG_NOT_VANILLA, REASON_LOGIN_INVALID_NAME, REASON_PLAY_WORLD_UNAVAILABLE, run,
};

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

fn login_start_frame(name: &str) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x00, &mut body);
    write_string(name, &mut body);
    body.extend_from_slice(&[0u8; 16]);
    encode_frame(&body)
}

const CLIENT_THRESHOLD: usize = 256;

fn compressed_login_acknowledged_frame() -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x03, &mut body);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

fn compressed_login_start_frame(name: &str) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x00, &mut body);
    write_string(name, &mut body);
    body.extend_from_slice(&[0u8; 16]);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

fn compressed_known_packs_frame(packs: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x07, &mut body);
    write_var_int(packs.len() as i32, &mut body);
    for (namespace, id, version) in packs {
        write_string(namespace, &mut body);
        write_string(id, &mut body);
        write_string(version, &mut body);
    }
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

fn compressed_finish_configuration_frame() -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x03, &mut body);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

async fn drive_to_login_acknowledged(client: &mut TcpStream, buf: &mut Vec<u8>, name: &str) {
    client.write_all(&handshake_frame(2)).await.unwrap();
    client.write_all(&login_start_frame(name)).await.unwrap();

    let (set_compression_id, set_compression_body) = read_one_frame(client, buf).await;
    assert_eq!(set_compression_id, 0x03);
    let (threshold, _) = read_var_int(&set_compression_body).unwrap().unwrap();
    assert_eq!(threshold, 256);

    let (success_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(success_id, 0x02);

    client
        .write_all(&compressed_login_acknowledged_frame())
        .await
        .unwrap();
}

fn assert_core_known_packs(payload: &[u8]) {
    let (count, header) = read_var_int(payload).unwrap().unwrap();
    assert_eq!(count, 1, "server advertises exactly one known pack");
    let mut cursor = &payload[header..];
    let (namespace, used) = read_string(cursor, MAX_STRING_LEN).unwrap().unwrap();
    cursor = &cursor[used..];
    let (id, used) = read_string(cursor, MAX_STRING_LEN).unwrap().unwrap();
    cursor = &cursor[used..];
    let (version, _used) = read_string(cursor, MAX_STRING_LEN).unwrap().unwrap();
    assert_eq!(namespace, "minecraft");
    assert_eq!(id, "core");
    assert_eq!(version, "26.3");
}

async fn expect_clean_eof(client: &mut TcpStream) {
    let mut scratch = [0u8; 64];
    loop {
        match client.read(&mut scratch).await {
            Ok(0) => break,
            Ok(_) => continue,
            Err(err) => panic!("expected clean EOF, got {err}"),
        }
    }
}

async fn read_one_compressed_frame(stream: &mut TcpStream, buf: &mut Vec<u8>) -> (i32, Vec<u8>) {
    let mut chunk = [0u8; 1024];
    loop {
        if let Some((body, consumed)) = decode_compressed_frame(buf, CLIENT_THRESHOLD).unwrap() {
            let (id, header) = read_var_int(&body).unwrap().unwrap();
            let payload = body[header..].to_vec();
            buf.drain(..consumed);
            return (id, payload);
        }
        let n = timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .expect("read did not time out")
            .expect("read ok");
        assert!(n > 0, "server closed before sending a full frame");
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn read_one_frame(stream: &mut TcpStream, buf: &mut Vec<u8>) -> (i32, Vec<u8>) {
    let mut chunk = [0u8; 1024];
    loop {
        let decoded = decode_frame(buf).unwrap().map(|(body, consumed)| {
            let (id, header) = read_var_int(body).unwrap().unwrap();
            (id, body[header..].to_vec(), consumed)
        });
        if let Some((id, payload, consumed)) = decoded {
            buf.drain(..consumed);
            return (id, payload);
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

    let mut buf = Vec::new();
    client.write_all(&handshake_frame(1)).await.unwrap();
    client.write_all(&status_request_frame()).await.unwrap();

    let (status_id, status_body) = read_one_frame(&mut client, &mut buf).await;
    assert_eq!(status_id, 0x00);
    let (json, _) = read_string(&status_body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.contains("\"name\":\"26.3\""));
    assert!(json.contains("\"protocol\":777"));
    assert!(json.contains("\"players\""));
    assert!(json.contains("\"description\""));

    let payload: i64 = 0x0123456789ABCDEF;
    client.write_all(&ping_frame(payload)).await.unwrap();
    let (pong_id, pong_body) = read_one_frame(&mut client, &mut buf).await;
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

    let mut buf = Vec::new();
    let (status_id, status_body) = read_one_frame(&mut client, &mut buf).await;
    assert_eq!(status_id, 0x00);
    let (json, _) = read_string(&status_body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.contains("\"protocol\":777"));
}

#[tokio::test]
async fn offline_login_success_carries_offline_identity() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    client.write_all(&handshake_frame(2)).await.unwrap();
    client.write_all(&login_start_frame("Verdi")).await.unwrap();

    let (set_compression_id, set_compression_body) = read_one_frame(&mut client, &mut buf).await;
    assert_eq!(
        set_compression_id, 0x03,
        "login start must first yield Set Compression 0x03 uncompressed"
    );
    let (threshold, _) = read_var_int(&set_compression_body).unwrap().unwrap();
    assert_eq!(threshold, 256, "compression threshold must be 256");

    let (success_id, success_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        success_id, 0x02,
        "login start must yield Login Success 0x02"
    );

    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&success_body[..16]);
    assert_eq!(u128::from_be_bytes(uuid_bytes), offline_uuid("Verdi"));

    let mut cursor = &success_body[16..];
    let (name, consumed) = read_string(cursor, MAX_USERNAME_LEN).unwrap().unwrap();
    assert_eq!(name, "Verdi");
    cursor = &cursor[consumed..];

    let (properties, consumed) = read_var_int(cursor).unwrap().unwrap();
    assert_eq!(properties, 0, "offline login carries zero properties");
    cursor = &cursor[consumed..];

    assert_eq!(cursor.len(), 16, "session id is a 16-byte uuid");
    assert_eq!(cursor[6] & 0xF0, 0x40, "session id must be uuid v4");
    assert_eq!(
        cursor[8] & 0xC0,
        0x80,
        "session id must use the ietf variant"
    );
}

#[tokio::test]
async fn configuration_full_sequence_reaches_play_disconnect() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    drive_to_login_acknowledged(&mut client, &mut buf, "Verdi").await;

    let (known_packs_id, known_packs_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        known_packs_id, 0x0F,
        "configuration starts with clientbound Known Packs 0x0F"
    );
    assert_core_known_packs(&known_packs_body);

    client
        .write_all(&compressed_known_packs_frame(&[(
            "minecraft",
            "core",
            "26.3",
        )]))
        .await
        .unwrap();

    for index in 0..29 {
        let (registry_id, _) = read_one_compressed_frame(&mut client, &mut buf).await;
        assert_eq!(
            registry_id, 0x07,
            "registry data #{index} must be Registry Data 0x07"
        );
    }

    let (tags_id, _) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(tags_id, 0x0E, "registries are followed by Update Tags 0x0E");

    let (finish_id, finish_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        finish_id, 0x03,
        "tags are followed by Finish Configuration 0x03"
    );
    assert!(
        finish_body.is_empty(),
        "Finish Configuration has no payload"
    );

    client
        .write_all(&compressed_finish_configuration_frame())
        .await
        .unwrap();

    let (disconnect_id, disconnect_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(disconnect_id, 0x20, "play disconnect is id 0x20");
    assert_eq!(disconnect_body[0], 0x08, "play disconnect is an nbt string");
    assert!(
        contains_subslice(&disconnect_body, REASON_PLAY_WORLD_UNAVAILABLE.as_bytes()),
        "play disconnect must say the world is unavailable"
    );

    expect_clean_eof(&mut client).await;
}

#[tokio::test]
async fn configuration_rejects_empty_known_packs() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    drive_to_login_acknowledged(&mut client, &mut buf, "Verdi").await;

    let (known_packs_id, known_packs_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(known_packs_id, 0x0F);
    assert_core_known_packs(&known_packs_body);

    client
        .write_all(&compressed_known_packs_frame(&[]))
        .await
        .unwrap();

    let (disconnect_id, disconnect_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(disconnect_id, 0x02, "configuration disconnect is id 0x02");
    assert_eq!(
        disconnect_body[0], 0x08,
        "config disconnect is an nbt string"
    );
    assert!(
        contains_subslice(&disconnect_body, REASON_CONFIG_NOT_VANILLA.as_bytes()),
        "empty known packs must be refused as non-vanilla"
    );

    expect_clean_eof(&mut client).await;
}

#[tokio::test]
async fn offline_login_rejects_invalid_name() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    client.write_all(&handshake_frame(2)).await.unwrap();
    client
        .write_all(&login_start_frame("Bad Name"))
        .await
        .unwrap();

    let mut buf = Vec::new();
    let (id, body) = read_one_frame(&mut client, &mut buf).await;
    assert_eq!(id, 0x00, "invalid name refusal is a login Disconnect 0x00");
    let (json, _) = read_string(&body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.starts_with("{\"text\":\""));
    assert!(json.contains(REASON_LOGIN_INVALID_NAME));
}

#[tokio::test]
async fn offline_login_rejects_name_over_sixteen() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    client.write_all(&handshake_frame(2)).await.unwrap();
    client
        .write_all(&login_start_frame(&"a".repeat(17)))
        .await
        .unwrap();

    let mut buf = Vec::new();
    let (id, body) = read_one_frame(&mut client, &mut buf).await;
    assert_eq!(
        id, 0x00,
        "oversized name refusal is a login Disconnect 0x00"
    );
    let (json, _) = read_string(&body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.starts_with("{\"text\":\""));
    assert!(json.contains(REASON_LOGIN_INVALID_NAME));
}

#[tokio::test]
async fn offline_login_rejects_unexpected_packet_after_success() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    client.write_all(&handshake_frame(2)).await.unwrap();
    client.write_all(&login_start_frame("Verdi")).await.unwrap();

    let (set_compression_id, _) = read_one_frame(&mut client, &mut buf).await;
    assert_eq!(set_compression_id, 0x03);

    let (success_id, _) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(success_id, 0x02);

    client
        .write_all(&compressed_login_start_frame("Verdi"))
        .await
        .unwrap();

    let (id, body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(id, 0x00, "unexpected packet refusal stays in login state");
    let (json, _) = read_string(&body, MAX_STRING_LEN).unwrap().unwrap();
    assert!(json.contains("login paquet inattendu"));
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
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
