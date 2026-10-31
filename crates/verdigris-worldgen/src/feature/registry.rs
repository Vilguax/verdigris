use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde_json::Value;

use crate::feature::error::{FeatureError, full_key, strip_ns};
use crate::feature::palette::StatePalette;

pub const FEATURES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/features.json"
));

pub const PLACEMENT_TYPES: &[&str] = &[
    "count",
    "count_on_every_layer",
    "noise_threshold_count",
    "noise_based_count",
    "rarity_filter",
    "random_chance",
    "in_square",
    "height_range",
    "heightmap",
    "biome",
    "block_predicate_filter",
    "surface_relative_threshold_filter",
    "surface_water_depth_filter",
    "environment_scan",
    "offset",
    "randomly_selected",
    "cuboid",
    "fixed_placement",
    "carving_mask",
];

pub const PROVIDER_TYPES: &[&str] = &[
    "simple_state_provider",
    "weighted",
    "weighted_state_provider",
    "random_block",
    "randomized_int_state_provider",
    "randomized_int",
    "rotated_block_provider",
    "rotated",
    "rule_based",
    "noise_provider",
    "noise",
    "noise_threshold_provider",
    "noise_threshold",
    "dual_noise_provider",
    "dual_noise",
];

pub const BLOCK_PREDICATE_TYPES: &[&str] = &[
    "matching_blocks",
    "matching_block_tag",
    "matching_fluids",
    "would_survive",
    "solid",
    "has_sturdy_face",
    "replaceable",
    "inside_world_bounds",
    "unobstructed",
    "true",
    "all_of",
    "any_of",
    "not",
    "volume_match",
];

pub const RULE_TEST_TYPES: &[&str] = &[
    "always_true",
    "block_match",
    "blockstate_match",
    "tag_match",
    "random_block_match",
    "random_blockstate_match",
    "height_match",
    "all_of",
    "any_of",
    "not",
];

pub const COMPOSITION_TYPES: &[&str] = &[
    "no_op",
    "sequence",
    "overlay",
    "random_selector",
    "simple_random_selector",
    "weighted_random_selector",
    "random_boolean_selector",
];

#[derive(Clone, Debug)]
pub enum PlacedRef {
    Named(String),
    Inline(Box<PlacedFeature>),
}

#[derive(Clone, Debug)]
pub enum FeatureRef {
    Named(String),
    Inline(Box<ConfiguredFeature>),
}

#[derive(Clone, Debug)]
pub struct PlacementSpec {
    pub ty: String,
    pub config: Value,
}

#[derive(Clone, Debug)]
pub struct PlacedFeature {
    pub feature: FeatureRef,
    pub placement: Vec<PlacementSpec>,
}

#[derive(Clone, Debug)]
pub enum ConfiguredFeature {
    NoOp,
    Sequence(Vec<PlacedRef>),
    Overlay(Vec<PlacedRef>),
    SimpleRandomSelector(Vec<PlacedRef>),
    WeightedRandomSelector(Vec<(i32, PlacedRef)>),
    RandomBooleanSelector {
        if_true: Box<PlacedRef>,
        if_false: Box<PlacedRef>,
    },
    RandomSelector {
        default: Box<PlacedRef>,
        entries: Vec<(f32, PlacedRef)>,
    },
    Leaf {
        ty: String,
        config: Value,
    },
}

impl ConfiguredFeature {
    pub fn is_composition(&self) -> bool {
        !matches!(self, ConfiguredFeature::Leaf { .. })
    }
}

#[derive(Debug, Default)]
pub struct FeatureRegistry {
    pub placed: BTreeMap<String, PlacedFeature>,
    pub features: BTreeMap<String, ConfiguredFeature>,
    pub providers: BTreeMap<String, Value>,
    pub palette: StatePalette,
    pub placement_types_seen: BTreeSet<String>,
    pub provider_types_seen: BTreeSet<String>,
}

impl FeatureRegistry {
    pub fn embedded() -> Result<Self, FeatureError> {
        let root: Value =
            serde_json::from_str(FEATURES_JSON).map_err(|e| FeatureError::Json(e.to_string()))?;
        Self::from_value(&root)
    }

