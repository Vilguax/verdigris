use std::collections::BTreeSet;

use verdigris_worldgen::feature::features::run_scattered_ore;
use verdigris_worldgen::feature::{FeatureRng, OreConfig, Region, TestRegion, ore_metrics};

#[test]
fn generator_scalar_vectors_11_2() {
    let mut r = FeatureRng::new(0);
    assert_eq!(r.next_bits(32), 707568776);
    assert_eq!(r.next_bits(32), -860085545);
    assert_eq!(r.next_bits(32), 1078879416);

    let mut r = FeatureRng::new(0);
    assert_eq!(r.next_long(), 3038984751730664151);
    assert_eq!(f64::from(r.next_float()), 0.25119614601135254);
    assert_eq!(r.next_double(), 0.11712488680649513);

    let mut r = FeatureRng::new(0);
    assert_eq!(f64::from(r.next_float()), 0.16474366188049316);

    let mut r = FeatureRng::new(0);
    assert_eq!(r.next_double(), 0.1647437035999555);
}

#[test]
fn population_seeds_11_2() {
    let cases: [(i64, i32, i32, i64); 6] = [
        (0, 0, 0, 0),
        (0, 1, -2, -7422581477381670832),
        (20261029, 0, 0, 20261029),
        (20261029, 1, -2, 6279314215104636341),
        (20261029, -7, 13, -5162208560712788027),
        (12345, 100, -100, 7751208020290508985),
    ];
    for (s, cx, cz, d) in cases {
        let mut r = FeatureRng::new(0);
        let got = r.set_decoration_seed(s, cx.wrapping_mul(16), cz.wrapping_mul(16));
        assert_eq!(got, d, "S={s} chunk=({cx},{cz})");
    }
}

#[test]
fn feature_seeds_11_2() {
    let d = 6279314215104636341i64;
    let cases: [(i32, i32, i64, [i32; 4]); 3] = [
        (6, 9, 6279314215104696350, [0, 9, 7, 1]),
        (9, 0, 6279314215104726341, [6, 3, 2, 15]),
        (10, 0, 6279314215104736341, [9, 4, 7, 8]),
    ];
    for (k, i, seed, draws) in cases {
        assert_eq!(
            d.wrapping_add(i64::from(i))
                .wrapping_add(10000i64.wrapping_mul(i64::from(k))),
            seed
        );
        let mut r = FeatureRng::new(0);
        r.set_feature_seed(d, i, k);
        for expected in draws {
            assert_eq!(r.next_int_bounded(16), expected, "step {k} index {i}");
        }
    }
}

#[test]
fn ore_metrics_11_1() {
    assert_eq!(ore_metrics(9), (1.125, 2, 2, 8, 8));
    assert_eq!(ore_metrics(17), (2.125, 2, 3, 10, 8));
    assert_eq!(ore_metrics(64), (8.0, 5, 8, 26, 14));
}

#[test]
fn disk_radius_two_is_thirteen_columns_11_1() {
    let r = 2i32;
    let mut count = 0;
    for z in -r..=r {
        for x in -r..=r {
            if x * x + z * z <= r * r {
                count += 1;
            }
        }
    }
    assert_eq!(count, 13);
}

fn java_round(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

#[test]
fn scattered_ore_draw_order_and_offsets_11() {
    let cfg = OreConfig::compile(&serde_json::json!({
        "size": 64,
        "discard_chance_on_air_exposure": 0,
        "targets": [{
            "state": "minecraft:iron_ore",
            "target": {"predicate_type": "minecraft:always_true"}
        }]
    }))
    .unwrap();

    let (px, py, pz) = (8, 60, 8);
    let mut scattered_total = 0usize;

    for seed in [1i64, 7, 42, 99, 20261029] {
        let mut replay = FeatureRng::new(seed);
        let count = replay.next_int_bounded(64 + 1);
        let mut expected: BTreeSet<(i32, i32, i32)> = BTreeSet::new();
        for i in 0..count {
            let m = i.min(7) as f32;
            let dx = java_round((replay.next_float() - replay.next_float()) * m);
            let dy = java_round((replay.next_float() - replay.next_float()) * m);
            let dz = java_round((replay.next_float() - replay.next_float()) * m);
            expected.insert((px + dx, py + dy, pz + dz));
        }

        let mut region = TestRegion::new((0, 0));
        let mut rng = FeatureRng::new(seed);
        assert!(run_scattered_ore(&cfg, &mut region, &mut rng, px, py, pz).unwrap());

        let air = region.read_block(px, py + 40, pz).unwrap();
        let mut changed: BTreeSet<(i32, i32, i32)> = BTreeSet::new();
        for x in px - 8..=px + 8 {
            for y in py - 8..=py + 8 {
                for z in pz - 8..=pz + 8 {
                    if region.read_block(x, y, z).unwrap() != air {
                        changed.insert((x, y, z));
                    }
                }
            }
        }

        assert_eq!(
            changed, expected,
            "scattered_ore seed {seed}: positions doivent suivre l'ordre de tirage §9.3"
        );
        scattered_total += expected.len();
    }

    assert!(
        scattered_total > 0,
        "au moins une graine doit disperser un point (n = next_int(size+1))"
    );
}
