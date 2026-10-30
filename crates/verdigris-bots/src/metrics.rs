use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

#[derive(Default)]
pub struct Metrics {
    pub in_play: AtomicI64,
    pub failures: AtomicU64,
    pub chunks: AtomicU64,
    pub bytes: AtomicU64,
}

impl Metrics {
    pub fn new() -> Metrics {
        Metrics::default()
    }

    pub fn add_chunks(&self, n: u64) {
        self.chunks.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_bytes(&self, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
    }

    pub fn enter_play(&self) {
        self.in_play.fetch_add(1, Ordering::Relaxed);
    }

    pub fn leave_play(&self) {
        self.in_play.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            in_play: self.in_play.load(Ordering::Relaxed).max(0) as u64,
            failures: self.failures.load(Ordering::Relaxed),
            chunks: self.chunks.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MetricsSnapshot {
    pub in_play: u64,
    pub failures: u64,
    pub chunks: u64,
    pub bytes: u64,
}
