use crate::common;

use std::collections::HashMap;

const INDEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests-data/oracle-features-minerais/index.json"
));

#[test]
fn features_minerais_match_oracle() {
    let g = common::generate("oracle-features-minerais", INDEX);
    assert_eq!(g.targets.len(), 84, "84 chunks");

    let mut union_total = 0usize;
    let mut union_identical = 0usize;
    let mut out_of_band_mismatch = 0usize;
    let mut pairs: HashMap<(u16, u16), usize> = HashMap::new();
    let mut diagnostics: Vec<String> = Vec::new();

    for &(cx, cz) in &g.targets {
        let pre = &g.pre[&(cx, cz)];
        let post = &g.post[&(cx, cz)];
        let oracle = &g.oracle[&(cx, cz)];

        let left = g.decorated.contains(&(cx - 1, cz));
        let right = g.decorated.contains(&(cx + 1, cz));
        let back = g.decorated.contains(&(cx, cz - 1));
        let front = g.decorated.contains(&(cx, cz + 1));

        for oi in 0..g.cell_count() {
            let lz = (oi % 256) / 16;
            let lx = oi % 16;
            let banded = common::in_band(lx, lz, left, right, back, front);

            let vanilla_modified = oracle[oi] != pre[oi];
            let verdigris_modified = post[oi] != pre[oi];
            if vanilla_modified || verdigris_modified {
                union_total += 1;
                if post[oi] == oracle[oi] {
                    union_identical += 1;
                } else {
                    *pairs.entry((oracle[oi], post[oi])).or_insert(0) += 1;
                }
            }

            if !banded && post[oi] != oracle[oi] {
                out_of_band_mismatch += 1;
                if diagnostics.len() < 40 {
                    let y = g.min_y + (oi / 256) as i32;
                    diagnostics.push(format!(
                        "HORS BANDE chunk ({cx},{cz}) x={lx} y={y} z={lz}: attendu {} obtenu {}",
                        oracle[oi], post[oi]
                    ));
                }
            }
        }
    }

    let rate = if union_total == 0 {
        1.0
    } else {
        union_identical as f64 / union_total as f64
    };

    let mut pv: Vec<_> = pairs.into_iter().collect();
    pv.sort_by_key(|a| std::cmp::Reverse(a.1));
    pv.truncate(20);

    assert_eq!(
        out_of_band_mismatch,
        0,
        "porte oracle features minerais: {out_of_band_mismatch} écart(s) hors bande D6\n{}",
        diagnostics.join("\n")
    );
    assert!(
        rate >= 0.99,
        "porte oracle features minerais: taux {:.4} < 0.99 ({union_identical}/{union_total} identiques sur l'union des cases modifiées)\npaires (attendu,obtenu): {pv:?}",
        rate
    );
}

#[test]
fn band_leaves_interior_when_neighbor_undecorated() {
    assert!(!common::in_band(10, 10, true, false, true, false));
    assert!(common::in_band(2, 10, true, false, true, false));
    assert!(common::in_band(10, 2, true, false, true, false));
    assert!(common::in_band(14, 10, false, true, false, false));
    assert!(!common::in_band(10, 10, false, false, false, false));
}
