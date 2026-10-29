use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use serde_json::Value;

use crate::blended::{BlendedParams, OldBlendedNoise, SCALE_MAX, SCALE_MIN, SMEAR_MAX, SMEAR_MIN};
use crate::normal_noise::{NormalNoiseDef, Normalize};
use crate::octaves::OctaveStack;
use crate::sources::positional_factory;
use crate::strict_json::{StrictJsonError, parse_strict};

pub const WORLDGEN_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/worldgen.json"
));

pub const BASE_3D_NOISE_KEY: &str = "minecraft:overworld/base_3d_noise";

#[derive(Clone, Debug, PartialEq)]
pub enum WorldgenError {
    Json(String),
    DuplicateKey(String),
    MissingEntry(String),
    NotAnObject(String),
    InvalidField {
        entry: String,
        field: String,
        reason: String,
    },
    OctaveOutOfRange {
        entry: String,
        value: i32,
    },
    OctaveCountOutOfRange {
        entry: String,
        value: i32,
    },
    TooManyModifiers {
        entry: String,
        value: usize,
    },
    ModifierOutOfRange {
        entry: String,
        value: f64,
    },
    AmplitudeLenMismatch {
        entry: String,
        expected: i32,
        got: usize,
    },
    InlineNoiseWithoutKey,
    BlendedFieldMissing(String),
    BlendedFieldOutOfRange {
        field: String,
    },
}

impl fmt::Display for WorldgenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(s) => write!(f, "invalid worldgen json: {s}"),
            Self::DuplicateKey(k) => write!(f, "duplicate key in worldgen json: {k}"),
            Self::MissingEntry(e) => write!(f, "missing registry entry: {e}"),
            Self::NotAnObject(e) => write!(f, "entry is not an object: {e}"),
            Self::InvalidField {
                entry,
                field,
                reason,
            } => write!(f, "invalid field {field} in {entry}: {reason}"),
            Self::OctaveOutOfRange { entry, value } => {
                write!(f, "base_octave {value} out of range in {entry}")
            }
            Self::OctaveCountOutOfRange { entry, value } => {
                write!(f, "octave_count {value} out of range in {entry}")
            }
            Self::TooManyModifiers { entry, value } => {
                write!(f, "too many amplitude_modifiers ({value}) in {entry}")
            }
            Self::ModifierOutOfRange { entry, value } => {
                write!(f, "amplitude modifier {value} out of range in {entry}")
            }
            Self::AmplitudeLenMismatch {
                entry,
                expected,
                got,
            } => write!(
                f,
                "amplitude_modifiers length {got} != octave_count {expected} in {entry}"
            ),
            Self::InlineNoiseWithoutKey => {
                write!(f, "inline noise definition without registry key")
            }
            Self::BlendedFieldMissing(field) => {
                write!(f, "missing old_blended_noise field: {field}")
            }
            Self::BlendedFieldOutOfRange { field } => {
                write!(f, "old_blended_noise field {field} out of range")
            }
        }
    }
}

impl std::error::Error for WorldgenError {}

#[derive(Debug)]
pub struct InstantiatedNoises {
    pub noises: BTreeMap<String, OctaveStack>,
    pub old_blended: OldBlendedNoise,
}

#[derive(Clone, Debug)]
pub struct NoiseRegistry {
    noises: BTreeMap<String, NormalNoiseDef>,
    blended: BlendedParams,
}

fn number(entry: &str, field: &str, v: &Value) -> Result<f64, WorldgenError> {
    v.as_f64().ok_or_else(|| WorldgenError::InvalidField {
        entry: entry.to_string(),
        field: field.to_string(),
        reason: "not a number".to_string(),
    })
}

fn integer(entry: &str, field: &str, v: &Value) -> Result<i64, WorldgenError> {
    v.as_i64().ok_or_else(|| WorldgenError::InvalidField {
        entry: entry.to_string(),
        field: field.to_string(),
        reason: "not an integer".to_string(),
    })
}

