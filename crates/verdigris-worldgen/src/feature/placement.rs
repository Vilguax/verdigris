use std::sync::OnceLock;

use serde_json::Value;

use crate::feature::error::{FeatureError, strip_ns};
use crate::feature::numbers::{HeightCtx, HeightProvider, IntProvider};
use crate::feature::palette::{block_state_range, fluid_id_of_state, physics_of_state};
use crate::feature::region::{Heightmap, Region, WORLD_MAX_Y_EXCLUSIVE, WORLD_MIN_Y};
use crate::feature::tags::{fluid_name, resolve_block_tag, resolve_fluid_tag};
use crate::material::Simplex;

pub type Pos = (i32, i32, i32);

fn biome_info_noise() -> &'static Simplex {
    static S: OnceLock<Simplex> = OnceLock::new();
    S.get_or_init(|| Simplex::from_lcg(2345))
}

pub fn biome_info_value(x: i32, z: i32, factor: f64) -> f64 {
    let g = biome_info_noise().value(f64::from(x) / factor, f64::from(z) / factor);
    f64::from(g)
}

#[derive(Clone, Debug)]
pub enum BlockPredicate {
    True,
    Not(Box<BlockPredicate>),
    AllOf(Vec<BlockPredicate>),
    AnyOf(Vec<BlockPredicate>),
    MatchingBlocks {
        states: Vec<(u32, u32)>,
        offset: Pos,
    },
    MatchingBlockTag {
        states: Vec<(u32, u32)>,
        offset: Pos,
    },
    MatchingFluids {
        fluids: Vec<String>,
        offset: Pos,
    },
    Solid {
        offset: Pos,
    },
    Replaceable {
        offset: Pos,
    },
    HasSturdyFace {
        direction: String,
        offset: Pos,
    },
    WouldSurvive {
        offset: Pos,
    },
    InsideWorldBounds {
        offset: Pos,
    },
    Unobstructed,
}

impl BlockPredicate {
    pub fn compile(v: &Value) -> Result<BlockPredicate, FeatureError> {
        Self::compile_depth(v, 0)
    }

