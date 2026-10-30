use serde_json::Value;
use verdigris_worldgen::data::WorldgenData;
use verdigris_worldgen::df::eval::{Mode, eval, eval_point};
use verdigris_worldgen::df::node::Node;
use verdigris_worldgen::df::volume::{
    Volume, VolumeContext, VolumeError, contains_interpolated, eval_volume, eval_volume_with,
};
use verdigris_worldgen::registry::worldgen_value;

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

fn named(key: &str, seed: i64) -> Node {
    data().build_named(key, seed).expect("named build").0
}

fn volume(node: &Node, vol: &Volume) -> Vec<f32> {
    Vec::from(eval_volume(node, vol).expect("volume eval"))
}

fn point_array(node: &Node, vol: &Volume) -> Vec<f32> {
    let mut out = vec![0.0f32; vol.len()];
    for k in 0..vol.sizes[2] {
        for i in 0..vol.sizes[0] {
            for j in 0..vol.sizes[1] {
                let idx = vol.linear_index(i, j, k);
                let [x, y, z] = vol.block_coord(i, j, k);
                out[idx] = eval_point(node, x, y, z);
            }
        }
    }
    out
}

fn pointwise_volume(node: &Node, vol: &Volume) -> Vec<f32> {
    let mut out = vec![0.0f32; vol.len()];
    for k in 0..vol.sizes[2] {
        for i in 0..vol.sizes[0] {
            for j in 0..vol.sizes[1] {
                let idx = vol.linear_index(i, j, k);
                let [x, y, z] = vol.block_coord(i, j, k);
                out[idx] = eval(node, x, y, z, Mode::Volume);
            }
        }
    }
    out
}

fn assert_bits_eq(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(x.to_bits(), y.to_bits(), "mismatch at index {i}");
    }
}

#[test]
fn linear_index_maps_non_cubic_volume() {
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [2, 3, 4]);
    assert_eq!(vol.len(), 24);
    let mut seen = vec![false; vol.len()];
    for k in 0..4 {
        for i in 0..2 {
            for j in 0..3 {
                let idx = vol.linear_index(i, j, k);
                assert!(idx < vol.len());
                assert!(!seen[idx], "index {idx} reused");
                seen[idx] = true;
                assert_eq!(idx, j + (i + k * 2) * 3);
            }
        }
    }
    assert!(seen.iter().all(|&b| b));
}

#[test]
fn block_coord_uses_origin_and_steps() {
    let vol = Volume::new([-10, -64, 7], [4, 8, -2], [3, 2, 2]);
    assert_eq!(vol.block_coord(0, 0, 0), [-10, -64, 7]);
    assert_eq!(vol.block_coord(2, 1, 1), [-10 + 8, -64 + 8, 7 - 2]);
}

#[test]
fn single_cell_volume_with_negative_origin() {
    let node = named("minecraft:y", 0);
    let vol = Volume::new([-5, -50, -9], [1, 1, 1], [1, 1, 1]);
    let out = volume(&node, &vol);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].to_bits(), (-50.0_f32).to_bits());
}

#[test]
fn interpolated_in_volume_is_an_error_without_panic() {
    let node = build(
        r#"{"type":"minecraft:interpolated","input":"minecraft:y","cell_size_xz":4,"cell_size_y":4}"#,
    );
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [2, 2, 2]);
    assert_eq!(
        eval_volume(&node, &vol),
        Err(VolumeError::InterpolatedUnsupported)
    );
}

#[test]
fn interpolated_nested_deep_is_an_error() {
    let node = build(
        r#"{"type":"minecraft:add","left":1.0,"right":{"type":"minecraft:interpolated","input":"minecraft:y","cell_size_xz":4,"cell_size_y":4}}"#,
    );
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [2, 2, 2]);
    assert_eq!(
        eval_volume(&node, &vol),
        Err(VolumeError::InterpolatedUnsupported)
    );
}

fn parity_vol() -> Volume {
    Volume::new([100, -40, -200], [8, 16, 8], [3, 3, 3])
}

fn assert_point_volume_parity(json: &str) {
    let node = build(json);
    let vol = parity_vol();
    assert_bits_eq(&volume(&node, &vol), &point_array(&node, &vol));
}

