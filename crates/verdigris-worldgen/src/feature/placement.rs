use serde_json::Value;

use crate::feature::error::{FeatureError, strip_ns};
use crate::feature::numbers::{HeightCtx, HeightProvider, IntProvider};
use crate::feature::region::{Heightmap, Region};

pub type Pos = (i32, i32, i32);

#[derive(Clone, Debug)]
pub enum BlockPredicate {
    True,
    MatchingBlocks(Vec<String>),
    MatchingBlockTag(String),
    MatchingFluids(Vec<String>),
    Solid,
    Replaceable,
    HasSturdyFace,
    InsideWorldBounds,
    WouldSurvive,
    All(Vec<BlockPredicate>),
    Any(Vec<BlockPredicate>),
    Not(Box<BlockPredicate>),
}

impl BlockPredicate {
    pub fn compile(v: &Value) -> Result<BlockPredicate, FeatureError> {
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("block_predicate".to_string()))?;
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .ok_or_else(|| FeatureError::MissingField("block_predicate.type".to_string()))?;
        match ty {
            "true" => Ok(BlockPredicate::True),
            "solid" => Ok(BlockPredicate::Solid),
            "replaceable" => Ok(BlockPredicate::Replaceable),
            "has_sturdy_face" => Ok(BlockPredicate::HasSturdyFace),
            "inside_world_bounds" => Ok(BlockPredicate::InsideWorldBounds),
            "would_survive" => Ok(BlockPredicate::WouldSurvive),
            "matching_block_tag" => Ok(BlockPredicate::MatchingBlockTag(
                obj.get("tag")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| FeatureError::MissingField("tag".to_string()))?,
            )),
            "matching_blocks" => Ok(BlockPredicate::MatchingBlocks(string_list(obj, "blocks"))),
            "matching_fluids" => Ok(BlockPredicate::MatchingFluids(string_list(obj, "fluids"))),
            "all_of" => Ok(BlockPredicate::All(compile_list(obj, "predicates")?)),
            "any_of" => Ok(BlockPredicate::Any(compile_list(obj, "predicates")?)),
            "not" => Ok(BlockPredicate::Not(Box::new(BlockPredicate::compile(
                obj.get("predicate")
                    .ok_or_else(|| FeatureError::MissingField("predicate".to_string()))?,
            )?))),
            "volume_match" | "unobstructed" => Ok(BlockPredicate::True),
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
            BlockPredicate::True => Ok(true),
            BlockPredicate::InsideWorldBounds => Ok(region.read_block(x, y, z).is_ok()),
            BlockPredicate::All(list) => {
                for p in list {
                    if !p.test(region, x, y, z)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            BlockPredicate::Any(list) => {
                for p in list {
                    if p.test(region, x, y, z)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            BlockPredicate::Not(inner) => Ok(!inner.test(region, x, y, z)?),
            _ => {
                let _ = region.read_block(x, y, z)?;
                Ok(true)
            }
        }
    }
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
) -> Result<Vec<BlockPredicate>, FeatureError> {
    let arr = obj
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| FeatureError::MissingField(key.to_string()))?;
    let mut out = Vec::with_capacity(arr.len());
    for p in arr {
        out.push(BlockPredicate::compile(p)?);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub enum Placement {
    Count(IntProvider),
    RarityFilter(i32),
    RandomChance(f64),
    InSquare,
    HeightRange(HeightProvider),
    Heightmap(Heightmap),
    Offset(IntProvider, IntProvider, IntProvider),
    Biome,
    BlockPredicateFilter(BlockPredicate),
    Passthrough(String),
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
            other => Ok(Placement::Passthrough(other.to_string())),
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

    #[test]
    fn compiles_in_square() {
        let ctx = HeightCtx {
            min_y: -64,
            height: 384,
        };
        let m = Placement::compile("in_square", &serde_json::json!({}), &ctx).unwrap();
        assert!(matches!(m, Placement::InSquare));
    }

    #[test]
    fn compiles_count_and_rarity() {
        let ctx = HeightCtx {
            min_y: -64,
            height: 384,
        };
        let c = Placement::compile("count", &serde_json::json!({"count": 5}), &ctx).unwrap();
        assert!(matches!(c, Placement::Count(_)));
        let r =
            Placement::compile("rarity_filter", &serde_json::json!({"chance": 32}), &ctx).unwrap();
        assert!(matches!(r, Placement::RarityFilter(32)));
    }

    #[test]
    fn unknown_modifier_compiles_as_passthrough() {
        let ctx = HeightCtx {
            min_y: -64,
            height: 384,
        };
        let m = Placement::compile("environment_scan", &serde_json::json!({}), &ctx).unwrap();
        assert!(matches!(m, Placement::Passthrough(_)));
    }
}
