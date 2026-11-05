use verdigris_worldgen::biome_names;
use verdigris_worldgen::feature::features::run_ore;
use verdigris_worldgen::feature::{
    Decoration, DecorationContext, FeatureRng, OreConfig, OreScratch, TestRegion,
};

#[test]
fn placement_buffer_is_reused_across_chunks_without_growth() {
    let dec = Decoration::embedded().unwrap();
    let plains = biome_names()
        .id_of("minecraft:plains")
        .expect("plains biome id");
    let fresh = || {
        let mut region = TestRegion::new((0, 0));
        region.set_default_biome(plains);
        region
    };
    let mut ctx = DecorationContext::new();

    let mut region = fresh();
    dec.decorate_chunk(&mut region, &mut ctx, 20261029, 0, 0)
        .unwrap();
    let warm_cap = ctx.placed_capacity();
    let hits = ctx.unimplemented_hits();

    for _ in 0..8 {
        let mut region = fresh();
        dec.decorate_chunk(&mut region, &mut ctx, 20261029, 0, 0)
            .unwrap();
        assert_eq!(
            ctx.placed_capacity(),
            warm_cap,
            "identical chunk must not reallocate the position buffer"
        );
        assert_eq!(
            ctx.unimplemented_hits(),
            hits,
            "decoration must be deterministic"
        );
    }
}

#[test]
fn ore_scratch_does_not_grow_across_veins() {
    let cfg = OreConfig::compile(&serde_json::json!({
        "size": 9,
        "discard_chance_on_air_exposure": 0,
        "targets": [{
            "state": "minecraft:iron_ore",
            "target": {"predicate_type": "minecraft:always_true"}
        }]
    }))
    .unwrap();

    let build = || {
        let mut region = TestRegion::new((0, 0));
        for x in -4..=20 {
            for z in -4..=20 {
                region.set_heightmap(x, z, 200);
                for y in 48..=72 {
                    region.set_block_preset(x, y, z, 1);
                }
            }
        }
        region
    };

    let mut scratch = OreScratch::new();
    let mut region = build();
    let mut rng = FeatureRng::new(7);
    assert!(run_ore(&cfg, &mut region, &mut rng, &mut scratch, 8, 60, 8).unwrap());
    let warm = scratch.capacities();

    for seed in 1..32i64 {
        let mut region = build();
        let mut rng = FeatureRng::new(seed);
        run_ore(&cfg, &mut region, &mut rng, &mut scratch, 8, 60, 8).unwrap();
        assert_eq!(
            scratch.capacities(),
            warm,
            "ore scratch buffers must not reallocate for identical-size veins"
        );
    }
}
