use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, timeout};

use verdigris_protocol::{
    MAX_STRING_LEN, MAX_USERNAME_LEN, decode_compressed_frame, decode_frame,
    encode_compressed_frame, encode_frame, offline_uuid, read_f64, read_i64, read_string,
    read_var_int, write_f32, write_f64, write_i64, write_string, write_u16, write_var_int,
};
use verdigris_server::{
    KeepAliveConfig, REASON_CONFIG_NOT_VANILLA, REASON_KEEP_ALIVE_TIMEOUT,
    REASON_LOGIN_INVALID_NAME, run, run_with_config,
};

async fn spawn_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = run(listener).await;
    });
    addr
}

async fn spawn_server_with_keep_alive(config: KeepAliveConfig) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = run_with_config(listener, config).await;
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

fn compressed_accept_teleportation_frame(
    teleport_id: i32,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x00, &mut body);
    write_var_int(teleport_id, &mut body);
    write_f64(x, &mut body);
    write_f64(y, &mut body);
    write_f64(z, &mut body);
    write_f32(yaw, &mut body);
    write_f32(pitch, &mut body);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

fn compressed_player_loaded_frame() -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x2C, &mut body);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

fn compressed_unknown_play_frame() -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x7F, &mut body);
    body.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

async fn drive_to_play(client: &mut TcpStream, buf: &mut Vec<u8>) {
    drive_to_login_acknowledged(client, buf, "Verdi").await;

    let (known_packs_id, known_packs_body) = read_one_compressed_frame(client, buf).await;
    assert_eq!(known_packs_id, 0x0F);
    assert_core_known_packs(&known_packs_body);

    client
        .write_all(&compressed_known_packs_frame(&[(
            "minecraft",
            "core",
            "26.3",
        )]))
        .await
        .unwrap();

    for _ in 0..verdigris_protocol::network::network_data().registries.len() {
        let (registry_id, _) = read_one_compressed_frame(client, buf).await;
        assert_eq!(registry_id, 0x07);
    }

    let (tags_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(tags_id, 0x0E);

    let (finish_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(finish_id, 0x03);

    client
        .write_all(&compressed_finish_configuration_frame())
        .await
        .unwrap();
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
async fn play_sequence_sends_empty_world_and_stays_open() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    drive_to_play(&mut client, &mut buf).await;

    let (login_id, _) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        login_id, 0x32,
        "entrée en play commence par Login play 0x32"
    );

    let (position_id, position_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        position_id, 0x49,
        "Login play is followed by Player Position"
    );
    let mut cursor = &position_body[..];
    let (teleport_id, consumed) = read_var_int(cursor).unwrap().unwrap();
    cursor = &cursor[consumed..];
    let (x, consumed) = read_f64(cursor).unwrap().unwrap();
    cursor = &cursor[consumed..];
    let (y, consumed) = read_f64(cursor).unwrap().unwrap();
    cursor = &cursor[consumed..];
    let (z, _) = read_f64(cursor).unwrap().unwrap();
    assert_eq!(
        (x, y, z),
        (0.5, -60.0, 0.5),
        "spawn position is (0.5,-60,0.5)"
    );

    let (game_event_id, game_event_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(game_event_id, 0x27, "position is followed by Game Event");
    assert_eq!(
        game_event_body[0], 13,
        "game event type is 13 (wait for chunks)"
    );

    let (center_id, _) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        center_id, 0x60,
        "Game Event is followed by Set Chunk Cache Center"
    );

    let (batch_start_id, batch_start_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        batch_start_id, 0x0C,
        "cache center is followed by Chunk Batch Start"
    );
    assert!(
        batch_start_body.is_empty(),
        "Chunk Batch Start has no payload"
    );

    let mut first_chunk = None;
    for index in 0..289 {
        let (chunk_id, chunk_body) = read_one_compressed_frame(&mut client, &mut buf).await;
        assert_eq!(
            chunk_id, 0x2E,
            "chunk #{index} must be Level Chunk With Light 0x2E"
        );
        if index == 0 {
            first_chunk = Some(chunk_coords(&chunk_body));
        }
    }
    assert_eq!(
        first_chunk,
        Some((0, 0)),
        "the nearest chunk to the spawn is sent first"
    );

    let (batch_finished_id, batch_finished_body) =
        read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(
        batch_finished_id, 0x0B,
        "chunks are followed by Chunk Batch Finished"
    );
    let (count, _) = read_var_int(&batch_finished_body).unwrap().unwrap();
    assert_eq!(
        count, 289,
        "Chunk Batch Finished reports the 289 chunks of the radius-8 square"
    );

    client
        .write_all(&compressed_accept_teleportation_frame(
            teleport_id,
            0.5,
            -60.0,
            0.5,
            0.0,
            0.0,
        ))
        .await
        .unwrap();
    client
        .write_all(&compressed_player_loaded_frame())
        .await
        .unwrap();
    client
        .write_all(&compressed_unknown_play_frame())
        .await
        .unwrap();

    let mut scratch = [0u8; 64];
    let outcome = timeout(Duration::from_millis(1000), client.read(&mut scratch)).await;
    assert!(
        outcome.is_err(),
        "connection must stay open in play without sending a disconnect or FIN"
    );
}

#[tokio::test]
async fn moving_three_chunks_loads_and_forgets() {
    use std::collections::HashSet;

    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    drive_to_play(&mut client, &mut buf).await;
    consume_empty_world(&mut client, &mut buf).await;

    client
        .write_all(&compressed_move_player_pos_frame(48.5, -60.0, 0.5, 0x01))
        .await
        .unwrap();

    let (center_id, center_body) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(center_id, 0x60, "a chunk change re-sends the cache center");
    let (cx, consumed) = read_var_int(&center_body).unwrap().unwrap();
    let (cz, _) = read_var_int(&center_body[consumed..]).unwrap().unwrap();
    assert_eq!((cx, cz), (3, 0), "the new center is the player's chunk");

    let mut forgotten = HashSet::new();
    for _ in 0..34 {
        let (forget_id, forget_body) = read_one_compressed_frame(&mut client, &mut buf).await;
        assert_eq!(
            forget_id, 0x26,
            "chunks leaving the radius-9 square use Forget Level Chunk 0x26"
        );
        forgotten.insert(forget_coords(&forget_body));
    }
    let mut expected_forget = HashSet::new();
    for x in -8..=-7 {
        for z in -8..=8 {
            expected_forget.insert((x, z));
        }
    }
    assert_eq!(
        forgotten, expected_forget,
        "only the two far columns are forgotten"
    );

    let (batch_start_id, _) = read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(batch_start_id, 0x0C, "the new chunks arrive in a batch");

    let mut loaded = Vec::new();
    for _ in 0..51 {
        let (chunk_id, chunk_body) = read_one_compressed_frame(&mut client, &mut buf).await;
        assert_eq!(chunk_id, 0x2E);
        loaded.push(chunk_coords(&chunk_body));
    }

    let (batch_finished_id, batch_finished_body) =
        read_one_compressed_frame(&mut client, &mut buf).await;
    assert_eq!(batch_finished_id, 0x0B);
    let (count, _) = read_var_int(&batch_finished_body).unwrap().unwrap();
    assert_eq!(count, 51, "exactly the 51 missing chunks are announced");

    let loaded_set: HashSet<(i32, i32)> = loaded.iter().copied().collect();
    let mut expected_load = HashSet::new();
    for x in 9..=11 {
        for z in -8..=8 {
            expected_load.insert((x, z));
        }
    }
    assert_eq!(
        loaded_set, expected_load,
        "only the three new columns are loaded"
    );

    assert_eq!(loaded[0], (9, 0), "the nearest new chunk is sent first");
    let mut previous = -1i64;
    for &(x, z) in &loaded {
        let dx = (x - 3) as i64;
        let dz = z as i64;
        let d2 = dx * dx + dz * dz;
        assert!(
            d2 >= previous,
            "chunks are sent in order of growing distance"
        );
        previous = d2;
    }
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

fn compressed_keep_alive_frame(id: i64) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x1C, &mut body);
    write_i64(id, &mut body);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

async fn consume_empty_world(client: &mut TcpStream, buf: &mut Vec<u8>) {
    let (login_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(login_id, 0x32);
    let (position_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(position_id, 0x49);
    let (game_event_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(game_event_id, 0x27);
    let (center_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(center_id, 0x60);
    let (batch_start_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(batch_start_id, 0x0C);
    for _ in 0..289 {
        let (chunk_id, _) = read_one_compressed_frame(client, buf).await;
        assert_eq!(chunk_id, 0x2E);
    }
    let (batch_finished_id, _) = read_one_compressed_frame(client, buf).await;
    assert_eq!(batch_finished_id, 0x0B);
}

fn chunk_coords(chunk_body: &[u8]) -> (i32, i32) {
    let mut x = [0u8; 4];
    x.copy_from_slice(&chunk_body[..4]);
    let mut z = [0u8; 4];
    z.copy_from_slice(&chunk_body[4..8]);
    (i32::from_be_bytes(x), i32::from_be_bytes(z))
}

fn forget_coords(forget_body: &[u8]) -> (i32, i32) {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&forget_body[..8]);
    let value = i64::from_be_bytes(bytes);
    ((value & 0xFFFF_FFFF) as i32, (value >> 32) as i32)
}

fn compressed_move_player_pos_frame(x: f64, y: f64, z: f64, flags: u8) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(0x1E, &mut body);
    write_f64(x, &mut body);
    write_f64(y, &mut body);
    write_f64(z, &mut body);
    body.push(flags);
    encode_compressed_frame(&body, CLIENT_THRESHOLD)
}

async fn read_keep_alive_id(client: &mut TcpStream, buf: &mut Vec<u8>) -> i64 {
    let (id, body) = read_one_compressed_frame(client, buf).await;
    assert_eq!(
        id, 0x2D,
        "serveur doit envoyer un Keep Alive clientbound 0x2D"
    );
    let (value, _) = read_i64(&body).unwrap().unwrap();
    value
}

async fn try_read_compressed_frame(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    budget: Duration,
) -> Option<(i32, Vec<u8>)> {
    let mut chunk = [0u8; 1024];
    loop {
        if let Some((body, consumed)) = decode_compressed_frame(buf, CLIENT_THRESHOLD).unwrap() {
            let (id, header) = read_var_int(&body).unwrap().unwrap();
            let payload = body[header..].to_vec();
            buf.drain(..consumed);
            return Some((id, payload));
        }
        let n = match timeout(budget, stream.read(&mut chunk)).await {
            Ok(Ok(n)) => n,
            Ok(Err(err)) => panic!("read error: {err}"),
            Err(_) => return None,
        };
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[tokio::test]
async fn keep_alive_responder_stays_connected_across_intervals() {
    let addr = spawn_server_with_keep_alive(KeepAliveConfig {
        interval: Duration::from_millis(120),
        timeout: Duration::from_millis(600),
        check_period: Duration::from_millis(20),
    })
    .await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    drive_to_play(&mut client, &mut buf).await;
    consume_empty_world(&mut client, &mut buf).await;

    let id1 = read_keep_alive_id(&mut client, &mut buf).await;
    assert_eq!(id1, 1, "le premier identifiant Keep Alive vaut 1");

    let gap = Duration::from_millis(250);
    let second = try_read_compressed_frame(&mut client, &mut buf, gap).await;
    assert!(second.is_none(), "pas de second Keep Alive avant réponse");

    client
        .write_all(&compressed_keep_alive_frame(id1))
        .await
        .unwrap();

    let id2 = read_keep_alive_id(&mut client, &mut buf).await;
    assert_eq!(
        id2,
        id1 + 1,
        "identifiants Keep Alive strictement croissants"
    );
    client
        .write_all(&compressed_keep_alive_frame(id2))
        .await
        .unwrap();

    let id3 = read_keep_alive_id(&mut client, &mut buf).await;
    assert_eq!(id3, id2 + 1);
    client
        .write_all(&compressed_keep_alive_frame(id3))
        .await
        .unwrap();

    let gap = Duration::from_millis(80);
    let silence = try_read_compressed_frame(&mut client, &mut buf, gap).await;
    assert!(
        silence.is_none(),
        "connexion maintenue sur plusieurs intervalles"
    );
}

#[tokio::test]
async fn silent_client_is_disconnected_after_timeout() {
    let addr = spawn_server_with_keep_alive(KeepAliveConfig {
        interval: Duration::from_millis(120),
        timeout: Duration::from_millis(300),
        check_period: Duration::from_millis(20),
    })
    .await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    drive_to_play(&mut client, &mut buf).await;
    consume_empty_world(&mut client, &mut buf).await;

    let id1 = read_keep_alive_id(&mut client, &mut buf).await;
    assert_eq!(id1, 1);

    let gap = Duration::from_secs(2);
    let frame = try_read_compressed_frame(&mut client, &mut buf, gap).await;
    let (disconnect_id, disconnect_body) = frame.expect("Disconnect play attendu");
    assert_eq!(
        disconnect_id, 0x20,
        "le Disconnect play a l'identifiant 0x20"
    );
    assert_eq!(
        disconnect_body[0], 0x08,
        "le Disconnect play est une chaîne NBT"
    );
    assert!(
        contains_subslice(&disconnect_body, REASON_KEEP_ALIVE_TIMEOUT.as_bytes()),
        "le Disconnect play mentionne le délai de keep alive dépassé"
    );

    expect_clean_eof(&mut client).await;
}

#[tokio::test]
async fn wrong_and_zero_keep_alive_ids_do_not_disconnect() {
    let addr = spawn_server_with_keep_alive(KeepAliveConfig {
        interval: Duration::from_millis(200),
        timeout: Duration::from_millis(800),
        check_period: Duration::from_millis(20),
    })
    .await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    let mut buf = Vec::new();
    drive_to_play(&mut client, &mut buf).await;
    consume_empty_world(&mut client, &mut buf).await;

    client
        .write_all(&compressed_keep_alive_frame(0))
        .await
        .unwrap();
    client
        .write_all(&compressed_keep_alive_frame(999))
        .await
        .unwrap();

    let id1 = read_keep_alive_id(&mut client, &mut buf).await;
    assert_eq!(id1, 1, "identifiant ignoré avant tout Keep Alive serveur");

    client
        .write_all(&compressed_keep_alive_frame(id1 + 12345))
        .await
        .unwrap();

    let gap = Duration::from_millis(300);
    let second = try_read_compressed_frame(&mut client, &mut buf, gap).await;
    assert!(
        second.is_none(),
        "identifiant erroné: attente conservée, pas d'envoi"
    );

    client
        .write_all(&compressed_keep_alive_frame(id1))
        .await
        .unwrap();

    let id2 = read_keep_alive_id(&mut client, &mut buf).await;
    assert_eq!(
        id2,
        id1 + 1,
        "surveillance maintenue après réponse correcte"
    );
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
