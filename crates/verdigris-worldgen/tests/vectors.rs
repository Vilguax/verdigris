use verdigris_worldgen::arith::{
    F32_FACTOR, F64_FACTOR, GOLDEN, SILVER, jabs_f32, jabs_f64, jclamp_f32, jclamp_f64, jmax_f32,
    jmax_f64, jmin_f32, jmin_f64, rotl, trunc32,
};
use verdigris_worldgen::hash::{hash_position, java_string_hash};
use verdigris_worldgen::lcg::Lcg;
use verdigris_worldgen::sources::{
    PositionalFactory, RootSource, aquifer_factory, clay_bands_source, end_outer_islands_source,
    nether_temperature_source, nether_vegetation_source, noise_source, positional_factory, root,
    terrain_noise_source,
};
use verdigris_worldgen::xoroshiro::{Xoroshiro, md5_words, mix13};

#[test]
fn factors_are_exact_powers_of_two() {
    assert_eq!(F64_FACTOR.to_bits(), 2.0_f64.powi(-53).to_bits());
    assert_eq!(F32_FACTOR.to_bits(), 2.0_f32.powi(-24).to_bits());
}

#[test]
fn mix13_vectors() {
    assert_eq!(mix13(0), 0);
    assert_eq!(mix13(1), 6_238_072_747_940_578_789);
}

#[test]
fn hash_position_vectors() {
    assert_eq!(hash_position(0, 0, 0), 0);
    assert_eq!(hash_position(0, 1, 0), 645);
    assert_eq!(hash_position(1, 0, 0), 133_076_631_897_947);
}

#[test]
fn java_string_hash_vectors() {
    assert_eq!(java_string_hash(""), 0);
    assert_eq!(java_string_hash("a"), 97);
    assert_eq!(java_string_hash("ab"), 3105);
    assert_eq!(
        java_string_hash("minecraft:offset"),
        "minecraft:offset"
            .chars()
            .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
    );
}

#[test]
fn lcg_matches_java_util_random_seed_zero() {
    let mut a = Lcg::from_seed(0);
    assert_eq!(a.next_int(), -1_155_484_576);
    assert_eq!(a.next_int(), -723_955_400);

    let mut b = Lcg::from_seed(0);
    assert_eq!(b.next_long(), -4_962_768_465_676_381_896);

    let mut c = Lcg::from_seed(0);
    assert_eq!(
        c.next_double().to_bits(),
        0.730_967_787_376_657_f64.to_bits()
    );

    let mut d = Lcg::from_seed(0);
    assert_eq!(d.next_float().to_bits(), 0.730_967_76_f32.to_bits());

    let mut e = Lcg::from_seed(0);
    assert!(e.next_boolean());
}

#[test]
fn lcg_next_int_bounded_power_of_two() {
    let mut r = Lcg::from_seed(123);
    let mut r2 = Lcg::from_seed(123);
    for _ in 0..64 {
        let v = r.next_int_bounded(16);
        assert!((0..16).contains(&v));
        assert_eq!(v, r2.next_int_bounded(16));
    }
}

#[test]
fn lcg_next_int_bounded_rejection_path() {
    let mut r = Lcg::from_seed(987);
    let mut r2 = Lcg::from_seed(987);
    for _ in 0..64 {
        let v = r.next_int_bounded(10);
        assert!((0..10).contains(&v));
        assert_eq!(v, r2.next_int_bounded(10));
    }
}

#[test]
fn xoroshiro_seed0_state() {
    let r = Xoroshiro::from_seed(0);
    let (s0, s1) = r.state();
    assert_eq!(s0 as i64, 3_847_398_142_028_685_078);
    assert_eq!(s1 as i64, 7_192_185_014_346_937_746);
}

#[test]
fn xoroshiro_seed0_next_long() {
    let mut r = Xoroshiro::from_seed(0);
    assert_eq!(r.next_long(), 3_038_984_756_725_240_190);
    assert_eq!(r.next_long(), -3_694_039_286_755_638_414);
    assert_eq!(r.next_long(), 4_633_751_808_701_151_732);
}

