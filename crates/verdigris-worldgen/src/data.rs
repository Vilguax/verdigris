use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::df::build::{Assets, BuildError, Builder};
use crate::df::node::Node;
use crate::df::volume::{Volume, VolumeContext, eval_volume_with};
use crate::material::compile::Compiler;
use crate::material::{
    FrozenNoise, MatNoises, MaterialRules, Simplex, TemperatureModifier, build_bands, zoom_seed,
};
use crate::octaves::OctaveStack;
use crate::registry::{NoiseRegistry, WorldgenError, worldgen_value};
use crate::sources::{PositionalFactory, aquifer_factory, positional_factory};

#[derive(Debug)]
pub enum DataError {
    Worldgen(WorldgenError),
    Build(BuildError),
    MissingSection(String),
    Material(String),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Worldgen(e) => write!(f, "{e}"),
            Self::Build(e) => write!(f, "{e}"),
            Self::MissingSection(s) => write!(f, "missing worldgen section: {s}"),
            Self::Material(s) => write!(f, "material rules error: {s}"),
        }
    }
}

impl std::error::Error for DataError {}

impl From<WorldgenError> for DataError {
    fn from(e: WorldgenError) -> Self {
        DataError::Worldgen(e)
    }
}

impl From<BuildError> for DataError {
    fn from(e: BuildError) -> Self {
        DataError::Build(e)
    }
}

#[derive(Debug)]
pub struct Router {
    pub chunk_surface_level: Node,
    pub continents: Node,
    pub depth: Node,
    pub erosion: Node,
    pub final_density: Node,
    pub ridges: Node,
    pub temperature: Node,
    pub vegetation: Node,
    pub minmax_eliminated: usize,
    pub min_y: i32,
    pub height: i32,
}

impl Router {
    pub fn final_density_chunk(&self, cx: i32, cz: i32) -> Vec<f32> {
        let origin = [16i32.wrapping_mul(cx), self.min_y, 16i32.wrapping_mul(cz)];
        let vol = Volume::new(origin, [1, 1, 1], [16, self.height.max(0) as usize, 16]);
        let mut ctx = VolumeContext::new();
        match eval_volume_with(&mut ctx, &self.final_density, &vol) {
            Ok(values) => Vec::from(values),
            Err(_) => vec![0.0f32; vol.len()],
        }
    }
}

#[derive(Debug)]
pub struct Aquifers {
    pub barrier: Node,
    pub exclusion: Node,
    pub fluid_level_floodedness: Node,
    pub fluid_level_spread: Node,
    pub lava: Node,
    pub surface_level: Node,
}

pub struct WorldDensity {
    pub final_density: Node,
    pub aquifers: Option<Aquifers>,
    pub factory: PositionalFactory,
    pub min_y: i32,
    pub height: i32,
    pub sea_level: i32,
}

pub struct WorldgenData {
    registry: NoiseRegistry,
    root: &'static Value,
}

impl WorldgenData {
    pub fn embedded() -> Result<Self, DataError> {
        Ok(Self {
            registry: NoiseRegistry::embedded()?,
            root: worldgen_value(),
        })
    }

