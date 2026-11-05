use serde_json::Value;

use crate::arith::floor_i32;
use crate::carver::sincos::sin_table;
use crate::feature::error::{FeatureError, strip_ns};
use crate::feature::numbers::IntProvider;
use crate::feature::palette::{
    StatePalette, block_state_range, default_state_id, physics_of_state,
};
use crate::feature::placement::BlockPredicate;
use crate::feature::region::Region;
use crate::feature::rng::FeatureRng;
use crate::feature::tags::resolve_block_tag;

const PI_F32: f32 = std::f32::consts::PI;
const Y_MIN: i32 = -64;
const Y_MAX: i32 = 319;

const NEIGHBORS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

#[derive(Clone, Debug)]
pub enum RuleTest {
    AlwaysTrue,
    Not(Box<RuleTest>),
    AnyOf(Vec<RuleTest>),
    AllOf(Vec<RuleTest>),
    BlockMatch(Vec<(u32, u32)>),
    BlockStateMatch(u32),
    TagMatch(Vec<(u32, u32)>),
    RandomBlockMatch {
        states: Vec<(u32, u32)>,
        probability: f32,
    },
    RandomBlockStateMatch {
        state: u32,
        probability: f32,
    },
    HeightMatch {
        min: i32,
        max: i32,
    },
}

impl RuleTest {
    pub fn compile(v: &Value) -> Result<RuleTest, FeatureError> {
        Self::compile_depth(v, 0)
    }

    fn compile_depth(v: &Value, depth: usize) -> Result<RuleTest, FeatureError> {
        if depth >= 64 {
            return Err(FeatureError::DepthLimit("rule_test".to_string()));
        }
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("rule_test".to_string()))?;
        let ty = obj
            .get("predicate_type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .ok_or_else(|| FeatureError::MissingField("rule_test.predicate_type".to_string()))?;
        match ty {
            "always_true" => Ok(RuleTest::AlwaysTrue),
            "not" => {
                let inner = obj
                    .get("rule")
                    .ok_or_else(|| FeatureError::MissingField("rule".to_string()))?;
                Ok(RuleTest::Not(Box::new(Self::compile_depth(
                    inner,
                    depth + 1,
                )?)))
            }
            "any_of" => Ok(RuleTest::AnyOf(Self::compile_list(obj, depth)?)),
            "all_of" => Ok(RuleTest::AllOf(Self::compile_list(obj, depth)?)),
            "block_match" => {
                let name = obj
                    .get("block")
                    .and_then(Value::as_str)
                    .ok_or_else(|| FeatureError::MissingField("block".to_string()))?;
                Ok(RuleTest::BlockMatch(block_ranges(name)))
            }
            "blockstate_match" => {
                let state = obj
                    .get("block_state")
                    .ok_or_else(|| FeatureError::MissingField("block_state".to_string()))?;
                Ok(RuleTest::BlockStateMatch(state_id_of(state)?))
            }
            "tag_match" => {
                let tag = obj
                    .get("tag")
                    .and_then(Value::as_str)
                    .ok_or_else(|| FeatureError::MissingField("tag".to_string()))?;
                Ok(RuleTest::TagMatch(tag_ranges(tag)))
            }
            "random_block_match" => {
                let name = obj
                    .get("block")
                    .and_then(Value::as_str)
                    .ok_or_else(|| FeatureError::MissingField("block".to_string()))?;
                Ok(RuleTest::RandomBlockMatch {
                    states: block_ranges(name),
                    probability: float_of(obj, "probability")?,
                })
            }
            "random_blockstate_match" => {
                let state = obj
                    .get("block_state")
                    .ok_or_else(|| FeatureError::MissingField("block_state".to_string()))?;
                Ok(RuleTest::RandomBlockStateMatch {
                    state: state_id_of(state)?,
                    probability: float_of(obj, "probability")?,
                })
            }
            "height_match" => Ok(RuleTest::HeightMatch {
                min: int_of(obj, "min_inclusive")?,
                max: int_of(obj, "max_inclusive")?,
            }),
            other => Err(FeatureError::UnknownType {
                namespace: "rule_test".to_string(),
                name: other.to_string(),
            }),
        }
    }