#[test]
fn parity_const_add_sub_unary_clamp() {
    assert_point_volume_parity(r#"{"type":"minecraft:constant","value":1.5}"#);
    assert_point_volume_parity(r#"{"type":"minecraft:add","left":"minecraft:y","right":2.0}"#);
    assert_point_volume_parity(
        r#"{"type":"minecraft:abs","input":{"type":"minecraft:add","left":"minecraft:y","right":-3.0}}"#,
    );
    assert_point_volume_parity(r#"{"type":"minecraft:squeeze","input":"minecraft:y"}"#);
    assert_point_volume_parity(r#"{"type":"minecraft:half_negative","input":"minecraft:y"}"#);
    assert_point_volume_parity(
        r#"{"type":"minecraft:clamp","input":"minecraft:y","min":-10.0,"max":10.0}"#,
    );
}

#[test]
fn parity_lerp_range_choice_interval_select() {
    assert_point_volume_parity(
        r#"{"type":"minecraft:lerp","alpha":0.25,"first":"minecraft:y","second":3.0}"#,
    );
    assert_point_volume_parity(
        r#"{"type":"minecraft:range_choice","input":"minecraft:y","min_inclusive":-10.0,"max_exclusive":10.0,"when_in_range":1.0,"when_out_of_range":"minecraft:y"}"#,
    );
    assert_point_volume_parity(
        r#"{"type":"minecraft:interval_select","input":"minecraft:y","thresholds":[0.0,10.0],"functions":[1.0,2.0,3.0]}"#,
    );
}

#[test]
fn gradient_squeeze_half_negative_volume_bits() {
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 3, 1]);
    let grad = build(
        r#"{"type":"minecraft:gradient","axis":"y","from_coordinate":-64,"from_value":1.5,"to_coordinate":320,"to_value":-1.5}"#,
    );
    let out = volume(&grad, &vol);
    assert_eq!(out[vol.linear_index(0, 0, 0)].to_bits(), 1.0_f32.to_bits());

    let sq = build(r#"{"type":"minecraft:squeeze","input":1.0}"#);
    assert_eq!(
        volume(&sq, &vol)[0].to_bits(),
        (0.4583333432674408_f64 as f32).to_bits()
    );

    let hn = build(r#"{"type":"minecraft:half_negative","input":-1.0}"#);
    assert_eq!(volume(&hn, &vol)[0].to_bits(), (-0.5_f32).to_bits());
}

#[test]
fn spline_volume_matches_hand_values() {
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 1, 1]);
    let two_point = build(
        r#"{"type":"minecraft:spline","spline":{"coordinate":0.0,"points":[{"location":-1.0,"value":0.0,"derivative":0.0},{"location":1.0,"value":1.0,"derivative":0.0}]}}"#,
    );
    assert_eq!(volume(&two_point, &vol)[0].to_bits(), 0.5_f32.to_bits());

    let ridges = build(
        r#"{"type":"minecraft:spline","spline":{"coordinate":0.0,"points":[{"location":-1.0,"value":-0.08880186,"derivative":0.38940096},{"location":1.0,"value":0.69000006,"derivative":0.38940096}]}}"#,
    );
    assert_eq!(
        volume(&ridges, &vol)[0].to_bits(),
        (0.3005990982055664_f64 as f32).to_bits()
    );
}

fn gradient_zero() -> &'static str {
    r#"{"type":"minecraft:gradient","axis":"y","from_coordinate":-64,"from_value":0.0,"to_coordinate":320,"to_value":0.0}"#
}

fn gradient_neg_five() -> &'static str {
    r#"{"type":"minecraft:gradient","axis":"y","from_coordinate":0,"from_value":-5.0,"to_coordinate":10,"to_value":5.0}"#
}

#[test]
fn mul_volume_has_no_zero_short_circuit() {
    let json = format!(
        r#"{{"type":"minecraft:mul","left":{},"right":{}}}"#,
        gradient_zero(),
        gradient_neg_five()
    );
    let node = build(&json);
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 1, 1]);
    let v = volume(&node, &vol)[0];
    assert_eq!(v.to_bits(), (-0.0_f32).to_bits());
    let p = eval_point(&node, 0, 0, 0);
    assert_eq!(p.to_bits(), 0.0_f32.to_bits());
    assert_ne!(v.to_bits(), p.to_bits());
}

#[test]
fn div_volume_has_no_zero_short_circuit() {
    let json = format!(
        r#"{{"type":"minecraft:div","left":{},"right":{}}}"#,
        gradient_zero(),
        gradient_neg_five()
    );
    let node = build(&json);
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 1, 1]);
    let v = volume(&node, &vol)[0];
    assert_eq!(v.to_bits(), (-0.0_f32).to_bits());
    let p = eval_point(&node, 0, 0, 0);
    assert_eq!(p.to_bits(), 0.0_f32.to_bits());
    assert_ne!(v.to_bits(), p.to_bits());
}

