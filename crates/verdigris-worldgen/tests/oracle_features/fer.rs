use crate::common;

const INDEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests-data/oracle-features-fer/index.json"
));

#[test]
fn features_fer_match_oracle() {
    let g = common::generate("oracle-features-fer", INDEX);
    assert_eq!(g.targets.len(), 84, "84 chunks");

    let mut total_mismatch = 0usize;
    let mut diagnostics: Vec<String> = Vec::new();

    for &(cx, cz) in &g.targets {
        let post = &g.post[&(cx, cz)];
        let oracle = &g.oracle[&(cx, cz)];
        let mut chunk_mismatch = 0usize;
        for oi in 0..g.cell_count() {
            if post[oi] != oracle[oi] {
                chunk_mismatch += 1;
                if diagnostics.len() < 40 {
                    let j = oi / 256;
                    let lz = (oi % 256) / 16;
                    let lx = oi % 16;
                    let y = g.min_y + j as i32;
                    diagnostics.push(format!(
                        "chunk ({cx},{cz}) x={lx} y={y} z={lz}: attendu id {} obtenu id {}",
                        oracle[oi], post[oi]
                    ));
                }
            }
        }
        total_mismatch += chunk_mismatch;
    }

    assert_eq!(
        total_mismatch,
        0,
        "porte oracle features fer: {total_mismatch} écart(s) sur 84 chunks\n{}",
        diagnostics.join("\n")
    );
}
