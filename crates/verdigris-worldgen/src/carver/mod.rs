pub mod apply;
pub mod canyon;
pub mod cave;
pub mod config;
pub mod geom;
pub mod mask;
pub mod rng;
pub mod sincos;

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use serde_json::Value;

use crate::biome::{BiomeSource, SearchState, biome_names};
use crate::carver::canyon::carve_canyon;
use crate::carver::cave::carve_cave;
use crate::carver::config::{CarverConfigError, CarverKind};
use crate::carver::mask::CarveMask;
use crate::carver::rng::{is_start, seed_large_feature};

pub use mask::{CARVE_MAX_Y, CARVE_MIN_Y, CARVE_ROWS};

pub const CARVE_RADIUS: i32 = 8;

const NETWORK_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/network.json"
));

const BLOCKS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../verdigris-protocol/data/26.3/blocks.json"
));

#[derive(Debug)]
pub enum CarverDataError {
    Config(CarverConfigError),
    Missing(String),
    Json(String),
}

impl std::fmt::Display for CarverDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(e) => write!(f, "{e}"),
            Self::Missing(s) => write!(f, "carver data missing: {s}"),
            Self::Json(s) => write!(f, "carver data json: {s}"),
        }
    }
}

impl std::error::Error for CarverDataError {}

impl From<CarverConfigError> for CarverDataError {
    fn from(e: CarverConfigError) -> Self {
        CarverDataError::Config(e)
    }
}

fn network_value() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(NETWORK_JSON).expect("network.json parses"))
}

fn blocks_value() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(BLOCKS_JSON).expect("blocks.json parses"))
}

fn block_id_to_name() -> &'static BTreeMap<i64, String> {
    static M: OnceLock<BTreeMap<i64, String>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = BTreeMap::new();
        if let Some(arr) = blocks_value().get("blocks").and_then(Value::as_array) {
            for b in arr {
                if let (Some(name), Some(id)) = (
                    b.get("name").and_then(Value::as_str),
                    b.get("id").and_then(Value::as_i64),
                ) {
                    m.insert(id, name.to_string());
                }
            }
        }
        m
    })
}

fn resolve_block_tag(name: &str, out: &mut HashSet<String>, seen: &mut HashSet<String>) {
    if !seen.insert(name.to_string()) {
        return;
    }
    let tags = match network_value().get("tags").and_then(Value::as_array) {
        Some(t) => t,
        None => return,
    };
    let block_group = tags
        .iter()
        .find(|g| g.get("registry").and_then(Value::as_str) == Some("minecraft:block"));
    let entries = match block_group
        .and_then(|g| g.get("tags"))
        .and_then(Value::as_array)
    {
        Some(e) => e,
        None => return,
    };
    let entry = entries
        .iter()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(name));
    let entry = match entry {
        Some(e) => e,
        None => return,
    };
    if let Some(ids) = entry.get("ids").and_then(Value::as_array) {
        let id_map = block_id_to_name();
        for v in ids {
            match v {
                Value::Number(n) => {
                    if let Some(id) = n.as_i64()
                        && let Some(bn) = id_map.get(&id)
                    {
                        out.insert(bn.clone());
                    }
                }
                Value::String(s) => {
                    if let Some(sub) = s.strip_prefix('#') {
                        resolve_block_tag(sub, out, seen);
                    } else {
                        out.insert(s.clone());
                    }
                }
                _ => {}
            }
        }
    }
}

pub struct CarverData {
    configs: Vec<CarverKind>,
    config_names: Vec<String>,
    carvers_by_biome: Vec<Vec<usize>>,
    uncarvable: HashSet<String>,
}

