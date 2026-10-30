use std::io::Read;
use std::path::PathBuf;

use flate2::read::ZlibDecoder;
use serde_json::Value;

use verdigris_worldgen::{AquiferMode, ChunkTerrain, WorldGenerator};

const CELL_SOLID: u8 = 1;
const CELL_EXCLUDED: u8 = 2;

fn oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests-data/oracle-1a")
}

fn index() -> Value {
    let path = oracle_dir().join("index.json");
    let text = std::fs::read_to_string(&path).expect("read index.json");
    serde_json::from_str(&text).expect("parse index.json")
}

fn read_chunk(file: &str) -> Vec<u8> {
    let path = oracle_dir().join(file);
    let compressed = std::fs::read(&path).expect("read oracle bin");
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).expect("inflate oracle bin");
    out
}

struct Mismatch {
    lx: usize,
    y: i32,
    lz: usize,
    expected_solid: bool,
    got_solid: bool,
    density: f32,
}

fn compare_chunk(
    terrain: &ChunkTerrain,
    cells: &[u8],
    min_y: i32,
    height: usize,
) -> (usize, Vec<Mismatch>) {
    let mut wrong = 0usize;
    let mut first = Vec::new();
    for lz in 0..16usize {
        for lx in 0..16usize {
            for j in 0..height {
                let y = min_y + j as i32;
                let cell = cells[(j * 256) + lz * 16 + lx];
                if cell == CELL_EXCLUDED {
                    continue;
                }
                let expected_solid = cell == CELL_SOLID;
                let got_solid = terrain.category_at(lx, y, lz).is_solid();
                if expected_solid != got_solid {
                    wrong += 1;
                    if first.len() < 10 {
                        first.push(Mismatch {
                            lx,
                            y,
                            lz,
                            expected_solid,
                            got_solid,
                            density: terrain.density_at(lx, y, lz),
                        });
                    }
                }
            }
        }
    }
    (wrong, first)
}

#[test]
fn final_fill_matches_oracle_on_all_chunks() {
    let idx = index();
    let seed = idx["seed"].as_i64().expect("seed");
    let min_y = idx["min_y"].as_i64().expect("min_y") as i32;
    let height = idx["height"].as_u64().expect("height") as usize;
    let area = height * 256;

    let generator = WorldGenerator::with_mode(seed, AquiferMode::Disabled).expect("generator");
    assert_eq!(generator.min_y(), min_y, "min_y matches oracle");
    assert_eq!(generator.height(), height, "height matches oracle");

    let chunks = idx["chunks"].as_array().expect("chunks array");
    assert!(!chunks.is_empty(), "oracle index lists chunks");

    let mut terrain = generator.terrain();
    let mut failures = Vec::new();

    for chunk in chunks {
        let cx = chunk["x"].as_i64().expect("chunk x") as i32;
        let cz = chunk["z"].as_i64().expect("chunk z") as i32;
        let file = chunk["file"].as_str().expect("chunk file");
        let cells = read_chunk(file);
        assert_eq!(cells.len(), area, "chunk {file} has {area} cells");

        generator
            .fill_chunk(cx, cz, &mut terrain)
            .expect("fill_chunk");
        let (wrong, first) = compare_chunk(&terrain, &cells, min_y, height);
        if wrong > 0 {
            let mut report = format!("chunk ({cx},{cz}) {file}: {wrong} cases fausses");
            for m in &first {
                report.push_str(&format!(
                    "\n  (lx={},y={},lz={}) attendu solide={} obtenu solide={} densite={:?} bits={:#010x}",
                    m.lx,
                    m.y,
                    m.lz,
                    m.expected_solid,
                    m.got_solid,
                    m.density,
                    m.density.to_bits()
                ));
            }
            failures.push(report);
        }
    }

    assert!(
        failures.is_empty(),
        "porte oracle: {} chunk(s) en écart sur {}\n{}",
        failures.len(),
        chunks.len(),
        failures.join("\n")
    );
}
