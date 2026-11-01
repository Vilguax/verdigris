use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde_json::Value;

use crate::biome::biome_names;
use crate::feature::error::FeatureError;
use crate::feature::numbers::HeightCtx;
use crate::feature::order::{GlobalOrder, build_order, overworld_biomes_from_features};
use crate::feature::palette::physics_of_state;
use crate::feature::placement::{Placement, Pos, biome_info_value};
use crate::feature::region::{Heightmap, Region, WORLD_MAX_Y_EXCLUSIVE, WORLD_MIN_Y};
use crate::feature::registry::{
    ConfiguredFeature, FEATURES_JSON, FeatureRef, FeatureRegistry, PlacedFeature, PlacedRef,
};
use crate::feature::rng::FeatureRng;
use crate::feature::zoom::biome_at_block;
use crate::material::zoom_seed;

const WORLD_CTX: HeightCtx = HeightCtx {
    min_y: -64,
    height: 384,
};

const MAX_COMPILE_DEPTH: usize = 64;

const BIOME_CELL_Y_MIN: i32 = -16;
const BIOME_CELL_Y_MAX_EXCLUSIVE: i32 = 80;

struct RunCtx<'a, R: Region> {
    region: &'a R,
    zoom_seed: i64,
    feature_biomes: &'a BTreeSet<u16>,
}

#[derive(Clone, Debug)]
pub struct CompiledPlaced {
    placement: Vec<Placement>,
    feature: CompiledFeature,
}

#[derive(Clone, Debug)]
pub enum CompiledFeature {
    NoOp,
    Sequence(Vec<CompiledPlaced>),
    Overlay(Vec<CompiledPlaced>),
    SimpleRandomSelector(Vec<CompiledPlaced>),
    WeightedRandomSelector(Vec<(i32, CompiledPlaced)>),
    RandomBooleanSelector(Box<CompiledPlaced>, Box<CompiledPlaced>),
    RandomSelector(Box<CompiledPlaced>, Vec<(f32, CompiledPlaced)>),
    Unimplemented(String),
}

#[derive(Default)]
pub struct DecorationContext {
    placed: Vec<Pos>,
    unimplemented: usize,
}

impl DecorationContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn placed(&self) -> &[Pos] {
        &self.placed
    }

    pub fn placed_capacity(&self) -> usize {
        self.placed.capacity()
    }

    pub fn unimplemented_hits(&self) -> usize {
        self.unimplemented
    }

    fn reset(&mut self) {
        self.placed.clear();
        self.unimplemented = 0;
    }
}

pub struct Decoration {
    order: GlobalOrder,
    compiled: BTreeMap<String, CompiledPlaced>,
    allowed_biomes: BTreeMap<String, BTreeSet<u16>>,
}

impl Decoration {
    pub fn embedded() -> Result<Self, FeatureError> {
        let root: Value =
            serde_json::from_str(FEATURES_JSON).map_err(|e| FeatureError::Json(e.to_string()))?;
        let registry = FeatureRegistry::from_value(&root)?;
        Self::build(&registry, &root)
    }

    pub fn build(registry: &FeatureRegistry, root: &Value) -> Result<Self, FeatureError> {
        let biomes = overworld_biomes_from_features(root)?;
        let order = build_order(&biomes)?;

        let mut compiled = BTreeMap::new();
        for (name, pf) in &registry.placed {
            compiled.insert(name.clone(), compile_placed(registry, pf, 0)?);
        }

        let names = biome_names();
        let mut allowed_biomes: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
        for (biome_name, steps) in &biomes {
            if let Some(id) = names.id_of(biome_name) {
                for step in steps {
                    for feat in step {
                        allowed_biomes.entry(feat.clone()).or_default().insert(id);
                    }
                }
            }
        }

        Ok(Decoration {
            order,
            compiled,
            allowed_biomes,
        })
    }

    pub fn order(&self) -> &GlobalOrder {
        &self.order
    }

