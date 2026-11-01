use std::cell::Cell;

use verdigris_worldgen::biome_names;
use verdigris_worldgen::feature::{
    Decoration, DecorationContext, FeatureError, Heightmap, Region, TestRegion,
};

struct CountingRegion {
    base: TestRegion,
    neighbor_reads: Cell<usize>,
    center: (i32, i32),
}

impl CountingRegion {
    fn new(biome: u16) -> Self {
        let mut base = TestRegion::new((0, 0));
        base.set_default_biome(biome);
        Self {
            base,
            neighbor_reads: Cell::new(0),
            center: (0, 0),
        }
    }
}

impl Region for CountingRegion {
    fn center(&self) -> (i32, i32) {
        self.base.center()
    }
    fn read_block(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError> {
        self.base.read_block(x, y, z)
    }
    fn read_heightmap(&self, map: Heightmap, x: i32, z: i32) -> Result<i32, FeatureError> {
        self.base.read_heightmap(map, x, z)
    }
    fn read_biome(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError> {
        if (x >> 4, z >> 4) != self.center {
            self.neighbor_reads.set(self.neighbor_reads.get() + 1);
        }
        self.base.read_biome(x, y, z)
    }
    fn write_block(&mut self, x: i32, y: i32, z: i32, state: u16) -> Result<bool, FeatureError> {
        self.base.write_block(x, y, z, state)
    }
}

#[test]
fn plains_activates_features_deterministically() {
    let dec = Decoration::embedded().unwrap();
    let plains = biome_names()
        .id_of("minecraft:plains")
        .expect("plains biome id");
    let mut region = TestRegion::new((0, 0));
    region.set_default_biome(plains);
    let mut ctx = DecorationContext::new();
    dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
        .unwrap();
    let hits = ctx.unimplemented_hits();
    assert!(hits > 0, "plains should activate block-placing features");
    dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
        .unwrap();
    assert_eq!(
        ctx.unimplemented_hits(),
        hits,
        "aggregation + biome gating must be deterministic"
    );
}

#[test]
fn biome_absent_from_overworld_activates_no_features() {
    let dec = Decoration::embedded().unwrap();
    let mut region = TestRegion::new((0, 0));
    region.set_default_biome(60000);
    let mut ctx = DecorationContext::new();
    dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
        .unwrap();
    assert_eq!(
        ctx.unimplemented_hits(),
        0,
        "a biome in no overworld feature list activates nothing"
    );
}

#[test]
fn aggregation_reads_stored_biomes_of_the_nine_chunks() {
    let dec = Decoration::embedded().unwrap();
    let plains = biome_names()
        .id_of("minecraft:plains")
        .expect("plains biome id");
    let region = CountingRegion::new(plains);
    let mut ctx = DecorationContext::new();
    dec.decorate_chunk(&region, &mut ctx, 20261029, 0, 0)
        .unwrap();
    assert!(
        region.neighbor_reads.get() > 0,
        "decorate_chunk must read stored biomes from the 8 neighbor chunks, not only the center"
    );
}
