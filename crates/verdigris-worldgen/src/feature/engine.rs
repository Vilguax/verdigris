use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde_json::Value;

use crate::biome::biome_names;
use crate::feature::error::FeatureError;
use crate::feature::numbers::HeightCtx;
use crate::feature::order::{GlobalOrder, build_order, overworld_biomes_from_features};
use crate::feature::placement::{Placement, Pos};
use crate::feature::region::Region;
use crate::feature::registry::{
    ConfiguredFeature, FEATURES_JSON, FeatureRef, FeatureRegistry, PlacedFeature, PlacedRef,
};
use crate::feature::rng::FeatureRng;

const WORLD_CTX: HeightCtx = HeightCtx {
    min_y: -64,
    height: 384,
};

const MAX_COMPILE_DEPTH: usize = 64;

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
    Leaf(String),
}

#[derive(Default)]
pub struct DecorationContext {
    placed: Vec<Pos>,
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

    fn reset(&mut self) {
        self.placed.clear();
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
        candidate_biomes: &BTreeSet<u16>,
    ) -> Result<(), FeatureError> {
        ctx.reset();
        let min_x = cx.wrapping_mul(16);
        let min_z = cz.wrapping_mul(16);
        let mut rng = FeatureRng::new(0);
        let decoration_seed = rng.set_decoration_seed(level_seed, min_x, min_z);

        for step in 0..self.order.steps().len() {
            for (i, name) in self.order.step(step).iter().enumerate() {
                if !self.is_active(name, candidate_biomes) {
                    continue;
                }
                let Some(compiled) = self.compiled.get(name) else {
                    continue;
                };
                rng.set_feature_seed(decoration_seed, i as i32, step as i32);
                run_placed(compiled, &mut rng, region, (min_x, 0, min_z), ctx)?;
            }
        }
        Ok(())
    }

    fn is_active(&self, name: &str, candidate: &BTreeSet<u16>) -> bool {
        match self.allowed_biomes.get(name) {
            Some(set) => set.iter().any(|b| candidate.contains(b)),
            None => true,
        }
    }
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
        return Ok(CompiledFeature::Leaf("depth_limit".to_string()));
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
        return Ok(CompiledPlaced {
            placement: Vec::new(),
            feature: CompiledFeature::Leaf("depth_limit".to_string()),
        });
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
        ConfiguredFeature::Leaf { ty, .. } => Ok(CompiledFeature::Leaf(ty.clone())),
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
    rng: &mut FeatureRng,
    region: &R,
    origin: Pos,
    ctx: &mut DecorationContext,
) -> Result<bool, FeatureError> {
    run_modifiers(&pf.placement, 0, &pf.feature, rng, region, origin, ctx)
}

fn run_modifiers<R: Region>(
    mods: &[Placement],
    idx: usize,
    feature: &CompiledFeature,
    rng: &mut FeatureRng,
    region: &R,
    pos: Pos,
    ctx: &mut DecorationContext,
) -> Result<bool, FeatureError> {
    if idx == mods.len() {
        return place_feature(feature, rng, region, pos, ctx);
    }
    let mut any = false;
    match &mods[idx] {
        Placement::Count(p) => {
            let n = p.sample(rng).max(0);
            for _ in 0..n {
                any |= run_modifiers(mods, idx + 1, feature, rng, region, pos, ctx)?;
            }
        }
        Placement::RarityFilter(chance) => {
            if rng.next_float() < 1.0 / (*chance as f32) {
                any |= run_modifiers(mods, idx + 1, feature, rng, region, pos, ctx)?;
            }
        }
        Placement::RandomChance(prob) => {
            if (rng.next_float() as f64) < *prob {
                any |= run_modifiers(mods, idx + 1, feature, rng, region, pos, ctx)?;
            }
        }
        Placement::InSquare => {
            let x = pos.0.wrapping_add(rng.next_int_bounded(16));
            let z = pos.2.wrapping_add(rng.next_int_bounded(16));
            any |= run_modifiers(mods, idx + 1, feature, rng, region, (x, pos.1, z), ctx)?;
        }
        Placement::HeightRange(h) => {
            let y = h.sample(rng);
            any |= run_modifiers(mods, idx + 1, feature, rng, region, (pos.0, y, pos.2), ctx)?;
        }
        Placement::Heightmap(map) => {
            let y = region.read_heightmap(*map, pos.0, pos.2)?;
            any |= run_modifiers(mods, idx + 1, feature, rng, region, (pos.0, y, pos.2), ctx)?;
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
            any |= run_modifiers(mods, idx + 1, feature, rng, region, np, ctx)?;
        }
        Placement::Biome => {
            any |= run_modifiers(mods, idx + 1, feature, rng, region, pos, ctx)?;
        }
        Placement::BlockPredicateFilter(pred) => {
            if pred.test(region, pos.0, pos.1, pos.2)? {
                any |= run_modifiers(mods, idx + 1, feature, rng, region, pos, ctx)?;
            }
        }
        Placement::Passthrough(_) => {
            any |= run_modifiers(mods, idx + 1, feature, rng, region, pos, ctx)?;
        }
    }
    Ok(any)
}

