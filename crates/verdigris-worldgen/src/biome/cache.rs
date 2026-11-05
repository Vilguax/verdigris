use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub const DEFAULT_BIOME_CACHE_CAPACITY: usize = 4096;

struct CacheInner {
    map: HashMap<(i32, i32), Arc<[u16]>>,
    order: VecDeque<(i32, i32)>,
}

pub(crate) struct BiomeGridCache {
    capacity: usize,
    inner: Mutex<CacheInner>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl BiomeGridCache {
    pub(crate) fn new(capacity: usize) -> Self {
        BiomeGridCache {
            capacity: capacity.max(1),
            inner: Mutex::new(CacheInner {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub(crate) fn get(&self, key: (i32, i32)) -> Option<Arc<[u16]>> {
        let found = {
            let inner = self.inner.lock().unwrap();
            inner.map.get(&key).cloned()
        };
        if found.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        found
    }

    pub(crate) fn insert(&self, key: (i32, i32), grid: Arc<[u16]>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.map.insert(key, grid).is_some() {
            return;
        }
        inner.order.push_back(key);
        while inner.map.len() > self.capacity {
            if let Some(evicted) = inner.order.pop_front() {
                inner.map.remove(&evicted);
            } else {
                break;
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.inner.lock().unwrap().map.len()
    }

    pub(crate) fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    pub(crate) fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}
