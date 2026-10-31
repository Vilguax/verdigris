use std::f32::consts::TAU;

use verdigris_worldgen::SearchState;
use verdigris_worldgen::carver::canyon::{carve_canyon, init_width_factors};
use verdigris_worldgen::carver::cave::carve_cave;
use verdigris_worldgen::carver::config::{CanyonConfig, CarverKind, CaveConfig};
use verdigris_worldgen::carver::mask::{CARVE_GEN_MIN_Y, CARVE_GEN_TOP_Y, CarveMask};
use verdigris_worldgen::carver::rng::{is_start, seed_large_feature};
use verdigris_worldgen::carver::{CarverData, carve_chunk_mask};
use verdigris_worldgen::data::WorldgenData;
use verdigris_worldgen::lcg::Lcg;
use verdigris_worldgen::registry::worldgen_value;

fn carver_data() -> CarverData {
    CarverData::from_worldgen(worldgen_value()).expect("carver data")
}

fn vanilla_configs() -> (CaveConfig, CaveConfig, CanyonConfig) {
    let data = carver_data();
    let mut cave = None;
    let mut cave_extra = None;
    let mut canyon = None;
    for i in 0..data.configs().len() {
        match (&data.configs()[i], data.config_name(i)) {
            (CarverKind::Cave(c), "minecraft:cave") => cave = Some(c.clone()),
            (CarverKind::Cave(c), "minecraft:cave_extra_underground") => {
                cave_extra = Some(c.clone())
            }
            (CarverKind::Canyon(c), "minecraft:canyon") => canyon = Some(c.clone()),
            _ => {}
        }
    }
    (
        cave.expect("cave"),
        cave_extra.expect("cave_extra_underground"),
        canyon.expect("canyon"),
    )
}

fn reseed_parts(s: i64, sx: i32, sz: i32, i: i64) -> (i64, i64, i64) {
    let mut r = Lcg::from_seed(s + i);
    let a = r.next_long();
    let b = r.next_long();
    let g = (i64::from(sx).wrapping_mul(a)) ^ (i64::from(sz).wrapping_mul(b)) ^ (s + i);
    (a, b, g)
}

type ReseedRow = (i64, i32, i32, i64, Option<i64>, Option<i64>, i64, f32);

fn first_nf(s: i64, sx: i32, sz: i32, i: i64) -> f32 {
    let mut r = seed_large_feature(s + i, sx, sz);
    r.next_float()
}

#[test]
fn reseed_table_8_2() {
    let rows: &[ReseedRow] = &[
        (
            0,
            0,
            0,
            0,
            Some(-4962768465676381896),
            Some(4437113781045784766),
            0,
            0.730_967_76,
        ),
        (
            0,
            0,
            0,
            1,
            Some(-4964420948893066024),
            Some(7564655870752979346),
            1,
            0.730_878_2,
        ),
        (
            0,
            0,
            0,
            2,
            Some(-4959463499243013640),
            Some(-1817970389778669801),
            2,
            0.731_146_93,
        ),
        (0, 1, 0, 1, None, None, -4964420948893066023, 0.017_425_24),
        (0, 0, 1, 0, None, None, 4437113781045784766, 0.837_778_4),
        (0, -1, -1, 0, None, None, -8740502649270427254, 0.793_393_6),
        (
            20261029,
            0,
            0,
            0,
            Some(-7952776518890286697),
            Some(-9142844432472299689),
            20261029,
            0.568_879_07,
        ),
        (
            20261029,
            0,
            0,
            1,
            Some(-7947819064945267017),
            Some(-78726623589364516),
            20261030,
            0.569_147_8,
        ),
        (
            20261029,
            0,
            0,
            2,
            Some(-7949471552456918441),
            Some(3048815470412797360),
            20261031,
            0.569_058_24,
        ),
        (
            20261029,
            0,
            1,
            0,
            None,
            None,
            -9142844432488230926,
            0.012_484_848_5,
        ),
        (
            20261029,
            -1,
            -1,
            2,
            None,
            None,
            -4908259240220178594,
            0.270_167_4,
        ),
        (
            20261029,
            -10,
            -5,
            0,
            None,
            None,
            3873917905843700722,
            0.012_355_506,
        ),
        (
            20261029,
            -10,
            -5,
            1,
            None,
            None,
            5444067385354992584,
            0.070_008_46,
        ),
        (
            20261029,
            -10,
            -5,
            2,
            None,
            None,
            7153931709669311405,
            0.010_839_522,
        ),
    ];
    for &(s, sx, sz, i, a, b, g, nf) in rows {
        let (ga, gb, gg) = reseed_parts(s, sx, sz, i);
        if let Some(a) = a {
            assert_eq!(ga, a, "a for S={s} ({sx},{sz}) i={i}");
        }
        if let Some(b) = b {
            assert_eq!(gb, b, "b for S={s} ({sx},{sz}) i={i}");
        }
        assert_eq!(gg, g, "g for S={s} ({sx},{sz}) i={i}");
        assert_eq!(
            first_nf(s, sx, sz, i).to_bits(),
            nf.to_bits(),
            "nf for S={s} ({sx},{sz}) i={i}"
        );
    }
}

