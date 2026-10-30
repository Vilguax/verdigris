use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tracing::error;
use verdigris_protocol::{
    ChunkColumn, ChunkGenerator, FlatGenerator, block_index, default_state_of_name, plains_biome_id,
};
use verdigris_worldgen::{Category, ChunkTerrain, WorldGenerator};

pub const GENERATOR_ENV: &str = "VERDIGRIS_GENERATOR";
pub const SEED_ENV: &str = "VERDIGRIS_SEED";
pub const DEFAULT_SEED: i64 = 20_261_029;
pub const GENERATOR_NOISE: &str = "noise";
pub const GENERATOR_FLAT: &str = "flat";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeneratorKind {
    Flat,
    Noise,
}

impl GeneratorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GeneratorKind::Flat => GENERATOR_FLAT,
            GeneratorKind::Noise => GENERATOR_NOISE,
        }
    }
}

pub fn parse_generator_kind(value: &str) -> Result<GeneratorKind, String> {
    match value.trim() {
        GENERATOR_NOISE => Ok(GeneratorKind::Noise),
        GENERATOR_FLAT => Ok(GeneratorKind::Flat),
        other => Err(format!(
            "{GENERATOR_ENV} invalide: {other:?} (attendu {GENERATOR_NOISE:?} ou {GENERATOR_FLAT:?})"
        )),
    }
}

pub fn read_generator_kind() -> Result<GeneratorKind, String> {
    match std::env::var(GENERATOR_ENV) {
        Ok(value) => parse_generator_kind(&value),
        Err(std::env::VarError::NotPresent) => Ok(GeneratorKind::Noise),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("{GENERATOR_ENV} contient une valeur non UTF-8"))
        }
    }
}

pub fn parse_seed(value: &str) -> Result<i64, String> {
    value
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("{SEED_ENV} invalide: {value:?} (entier i64 attendu)"))
}

pub fn read_seed() -> Result<i64, String> {
    match std::env::var(SEED_ENV) {
        Ok(value) => parse_seed(&value),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_SEED),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("{SEED_ENV} contient une valeur non UTF-8"))
        }
    }
}

#[derive(Default)]
pub struct GenMetrics {
    count: AtomicU64,
    nanos: AtomicU64,
}

impl GenMetrics {
    pub fn record(&self, nanos: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.nanos.fetch_add(nanos, Ordering::Relaxed);
    }

    pub fn take(&self) -> (u64, u64) {
        let count = self.count.swap(0, Ordering::Relaxed);
        let nanos = self.nanos.swap(0, Ordering::Relaxed);
        (count, nanos)
    }
}

fn state_of(name: &str) -> Result<u16, String> {
    default_state_of_name(name)
        .map(|state| state as u16)
        .ok_or_else(|| format!("bloc absent du registre: {name}"))
}

pub struct NoiseGenerator {
    generator: Arc<WorldGenerator>,
    metrics: Arc<GenMetrics>,
    stone: u16,
    water: u16,
    lava: u16,
    bedrock: u16,
    biome: u16,
    min_y: i32,
    height: usize,
}

impl NoiseGenerator {
    pub fn new(generator: Arc<WorldGenerator>, metrics: Arc<GenMetrics>) -> Result<Self, String> {
        Ok(NoiseGenerator {
            stone: state_of("minecraft:stone")?,
            water: state_of("minecraft:water")?,
            lava: state_of("minecraft:lava")?,
            bedrock: state_of("minecraft:bedrock")?,
            biome: plains_biome_id().map_err(|err| err.to_string())? as u16,
            min_y: generator.min_y(),
            height: generator.height(),
            generator,
            metrics,
        })
    }

    fn block_state(&self, category: Category) -> u16 {
        match category {
            Category::Stone => self.stone,
            Category::Water => self.water,
            Category::Lava => self.lava,
            Category::Air => 0,
        }
    }