    pub fn from_value(root: &Value) -> Result<Self, FeatureError> {
        let mut reg = FeatureRegistry::default();

        if let Some(features) = root.get("feature").and_then(Value::as_object) {
            for (name, body) in features {
                let cf = reg.parse_feature(body)?;
                reg.features.insert(name.clone(), cf);
            }
        }

        if let Some(placed) = root.get("placed_feature").and_then(Value::as_object) {
            for (name, body) in placed {
                let pf = reg.parse_placed(body)?;
                reg.placed.insert(name.clone(), pf);
            }
        }

        if let Some(providers) = root.get("block_state_provider").and_then(Value::as_object) {
            for (name, body) in providers {
                reg.validate_provider(body)?;
                let _ = reg.intern_provider_states(body);
                reg.providers.insert(name.clone(), body.clone());
            }
        }

        reg.validate_references()?;
        Ok(reg)
    }

    fn parse_feature(&mut self, v: &Value) -> Result<ConfiguredFeature, FeatureError> {
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("feature".to_string()))?;
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .ok_or_else(|| FeatureError::MissingField("feature.type".to_string()))?;
        match ty {
            "no_op" => Ok(ConfiguredFeature::NoOp),
            "sequence" => Ok(ConfiguredFeature::Sequence(
                self.parse_placed_list(obj.get("features"))?,
            )),
            "overlay" => Ok(ConfiguredFeature::Overlay(
                self.parse_placed_list(obj.get("features"))?,
            )),
            "simple_random_selector" => Ok(ConfiguredFeature::SimpleRandomSelector(
                self.parse_placed_list(obj.get("features"))?,
            )),
            "weighted_random_selector" => {
                let arr = obj
                    .get("features")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let mut out = Vec::with_capacity(arr.len());
                for e in arr {
                    let eo = e
                        .as_object()
                        .ok_or_else(|| FeatureError::BadField("weighted entry".to_string()))?;
                    let weight = eo
                        .get("weight")
                        .and_then(Value::as_i64)
                        .ok_or_else(|| FeatureError::MissingField("weight".to_string()))?
                        as i32;
                    let data = eo
                        .get("data")
                        .ok_or_else(|| FeatureError::MissingField("data".to_string()))?;
                    out.push((weight, self.parse_placed_ref(data)?));
                }
                Ok(ConfiguredFeature::WeightedRandomSelector(out))
            }
            "random_boolean_selector" => {
                Ok(ConfiguredFeature::RandomBooleanSelector {
                    if_true: Box::new(self.parse_placed_ref(
                        obj.get("feature_true").ok_or_else(|| {
                            FeatureError::MissingField("feature_true".to_string())
                        })?,
                    )?),
                    if_false: Box::new(self.parse_placed_ref(
                        obj.get("feature_false").ok_or_else(|| {
                            FeatureError::MissingField("feature_false".to_string())
                        })?,
                    )?),
                })
            }
            "random_selector" => {
                let default = Box::new(
                    self.parse_placed_ref(
                        obj.get("default")
                            .ok_or_else(|| FeatureError::MissingField("default".to_string()))?,
                    )?,
                );
                let arr = obj
                    .get("features")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let mut entries = Vec::with_capacity(arr.len());
                for e in arr {
                    let eo = e
                        .as_object()
                        .ok_or_else(|| FeatureError::BadField("selector entry".to_string()))?;
                    let chance = eo
                        .get("chance")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| FeatureError::MissingField("chance".to_string()))?
                        as f32;
                    let feat = eo
                        .get("feature")
                        .ok_or_else(|| FeatureError::MissingField("feature".to_string()))?;
                    entries.push((chance, self.parse_placed_ref(feat)?));
                }
                Ok(ConfiguredFeature::RandomSelector { default, entries })
            }
            other => Ok(ConfiguredFeature::Leaf {
                ty: other.to_string(),
                config: v.clone(),
            }),
        }
    }

    fn parse_placed_list(&mut self, v: Option<&Value>) -> Result<Vec<PlacedRef>, FeatureError> {
        let arr = match v.and_then(Value::as_array) {
            Some(a) => a,
            None => return Ok(Vec::new()),
        };
        let mut out = Vec::with_capacity(arr.len());
        for e in arr {
            out.push(self.parse_placed_ref(e)?);
        }
        Ok(out)
    }

    fn parse_placed_ref(&mut self, v: &Value) -> Result<PlacedRef, FeatureError> {
        match v {
            Value::String(s) => Ok(PlacedRef::Named(full_key(s))),
            Value::Object(_) => Ok(PlacedRef::Inline(Box::new(self.parse_placed(v)?))),
            _ => Err(FeatureError::BadField("placed feature ref".to_string())),
        }
    }

    fn parse_placed(&mut self, v: &Value) -> Result<PlacedFeature, FeatureError> {
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("placed_feature".to_string()))?;
        let feature = match obj.get("feature") {
            Some(Value::String(s)) => FeatureRef::Named(full_key(s)),
            Some(other @ Value::Object(_)) => {
                FeatureRef::Inline(Box::new(self.parse_feature(other)?))
            }
            _ => {
                return Err(FeatureError::MissingField(
                    "placed_feature.feature".to_string(),
                ));
            }
        };
        let placement = match obj.get("placement") {
            Some(Value::Array(a)) => {
                let mut out = Vec::with_capacity(a.len());
                for m in a {
                    out.push(self.parse_placement(m)?);
                }
                out
            }
            None => Vec::new(),
            _ => return Err(FeatureError::BadField("placement".to_string())),
        };
        Ok(PlacedFeature { feature, placement })
    }

    fn parse_placement(&mut self, v: &Value) -> Result<PlacementSpec, FeatureError> {
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("placement modifier".to_string()))?;
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .ok_or_else(|| FeatureError::MissingField("placement.type".to_string()))?;
        if !PLACEMENT_TYPES.contains(&ty) {
            return Err(FeatureError::UnknownType {
                namespace: "placement_modifier".to_string(),
                name: ty.to_string(),
            });
        }
        if ty == "block_predicate_filter"
            && let Some(pred) = obj.get("predicate")
        {
            validate_block_predicate(pred)?;
        }
        self.placement_types_seen.insert(ty.to_string());
        Ok(PlacementSpec {
            ty: ty.to_string(),
            config: v.clone(),
        })
    }

    fn validate_provider(&mut self, v: &Value) -> Result<(), FeatureError> {
        match v {
            Value::String(_) => Ok(()),
            Value::Object(o) => {
                if o.contains_key("Name") || o.contains_key("id") {
                    return Ok(());
                }
                let ty = o
                    .get("type")
                    .and_then(Value::as_str)
                    .map(strip_ns)
                    .ok_or_else(|| FeatureError::MissingField("provider.type".to_string()))?;
                if !PROVIDER_TYPES.contains(&ty) {
                    return Err(FeatureError::UnknownType {
                        namespace: "block_state_provider".to_string(),
                        name: ty.to_string(),
                    });
                }
                self.provider_types_seen.insert(ty.to_string());
                Ok(())
            }
            _ => Err(FeatureError::BadField("block_state_provider".to_string())),
        }
    }

    fn intern_provider_states(&mut self, v: &Value) -> Result<(), FeatureError> {
        match v {
            Value::String(s) => {
                if !s.starts_with('#') {
                    self.palette.intern_name(s)?;
                }
                Ok(())
            }
            Value::Object(o) => {
                if o.contains_key("Name") || o.contains_key("id") {
                    self.palette.intern_state(v)?;
                    return Ok(());
                }
                if let Some(entries) = o.get("entries").and_then(Value::as_array) {
                    for e in entries {
                        if let Some(data) = e.get("data") {
                            self.intern_provider_states(data)?;
                        }
                    }
                }
                for key in ["state", "to_place"] {
                    if let Some(inner) = o.get(key) {
                        self.intern_provider_states(inner)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn validate_references(&self) -> Result<(), FeatureError> {
        for pf in self.placed.values() {
            self.check_feature_ref(&pf.feature)?;
        }
        for cf in self.features.values() {
            self.check_configured(cf)?;
        }
        Ok(())
    }

    fn check_feature_ref(&self, r: &FeatureRef) -> Result<(), FeatureError> {
        match r {
            FeatureRef::Named(n) => {
                if self.features.contains_key(n) {
                    Ok(())
                } else {
                    Err(FeatureError::UnknownReference(n.clone()))
                }
            }
            FeatureRef::Inline(cf) => self.check_configured(cf),
        }
    }

    fn check_placed_ref(&self, r: &PlacedRef) -> Result<(), FeatureError> {
        match r {
            PlacedRef::Named(n) => {
                if self.placed.contains_key(n) {
                    Ok(())
                } else {
                    Err(FeatureError::UnknownReference(n.clone()))
                }
            }
            PlacedRef::Inline(pf) => self.check_feature_ref(&pf.feature),
        }
    }

    fn check_configured(&self, cf: &ConfiguredFeature) -> Result<(), FeatureError> {
        match cf {
            ConfiguredFeature::NoOp | ConfiguredFeature::Leaf { .. } => Ok(()),
            ConfiguredFeature::Sequence(v)
            | ConfiguredFeature::Overlay(v)
            | ConfiguredFeature::SimpleRandomSelector(v) => {
                for r in v {
                    self.check_placed_ref(r)?;
                }
                Ok(())
            }
            ConfiguredFeature::WeightedRandomSelector(v) => {
                for (_, r) in v {
                    self.check_placed_ref(r)?;
                }
                Ok(())
            }
            ConfiguredFeature::RandomBooleanSelector { if_true, if_false } => {
                self.check_placed_ref(if_true)?;
                self.check_placed_ref(if_false)
            }
            ConfiguredFeature::RandomSelector { default, entries } => {
                self.check_placed_ref(default)?;
                for (_, r) in entries {
                    self.check_placed_ref(r)?;
                }
                Ok(())
            }
        }
    }
}

fn validate_block_predicate(v: &Value) -> Result<(), FeatureError> {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return Err(FeatureError::BadField("block_predicate".to_string())),
    };
    let ty = obj
        .get("type")
        .and_then(Value::as_str)
        .map(strip_ns)
        .ok_or_else(|| FeatureError::MissingField("block_predicate.type".to_string()))?;
    if !BLOCK_PREDICATE_TYPES.contains(&ty) {
        return Err(FeatureError::UnknownType {
            namespace: "block_predicate".to_string(),
            name: ty.to_string(),
        });
    }
    match ty {
        "all_of" | "any_of" => {
            if let Some(arr) = obj.get("predicates").and_then(Value::as_array) {
                for p in arr {
                    validate_block_predicate(p)?;
                }
            }
        }
        "not" => {
            if let Some(p) = obj.get("predicate") {
                validate_block_predicate(p)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_rule_test(v: &Value) -> Result<(), FeatureError> {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return Err(FeatureError::BadField("rule_test".to_string())),
    };
    let ty = obj
        .get("predicate_type")
        .and_then(Value::as_str)
        .map(strip_ns)
        .ok_or_else(|| FeatureError::MissingField("rule_test.predicate_type".to_string()))?;
    if !RULE_TEST_TYPES.contains(&ty) {
        return Err(FeatureError::UnknownType {
            namespace: "rule_test".to_string(),
            name: ty.to_string(),
        });
    }
    match ty {
        "all_of" | "any_of" => {
            if let Some(arr) = obj.get("rules").and_then(Value::as_array) {
                for p in arr {
                    validate_rule_test(p)?;
                }
            }
        }
        "not" => {
            if let Some(p) = obj.get("rule") {
                validate_rule_test(p)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_all_features_without_error() {
        let reg = FeatureRegistry::embedded().expect("features.json loads");
        assert!(!reg.placed.is_empty());
        assert!(!reg.features.is_empty());
        assert!(reg.features.contains_key("minecraft:ore_coal"));
        assert!(reg.placed.contains_key("minecraft:ore_coal_upper"));
    }

    #[test]
    fn every_placement_type_in_data_is_known() {
        let reg = FeatureRegistry::embedded().unwrap();
        for ty in &reg.placement_types_seen {
            assert!(
                PLACEMENT_TYPES.contains(&ty.as_str()),
                "placement type {ty} missing from code list"
            );
        }
    }

    #[test]
    fn unknown_placement_type_is_rejected() {
        let mut reg = FeatureRegistry::default();
        let bad = serde_json::json!({"type": "minecraft:not_a_modifier"});
        assert!(matches!(
            reg.parse_placement(&bad),
            Err(FeatureError::UnknownType { .. })
        ));
    }

    #[test]
    fn unknown_block_predicate_is_rejected() {
        let bad = serde_json::json!({"type": "minecraft:not_a_predicate"});
        assert!(matches!(
            validate_block_predicate(&bad),
            Err(FeatureError::UnknownType { .. })
        ));
    }

    #[test]
    fn unknown_rule_test_is_rejected() {
        let bad = serde_json::json!({"predicate_type": "minecraft:not_a_rule"});
        assert!(matches!(
            validate_rule_test(&bad),
            Err(FeatureError::UnknownType { .. })
        ));
    }

    #[test]
    fn unknown_reference_is_rejected() {
        let root = serde_json::json!({
            "placed_feature": {
                "minecraft:x": {"feature": "minecraft:does_not_exist", "placement": []}
            }
        });
        assert!(matches!(
            FeatureRegistry::from_value(&root),
            Err(FeatureError::UnknownReference(_))
        ));
    }
}
