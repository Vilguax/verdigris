use std::time::Duration;

use tokio::net::TcpListener;
use tokio::time::timeout;

use verdigris_bots::orchestrator::{Config, run};

fn spawn_verdigris() -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            let _ = verdigris_server::run(listener).await;
        });
    });
    rx.recv().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn twenty_bots_all_reach_play_against_verdigris() {
    let port = spawn_verdigris();

    let config = Config {
        host: "127.0.0.1".to_owned(),
        port,
        count: 20,
        spawn_interval: Duration::from_millis(10),
        duration: Duration::from_secs(30),
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