    fn density_functions(&self) -> Result<&'static Map<String, Value>, DataError> {
        self.root
            .get("density_function")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("density_function".to_string()))
    }

    fn overworld(&self) -> Result<&'static Map<String, Value>, DataError> {
        self.root
            .get("noise_settings")
            .and_then(|v| v.get("minecraft:overworld"))
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("noise_settings/overworld".to_string()))
    }

    pub fn assets(&self, seed: i64) -> Result<Assets, DataError> {
        let instantiated = self.registry.instantiate(seed)?;
        let mut noises = HashMap::with_capacity(instantiated.noises.len());
        for (key, stack) in instantiated.noises {
            noises.insert(key, Arc::new(stack));
        }
        Ok(Assets {
            noises,
            blended_params: *self.registry.blended(),
            blended: Arc::new(instantiated.old_blended),
            factory: positional_factory(seed, false),
        })
    }

    pub fn build_node(&self, value: &Value, assets: &Assets) -> Result<(Node, usize), DataError> {
        let functions = self.density_functions()?;
        let mut builder = Builder::new(functions, assets);
        let node = builder.build(value)?;
        Ok((node, builder.minmax_eliminated))
    }

    pub fn build_all(&self, seed: i64) -> Result<usize, DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let mut count = 0usize;
        for value in functions.values() {
            let mut builder = Builder::new(functions, &assets);
            builder.build(value)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn build_router(&self, seed: i64) -> Result<Router, DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let overworld = self.overworld()?;
        let router = overworld
            .get("noise_router")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("noise_router".to_string()))?;

        let noise = overworld
            .get("noise")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("noise_settings/noise".to_string()))?;
        let read_i32 = |name: &str| -> Result<i32, DataError> {
            noise
                .get(name)
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| DataError::MissingSection(format!("noise_settings/noise/{name}")))
        };
        let min_y = read_i32("min_y")?;
        let height = read_i32("height")?;

        let mut eliminated = 0usize;
        let field = |eliminated: &mut usize, name: &str| -> Result<Node, DataError> {
            let value = router
                .get(name)
                .ok_or_else(|| DataError::MissingSection(format!("noise_router/{name}")))?;
            let mut builder = Builder::new(functions, &assets);
            let node = builder.build(value)?;
            *eliminated += builder.minmax_eliminated;
            Ok(node)
        };

        let chunk_surface_level = field(&mut eliminated, "chunk_surface_level")?;
        let continents = field(&mut eliminated, "continents")?;
        let depth = field(&mut eliminated, "depth")?;
        let erosion = field(&mut eliminated, "erosion")?;
        let final_density = field(&mut eliminated, "final_density")?;
        let ridges = field(&mut eliminated, "ridges")?;
        let temperature = field(&mut eliminated, "temperature")?;
        let vegetation = field(&mut eliminated, "vegetation")?;

        Ok(Router {
            chunk_surface_level,
            continents,
            depth,
            erosion,
            final_density,
            ridges,
            temperature,
            vegetation,
            minmax_eliminated: eliminated,
            min_y,
            height,
        })
    }

    pub fn build_final_density(&self, seed: i64) -> Result<(Node, usize), DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let overworld = self.overworld()?;
        let value = overworld
            .get("noise_router")
            .and_then(|v| v.get("final_density"))
            .ok_or_else(|| DataError::MissingSection("noise_router/final_density".to_string()))?;
        let mut builder = Builder::new(functions, &assets);
        let node = builder.build(value)?;
        Ok((node, builder.minmax_eliminated))
    }

    pub fn build_aquifers(&self, seed: i64) -> Result<Aquifers, DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let overworld = self.overworld()?;
        let aquifers = overworld
            .get("aquifers")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("aquifers".to_string()))?;

        let field = |name: &str| -> Result<Node, DataError> {
            let value = aquifers
                .get(name)
                .ok_or_else(|| DataError::MissingSection(format!("aquifers/{name}")))?;
            let mut builder = Builder::new(functions, &assets);
            Ok(builder.build(value)?)
        };

        Ok(Aquifers {
            barrier: field("barrier")?,
            exclusion: field("exclusion")?,
            fluid_level_floodedness: field("fluid_level_floodedness")?,
            fluid_level_spread: field("fluid_level_spread")?,
            lava: field("lava")?,
            surface_level: field("surface_level")?,
        })
    }

    pub fn build_world_density(&self, seed: i64) -> Result<WorldDensity, DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let overworld = self.overworld()?;

        let noise = overworld
            .get("noise")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("noise_settings/noise".to_string()))?;
        let read_i32 = |name: &str| -> Result<i32, DataError> {
            noise
                .get(name)
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| DataError::MissingSection(format!("noise_settings/noise/{name}")))
        };
        let min_y = read_i32("min_y")?;
        let height = read_i32("height")?;
        let sea_level = overworld
            .get("sea_level")
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or(63);

        let final_value = overworld
            .get("noise_router")
            .and_then(|v| v.get("final_density"))
            .ok_or_else(|| DataError::MissingSection("noise_router/final_density".to_string()))?;
        let mut final_builder = Builder::new(functions, &assets);
        let final_density = final_builder.build(final_value)?;

        let aquifers = match overworld.get("aquifers").and_then(Value::as_object) {
            None => None,
            Some(aq) => {
                let field = |name: &str| -> Result<Node, DataError> {
                    let value = aq
                        .get(name)
                        .ok_or_else(|| DataError::MissingSection(format!("aquifers/{name}")))?;
                    let mut builder = Builder::new(functions, &assets);
                    Ok(builder.build(value)?)
                };
                Some(Aquifers {
                    barrier: field("barrier")?,
                    exclusion: field("exclusion")?,
                    fluid_level_floodedness: field("fluid_level_floodedness")?,
                    fluid_level_spread: field("fluid_level_spread")?,
                    lava: field("lava")?,
                    surface_level: field("surface_level")?,
                })
            }
        };

        let factory = aquifer_factory(&assets.factory);

        Ok(WorldDensity {
            final_density,
            aquifers,
            factory,
            min_y,
            height,
            sea_level,
        })
    }

    pub fn build_biome_source(&self, seed: i64) -> Result<crate::biome::BiomeSource, DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let overworld = self.overworld()?;
        let router = overworld
            .get("noise_router")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("noise_router".to_string()))?;

        let noise = overworld
            .get("noise")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("noise_settings/noise".to_string()))?;
        let read_i32 = |name: &str| -> Result<i32, DataError> {
            noise
                .get(name)
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| DataError::MissingSection(format!("noise_settings/noise/{name}")))
        };
        let min_y = read_i32("min_y")?;
        let height = read_i32("height")?;

        let field = |name: &str| -> Result<Node, DataError> {
            let value = router
                .get(name)
                .ok_or_else(|| DataError::MissingSection(format!("noise_router/{name}")))?;
            let mut builder = Builder::new(functions, &assets);
            Ok(builder.build(value)?)
        };

        Ok(crate::biome::BiomeSource::new(
            field("temperature")?,
            field("vegetation")?,
            field("continents")?,
            field("erosion")?,
            field("depth")?,
            field("ridges")?,
            min_y,
            height.max(0) as usize,
        ))
    }

    pub fn build_material_rules(&self, seed: i64) -> Result<MaterialRules, DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let overworld = self.overworld()?;

        let noise = overworld
            .get("noise")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("noise_settings/noise".to_string()))?;
        let read_i32 = |name: &str| -> Result<i32, DataError> {
            noise
                .get(name)
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| DataError::MissingSection(format!("noise_settings/noise/{name}")))
        };
        let min_y = read_i32("min_y")?;
        let height = read_i32("height")?;
        let sea_level = overworld
            .get("sea_level")
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or(63);

        let root_name = overworld
            .get("material_rule")
            .and_then(Value::as_str)
            .ok_or_else(|| DataError::MissingSection("noise_settings/material_rule".to_string()))?
            .to_string();

        let rules = self
            .root
            .get("material_rule")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("material_rule".to_string()))?;
        let conds = self
            .root
            .get("material_condition")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("material_condition".to_string()))?;

        let biome_names = crate::biome::biome_names();

        let mut compiler = Compiler::new(
            rules,
            conds,
            &assets.noises,
            &assets.factory,
            biome_names,
            sea_level,
        );
        let root = compiler
            .compile_rule(&Value::String(root_name))
            .map_err(|e| DataError::Material(e.to_string()))?;

        let terracotta = compiler.block_idx("minecraft:terracotta");
        let orange = compiler.block_idx("minecraft:orange_terracotta");
        let yellow = compiler.block_idx("minecraft:yellow_terracotta");
        let brown = compiler.block_idx("minecraft:brown_terracotta");
        let red = compiler.block_idx("minecraft:red_terracotta");
        let white = compiler.block_idx("minecraft:white_terracotta");
        let light_gray = compiler.block_idx("minecraft:light_gray_terracotta");
        let snow_block_idx = compiler.block_idx("minecraft:snow_block");
        let packed_ice_idx = compiler.block_idx("minecraft:packed_ice");

        let air_idx = compiler.air_idx();
        let water_idx = compiler.water_idx();
        let lava_idx = compiler.lava_idx();
        let stone_idx = compiler.stone_idx();

        let bands = build_bands(
            assets.factory.from_hash_of("minecraft:clay_bands"),
            terracotta,
            orange,
            yellow,
            brown,
            red,
            white,
            light_gray,
        );

        let blocks = compiler.blocks.clone();

        let build_df = |key: &str| -> Result<Node, DataError> {
            let mut b = Builder::new(functions, &assets);
            Ok(b.build(&Value::String(key.to_string()))?)
        };
        let copper_density = build_df("minecraft:overworld/ore_vein/copper_density")?;
        let iron_density = build_df("minecraft:overworld/ore_vein/iron_density")?;
        let richness = build_df("minecraft:overworld/ore_vein/richness")?;
        let gap = build_df("minecraft:overworld/ore_vein/gap")?;
        let prelim_surface = build_df("minecraft:overworld/preliminary_surface_level")?;

        let get_noise = |key: &str| -> Result<std::sync::Arc<OctaveStack>, DataError> {
            assets
                .noises
                .get(key)
                .cloned()
                .ok_or_else(|| DataError::Material(format!("missing noise {key}")))
        };
        let noises = MatNoises {
            surface: get_noise("minecraft:surface")?,
            surface_secondary: get_noise("minecraft:surface_secondary")?,
            clay_bands_offset: get_noise("minecraft:clay_bands_offset")?,
            badlands_surface: get_noise("minecraft:badlands_surface")?,
            badlands_pillar: get_noise("minecraft:badlands_pillar")?,
            badlands_pillar_roof: get_noise("minecraft:badlands_pillar_roof")?,
            iceberg_surface: get_noise("minecraft:iceberg_surface")?,
            iceberg_pillar: get_noise("minecraft:iceberg_pillar")?,
            iceberg_pillar_roof: get_noise("minecraft:iceberg_pillar_roof")?,
        };

        let ore_factory = assets
            .factory
            .from_hash_of("minecraft:ore")
            .fork_positional();
        let factory = assets.factory.clone();

        let biome_map = self
            .root
            .get("biome")
            .and_then(Value::as_object)
            .ok_or_else(|| DataError::MissingSection("biome".to_string()))?;
        let mut biome_temps = vec![(0.0f32, TemperatureModifier::None); biome_names.len()];
        for (id, slot) in biome_temps.iter_mut().enumerate() {
            if let Some(name) = biome_names.name_of(id as u16)
                && let Some(b) = biome_map.get(name).and_then(Value::as_object)
            {
                let t0 = b.get("temperature").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let modi = match b.get("temperature_modifier").and_then(Value::as_str) {
                    Some("frozen") => TemperatureModifier::Frozen,
                    _ => TemperatureModifier::None,
                };
                *slot = (t0, modi);
            }
        }

        let biome_source = self.build_biome_source(seed)?;

        Ok(MaterialRules {
            root,
            blocks,
            air_idx,
            water_idx,
            lava_idx,
            stone_idx,
            copper_density,
            iron_density,
            richness,
            gap,
            prelim_surface,
            ore_factory,
            factory,
            noises,
            bands,
            frozen: FrozenNoise::new(),
            temp_simplex: Simplex::from_lcg(1234),
            biome_temps,
            zoom_seed: zoom_seed(seed),
            biome_source,
            min_y,
            height: height.max(0) as usize,
            sea_level,
            eroded_badlands_id: biome_names.id_of("minecraft:eroded_badlands"),
            frozen_ocean_id: biome_names.id_of("minecraft:frozen_ocean"),
            deep_frozen_ocean_id: biome_names.id_of("minecraft:deep_frozen_ocean"),
            snow_block_idx,
            packed_ice_idx,
        })
    }

    pub fn build_named(&self, key: &str, seed: i64) -> Result<(Node, usize), DataError> {
        let assets = self.assets(seed)?;
        let functions = self.density_functions()?;
        let value = functions
            .get(key)
            .ok_or_else(|| DataError::MissingSection(key.to_string()))?;
        let mut builder = Builder::new(functions, &assets);
        let node = builder.build(value)?;
        Ok((node, builder.minmax_eliminated))
    }
}
