use std::collections::HashMap;
use std::sync::Arc;

use crate::biome::SearchState;
use crate::chunk_fill::{CHUNK_WIDTH, ChunkTerrain, WorldGenerator};
use crate::df::VolumeError;
use crate::feature::error::FeatureError;
use crate::feature::palette::{block_id_of_state, default_state_id, physics_of_state};
use crate::feature::region::{
    AIR, BIOME_RADIUS, Heightmap, READ_RADIUS, Region, VOID_AIR, WORLD_MAX_Y_EXCLUSIVE,
    WORLD_MIN_Y, WRITE_RADIUS, chunk_distance,
};

pub struct ProtoChunk {
    states: Vec<u16>,
    ocean_floor_wg: Vec<i32>,
    world_surface_wg: Vec<i32>,
    motion_blocking: Vec<i32>,
    motion_blocking_no_leaves: Vec<i32>,
    biome: Arc<[u16]>,
}

fn index_to_state_table(generator: &WorldGenerator) -> Vec<u16> {
    generator
        .material()
        .blocks
        .iter()
        .map(|name| default_state_id(name).unwrap_or(0) as u16)
        .collect()
}

impl ProtoChunk {
    pub fn from_terrain(
        generator: &WorldGenerator,
        cx: i32,
        cz: i32,
        terrain: &ChunkTerrain,
        search: &mut SearchState,
    ) -> ProtoChunk {
        let height = generator.height();
        let min_y = generator.min_y();
        let index_to_state = index_to_state_table(generator);

        let mut states = vec![0u16; height * 256];
        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                let base = (lx + CHUNK_WIDTH * lz) * height;
                for j in 0..height {
                    let y = min_y + j as i32;
                    let idx = terrain.block_id_at(lx, y, lz);
                    states[base + j] = index_to_state[idx as usize];
                }
            }
        }

        let mut ocean_floor_wg = vec![min_y; 256];
        let mut world_surface_wg = vec![min_y; 256];
        let mut motion_blocking = vec![min_y; 256];
        let mut motion_blocking_no_leaves = vec![min_y; 256];
        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                let col = lx + CHUNK_WIDTH * lz;
                let base = col * height;
                let mut got_ws = false;
                let mut got_of = false;
                let mut got_mb = false;
                for j in (0..height).rev() {
                    let state = u32::from(states[base + j]);
                    let ph = physics_of_state(state);
                    let y = min_y + j as i32;
                    if !got_ws && !ph.is_air() {
                        world_surface_wg[col] = y + 1;
                        got_ws = true;
                    }
                    if !got_of && ph.blocks_motion() {
                        ocean_floor_wg[col] = y + 1;
                        got_of = true;
                    }
                    if !got_mb && (ph.blocks_motion() || ph.is_liquid()) {
                        motion_blocking[col] = y + 1;
                        motion_blocking_no_leaves[col] = y + 1;
                        got_mb = true;
                    }
                    if got_ws && got_of && got_mb {
                        break;
                    }
                }
            }
        }

        let biome = generator.material().biome_source.grid(cx, cz, search);

        ProtoChunk {
            states,
            ocean_floor_wg,
            world_surface_wg,
            motion_blocking,
            motion_blocking_no_leaves,
            biome,
        }
    }

    pub fn empty(
        generator: &WorldGenerator,
        cx: i32,
        cz: i32,
        search: &mut SearchState,
    ) -> ProtoChunk {
        let height = generator.height();
        let min_y = generator.min_y();
        let biome = generator.material().biome_source.grid(cx, cz, search);
        ProtoChunk {
            states: vec![0u16; height * 256],
            ocean_floor_wg: vec![min_y; 256],
            world_surface_wg: vec![min_y; 256],
            motion_blocking: vec![min_y; 256],
            motion_blocking_no_leaves: vec![min_y; 256],
            biome,
        }
    }
}

pub struct TerrainFeatureWorld {
    generator: Arc<WorldGenerator>,
    scratch: ChunkTerrain,
    search: SearchState,
    min_y: i32,
    height: usize,
    qy_count: usize,
    chunks: HashMap<(i32, i32), ProtoChunk>,
}