    pub fn decorate_chunk<R: Region>(
        &self,
        region: &R,
        ctx: &mut DecorationContext,
        level_seed: i64,
        cx: i32,
        cz: i32,
    ) -> Result<(), FeatureError> {
        ctx.reset();
        let candidate = aggregate_stored_biomes(region, cx, cz)?;
        let seed = zoom_seed(level_seed);
        let min_x = cx.wrapping_mul(16);
        let min_z = cz.wrapping_mul(16);
        let mut rng = FeatureRng::new(0);
        let decoration_seed = rng.set_decoration_seed(level_seed, min_x, min_z);
        let empty: BTreeSet<u16> = BTreeSet::new();

        for step in 0..self.order.steps().len() {
            for (i, name) in self.order.step(step).iter().enumerate() {
                if !self.is_active(name, &candidate) {
                    continue;
                }
                let Some(compiled) = self.compiled.get(name) else {
                    continue;
                };
                rng.set_feature_seed(decoration_seed, i as i32, step as i32);
                let rc = RunCtx {
                    region,
                    zoom_seed: seed,
                    feature_biomes: self.allowed_biomes.get(name).unwrap_or(&empty),
                };
                run_placed(compiled, &rc, &mut rng, ctx, (min_x, 0, min_z))?;
            }
        }
        Ok(())
    }

    fn is_active(&self, name: &str, candidate: &BTreeSet<u16>) -> bool {
        match self.allowed_biomes.get(name) {
            Some(set) => set.iter().any(|b| candidate.contains(b)),
            None => false,
        }
    }
}

fn aggregate_stored_biomes<R: Region>(
    region: &R,
    cx: i32,
    cz: i32,
) -> Result<BTreeSet<u16>, FeatureError> {
    let mut set = BTreeSet::new();
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            let base_cx = cx.wrapping_add(dx).wrapping_mul(4);
            let base_cz = cz.wrapping_add(dz).wrapping_mul(4);
            for gx in 0..4 {
                for gz in 0..4 {
                    for qy in BIOME_CELL_Y_MIN..BIOME_CELL_Y_MAX_EXCLUSIVE {
                        set.insert(region.stored_biome(base_cx + gx, qy, base_cz + gz)?);
                    }
                }
            }
        }
    }
    Ok(set)
}

fn compile_placed(
    registry: &FeatureRegistry,
    pf: &PlacedFeature,
    depth: usize,
) -> Result<CompiledPlaced, FeatureError> {
    let mut placement = Vec::with_capacity(pf.placement.len());
    for spec in &pf.placement {
        placement.push(Placement::compile(&spec.ty, &spec.config, &WORLD_CTX)?);
    }
    let feature = compile_feature_ref(registry, &pf.feature, depth)?;
    Ok(CompiledPlaced { placement, feature })
}

fn compile_feature_ref(
    registry: &FeatureRegistry,
    r: &FeatureRef,
    depth: usize,
) -> Result<CompiledFeature, FeatureError> {
    if depth >= MAX_COMPILE_DEPTH {
        return Err(FeatureError::DepthLimit("feature".to_string()));
    }
    match r {
        FeatureRef::Named(n) => {
            let cf = registry
                .features
                .get(n)
                .ok_or_else(|| FeatureError::UnknownReference(n.clone()))?;
            compile_configured(registry, cf, depth + 1)
        }
        FeatureRef::Inline(cf) => compile_configured(registry, cf, depth + 1),
    }
}

fn compile_placed_ref(
    registry: &FeatureRegistry,
    r: &PlacedRef,
    depth: usize,
) -> Result<CompiledPlaced, FeatureError> {
    if depth >= MAX_COMPILE_DEPTH {
        return Err(FeatureError::DepthLimit("placed_feature".to_string()));
    }
    match r {
        PlacedRef::Named(n) => {
            let pf = registry
                .placed
                .get(n)
                .ok_or_else(|| FeatureError::UnknownReference(n.clone()))?;
            compile_placed(registry, pf, depth + 1)
        }
        PlacedRef::Inline(pf) => compile_placed(registry, pf, depth + 1),
    }
}

