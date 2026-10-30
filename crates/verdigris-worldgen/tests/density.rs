use serde_json::Value;
use verdigris_worldgen::data::WorldgenData;
use verdigris_worldgen::df::axes::axes;
use verdigris_worldgen::df::eval::eval_point;
use verdigris_worldgen::df::node::Node;

const SEED: i64 = 20261029;

fn data() -> WorldgenData {
    WorldgenData::embedded().expect("embedded worldgen data")
}

fn build(json: &str) -> Node {
    let d = data();
    let assets = d.assets(0).expect("assets");
    let value: Value = serde_json::from_str(json).expect("json");
    d.build_node(&value, &assets).expect("build").0
}

fn build_err(json: &str) -> bool {
    let d = data();
    let assets = d.assets(0).expect("assets");
    let value: Value = serde_json::from_str(json).expect("json");
    d.build_node(&value, &assets).is_err()
}

fn named(key: &str, seed: i64) -> Node {
    data().build_named(key, seed).expect("named build").0
}

fn bits(v: f32) -> u32 {
    v.to_bits()
}

fn f32_of(v: f64) -> u32 {
    (v as f32).to_bits()
}

#[test]
fn gradient_minecraft_y_is_identity() {
    let node = named("minecraft:y", 0);
    assert_eq!(bits(eval_point(&node, 0, 0, 0)), 0.0_f32.to_bits());
    assert_eq!(bits(eval_point(&node, 0, 100, 0)), 100.0_f32.to_bits());
    assert_eq!(bits(eval_point(&node, 0, -50, 0)), (-50.0_f32).to_bits());
}

#[test]
fn gradient_depth_hand_values() {
    let node = build(
        r#"{"type":"minecraft:gradient","axis":"y","from_coordinate":-64,"from_value":1.5,"to_coordinate":320,"to_value":-1.5}"#,
    );
    assert_eq!(bits(eval_point(&node, 0, 0, 0)), 1.0_f32.to_bits());
    assert_eq!(bits(eval_point(&node, 0, 100, 0)), 0.21875_f32.to_bits());
}

#[test]
fn gradient_caves_entrances_hand_values() {
    let node = build(
        r#"{"type":"minecraft:gradient","axis":"y","from_coordinate":-10,"from_value":0.3,"to_coordinate":30,"to_value":0.0}"#,
    );
    assert_eq!(
        bits(eval_point(&node, 0, 0, 0)),
        f32_of(0.22500000894069672)
    );
}

