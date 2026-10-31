use verdigris_worldgen::biome::quantize::q;
use verdigris_worldgen::data::WorldgenData;
use verdigris_worldgen::eval_point;

fn named(key: &str, seed: i64) -> verdigris_worldgen::Node {
    WorldgenData::embedded()
        .expect("worldgen data")
        .build_named(key, seed)
        .expect("build density function")
        .0
}

fn bits(v: f32) -> u32 {
    v.to_bits()
}

fn f32_of(v: f64) -> u32 {
    (v as f32).to_bits()
}

#[test]
fn shift_vectors_seed0_bit_exact() {
    let shift_x = named("minecraft:shift_x", 0);
    let shift_z = named("minecraft:shift_z", 0);
    assert_eq!(
        bits(eval_point(&shift_x, 0, 0, 0)),
        f32_of(-2.174344539642334)
    );
    assert_eq!(
        bits(eval_point(&shift_z, 0, 0, 0)),
        f32_of(-2.174344539642334)
    );
    assert_eq!(
        bits(eval_point(&shift_x, 100, 0, -200)),
        f32_of(0.029261112213134766)
    );
    assert_eq!(
        bits(eval_point(&shift_z, 100, 0, -200)),
        f32_of(-0.86368727684021)
    );
}

#[test]
fn shift_vectors_seed12345_bit_exact() {
    let shift_x = named("minecraft:shift_x", 12345);
    let shift_z = named("minecraft:shift_z", 12345);
    assert_eq!(
        bits(eval_point(&shift_x, 0, 0, 0)),
        f32_of(-2.1486198902130127)
    );
    assert_eq!(
        bits(eval_point(&shift_z, 0, 0, 0)),
        f32_of(-2.1486198902130127)
    );
    assert_eq!(
        bits(eval_point(&shift_x, 100, 0, -200)),
        f32_of(-0.0824139341711998)
    );
    assert_eq!(
        bits(eval_point(&shift_z, 100, 0, -200)),
        f32_of(1.8404057025909424)
    );
}

#[test]
fn continents_erosion_seed0_bit_exact() {
    let continents = named("minecraft:overworld/continents", 0);
    let erosion = named("minecraft:overworld/erosion", 0);
    assert_eq!(
        bits(eval_point(&continents, 0, 0, 0)),
        f32_of(-0.008171876892447472)
    );
    assert_eq!(
        bits(eval_point(&erosion, 0, 0, 0)),
        f32_of(-0.1039106473326683)
    );
    assert_eq!(
        bits(eval_point(&continents, 100, 0, -200)),
        f32_of(-0.030818600207567215)
    );
    assert_eq!(
        bits(eval_point(&erosion, 100, 0, -200)),
        f32_of(0.020620547235012054)
    );
    assert_eq!(
        bits(eval_point(&continents, -1000, 0, 5000)),
        f32_of(0.31970715522766113)
    );
    assert_eq!(
        bits(eval_point(&erosion, -1000, 0, 5000)),
        f32_of(0.5192112326622009)
    );
}

#[test]
fn climate_vectors_quantize_to_expected_integers() {
    let continents = named("minecraft:overworld/continents", 0);
    let erosion = named("minecraft:overworld/erosion", 0);
    assert_eq!(q(eval_point(&continents, 0, 0, 0)), -81);
    assert_eq!(q(eval_point(&erosion, 0, 0, 0)), -1039);
    assert_eq!(q(eval_point(&continents, 100, 0, -200)), -308);
    assert_eq!(q(eval_point(&erosion, 100, 0, -200)), 206);
    assert_eq!(q(eval_point(&continents, -1000, 0, 5000)), 3197);
    assert_eq!(q(eval_point(&erosion, -1000, 0, 5000)), 5192);
}