#[test]
fn cave_start_draws_8_2() {
    let (cave, _, _) = vanilla_configs();
    let mut r = seed_large_feature(20261029, -10, -5);
    assert!(is_start(&mut r, cave.probability));

    let n = cave.count.sample(&mut r);
    assert_eq!(n, 7, "count N");

    let x = 16i32.wrapping_mul(-10) + r.next_int_bounded(16);
    assert_eq!(x, -147);
    let y = cave.y.sample(&mut r, CARVE_GEN_MIN_Y, CARVE_GEN_TOP_Y);
    assert_eq!(y, 19);
    let z = 16i32.wrapping_mul(-5) + r.next_int_bounded(16);
    assert_eq!(z, -72);
    let m_h = cave.horizontal_radius_multiplier.sample(&mut r);
    assert_eq!(m_h.to_bits(), 1.288_817_2_f32.to_bits());
    let m_v = cave.vertical_radius_multiplier.sample(&mut r);
    assert_eq!(m_v.to_bits(), 0.800_307_04_f32.to_bits());
    let p = cave.floor_level.sample(&mut r);
    assert_eq!(p.to_bits(), (-0.939_928_8_f32).to_bits());

    assert_ne!(r.next_int_bounded(4), 0, "no room for system 1");

    let theta = r.next_float() * TAU;
    assert_eq!(theta.to_bits(), 4.439_266_f32.to_bits());
    let phi = (r.next_float() - 0.5) / 4.0;
    assert_eq!(phi.to_bits(), (-0.076_191_04_f32).to_bits());
    let mut t = cave.thickness.sample(&mut r);
    if cave.weird_thickness_bias && r.next_int_bounded(10) == 0 {
        t *= r.next_float() * r.next_float() * 3.0 + 1.0;
    }
    assert_eq!(t.to_bits(), 1.849_765_2_f32.to_bits());
    let l = 112 - r.next_int_bounded(28);
    assert_eq!(l, 108);
    let seed = r.next_long();
    assert_eq!(seed, 4302709288594276504);
}

#[test]
fn canyon_start_draws_8_2() {
    let (_, _, canyon) = vanilla_configs();
    let mut probe = seed_large_feature(20261029 + 2, -5, -8);
    assert_eq!(probe.next_float().to_bits(), 0.002_673_804_8_f32.to_bits());

    let mut r = seed_large_feature(20261029 + 2, -5, -8);
    assert!(is_start(&mut r, canyon.probability));
    let x = 16i32.wrapping_mul(-5) + r.next_int_bounded(16);
    assert_eq!(x, -66);
    let y = canyon.y.sample(&mut r, CARVE_GEN_MIN_Y, CARVE_GEN_TOP_Y);
    assert_eq!(y, 56);
    let z = 16i32.wrapping_mul(-8) + r.next_int_bounded(16);
    assert_eq!(z, -116);
    let theta = r.next_float() * TAU;
    assert_eq!(theta.to_bits(), 3.423_227_5_f32.to_bits());
    let phi = canyon.vertical_rotation.sample(&mut r);
    assert_eq!(phi.to_bits(), 0.018_689_081_f32.to_bits());
    let t = canyon.thickness.sample(&mut r);
    assert_eq!(t.to_bits(), 4.981_202_6_f32.to_bits());
    let df = canyon.distance_factor.sample(&mut r);
    assert_eq!(df.to_bits(), 0.774_869_86_f32.to_bits());
    let l = (112.0f32 * df) as i32;
    assert_eq!(l, 86);
    let seed = r.next_long();
    assert_eq!(seed, -8188528839801648741);

    let mut rr = Lcg::from_seed(seed);
    let fs = init_width_factors(&canyon, &mut rr);
    assert_eq!(fs[0].to_bits(), 1.716_246_f32.to_bits());
    assert_eq!(fs[1].to_bits(), 1.716_246_f32.to_bits());
    assert_eq!(fs[2].to_bits(), 1.716_246_f32.to_bits());
    assert_eq!(fs[3].to_bits(), 1.771_174_4_f32.to_bits());
}

#[test]
fn single_cave_marks_111_in_target_8_2() {
    let (cave, _, _) = vanilla_configs();
    let mut r = seed_large_feature(20261029, -10, -5);
    assert!(is_start(&mut r, cave.probability));
    let mut mask = CarveMask::new();
    carve_cave(&mut mask, &cave, &mut r, -10, -5, -8, -8);
    assert_eq!(mask.count(), 111);
}

#[test]
fn single_canyon_marks_1403_in_target_8_2() {
    let (_, _, canyon) = vanilla_configs();
    let mut r = seed_large_feature(20261029 + 2, -5, -8);
    assert!(is_start(&mut r, canyon.probability));
    let mut mask = CarveMask::new();
    carve_canyon(&mut mask, &canyon, &mut r, -5, -8, -6, -8);
    assert_eq!(mask.count(), 1403);
}

#[test]
fn full_chunk_masks_8_2() {
    let data = carver_data();
    let wg = WorldgenData::embedded().expect("data");
    let mat = wg.build_material_rules(20261029).expect("material");
    let mut st = SearchState::new();
    assert_eq!(
        carve_chunk_mask(&data, &mat.biome_source, &mut st, 20261029, -8, -8).count(),
        1122,
        "chunk (-8,-8)"
    );
    assert_eq!(
        carve_chunk_mask(&data, &mat.biome_source, &mut st, 20261029, -6, -8).count(),
        3636,
        "chunk (-6,-8)"
    );
    assert_eq!(
        carve_chunk_mask(&data, &mat.biome_source, &mut st, 20261029, 7, 5).count(),
        3237,
        "chunk (7,5)"
    );
}
