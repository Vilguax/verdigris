use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

use crate::feature::error::{FeatureError, full_key};

pub const PHYSICS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/physics.json"
));

pub const BLOCKS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/blocks.json"
));

fn physics_value() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(PHYSICS_JSON).expect("physics.json parses"))
}

fn blocks_value() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(BLOCKS_JSON).expect("blocks.json parses"))
}

#[derive(Clone, Debug)]
struct BlockDef {
    first_state: u32,
    default_state: u32,
    properties: Vec<(String, Vec<String>)>,
}

fn block_defs() -> &'static HashMap<String, BlockDef> {
    static M: OnceLock<HashMap<String, BlockDef>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        if let Some(arr) = blocks_value().get("blocks").and_then(Value::as_array) {
            for b in arr {
                let Some(name) = b.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let first_state = b.get("first_state").and_then(Value::as_u64).unwrap_or(0) as u32;
                let default_state =
                    b.get("default_state").and_then(Value::as_u64).unwrap_or(0) as u32;
                let mut properties = Vec::new();
                if let Some(props) = b.get("properties").and_then(Value::as_array) {
                    for p in props {
                        let Some(pn) = p.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        let mut values = Vec::new();
                        if let Some(vs) = p.get("values").and_then(Value::as_array) {
                            for v in vs {
                                if let Some(s) = v.as_str() {
                                    values.push(s.to_string());
                                }
                            }
                        }
                        properties.push((pn.to_string(), values));
                    }
                }
                m.insert(
                    name.to_string(),
                    BlockDef {
                        first_state,
                        default_state,
                        properties,
                    },
                );
            }
        }
        m
    })
}

fn flag_bits() -> &'static HashMap<String, u32> {
    static M: OnceLock<HashMap<String, u32>> = OnceLock::new();
    M.get_or_init(|| index_map("flags"))
}

fn face_bits() -> &'static HashMap<String, u32> {
    static M: OnceLock<HashMap<String, u32>> = OnceLock::new();
    M.get_or_init(|| index_map("faces"))
}

fn direction_bits() -> &'static HashMap<String, u32> {
    static M: OnceLock<HashMap<String, u32>> = OnceLock::new();
    M.get_or_init(|| index_map("directions"))
}

fn index_map(key: &str) -> HashMap<String, u32> {
    let mut m = HashMap::new();
    if let Some(arr) = physics_value().get(key).and_then(Value::as_array) {
        for (i, v) in arr.iter().enumerate() {
            if let Some(s) = v.as_str() {
                m.insert(s.to_string(), i as u32);
            }
        }
    }
    m
}

pub const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

#[derive(Clone, Copy, Debug, Default)]
pub struct Physics {
    pub flags: u64,
    pub faces: u64,
    pub fluid: u16,
    pub fluid_amount: u8,
}

impl Physics {
    fn flag(&self, name: &str) -> bool {
        flag_bits()
            .get(name)
            .map(|b| (self.flags >> b) & 1 != 0)
            .unwrap_or(false)
    }

    pub fn is_air(&self) -> bool {
        self.flag("air")
    }
    pub fn is_liquid(&self) -> bool {
        self.flag("liquid")
    }
    pub fn is_legacy_solid(&self) -> bool {
        self.flag("legacy_solid")
    }
    pub fn is_replaceable(&self) -> bool {
        self.flag("replaceable")
    }
    pub fn is_solid_render(&self) -> bool {
        self.flag("solid_render")
    }

    pub fn face(&self, face: &str, dir: &str) -> bool {
        let fb = match face_bits().get(face) {
            Some(b) => *b,
            None => return false,
        };
        let db = match direction_bits().get(dir) {
            Some(b) => *b,
            None => return false,
        };
        let dir_count = direction_bits().len() as u32;
        let bit = fb * dir_count + db;
        (self.faces >> bit) & 1 != 0
    }

    pub fn sturdy_full(&self, dir: &str) -> bool {
        self.face("sturdy_full", dir)
    }
    pub fn collision_face_full(&self, dir: &str) -> bool {
        self.face("collision_face_full", dir)
    }
    pub fn collision_face_nonempty(&self, dir: &str) -> bool {
        self.face("collision_face_nonempty", dir)
    }
    pub fn occlusion_face_full(&self, dir: &str) -> bool {
        self.face("occlusion_face_full", dir)
    }
}

fn physics_for_state(state_id: u32) -> Physics {
    let states = match physics_value().get("states").and_then(Value::as_array) {
        Some(s) => s,
        None => return Physics::default(),
    };
    let Some(row) = states.get(state_id as usize).and_then(Value::as_array) else {
        return Physics::default();
    };
    let get_u64 = |i: usize| row.get(i).and_then(Value::as_u64).unwrap_or(0);
    Physics {
        flags: get_u64(0),
        faces: get_u64(1),
        fluid: get_u64(2) as u16,
        fluid_amount: get_u64(3) as u8,
    }
}

#[derive(Clone, Debug)]
pub struct StateEntry {
    pub key: String,
    pub block_name: String,
    pub state_id: u32,
    pub props: Vec<(String, String)>,
    pub physics: Physics,
}