    fn compile_list(
        obj: &serde_json::Map<String, Value>,
        depth: usize,
    ) -> Result<Vec<RuleTest>, FeatureError> {
        let arr = obj
            .get("rules")
            .and_then(Value::as_array)
            .ok_or_else(|| FeatureError::MissingField("rules".to_string()))?;
        let mut out = Vec::with_capacity(arr.len());
        for r in arr {
            out.push(Self::compile_depth(r, depth + 1)?);
        }
        Ok(out)
    }

    pub fn test<R: Region>(
        &self,
        region: &R,
        x: i32,
        y: i32,
        z: i32,
        rng: &mut FeatureRng,
    ) -> Result<bool, FeatureError> {
        match self {
            RuleTest::AlwaysTrue => Ok(true),
            RuleTest::Not(inner) => Ok(!inner.test(region, x, y, z, rng)?),
            RuleTest::AnyOf(list) => {
                for r in list {
                    if r.test(region, x, y, z, rng)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            RuleTest::AllOf(list) => {
                for r in list {
                    if !r.test(region, x, y, z, rng)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            RuleTest::BlockMatch(ranges) | RuleTest::TagMatch(ranges) => {
                let s = u32::from(region.read_block(x, y, z)?);
                Ok(in_ranges(ranges, s))
            }
            RuleTest::BlockStateMatch(state) => {
                let s = u32::from(region.read_block(x, y, z)?);
                Ok(s == *state)
            }
            RuleTest::RandomBlockMatch {
                states,
                probability,
            } => {
                let s = u32::from(region.read_block(x, y, z)?);
                if in_ranges(states, s) {
                    Ok(rng.next_float() < *probability)
                } else {
                    Ok(false)
                }
            }
            RuleTest::RandomBlockStateMatch { state, probability } => {
                let s = u32::from(region.read_block(x, y, z)?);
                if s == *state {
                    Ok(rng.next_float() < *probability)
                } else {
                    Ok(false)
                }
            }
            RuleTest::HeightMatch { min, max } => Ok((*min..=*max).contains(&y)),
        }
    }
}

#[derive(Clone, Debug)]
pub enum StateProvider {
    Simple(u32),
    Weighted(Vec<(i32, u32)>),
    RandomBlock(Vec<u32>),
    RuleBased {
        rules: Vec<(BlockPredicate, StateProvider)>,
        fallback: Option<Box<StateProvider>>,
    },
    Unsupported(String),
}

impl StateProvider {
    pub fn compile(v: &Value) -> Result<StateProvider, FeatureError> {
        match v {
            Value::String(s) => {
                if s.starts_with('#') {
                    return Err(FeatureError::BadField("state_provider tag".to_string()));
                }
                Ok(StateProvider::Simple(
                    default_state_id(s).ok_or_else(|| FeatureError::UnknownState(s.clone()))?,
                ))
            }
            Value::Object(o) => {
                if o.contains_key("Name") || o.contains_key("id") {
                    return Ok(StateProvider::Simple(state_id_of(v)?));
                }
                let ty = o
                    .get("type")
                    .and_then(Value::as_str)
                    .map(strip_ns)
                    .ok_or_else(|| FeatureError::MissingField("provider.type".to_string()))?;
                match ty {
                    "simple_state_provider" | "simple" => {
                        let state = o
                            .get("state")
                            .ok_or_else(|| FeatureError::MissingField("state".to_string()))?;
                        Ok(StateProvider::Simple(state_id_of(state)?))
                    }
                    "weighted_state_provider" | "weighted" => {
                        let arr = o
                            .get("entries")
                            .and_then(Value::as_array)
                            .ok_or_else(|| FeatureError::MissingField("entries".to_string()))?;
                        let mut out = Vec::with_capacity(arr.len());
                        for e in arr {
                            let eo = e.as_object().ok_or_else(|| {
                                FeatureError::BadField("weighted entry".to_string())
                            })?;
                            let weight =
                                eo.get("weight").and_then(Value::as_i64).ok_or_else(|| {
                                    FeatureError::MissingField("weight".to_string())
                                })? as i32;
                            let data = eo
                                .get("data")
                                .ok_or_else(|| FeatureError::MissingField("data".to_string()))?;
                            out.push((weight, state_id_of(data)?));
                        }
                        Ok(StateProvider::Weighted(out))
                    }
                    "random_block" => {
                        let names = match o.get("blocks") {
                            Some(Value::String(s)) => vec![s.clone()],
                            Some(Value::Array(a)) => a
                                .iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect(),
                            _ => Vec::new(),
                        };
                        let mut out = Vec::new();
                        for n in names {
                            if let Some(sub) = n.strip_prefix('#') {
                                for m in resolve_block_tag(sub) {
                                    if let Some(s) = default_state_id(&m) {
                                        out.push(s);
                                    }
                                }
                            } else if let Some(s) = default_state_id(&n) {
                                out.push(s);
                            }
                        }
                        Ok(StateProvider::RandomBlock(out))
                    }
                    "rule_based" => {
                        let arr = o
                            .get("rules")
                            .and_then(Value::as_array)
                            .ok_or_else(|| FeatureError::MissingField("rules".to_string()))?;
                        let mut rules = Vec::with_capacity(arr.len());
                        for r in arr {
                            let ro = r.as_object().ok_or_else(|| {
                                FeatureError::BadField("rule_based rule".to_string())
                            })?;
                            let pred =
                                BlockPredicate::compile(ro.get("if_true").ok_or_else(|| {
                                    FeatureError::MissingField("if_true".to_string())
                                })?)?;
                            let then =
                                StateProvider::compile(ro.get("then").ok_or_else(|| {
                                    FeatureError::MissingField("then".to_string())
                                })?)?;
                            rules.push((pred, then));
                        }
                        let fallback = match o.get("fallback") {
                            Some(fb) => Some(Box::new(StateProvider::compile(fb)?)),
                            None => None,
                        };
                        Ok(StateProvider::RuleBased { rules, fallback })
                    }
                    other => Ok(StateProvider::Unsupported(other.to_string())),
                }
            }
            _ => Err(FeatureError::BadField("state_provider".to_string())),
        }
    }

    pub fn optional_state<R: Region>(
        &self,
        region: &R,
        x: i32,
        y: i32,
        z: i32,
        rng: &mut FeatureRng,
    ) -> Result<Option<u32>, FeatureError> {
        match self {
            StateProvider::Simple(s) => Ok(Some(*s)),
            StateProvider::Weighted(entries) => {
                if entries.is_empty() {
                    return Ok(None);
                }
                let total: i32 = entries.iter().map(|(w, _)| *w).sum();
                if total <= 0 {
                    return Ok(None);
                }
                let mut roll = rng.next_int_bounded(total);
                for (w, s) in entries {
                    roll -= *w;
                    if roll < 0 {
                        return Ok(Some(*s));
                    }
                }
                Ok(Some(entries[entries.len() - 1].1))
            }
            StateProvider::RandomBlock(states) => {
                if states.is_empty() {
                    return Ok(None);
                }
                let pick = rng.next_int_bounded(states.len() as i32) as usize;
                Ok(Some(states[pick]))
            }
            StateProvider::RuleBased { rules, fallback } => {
                for (pred, then) in rules {
                    if pred.test(region, x, y, z)?
                        && let Some(s) = then.optional_state(region, x, y, z, rng)?
                    {
                        return Ok(Some(s));
                    }
                }
                match fallback {
                    Some(fb) => fb.optional_state(region, x, y, z, rng),
                    None => Ok(None),
                }
            }
            StateProvider::Unsupported(name) => {
                Err(FeatureError::NotYetImplemented(format!("provider {name}")))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct OreTarget {
    pub test: RuleTest,
    pub state: u32,
}

#[derive(Clone, Debug)]
pub struct OreConfig {
    pub size: i32,
    pub discard: f32,
    pub targets: Vec<OreTarget>,
}

impl OreConfig {
    pub fn compile(config: &Value) -> Result<OreConfig, FeatureError> {
        let obj = config
            .as_object()
            .ok_or_else(|| FeatureError::BadField("ore".to_string()))?;
        let size =
            obj.get("size")
                .and_then(Value::as_i64)
                .ok_or_else(|| FeatureError::MissingField("size".to_string()))? as i32;
        let discard = obj
            .get("discard_chance_on_air_exposure")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32;
        let arr = obj
            .get("targets")
            .and_then(Value::as_array)
            .ok_or_else(|| FeatureError::MissingField("targets".to_string()))?;
        let mut targets = Vec::with_capacity(arr.len());
        for t in arr {
            let to = t
                .as_object()
                .ok_or_else(|| FeatureError::BadField("ore target".to_string()))?;
            let test = RuleTest::compile(
                to.get("target")
                    .ok_or_else(|| FeatureError::MissingField("target".to_string()))?,
            )?;
            let state = state_id_of(
                to.get("state")
                    .ok_or_else(|| FeatureError::MissingField("state".to_string()))?,
            )?;
            targets.push(OreTarget { test, state });
        }
        Ok(OreConfig {
            size,
            discard,
            targets,
        })
    }
}

#[derive(Clone, Debug)]
pub struct DiskConfig {
    pub half_height: i32,
    pub radius: IntProvider,
    pub provider: StateProvider,
    pub target: BlockPredicate,
}

impl DiskConfig {
    pub fn compile(config: &Value) -> Result<DiskConfig, FeatureError> {
        let obj = config
            .as_object()
            .ok_or_else(|| FeatureError::BadField("disk".to_string()))?;
        let half_height = obj
            .get("half_height")
            .and_then(Value::as_i64)
            .ok_or_else(|| FeatureError::MissingField("half_height".to_string()))?
            as i32;
        let radius = IntProvider::parse(
            obj.get("radius")
                .ok_or_else(|| FeatureError::MissingField("radius".to_string()))?,
        )?;
        let provider = StateProvider::compile(
            obj.get("state_provider")
                .ok_or_else(|| FeatureError::MissingField("state_provider".to_string()))?,
        )?;
        let target = BlockPredicate::compile(
            obj.get("target")
                .ok_or_else(|| FeatureError::MissingField("target".to_string()))?,
        )?;
        Ok(DiskConfig {
            half_height,
            radius,
            provider,
            target,
        })
    }
}

#[derive(Default)]
pub struct OreScratch {
    xc: Vec<f64>,
    yc: Vec<f64>,
    zc: Vec<f64>,
    rad: Vec<f64>,
    mask: Vec<u64>,
    side: i64,
    ox: i32,
    oy: i32,
    oz: i32,
}

impl OreScratch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn capacities(&self) -> (usize, usize, usize, usize, usize) {
        (
            self.xc.capacity(),
            self.yc.capacity(),
            self.zc.capacity(),
            self.rad.capacity(),
            self.mask.capacity(),
        )
    }

    fn reset_points(&mut self, n: usize) {
        self.xc.clear();
        self.yc.clear();
        self.zc.clear();
        self.rad.clear();
        self.xc.resize(n, 0.0);
        self.yc.resize(n, 0.0);
        self.zc.resize(n, 0.0);
        self.rad.resize(n, 0.0);
    }

    fn reset_mask(&mut self, side: i64, ox: i32, oy: i32, oz: i32) {
        self.side = side.max(1);
        self.ox = ox;
        self.oy = oy;
        self.oz = oz;
        let bits = (self.side * self.side * self.side) as usize;
        let words = bits.div_ceil(64);
        if self.mask.len() < words {
            self.mask.resize(words, 0);
        }
        for w in &mut self.mask[..words] {
            *w = 0;
        }
    }

    fn insert(&mut self, x: i32, y: i32, z: i32) -> bool {
        let dx = i64::from(x - self.ox);
        let dy = i64::from(y - self.oy);
        let dz = i64::from(z - self.oz);
        if dx < 0 || dy < 0 || dz < 0 || dx >= self.side || dy >= self.side || dz >= self.side {
            return true;
        }
        let idx = (dx + self.side * (dy + self.side * dz)) as usize;
        let word = idx / 64;
        let bit = 1u64 << (idx % 64);
        if self.mask[word] & bit != 0 {
            return false;
        }
        self.mask[word] |= bit;
        true
    }
}

pub fn ore_metrics(size: i32) -> (f32, i32, i32, i32, i32) {
    let sigma = size as f32 / 8.0;
    let rho = ((size as f32 / 16.0 * 2.0 + 1.0) / 2.0).ceil() as i32;
    let c = sigma.ceil() as i32;
    let t = 2 * (c + rho);
    let ty = 2 * (2 + rho);
    (sigma, rho, c, t, ty)
}

#[allow(clippy::needless_range_loop)]
pub fn run_ore<R: Region>(
    cfg: &OreConfig,
    region: &mut R,
    rng: &mut FeatureRng,
    scratch: &mut OreScratch,
    px: i32,
    py: i32,
    pz: i32,
) -> Result<bool, FeatureError> {
    let size = cfg.size;
    if size <= 0 {
        return Ok(false);
    }
    let theta = rng.next_float() * PI_F32;
    let sigma = size as f32 / 8.0;
    let rho = ((size as f32 / 16.0 * 2.0 + 1.0) / 2.0).ceil() as i32;

    let sin_t = f64::from(theta).sin();
    let cos_t = f64::from(theta).cos();
    let sig = f64::from(sigma);
    let x0 = f64::from(px) + sin_t * sig;
    let x1 = f64::from(px) - sin_t * sig;
    let z0 = f64::from(pz) + cos_t * sig;
    let z1 = f64::from(pz) - cos_t * sig;
    let y0 = f64::from(py + rng.next_int_bounded(3) - 2);
    let y1 = f64::from(py + rng.next_int_bounded(3) - 2);

    let c = sigma.ceil() as i32;
    let xs = px - c - rho;
    let ys = py - 2 - rho;
    let zs = pz - c - rho;
    let t = 2 * (c + rho);

    let mut altitude_ok = false;
    'alt: for x in xs..=xs + t {
        for z in zs..=zs + t {
            let floor =
                region.read_heightmap(crate::feature::region::Heightmap::OceanFloorWg, x, z)?;
            if ys <= floor {
                altitude_ok = true;
                break 'alt;
            }
        }
    }
    if !altitude_ok {
        return Ok(false);
    }

    let n = size as usize;
    scratch.reset_points(n);
    let table = sin_table();
    for i in 0..n {
        let si = i as f32 / size as f32;
        scratch.xc[i] = x0 + f64::from(si) * (x1 - x0);
        scratch.yc[i] = y0 + f64::from(si) * (y1 - y0);
        scratch.zc[i] = z0 + f64::from(si) * (z1 - z0);
        let e = rng.next_double() * f64::from(size) / 16.0;
        let st = table.sin_t(PI_F32 * si) + 1.0f32;
        scratch.rad[i] = (f64::from(st) * e + 1.0) / 2.0;
    }

    for i in 0..n.saturating_sub(1) {
        if scratch.rad[i] > 0.0 {
            for j in (i + 1)..n {
                if scratch.rad[j] > 0.0 {
                    let dx = scratch.xc[i] - scratch.xc[j];
                    let dy = scratch.yc[i] - scratch.yc[j];
                    let dz = scratch.zc[i] - scratch.zc[j];
                    let dr = scratch.rad[i] - scratch.rad[j];
                    if dr * dr > dx * dx + dy * dy + dz * dz {
                        if dr > 0.0 {
                            scratch.rad[j] = -1.0;
                        } else {
                            scratch.rad[i] = -1.0;
                        }
                    }
                }
            }
        }
    }

    scratch.reset_mask(i64::from(t) + 1, xs, ys, zs);
    let mut wrote = false;
    for i in 0..n {
        let r = scratch.rad[i];
        if r < 0.0 {
            continue;
        }
        let (xi, yi, zi) = (scratch.xc[i], scratch.yc[i], scratch.zc[i]);
        let x_min = floor_i32(xi - r).max(xs);
        let y_min = floor_i32(yi - r).max(ys);
        let z_min = floor_i32(zi - r).max(zs);
        let x_max = floor_i32(xi + r).max(x_min);
        let y_max = floor_i32(yi + r).max(y_min);
        let z_max = floor_i32(zi + r).max(z_min);
        for x in x_min..=x_max {
            let u = (f64::from(x) + 0.5 - xi) / r;
            if u * u >= 1.0 {
                continue;
            }
            for y in y_min..=y_max {
                let v = (f64::from(y) + 0.5 - yi) / r;
                if u * u + v * v >= 1.0 {
                    continue;
                }
                for z in z_min..=z_max {
                    let w = (f64::from(z) + 0.5 - zi) / r;
                    if u * u + v * v + w * w >= 1.0 || !(Y_MIN..=Y_MAX).contains(&y) {
                        continue;
                    }
                    if !scratch.insert(x, y, z) {
                        continue;
                    }
                    if place_ore_block(region, rng, &cfg.targets, cfg.discard, x, y, z)? {
                        wrote = true;
                    }
                }
            }
        }
    }
    Ok(wrote)
}

pub fn run_scattered_ore<R: Region>(
    cfg: &OreConfig,
    region: &mut R,
    rng: &mut FeatureRng,
    px: i32,
    py: i32,
    pz: i32,
) -> Result<bool, FeatureError> {
    let count = rng.next_int_bounded(cfg.size + 1);
    for i in 0..count {
        let m = i.min(7) as f32;
        let dx = java_round((rng.next_float() - rng.next_float()) * m);
        let dy = java_round((rng.next_float() - rng.next_float()) * m);
        let dz = java_round((rng.next_float() - rng.next_float()) * m);
        let x = px.wrapping_add(dx);
        let y = py.wrapping_add(dy);
        let z = pz.wrapping_add(dz);
        place_ore_block(region, rng, &cfg.targets, cfg.discard, x, y, z)?;
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn place_ore_block<R: Region>(
    region: &mut R,
    rng: &mut FeatureRng,
    targets: &[OreTarget],
    discard: f32,
    x: i32,
    y: i32,
    z: i32,
) -> Result<bool, FeatureError> {
    for target in targets {
        if target.test.test(region, x, y, z, rng)? {
            if exposure_accept(region, rng, discard, x, y, z)? {
                return region.write_block(x, y, z, target.state as u16);
            }
            return Ok(false);
        }
    }
    Ok(false)
}

fn exposure_accept<R: Region>(
    region: &R,
    rng: &mut FeatureRng,
    discard: f32,
    x: i32,
    y: i32,
    z: i32,
) -> Result<bool, FeatureError> {
    if discard <= 0.0 {
        return Ok(true);
    }
    if discard >= 1.0 {
        return Ok(!any_neighbor_air(region, x, y, z)?);
    }
    if rng.next_float() >= discard {
        Ok(true)
    } else {
        Ok(!any_neighbor_air(region, x, y, z)?)
    }
}

fn any_neighbor_air<R: Region>(region: &R, x: i32, y: i32, z: i32) -> Result<bool, FeatureError> {
    for (dx, dy, dz) in NEIGHBORS {
        let ny = y + dy;
        if !(Y_MIN..=Y_MAX).contains(&ny) {
            return Ok(true);
        }
        let b = u32::from(region.read_block(x + dx, ny, z + dz)?);
        if physics_of_state(b).is_air() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn run_disk<R: Region>(
    cfg: &DiskConfig,
    region: &mut R,
    rng: &mut FeatureRng,
    px: i32,
    py: i32,
    pz: i32,
) -> Result<bool, FeatureError> {
    let radius = cfg.radius.sample(rng);
    let h1 = py + cfg.half_height;
    let h0 = py - cfg.half_height - 1;
    let mut placed = false;
    for z in pz - radius..=pz + radius {
        for x in px - radius..=px + radius {
            let dx = x - px;
            let dz = z - pz;
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            let mut y = h1;
            while y > h0 {
                if cfg.target.test(region, x, y, z)?
                    && let Some(state) = cfg.provider.optional_state(region, x, y, z, rng)?
                    && region.write_block(x, y, z, state as u16)?
                {
                    placed = true;
                }
                y -= 1;
            }
        }
    }
    Ok(placed)
}

fn java_round(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

fn in_ranges(ranges: &[(u32, u32)], s: u32) -> bool {
    ranges.iter().any(|(a, b)| (*a..*b).contains(&s))
}

fn block_ranges(name: &str) -> Vec<(u32, u32)> {
    block_state_range(name).into_iter().collect()
}

fn tag_ranges(tag: &str) -> Vec<(u32, u32)> {
    let name = tag.strip_prefix('#').unwrap_or(tag);
    resolve_block_tag(name)
        .iter()
        .filter_map(|m| block_state_range(m))
        .collect()
}

fn state_id_of(v: &Value) -> Result<u32, FeatureError> {
    match v {
        Value::String(s) => {
            default_state_id(s).ok_or_else(|| FeatureError::UnknownState(s.clone()))
        }
        Value::Object(_) => {
            let mut pal = StatePalette::new();
            let idx = pal.intern_state(v)?;
            pal.entry(idx)
                .map(|e| e.state_id)
                .ok_or_else(|| FeatureError::BadField("state".to_string()))
        }
        _ => Err(FeatureError::BadField("state".to_string())),
    }
}

fn int_of(obj: &serde_json::Map<String, Value>, key: &str) -> Result<i32, FeatureError> {
    obj.get(key)
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .ok_or_else(|| FeatureError::MissingField(key.to_string()))
}

fn float_of(obj: &serde_json::Map<String, Value>, key: &str) -> Result<f32, FeatureError> {
    obj.get(key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .ok_or_else(|| FeatureError::MissingField(key.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ore_metrics_match_11_1() {
        let (sigma, rho, c, t, ty) = ore_metrics(9);
        assert_eq!(sigma.to_bits(), 1.125f32.to_bits());
        assert_eq!((rho, c, t, ty), (2, 2, 8, 8));

        let (sigma, rho, c, t, ty) = ore_metrics(64);
        assert_eq!(sigma.to_bits(), 8.0f32.to_bits());
        assert_eq!((rho, c, t, ty), (5, 8, 26, 14));

        let (sigma, rho, c, t, ty) = ore_metrics(17);
        assert_eq!(sigma.to_bits(), 2.125f32.to_bits());
        assert_eq!((rho, c, t, ty), (2, 3, 10, 8));
    }

    #[test]
    fn disk_radius_two_covers_thirteen_columns() {
        let mut count = 0;
        let r = 2i32;
        for z in -r..=r {
            for x in -r..=r {
                if x * x + z * z <= r * r {
                    count += 1;
                }
            }
        }
        assert_eq!(count, 13);
    }

    #[test]
    fn java_round_half_up() {
        assert_eq!(java_round(0.5), 1);
        assert_eq!(java_round(-0.5), 0);
        assert_eq!(java_round(2.49), 2);
        assert_eq!(java_round(-2.5), -2);
    }

    #[test]
    fn disk_poses_provider_state_over_target() {
        use crate::feature::region::TestRegion;
        let dirt = default_state_id("minecraft:dirt").unwrap() as u16;
        let clay = default_state_id("minecraft:clay").unwrap() as u16;
        let cfg = DiskConfig::compile(&serde_json::json!({
            "half_height": 0,
            "radius": 0,
            "state_provider": {"id": "minecraft:clay"},
            "target": {"type": "minecraft:matching_blocks", "blocks": "minecraft:dirt"}
        }))
        .unwrap();
        let mut region = TestRegion::new((0, 0));
        region.set_block_preset(0, 70, 0, dirt);
        let mut rng = FeatureRng::new(1);
        let placed = run_disk(&cfg, &mut region, &mut rng, 0, 70, 0).unwrap();
        assert!(placed, "disk must pose clay over the dirt column");
        assert_eq!(region.written(0, 70, 0), Some(clay));
    }

    #[test]
    fn disk_skips_non_target_blocks() {
        use crate::feature::region::TestRegion;
        let cfg = DiskConfig::compile(&serde_json::json!({
            "half_height": 0,
            "radius": 0,
            "state_provider": {"id": "minecraft:clay"},
            "target": {"type": "minecraft:matching_blocks", "blocks": "minecraft:dirt"}
        }))
        .unwrap();
        let mut region = TestRegion::new((0, 0));
        let mut rng = FeatureRng::new(1);
        let placed = run_disk(&cfg, &mut region, &mut rng, 0, 70, 0).unwrap();
        assert!(!placed, "air is not a disk target");
        assert_eq!(region.written(0, 70, 0), None);
    }

    #[test]
    fn ore_fills_targets_within_stone_box() {
        use crate::feature::region::TestRegion;
        let iron = default_state_id("minecraft:iron_ore").unwrap() as u16;
        let stone = default_state_id("minecraft:stone").unwrap() as u16;
        let cfg = OreConfig::compile(&serde_json::json!({
            "size": 9,
            "discard_chance_on_air_exposure": 0,
            "targets": [{
                "state": "minecraft:iron_ore",
                "target": {"predicate_type": "minecraft:always_true"}
            }]
        }))
        .unwrap();
        let mut region = TestRegion::new((0, 0));
        for x in 2..=14 {
            for z in 2..=14 {
                region.set_heightmap(x, z, 200);
                for y in 48..=72 {
                    region.set_block_preset(x, y, z, stone);
                }
            }
        }
        let mut rng = FeatureRng::new(42);
        let mut scratch = OreScratch::new();
        let wrote = run_ore(&cfg, &mut region, &mut rng, &mut scratch, 8, 60, 8).unwrap();
        assert!(wrote, "ore must write at least one block in a solid box");
        let mut iron_count = 0;
        for x in 2..=14 {
            for z in 2..=14 {
                for y in 48..=72 {
                    if region.written(x, y, z) == Some(iron) {
                        iron_count += 1;
                    }
                }
            }
        }
        assert!(
            iron_count > 0,
            "expected iron ore blocks, found {iron_count}"
        );
    }
}
