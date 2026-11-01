use verdigris_worldgen::biome_names;
use verdigris_worldgen::feature::{Decoration, DecorationContext, TestRegion};

#[test]
fn placement_buffer_is_reused_across_chunks_without_growth() {
    let dec = Decoration::embedded().unwrap();
    let plains = biome_names()
        .id_of("minecraft:plains")
        .expect("plains biome id");
    let mut region = TestRegion::new((0, 0));
    region.set_default_biome(plains);
    let mut ctx = DecorationContext::new();

    dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
        .unwrap();
    let warm_cap = ctx.placed_capacity();
    let hits = ctx.unimplemented_hits();

    for _ in 0..8 {
        dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
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
