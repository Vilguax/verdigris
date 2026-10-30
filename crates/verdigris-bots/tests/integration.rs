use std::time::Duration;

use tokio::net::TcpListener;
use tokio::time::timeout;

use verdigris_bots::orchestrator::{Config, run};

async fn spawn_verdigris() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = verdigris_server::run(listener).await;
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn twenty_bots_all_reach_play_against_verdigris() {
    let port = spawn_verdigris().await;

    let config = Config {
        host: "127.0.0.1".to_owned(),
        port,
        count: 20,
        spawn_interval: Duration::from_millis(10),
        duration: Duration::from_secs(12),
        prefix: "Bot".to_owned(),
        view_distance: 2,
        walk: 4.0,
        json: None,
    };

    let report = timeout(Duration::from_secs(120), run(config))
        .await
        .expect("le générateur doit se terminer");

    assert_eq!(report.requested, 20);
    assert_eq!(report.in_play, 20, "les 20 bots atteignent le mode jeu");
    assert_eq!(
        report.failed, 0,
        "aucun échec: {:?}",
        report.failures_by_cause
    );
    assert!(
        report.total_chunks > 0,
        "les bots reçoivent des chunks du monde Verdigris"
    );
    assert!(
        report.play.p50_ms.is_some(),
        "percentile d'entrée en jeu présent"
    );
    assert_eq!(report.play.n, 20);
}