impl TerrainFeatureWorld {
    pub fn new(generator: Arc<WorldGenerator>) -> Self {
        let min_y = generator.min_y();
        let height = generator.height();
        let qy_count = generator.material().biome_source.quart_y_count();
        Self {
            scratch: generator.terrain(),
            generator,
            search: SearchState::new(),
            min_y,
            height,
            qy_count,
            chunks: HashMap::new(),
        }
    }

    pub fn generator(&self) -> &Arc<WorldGenerator> {
        &self.generator
    }

    pub fn material_default_states(&self) -> Vec<u16> {
        index_to_state_table(&self.generator)
    }

    pub fn is_loaded(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn insert_chunk(&mut self, cx: i32, cz: i32, proto: ProtoChunk) {
        self.chunks.entry((cx, cz)).or_insert(proto);
    }

    pub fn remove_chunk(&mut self, cx: i32, cz: i32) -> bool {
        self.chunks.remove(&(cx, cz)).is_some()
    }

    pub fn load_chunk(&mut self, cx: i32, cz: i32) -> Result<(), VolumeError> {
        if self.chunks.contains_key(&(cx, cz)) {
            return Ok(());
        }
        let generator = Arc::clone(&self.generator);
        generator.fill_chunk(cx, cz, &mut self.scratch)?;
        let proto = ProtoChunk::from_terrain(&generator, cx, cz, &self.scratch, &mut self.search);
        self.chunks.insert((cx, cz), proto);
        Ok(())
    }

    pub fn region(&mut self, cx: i32, cz: i32) -> TerrainRegion<'_> {
        TerrainRegion {
            world: self,
            center: (cx, cz),
        }
    }

    pub fn state_at(&self, cx: i32, cz: i32, lx: usize, y: i32, lz: usize) -> u16 {
        if lx >= CHUNK_WIDTH || lz >= CHUNK_WIDTH {
            return 0;
        }
        if y < self.min_y || y >= self.min_y + self.height as i32 {
            return 0;
        }
        match self.chunks.get(&(cx, cz)) {
            Some(ch) => {
                let j = (y - self.min_y) as usize;
                ch.states[j + (lx + CHUNK_WIDTH * lz) * self.height]
            }
            None => 0,
        }
    }

    pub fn column_states(&self, cx: i32, cz: i32) -> Option<&[u16]> {
        self.chunks.get(&(cx, cz)).map(|ch| ch.states.as_slice())
    }

    pub fn column_biomes(&self, cx: i32, cz: i32) -> Option<&[u16]> {
        self.chunks.get(&(cx, cz)).map(|ch| &*ch.biome)
    }

    pub fn block_id_at(&self, cx: i32, cz: i32, lx: usize, y: i32, lz: usize) -> u16 {
        if lx >= CHUNK_WIDTH || lz >= CHUNK_WIDTH {
            return 0;
        }
        if y < self.min_y || y >= self.min_y + self.height as i32 {
            return 0;
        }
        match self.chunks.get(&(cx, cz)) {
            Some(ch) => {
                let j = (y - self.min_y) as usize;
                block_id_of_state(u32::from(
                    ch.states[j + (lx + CHUNK_WIDTH * lz) * self.height],
                ))
            }
            None => 0,
        }
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    pub fn height(&self) -> usize {
        self.height
    }

    fn column_index(&self, x: i32, y: i32, z: i32) -> usize {
        let lx = (x & 15) as usize;
        let lz = (z & 15) as usize;
        let j = (y - self.min_y) as usize;
        j + (lx + CHUNK_WIDTH * lz) * self.height
    }

    fn heightmap_value(&self, map: Heightmap, ch: &ProtoChunk, x: i32, z: i32) -> i32 {
        let col = (x & 15) as usize + CHUNK_WIDTH * (z & 15) as usize;
        match map {
            Heightmap::OceanFloorWg | Heightmap::OceanFloor => ch.ocean_floor_wg[col],
            Heightmap::WorldSurfaceWg | Heightmap::WorldSurface => ch.world_surface_wg[col],
            Heightmap::MotionBlocking => ch.motion_blocking[col],
            Heightmap::MotionBlockingNoLeaves => ch.motion_blocking_no_leaves[col],
        }
    }

    fn biome_cell(&self, ch: &ProtoChunk, cell_x: i32, cell_y: i32, cell_z: i32) -> u16 {
        let gx = (cell_x & 3) as usize;
        let gz = (cell_z & 3) as usize;
        let top = self.qy_count as i32 - 1 - 16;
        let qy = cell_y.clamp(-16, top);
        let qy_idx = (qy + 16) as usize;
        let idx = qy_idx * 16 + gz * 4 + gx;
        ch.biome.get(idx).copied().unwrap_or(0)
    }
}

pub struct TerrainRegion<'a> {
    world: &'a mut TerrainFeatureWorld,
    center: (i32, i32),
}

