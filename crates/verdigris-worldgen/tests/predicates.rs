use serde_json::Value;
use verdigris_worldgen::feature::{BlockPredicate, TestRegion};

fn pred(v: Value) -> BlockPredicate {
    BlockPredicate::compile(&v).unwrap()
}

#[test]
fn inside_world_bounds_respects_vertical_limits() {
    let r = TestRegion::new((0, 0));
    let p = pred(serde_json::json!({"type": "inside_world_bounds"}));
    assert!(p.test(&r, 0, -64, 0).unwrap());
    assert!(p.test(&r, 0, 319, 0).unwrap());
    assert!(!p.test(&r, 0, -65, 0).unwrap());
    assert!(!p.test(&r, 0, 320, 0).unwrap());
}

#[test]
fn matching_blocks_distinguishes_air_from_stone() {
    let r = TestRegion::new((0, 0));
    let air = pred(serde_json::json!({"type": "matching_blocks", "blocks": "minecraft:air"}));
    let stone = pred(serde_json::json!({"type": "matching_blocks", "blocks": "minecraft:stone"}));
    assert!(air.test(&r, 0, 10, 0).unwrap());
    assert!(!stone.test(&r, 0, 10, 0).unwrap());
}

#[test]
fn solid_predicate_reads_physics() {
    let mut r = TestRegion::new((0, 0));
    let p = pred(serde_json::json!({"type": "solid"}));
    assert!(!p.test(&r, 0, 10, 0).unwrap());
    r.set_block_preset(0, 10, 0, 1);
    assert!(p.test(&r, 0, 10, 0).unwrap());
}

#[test]
fn matching_blocks_offset_reads_below() {
    let mut r = TestRegion::new((0, 0));
    r.set_block_preset(0, 9, 0, 1);
    let p = pred(serde_json::json!({
        "type": "matching_blocks",
        "blocks": "minecraft:stone",
        "offset": [0, -1, 0]
    }));
    assert!(p.test(&r, 0, 10, 0).unwrap());
    assert!(!p.test(&r, 0, 11, 0).unwrap());
}

#[test]
fn not_and_all_of_compose_without_permissive_default() {
    let r = TestRegion::new((0, 0));
    let p = pred(serde_json::json!({
        "type": "all_of",
        "predicates": [
            {"type": "true"},
            {"type": "not", "predicate": {"type": "matching_blocks", "blocks": "minecraft:stone"}}
        ]
    }));
    assert!(p.test(&r, 0, 10, 0).unwrap());
}

#[test]
fn would_survive_requires_solid_ground_below() {
    let mut r = TestRegion::new((0, 0));
    let p = pred(serde_json::json!({"type": "would_survive", "state": "minecraft:oak_sapling"}));
    assert!(!p.test(&r, 0, 10, 0).unwrap());
    r.set_block_preset(0, 9, 0, 1);
    assert!(p.test(&r, 0, 10, 0).unwrap());
}

#[test]
fn unknown_predicate_type_is_error() {
    assert!(BlockPredicate::compile(&serde_json::json!({"type": "minecraft:nope"})).is_err());
}
