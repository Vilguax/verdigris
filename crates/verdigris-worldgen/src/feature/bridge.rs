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

struct LoadedChunk {
    states: Vec<u16>,
    ocean_floor_wg: Vec<i32>,
    world_surface_wg: Vec<i32>,
    motion_blocking: Vec<i32>,
    motion_blocking_no_leaves: Vec<i32>,
    biome: Arc<[u16]>,
}

pub struct TerrainFeatureWorld<'g> {
    generator: &'g WorldGenerator,
    scratch: ChunkTerrain,
    search: SearchState,
    index_to_state: Vec<u16>,
    min_y: i32,
    height: usize,
    qy_count: usize,
    chunks: HashMap<(i32, i32), LoadedChunk>,
}

impl<'g> TerrainFeatureWorld<'g> {
    pub fn new(generator: &'g WorldGenerator) -> Self {
        let min_y = generator.min_y();
        let height = generator.height();
        let qy_count = generator.material().biome_source.quart_y_count();
        let index_to_state: Vec<u16> = generator
            .material()
            .blocks
            .iter()
            .map(|name| default_state_id(name).unwrap_or(0) as u16)
            .collect();
        Self {
            scratch: generator.terrain(),
            generator,
            search: SearchState::new(),
            index_to_state,
            min_y,
            height,
            qy_count,
            chunks: HashMap::new(),
        }
    }

    pub fn is_loaded(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    pub fn load_chunk(&mut self, cx: i32, cz: i32) -> Result<(), VolumeError> {
        if self.chunks.contains_key(&(cx, cz)) {
            return Ok(());
        }
        let generator = self.generator;
        generator.fill_chunk(cx, cz, &mut self.scratch)?;
        let height = self.height;
        let min_y = self.min_y;
        let mut states = vec![0u16; height * 256];
        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                let base = (lx + CHUNK_WIDTH * lz) * height;
                for j in 0..height {
                    let y = min_y + j as i32;
                    let idx = self.scratch.block_id_at(lx, y, lz);
                    states[base + j] = self.index_to_state[idx as usize];
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

        let biome = generator
            .material()
            .biome_source
            .grid(cx, cz, &mut self.search);

        self.chunks.insert(
            (cx, cz),
            LoadedChunk {
                states,
                ocean_floor_wg,
                world_surface_wg,
                motion_blocking,
                motion_blocking_no_leaves,
                biome,
            },
        );
        Ok(())
    }

    pub fn region(&mut self, cx: i32, cz: i32) -> TerrainRegion<'_, 'g> {
        TerrainRegion {
            world: self,
            center: (cx, cz),
        }
    }

    pub fn block_id_at(&self, cx: i32, cz: i32, lx: usize, y: i32, lz: usize) -> u16 {
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

    fn column_index(&self, x: i32, y: i32, z: i32) -> usize {
        let lx = (x & 15) as usize;
        let lz = (z & 15) as usize;
        let j = (y - self.min_y) as usize;
        j + (lx + CHUNK_WIDTH * lz) * self.height
    }

    fn heightmap_value(&self, map: Heightmap, ch: &LoadedChunk, x: i32, z: i32) -> i32 {
        let col = (x & 15) as usize + CHUNK_WIDTH * (z & 15) as usize;
        match map {
            Heightmap::OceanFloorWg | Heightmap::OceanFloor => ch.ocean_floor_wg[col],
            Heightmap::WorldSurfaceWg | Heightmap::WorldSurface => ch.world_surface_wg[col],
            Heightmap::MotionBlocking => ch.motion_blocking[col],
            Heightmap::MotionBlockingNoLeaves => ch.motion_blocking_no_leaves[col],
        }
    }

    fn biome_cell(&self, ch: &LoadedChunk, cell_x: i32, cell_y: i32, cell_z: i32) -> u16 {
        let gx = (cell_x & 3) as usize;
        let gz = (cell_z & 3) as usize;
        let top = self.qy_count as i32 - 1 - 16;
        let qy = cell_y.clamp(-16, top);
        let qy_idx = (qy + 16) as usize;
        let idx = qy_idx * 16 + gz * 4 + gx;
        ch.biome.get(idx).copied().unwrap_or(0)
    }
}

pub struct TerrainRegion<'a, 'g> {
    world: &'a mut TerrainFeatureWorld<'g>,
    center: (i32, i32),
}

impl Region for TerrainRegion<'_, '_> {
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
