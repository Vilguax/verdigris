use std::sync::OnceLock;

use serde_json::Value;

use crate::registry::worldgen_value;

pub const NETWORK_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/network.json"
));

pub const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
pub const OVERWORLD: &str = "minecraft:overworld";
pub const DIMENSIONS: usize = 7;
pub const ENTRY_COUNT: usize = 7594;

#[derive(Debug)]
pub struct BiomeNames {
    names: Vec<String>,
}

impl BiomeNames {
    pub fn id_of(&self, name: &str) -> Option<u16> {
        self.names.iter().position(|n| n == name).map(|p| p as u16)
    }

    pub fn name_of(&self, id: u16) -> Option<&str> {
        self.names.get(id as usize).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RawEntry {
    pub biome_id: u16,
    pub bounds: [(f32, f32); DIMENSIONS],
}

pub fn biome_names() -> &'static BiomeNames {
    static NAMES: OnceLock<BiomeNames> = OnceLock::new();
    NAMES.get_or_init(|| {
        let root: Value =
            serde_json::from_str(NETWORK_JSON).expect("embedded network.json must parse");
        let registries = root
            .get("registries")
            .and_then(Value::as_array)
            .expect("network.json registries array");
        let biome = registries
            .iter()
            .find(|r| r.get("id").and_then(Value::as_str) == Some(BIOME_REGISTRY))
            .expect("network.json biome registry");
        let entries = biome
            .get("entries")
            .and_then(Value::as_array)
            .expect("biome registry entries array");
        let names = entries
            .iter()
            .map(|v| v.as_str().expect("biome name string").to_string())
            .collect();
        BiomeNames { names }
    })
}

fn field_bounds(value: &Value) -> (f32, f32) {
    match value {
        Value::Array(pair) => {
            let min = pair[0].as_f64().expect("climate min number") as f32;
            let max = pair[1].as_f64().expect("climate max number") as f32;
            (min, max)
        }
        Value::Number(_) => {
            let v = value.as_f64().expect("climate scalar number") as f32;
            (v, v)
        }
        other => panic!("unexpected climate field {other:?}"),
    }
}

pub fn biome_entries() -> &'static Vec<RawEntry> {
    static ENTRIES: OnceLock<Vec<RawEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let names = biome_names();
        let root = worldgen_value();
        let params = root
            .get("biome_parameters")
            .and_then(Value::as_object)
            .expect("worldgen.json biome_parameters");
        let dims = params
            .get("dimensions")
            .and_then(Value::as_array)
            .expect("biome_parameters dimensions");
        assert_eq!(dims.len(), DIMENSIONS, "7 climate dimensions expected");
        let overworld = params
            .get("entries")
            .and_then(|v| v.get(OVERWORLD))
            .and_then(Value::as_array)
            .expect("biome_parameters overworld entries");
        let mut out = Vec::with_capacity(overworld.len());
        for entry in overworld {
            let fields = entry.as_array().expect("entry is an array");
            assert_eq!(
                fields.len(),
                DIMENSIONS + 1,
                "entry holds a name plus 7 fields"
            );
            let name = fields[0].as_str().expect("entry biome name");
            let biome_id = names
                .id_of(name)
                .unwrap_or_else(|| panic!("biome {name} absent from registry"));
            let mut bounds = [(0.0f32, 0.0f32); DIMENSIONS];
            for (d, slot) in bounds.iter_mut().enumerate() {
                *slot = field_bounds(&fields[d + 1]);
            }
            out.push(RawEntry { biome_id, bounds });
        }
        assert_eq!(out.len(), ENTRY_COUNT, "7594 overworld entries expected");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::{BIOME_REGISTRY, ENTRY_COUNT, biome_entries, biome_names};
    use crate::biome::quantize::{q, q_entry};
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[test]
    fn registry_has_sixty_seven_biomes() {
        assert_eq!(biome_names().len(), 67);
        assert_eq!(biome_names().id_of("minecraft:plains"), Some(41));
        assert_eq!(biome_names().id_of("minecraft:deep_dark"), Some(11));
        assert_eq!(biome_names().id_of("minecraft:badlands"), Some(0));
    }

    #[test]
    fn entries_count_and_distinct_biomes() {
        let entries = biome_entries();
        assert_eq!(entries.len(), ENTRY_COUNT);
        let distinct: BTreeSet<u16> = entries.iter().map(|e| e.biome_id).collect();
        assert_eq!(distinct.len(), 56, "56 distinct biomes expected");
    }

    #[test]
    fn q_entry_matches_float_path_on_all_distinct_values() {
        let entries = biome_entries();
        let mut values: BTreeSet<u64> = BTreeSet::new();
        for e in entries {
            for (lo, hi) in e.bounds {
                values.insert(f64::from(lo).to_bits());
                values.insert(f64::from(hi).to_bits());
            }
        }
        for bits in values {
            let v = f64::from_bits(bits);
            assert_eq!(q_entry(v), q(v as f32), "mismatch at {v}");
        }
    }

    #[test]
    fn registry_order_is_alphabetical() {
        let root: Value = serde_json::from_str(super::NETWORK_JSON).unwrap();
        let registries = root["registries"].as_array().unwrap();
        let biome = registries
            .iter()
            .find(|r| r["id"].as_str() == Some(BIOME_REGISTRY))
            .unwrap();
        let entries: Vec<&str> = biome["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let mut sorted = entries.clone();
        sorted.sort_unstable();
        assert_eq!(entries, sorted);
    }
}
