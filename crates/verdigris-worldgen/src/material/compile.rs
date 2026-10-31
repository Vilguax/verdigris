use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::biome::BiomeNames;
use crate::octaves::OctaveStack;
use crate::sources::PositionalFactory;

#[derive(Clone, Debug)]
pub enum MaterialError {
    NotObject(String),
    MissingType,
    UnknownType(String),
    MissingField(String),
    BadField(String),
    MissingReference(String),
    UnknownNoise(String),
    UnknownBiome(String),
}

impl std::fmt::Display for MaterialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotObject(s) => write!(f, "material value is not an object: {s}"),
            Self::MissingType => write!(f, "material object without type"),
            Self::UnknownType(t) => write!(f, "unknown material type: {t}"),
            Self::MissingField(s) => write!(f, "missing material field: {s}"),
            Self::BadField(s) => write!(f, "bad material field: {s}"),
            Self::MissingReference(s) => write!(f, "missing material reference: {s}"),
            Self::UnknownNoise(s) => write!(f, "unknown material noise: {s}"),
            Self::UnknownBiome(s) => write!(f, "unknown biome in rule: {s}"),
        }
    }
}

impl std::error::Error for MaterialError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OreKind {
    Copper,
    Iron,
}

#[derive(Clone, Debug)]
pub struct OreVein {
    pub ore: u16,
    pub raw_ore: u16,
    pub filler: u16,
    pub raw_ore_chance: f32,
    pub kind: OreKind,
}

#[derive(Clone, Debug)]
pub enum CRule {
    Sequence(Vec<CRule>),
    Condition { cond: CCond, then: Box<CRule> },
    Block(u16),
    Bandlands,
    OreVein(OreVein),
}

#[derive(Clone, Debug)]
pub enum CCond {
    Biome(u128),
    Not(Box<CCond>),
    NoiseThreshold {
        stack: Arc<OctaveStack>,
        min: f64,
        max: f64,
        is_3d: bool,
    },
    VerticalGradient {
        factory: PositionalFactory,
        a: i32,
        c: i32,
    },
    YAbove {
        anchor: i32,
        mult: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        mult: i32,
        add_stone_depth: bool,
    },
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary: i32,
        ceiling: bool,
    },
    Steep,
    Hole,
    AbovePreliminarySurface,
    Temperature,
}

pub struct Compiler<'a> {
    pub rules: &'a Map<String, Value>,
    pub conds: &'a Map<String, Value>,
    pub noises: &'a HashMap<String, Arc<OctaveStack>>,
    pub factory: &'a PositionalFactory,
    pub biome_names: &'a BiomeNames,
    pub blocks: Vec<String>,
    pub block_index: HashMap<String, u16>,
    pub min_gen_y: i32,
    pub gen_depth: i32,
    pub sea_level: i32,
}

