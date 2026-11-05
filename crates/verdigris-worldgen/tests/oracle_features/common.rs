#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;

use flate2::read::ZlibDecoder;
use serde_json::Value;

use verdigris_worldgen::AquiferMode;
use verdigris_worldgen::WorldGenerator;
use verdigris_worldgen::chunk_fill::CHUNK_WIDTH;
use verdigris_worldgen::feature::bridge::TerrainFeatureWorld;
use verdigris_worldgen::feature::{Decoration, DecorationContext};

pub struct Generated {
    pub min_y: i32,
    pub height: usize,
    pub targets: Vec<(i32, i32)>,
    pub enabled: BTreeSet<String>,
    pub decorated: HashSet<(i32, i32)>,
    pub pre: HashMap<(i32, i32), Vec<u16>>,
    pub post: HashMap<(i32, i32), Vec<u16>>,
    pub oracle: HashMap<(i32, i32), Vec<u16>>,
}

pub const D6_BAND: i32 = 8;

pub fn in_band(lx: usize, lz: usize, left: bool, right: bool, back: bool, front: bool) -> bool {
    let lx = lx as i32;
    let lz = lz as i32;
    (left && lx < D6_BAND)
        || (right && (15 - lx) < D6_BAND)
        || (back && lz < D6_BAND)
        || (front && (15 - lz) < D6_BAND)
}

impl Generated {
    pub fn cell_count(&self) -> usize {
        self.height * 256
    }
}

fn oracle_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests-data")
        .join(name)
}

fn read_chunk(name: &str, file: &str) -> Vec<u8> {
    let path = oracle_dir(name).join(file);
    let compressed = std::fs::read(&path).unwrap_or_else(|e| panic!("read {file}: {e}"));
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).expect("inflate oracle bin");
    out
}

pub fn generate(name: &str, index_str: &str) -> Generated {
    let idx: Value = serde_json::from_str(index_str).expect("parse index.json");
    assert_eq!(idx["game_version"].as_str(), Some("26.3"));
    let seed = idx["seed"].as_i64().expect("seed");
    let min_y = idx["min_y"].as_i64().expect("min_y") as i32;
    let height = idx["height"].as_u64().expect("height") as usize;

    let chunks = idx["chunks"].as_array().expect("chunks");
    let targets: Vec<(i32, i32)> = chunks
        .iter()
        .map(|c| {
            (
                c["x"].as_i64().unwrap() as i32,
                c["z"].as_i64().unwrap() as i32,
            )
        })
        .collect();
    let files: HashMap<(i32, i32), String> = chunks
        .iter()
        .map(|c| {
            (
                (
                    c["x"].as_i64().unwrap() as i32,
                    c["z"].as_i64().unwrap() as i32,
                ),
                c["file"].as_str().unwrap().to_string(),
            )
        })
        .collect();

    let enabled: BTreeSet<String> = idx["enabled_placed_features"]
        .as_array()
        .expect("enabled_placed_features")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let generator = std::sync::Arc::new(
        WorldGenerator::with_options(seed, AquiferMode::Noise, true).expect("generator"),
    );
    let decoration = Decoration::embedded().expect("decoration");
    let mut world = TerrainFeatureWorld::new(generator);

    let target_set: HashSet<(i32, i32)> = targets.iter().copied().collect();
    let (min_cx, max_cx) = (
        targets.iter().map(|t| t.0).min().expect("targets"),
        targets.iter().map(|t| t.0).max().expect("targets"),
    );
    let (min_cz, max_cz) = (
        targets.iter().map(|t| t.1).min().expect("targets"),
        targets.iter().map(|t| t.1).max().expect("targets"),
    );
    let mut decorate_set: HashSet<(i32, i32)> = HashSet::new();
    for &(cx, cz) in &target_set {
        for dz in -1..=1 {
            for dx in -1..=1 {
                let (nx, nz) = (cx + dx, cz + dz);
                if (min_cx..=max_cx).contains(&nx) && (min_cz..=max_cz).contains(&nz) {
                    decorate_set.insert((nx, nz));
                }
            }
        }
    }
    let mut load_set: HashSet<(i32, i32)> = HashSet::new();
    for &(cx, cz) in &decorate_set {
        for dz in -1..=1 {
            for dx in -1..=1 {
                load_set.insert((cx + dx, cz + dz));
            }
        }
    }

    for &(cx, cz) in &load_set {
        world.load_chunk(cx, cz).expect("fill terrain");
    }

    let snapshot = |world: &TerrainFeatureWorld, cx: i32, cz: i32| -> Vec<u16> {
        let mut out = vec![0u16; height * 256];
        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                for j in 0..height {
                    let y = min_y + j as i32;
                    let oi = j * 256 + lz * 16 + lx;
                    out[oi] = world.block_id_at(cx, cz, lx, y, lz);
                }
            }
        }
        out
    };

    let mut pre = HashMap::new();
    for &(cx, cz) in &targets {
        pre.insert((cx, cz), snapshot(&world, cx, cz));
    }

    let mut order: Vec<(i32, i32)> = decorate_set.iter().copied().collect();
    order.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));

    let mut ctx = DecorationContext::new();
    for &(cx, cz) in &order {
        let mut region = world.region(cx, cz);
        decoration
            .decorate_chunk_filtered(&mut region, &mut ctx, seed, cx, cz, Some(&enabled))
            .unwrap_or_else(|e| panic!("decorate chunk ({cx},{cz}): {e}"));
    }

    let mut post = HashMap::new();
    let mut oracle = HashMap::new();
    for &(cx, cz) in &targets {
        post.insert((cx, cz), snapshot(&world, cx, cz));
        let bytes = read_chunk(name, &files[&(cx, cz)]);
        assert_eq!(bytes.len(), height * 256 * 2, "chunk byte count");
        let mut decoded = vec![0u16; height * 256];
        for (i, slot) in decoded.iter_mut().enumerate() {
            *slot = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        }
        oracle.insert((cx, cz), decoded);
    }

    Generated {
        min_y,
        height,
        targets,
        enabled,
        decorated: decorate_set,
        pre,
        post,
        oracle,
    }
}