#[test]
fn xoroshiro_seed1_state() {
    let r = Xoroshiro::from_seed(1);
    let (s0, s1) = r.state();
    assert_eq!(s0 as i64, 5_272_463_233_947_570_727);
    assert_eq!(s1 as i64, 1_927_618_558_350_093_866);
}

#[test]
fn xoroshiro_seed1_next_long() {
    let mut r = Xoroshiro::from_seed(1);
    assert_eq!(r.next_long(), -1_033_667_707_219_518_978);
    assert_eq!(r.next_long(), 6_451_672_561_743_293_322);
    assert_eq!(r.next_long(), -1_821_890_263_888_393_630);
}

#[test]
fn xoroshiro_seed_promotion_matches_mix13() {
    for &seed in &[0_i64, 1, 12345, -42] {
        let lo = (seed as u64) ^ SILVER;
        let hi = lo.wrapping_add(GOLDEN);
        assert_eq!(Xoroshiro::from_seed(seed).state(), (mix13(lo), mix13(hi)));
    }
}

#[test]
fn md5_words_table() {
    let o = md5_words("minecraft:offset");
    assert_eq!(o.0 as i64, 577_895_406_318_539_652);
    assert_eq!(o.1 as i64, 4_557_074_653_038_767_061);

    let t = md5_words("minecraft:terrain");
    assert_eq!(t.0 as i64, 2_226_279_196_109_926_164);
    assert_eq!(t.1 as i64, -2_108_001_439_914_377_933);

    let c = md5_words("minecraft:continentalness");
    assert_eq!(c.0 as i64, -8_968_799_236_159_986_078);
    assert_eq!(c.1 as i64, -5_789_877_984_865_097_555);

    let g = md5_words("octave_-3");
    assert_eq!(g.0 as i64, 6_040_343_492_819_601_496);
    assert_eq!(g.1 as i64, -4_840_870_798_921_671_874);
}

#[test]
fn positional_p_and_offset_seed0() {
    let p = positional_factory(0, false);
    let PositionalFactory::Xoroshiro(xp) = &p else {
        unreachable!("overworld uses Xoroshiro");
    };
    let (plo, phi) = xp.params();
    assert_eq!(plo as i64, 3_038_984_756_725_240_190);
    assert_eq!(phi as i64, -3_694_039_286_755_638_414);

    let (s0, s1) = xp.from_hash_of("minecraft:offset").state();
    assert_eq!(s0 as i64, 2_461_705_687_090_922_234);
    assert_eq!(s1 as i64, -900_199_205_587_900_249);
}

#[test]
fn positional_p_and_offset_seed12345() {
    let p = positional_factory(12345, false);
    let PositionalFactory::Xoroshiro(xp) = &p else {
        unreachable!("overworld uses Xoroshiro");
    };
    let (plo, phi) = xp.params();
    assert_eq!(plo as i64, -8_118_485_274_630_516_485);
    assert_eq!(phi as i64, 8_241_557_746_459_281_790);

    let (s0, s1) = xp.from_hash_of("minecraft:offset").state();
    assert_eq!(s0 as i64, -8_696_380_067_554_868_353);
    assert_eq!(s1 as i64, 5_576_052_662_494_618_283);
}

#[test]
fn xoroshiro_from_words_zero_edge_case() {
    let mut a = Xoroshiro::from_words(0, 0);
    let mut b = Xoroshiro::from_words(GOLDEN, SILVER);
    for _ in 0..8 {
        assert_eq!(a.next_long(), b.next_long());
    }
}

#[test]
fn xoroshiro_sequence_is_deterministic() {
    let mut a = Xoroshiro::from_seed(7);
    let mut b = Xoroshiro::from_seed(7);
    for _ in 0..16 {
        assert_eq!(a.next_long(), b.next_long());
    }
}