#[test]
fn min_max_volume_select_plus_zero_over_minus_zero() {
    let neg_zero = format!(
        r#"{{"type":"minecraft:negate","input":{}}}"#,
        gradient_zero()
    );
    let min_json = format!(
        r#"{{"type":"minecraft:min","left":{},"right":{neg_zero}}}"#,
        gradient_zero()
    );
    let max_json = format!(
        r#"{{"type":"minecraft:max","left":{},"right":{neg_zero}}}"#,
        gradient_zero()
    );
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 1, 1]);
    assert!(matches!(build(&min_json), Node::Min { .. }));
    assert!(matches!(build(&max_json), Node::Max { .. }));
    assert_eq!(
        volume(&build(&min_json), &vol)[0].to_bits(),
        0.0_f32.to_bits()
    );
    assert_eq!(
        volume(&build(&max_json), &vol)[0].to_bits(),
        0.0_f32.to_bits()
    );
}

#[test]
fn min_const_volume_is_strict_not_jmin() {
    let json = format!(
        r#"{{"type":"minecraft:min","left":{},"right":-0.0}}"#,
        gradient_zero()
    );
    let node = build(&json);
    assert!(matches!(node, Node::MinConst { .. }));
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 1, 1]);
    let v = volume(&node, &vol)[0];
    let p = eval_point(&node, 0, 0, 0);
    assert_eq!(v.to_bits(), 0.0_f32.to_bits());
    assert_eq!(p.to_bits(), (-0.0_f32).to_bits());
    assert_ne!(v.to_bits(), p.to_bits());
}

#[test]
fn max_const_volume_is_strict_not_jmax() {
    let neg_zero = format!(
        r#"{{"type":"minecraft:negate","input":{}}}"#,
        gradient_zero()
    );
    let json = format!(r#"{{"type":"minecraft:max","left":{neg_zero},"right":0.0}}"#);
    let node = build(&json);
    assert!(matches!(node, Node::MaxConst { .. }));
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 1, 1]);
    let v = volume(&node, &vol)[0];
    let p = eval_point(&node, 0, 0, 0);
    assert_eq!(v.to_bits(), (-0.0_f32).to_bits());
    assert_eq!(p.to_bits(), 0.0_f32.to_bits());
    assert_ne!(v.to_bits(), p.to_bits());
}

#[test]
fn min_max_non_const_differ_from_point_on_signed_zero() {
    let neg_zero_wide = r#"{"type":"minecraft:negate","input":{"type":"minecraft:gradient","axis":"y","from_coordinate":0,"from_value":0.0,"to_coordinate":10,"to_value":10.0}}"#;
    let min_json = format!(
        r#"{{"type":"minecraft:min","left":{},"right":{neg_zero_wide}}}"#,
        gradient_zero()
    );
    let node = build(&min_json);
    assert!(matches!(node, Node::Min { .. }));
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [1, 1, 1]);
    let v = volume(&node, &vol)[0];
    let p = eval_point(&node, 0, 0, 0);
    assert_eq!(v.to_bits(), 0.0_f32.to_bits());
    assert_eq!(p.to_bits(), (-0.0_f32).to_bits());
    assert_ne!(v.to_bits(), p.to_bits());
}

#[test]
fn noise_volume_is_reproducible_and_uses_volume_grouping() {
    let node = build(
        r#"{"type":"minecraft:noise","noise":"minecraft:continentalness","xz_scale":0.3,"y_scale":0.0}"#,
    );
    let vol = Volume::new([37, 0, -91], [13, 1, 13], [4, 1, 4]);
    let a = volume(&node, &vol);
    let b = volume(&node, &vol);
    assert_bits_eq(&a, &b);
    assert_bits_eq(&a, &pointwise_volume(&node, &vol));
}

