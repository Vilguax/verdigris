use std::sync::Arc;
use std::thread;
use std::time::Instant;

use verdigris_worldgen::data::WorldgenData;
use verdigris_worldgen::{AquiferMode, SearchState, WorldGenerator};

const SEED: i64 = 20_261_029;

fn chunk_coords() -> Vec<(i32, i32)> {
    let mut coords = Vec::new();
    for cz in -2i32..=2 {
        for cx in -2i32..=2 {
            coords.push((cx, cz));
        }
    }
    coords
}

#[test]
fn cached_grid_matches_uncached_reference() {
    let data = WorldgenData::embedded().expect("worldgen data");
    let cached = data.build_biome_source(SEED).expect("cached source");
    let reference = data.build_biome_source(SEED).expect("reference source");

    let coords = chunk_coords();
    assert!(coords.len() >= 20);

    let mut state_a = SearchState::new();
    let mut state_b = SearchState::new();

    for &(cx, cz) in &coords {
        let got = cached.grid(cx, cz, &mut state_a);
        let want = reference.compute_grid_uncached(cx, cz, &mut state_b);
        assert_eq!(got.len(), cached.cell_count());
        assert_eq!(&got[..], &want[..], "grid mismatch at ({cx}, {cz})");
    }

    for &(cx, cz) in &coords {
        let again = cached.grid(cx, cz, &mut state_a);
        let want = reference.compute_grid_uncached(cx, cz, &mut state_b);
        assert_eq!(&again[..], &want[..], "cached hit mismatch at ({cx}, {cz})");
    }
}

#[test]
fn ordered_nine_by_nine_bounds_misses() {
    let data = WorldgenData::embedded().expect("worldgen data");
    let source = data.build_biome_source(SEED).expect("biome source");
    source.reset_cache_stats();

    let mut state = SearchState::new();
    for cz in 0i32..9 {
        for cx in 0i32..9 {
            for dz in -1i32..=1 {
                for dx in -1i32..=1 {
                    let _ = source.grid(cx + dx, cz + dz, &mut state);
                }
            }
        }
    }

    let (hits, misses) = source.cache_stats();
    assert!(misses <= 169, "misses {misses} exceed 13*13");
    assert_eq!(misses as usize, source.cache_len());
    assert!(hits > 0, "neighbourhood overlap should reuse the cache");
}

#[test]
fn eviction_keeps_cache_within_capacity() {
    let capacity = 8usize;
    let data = WorldgenData::embedded().expect("worldgen data");
    let source = data
        .build_biome_source_with_capacity(SEED, capacity)
        .expect("small source");

    let mut state = SearchState::new();
    for cz in 0i32..6 {
        for cx in 0i32..6 {
            let _ = source.grid(cx, cz, &mut state);
            assert!(source.cache_len() <= capacity, "cache overflow");
        }
    }
    assert!(source.cache_len() <= capacity);
}

#[test]
fn eviction_survivors_stay_alive_via_arc() {
    let capacity = 2usize;
    let data = WorldgenData::embedded().expect("worldgen data");
    let source = data
        .build_biome_source_with_capacity(SEED, capacity)
        .expect("tiny source");

    let mut state = SearchState::new();
    let held = source.grid(0, 0, &mut state);
    let reference = source.compute_grid_uncached(0, 0, &mut state);

    for cz in 0i32..4 {
        for cx in 0i32..4 {
            if (cx, cz) == (0, 0) {
                continue;
            }
            let _ = source.grid(cx, cz, &mut state);
        }
    }

    assert!(source.cache_len() <= capacity);
    assert_eq!(&held[..], &reference[..]);
}

#[test]
fn shared_across_threads_yields_identical_grids() {
    let data = WorldgenData::embedded().expect("worldgen data");
    let source = Arc::new(data.build_biome_source(SEED).expect("shared source"));

    let mut single = SearchState::new();
    let expected: Vec<(i32, i32, Vec<u16>)> = chunk_coords()
        .into_iter()
        .map(|(cx, cz)| {
            let g = source.compute_grid_uncached(cx, cz, &mut single);
            (cx, cz, g)
        })
        .collect();
    let expected = Arc::new(expected);

    let mut handles = Vec::new();
    for _ in 0..8 {
        let source = Arc::clone(&source);
        let expected = Arc::clone(&expected);
        handles.push(thread::spawn(move || {
            let mut state = SearchState::new();
            for _ in 0..4 {
                for (cx, cz, want) in expected.iter() {
                    let got = source.grid(*cx, *cz, &mut state);
                    assert_eq!(&got[..], &want[..], "thread grid mismatch at ({cx}, {cz})");
                }
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread panicked");
    }
}

#[test]
#[ignore]
fn bench_ordered_zone_ms_per_chunk() {
    let generator =
        WorldGenerator::with_options(SEED, AquiferMode::Noise, true).expect("generator");
    let mut terrain = generator.terrain();

    for cz in -1i32..=0 {
        for cx in -1i32..=0 {
            generator.fill_chunk(cx, cz, &mut terrain).expect("warmup");
        }
    }

    let start = Instant::now();
    let mut count = 0u32;
    for cz in 0i32..8 {
        for cx in 0i32..8 {
            generator.fill_chunk(cx, cz, &mut terrain).expect("fill");
            count += 1;
        }
    }
    let per = start.elapsed().as_secs_f64() * 1000.0 / f64::from(count);
    println!("after: {per:.1} ms/chunk over {count} ordered chunks (seed {SEED})");
}
