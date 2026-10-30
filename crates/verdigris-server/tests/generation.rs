use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use verdigris_protocol::{World, write_chunk_payload};
use verdigris_server::{DEFAULT_SEED, GenMetrics, NoiseGenerator};
use verdigris_worldgen::WorldGenerator;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn noise_chunk_builds_off_the_network_runtime() {
    let generator = Arc::new(WorldGenerator::new(DEFAULT_SEED).expect("generator"));
    let metrics = Arc::new(GenMetrics::default());
    let noise = NoiseGenerator::new(Arc::clone(&generator), Arc::clone(&metrics)).expect("noise");
    let world = Arc::new(World::new(Box::new(noise)));

    let progressed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&progressed);
    let concurrent = tokio::spawn(async move {
        flag.store(true, Ordering::SeqCst);
    });

    let world_for_blocking = Arc::clone(&world);
    let payload_len = tokio::task::spawn_blocking(move || {
        let column = world_for_blocking.chunk(0, 0);
        let column = column.lock().unwrap();
        let mut payload = Vec::new();
        write_chunk_payload(0, 0, &column, &mut payload);
        payload.len()
    })
    .await
    .expect("génération hors runtime réseau");

    concurrent.await.expect("tâche réseau concurrente");
    assert!(
        progressed.load(Ordering::SeqCst),
        "une tâche concurrente progresse pendant la génération"
    );
    assert!(payload_len > 0, "le chunk noise produit un payload");

    let (count, nanos) = metrics.take();
    assert_eq!(count, 1, "exactement une génération comptabilisée");
    assert!(nanos > 0, "la durée de génération est enregistrée");
}
