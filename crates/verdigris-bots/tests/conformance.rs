use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use verdigris_protocol::{
    build_compressed_packet, build_packet, decode_compressed_frame, decode_frame, read_string,
    read_var_int, write_f64, write_string, write_var_int,
};

use verdigris_bots::bot::{BotConfig, run_session};
use verdigris_bots::codec;
use verdigris_bots::conn::Conn;
use verdigris_bots::metrics::Metrics;

const THRESHOLD: usize = 256;

struct FakeServer {
    stream: DuplexStream,
    buf: Vec<u8>,
    compression: Option<usize>,
}

impl FakeServer {
    fn new(stream: DuplexStream) -> FakeServer {
        FakeServer {
            stream,
            buf: Vec::new(),
            compression: None,
        }
    }

    fn enable_compression(&mut self, threshold: usize) {
        self.compression = Some(threshold);
    }

    async fn send(&mut self, id: i32, payload: &[u8]) {
        let frame = match self.compression {
            Some(threshold) => build_compressed_packet(id, payload, threshold),
            None => build_packet(id, payload),
        }
        .unwrap();
        self.stream.write_all(&frame).await.unwrap();
    }

    async fn recv(&mut self) -> (i32, Vec<u8>) {
        let mut chunk = [0u8; 4096];
        loop {
            if let Some((id, payload, consumed)) = self.try_decode() {
                self.buf.drain(..consumed);
                return (id, payload);
            }
            let n = timeout(Duration::from_secs(5), self.stream.read(&mut chunk))
                .await
                .expect("le bot doit répondre avant expiration")
                .expect("lecture ok");
            assert!(n > 0, "le bot a fermé la connexion prématurément");
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    async fn recv_skipping(&mut self, skip: &[i32]) -> (i32, Vec<u8>) {
        loop {
            let frame = self.recv().await;
            if !skip.contains(&frame.0) {
                return frame;
            }
        }
    }

    fn try_decode(&mut self) -> Option<(i32, Vec<u8>, usize)> {
        match self.compression {
            Some(threshold) => {
                let (packet, consumed) = decode_compressed_frame(&self.buf, threshold).unwrap()?;
                let (id, header) = read_var_int(&packet).unwrap().unwrap();
                Some((id, packet[header..].to_vec(), consumed))
            }
            None => {
                let (body, consumed) = decode_frame(&self.buf).unwrap()?;
                let (id, header) = read_var_int(body).unwrap().unwrap();
                Some((id, body[header..].to_vec(), consumed))
            }
        }
    }
}

fn player_position_body(teleport_id: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(teleport_id, &mut body);
    for value in [0.5f64, 64.0, 0.5] {
        write_f64(value, &mut body);
    }
    for value in [0.0f64, 0.0, 0.0] {
        write_f64(value, &mut body);
    }
    body.extend_from_slice(&0.0f32.to_be_bytes());
    body.extend_from_slice(&0.0f32.to_be_bytes());
    body.extend_from_slice(&0i32.to_be_bytes());
    body
}

fn set_compression_body(threshold: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(threshold, &mut body);
    body
}

fn select_known_packs_body() -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(1, &mut body);
    write_string("minecraft", &mut body);
    write_string("core", &mut body);
    write_string("26.3", &mut body);
    body
}

fn chunk_batch_finished_body(count: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(count, &mut body);
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vanilla_offline_sequence_drives_state_machine() {
    let (bot_io, srv_io) = tokio::io::duplex(128 * 1024);
    let token = CancellationToken::new();
    let bot_token = token.clone();
    let metrics = Arc::new(Metrics::new());

    let cfg = BotConfig {
        host: "vanilla.example".to_owned(),
        port: 25565,
        name: "Verdi".to_owned(),
        view_distance: 8,
        walk: 0.0,
    };

    let handle = tokio::spawn(async move {
        let mut conn = Conn::new(bot_io);
        run_session(&mut conn, &cfg, 0, &bot_token, Instant::now(), &metrics).await
    });

    let mut srv = FakeServer::new(srv_io);

    let (handshake_id, handshake_body) = srv.recv().await;
    assert_eq!(handshake_id, codec::handshake::HANDSHAKE);
    let (protocol, consumed) = read_var_int(&handshake_body).unwrap().unwrap();
    assert_eq!(protocol, 777, "le bot annonce le protocole 777");
    let (address, used) = read_string(&handshake_body[consumed..], 255)
        .unwrap()
        .unwrap();
    assert_eq!(address, "vanilla.example");
    let port_off = consumed + used;
    assert_eq!(
        u16::from_be_bytes([handshake_body[port_off], handshake_body[port_off + 1]]),
        25565
    );
    let (intent, _) = read_var_int(&handshake_body[port_off + 2..])
        .unwrap()
        .unwrap();
    assert_eq!(intent, 2, "intention login");

    let (hello_id, hello_body) = srv.recv().await;
    assert_eq!(hello_id, codec::login_sb::HELLO);
    let (name, name_used) = read_string(&hello_body, 16).unwrap().unwrap();
    assert_eq!(name, "Verdi");
    assert_eq!(
        &hello_body[name_used..name_used + 16],
        &codec::offline_uuid_bytes("Verdi"),
        "l'UUID hors ligne accompagne le nom"
    );

    srv.send(
        codec::login_cb::SET_COMPRESSION,
        &set_compression_body(THRESHOLD as i32),
    )
    .await;
    srv.enable_compression(THRESHOLD);
    srv.send(codec::login_cb::FINISHED, &[]).await;

    let (ack_id, _) = srv.recv().await;
    assert_eq!(ack_id, codec::login_sb::ACKNOWLEDGED);

    let (info_id, info_body) = srv.recv().await;
    assert_eq!(info_id, codec::config_sb::CLIENT_INFORMATION);
    assert_eq!(
        info_body,
        codec::client_information_payload(8),
        "client information conforme au format 26.3"
    );

    srv.send(
        codec::config_cb::SELECT_KNOWN_PACKS,
        &select_known_packs_body(),
    )
    .await;
    let (packs_id, packs_body) = srv.recv().await;
    assert_eq!(packs_id, codec::config_sb::SELECT_KNOWN_PACKS);
    assert_eq!(
        packs_body,
        codec::select_known_packs_reply(),
        "le bot répond le pack core 26.3"
    );

    srv.send(codec::config_cb::FINISH_CONFIGURATION, &[]).await;
    let (finish_id, _) = srv.recv().await;
    assert_eq!(finish_id, codec::config_sb::FINISH_CONFIGURATION);

    srv.send(codec::play_cb::PLAYER_POSITION, &player_position_body(7))
        .await;

    let (accept_id, accept_body) = srv.recv_skipping(&[codec::play_sb::CLIENT_TICK_END]).await;
    assert_eq!(accept_id, codec::play_sb::ACCEPT_TELEPORTATION);
    let (teleport_id, _) = read_var_int(&accept_body).unwrap().unwrap();
    assert_eq!(teleport_id, 7, "le bot confirme la téléportation reçue");

    let (loaded_id, _) = srv.recv_skipping(&[codec::play_sb::CLIENT_TICK_END]).await;
    assert_eq!(loaded_id, codec::play_sb::PLAYER_LOADED);

    srv.send(codec::play_cb::LEVEL_CHUNK_WITH_LIGHT, &[0u8; 16])
        .await;
    srv.send(
        codec::play_cb::CHUNK_BATCH_FINISHED,
        &chunk_batch_finished_body(1),
    )
    .await;

    let (received_id, received_body) = srv.recv_skipping(&[codec::play_sb::CLIENT_TICK_END]).await;
    assert_eq!(received_id, codec::play_sb::CHUNK_BATCH_RECEIVED);
    assert_eq!(
        f32::from_be_bytes([
            received_body[0],
            received_body[1],
            received_body[2],
            received_body[3],
        ]),
        25.0,
        "chunk batch received demande 25 chunks/tick"
    );

    token.cancel();
    let outcome = timeout(Duration::from_secs(5), handle)
        .await
        .expect("la session doit se terminer après annulation")
        .expect("pas de panique");

    assert!(outcome.reached_login_finished.is_some());
    assert!(outcome.reached_play.is_some());
    assert!(outcome.first_chunk.is_some());
    assert_eq!(outcome.chunks, 1);
    assert!(
        outcome.failure.is_none(),
        "annulé en jeu: compté comme en jeu, pas un échec"
    );
    assert!(outcome.is_in_play());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_mode_server_aborts_bot() {
    let (bot_io, srv_io) = tokio::io::duplex(64 * 1024);
    let token = CancellationToken::new();
    let bot_token = token.clone();
    let metrics = Arc::new(Metrics::new());

    let cfg = BotConfig {
        host: "online.example".to_owned(),
        port: 25565,
        name: "Verdi".to_owned(),
        view_distance: 8,
        walk: 0.0,
    };

    let handle = tokio::spawn(async move {
        let mut conn = Conn::new(bot_io);
        run_session(&mut conn, &cfg, 0, &bot_token, Instant::now(), &metrics).await
    });

    let mut srv = FakeServer::new(srv_io);
    let _ = srv.recv().await;
    let _ = srv.recv().await;
    srv.send(codec::login_cb::HELLO, &[0u8; 4]).await;

    let outcome = timeout(Duration::from_secs(5), handle)
        .await
        .expect("la session doit se terminer")
        .expect("pas de panique");
    assert_eq!(
        outcome.failure,
        Some(verdigris_bots::bot::FailureCause::Online),
        "une requête de chiffrement signale un serveur en mode en ligne"
    );
}

#[tokio::test]
#[ignore = "requiert un serveur vanilla/Paper 26.3 hors ligne; définir VERDIGRIS_BOTS_EXTERNAL_PORT"]
async fn external_offline_server_round_trip() {
    let Ok(port) = std::env::var("VERDIGRIS_BOTS_EXTERNAL_PORT") else {
        return;
    };
    let port: u16 = port.parse().expect("port numérique");

    let config = verdigris_bots::orchestrator::Config {
        host: "127.0.0.1".to_owned(),
        port,
        count: 20,
        spawn_interval: Duration::from_millis(20),
        duration: Duration::from_secs(5),
        prefix: "Bot".to_owned(),
        view_distance: 8,
        walk: 4.0,
        json: None,
    };
    let report = verdigris_bots::orchestrator::run(config).await;
    assert_eq!(report.in_play, 20, "tous les bots en jeu");
    assert_eq!(report.failed, 0, "aucun échec");
    assert!(report.total_chunks > 0, "des chunks sont reçus");
}