fn full_key(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

impl<'a> Compiler<'a> {
    pub fn new(
        rules: &'a Map<String, Value>,
        conds: &'a Map<String, Value>,
        noises: &'a HashMap<String, Arc<OctaveStack>>,
        factory: &'a PositionalFactory,
        biome_names: &'a BiomeNames,
        sea_level: i32,
    ) -> Self {
        let mut c = Compiler {
            rules,
            conds,
            noises,
            factory,
            biome_names,
            blocks: Vec::new(),
            block_index: HashMap::new(),
            min_gen_y: -64,
            gen_depth: 384,
            sea_level,
        };
        for name in [
            "minecraft:air",
            "minecraft:water",
            "minecraft:lava",
            "minecraft:stone",
        ] {
            c.block_idx(name);
        }
        c
    }

    pub fn block_idx(&mut self, name: &str) -> u16 {
        let full = full_key(name);
        if let Some(&i) = self.block_index.get(&full) {
            return i;
        }
        let i = self.blocks.len() as u16;
        self.block_index.insert(full.clone(), i);
        self.blocks.push(full);
        i
    }

    pub fn air_idx(&self) -> u16 {
        self.block_index["minecraft:air"]
    }
    pub fn water_idx(&self) -> u16 {
        self.block_index["minecraft:water"]
    }
    pub fn lava_idx(&self) -> u16 {
        self.block_index["minecraft:lava"]
    }
    pub fn stone_idx(&self) -> u16 {
        self.block_index["minecraft:stone"]
    }

    fn biome_bit(&self, name: &str) -> Result<u128, MaterialError> {
        let id = self
            .biome_names
            .id_of(&full_key(name))
            .ok_or_else(|| MaterialError::UnknownBiome(name.to_string()))?;
        Ok(1u128 << id)
    }

    fn resolve_anchor(&self, v: &Value) -> Result<i32, MaterialError> {
        let obj = v
            .as_object()
            .ok_or_else(|| MaterialError::BadField("anchor".to_string()))?;
        if let Some(k) = obj.get("absolute").and_then(Value::as_i64) {
            Ok(k as i32)
        } else if let Some(k) = obj.get("above_bottom").and_then(Value::as_i64) {
            Ok(self.min_gen_y.wrapping_add(k as i32))
        } else if let Some(k) = obj.get("below_top").and_then(Value::as_i64) {
            Ok(self
                .gen_depth
                .wrapping_sub(1)
                .wrapping_add(self.min_gen_y)
                .wrapping_sub(k as i32))
        } else if let Some(k) = obj.get("relative_to_sea_level").and_then(Value::as_i64) {
            Ok(self.sea_level.wrapping_add(k as i32))
        } else {
            Err(MaterialError::BadField("anchor kind".to_string()))
        }
    }

    fn block_state_idx(&mut self, v: &Value) -> Result<u16, MaterialError> {
        match v {
            Value::String(s) => Ok(self.block_idx(s)),
            Value::Object(o) => {
                let id = o
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| MaterialError::BadField("result_state.id".to_string()))?;
                Ok(self.block_idx(id))
            }
            _ => Err(MaterialError::BadField("result_state".to_string())),
        }
    }

    pub fn compile_rule(&mut self, v: &Value) -> Result<CRule, MaterialError> {
        match v {
            Value::String(s) => {
                let full = full_key(s);
                let target = self
                    .rules
                    .get(&full)
                    .ok_or_else(|| MaterialError::MissingReference(full.clone()))?
                    .clone();
                self.compile_rule(&target)
            }
            Value::Object(o) => self.compile_rule_obj(o),
            _ => Err(MaterialError::NotObject(v.to_string())),
        }
    }

    fn compile_rule_obj(&mut self, o: &Map<String, Value>) -> Result<CRule, MaterialError> {
        let ty = o
            .get("type")
            .and_then(Value::as_str)
            .ok_or(MaterialError::MissingType)?;
        let ty = ty.strip_prefix("minecraft:").unwrap_or(ty);
        match ty {
            "sequence" => {
                let arr = o
                    .get("sequence")
                    .and_then(Value::as_array)
                    .ok_or_else(|| MaterialError::MissingField("sequence".to_string()))?;
                let mut seq = Vec::with_capacity(arr.len());
                for e in arr {
                    seq.push(self.compile_rule(e)?);
                }
                Ok(CRule::Sequence(seq))
            }
            "condition" => {
                let if_true = o
                    .get("if_true")
                    .ok_or_else(|| MaterialError::MissingField("if_true".to_string()))?
                    .clone();
                let then_run = o
                    .get("then_run")
                    .ok_or_else(|| MaterialError::MissingField("then_run".to_string()))?
                    .clone();
                let cond = self.compile_cond(&if_true)?;
                let then = self.compile_rule(&then_run)?;
                Ok(CRule::Condition {
                    cond,
                    then: Box::new(then),
                })
            }
            "block" => {
                let rs = o
                    .get("result_state")
                    .ok_or_else(|| MaterialError::MissingField("result_state".to_string()))?
                    .clone();
                Ok(CRule::Block(self.block_state_idx(&rs)?))
            }
            "bandlands" => Ok(CRule::Bandlands),
            "ore_vein" => {
                let ore = self.block_idx(
                    o.get("ore_block")
                        .and_then(Value::as_str)
                        .ok_or_else(|| MaterialError::MissingField("ore_block".to_string()))?,
                );
                let raw_ore = self.block_idx(
                    o.get("raw_ore_block")
                        .and_then(Value::as_str)
                        .ok_or_else(|| MaterialError::MissingField("raw_ore_block".to_string()))?,
                );
                let filler = self.block_idx(
                    o.get("filler_block")
                        .and_then(Value::as_str)
                        .ok_or_else(|| MaterialError::MissingField("filler_block".to_string()))?,
                );
                let raw_ore_chance = o
                    .get("raw_ore_chance")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| MaterialError::MissingField("raw_ore_chance".to_string()))?
                    as f32;
                let density = o
                    .get("density")
                    .and_then(Value::as_str)
                    .ok_or_else(|| MaterialError::MissingField("density".to_string()))?;
                let kind = if density.contains("iron") {
                    OreKind::Iron
                } else {
                    OreKind::Copper
                };
                Ok(CRule::OreVein(OreVein {
                    ore,
                    raw_ore,
                    filler,
                    raw_ore_chance,
                    kind,
                }))
            }
            other => Err(MaterialError::UnknownType(other.to_string())),
        }
    }

    pub fn compile_cond(&mut self, v: &Value) -> Result<CCond, MaterialError> {
        match v {
            Value::String(s) => {
                let full = full_key(s);
                let target = self
                    .conds
                    .get(&full)
                    .ok_or_else(|| MaterialError::MissingReference(full.clone()))?
                    .clone();
                self.compile_cond(&target)
            }
            Value::Object(o) => self.compile_cond_obj(o),
            _ => Err(MaterialError::NotObject(v.to_string())),
        }
    }

    fn compile_cond_obj(&mut self, o: &Map<String, Value>) -> Result<CCond, MaterialError> {
        let ty = o
            .get("type")
            .and_then(Value::as_str)
            .ok_or(MaterialError::MissingType)?;
        let ty = ty.strip_prefix("minecraft:").unwrap_or(ty);
        match ty {
            "biome" => {
                let biome_is = o
                    .get("biome_is")
                    .ok_or_else(|| MaterialError::MissingField("biome_is".to_string()))?;
                let mut mask: u128 = 0;
                match biome_is {
                    Value::String(s) => mask |= self.biome_bit(s)?,
                    Value::Array(a) => {
                        for e in a {
                            let s = e.as_str().ok_or_else(|| {
                                MaterialError::BadField("biome_is entry".to_string())
                            })?;
                            mask |= self.biome_bit(s)?;
                        }
                    }
                    _ => return Err(MaterialError::BadField("biome_is".to_string())),
                }
                Ok(CCond::Biome(mask))
            }
            "not" => {
                let inv = o
                    .get("invert")
                    .ok_or_else(|| MaterialError::MissingField("invert".to_string()))?
                    .clone();
                Ok(CCond::Not(Box::new(self.compile_cond(&inv)?)))
            }
            "noise_threshold" => {
                let noise = o
                    .get("noise")
                    .and_then(Value::as_str)
                    .ok_or_else(|| MaterialError::MissingField("noise".to_string()))?;
                let key = full_key(noise);
                let stack = self
                    .noises
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| MaterialError::UnknownNoise(key.clone()))?;
                let min = o
                    .get("min_threshold")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| MaterialError::MissingField("min_threshold".to_string()))?;
                let max = o
                    .get("max_threshold")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| MaterialError::MissingField("max_threshold".to_string()))?;
                let is_3d = o.get("is_3d").and_then(Value::as_bool).unwrap_or(false);
                Ok(CCond::NoiseThreshold {
                    stack,
                    min,
                    max,
                    is_3d,
                })
            }
            "vertical_gradient" => {
                let random_name = o
                    .get("random_name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| MaterialError::MissingField("random_name".to_string()))?;
                let a = self.resolve_anchor(o.get("true_at_and_below").ok_or_else(|| {
                    MaterialError::MissingField("true_at_and_below".to_string())
                })?)?;
                let c = self.resolve_anchor(o.get("false_at_and_above").ok_or_else(|| {
                    MaterialError::MissingField("false_at_and_above".to_string())
                })?)?;
                let factory = self
                    .factory
                    .from_hash_of(&full_key(random_name))
                    .fork_positional();
                Ok(CCond::VerticalGradient { factory, a, c })
            }
            "y_above" => {
                let anchor = self.resolve_anchor(
                    o.get("anchor")
                        .ok_or_else(|| MaterialError::MissingField("anchor".to_string()))?,
                )?;
                let mult = o
                    .get("surface_depth_multiplier")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        MaterialError::MissingField("surface_depth_multiplier".to_string())
                    })? as i32;
                let add_stone_depth = o
                    .get("add_stone_depth")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(CCond::YAbove {
                    anchor,
                    mult,
                    add_stone_depth,
                })
            }
            "water" => {
                let offset = o
                    .get("offset")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| MaterialError::MissingField("offset".to_string()))?
                    as i32;
                let mult = o
                    .get("surface_depth_multiplier")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        MaterialError::MissingField("surface_depth_multiplier".to_string())
                    })? as i32;
                let add_stone_depth = o
                    .get("add_stone_depth")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(CCond::Water {
                    offset,
                    mult,
                    add_stone_depth,
                })
            }
            "stone_depth" => {
                let offset = o
                    .get("offset")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| MaterialError::MissingField("offset".to_string()))?
                    as i32;
                let add_surface_depth = o
                    .get("add_surface_depth")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let secondary = o
                    .get("secondary_depth_range")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        MaterialError::MissingField("secondary_depth_range".to_string())
                    })? as i32;
                let ceiling = matches!(
                    o.get("surface_type").and_then(Value::as_str),
                    Some("ceiling")
                );
                Ok(CCond::StoneDepth {
                    offset,
                    add_surface_depth,
                    secondary,
                    ceiling,
                })
            }
            "steep" => Ok(CCond::Steep),
            "hole" => Ok(CCond::Hole),
            "above_preliminary_surface" => Ok(CCond::AbovePreliminarySurface),
            "temperature" => Ok(CCond::Temperature),
            other => Err(MaterialError::UnknownType(other.to_string())),
        }
    }
}