fn compile_configured(
    registry: &FeatureRegistry,
    cf: &ConfiguredFeature,
    depth: usize,
) -> Result<CompiledFeature, FeatureError> {
    match cf {
        ConfiguredFeature::NoOp => Ok(CompiledFeature::NoOp),
        ConfiguredFeature::Leaf { ty, .. } => Ok(CompiledFeature::Unimplemented(ty.clone())),
        ConfiguredFeature::Sequence(v) => {
            Ok(CompiledFeature::Sequence(compile_refs(registry, v, depth)?))
        }
        ConfiguredFeature::Overlay(v) => {
            Ok(CompiledFeature::Overlay(compile_refs(registry, v, depth)?))
        }
        ConfiguredFeature::SimpleRandomSelector(v) => Ok(CompiledFeature::SimpleRandomSelector(
            compile_refs(registry, v, depth)?,
        )),
        ConfiguredFeature::WeightedRandomSelector(v) => {
            let mut out = Vec::with_capacity(v.len());
            for (w, r) in v {
                out.push((*w, compile_placed_ref(registry, r, depth)?));
            }
            Ok(CompiledFeature::WeightedRandomSelector(out))
        }
        ConfiguredFeature::RandomBooleanSelector { if_true, if_false } => {
            Ok(CompiledFeature::RandomBooleanSelector(
                Box::new(compile_placed_ref(registry, if_true, depth)?),
                Box::new(compile_placed_ref(registry, if_false, depth)?),
            ))
        }
        ConfiguredFeature::RandomSelector { default, entries } => {
            let def = Box::new(compile_placed_ref(registry, default, depth)?);
            let mut out = Vec::with_capacity(entries.len());
            for (c, r) in entries {
                out.push((*c, compile_placed_ref(registry, r, depth)?));
            }
            Ok(CompiledFeature::RandomSelector(def, out))
        }
    }
}

fn compile_refs(
    registry: &FeatureRegistry,
    refs: &[PlacedRef],
    depth: usize,
) -> Result<Vec<CompiledPlaced>, FeatureError> {
    let mut out = Vec::with_capacity(refs.len());
    for r in refs {
        out.push(compile_placed_ref(registry, r, depth)?);
    }
    Ok(out)
}

fn run_placed<R: Region>(
    pf: &CompiledPlaced,
    rc: &RunCtx<R>,
    rng: &mut FeatureRng,
    ctx: &mut DecorationContext,
    origin: Pos,
) -> Result<bool, FeatureError> {
    run_modifiers(&pf.placement, &pf.feature, rc, rng, ctx, origin)
}

fn run_modifiers<R: Region>(
    mods: &[Placement],
    feature: &CompiledFeature,
    rc: &RunCtx<R>,
    rng: &mut FeatureRng,
    ctx: &mut DecorationContext,
    pos: Pos,
) -> Result<bool, FeatureError> {
    match mods.split_first() {
        None => place_feature(feature, rc, rng, ctx, pos),
        Some((head, tail)) => step(head, tail, feature, rc, rng, ctx, pos),
    }
}