#[test]
fn xoroshiro_next_int_bounded_in_range_and_deterministic() {
    let mut r = Xoroshiro::from_seed(55);
    let mut r2 = Xoroshiro::from_seed(55);
    for _ in 0..64 {
        let v = r.next_int_bounded(100);
        assert!((0..100).contains(&v));
        assert_eq!(v, r2.next_int_bounded(100));
    }
}

#[test]
fn xoroshiro_float_double_in_unit_range() {
    let mut r = Xoroshiro::from_seed(9);
    for _ in 0..64 {
        let f = r.next_float();
        assert!((0.0..1.0).contains(&f));
        let d = r.next_double();
        assert!((0.0..1.0).contains(&d));
    }
}

#[test]
fn lcg_positional_at_composition() {
    let seed = 12345_i64;
    let g = Lcg::from_seed(seed).next_long();
    let p = positional_factory(seed, true);
    let mut from_at = p.at(4, 5, 6);
    let mut expected = Lcg::from_seed(hash_position(4, 5, 6) ^ g);
    for _ in 0..8 {
        assert_eq!(from_at.next_long(), expected.next_long());
    }
}

#[test]
fn lcg_positional_from_seed_ignores_factory() {
    let p = positional_factory(777, true);
    let mut from_factory = p.from_seed(99);
    let mut direct = Lcg::from_seed(99);
    for _ in 0..8 {
        assert_eq!(from_factory.next_long(), direct.next_long());
    }
}

#[test]
fn xoroshiro_positional_at_and_from_seed_composition() {
    let seed = 12345_i64;
    let mut r = Xoroshiro::from_seed(seed);
    let plo = r.next_long() as u64;
    let phi = r.next_long() as u64;
    let p = positional_factory(seed, false);

    let mut from_at = p.at(1, 2, 3);
    let mut expected_at = Xoroshiro::from_words((hash_position(1, 2, 3) as u64) ^ plo, phi);
    for _ in 0..8 {
        assert_eq!(from_at.next_long(), expected_at.next_long());
    }

    let mut from_seed = p.from_seed(555);
    let mut expected_seed = Xoroshiro::from_words((555_u64) ^ plo, (555_u64) ^ phi);
    for _ in 0..8 {
        assert_eq!(from_seed.next_long(), expected_seed.next_long());
    }
}

#[test]
fn from_hash_of_is_deterministic_and_name_sensitive() {
    let p = positional_factory(42, false);
    let mut a = p.from_hash_of("minecraft:offset");
    let mut b = p.from_hash_of("minecraft:offset");
    let mut c = p.from_hash_of("minecraft:terrain");
    assert_eq!(a.next_long(), b.next_long());
    let mut a2 = p.from_hash_of("minecraft:offset");
    let mut c2 = p.from_hash_of("minecraft:terrain");
    assert_ne!(a2.next_long(), c.next_long());
    let _ = c2.next_long();
}

#[test]
fn root_and_positional_select_algorithm() {
    assert!(matches!(root(0, true), RootSource::Lcg(_)));
    assert!(matches!(root(0, false), RootSource::Xoroshiro(_)));
    assert!(matches!(
        positional_factory(0, true),
        PositionalFactory::Lcg(_)
    ));
    assert!(matches!(
        positional_factory(0, false),
        PositionalFactory::Xoroshiro(_)
    ));
}

#[test]
fn noise_source_matches_from_hash_of() {
    let p = positional_factory(123, false);
    let mut via_helper = noise_source(&p, "minecraft:continentalness");
    let mut direct = p.from_hash_of("minecraft:continentalness");
    for _ in 0..8 {
        assert_eq!(via_helper.next_long(), direct.next_long());
    }
}

