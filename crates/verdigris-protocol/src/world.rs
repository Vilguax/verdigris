use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::blocks::default_state_of_name;
use crate::chunk::{ChunkColumn, block_index};
use crate::play::{PlayError, plains_biome_id};

pub const BEDROCK: &str = "minecraft:bedrock";
pub const DIRT: &str = "minecraft:dirt";
pub const GRASS_BLOCK: &str = "minecraft:grass_block";

pub const BLOCK_REGISTRY: &str = "minecraft:block";

pub const FLOOR_BEDROCK_Y: i32 = -64;
pub const FLOOR_DIRT_LOWER_Y: i32 = -63;
pub const FLOOR_DIRT_UPPER_Y: i32 = -62;
pub const FLOOR_GRASS_Y: i32 = -61;

pub trait ChunkGenerator: Send + Sync {
    fn generate(&self, chunk_x: i32, chunk_z: i32) -> ChunkColumn;
}

fn block_state(name: &'static str) -> Result<u16, PlayError> {
    default_state_of_name(name)
        .map(|state| state as u16)
        .ok_or(PlayError::MissingEntry {
            registry: BLOCK_REGISTRY,
            entry: name,
        })
}

pub struct FlatGenerator {
    bedrock: u16,
    dirt: u16,
    grass: u16,
    biome: u16,
}

impl FlatGenerator {
    pub fn new() -> Result<Self, PlayError> {
        Ok(FlatGenerator {
            bedrock: block_state(BEDROCK)?,
            dirt: block_state(DIRT)?,
            grass: block_state(GRASS_BLOCK)?,
            biome: plains_biome_id()? as u16,
        })
    }
}

impl ChunkGenerator for FlatGenerator {
    fn generate(&self, _chunk_x: i32, _chunk_z: i32) -> ChunkColumn {
        let mut column = ChunkColumn::empty(self.biome);
        let floor = &mut column.sections[0];
        for z in 0..16 {
            for x in 0..16 {
                floor.states[block_index(x, 0, z)] = self.bedrock;
                floor.states[block_index(x, 1, z)] = self.dirt;
                floor.states[block_index(x, 2, z)] = self.dirt;
                floor.states[block_index(x, 3, z)] = self.grass;
            }
        }
        column
    }
}

pub struct World {
    generator: Box<dyn ChunkGenerator>,
    cache: Mutex<HashMap<(i32, i32), Arc<ChunkColumn>>>,
}

impl World {
    pub fn new(generator: Box<dyn ChunkGenerator>) -> Self {
        World {
            generator,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn flat() -> Result<Self, PlayError> {
        Ok(World::new(Box::new(FlatGenerator::new()?)))
    }

    pub fn chunk(&self, chunk_x: i32, chunk_z: i32) -> Arc<ChunkColumn> {
        let mut cache = self.cache.lock().unwrap();
        cache
            .entry((chunk_x, chunk_z))
            .or_insert_with(|| Arc::new(self.generator.generate(chunk_x, chunk_z)))
            .clone()
    }

    pub fn loaded_len(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::MIN_Y;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn state_at(column: &ChunkColumn, y: i32) -> u16 {
        let rel = (y - MIN_Y) as usize;
        column.sections[rel / 16].states[block_index(0, rel % 16, 0)]
    }

    #[test]
    fn flat_terrain_layers() {
        let generator = FlatGenerator::new().unwrap();
        let column = generator.generate(3, -7);

        assert_eq!(
            state_at(&column, FLOOR_BEDROCK_Y),
            block_state(BEDROCK).unwrap()
        );
        assert_eq!(
            state_at(&column, FLOOR_DIRT_LOWER_Y),
            block_state(DIRT).unwrap()
        );
        assert_eq!(
            state_at(&column, FLOOR_DIRT_UPPER_Y),
            block_state(DIRT).unwrap()
        );
        assert_eq!(
            state_at(&column, FLOOR_GRASS_Y),
            block_state(GRASS_BLOCK).unwrap()
        );
        assert_eq!(state_at(&column, FLOOR_GRASS_Y + 1), 0);
        assert_eq!(state_at(&column, 100), 0);
    }

    #[test]
    fn flat_terrain_biome_is_plains_everywhere() {
        let generator = FlatGenerator::new().unwrap();
        let column = generator.generate(0, 0);
        let plains = plains_biome_id().unwrap() as u16;
        for section in &column.sections {
            assert!(section.biomes.iter().all(|&biome| biome == plains));
        }
    }

    struct Counting {
        calls: Arc<AtomicUsize>,
    }

    impl ChunkGenerator for Counting {
        fn generate(&self, _chunk_x: i32, _chunk_z: i32) -> ChunkColumn {
            self.calls.fetch_add(1, Ordering::SeqCst);
            ChunkColumn::empty(0)
        }
    }

    #[test]
    fn each_chunk_generated_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let world = World::new(Box::new(Counting {
            calls: Arc::clone(&calls),
        }));

        let first = world.chunk(2, 5);
        let second = world.chunk(2, 5);
        world.chunk(2, 5);

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(world.loaded_len(), 1);

        world.chunk(2, 6);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(world.loaded_len(), 2);
    }

    #[test]
    fn generator_is_interchangeable() {
        let world = World::new(Box::new(FlatGenerator::new().unwrap()));
        let column = world.chunk(-4, 9);
        assert_eq!(
            state_at(&column, FLOOR_BEDROCK_Y),
            block_state(BEDROCK).unwrap()
        );
    }
}
