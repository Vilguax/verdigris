use std::collections::HashMap;

use crate::feature::error::FeatureError;

pub const WORLD_MIN_Y: i32 = -64;
pub const WORLD_MAX_Y_EXCLUSIVE: i32 = 320;
pub const VOID_AIR: u16 = u16::MAX;
pub const AIR: u16 = 0;

pub const READ_RADIUS: i32 = 8;
pub const BIOME_RADIUS: i32 = 1;
pub const WRITE_RADIUS: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heightmap {
    WorldSurfaceWg,
    WorldSurface,
    OceanFloorWg,
    OceanFloor,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

pub fn chunk_distance(center: (i32, i32), x: i32, z: i32) -> i32 {
    let cx = x >> 4;
    let cz = z >> 4;
    (cx - center.0).abs().max((cz - center.1).abs())
}

pub trait Region {
    fn center(&self) -> (i32, i32);

    fn read_block(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError>;

    fn read_heightmap(&self, map: Heightmap, x: i32, z: i32) -> Result<i32, FeatureError>;

    fn read_biome(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError>;

    fn write_block(&mut self, x: i32, y: i32, z: i32, state: u16) -> Result<bool, FeatureError>;
}

#[derive(Default)]
pub struct TestRegion {
    center: (i32, i32),
    blocks: HashMap<(i32, i32, i32), u16>,
    terrain_ready: HashMap<(i32, i32), bool>,
    biomes: HashMap<(i32, i32, i32), u16>,
    heightmaps: HashMap<(i32, i32), i32>,
    default_biome: u16,
}

impl TestRegion {
    pub fn new(center: (i32, i32)) -> Self {
        Self {
            center,
            default_biome: 0,
            ..Default::default()
        }
    }

    pub fn set_terrain_ready(&mut self, cx: i32, cz: i32, ready: bool) {
        self.terrain_ready.insert((cx, cz), ready);
    }

    pub fn set_block_preset(&mut self, x: i32, y: i32, z: i32, state: u16) {
        self.blocks.insert((x, y, z), state);
    }

    pub fn set_biome(&mut self, x: i32, y: i32, z: i32, biome: u16) {
        self.biomes.insert((x, y, z), biome);
    }

    pub fn set_heightmap(&mut self, x: i32, z: i32, y: i32) {
        self.heightmaps.insert((x, z), y);
    }

    pub fn set_default_biome(&mut self, biome: u16) {
        self.default_biome = biome;
    }

    pub fn written(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        self.blocks.get(&(x, y, z)).copied()
    }

    fn is_terrain_ready(&self, cx: i32, cz: i32) -> bool {
        self.terrain_ready.get(&(cx, cz)).copied().unwrap_or(true)
    }
}

impl Region for TestRegion {
    fn center(&self) -> (i32, i32) {
        self.center
    }

    fn read_block(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError> {
        let d = chunk_distance(self.center, x, z);
        if d > READ_RADIUS {
            return Err(FeatureError::Region(format!(
                "read_block chunk distance {d} > {READ_RADIUS}"
            )));
        }
        if !(WORLD_MIN_Y..WORLD_MAX_Y_EXCLUSIVE).contains(&y) {
            return Ok(VOID_AIR);
        }
        if let Some(&b) = self.blocks.get(&(x, y, z)) {
            return Ok(b);
        }
        let cx = x >> 4;
        let cz = z >> 4;
        if d >= 2 && !self.is_terrain_ready(cx, cz) {
            return Ok(AIR);
        }
        Ok(AIR)
    }

    fn read_heightmap(&self, _map: Heightmap, x: i32, z: i32) -> Result<i32, FeatureError> {
        let d = chunk_distance(self.center, x, z);
        if d > READ_RADIUS {
            return Err(FeatureError::Region(format!(
                "read_heightmap chunk distance {d} > {READ_RADIUS}"
            )));
        }
        Ok(self.heightmaps.get(&(x, z)).copied().unwrap_or(WORLD_MIN_Y))
    }

    fn read_biome(&self, x: i32, y: i32, z: i32) -> Result<u16, FeatureError> {
        let d = chunk_distance(self.center, x, z);
        if d > BIOME_RADIUS {
            return Err(FeatureError::Region(format!(
                "read_biome chunk distance {d} > {BIOME_RADIUS}"
            )));
        }
        Ok(self
            .biomes
            .get(&(x, y, z))
            .copied()
            .unwrap_or(self.default_biome))
    }

    fn write_block(&mut self, x: i32, y: i32, z: i32, state: u16) -> Result<bool, FeatureError> {
        let d = chunk_distance(self.center, x, z);
        if d > WRITE_RADIUS {
            return Ok(false);
        }
        if !(WORLD_MIN_Y..WORLD_MAX_Y_EXCLUSIVE).contains(&y) {
            return Ok(false);
        }
        self.blocks.insert((x, y, z), state);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_block_far_is_error() {
        let r = TestRegion::new((0, 0));
        assert!(r.read_block(16 * 9, 0, 0).is_err());
        assert!(r.read_block(16 * 8, 0, 0).is_ok());
    }

    #[test]
    fn read_block_vertical_oob_is_void_air() {
        let r = TestRegion::new((0, 0));
        assert_eq!(r.read_block(0, -65, 0).unwrap(), VOID_AIR);
        assert_eq!(r.read_block(0, 320, 0).unwrap(), VOID_AIR);
    }

    #[test]
    fn unready_chunk_reads_air() {
        let mut r = TestRegion::new((0, 0));
        r.set_terrain_ready(3, 0, false);
        assert_eq!(r.read_block(16 * 3, 0, 0).unwrap(), AIR);
    }

    #[test]
    fn biome_distance_two_is_error() {
        let r = TestRegion::new((0, 0));
        assert!(r.read_biome(32, 0, 0).is_err());
        assert!(r.read_biome(16, 0, 0).is_ok());
    }

    #[test]
    fn write_far_is_silent_false() {
        let mut r = TestRegion::new((0, 0));
        assert!(!r.write_block(32, 0, 0, 5).unwrap());
        assert!(r.write_block(0, 0, 0, 5).unwrap());
    }

    #[test]
    fn write_vertical_oob_is_silent_false() {
        let mut r = TestRegion::new((0, 0));
        assert!(!r.write_block(0, 400, 0, 5).unwrap());
    }
}
