use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use flate2::read::ZlibDecoder;
use serde_json::Value;

use verdigris_worldgen::WorldGenerator;
use verdigris_worldgen::aquifer::AquiferMode;
use verdigris_worldgen::chunk_fill::CHUNK_WIDTH;

const INDEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests-data/oracle-carvers/index.json"
));

const BLOCKS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/blocks.json"
));

fn oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests-data/oracle-carvers")
}

fn read_chunk(file: &str) -> Vec<u8> {
    let path = oracle_dir().join(file);
    let compressed = std::fs::read(&path).unwrap_or_else(|e| panic!("read {file}: {e}"));
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).expect("inflate oracle bin");
    out
}

fn block_id_map() -> HashMap<String, u16> {
    let root: Value = serde_json::from_str(BLOCKS_JSON).expect("blocks.json");
    let blocks = root["blocks"].as_array().expect("blocks array");
    let mut m = HashMap::new();
    for b in blocks {
        let name = b["name"].as_str().expect("block name").to_string();
        let id = b["id"].as_u64().expect("block id") as u16;
        m.insert(name, id);
    }
    m
}

#[test]
fn carvers_match_oracle() {
    let idx: Value = serde_json::from_str(INDEX).expect("parse index.json");
    assert_eq!(idx["game_version"].as_str(), Some("26.3"));
    let seed = idx["seed"].as_i64().expect("seed");
    assert_eq!(seed, 20_261_029);
    let min_y = idx["min_y"].as_i64().expect("min_y") as i32;
    assert_eq!(min_y, -64);
    let height = idx["height"].as_u64().expect("height") as usize;
    assert_eq!(height, 384);

    let chunks = idx["chunks"].as_array().expect("chunks array");
    assert_eq!(chunks.len(), 48, "48 chunks");

    let ids = block_id_map();
    let generator =
        WorldGenerator::with_options(seed, AquiferMode::Noise, true).expect("generator");

    let table: Vec<u16> = generator
        .material()
        .blocks
        .iter()
        .map(|name| {
            *ids.get(name)
                .unwrap_or_else(|| panic!("block absent du registre: {name}"))
        })
        .collect();

    let mut terrain = generator.terrain();
    let mut total_mismatch = 0usize;
    let mut diagnostics: Vec<String> = Vec::new();

    for chunk in chunks {
        let cx = chunk["x"].as_i64().expect("x") as i32;
        let cz = chunk["z"].as_i64().expect("z") as i32;
        let file = chunk["file"].as_str().expect("file");
        let expected = read_chunk(file);
        assert_eq!(expected.len(), height * 256 * 2, "chunk {file} byte count");

        generator.fill_chunk(cx, cz, &mut terrain).expect("fill");

        let mut chunk_mismatch = 0usize;
        let mut pairs: HashMap<(u16, u16), usize> = HashMap::new();
        for y in min_y..(min_y + height as i32) {
            for lz in 0..CHUNK_WIDTH {
                for lx in 0..CHUNK_WIDTH {
                    let exp_index = ((y - min_y) as usize) * 256 + lz * 16 + lx;
                    let want =
                        u16::from_le_bytes([expected[exp_index * 2], expected[exp_index * 2 + 1]]);
                    let got = table[terrain.block_id_at(lx, y, lz) as usize];
                    if got != want {
                        chunk_mismatch += 1;
                        *pairs.entry((want, got)).or_insert(0) += 1;
                        if diagnostics.len() < 40 {
                            diagnostics.push(format!(
                                "chunk ({cx},{cz}) {file} x={lx} y={y} z={lz}: attendu id {want} obtenu id {got}"
                            ));
                        }
                    }
                }
            }
        }
        if chunk_mismatch > 0 {
            total_mismatch += chunk_mismatch;
            let mut pv: Vec<_> = pairs.into_iter().collect();
            pv.sort_by_key(|a| std::cmp::Reverse(a.1));
            diagnostics.push(format!(
                "chunk ({cx},{cz}) {file}: {chunk_mismatch} écarts, paires {pv:?}"
            ));
        }
    }

    assert_eq!(
        total_mismatch,
        0,
        "porte oracle carvers: {total_mismatch} écart(s) sur 48 chunks\n{}",
        diagnostics.join("\n")
    );
}