#[test]
fn terrain_noise_source_legacy_vs_modern() {
    let p_legacy = positional_factory(10, true);
    let mut legacy = terrain_noise_source(&p_legacy, 10, true);
    let mut expected_legacy = Lcg::from_seed(10);
    for _ in 0..8 {
        assert_eq!(legacy.next_long(), expected_legacy.next_long());
    }

    let p_modern = positional_factory(10, false);
    let mut modern = terrain_noise_source(&p_modern, 10, false);
    let mut expected_modern = p_modern.from_hash_of("minecraft:terrain");
    for _ in 0..8 {
        assert_eq!(modern.next_long(), expected_modern.next_long());
    }
}

#[test]
fn aquifer_factory_forks_from_named_source() {
    let p = positional_factory(321, false);
    let af = aquifer_factory(&p);
    let mut src = p.from_hash_of("minecraft:aquifer");
    let expected = src.fork_positional();
    let mut a = af.at(0, 0, 0);
    let mut b = expected.at(0, 0, 0);
    for _ in 0..8 {
        assert_eq!(a.next_long(), b.next_long());
    }
}

#[test]
fn clay_bands_source_matches_named() {
    let p = positional_factory(7, false);
    let mut a = clay_bands_source(&p);
    let mut b = p.from_hash_of("minecraft:clay_bands");
    for _ in 0..8 {
        assert_eq!(a.next_long(), b.next_long());
    }
}

#[test]
fn nether_and_end_sources_use_offsets() {
    let seed = 9876_i64;
    let mut temp = nether_temperature_source(seed);
    let mut expected_temp = Lcg::from_seed(seed);
    assert_eq!(temp.next_long(), expected_temp.next_long());

    let mut veg = nether_vegetation_source(seed);
    let mut expected_veg = Lcg::from_seed(seed.wrapping_add(1));
    assert_eq!(veg.next_long(), expected_veg.next_long());

    let mut end = end_outer_islands_source(seed);
    let mut expected_end = Lcg::from_seed(seed);
    assert_eq!(end.next_long(), expected_end.next_long());
}

