use serde_json::Value;
use verdigris_worldgen::feature::order::{
    DECORATION_STEPS, global_order_from_features, overworld_biome_order,
    overworld_biomes_from_features,
};
use verdigris_worldgen::feature::registry::FEATURES_JSON;

#[test]
fn exactly_56_overworld_biomes_in_first_appearance_order() {
    let order = overworld_biome_order();
    assert_eq!(order.len(), 56, "56 overworld biomes expected");
    let root: Value = serde_json::from_str(FEATURES_JSON).unwrap();
    let biomes = overworld_biomes_from_features(&root).unwrap();
    assert_eq!(biomes.len(), 56);
    for (i, (name, _)) in biomes.iter().enumerate() {
        assert_eq!(&order[i], name, "biome order mismatch at index {i}");
    }
}

#[test]
fn global_order_has_171_features_in_11_steps() {
    let root: Value = serde_json::from_str(FEATURES_JSON).unwrap();
    let go = global_order_from_features(&root).unwrap();
    assert_eq!(go.steps().len(), DECORATION_STEPS);
    assert_eq!(
        go.total(),
        171,
        "171 placed feature entries expected (§2.5)"
    );
}

#[test]
fn per_step_order_is_consistent_with_every_biome() {
    let root: Value = serde_json::from_str(FEATURES_JSON).unwrap();
    let go = global_order_from_features(&root).unwrap();
    let biomes = overworld_biomes_from_features(&root).unwrap();
    for (_, steps) in &biomes {
        for (step, feats) in steps.iter().enumerate() {
            let g = go.step(step);
            let mut last = None;
            for name in feats {
                let pos = g.iter().position(|f| f == name);
                assert!(pos.is_some(), "feature {name} absent from step {step}");
                if let (Some(p), Some(c)) = (last, pos) {
                    assert!(p < c, "order violated for {name} in step {step}");
                }
                last = pos;
            }
        }
    }
}
