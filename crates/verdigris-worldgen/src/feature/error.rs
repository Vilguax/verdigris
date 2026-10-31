use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum FeatureError {
    Json(String),
    MissingField(String),
    BadField(String),
    UnknownType { namespace: String, name: String },
    UnknownReference(String),
    UnknownState(String),
    Cycle(String),
    Region(String),
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(s) => write!(f, "feature json error: {s}"),
            Self::MissingField(s) => write!(f, "missing field: {s}"),
            Self::BadField(s) => write!(f, "bad field: {s}"),
            Self::UnknownType { namespace, name } => {
                write!(f, "unknown {namespace} type: {name}")
            }
            Self::UnknownReference(s) => write!(f, "unknown reference: {s}"),
            Self::UnknownState(s) => write!(f, "unknown block state: {s}"),
            Self::Cycle(s) => write!(f, "cycle in feature ordering: {s}"),
            Self::Region(s) => write!(f, "region access error: {s}"),
        }
    }
}

impl std::error::Error for FeatureError {}

pub fn full_key(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

pub fn strip_ns(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}
