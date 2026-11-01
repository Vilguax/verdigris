use std::collections::BTreeSet;

use serde_json::Value;
use verdigris_worldgen::feature::Decoration;
use verdigris_worldgen::feature::registry::{
    COMPOSITION_TYPES, FEATURE_LEAF_TYPES, FEATURES_JSON, FeatureRegistry, PLACEMENT_TYPES,
    PROVIDER_TYPES, RULE_TEST_TYPES,
};

#[test]
fn decoration_compiles_every_modifier_predicate_and_feature() {
    Decoration::embedded().expect("every modifier, predicate and feature type compiles");
}

#[test]
fn seen_types_are_all_known() {
    let reg = FeatureRegistry::embedded().unwrap();
    for ty in &reg.placement_types_seen {
        assert!(PLACEMENT_TYPES.contains(&ty.as_str()), "placement {ty}");
    }
    for ty in &reg.provider_types_seen {
        assert!(PROVIDER_TYPES.contains(&ty.as_str()), "provider {ty}");
    }
    for ty in &reg.feature_types_seen {
        assert!(
            COMPOSITION_TYPES.contains(&ty.as_str()) || FEATURE_LEAF_TYPES.contains(&ty.as_str()),
            "feature {ty}"
        );
    }
}

fn collect_pred_types(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(o) => {
            if let Some(t) = o.get("predicate_type").and_then(Value::as_str) {
                out.insert(t.strip_prefix("minecraft:").unwrap_or(t).to_string());
            }
            for x in o.values() {
                collect_pred_types(x, out);
            }
        }
        Value::Array(a) => {
            for x in a {
                collect_pred_types(x, out);
            }
        }
        _ => {}
    }
}

#[test]
fn every_rule_test_type_is_known() {
    let root: Value = serde_json::from_str(FEATURES_JSON).unwrap();
    let mut set = BTreeSet::new();
    collect_pred_types(&root, &mut set);
    assert!(!set.is_empty(), "expected at least one rule_test in data");
    for t in &set {
        assert!(RULE_TEST_TYPES.contains(&t.as_str()), "rule_test {t}");
    }
}

#[test]
fn block_placing_features_are_recognized_but_not_executable() {
    for t in ["ore", "disk", "tree", "lake", "geode", "vegetation_patch"] {
        assert!(
            FEATURE_LEAF_TYPES.contains(&t),
            "{t} should be a known leaf feature"
        );
        assert!(!COMPOSITION_TYPES.contains(&t), "{t} is not a composition");
    }
}

#[test]
fn fabricated_unknown_feature_type_is_load_error() {
    let root = serde_json::json!({
        "feature": {"minecraft:bad": {"type": "minecraft:not_a_feature"}},
        "placed_feature": {},
    });
    assert!(FeatureRegistry::from_value(&root).is_err());
}