pub fn parse_noise_def(entry: &str, v: &Value) -> Result<NormalNoiseDef, WorldgenError> {
    let obj = v
        .as_object()
        .ok_or_else(|| WorldgenError::NotAnObject(entry.to_string()))?;

    let base_amplitude = match obj.get("base_amplitude") {
        None => 1.0,
        Some(x) => number(entry, "base_amplitude", x)?,
    };

    let base_octave = match obj.get("base_octave") {
        None => {
            return Err(WorldgenError::InvalidField {
                entry: entry.to_string(),
                field: "base_octave".to_string(),
                reason: "required".to_string(),
            });
        }
        Some(x) => integer(entry, "base_octave", x)?,
    };

    let octave_count = match obj.get("octave_count") {
        None => 1,
        Some(x) => integer(entry, "octave_count", x)?,
    };

    let amplitude_modifiers = match obj.get("amplitude_modifiers") {
        None => Vec::new(),
        Some(Value::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for e in a {
                out.push(number(entry, "amplitude_modifiers", e)?);
            }
            out
        }
        Some(_) => {
            return Err(WorldgenError::InvalidField {
                entry: entry.to_string(),
                field: "amplitude_modifiers".to_string(),
                reason: "not an array".to_string(),
            });
        }
    };

    let normalize = match obj.get("normalize") {
        None => Normalize::Enabled,
        Some(Value::Bool(true)) => Normalize::Enabled,
        Some(Value::Bool(false)) => Normalize::Disabled,
        Some(Value::String(s)) if s == "legacy" => Normalize::Legacy,
        Some(_) => {
            return Err(WorldgenError::InvalidField {
                entry: entry.to_string(),
                field: "normalize".to_string(),
                reason: "expected bool or \"legacy\"".to_string(),
            });
        }
    };

    let clamp_octave = |x: i64| -> i32 { x.clamp(i32::MIN as i64, i32::MAX as i64) as i32 };

    let def = NormalNoiseDef {
        base_amplitude,
        base_octave: clamp_octave(base_octave),
        octave_count: clamp_octave(octave_count),
        amplitude_modifiers,
        normalize,
    };
    def.validate(entry)?;
    Ok(def)
}

pub fn parse_blended(entry: &str, v: &Value) -> Result<BlendedParams, WorldgenError> {
    let obj = v
        .as_object()
        .ok_or_else(|| WorldgenError::NotAnObject(entry.to_string()))?;

    let required = |field: &str| -> Result<f64, WorldgenError> {
        match obj.get(field) {
            None => Err(WorldgenError::BlendedFieldMissing(field.to_string())),
            Some(x) => x.as_f64().ok_or_else(|| WorldgenError::InvalidField {
                entry: entry.to_string(),
                field: field.to_string(),
                reason: "not a number".to_string(),
            }),
        }
    };

    let in_scale = |field: &str, value: f64| -> Result<f64, WorldgenError> {
        if (SCALE_MIN..=SCALE_MAX).contains(&value) {
            Ok(value)
        } else {
            Err(WorldgenError::BlendedFieldOutOfRange {
                field: field.to_string(),
            })
        }
    };

    let xz_scale = in_scale("xz_scale", required("xz_scale")?)?;
    let y_scale = in_scale("y_scale", required("y_scale")?)?;
    let xz_factor = in_scale("xz_factor", required("xz_factor")?)?;
    let y_factor = in_scale("y_factor", required("y_factor")?)?;
    let smear = required("smear_scale_multiplier")?;
    if !(SMEAR_MIN..=SMEAR_MAX).contains(&smear) {
        return Err(WorldgenError::BlendedFieldOutOfRange {
            field: "smear_scale_multiplier".to_string(),
        });
    }

    Ok(BlendedParams {
        xz_scale,
        y_scale,
        xz_factor,
        y_factor,
        smear_scale_multiplier: smear,
    })
}

impl NoiseRegistry {
    pub fn from_json(root: &Value) -> Result<Self, WorldgenError> {
        let noise_obj = root
            .get("noise")
            .and_then(Value::as_object)
            .ok_or_else(|| WorldgenError::MissingEntry("noise".to_string()))?;

        let mut noises = BTreeMap::new();
        for (key, value) in noise_obj {
            noises.insert(key.clone(), parse_noise_def(key, value)?);
        }

        let df = root
            .get("density_function")
            .and_then(Value::as_object)
            .ok_or_else(|| WorldgenError::MissingEntry("density_function".to_string()))?;
        let base3d = df
            .get(BASE_3D_NOISE_KEY)
            .ok_or_else(|| WorldgenError::MissingEntry(BASE_3D_NOISE_KEY.to_string()))?;
        let blended = parse_blended(BASE_3D_NOISE_KEY, base3d)?;

        Ok(Self { noises, blended })
    }

