use std::io::Read;
use std::path::PathBuf;

use flate2::read::ZlibDecoder;
use serde_json::Value;

use verdigris_worldgen::{AquiferMode, Category, ChunkTerrain, WorldGenerator};

const CELL_AIR: u8 = 0;
const CELL_SOLID: u8 = 1;
const CELL_EXCLUDED: u8 = 2;
const CELL_WATER: u8 = 3;
const CELL_LAVA: u8 = 4;

fn oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests-data/oracle-1b")
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

fn expected_code(category: Category) -> u8 {
    match category {
        Category::Stone => CELL_SOLID,
        Category::Water => CELL_WATER,
        Category::Lava => CELL_LAVA,
        Category::Air => CELL_AIR,
    }
}

fn label(code: u8) -> &'static str {
    match code {
        CELL_AIR => "air",
        CELL_SOLID => "solide",
        CELL_EXCLUDED => "exclu",
        CELL_WATER => "eau",
        CELL_LAVA => "lave",
        _ => "?",
    }
}

struct Report {
    wrong: usize,
    pairs: Vec<(u8, u8, usize)>,
    first: Vec<(usize, i32, usize, u8, u8)>,
}

fn compare_chunk(terrain: &ChunkTerrain, cells: &[u8], min_y: i32, height: usize) -> Report {
    let mut wrong = 0usize;
    let mut pairs: Vec<(u8, u8, usize)> = Vec::new();
    let mut first = Vec::new();
    for lz in 0..16usize {
        for lx in 0..16usize {
            for j in 0..height {
                let y = min_y + j as i32;
                let expected = cells[(j * 256) + lz * 16 + lx];
                if expected == CELL_EXCLUDED {
                    continue;
                }
                let got = expected_code(terrain.category_at(lx, y, lz));
                if expected != got {
                    wrong += 1;
                    match pairs
                        .iter_mut()
                        .find(|(e, g, _)| *e == expected && *g == got)
                    {
                        Some((_, _, n)) => *n += 1,
                        None => pairs.push((expected, got, 1)),
                    }
                    if first.len() < 10 {
                        first.push((lx, y, lz, expected, got));
                    }
                }
            }
        }
    }
    Report {
        wrong,
        pairs,
        first,
    }
}

#[test]
fn aquifer_categories_match_oracle_1b() {
    let idx = index();
    assert_eq!(idx["format"].as_i64(), Some(2), "index format 2 exigé");
    assert_eq!(
        idx["game_version"].as_str(),
        Some("26.3"),
        "game_version 26.3 exigé"
    );
    let seed = idx["seed"].as_i64().expect("seed");
    assert_eq!(seed, 20_261_029, "seed 20261029 exigée");
    let min_y = idx["min_y"].as_i64().expect("min_y") as i32;
    assert_eq!(min_y, -64, "min_y -64 exigé");
    let height = idx["height"].as_u64().expect("height") as usize;
    assert_eq!(height, 384, "height 384 exigée");
    let area = height * 256;

    let chunks = idx["chunks"].as_array().expect("chunks array");
    assert_eq!(chunks.len(), 64, "64 chunks exigés");

    let generator = WorldGenerator::with_mode(seed, AquiferMode::Noise).expect("generator");
    assert_eq!(generator.min_y(), min_y, "min_y matches oracle");
    assert_eq!(generator.height(), height, "height matches oracle");

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
        let report = compare_chunk(&terrain, &cells, min_y, height);
        if report.wrong > 0 {
            let mut txt = format!("chunk ({cx},{cz}) {file}: {} cases fausses", report.wrong);
            for (e, g, n) in &report.pairs {
                txt.push_str(&format!(
                    "\n  attendu {} obtenu {} : {n}",
                    label(*e),
                    label(*g)
                ));
            }
            for (lx, y, lz, e, g) in &report.first {
                txt.push_str(&format!(
                    "\n    (lx={lx},y={y},lz={lz}) attendu {} obtenu {}",
                    label(*e),
                    label(*g)
                ));
            }
            failures.push(txt);
        }
    }

    assert!(
        failures.is_empty(),
        "porte oracle 1b: {} chunk(s) en écart sur {}\n{}",
        failures.len(),
        chunks.len(),
        failures.join("\n")
    );
}