impl CarverData {
    pub fn from_worldgen(root: &Value) -> Result<Self, CarverDataError> {
        let names = biome_names();

        let carver_obj = root
            .get("carver")
            .and_then(Value::as_object)
            .ok_or_else(|| CarverDataError::Missing("carver".to_string()))?;

        let mut configs = Vec::new();
        let mut config_names = Vec::new();
        let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
        for (key, value) in carver_obj {
            let kind = CarverKind::parse(value)?;
            let idx = configs.len();
            configs.push(kind);
            config_names.push(key.clone());
            by_name.insert(key.clone(), idx);
        }

        let biome_obj = root
            .get("biome")
            .and_then(Value::as_object)
            .ok_or_else(|| CarverDataError::Missing("biome".to_string()))?;

        let mut carvers_by_biome = vec![Vec::new(); names.len()];
        for (biome_name, body) in biome_obj {
            let id = match names.id_of(biome_name) {
                Some(id) => id as usize,
                None => continue,
            };
            let mut list = Vec::new();
            match body.get("carvers") {
                Some(Value::String(s)) => {
                    if let Some(&idx) = by_name.get(s) {
                        list.push(idx);
                    }
                }
                Some(Value::Array(a)) => {
                    for e in a {
                        if let Some(s) = e.as_str()
                            && let Some(&idx) = by_name.get(s)
                        {
                            list.push(idx);
                        }
                    }
                }
                _ => {}
            }
            carvers_by_biome[id] = list;
        }

        let mut uncarvable = HashSet::new();
        let mut seen = HashSet::new();
        resolve_block_tag("minecraft:uncarvable", &mut uncarvable, &mut seen);

        Ok(CarverData {
            configs,
            config_names,
            carvers_by_biome,
            uncarvable,
        })
    }

    pub fn configs(&self) -> &[CarverKind] {
        &self.configs
    }

    pub fn config_name(&self, idx: usize) -> &str {
        &self.config_names[idx]
    }

    pub fn carvers_for(&self, biome: u16) -> &[usize] {
        self.carvers_by_biome
            .get(biome as usize)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn is_uncarvable(&self, name: &str) -> bool {
        self.uncarvable.contains(name)
    }

    pub fn uncarvable_mask(&self, block_names: &[String]) -> Vec<bool> {
        block_names
            .iter()
            .map(|n| self.uncarvable.contains(n))
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn carve_chunk_mask_into(
    data: &CarverData,
    biomes: &BiomeSource,
    state: &mut SearchState,
    seed: i64,
    cx: i32,
    cz: i32,
    mask: &mut CarveMask,
) {
    mask.clear();
    let mut dz = -CARVE_RADIUS;
    while dz <= CARVE_RADIUS {
        let mut dx = -CARVE_RADIUS;
        while dx <= CARVE_RADIUS {
            let sx = cx + dx;
            let sz = cz + dz;
            let biome =
                biomes.biome_at_point(state, 16i32.wrapping_mul(sx), 0, 16i32.wrapping_mul(sz));
            let list = data.carvers_for(biome);
            for (i, &cfg_idx) in list.iter().enumerate() {
                let kind = &data.configs[cfg_idx];
                let mut r = seed_large_feature(seed.wrapping_add(i as i64), sx, sz);
                if is_start(&mut r, kind.probability()) {
                    match kind {
                        CarverKind::Cave(c) => carve_cave(mask, c, &mut r, sx, sz, cx, cz),
                        CarverKind::Canyon(c) => carve_canyon(mask, c, &mut r, sx, sz, cx, cz),
                    }
                }
            }
            dx += 1;
        }
        dz += 1;
    }
}

pub fn carve_chunk_mask(
    data: &CarverData,
    biomes: &BiomeSource,
    state: &mut SearchState,
    seed: i64,
    cx: i32,
    cz: i32,
) -> CarveMask {
    let mut mask = CarveMask::new();
    carve_chunk_mask_into(data, biomes, state, seed, cx, cz, &mut mask);
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::worldgen_value;

    fn data() -> CarverData {
        CarverData::from_worldgen(worldgen_value()).expect("carver data")
    }

    #[test]
    fn three_overworld_carvers_present() {
        let d = data();
        let names: Vec<&str> = (0..d.configs().len()).map(|i| d.config_name(i)).collect();
        assert!(names.contains(&"minecraft:cave"));
        assert!(names.contains(&"minecraft:cave_extra_underground"));
        assert!(names.contains(&"minecraft:canyon"));
    }

    #[test]
    fn plains_has_three_carvers_in_order() {
        let d = data();
        let names = biome_names();
        let plains = names.id_of("minecraft:plains").expect("plains");
        let list = d.carvers_for(plains);
        assert_eq!(list.len(), 3);
        let resolved: Vec<&str> = list.iter().map(|&i| d.config_name(i)).collect();
        assert_eq!(
            resolved,
            vec![
                "minecraft:cave",
                "minecraft:cave_extra_underground",
                "minecraft:canyon"
            ]
        );
    }

    #[test]
    fn uncarvable_is_bedrock() {
        let d = data();
        assert!(d.is_uncarvable("minecraft:bedrock"));
        assert!(!d.is_uncarvable("minecraft:stone"));
    }
}