#[allow(clippy::too_many_arguments)]
fn step<R: Region>(
    head: &Placement,
    tail: &[Placement],
    feature: &CompiledFeature,
    rc: &RunCtx<R>,
    rng: &mut FeatureRng,
    ctx: &mut DecorationContext,
    pos: Pos,
) -> Result<bool, FeatureError> {
    let mut any = false;
    match head {
        Placement::Count(p) => {
            let n = p.sample(rng).max(0);
            for _ in 0..n {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::CountOnEveryLayer(p) => {
            let mut layer = 0i32;
            loop {
                let mut placed_any = false;
                let mut i = 0i32;
                while i < p.sample(rng) {
                    let x = pos.0.wrapping_add(rng.next_int_bounded(16));
                    let z = pos.2.wrapping_add(rng.next_int_bounded(16));
                    if let Some(y) = find_on_ground(rc.region, x, z, layer)? {
                        any |= run_modifiers(tail, feature, rc, rng, ctx, (x, y, z))?;
                        placed_any = true;
                    }
                    i += 1;
                }
                if !placed_any {
                    break;
                }
                layer += 1;
            }
        }
        Placement::NoiseThresholdCount {
            noise_level,
            below,
            above,
        } => {
            let noise = biome_info_value(pos.0, pos.2, 200.0);
            let count = if noise < *noise_level { *below } else { *above };
            for _ in 0..count.max(0) {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::NoiseBasedCount {
            noise_factor,
            noise_offset,
            ratio,
        } => {
            let noise = biome_info_value(pos.0, pos.2, *noise_factor);
            let count = ((noise + noise_offset) * f64::from(*ratio)).ceil() as i32;
            for _ in 0..count.max(0) {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::RarityFilter(chance) => {
            if rng.next_float() < 1.0 / (*chance as f32) {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::RandomChance(prob) => {
            if (rng.next_float() as f64) < *prob {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::InSquare => {
            let x = pos.0.wrapping_add(rng.next_int_bounded(16));
            let z = pos.2.wrapping_add(rng.next_int_bounded(16));
            any |= run_modifiers(tail, feature, rc, rng, ctx, (x, pos.1, z))?;
        }
        Placement::HeightRange(h) => {
            let y = h.sample(rng);
            any |= run_modifiers(tail, feature, rc, rng, ctx, (pos.0, y, pos.2))?;
        }
        Placement::Heightmap(map) => {
            let y = rc.region.read_heightmap(*map, pos.0, pos.2)?;
            any |= run_modifiers(tail, feature, rc, rng, ctx, (pos.0, y, pos.2))?;
        }
        Placement::Offset(x, y, z) => {
            let dx = x.sample(rng);
            let dy = y.sample(rng);
            let dz = z.sample(rng);
            let np = (
                pos.0.wrapping_add(dx),
                pos.1.wrapping_add(dy),
                pos.2.wrapping_add(dz),
            );
            any |= run_modifiers(tail, feature, rc, rng, ctx, np)?;
        }
        Placement::Biome => {
            if biome_allows(rc, pos)? {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::BlockPredicateFilter(pred) => {
            if pred.test(rc.region, pos.0, pos.1, pos.2)? {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::SurfaceRelativeThresholdFilter {
            heightmap,
            min,
            max,
        } => {
            let surface = i64::from(rc.region.read_heightmap(*heightmap, pos.0, pos.2)?);
            let lo = surface + i64::from(*min);
            let hi = surface + i64::from(*max);
            if (lo..=hi).contains(&i64::from(pos.1)) {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::SurfaceWaterDepthFilter { max_depth } => {
            let floor = rc
                .region
                .read_heightmap(Heightmap::OceanFloorWg, pos.0, pos.2)?;
            let surface = rc
                .region
                .read_heightmap(Heightmap::WorldSurfaceWg, pos.0, pos.2)?;
            if surface - floor <= *max_depth {
                any |= run_modifiers(tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::EnvironmentScan {
            up,
            target,
            allowed,
            max_steps,
        } => {
            let dir = if *up { 1 } else { -1 };
            let mut cur = pos;
            for _ in 0..(*max_steps).max(0) {
                if target.test(rc.region, cur.0, cur.1, cur.2)? {
                    any |= run_modifiers(tail, feature, rc, rng, ctx, cur)?;
                    break;
                }
                let ny = cur.1 + dir;
                if !(WORLD_MIN_Y..WORLD_MAX_Y_EXCLUSIVE).contains(&ny) {
                    break;
                }
                if let Some(a) = allowed
                    && !a.test(rc.region, cur.0, ny, cur.2)?
                {
                    break;
                }
                cur = (cur.0, ny, cur.2);
            }
        }
        Placement::RandomlySelected(list) => {
            if !list.is_empty() {
                let pick = rng.next_int_bounded(list.len() as i32) as usize;
                any |= step(&list[pick], tail, feature, rc, rng, ctx, pos)?;
            }
        }
        Placement::FixedPlacement(positions) => {
            for p in positions {
                any |= run_modifiers(tail, feature, rc, rng, ctx, *p)?;
            }
        }
        Placement::Cuboid {
            include_edges,
            include_interior,
            xz_size,
            y_size,
        } => {
            let sx = xz_size.sample(rng).max(0);
            let sy = y_size.sample(rng).max(0);
            for dx in -sx..=sx {
                for dy in -sy..=sy {
                    for dz in -sx..=sx {
                        let edge = dx.abs() == sx || dy.abs() == sy || dz.abs() == sx;
                        if (edge && *include_edges) || (!edge && *include_interior) {
                            let np = (
                                pos.0.wrapping_add(dx),
                                pos.1.wrapping_add(dy),
                                pos.2.wrapping_add(dz),
                            );
                            any |= run_modifiers(tail, feature, rc, rng, ctx, np)?;
                        }
                    }
                }
            }
        }
    }
    Ok(any)
}

fn biome_allows<R: Region>(rc: &RunCtx<R>, pos: Pos) -> Result<bool, FeatureError> {
    if rc.feature_biomes.is_empty() {
        return Ok(true);
    }
    match biome_at_block(rc.region, rc.zoom_seed, pos.0, pos.1, pos.2) {
        Ok(b) => Ok(rc.feature_biomes.contains(&b)),
        Err(FeatureError::Region(_)) => Ok(true),
        Err(e) => Err(e),
    }
}

fn find_on_ground<R: Region>(
    region: &R,
    x: i32,
    z: i32,
    target_layer: i32,
) -> Result<Option<i32>, FeatureError> {
    let top = region.read_heightmap(Heightmap::OceanFloorWg, x, z)?;
    let mut found = 0i32;
    let mut y = top;
    while y > WORLD_MIN_Y {
        let here = physics_of_state(u32::from(region.read_block(x, y, z)?));
        let below = physics_of_state(u32::from(region.read_block(x, y - 1, z)?));
        if (here.is_air() || here.is_replaceable()) && below.is_legacy_solid() {
            if found == target_layer {
                return Ok(Some(y));
            }
            found += 1;
        }
        y -= 1;
    }
    Ok(None)
}

fn place_feature<R: Region>(
    feature: &CompiledFeature,
    rc: &RunCtx<R>,
    rng: &mut FeatureRng,
    ctx: &mut DecorationContext,
    pos: Pos,
) -> Result<bool, FeatureError> {
    match feature {
        CompiledFeature::NoOp => Ok(false),
        CompiledFeature::Unimplemented(_) => {
            ctx.placed.push(pos);
            ctx.unimplemented += 1;
            Ok(false)
        }
        CompiledFeature::Sequence(children) => {
            for child in children {
                if !run_placed(child, rc, rng, ctx, pos)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        CompiledFeature::Overlay(children) => {
            let mut any = false;
            for child in children {
                any |= run_placed(child, rc, rng, ctx, pos)?;
            }
            Ok(any)
        }
        CompiledFeature::SimpleRandomSelector(children) => {
            if children.is_empty() {
                return Ok(false);
            }
            let pick = rng.next_int_bounded(children.len() as i32) as usize;
            run_placed(&children[pick], rc, rng, ctx, pos)
        }
        CompiledFeature::WeightedRandomSelector(children) => {
            if children.is_empty() {
                return Ok(false);
            }
            let total: i32 = children.iter().map(|(w, _)| *w).sum();
            if total <= 0 {
                return Ok(false);
            }
            let mut roll = rng.next_int_bounded(total);
            for (w, child) in children {
                roll -= *w;
                if roll < 0 {
                    return run_placed(child, rc, rng, ctx, pos);
                }
            }
            run_placed(&children[children.len() - 1].1, rc, rng, ctx, pos)
        }
        CompiledFeature::RandomBooleanSelector(if_true, if_false) => {
            if rng.next_boolean() {
                run_placed(if_true, rc, rng, ctx, pos)
            } else {
                run_placed(if_false, rc, rng, ctx, pos)
            }
        }
        CompiledFeature::RandomSelector(default, entries) => {
            for (chance, child) in entries {
                if rng.next_float() < *chance {
                    return run_placed(child, rc, rng, ctx, pos);
                }
            }
            run_placed(default, rc, rng, ctx, pos)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::numbers::IntProvider;
    use crate::feature::region::TestRegion;
    use std::collections::BTreeSet;

    fn load() -> Decoration {
        Decoration::embedded().expect("decoration loads")
    }

    fn leaf(count: i32) -> CompiledPlaced {
        CompiledPlaced {
            placement: vec![Placement::Count(IntProvider::Constant(count))],
            feature: CompiledFeature::Unimplemented("test".to_string()),
        }
    }

    #[test]
    fn sequence_stops_at_first_failure_overlay_runs_all() {
        let region = TestRegion::new((0, 0));
        let seq = CompiledPlaced {
            placement: Vec::new(),
            feature: CompiledFeature::Sequence(vec![leaf(2), leaf(3)]),
        };
        let ovl = CompiledPlaced {
            placement: Vec::new(),
            feature: CompiledFeature::Overlay(vec![leaf(2), leaf(3)]),
        };
        let fb = BTreeSet::new();
        let rc = RunCtx {
            region: &region,
            zoom_seed: 0,
            feature_biomes: &fb,
        };
        let mut rng = FeatureRng::new(1);
        let mut ctx = DecorationContext::new();
        run_placed(&seq, &rc, &mut rng, &mut ctx, (0, 0, 0)).unwrap();
        assert_eq!(
            ctx.placed().len(),
            2,
            "sequence stops after first child fails"
        );

        let mut rng2 = FeatureRng::new(1);
        let mut ctx2 = DecorationContext::new();
        run_placed(&ovl, &rc, &mut rng2, &mut ctx2, (0, 0, 0)).unwrap();
        assert_eq!(ctx2.placed().len(), 5, "overlay runs every child");
    }

    #[test]
    fn in_square_consumes_two_int_draws() {
        let region = TestRegion::new((0, 0));
        let pf = CompiledPlaced {
            placement: vec![Placement::InSquare],
            feature: CompiledFeature::Unimplemented("test".to_string()),
        };
        let fb = BTreeSet::new();
        let rc = RunCtx {
            region: &region,
            zoom_seed: 0,
            feature_biomes: &fb,
        };
        let mut rng = FeatureRng::new(5);
        let probe = rng.clone();
        let mut ctx = DecorationContext::new();
        run_placed(&pf, &rc, &mut rng, &mut ctx, (0, 64, 0)).unwrap();
        assert_eq!(ctx.placed().len(), 1);
        let mut check = probe;
        check.next_int_bounded(16);
        check.next_int_bounded(16);
        assert_eq!(rng.next_bits(32), check.next_bits(32));
    }

    #[test]
    fn decoration_builds() {
        let dec = load();
        assert_eq!(dec.order().steps().len(), 11);
        assert!(dec.compiled.contains_key("minecraft:ore_coal_upper"));
    }

    #[test]
    fn decoration_seed_is_world_seed_at_origin() {
        let mut rng = FeatureRng::new(0);
        let d = rng.set_decoration_seed(123456789, 0, 0);
        assert_eq!(d, 123456789);
    }

    #[test]
    fn decorate_chunk_runs_and_reuses_buffers() {
        let dec = load();
        let mut region = TestRegion::new((0, 0));
        region.set_default_biome(0);
        for cx in -8..=8 {
            for cz in -8..=8 {
                region.set_terrain_ready(cx, cz, true);
            }
        }
        let mut ctx = DecorationContext::new();
        dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
            .expect("decorates");
        let cap_after_first = ctx.placed_capacity();
        dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
            .expect("decorates again");
        assert_eq!(ctx.placed_capacity(), cap_after_first);
    }

    #[test]
    fn fixed_placement_emits_configured_positions() {
        let region = TestRegion::new((0, 0));
        let pf = CompiledPlaced {
            placement: vec![Placement::FixedPlacement(vec![(1, 2, 3), (4, 5, 6)])],
            feature: CompiledFeature::Unimplemented("test".to_string()),
        };
        let fb = BTreeSet::new();
        let rc = RunCtx {
            region: &region,
            zoom_seed: 0,
            feature_biomes: &fb,
        };
        let mut rng = FeatureRng::new(1);
        let mut ctx = DecorationContext::new();
        run_placed(&pf, &rc, &mut rng, &mut ctx, (0, 0, 0)).unwrap();
        assert_eq!(ctx.placed(), &[(1, 2, 3), (4, 5, 6)]);
    }

    #[test]
    fn count_on_every_layer_without_ground_emits_nothing() {
        let region = TestRegion::new((0, 0));
        let pf = CompiledPlaced {
            placement: vec![Placement::CountOnEveryLayer(IntProvider::Constant(6))],
            feature: CompiledFeature::Unimplemented("test".to_string()),
        };
        let fb = BTreeSet::new();
        let rc = RunCtx {
            region: &region,
            zoom_seed: 0,
            feature_biomes: &fb,
        };
        let mut rng = FeatureRng::new(1);
        let mut ctx = DecorationContext::new();
        run_placed(&pf, &rc, &mut rng, &mut ctx, (0, 0, 0)).unwrap();
        assert!(ctx.placed().is_empty());
    }

    #[test]
    fn randomly_selected_picks_then_applies_offset() {
        let region = TestRegion::new((0, 0));
        let pf = CompiledPlaced {
            placement: vec![Placement::RandomlySelected(vec![Placement::Offset(
                IntProvider::Constant(1),
                IntProvider::Constant(-1),
                IntProvider::Constant(2),
            )])],
            feature: CompiledFeature::Unimplemented("test".to_string()),
        };
        let fb = BTreeSet::new();
        let rc = RunCtx {
            region: &region,
            zoom_seed: 0,
            feature_biomes: &fb,
        };
        let mut rng = FeatureRng::new(1);
        let mut ctx = DecorationContext::new();
        run_placed(&pf, &rc, &mut rng, &mut ctx, (10, 20, 30)).unwrap();
        assert_eq!(ctx.placed(), &[(11, 19, 32)]);
        assert_eq!(ctx.unimplemented_hits(), 1);
    }
}