#[test]
fn jmin_jmax_java_semantics() {
    assert_eq!(jmin_f64(-0.0, 0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(jmin_f64(0.0, -0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(jmax_f64(-0.0, 0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(jmax_f64(0.0, -0.0).to_bits(), 0.0_f64.to_bits());
    assert!(jmin_f64(f64::NAN, 1.0).is_nan());
    assert!(jmax_f64(f64::NAN, 1.0).is_nan());
    assert_eq!(jmin_f64(2.0, 3.0).to_bits(), 2.0_f64.to_bits());
    assert_eq!(jmax_f64(2.0, 3.0).to_bits(), 3.0_f64.to_bits());

    assert_eq!(jmin_f32(-0.0, 0.0).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(jmax_f32(-0.0, 0.0).to_bits(), 0.0_f32.to_bits());
    assert!(jmin_f32(f32::NAN, 1.0).is_nan());
    assert!(jmax_f32(f32::NAN, 1.0).is_nan());
}

#[test]
fn jabs_and_jclamp_java_semantics() {
    assert_eq!(jabs_f64(0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(jabs_f64(-0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(jabs_f64(-5.0).to_bits(), 5.0_f64.to_bits());
    assert_eq!(jabs_f64(5.0).to_bits(), 5.0_f64.to_bits());
    assert!(jabs_f64(f64::NAN).is_nan());
    assert_eq!(jabs_f32(0.0).to_bits(), 0.0_f32.to_bits());
    assert_eq!(jabs_f32(-0.0).to_bits(), 0.0_f32.to_bits());

    assert_eq!(jclamp_f64(5.0, 0.0, 1.0).to_bits(), 1.0_f64.to_bits());
    assert_eq!(jclamp_f64(-1.0, 0.0, 1.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(jclamp_f64(0.5, 0.0, 1.0).to_bits(), 0.5_f64.to_bits());
    assert_eq!(jclamp_f64(-0.0, 0.0, 1.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(jclamp_f64(0.0, -1.0, 1.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(jclamp_f32(5.0, 0.0, 1.0).to_bits(), 1.0_f32.to_bits());
    assert_eq!(jclamp_f32(-1.0, 0.0, 1.0).to_bits(), 0.0_f32.to_bits());
    assert_eq!(jclamp_f32(-0.0, 0.0, 1.0).to_bits(), (-0.0_f32).to_bits());
}

#[test]
fn arith_helpers() {
    assert_eq!(rotl(1, 1), 2);
    assert_eq!(rotl(0x8000_0000_0000_0000, 1), 1);
    assert_eq!(trunc32(0xFFFF_FFFF_0000_0001), 1);
    assert_eq!(trunc32(0x0000_0000_FFFF_FFFF), -1);
}

#[test]
#[should_panic]
fn lcg_next_int_bounded_zero_panics() {
    Lcg::from_seed(0).next_int_bounded(0);
}

#[test]
#[should_panic]
fn lcg_next_int_bounded_negative_panics() {
    Lcg::from_seed(0).next_int_bounded(-5);
}

#[test]
#[should_panic]
fn xoroshiro_next_int_bounded_zero_panics() {
    Xoroshiro::from_seed(0).next_int_bounded(0);
}

#[test]
#[should_panic]
fn xoroshiro_next_int_bounded_negative_panics() {
    Xoroshiro::from_seed(0).next_int_bounded(-5);
}

#[test]
fn lcg_consume_advances_like_next_int() {
    let mut a = Lcg::from_seed(5);
    a.consume(3);
    let mut b = Lcg::from_seed(5);
    for _ in 0..3 {
        b.next_int();
    }
    assert_eq!(a.next_long(), b.next_long());

    let mut c = Lcg::from_seed(5);
    c.consume(0);
    let mut d = Lcg::from_seed(5);
    assert_eq!(c.next_long(), d.next_long());
}

#[test]
fn xoroshiro_consume_advances_like_next_int() {
    let mut a = Xoroshiro::from_seed(5);
    a.consume(3);
    let mut b = Xoroshiro::from_seed(5);
    for _ in 0..3 {
        b.next_int();
    }
    assert_eq!(a.next_long(), b.next_long());
}

#[test]
fn hash_position_extreme_coordinates() {
    assert_eq!(
        hash_position(i32::MIN, i32::MIN, i32::MIN),
        28_509_996_875_776
    );
    assert_eq!(
        hash_position(i32::MAX, i32::MAX, i32::MAX),
        9_811_763_908_338
    );
    assert_eq!(hash_position(i32::MIN, 0, i32::MAX), -86_341_287_861_592);
    assert_eq!(hash_position(0, i32::MIN, 0), 70_368_743_817_216);
    assert_eq!(
        hash_position(i32::MAX, i32::MIN, i32::MAX),
        71_483_002_586_549
    );
}

#[test]
fn java_string_hash_unicode_empty_and_long() {
    assert_eq!(java_string_hash(""), 0);
    assert_eq!(java_string_hash("é"), 233);
    assert_eq!(java_string_hash("caféñ"), 94_423_792);
    assert_eq!(java_string_hash("日本語"), 25_921_943);
    assert_eq!(
        java_string_hash(&"minecraft:offset".repeat(10)),
        -945_132_032
    );
}

#[test]
fn md5_words_unicode_and_long() {
    assert_eq!(md5_words("é").0 as i64, 7_412_306_613_632_936_882);
    assert_eq!(md5_words("é").1 as i64, -649_773_328_884_840_593);
    assert_eq!(md5_words("caféñ").0 as i64, -3_409_856_749_519_635_934);
    assert_eq!(md5_words("caféñ").1 as i64, 5_631_396_038_002_562_763);
    assert_eq!(md5_words("日本語").0 as i64, 4_797_137_895_898_867);
    assert_eq!(md5_words("日本語").1 as i64, -636_044_668_861_994_004);
    let long = "minecraft:offset".repeat(10);
    assert_eq!(md5_words(&long).0 as i64, -8_386_070_199_711_205_956);
    assert_eq!(md5_words(&long).1 as i64, -1_464_344_147_433_187_762);
}