    fn compile_depth(v: &Value, depth: usize) -> Result<BlockPredicate, FeatureError> {
        if depth >= MAX_PREDICATE_DEPTH {
            return Err(FeatureError::DepthLimit("block_predicate".to_string()));
        }
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("block_predicate".to_string()))?;
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .ok_or_else(|| FeatureError::MissingField("block_predicate.type".to_string()))?;
        let offset = parse_offset(obj);
        match ty {
            "true" => Ok(BlockPredicate::True),
            "unobstructed" | "volume_match" => Ok(BlockPredicate::Unobstructed),
            "not" => Ok(BlockPredicate::Not(Box::new(
                BlockPredicate::compile_depth(
                    obj.get("predicate")
                        .ok_or_else(|| FeatureError::MissingField("predicate".to_string()))?,
                    depth + 1,
                )?,
            ))),
            "all_of" => Ok(BlockPredicate::AllOf(compile_list(
                obj,
                "predicates",
                depth,
            )?)),
            "any_of" => Ok(BlockPredicate::AnyOf(compile_list(
                obj,
                "predicates",
                depth,
            )?)),
            "matching_blocks" => Ok(BlockPredicate::MatchingBlocks {
                states: names_to_ranges(&string_list(obj, "blocks")),
                offset,
            }),
            "matching_block_tag" => {
                let tag = obj
                    .get("tag")
                    .and_then(Value::as_str)
                    .ok_or_else(|| FeatureError::MissingField("tag".to_string()))?;
                Ok(BlockPredicate::MatchingBlockTag {
                    states: names_to_ranges(&resolve_block_tag(tag)),
                    offset,
                })
            }
            "matching_fluids" => Ok(BlockPredicate::MatchingFluids {
                fluids: resolve_fluid_names(&string_list(obj, "fluids")),
                offset,
            }),
            "solid" => Ok(BlockPredicate::Solid { offset }),
            "replaceable" => Ok(BlockPredicate::Replaceable { offset }),
            "has_sturdy_face" => {
                let direction = obj
                    .get("direction")
                    .and_then(Value::as_str)
                    .map(strip_ns)
                    .unwrap_or("up")
                    .to_string();
                Ok(BlockPredicate::HasSturdyFace { direction, offset })
            }
            "would_survive" => Ok(BlockPredicate::WouldSurvive { offset }),
            "inside_world_bounds" => Ok(BlockPredicate::InsideWorldBounds { offset }),
            other => Err(FeatureError::UnknownType {
                namespace: "block_predicate".to_string(),
                name: other.to_string(),
            }),
        }
    }

    pub fn test<R: Region>(
        &self,
        region: &R,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<bool, FeatureError> {
        match self {
            BlockPredicate::True | BlockPredicate::Unobstructed => Ok(true),
            BlockPredicate::Not(inner) => Ok(!inner.test(region, x, y, z)?),
            BlockPredicate::AllOf(list) => {
                for p in list {
                    if !p.test(region, x, y, z)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            BlockPredicate::AnyOf(list) => {
                for p in list {
                    if p.test(region, x, y, z)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            BlockPredicate::MatchingBlocks { states, offset }
            | BlockPredicate::MatchingBlockTag { states, offset } => {
                let (ox, oy, oz) = *offset;
                let s = u32::from(region.read_block(x + ox, y + oy, z + oz)?);
                Ok(states.iter().any(|(a, b)| (*a..*b).contains(&s)))
            }
            BlockPredicate::MatchingFluids { fluids, offset } => {
                let (ox, oy, oz) = *offset;
                let s = u32::from(region.read_block(x + ox, y + oy, z + oz)?);
                match fluid_name(fluid_id_of_state(s)) {
                    Some(name) => Ok(fluids.contains(&name)),
                    None => Ok(false),
                }
            }
            BlockPredicate::Solid { offset } => {
                let (ox, oy, oz) = *offset;
                let s = u32::from(region.read_block(x + ox, y + oy, z + oz)?);
                Ok(physics_of_state(s).is_legacy_solid())
            }
            BlockPredicate::Replaceable { offset } => {
                let (ox, oy, oz) = *offset;
                let s = u32::from(region.read_block(x + ox, y + oy, z + oz)?);
                Ok(physics_of_state(s).is_replaceable())
            }
            BlockPredicate::HasSturdyFace { direction, offset } => {
                let (ox, oy, oz) = *offset;
                let s = u32::from(region.read_block(x + ox, y + oy, z + oz)?);
                Ok(physics_of_state(s).sturdy_full(direction))
            }
            BlockPredicate::WouldSurvive { offset } => {
                let (ox, oy, oz) = *offset;
                let below = u32::from(region.read_block(x + ox, y + oy - 1, z + oz)?);
                Ok(physics_of_state(below).is_legacy_solid())
            }
            BlockPredicate::InsideWorldBounds { offset } => {
                let (ox, oy, oz) = *offset;
                let ty = y + oy;
                Ok((WORLD_MIN_Y..WORLD_MAX_Y_EXCLUSIVE).contains(&ty)
                    && region.read_block(x + ox, ty, z + oz).is_ok())
            }
        }
    }
}

const MAX_PREDICATE_DEPTH: usize = 64;

fn parse_offset(obj: &serde_json::Map<String, Value>) -> Pos {
    match obj.get("offset").and_then(Value::as_array) {
        Some(a) if a.len() == 3 => {
            let g = |i: usize| a[i].as_i64().unwrap_or(0) as i32;
            (g(0), g(1), g(2))
        }
        _ => (0, 0, 0),
    }
}

fn names_to_ranges(names: &[String]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for n in names {
        if let Some(sub) = n.strip_prefix('#') {
            for m in resolve_block_tag(sub) {
                if let Some(r) = block_state_range(&m) {
                    out.push(r);
                }
            }
        } else if let Some(r) = block_state_range(n) {
            out.push(r);
        }
    }
    out
}

fn resolve_fluid_names(names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for n in names {
        if let Some(sub) = n.strip_prefix('#') {
            out.extend(resolve_fluid_tag(sub));
        } else {
            out.push(n.clone());
        }
    }
    out
}

fn string_list(obj: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    match obj.get(key) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn compile_list(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    depth: usize,
) -> Result<Vec<BlockPredicate>, FeatureError> {
    let arr = obj
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| FeatureError::MissingField(key.to_string()))?;
    let mut out = Vec::with_capacity(arr.len());
    for p in arr {
        out.push(BlockPredicate::compile_depth(p, depth + 1)?);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub enum Placement {
    Count(IntProvider),
    CountOnEveryLayer(IntProvider),
    NoiseThresholdCount {
        noise_level: f64,
        below: i32,
        above: i32,
    },
    NoiseBasedCount {
        noise_factor: f64,
        noise_offset: f64,
        ratio: i32,
    },
    RarityFilter(i32),
    RandomChance(f64),
    InSquare,
    HeightRange(HeightProvider),
    Heightmap(Heightmap),
    Offset(IntProvider, IntProvider, IntProvider),
    Biome,
    BlockPredicateFilter(BlockPredicate),
    SurfaceRelativeThresholdFilter {
        heightmap: Heightmap,
        min: i32,
        max: i32,
    },
    SurfaceWaterDepthFilter {
        max_depth: i32,
    },
    EnvironmentScan {
        up: bool,
        target: BlockPredicate,
        allowed: Option<BlockPredicate>,
        max_steps: i32,
    },
    RandomlySelected(Vec<Placement>),
    FixedPlacement(Vec<Pos>),
    Cuboid {
        include_edges: bool,
        include_interior: bool,
        xz_size: IntProvider,
        y_size: IntProvider,
    },
}

impl Placement {
    pub fn compile(
        spec_ty: &str,
        config: &Value,
        ctx: &HeightCtx,
    ) -> Result<Placement, FeatureError> {
        let obj = config.as_object();
        match spec_ty {
            "count" => Ok(Placement::Count(count_provider(obj)?)),
            "count_on_every_layer" => Ok(Placement::CountOnEveryLayer(count_provider(obj)?)),
            "noise_threshold_count" => Ok(Placement::NoiseThresholdCount {
                noise_level: field_f64(obj, "noise_level").unwrap_or(0.0),
                below: field_i64(obj, "below_noise").unwrap_or(0) as i32,
                above: field_i64(obj, "above_noise").unwrap_or(0) as i32,
            }),
            "noise_based_count" => Ok(Placement::NoiseBasedCount {
                noise_factor: field_f64(obj, "noise_factor").unwrap_or(1.0),
                noise_offset: field_f64(obj, "noise_offset").unwrap_or(0.0),
                ratio: field_i64(obj, "noise_to_count_ratio").unwrap_or(0) as i32,
            }),
            "rarity_filter" => Ok(Placement::RarityFilter(
                field_i64(obj, "chance").unwrap_or(1) as i32,
            )),
            "random_chance" => Ok(Placement::RandomChance(
                field_f64(obj, "chance").unwrap_or(1.0),
            )),
            "in_square" => Ok(Placement::InSquare),
            "height_range" => {
                let h = obj
                    .and_then(|o| o.get("height"))
                    .ok_or_else(|| FeatureError::MissingField("height".to_string()))?;
                Ok(Placement::HeightRange(HeightProvider::parse(h, ctx)?))
            }
            "heightmap" => Ok(Placement::Heightmap(parse_heightmap(obj)?)),
            "offset" => {
                let x = obj.and_then(|o| o.get("x"));
                let y = obj.and_then(|o| o.get("y"));
                let z = obj.and_then(|o| o.get("z"));
                Ok(Placement::Offset(
                    opt_int_provider(x)?,
                    opt_int_provider(y)?,
                    opt_int_provider(z)?,
                ))
            }
            "biome" => Ok(Placement::Biome),
            "block_predicate_filter" => {
                let pred = obj
                    .and_then(|o| o.get("predicate"))
                    .ok_or_else(|| FeatureError::MissingField("predicate".to_string()))?;
                Ok(Placement::BlockPredicateFilter(BlockPredicate::compile(
                    pred,
                )?))
            }
            "surface_relative_threshold_filter" => Ok(Placement::SurfaceRelativeThresholdFilter {
                heightmap: parse_heightmap(obj)?,
                min: field_i64(obj, "min_inclusive")
                    .map(|v| v as i32)
                    .unwrap_or(i32::MIN),
                max: field_i64(obj, "max_inclusive")
                    .map(|v| v as i32)
                    .unwrap_or(i32::MAX),
            }),
            "surface_water_depth_filter" => Ok(Placement::SurfaceWaterDepthFilter {
                max_depth: field_i64(obj, "max_water_depth").unwrap_or(0) as i32,
            }),
            "environment_scan" => {
                let o =
                    obj.ok_or_else(|| FeatureError::BadField("environment_scan".to_string()))?;
                let up = match o.get("direction_of_search").and_then(Value::as_str) {
                    Some("up") => true,
                    Some("down") => false,
                    _ => {
                        return Err(FeatureError::MissingField(
                            "direction_of_search".to_string(),
                        ));
                    }
                };
                let target =
                    BlockPredicate::compile(o.get("target_condition").ok_or_else(|| {
                        FeatureError::MissingField("target_condition".to_string())
                    })?)?;
                let allowed = match o.get("allowed_search_condition") {
                    Some(p) => Some(BlockPredicate::compile(p)?),
                    None => None,
                };
                let max_steps = field_i64(obj, "max_steps").unwrap_or(1) as i32;
                Ok(Placement::EnvironmentScan {
                    up,
                    target,
                    allowed,
                    max_steps,
                })
            }
            "randomly_selected" => {
                let arr = obj
                    .and_then(|o| o.get("placements"))
                    .and_then(Value::as_array)
                    .ok_or_else(|| FeatureError::MissingField("placements".to_string()))?;
                let mut out = Vec::with_capacity(arr.len());
                for m in arr {
                    let mo = m
                        .as_object()
                        .ok_or_else(|| FeatureError::BadField("placement".to_string()))?;
                    let ty = mo
                        .get("type")
                        .and_then(Value::as_str)
                        .map(strip_ns)
                        .ok_or_else(|| FeatureError::MissingField("placement.type".to_string()))?;
                    out.push(Placement::compile(ty, m, ctx)?);
                }
                Ok(Placement::RandomlySelected(out))
            }
            "fixed_placement" => {
                let arr = obj
                    .and_then(|o| o.get("positions"))
                    .and_then(Value::as_array)
                    .ok_or_else(|| FeatureError::MissingField("positions".to_string()))?;
                let mut out = Vec::with_capacity(arr.len());
                for p in arr {
                    let t = p
                        .as_array()
                        .filter(|t| t.len() == 3)
                        .ok_or_else(|| FeatureError::BadField("fixed position".to_string()))?;
                    let g = |i: usize| t[i].as_i64().unwrap_or(0) as i32;
                    out.push((g(0), g(1), g(2)));
                }
                Ok(Placement::FixedPlacement(out))
            }
            "cuboid" => {
                let o = obj.ok_or_else(|| FeatureError::BadField("cuboid".to_string()))?;
                Ok(Placement::Cuboid {
                    include_edges: o
                        .get("include_edges")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    include_interior: o
                        .get("include_interior")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    xz_size: IntProvider::parse(
                        o.get("xz_size")
                            .ok_or_else(|| FeatureError::MissingField("xz_size".to_string()))?,
                    )?,
                    y_size: IntProvider::parse(
                        o.get("y_size")
                            .ok_or_else(|| FeatureError::MissingField("y_size".to_string()))?,
                    )?,
                })
            }
            other => Err(FeatureError::UnknownType {
                namespace: "placement_modifier".to_string(),
                name: other.to_string(),
            }),
        }
    }
}

fn count_provider(
    obj: Option<&serde_json::Map<String, Value>>,
) -> Result<IntProvider, FeatureError> {
    let v = obj
        .and_then(|o| o.get("count"))
        .ok_or_else(|| FeatureError::MissingField("count".to_string()))?;
    IntProvider::parse(v)
}

fn opt_int_provider(v: Option<&Value>) -> Result<IntProvider, FeatureError> {
    match v {
        Some(val) => IntProvider::parse(val),
        None => Ok(IntProvider::Constant(0)),
    }
}

fn field_i64(obj: Option<&serde_json::Map<String, Value>>, key: &str) -> Option<i64> {
    obj.and_then(|o| o.get(key)).and_then(Value::as_i64)
}

fn field_f64(obj: Option<&serde_json::Map<String, Value>>, key: &str) -> Option<f64> {
    obj.and_then(|o| o.get(key)).and_then(Value::as_f64)
}

fn parse_heightmap(
    obj: Option<&serde_json::Map<String, Value>>,
) -> Result<Heightmap, FeatureError> {
    let name = obj
        .and_then(|o| o.get("heightmap"))
        .and_then(Value::as_str)
        .ok_or_else(|| FeatureError::MissingField("heightmap".to_string()))?;
    Ok(match name {
        "WORLD_SURFACE_WG" => Heightmap::WorldSurfaceWg,
        "WORLD_SURFACE" => Heightmap::WorldSurface,
        "OCEAN_FLOOR_WG" => Heightmap::OceanFloorWg,
        "OCEAN_FLOOR" => Heightmap::OceanFloor,
        "MOTION_BLOCKING" => Heightmap::MotionBlocking,
        "MOTION_BLOCKING_NO_LEAVES" => Heightmap::MotionBlockingNoLeaves,
        _ => Heightmap::WorldSurfaceWg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> HeightCtx {
        HeightCtx {
            min_y: -64,
            height: 384,
        }
    }

    #[test]
    fn compiles_in_square() {
        let m = Placement::compile("in_square", &serde_json::json!({}), &ctx()).unwrap();
        assert!(matches!(m, Placement::InSquare));
    }

    #[test]
    fn compiles_count_and_rarity() {
        let c = Placement::compile("count", &serde_json::json!({"count": 5}), &ctx()).unwrap();
        assert!(matches!(c, Placement::Count(_)));
        let r = Placement::compile("rarity_filter", &serde_json::json!({"chance": 32}), &ctx())
            .unwrap();
        assert!(matches!(r, Placement::RarityFilter(32)));
    }

    #[test]
    fn compiles_count_on_every_layer_and_fixed_placement() {
        let c = Placement::compile(
            "count_on_every_layer",
            &serde_json::json!({"count": 6}),
            &ctx(),
        )
        .unwrap();
        assert!(matches!(c, Placement::CountOnEveryLayer(_)));
        let f = Placement::compile(
            "fixed_placement",
            &serde_json::json!({"positions": [[100, 49, 0]]}),
            &ctx(),
        )
        .unwrap();
        match f {
            Placement::FixedPlacement(v) => assert_eq!(v, vec![(100, 49, 0)]),
            _ => panic!("expected fixed_placement"),
        }
    }

    #[test]
    fn unknown_modifier_is_rejected() {
        let e = Placement::compile("not_a_modifier", &serde_json::json!({}), &ctx());
        assert!(matches!(e, Err(FeatureError::UnknownType { .. })));
    }

    #[test]
    fn carving_mask_is_rejected_when_encountered() {
        let e = Placement::compile("carving_mask", &serde_json::json!({}), &ctx());
        assert!(matches!(e, Err(FeatureError::UnknownType { .. })));
    }

    #[test]
    fn unknown_predicate_is_rejected() {
        let bad = serde_json::json!({"type": "minecraft:not_a_predicate"});
        assert!(matches!(
            BlockPredicate::compile(&bad),
            Err(FeatureError::UnknownType { .. })
        ));
    }

    #[test]
    fn matching_blocks_compiles_to_state_ranges() {
        let v = serde_json::json!({"type": "matching_blocks", "blocks": "minecraft:stone"});
        let p = BlockPredicate::compile(&v).unwrap();
        match p {
            BlockPredicate::MatchingBlocks { states, offset } => {
                assert_eq!(offset, (0, 0, 0));
                assert!(!states.is_empty());
            }
            _ => panic!("expected matching_blocks"),
        }
    }
}
