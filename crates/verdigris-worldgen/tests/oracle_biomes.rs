use std::io::Read;
use std::path::PathBuf;

use flate2::read::ZlibDecoder;
use serde_json::Value;

use verdigris_worldgen::data::WorldgenData;
use verdigris_worldgen::{BiomeTree, SearchState};

const _INDEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests-data/oracle-biomes/index.json"
));

fn oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests-data/oracle-biomes")
}

fn index() -> Value {
    serde_json::from_str(_INDEX).expect("parse index.json")
}

fn read_chunk(file: &str) -> Vec<u8> {
    let path = oracle_dir().join(file);
    let compressed = std::fs::read(&path).unwrap_or_else(|e| panic!("read {file}: {e}"));
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).expect("inflate oracle bin");
    out
}

#[test]
fn biomes_match_oracle() {
    let idx = index();
    assert_eq!(idx["game_version"].as_str(), Some("26.3"), "game_version");
    let seed = idx["seed"].as_i64().expect("seed");
    assert_eq!(seed, 20_261_029, "seed");
    let min_y = idx["min_y"].as_i64().expect("min_y") as i32;
    assert_eq!(min_y, -64);
    let height = idx["height"].as_u64().expect("height") as usize;
    assert_eq!(height, 384);
    let cell = idx["cell"].as_i64().expect("cell");
    assert_eq!(cell, 4);
    let cells = (height / 4) * 16;

    let chunks = idx["chunks"].as_array().expect("chunks array");
    assert_eq!(chunks.len(), 64, "64 chunks");

    let data = WorldgenData::embedded().expect("worldgen data");
    let source = data.build_biome_source(seed).expect("biome source");
    let tree = BiomeTree::embedded();

    let mut tie_cells = 0usize;
    let mut tie_divergent = 0usize;
    let mut tie_exact = 0usize;
    let mut unique_total = 0usize;
    let mut unique_mismatch = 0usize;
    let mut diagnostics: Vec<String> = Vec::new();

    let mut state = SearchState::new();
    let mut produced = vec![0u16; cells];

    for chunk in chunks {
        let cx = chunk["x"].as_i64().expect("x") as i32;
        let cz = chunk["z"].as_i64().expect("z") as i32;
        let file = chunk["file"].as_str().expect("file");
        let expected = read_chunk(file);
        assert_eq!(expected.len(), cells, "chunk {file} cell count");

        let targets = source.targets(cx, cz);
        assert_eq!(targets.len(), cells);
        source.fill_chunk(cx, cz, &mut state, &mut produced);

        for (i, target) in targets.iter().enumerate() {
            let want = expected[i] as u16;
            let got = produced[i];
            let (_dist, biomes) = tree.min_info(target);
            assert!(
                biomes.contains(&got),
                "chunk ({cx},{cz}) {file} cell {i}: production {got} hors ensemble minimal {biomes:?}"
            );
            if biomes.len() == 1 {
                unique_total += 1;
                if got != want {
                    unique_mismatch += 1;
                    if diagnostics.len() < 20 {
                        let qy = i / 16;
                        let rem = i % 16;
                        diagnostics.push(format!(
                            "chunk ({cx},{cz}) {file} cell {i} (qy={qy},qz={},qx={}): attendu {want} produit {got} climat {target:?}",
                            rem / 4,
                            rem % 4,
                        ));
                    }
                }
            } else {
                tie_cells += 1;
                if got == want {
                    tie_exact += 1;
                }
                if !biomes.contains(&want) {
                    tie_divergent += 1;
                    if diagnostics.len() < 20 {
                        diagnostics.push(format!(
                            "chunk ({cx},{cz}) {file} cell {i}: égalité, attendu {want} absent de {biomes:?}"
                        ));
                    }
                }
            }
        }
    }

    eprintln!(
        "porte oracle biomes (chemin de production fill_chunk/search): {unique_total} cellules à minimum unique, {unique_mismatch} écarts uniques, {tie_cells} cellules à égalité ({tie_exact} identiques à vanilla), {tie_divergent} égalités divergentes"
    );
    assert_eq!(
        unique_mismatch,
        0,
        "porte oracle biomes: {unique_mismatch} écart(s) sur minimum unique\n{}",
        diagnostics.join("\n")
    );
    assert_eq!(
        tie_divergent,
        0,
        "porte oracle biomes: {tie_divergent} égalité(s) où le biome vanilla n'est pas minimal\n{}",
        diagnostics.join("\n")
    );
}
