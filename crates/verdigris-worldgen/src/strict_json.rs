use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

const DUPLICATE_OPEN: &str = "duplicate key `";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrictJsonError {
    DuplicateKey(String),
    Syntax(String),
}

impl fmt::Display for StrictJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey(key) => write!(f, "duplicate key {key}"),
            Self::Syntax(s) => f.write_str(s),
        }
    }
}

struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non finite number"))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Strict::deserialize(deserializer).map(|s| s.0)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(Strict(v)) = seq.next_element()? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if out.contains_key(&key) {
                return Err(de::Error::custom(format!("{DUPLICATE_OPEN}{key}`")));
            }
            let Strict(v) = map.next_value()?;
            out.insert(key, v);
        }
        Ok(Value::Object(out))
    }
}

pub fn parse_strict(s: &str) -> Result<Value, StrictJsonError> {
    serde_json::from_str::<Strict>(s).map(|s| s.0).map_err(|e| {
        let message = e.to_string();
        match message
            .strip_prefix(DUPLICATE_OPEN)
            .and_then(|rest| rest.split_once('`'))
        {
            Some((key, _)) => StrictJsonError::DuplicateKey(key.to_owned()),
            None => StrictJsonError::Syntax(message),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{StrictJsonError, parse_strict};
    use serde_json::json;

    #[test]
    fn same_value_as_serde_json_without_duplicates() {
        let text = r#"{"a": [1, -2, 3.5, true, null, "x"], "b": {"c": 0.1}}"#;
        let loose: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parse_strict(text).unwrap(), loose);
    }

    #[test]
    fn top_level_duplicate_rejected() {
        assert_eq!(
            parse_strict(r#"{"a": 1, "a": 2}"#),
            Err(StrictJsonError::DuplicateKey("a".to_owned()))
        );
    }

    #[test]
    fn nested_duplicate_rejected() {
        let text = r#"{"noise": {"minecraft:offset": {"base_octave": -3, "base_octave": -4}}}"#;
        assert_eq!(
            parse_strict(text),
            Err(StrictJsonError::DuplicateKey("base_octave".to_owned()))
        );
    }

    #[test]
    fn same_key_in_sibling_objects_is_fine() {
        assert_eq!(
            parse_strict(r#"[{"a": 1}, {"a": 2}]"#).unwrap(),
            json!([{"a": 1}, {"a": 2}])
        );
    }

    #[test]
    fn syntax_error_is_typed() {
        assert!(matches!(
            parse_strict("{ not json"),
            Err(StrictJsonError::Syntax(_))
        ));
    }
}