    pub fn from_json_str(s: &str) -> Result<Self, WorldgenError> {
        let root = parse_strict(s).map_err(|e| match e {
            StrictJsonError::DuplicateKey(k) => WorldgenError::DuplicateKey(k),
            StrictJsonError::Syntax(m) => WorldgenError::Json(m),
        })?;
        Self::from_json(&root)
    }

    pub fn embedded() -> Result<Self, WorldgenError> {
        Self::from_json(worldgen_value())
    }

    pub fn noises(&self) -> &BTreeMap<String, NormalNoiseDef> {
        &self.noises
    }

    pub fn blended(&self) -> &BlendedParams {
        &self.blended
    }

    pub fn instantiate(&self, seed: i64) -> Result<InstantiatedNoises, WorldgenError> {
        let p = positional_factory(seed, false);
        let mut noises = BTreeMap::new();
        for (key, def) in &self.noises {
            noises.insert(key.clone(), def.instantiate(&p, key)?);
        }
        let old_blended = OldBlendedNoise::new(&p, &self.blended);
        Ok(InstantiatedNoises {
            noises,
            old_blended,
        })
    }
}

pub fn worldgen_value() -> &'static Value {
    static JSON: OnceLock<Value> = OnceLock::new();
    JSON.get_or_init(|| {
        parse_strict(WORLDGEN_JSON)
            .expect("embedded worldgen.json must parse without duplicate keys")
    })
}

pub fn instantiate_all(seed: i64) -> Result<InstantiatedNoises, WorldgenError> {
    NoiseRegistry::embedded()?.instantiate(seed)
}

#[cfg(test)]
mod tests {
    use super::{
        NoiseRegistry, WorldgenError, instantiate_all, parse_blended, parse_noise_def,
        worldgen_value,
    };
    use serde_json::json;

    #[test]
    fn embedded_registry_parses() {
        let reg = NoiseRegistry::embedded().expect("embedded registry parses");
        assert!(reg.noises().contains_key("minecraft:offset"));
        assert!(reg.noises().contains_key("minecraft:continentalness"));
        assert!(reg.noises().contains_key("minecraft:jagged"));
    }

    #[test]
    fn all_noises_instantiate_for_seed_20261029() {
        let result = instantiate_all(20261029).expect("all noises instantiate");
        let reg = NoiseRegistry::embedded().unwrap();
        assert_eq!(result.noises.len(), reg.noises().len());
        for (key, stack) in &result.noises {
            assert!(
                !stack.layers().is_empty(),
                "{key} produced an empty octave stack"
            );
        }
        let _ = result.old_blended.interval();
    }