impl Region for TerrainRegion<'_> {
    fn center(&self) -> (i32, i32) {
        self.center
    }

    fn read_block(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError> {
        if !(WORLD_MIN_Y..WORLD_MAX_Y_EXCLUSIVE).contains(&y) {
            return Ok(VOID_AIR);
        }
        let d = chunk_distance(self.center, x, z);
        if d > READ_RADIUS {
            return Err(FeatureError::Region(format!(
                "read_block chunk distance {d} > {READ_RADIUS}"
            )));
        }
        match self.world.chunks.get(&(x >> 4, z >> 4)) {
            Some(ch) => Ok(ch.states[self.world.column_index(x, y, z)]),
            None => Ok(AIR),
        }
    }

    fn read_heightmap(&self, map: Heightmap, x: i32, z: i32) -> Result<i32, FeatureError> {
        let d = chunk_distance(self.center, x, z);
        if d > READ_RADIUS {
            return Err(FeatureError::Region(format!(
                "read_heightmap chunk distance {d} > {READ_RADIUS}"
            )));
        }
        match self.world.chunks.get(&(x >> 4, z >> 4)) {
            Some(ch) => Ok(self.world.heightmap_value(map, ch, x, z)),
            None => Ok(WORLD_MIN_Y),
        }
    }

    fn read_biome(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError> {
        let d = chunk_distance(self.center, x, z);
        if d > BIOME_RADIUS {
            return Err(FeatureError::Region(format!(
                "read_biome chunk distance {d} > {BIOME_RADIUS}"
            )));
        }
        self.stored_biome(x >> 2, y >> 2, z >> 2)
    }

    fn stored_biome(&self, cell_x: i32, cell_y: i32, cell_z: i32) -> Result<u16, FeatureError> {
        let ccx = cell_x >> 2;
        let ccz = cell_z >> 2;
        let d = (ccx - self.center.0).abs().max((ccz - self.center.1).abs());
        if d > BIOME_RADIUS {
            return Err(FeatureError::Region(format!(
                "stored_biome chunk distance {d} > {BIOME_RADIUS}"
            )));
        }
        match self.world.chunks.get(&(ccx, ccz)) {
            Some(ch) => Ok(self.world.biome_cell(ch, cell_x, cell_y, cell_z)),
            None => Ok(0),
        }
    }

    fn write_block(&mut self, x: i32, y: i32, z: i32, state: u16) -> Result<bool, FeatureError> {
        let d = chunk_distance(self.center, x, z);
        if d > READ_RADIUS {
            return Err(FeatureError::Region(format!(
                "write_block chunk distance {d} > {READ_RADIUS}"
            )));
        }
        if d > WRITE_RADIUS {
            return Ok(false);
        }
        if !(WORLD_MIN_Y..WORLD_MAX_Y_EXCLUSIVE).contains(&y) {
            return Ok(false);
        }
        let idx = self.world.column_index(x, y, z);
        match self.world.chunks.get_mut(&(x >> 4, z >> 4)) {
            Some(ch) => {
                ch.states[idx] = state;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_fill::WorldGenerator;

    fn world() -> TerrainFeatureWorld {
        let generator = Arc::new(WorldGenerator::new(1).expect("generator"));
        TerrainFeatureWorld::new(generator)
    }

    fn insert_empty(world: &mut TerrainFeatureWorld, cx: i32, cz: i32) {
        let proto = {
            let generator = Arc::clone(world.generator());
            let mut search = SearchState::new();
            ProtoChunk::empty(&generator, cx, cz, &mut search)
        };
        world.insert_chunk(cx, cz, proto);
    }

    #[test]
    fn state_at_out_of_bounds_never_panics() {
        let mut world = world();
        let min_y = world.min_y();
        let top = min_y + world.height() as i32;

        assert_eq!(world.state_at(0, 0, 0, min_y, 0), 0);
        assert_eq!(world.state_at(-5, 7, 0, min_y, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, min_y - 1, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, min_y - 1000, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, top, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, i32::MIN, 0), 0);
        assert_eq!(world.state_at(0, 0, 16, 0, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, 0, 16), 0);
        assert_eq!(world.state_at(0, 0, usize::MAX, 0, usize::MAX), 0);

        insert_empty(&mut world, 0, 0);
        assert_eq!(world.state_at(0, 0, 0, min_y, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, min_y - 1, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, top, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, top + 500, 0), 0);
        assert_eq!(world.state_at(0, 0, 16, min_y, 0), 0);
        assert_eq!(world.state_at(0, 0, 0, min_y, 16), 0);
        assert_eq!(world.state_at(0, 0, usize::MAX, i32::MAX, usize::MAX), 0);
    }

    #[test]
    fn block_id_at_out_of_bounds_never_panics() {
        let mut world = world();
        let min_y = world.min_y();
        let top = min_y + world.height() as i32;
        insert_empty(&mut world, 2, -3);

        assert_eq!(world.block_id_at(2, -3, 0, min_y, 0), 0);
        assert_eq!(world.block_id_at(2, -3, 0, min_y - 1, 0), 0);
        assert_eq!(world.block_id_at(2, -3, 0, top, 0), 0);
        assert_eq!(world.block_id_at(2, -3, 0, i32::MIN, 0), 0);
        assert_eq!(world.block_id_at(2, -3, 16, min_y, 0), 0);
        assert_eq!(world.block_id_at(2, -3, 0, min_y, 99), 0);
        assert_eq!(world.block_id_at(9, 9, 0, min_y, 0), 0);
    }

    #[test]
    fn empty_proto_is_air_with_floor_heightmaps() {
        let mut world = world();
        let min_y = world.min_y();
        insert_empty(&mut world, 0, 0);
        assert_eq!(world.chunk_count(), 1);

        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                for j in 0..world.height() {
                    let y = min_y + j as i32;
                    assert_eq!(world.state_at(0, 0, lx, y, lz), 0);
                    assert_eq!(world.block_id_at(0, 0, lx, y, lz), 0);
                }
            }
        }

        let biomes = world.column_biomes(0, 0).expect("biomes présents");
        assert!(!biomes.is_empty());

        let region = world.region(0, 0);
        for map in [
            Heightmap::OceanFloorWg,
            Heightmap::WorldSurfaceWg,
            Heightmap::MotionBlocking,
            Heightmap::MotionBlockingNoLeaves,
        ] {
            assert_eq!(region.read_heightmap(map, 0, 0).unwrap(), min_y);
        }

        assert!(world.remove_chunk(0, 0));
        assert_eq!(world.chunk_count(), 0);
        assert!(!world.remove_chunk(0, 0));
    }

    #[test]
    fn region_far_reads_air_then_err_before_and_after_release() {
        let mut world = world();
        insert_empty(&mut world, 0, 0);
        insert_empty(&mut world, 3, 0);

        {
            let region = world.region(0, 0);
            for d in 2..=READ_RADIUS {
                let x = d * CHUNK_WIDTH as i32;
                assert_eq!(region.read_block(x, 0, 0).unwrap(), AIR);
            }
            let out = (READ_RADIUS + 1) * CHUNK_WIDTH as i32;
            assert!(region.read_block(out, 0, 0).is_err());
            assert_eq!(
                region.read_block(3 * CHUNK_WIDTH as i32, 0, 0).unwrap(),
                AIR
            );
        }

        assert!(world.remove_chunk(3, 0));
        let region = world.region(0, 0);
        assert_eq!(
            region.read_block(3 * CHUNK_WIDTH as i32, 0, 0).unwrap(),
            AIR
        );
        for d in 2..=READ_RADIUS {
            let x = d * CHUNK_WIDTH as i32;
            assert_eq!(region.read_block(x, 0, 0).unwrap(), AIR);
        }
    }
}