#[derive(Debug, Default)]
pub struct StatePalette {
    entries: Vec<StateEntry>,
    index: HashMap<String, u16>,
}

impl StatePalette {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, idx: u16) -> Option<&StateEntry> {
        self.entries.get(idx as usize)
    }

    pub fn index_of(&self, key: &str) -> Option<u16> {
        self.index.get(key).copied()
    }

    pub fn intern_name(&mut self, name: &str) -> Result<u16, FeatureError> {
        self.intern(name, &[])
    }

    pub fn intern_state(&mut self, v: &Value) -> Result<u16, FeatureError> {
        match v {
            Value::String(s) => self.intern(s, &[]),
            Value::Object(o) => {
                let name = o
                    .get("Name")
                    .or_else(|| o.get("id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| FeatureError::BadField("state.Name".to_string()))?;
                let mut props = Vec::new();
                if let Some(p) = o
                    .get("Properties")
                    .or_else(|| o.get("properties"))
                    .and_then(Value::as_object)
                {
                    for (k, val) in p {
                        let sv = val
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| val.to_string());
                        props.push((k.clone(), sv));
                    }
                }
                let refs: Vec<(&str, &str)> = props
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                self.intern(name, &refs)
            }
            _ => Err(FeatureError::BadField("state".to_string())),
        }
    }

    fn intern(&mut self, name: &str, props: &[(&str, &str)]) -> Result<u16, FeatureError> {
        let full = full_key(name);
        let def = block_defs()
            .get(&full)
            .ok_or_else(|| FeatureError::UnknownState(full.clone()))?;
        let mut assigned: Vec<(String, String)> = Vec::with_capacity(def.properties.len());
        let mut local = 0u32;
        for (pname, pvalues) in &def.properties {
            let chosen = props
                .iter()
                .find(|(k, _)| *k == pname.as_str())
                .map(|(_, v)| (*v).to_string());
            let value = match chosen {
                Some(v) => v,
                None => default_value(def, pname, pvalues),
            };
            let vindex = pvalues
                .iter()
                .position(|v| v == &value)
                .ok_or_else(|| FeatureError::UnknownState(format!("{full}[{pname}={value}]")))?;
            local = local * (pvalues.len() as u32) + vindex as u32;
            assigned.push((pname.clone(), value));
        }
        let state_id = def.first_state + local;
        let key = canonical_key(&full, &assigned);
        if let Some(&i) = self.index.get(&key) {
            return Ok(i);
        }
        let i = self.entries.len() as u16;
        self.entries.push(StateEntry {
            key: key.clone(),
            block_name: full,
            state_id,
            props: assigned,
            physics: physics_for_state(state_id),
        });
        self.index.insert(key, i);
        Ok(i)
    }
}

fn default_value(def: &BlockDef, pname: &str, pvalues: &[String]) -> String {
    let mut rem = def.default_state.saturating_sub(def.first_state);
    let mut strides = vec![1u32; def.properties.len()];
    for i in (0..def.properties.len()).rev() {
        if i + 1 < def.properties.len() {
            strides[i] = strides[i + 1] * def.properties[i + 1].1.len() as u32;
        }
    }
    for (i, (name, vs)) in def.properties.iter().enumerate() {
        let vindex = (rem / strides[i]) as usize % vs.len().max(1);
        rem %= strides[i];
        if name == pname {
            return vs
                .get(vindex)
                .cloned()
                .unwrap_or_else(|| pvalues.first().cloned().unwrap_or_default());
        }
    }
    pvalues.first().cloned().unwrap_or_default()
}

fn canonical_key(name: &str, props: &[(String, String)]) -> String {
    if props.is_empty() {
        return name.to_string();
    }
    let mut s = String::from(name);
    s.push('[');
    for (i, (k, v)) in props.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(k);
        s.push('=');
        s.push_str(v);
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_block_snowy_states() {
        let mut p = StatePalette::new();
        let def = &block_defs()["minecraft:grass_block"];
        assert_eq!(def.first_state, 8);
        let i_true = p
            .intern("minecraft:grass_block", &[("snowy", "true")])
            .unwrap();
        let i_false = p
            .intern("minecraft:grass_block", &[("snowy", "false")])
            .unwrap();
        assert_eq!(p.entry(i_true).unwrap().state_id, 8);
        assert_eq!(p.entry(i_false).unwrap().state_id, 9);
    }

    #[test]
    fn unknown_state_is_error() {
        let mut p = StatePalette::new();
        assert!(p.intern_name("minecraft:not_a_block").is_err());
    }

    #[test]
    fn air_physics_flag() {
        let mut p = StatePalette::new();
        let i = p.intern_name("minecraft:air").unwrap();
        assert!(p.entry(i).unwrap().physics.is_air());
    }

    #[test]
    fn stone_default_state() {
        let mut p = StatePalette::new();
        let i = p.intern_name("minecraft:stone").unwrap();
        assert_eq!(p.entry(i).unwrap().state_id, 1);
        assert!(!p.entry(i).unwrap().physics.is_air());
    }
}