#[test]
fn shift_a_and_shift_b_parity_point_volume() {
    let vol = parity_vol();
    let shift_a = build(r#"{"type":"minecraft:shift_a","noise":"minecraft:offset"}"#);
    let shift_b = build(r#"{"type":"minecraft:shift_b","noise":"minecraft:offset"}"#);
    assert_bits_eq(&volume(&shift_a, &vol), &point_array(&shift_a, &vol));
    assert_bits_eq(&volume(&shift_b, &vol), &point_array(&shift_b, &vol));
}

#[test]
#[ignore = "5.2 INCERTAIN: calcul indépendant non confronté à vanilla"]
fn shift_a_equals_shift_b_5_2_vectors() {
    let shift_a = build(r#"{"type":"minecraft:shift_a","noise":"minecraft:offset"}"#);
    let shift_b = build(r#"{"type":"minecraft:shift_b","noise":"minecraft:offset"}"#);
    let vol = Volume::new([100, 0, -200], [1, 1, 1], [1, 1, 1]);
    assert_bits_eq(&volume(&shift_a, &vol), &volume(&shift_b, &vol));
}

#[test]
fn old_blended_volume_reproducible() {
    let node = named("minecraft:overworld/base_3d_noise", SEED);
    let vol = Volume::new([16, -16, 48], [4, 8, 4], [3, 3, 3]);
    let a = volume(&node, &vol);
    let b = volume(&node, &vol);
    assert_bits_eq(&a, &b);
    assert_eq!(a.len(), 27);
}

#[test]
fn cache_reuses_same_key_and_recomputes_otherwise() {
    let node = build(r#"{"type":"minecraft:cache","input":"minecraft:y"}"#);
    let vol_a = Volume::new([0, 0, 0], [1, 1, 1], [2, 2, 2]);
    let vol_b = Volume::new([0, 8, 0], [1, 1, 1], [2, 2, 2]);

    let mut ctx = VolumeContext::new();
    let first = eval_volume_with(&mut ctx, &node, &vol_a).expect("first");
    assert_eq!(ctx.cache_hits(), 0);
    assert_eq!(ctx.cache_misses(), 1);

    let second = eval_volume_with(&mut ctx, &node, &vol_a).expect("second");
    assert_eq!(ctx.cache_hits(), 1);
    assert_eq!(ctx.cache_misses(), 1);
    assert_bits_eq(&first, &second);

    let _third = eval_volume_with(&mut ctx, &node, &vol_b).expect("third");
    assert_eq!(ctx.cache_hits(), 1);
    assert_eq!(ctx.cache_misses(), 2);

    let mut fresh = VolumeContext::new();
    let _ = eval_volume_with(&mut fresh, &node, &vol_a).expect("fresh");
    assert_eq!(fresh.cache_hits(), 0);
    assert_eq!(fresh.cache_misses(), 1);
}

#[test]
fn slice_broadcasts_fixed_coordinate() {
    let node =
        build(r#"{"type":"minecraft:slice","axis":"y","coordinate":5,"input":"minecraft:y"}"#);
    let vol = Volume::new([0, -20, 0], [1, 1, 1], [2, 4, 2]);
    let out = volume(&node, &vol);
    for &v in &out {
        assert_eq!(v.to_bits(), 5.0_f32.to_bits());
    }
}

#[test]
fn blend_functions_volume_values() {
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [2, 2, 2]);
    let alpha = build(r#"{"type":"minecraft:blend_alpha"}"#);
    let offset = build(r#"{"type":"minecraft:blend_offset"}"#);
    let beard = build(r#"{"type":"minecraft:beardifier"}"#);
    for &v in &volume(&alpha, &vol) {
        assert_eq!(v.to_bits(), 1.0_f32.to_bits());
    }
    for &v in &volume(&offset, &vol) {
        assert_eq!(v.to_bits(), 0.0_f32.to_bits());
    }
    for &v in &volume(&beard, &vol) {
        assert_eq!(v.to_bits(), 0.0_f32.to_bits());
    }

    let density = build(r#"{"type":"minecraft:blend_density","input":"minecraft:y"}"#);
    let y = named("minecraft:y", 0);
    assert_bits_eq(&volume(&density, &vol), &volume(&y, &vol));
}

#[test]
fn find_top_surface_volume_equals_point() {
    let node = build(
        r#"{"type":"minecraft:find_top_surface","density":{"type":"minecraft:constant","value":1.0},"upper_bound":{"type":"minecraft:constant","value":100.0},"lower_bound":-64,"cell_height":8}"#,
    );
    let vol = Volume::new([10, -64, -30], [4, 8, 4], [3, 3, 3]);
    assert_bits_eq(&volume(&node, &vol), &point_array(&node, &vol));
}

#[test]
fn every_density_function_evaluates_in_volume_without_panic() {
    let root = worldgen_value();
    let df = root
        .get("density_function")
        .and_then(Value::as_object)
        .expect("density_function section");
    let vol = Volume::new([0, 0, 0], [1, 1, 1], [4, 8, 4]);
    for key in df.keys() {
        let node = named(key, SEED);
        let result = eval_volume(&node, &vol);
        if contains_interpolated(&node) {
            assert_eq!(
                result,
                Err(VolumeError::InterpolatedUnsupported),
                "{key} contains interpolated and must error"
            );
        } else {
            let values = result.unwrap_or_else(|_| panic!("{key} should evaluate in volume"));
            assert_eq!(values.len(), vol.len(), "{key} produced wrong length");
        }
    }
}