#[test]
fn squeeze_hand_values() {
    let one = build(r#"{"type":"minecraft:squeeze","input":1.0}"#);
    let neg = build(r#"{"type":"minecraft:squeeze","input":-1.0}"#);
    let two = build(r#"{"type":"minecraft:squeeze","input":2.0}"#);
    let half = build(r#"{"type":"minecraft:squeeze","input":0.5}"#);
    assert_eq!(bits(eval_point(&one, 0, 0, 0)), f32_of(0.4583333432674408));
    assert_eq!(bits(eval_point(&neg, 0, 0, 0)), f32_of(-0.4583333432674408));
    assert_eq!(bits(eval_point(&two, 0, 0, 0)), f32_of(0.4583333432674408));
    assert_eq!(bits(eval_point(&half, 0, 0, 0)), f32_of(0.2447916716337204));
}

#[test]
fn half_and_quarter_negative_hand_values() {
    let hn = build(r#"{"type":"minecraft:half_negative","input":-1.0}"#);
    let qn = build(r#"{"type":"minecraft:quarter_negative","input":-2.0}"#);
    assert_eq!(bits(eval_point(&hn, 0, 0, 0)), (-0.5_f32).to_bits());
    assert_eq!(bits(eval_point(&qn, 0, 0, 0)), (-0.5_f32).to_bits());
}

#[test]
fn lerp_alpha_one_returns_second_exactly() {
    let node = build(r#"{"type":"minecraft:lerp","alpha":1.0,"first":0.3,"second":0.1}"#);
    assert_eq!(bits(eval_point(&node, 0, 0, 0)), 0.1_f32.to_bits());
    assert_ne!(bits(eval_point(&node, 0, 0, 0)), f32_of(0.099999994));
}

fn two_point_spline(u: f64) -> f32 {
    let json = format!(
        r#"{{"type":"minecraft:spline","spline":{{"coordinate":{u},"points":[{{"location":-1.0,"value":0.0,"derivative":0.0}},{{"location":1.0,"value":1.0,"derivative":0.0}}]}}}}"#
    );
    eval_point(&build(&json), 0, 0, 0)
}

#[test]
fn spline_two_points_hand_values() {
    assert_eq!(bits(two_point_spline(0.0)), 0.5_f32.to_bits());
    assert_eq!(bits(two_point_spline(0.5)), 0.84375_f32.to_bits());
    assert_eq!(bits(two_point_spline(2.0)), 1.0_f32.to_bits());
    assert_eq!(bits(two_point_spline(-5.0)), 0.0_f32.to_bits());
}

fn ridges_folded_spline(u: f64) -> f32 {
    let json = format!(
        r#"{{"type":"minecraft:spline","spline":{{"coordinate":{u},"points":[{{"location":-1.0,"value":-0.08880186,"derivative":0.38940096}},{{"location":1.0,"value":0.69000006,"derivative":0.38940096}}]}}}}"#
    );
    eval_point(&build(&json), 0, 0, 0)
}

#[test]
fn spline_ridges_folded_hand_values() {
    assert_eq!(bits(ridges_folded_spline(0.0)), f32_of(0.3005990982055664));
    assert_eq!(bits(ridges_folded_spline(0.3)), f32_of(0.4174193739891052));
}

fn assert_interval(key: &str, lo: f64, hi: f64) {
    let node = named(key, SEED);
    let iv = node.interval();
    assert_eq!(iv.min.to_bits(), f32_of(lo), "{key} min");
    assert_eq!(iv.max.to_bits(), f32_of(hi), "{key} max");
}

#[test]
fn computed_intervals_match_validation_list() {
    assert_interval(
        "minecraft:overworld/continents",
        -2.165463447570801,
        2.165463447570801,
    );
    assert_interval(
        "minecraft:overworld/ridges_folded",
        -3.2723326683044434,
        1.0,
    );
    assert_interval("minecraft:overworld/factor", 0.625, 10.0);
    assert_interval("minecraft:overworld/jaggedness", 0.0, 0.6299999952316284);
    assert_interval(
        "minecraft:overworld/base_3d_noise",
        -2.167062282562256,
        2.167062282562256,
    );
    assert_interval(
        "minecraft:overworld/caves/entrances",
        -0.504351019859314,
        1.0687991380691528,
    );
    assert_interval(
        "minecraft:overworld/caves/noodle",
        -0.12276944518089294,
        64.0,
    );
    assert_interval(
        "minecraft:overworld/caves/pillars",
        -27.52654457092285,
        19.32028579711914,
    );
    assert_interval(
        "minecraft:overworld/caves/spaghetti_2d",
        -0.13435807824134827,
        1.0,
    );
}

#[test]
fn offset_and_sloped_cheese_intervals_are_unbounded() {
    let offset = named("minecraft:overworld/offset", SEED);
    let iv = offset.interval();
    assert_eq!(iv.min.to_bits(), f32::NEG_INFINITY.to_bits());
    assert_eq!(iv.max.to_bits(), f32::INFINITY.to_bits());

    let sloped = named("minecraft:overworld/sloped_cheese", SEED);
    let iv = sloped.interval();
    assert_eq!(iv.min.to_bits(), f32::NEG_INFINITY.to_bits());
    assert_eq!(iv.max.to_bits(), f32::INFINITY.to_bits());
}

#[test]
fn preliminary_surface_level_interval() {
    let node = named("minecraft:overworld/preliminary_surface_level", SEED);
    let iv = node.interval();
    assert_eq!(iv.min.to_bits(), (-64.0_f32).to_bits());
    assert_eq!(iv.max.to_bits(), 320.0_f32.to_bits());
}

#[test]
fn no_minmax_eliminated_in_final_density() {
    let (_node, eliminated) = data().build_final_density(SEED).expect("final_density");
    assert_eq!(
        eliminated, 0,
        "no min/max eliminated in overworld final_density"
    );
}

#[test]
fn every_density_function_builds() {
    let count = data().build_all(SEED).expect("build all density functions");
    assert_eq!(count, 55);
}

#[test]
fn router_and_aquifers_build() {
    let d = data();
    let router = d.build_router(SEED).expect("router");
    let _ = router.chunk_surface_level.interval();
    let _ = router.final_density.interval();
    assert_eq!(router.minmax_eliminated, 0);
    let aquifers = d.build_aquifers(SEED).expect("aquifers");
    let _ = aquifers.surface_level.interval();
    let _ = aquifers.barrier.interval();
}

#[test]
fn dependency_axes_match_spec() {
    for key in [
        "minecraft:overworld/continents",
        "minecraft:overworld/erosion",
        "minecraft:overworld/ridges",
        "minecraft:overworld/offset",
        "minecraft:overworld/factor",
        "minecraft:overworld/jaggedness",
    ] {
        let a = axes(&named(key, SEED));
        assert!(a.has_x() && a.has_z() && !a.has_y(), "{key} should be 2D");
    }
    for key in [
        "minecraft:overworld/depth",
        "minecraft:overworld/sloped_cheese",
        "minecraft:overworld/final_density",
    ] {
        let a = axes(&named(key, SEED));
        assert!(a.has_x() && a.has_y() && a.has_z(), "{key} should be 3D");
    }
}

#[test]
fn shift_values_seed0() {
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
fn shift_values_seed12345() {
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
fn continents_and_erosion_values_seed0() {
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
fn malformed_registries_fail_without_panic() {
    assert!(build_err(
        r#"{"type":"minecraft:interval_select","input":0.0,"thresholds":[],"functions":[]}"#
    ));
    assert!(build_err(
        r#"{"type":"minecraft:interval_select","input":0.0,"thresholds":[0.0],"functions":[1.0]}"#
    ));
    assert!(build_err(
        r#"{"type":"minecraft:interval_select","input":0.0,"thresholds":[0.0,0.5],"functions":[1.0,2.0]}"#
    ));
    assert!(build_err(
        r#"{"type":"minecraft:spline","spline":{"coordinate":0.0,"points":[]}}"#
    ));
    assert!(build_err(r#"{"type":"minecraft:abs"}"#));
    assert!(build_err(
        r#"{"type":"minecraft:gradient","axis":7,"from_coordinate":0,"from_value":0.0,"to_coordinate":1,"to_value":1.0}"#
    ));
    assert!(build_err(
        r#"{"type":"minecraft:constant","value":2000000.0}"#
    ));
    assert!(build_err(r#""minecraft:does_not_exist""#));
    assert!(build_err(r#"{"type":"minecraft:not_a_real_type"}"#));
    assert!(build_err(
        r#"{"type":"minecraft:noise","noise":"minecraft:does_not_exist","xz_scale":1.0,"y_scale":1.0}"#
    ));
    assert!(build_err(r#"[1,2,3]"#));
}

#[test]
fn well_formed_interval_select_builds() {
    let node = build(
        r#"{"type":"minecraft:interval_select","input":"minecraft:y","thresholds":[0.0,10.0],"functions":[1.0,2.0,3.0]}"#,
    );
    assert_eq!(bits(eval_point(&node, 0, -5, 0)), 1.0_f32.to_bits());
    assert_eq!(bits(eval_point(&node, 0, 5, 0)), 2.0_f32.to_bits());
    assert_eq!(bits(eval_point(&node, 0, 20, 0)), 3.0_f32.to_bits());
}

mod oracle {
    use flate2::read::ZlibDecoder;
    use serde_json::Value;
    use std::io::Read;
    use std::path::PathBuf;

    const MIN_Y: i32 = -64;
    const HEIGHT: usize = 384;
    const AREA: usize = HEIGHT * 256;

    fn oracle_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests-data/oracle-1a")
    }

    fn read_chunk(file: &str) -> Vec<u8> {
        let path = oracle_dir().join(file);
        let compressed = std::fs::read(&path).expect("read oracle bin");
        let mut decoder = ZlibDecoder::new(&compressed[..]);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).expect("inflate oracle bin");
        out
    }

    fn index() -> Value {
        let path = oracle_dir().join("index.json");
        let text = std::fs::read_to_string(&path).expect("read index.json");
        serde_json::from_str(&text).expect("parse index.json")
    }

    #[test]
    fn oracle_format_is_readable() {
        let idx = index();
        assert_eq!(idx["min_y"].as_i64(), Some(i64::from(MIN_Y)));
        assert_eq!(idx["height"].as_i64(), Some(HEIGHT as i64));
        let first = idx["chunks"][0]["file"].as_str().expect("first chunk file");
        let cells = read_chunk(first);
        assert_eq!(cells.len(), AREA);
        let solid = cells.iter().filter(|&&c| c == 1).count();
        assert_eq!(
            solid as i64,
            idx["chunks"][0]["solid"].as_i64().expect("solid count")
        );
    }

    #[test]
    #[ignore = "nécessite le mode volume (tâche suivante): final_density est évaluée en mode volume par le remplissage vanilla"]
    fn final_density_matches_oracle() {
        let _ = (MIN_Y, AREA, index(), read_chunk("-7.0.bin"));
    }
}