    #[test]
    fn worldgen_value_is_cached_and_stable() {
        let a = worldgen_value();
        let b = worldgen_value();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn base_amplitude_too_small_rejected() {
        let v = json!({ "base_amplitude": 1.0e-6, "base_octave": -3 });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::InvalidField { .. })
        ));
    }

    #[test]
    fn base_amplitude_too_large_rejected() {
        let v = json!({ "base_amplitude": 2.0e6, "base_octave": -3 });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::InvalidField { .. })
        ));
    }

    #[test]
    fn octave_count_zero_rejected() {
        let v = json!({ "base_octave": -3, "octave_count": 0 });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::OctaveCountOutOfRange { .. })
        ));
    }

    #[test]
    fn octave_count_too_large_rejected() {
        let v = json!({ "base_octave": -3, "octave_count": 33 });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::OctaveCountOutOfRange { .. })
        ));
    }

    #[test]
    fn base_octave_out_of_range_rejected() {
        let v = json!({ "base_octave": 33 });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::OctaveOutOfRange { .. })
        ));
        let v = json!({ "base_octave": -33 });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::OctaveOutOfRange { .. })
        ));
    }

    #[test]
    fn modifier_out_of_range_rejected() {
        let v = json!({ "base_octave": -3, "octave_count": 1, "amplitude_modifiers": [2.0e6] });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::ModifierOutOfRange { .. })
        ));
    }

    #[test]
    fn modifier_length_mismatch_rejected() {
        let v = json!({ "base_octave": -3, "octave_count": 3, "amplitude_modifiers": [1.0, 1.0] });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::AmplitudeLenMismatch { .. })
        ));
    }

    #[test]
    fn base_octave_required() {
        let v = json!({ "base_amplitude": 0.5 });
        assert!(matches!(
            parse_noise_def("test", &v),
            Err(WorldgenError::InvalidField { .. })
        ));
    }

    #[test]
    fn blended_missing_field_rejected() {
        let v = json!({
            "xz_scale": 0.25,
            "y_scale": 0.125,
            "xz_factor": 80.0,
            "y_factor": 160.0
        });
        assert!(matches!(
            parse_blended("test", &v),
            Err(WorldgenError::BlendedFieldMissing(_))
        ));
    }

    #[test]
    fn blended_non_numeric_field_rejected_as_typed() {
        let v = json!({
            "xz_scale": "oops",
            "y_scale": 0.125,
            "xz_factor": 80.0,
            "y_factor": 160.0,
            "smear_scale_multiplier": 8.0
        });
        assert!(matches!(
            parse_blended("test", &v),
            Err(WorldgenError::InvalidField { field, .. }) if field == "xz_scale"
        ));
    }

    #[test]
    fn blended_smear_out_of_range_rejected() {
        let v = json!({
            "xz_scale": 0.25,
            "y_scale": 0.125,
            "xz_factor": 80.0,
            "y_factor": 160.0,
            "smear_scale_multiplier": 9.0
        });
        assert!(matches!(
            parse_blended("test", &v),
            Err(WorldgenError::BlendedFieldOutOfRange { field }) if field == "smear_scale_multiplier"
        ));
    }

    #[test]
    fn blended_scale_out_of_range_rejected() {
        let v = json!({
            "xz_scale": 5000.0,
            "y_scale": 0.125,
            "xz_factor": 80.0,
            "y_factor": 160.0,
            "smear_scale_multiplier": 8.0
        });
        assert!(matches!(
            parse_blended("test", &v),
            Err(WorldgenError::BlendedFieldOutOfRange { field }) if field == "xz_scale"
        ));
    }

    #[test]
    fn embedded_blended_params_are_overworld_defaults() {
        let reg = NoiseRegistry::embedded().unwrap();
        let b = reg.blended();
        assert_eq!(b.xz_scale.to_bits(), 0.25_f64.to_bits());
        assert_eq!(b.y_scale.to_bits(), 0.125_f64.to_bits());
        assert_eq!(b.xz_factor.to_bits(), 80.0_f64.to_bits());
        assert_eq!(b.y_factor.to_bits(), 160.0_f64.to_bits());
        assert_eq!(b.smear_scale_multiplier.to_bits(), 8.0_f64.to_bits());
    }

    #[test]
    fn duplicate_noise_entry_is_typed_error() {
        let text = r#"{"noise": {"minecraft:a": {}, "minecraft:a": {}}, "density_function": {}}"#;
        assert_eq!(
            NoiseRegistry::from_json_str(text).err(),
            Some(WorldgenError::DuplicateKey("minecraft:a".to_string()))
        );
    }

    #[test]
    fn embedded_json_has_no_duplicate_key() {
        assert!(NoiseRegistry::from_json_str(super::WORLDGEN_JSON).is_ok());
    }

    #[test]
    fn malformed_json_string_is_json_error() {
        assert!(matches!(
            NoiseRegistry::from_json_str("{ not valid json "),
            Err(WorldgenError::Json(_))
        ));
    }

    #[test]
    fn empty_key_is_inline_noise_error() {
        let reg = NoiseRegistry::embedded().unwrap();
        let def = reg.noises().get("minecraft:offset").unwrap().clone();
        let p = crate::sources::positional_factory(0, false);
        assert!(matches!(
            def.instantiate(&p, ""),
            Err(WorldgenError::InlineNoiseWithoutKey)
        ));
    }
}
