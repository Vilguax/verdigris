use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tracing::error;
use verdigris_protocol::{
    ChunkColumn, ChunkGenerator, FlatGenerator, biome_index, block_index, default_state_of_name,
    plains_biome_id,
};
use verdigris_worldgen::{
    AquiferMode, BiomeSource, ChunkTerrain, SearchState, WorldGenerator, WorldgenData,
};

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
    biomes: Arc<BiomeSource>,
    metrics: Arc<GenMetrics>,
    states: Vec<u16>,
    biome: u16,
    min_y: i32,
    height: usize,
}

impl NoiseGenerator {
    pub fn new(generator: Arc<WorldGenerator>, metrics: Arc<GenMetrics>) -> Result<Self, String> {
        let data = WorldgenData::embedded().map_err(|err| err.to_string())?;
        let biomes = data
            .build_biome_source(generator.seed())
            .map_err(|err| err.to_string())?;
        let states = generator
            .material()
            .blocks
            .iter()
            .map(|name| state_of(name))
            .collect::<Result<Vec<u16>, String>>()?;
        Ok(NoiseGenerator {
            states,
            biome: plains_biome_id().map_err(|err| err.to_string())? as u16,
            min_y: generator.min_y(),
            height: generator.height(),
            generator,
            biomes: Arc::new(biomes),
            metrics,
        })
    }

    fn build_column(&self, terrain: &ChunkTerrain, biome_cells: &[u16]) -> ChunkColumn {
        let mut column = ChunkColumn::empty(self.biome);
        for lz in 0..16usize {
            for lx in 0..16usize {
                for j in 0..self.height {
                    let y = self.min_y + j as i32;
                    let state = self.states[terrain.block_id_at(lx, y, lz) as usize];
                    if state != 0 {
                        column.sections[j / 16].states[block_index(lx, j % 16, lz)] = state;
                    }
                }
            }
        }
        for (j, section) in column.sections.iter_mut().enumerate() {
            for yl in 0..4usize {
                let qy = j * 4 + yl;
                for qz in 0..4usize {
                    for qx in 0..4usize {
                        section.biomes[biome_index(qx, yl, qz)] =
                            biome_cells[qy * 16 + qz * 4 + qx];
                    }
                }
            }
        }
        column
    }
}

struct Scratch {
    terrain: ChunkTerrain,
    biome_cells: Vec<u16>,
    state: SearchState,
}

thread_local! {
    static SCRATCH: RefCell<Option<Scratch>> = const { RefCell::new(None) };
}

impl ChunkGenerator for NoiseGenerator {
    fn generate(&self, chunk_x: i32, chunk_z: i32) -> ChunkColumn {
        let start = Instant::now();
        let column = SCRATCH.with(|cell| {
            let mut slot = cell.borrow_mut();
            let scratch = slot.get_or_insert_with(|| Scratch {
                terrain: self.generator.terrain(),
                biome_cells: vec![self.biome; self.biomes.cell_count()],
                state: SearchState::new(),
            });
            self.biomes.fill_chunk(
                chunk_x,
                chunk_z,
                &mut scratch.state,
                &mut scratch.biome_cells,
            );
            match self
                .generator
                .fill_chunk(chunk_x, chunk_z, &mut scratch.terrain)
            {
                Ok(()) => self.build_column(&scratch.terrain, &scratch.biome_cells),
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
                let generator = Arc::new(
                    WorldGenerator::with_options(seed, AquiferMode::Noise, true)
                        .map_err(|err| err.to_string())?,
                );
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
    fn noise_column_matches_material_block_grid() {
        use verdigris_worldgen::chunk_fill::CHUNK_WIDTH;
        let generator = Arc::new(WorldGenerator::new(DEFAULT_SEED).unwrap());
        let metrics = Arc::new(GenMetrics::default());
        let noise = NoiseGenerator::new(Arc::clone(&generator), metrics).unwrap();
        let column = noise.generate(-7, 0);

        let mut terrain = generator.terrain();
        generator.fill_chunk(-7, 0, &mut terrain).unwrap();
        let states: Vec<u16> = generator
            .material()
            .blocks
            .iter()
            .map(|name| state_of(name).unwrap())
            .collect();

        let min_y = generator.min_y();
        let bedrock = state_of("minecraft:bedrock").unwrap();
        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                for j in 0..generator.height() {
                    let y = min_y + j as i32;
                    let want = states[terrain.block_id_at(lx, y, lz) as usize];
                    let got = column.sections[j / 16].states[block_index(lx, j % 16, lz)];
                    if want != 0 {
                        assert_eq!(got, want, "état incohérent en ({lx},{y},{lz})");
                    }
                }
            }
        }
        let mut seen_bedrock = false;
        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                if column.sections[0].states[block_index(lx, 0, lz)] == bedrock {
                    seen_bedrock = true;
                }
            }
        }
        assert!(
            seen_bedrock,
            "au moins une colonne a de la bedrock en y=-64"
        );
    }
}