    fn build_column(&self, terrain: &ChunkTerrain) -> ChunkColumn {
        let mut column = ChunkColumn::empty(self.biome);
        for lz in 0..16usize {
            for lx in 0..16usize {
                for j in 0..self.height {
                    let y = self.min_y + j as i32;
                    let state = if y == self.min_y {
                        self.bedrock
                    } else {
                        self.block_state(terrain.category_at(lx, y, lz))
                    };
                    if state != 0 {
                        column.sections[j / 16].states[block_index(lx, j % 16, lz)] = state;
                    }
                }
            }
        }
        column
    }
}

thread_local! {
    static TERRAIN: RefCell<Option<ChunkTerrain>> = const { RefCell::new(None) };
}

impl ChunkGenerator for NoiseGenerator {
    fn generate(&self, chunk_x: i32, chunk_z: i32) -> ChunkColumn {
        let start = Instant::now();
        let column = TERRAIN.with(|cell| {
            let mut slot = cell.borrow_mut();
            let terrain = slot.get_or_insert_with(|| self.generator.terrain());
            match self.generator.fill_chunk(chunk_x, chunk_z, terrain) {
                Ok(()) => self.build_column(terrain),
                Err(err) => {
                    error!(
                        x = chunk_x,
                        z = chunk_z,
                        erreur = %err,
                        "remplissage de chunk échoué, chunk d'air renvoyé"
                    );
                    ChunkColumn::empty(self.biome)
                }
            }
        });
        self.metrics
            .record(start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        column
    }
}

pub enum GeneratorSetup {
    Flat,
    Noise {
        generator: Arc<WorldGenerator>,
        metrics: Arc<GenMetrics>,
    },
}

impl GeneratorSetup {
    pub fn build(kind: GeneratorKind, seed: i64) -> Result<Self, String> {
        match kind {
            GeneratorKind::Flat => Ok(GeneratorSetup::Flat),
            GeneratorKind::Noise => {
                let generator = Arc::new(WorldGenerator::new(seed).map_err(|err| err.to_string())?);
                Ok(GeneratorSetup::Noise {
                    generator,
                    metrics: Arc::new(GenMetrics::default()),
                })
            }
        }
    }

    pub fn metrics(&self) -> Option<Arc<GenMetrics>> {
        match self {
            GeneratorSetup::Flat => None,
            GeneratorSetup::Noise { metrics, .. } => Some(Arc::clone(metrics)),
        }
    }

    pub fn into_generator(self) -> Result<Box<dyn ChunkGenerator>, String> {
        match self {
            GeneratorSetup::Flat => Ok(Box::new(FlatGenerator::new().map_err(|e| e.to_string())?)),
            GeneratorSetup::Noise { generator, metrics } => {
                Ok(Box::new(NoiseGenerator::new(generator, metrics)?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_kind_parsing() {
        assert_eq!(parse_generator_kind("noise"), Ok(GeneratorKind::Noise));
        assert_eq!(parse_generator_kind(" flat "), Ok(GeneratorKind::Flat));
        assert!(parse_generator_kind("plat").is_err());
    }

    #[test]
    fn seed_parsing() {
        assert_eq!(parse_seed("42"), Ok(42));
        assert_eq!(parse_seed(" -7 "), Ok(-7));
        assert!(parse_seed("xyz").is_err());
    }

    #[test]
    fn noise_column_has_bedrock_floor_and_matches_fill() {
        let generator = Arc::new(WorldGenerator::new(DEFAULT_SEED).unwrap());
        let metrics = Arc::new(GenMetrics::default());
        let noise = NoiseGenerator::new(Arc::clone(&generator), metrics).unwrap();
        let column = noise.generate(-7, 0);

        let bedrock = state_of("minecraft:bedrock").unwrap();
        for lz in 0..16usize {
            for lx in 0..16usize {
                assert_eq!(
                    column.sections[0].states[block_index(lx, 0, lz)],
                    bedrock,
                    "bedrock forcé à y=-64"
                );
            }
        }
    }
}
