use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::blocks::default_state_of_name;
use crate::chunk::{ChunkColumn, MIN_Y, WORLD_HEIGHT, block_index};
use crate::play::{PlayError, plains_biome_id};

pub const BEDROCK: &str = "minecraft:bedrock";
pub const DIRT: &str = "minecraft:dirt";
pub const GRASS_BLOCK: &str = "minecraft:grass_block";

pub const BLOCK_REGISTRY: &str = "minecraft:block";

pub const WORLD_MIN_Y: i32 = MIN_Y;
pub const WORLD_MAX_Y: i32 = MIN_Y + WORLD_HEIGHT as i32 - 1;

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

pub type SharedChunk = Arc<Mutex<ChunkColumn>>;

pub struct World {
    generator: Box<dyn ChunkGenerator>,
    cache: Mutex<HashMap<(i32, i32), SharedChunk>>,
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

    pub fn chunk(&self, chunk_x: i32, chunk_z: i32) -> SharedChunk {
        let mut cache = self.cache.lock().unwrap();
        cache
            .entry((chunk_x, chunk_z))
            .or_insert_with(|| Arc::new(Mutex::new(self.generator.generate(chunk_x, chunk_z))))
            .clone()
    }

    pub fn state_at(&self, x: i32, y: i32, z: i32) -> u16 {
        if !(WORLD_MIN_Y..=WORLD_MAX_Y).contains(&y) {
            return 0;
        }
        let column = self.chunk(x.div_euclid(16), z.div_euclid(16));
        let column = column.lock().unwrap();
        column.state_at(
            x.rem_euclid(16) as usize,
            (y - WORLD_MIN_Y) as usize,
            z.rem_euclid(16) as usize,
        )
    }

    pub fn set_state(&self, x: i32, y: i32, z: i32, state: u16) {
        if !(WORLD_MIN_Y..=WORLD_MAX_Y).contains(&y) {
            return;
        }
        let column = self.chunk(x.div_euclid(16), z.div_euclid(16));
        let mut column = column.lock().unwrap();
        let rel = (y - WORLD_MIN_Y) as usize;
        let index = block_index(
            x.rem_euclid(16) as usize,
            rel % 16,
            z.rem_euclid(16) as usize,
        );
        column.sections[rel / 16].states[index] = state;
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
        let column = column.lock().unwrap();
        assert_eq!(
            state_at(&column, FLOOR_BEDROCK_Y),
            block_state(BEDROCK).unwrap()
        );
    }

    #[test]
    fn set_and_read_state_round_trip() {
        let world = World::flat().unwrap();
        assert_eq!(world.state_at(3, -60, -7), 0);
        world.set_state(3, -60, -7, 10);
        assert_eq!(world.state_at(3, -60, -7), 10);
    }

    #[test]
    fn mutation_survives_chunk_reread() {
        let world = World::flat().unwrap();
        world.set_state(-1, 10, -1, 1);
        let column = world.chunk((-1i32).div_euclid(16), (-1i32).div_euclid(16));
        let column = column.lock().unwrap();
        let rel = (10 - WORLD_MIN_Y) as usize;
        let index = block_index(
            (-1i32).rem_euclid(16) as usize,
            rel % 16,
            (-1i32).rem_euclid(16) as usize,
        );
        assert_eq!(column.sections[rel / 16].states[index], 1);
    }

    #[test]
    fn section_counters_follow_mutations_on_resend() {
        use crate::chunk::{write_chunk_payload, write_section};

        const WATER: u16 = 89;
        let dirt = block_state(DIRT).unwrap();
        let world = World::flat().unwrap();

        world.set_state(0, -49, 0, WATER);
        world.set_state(0, -48, 0, dirt);
        world.set_state(0, -61, 0, 0);

        let shared = world.chunk(0, 0);
        let column = shared.lock().unwrap();

        let mut section0 = Vec::new();
        write_section(&column.sections[0], &mut section0);
        assert_eq!(u16::from_be_bytes([section0[0], section0[1]]), 1024);
        assert_eq!(u16::from_be_bytes([section0[2], section0[3]]), 1);

        let mut section1 = Vec::new();
        write_section(&column.sections[1], &mut section1);
        assert_eq!(u16::from_be_bytes([section1[0], section1[1]]), 1);
        assert_eq!(u16::from_be_bytes([section1[2], section1[3]]), 0);

        let mut emitted = Vec::new();
        write_chunk_payload(0, 0, &column, &mut emitted);

        let mut reference = FlatGenerator::new().unwrap().generate(0, 0);
        reference.sections[0].states[block_index(0, 15, 0)] = WATER;
        reference.sections[1].states[block_index(0, 0, 0)] = dirt;
        reference.sections[0].states[block_index(0, 3, 0)] = 0;
        let mut expected = Vec::new();
        write_chunk_payload(0, 0, &reference, &mut expected);

        assert_eq!(
            emitted, expected,
            "le chunk réémis doit correspondre aux states mutés"
        );
    }

    #[test]
    fn out_of_bounds_reads_air_and_ignores_writes() {
        let world = World::flat().unwrap();
        assert_eq!(world.state_at(0, -65, 0), 0);
        assert_eq!(world.state_at(0, 320, 0), 0);
        world.set_state(0, -65, 0, 1);
        world.set_state(0, 320, 0, 1);
        assert_eq!(world.state_at(0, -65, 0), 0);
        assert_eq!(world.state_at(0, 320, 0), 0);
    }
}