fn place_feature<R: Region>(
    feature: &CompiledFeature,
    rng: &mut FeatureRng,
    region: &R,
    pos: Pos,
    ctx: &mut DecorationContext,
) -> Result<bool, FeatureError> {
    match feature {
        CompiledFeature::NoOp => Ok(false),
        CompiledFeature::Leaf(_) => {
            ctx.placed.push(pos);
            Ok(false)
        }
        CompiledFeature::Sequence(children) => {
            for child in children {
                if !run_placed(child, rng, region, pos, ctx)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        CompiledFeature::Overlay(children) => {
            let mut any = false;
            for child in children {
                any |= run_placed(child, rng, region, pos, ctx)?;
            }
            Ok(any)
        }
        CompiledFeature::SimpleRandomSelector(children) => {
            if children.is_empty() {
                return Ok(false);
            }
            let pick = rng.next_int_bounded(children.len() as i32) as usize;
            run_placed(&children[pick], rng, region, pos, ctx)
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
                    return run_placed(child, rng, region, pos, ctx);
                }
            }
            run_placed(&children[children.len() - 1].1, rng, region, pos, ctx)
        }
        CompiledFeature::RandomBooleanSelector(if_true, if_false) => {
            if rng.next_boolean() {
                run_placed(if_true, rng, region, pos, ctx)
            } else {
                run_placed(if_false, rng, region, pos, ctx)
            }
        }
        CompiledFeature::RandomSelector(default, entries) => {
            for (chance, child) in entries {
                if rng.next_float() < *chance {
                    return run_placed(child, rng, region, pos, ctx);
                }
            }
            run_placed(default, rng, region, pos, ctx)
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
            feature: CompiledFeature::Leaf("test".to_string()),
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
        let mut rng = FeatureRng::new(1);
        let mut ctx = DecorationContext::new();
        run_placed(&seq, &mut rng, &region, (0, 0, 0), &mut ctx).unwrap();
        assert_eq!(
            ctx.placed().len(),
            2,
            "sequence stops after first child fails"
        );

        let mut rng2 = FeatureRng::new(1);
        let mut ctx2 = DecorationContext::new();
        run_placed(&ovl, &mut rng2, &region, (0, 0, 0), &mut ctx2).unwrap();
        assert_eq!(ctx2.placed().len(), 5, "overlay runs every child");
    }

    #[test]
    fn in_square_consumes_two_int_draws() {
        let region = TestRegion::new((0, 0));
        let pf = CompiledPlaced {
            placement: vec![Placement::InSquare],
            feature: CompiledFeature::Leaf("test".to_string()),
        };
        let mut rng = FeatureRng::new(5);
        let probe = rng.clone();
        let mut ctx = DecorationContext::new();
        run_placed(&pf, &mut rng, &region, (0, 64, 0), &mut ctx).unwrap();
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
        let mut candidate = BTreeSet::new();
        candidate.insert(0u16);
        let mut ctx = DecorationContext::new();
        dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0, &candidate)
            .expect("decorates");
        let cap_after_first = ctx.placed_capacity();
        dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0, &candidate)
            .expect("decorates again");
        assert_eq!(ctx.placed_capacity(), cap_after_first);
    }
}
