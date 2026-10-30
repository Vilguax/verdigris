use crate::aquifer::{AquiferFunctions, AquiferMode, AquiferScratch, Filled};
use crate::data::{DataError, WorldgenData};
use crate::df::node::Node;
use crate::df::volume::{Volume, VolumeContext, VolumeError, eval_volume_with};

pub const CHUNK_WIDTH: usize = 16;
pub const LAVA_CEILING: i32 = -54;
pub const SEA_LEVEL: i32 = 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Stone,
    Water,
    Lava,
    Air,
}

impl Category {
    pub fn is_solid(self) -> bool {
        matches!(self, Category::Stone)
    }
}

pub struct WorldGenerator {
    final_density: Node,
    aquifers: Option<AquiferFunctions>,
    mode: AquiferMode,
    seed: i64,
    min_y: i32,
    height: usize,
}

impl WorldGenerator {
    pub fn new(seed: i64) -> Result<Self, DataError> {
        Self::with_mode(seed, AquiferMode::Noise)
    }

    pub fn with_mode(seed: i64, mode: AquiferMode) -> Result<Self, DataError> {
        let data = WorldgenData::embedded()?;
        let world = data.build_world_density(seed)?;
        let crate::data::WorldDensity {
            final_density,
            aquifers,
            factory,
            min_y,
            height,
            sea_level,
        } = world;
        let aquifers = aquifers.map(|aq| AquiferFunctions {
            barrier: aq.barrier,
            exclusion: aq.exclusion,
            floodedness: aq.fluid_level_floodedness,
            spread: aq.fluid_level_spread,
            lava: aq.lava,
            surface_level: aq.surface_level,
            factory,
            sea_level,
        });
        if matches!(mode, AquiferMode::Noise) && aquifers.is_none() {
            return Err(DataError::MissingSection("aquifers".to_string()));
        }
        Ok(Self {
            final_density,
            aquifers,
            mode,
            seed,
            min_y,
            height: height.max(0) as usize,
        })
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    pub fn mode(&self) -> AquiferMode {
        self.mode
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn terrain(&self) -> ChunkTerrain {
        ChunkTerrain::new(self.min_y, self.height)
    }

    pub fn fill_chunk(&self, cx: i32, cz: i32, out: &mut ChunkTerrain) -> Result<(), VolumeError> {
        out.ensure(self.min_y, self.height);
        let origin = [CHUNK_WIDTH as i32 * cx, self.min_y, CHUNK_WIDTH as i32 * cz];
        let vol = Volume::new(origin, [1, 1, 1], [CHUNK_WIDTH, self.height, CHUNK_WIDTH]);
        let values = eval_volume_with(&mut out.ctx, &self.final_density, &vol)?;
        out.density.clear();
        out.density.extend_from_slice(&values);
        out.post.fill(0);

        match (self.mode, &self.aquifers) {
            (AquiferMode::Noise, Some(funcs)) => {
                let x0 = CHUNK_WIDTH as i32 * cx;
                let z0 = CHUNK_WIDTH as i32 * cz;
                let ymax = self.min_y + self.height as i32 - 1;
                out.aquifer
                    .prepare(funcs, x0, x0 + 15, self.min_y, ymax, z0, z0 + 15);
                for lz in 0..CHUNK_WIDTH {
                    for lx in 0..CHUNK_WIDTH {
                        let x = x0 + lx as i32;
                        let z = z0 + lz as i32;
                        for j in 0..self.height {
                            let y = self.min_y + j as i32;
                            let idx = j + (lx + CHUNK_WIDTH * lz) * self.height;
                            let d = f64::from(out.density[idx]);
                            let (filled, phi) = out.aquifer.decide(funcs, x, y, z, d);
                            let cat = match filled {
                                Filled::Nothing => Category::Stone,
                                Filled::Air => Category::Air,
                                Filled::Water => Category::Water,
                                Filled::Lava => Category::Lava,
                            };
                            out.categories[idx] = cat;
                            if phi && matches!(cat, Category::Water | Category::Lava) {
                                out.post[idx >> 6] |= 1u64 << (idx & 63);
                            }
                        }
                    }
                }
            }
            _ => {
                for (idx, &d) in out.density.iter().enumerate() {
                    let j = idx % self.height;
                    let y = self.min_y + j as i32;
                    out.categories[idx] = category_for(d, y);
                }
            }
        }
        Ok(())
    }

    pub fn column_base_height(&self, x: i32, z: i32) -> Result<Option<i32>, VolumeError> {
        let origin = [x, self.min_y, z];
        let vol = Volume::new(origin, [1, 1, 1], [1, self.height, 1]);
        let mut ctx = VolumeContext::new();
        let density = eval_volume_with(&mut ctx, &self.final_density, &vol)?;
        match (self.mode, &self.aquifers) {
            (AquiferMode::Noise, Some(funcs)) => {
                let mut scratch = AquiferScratch::new();
                Ok(scratch.column_base_height(
                    funcs,
                    x,
                    z,
                    self.min_y,
                    self.height as i32,
                    &density,
                ))
            }
            _ => {
                let mut y = self.min_y + self.height as i32 - 1;
                while y >= self.min_y {
                    let j = (y - self.min_y) as usize;
                    if category_for(density[j], y) != Category::Air {
                        return Ok(Some(y));
                    }
                    y -= 1;
                }
                Ok(None)
            }
        }
    }
}

pub fn category_for(density: f32, y: i32) -> Category {
    if f64::from(density) > 0.0 {
        Category::Stone
    } else if y < LAVA_CEILING {
        Category::Lava
    } else if y < SEA_LEVEL {
        Category::Water
    } else {
        Category::Air
    }
}

pub struct ChunkTerrain {
    min_y: i32,
    height: usize,
    categories: Vec<Category>,
    density: Vec<f32>,
    post: Vec<u64>,
    ctx: VolumeContext,
    aquifer: AquiferScratch,
}

impl ChunkTerrain {
    pub fn new(min_y: i32, height: usize) -> Self {
        let len = CHUNK_WIDTH * height * CHUNK_WIDTH;
        Self {
            min_y,
            height,
            categories: vec![Category::Air; len],
            density: Vec::with_capacity(len),
            post: vec![0u64; len.div_ceil(64)],
            ctx: VolumeContext::new(),
            aquifer: AquiferScratch::new(),
        }
    }

    fn ensure(&mut self, min_y: i32, height: usize) {
        if self.min_y != min_y || self.height != height {
            *self = ChunkTerrain::new(min_y, height);
        }
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    pub fn height(&self) -> usize {
        self.height
    }

    fn index(&self, lx: usize, y: i32, lz: usize) -> usize {
        let j = (y - self.min_y) as usize;
        j + (lx + CHUNK_WIDTH * lz) * self.height
    }

    pub fn category_at(&self, lx: usize, y: i32, lz: usize) -> Category {
        self.categories[self.index(lx, y, lz)]
    }

    pub fn density_at(&self, lx: usize, y: i32, lz: usize) -> f32 {
        self.density[self.index(lx, y, lz)]
    }

    pub fn post_flag_at(&self, lx: usize, y: i32, lz: usize) -> bool {
        let idx = self.index(lx, y, lz);
        (self.post[idx >> 6] >> (idx & 63)) & 1 != 0
    }

    pub fn categories(&self) -> &[Category] {
        &self.categories
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_rule_is_strict_positive() {
        assert_eq!(category_for(1.0, 0), Category::Stone);
        assert_eq!(category_for(0.0, 0), Category::Water);
        assert_eq!(category_for(-0.0, 0), Category::Water);
        assert_eq!(category_for(f32::NAN, 0), Category::Water);
    }

    #[test]
    fn fluid_rule_follows_altitude() {
        assert_eq!(category_for(0.0, -55), Category::Lava);
        assert_eq!(category_for(0.0, -54), Category::Water);
        assert_eq!(category_for(0.0, 62), Category::Water);
        assert_eq!(category_for(0.0, 63), Category::Air);
    }

    #[test]
    fn density_buffer_matches_router_bit_for_bit() {
        use crate::data::WorldgenData;
        let data = WorldgenData::embedded().expect("data");
        let router = data.build_router(20261029).expect("router");
        let reference = router.final_density_chunk(3, -2);

        let generator =
            WorldGenerator::with_mode(20261029, AquiferMode::Disabled).expect("generator");
        let mut terrain = generator.terrain();
        generator.fill_chunk(3, -2, &mut terrain).expect("fill");

        assert_eq!(terrain.density.len(), reference.len());
        for (idx, r) in reference.iter().enumerate() {
            assert_eq!(
                terrain.density[idx].to_bits(),
                r.to_bits(),
                "density mismatch at index {idx}"
            );
        }
    }

    #[test]
    fn fill_assigns_bottom_bedrock_band_as_stone_or_lava() {
        let generator = WorldGenerator::new(20261029).expect("generator");
        let mut terrain = generator.terrain();
        generator.fill_chunk(-7, 0, &mut terrain).expect("fill");
        for lx in 0..CHUNK_WIDTH {
            for lz in 0..CHUNK_WIDTH {
                let c = terrain.category_at(lx, generator.min_y(), lz);
                assert!(matches!(c, Category::Stone | Category::Lava));
            }
        }
    }

    #[test]
    fn disabled_mode_matches_simple_rule() {
        let generator =
            WorldGenerator::with_mode(20261029, AquiferMode::Disabled).expect("generator");
        let mut terrain = generator.terrain();
        generator.fill_chunk(3, -2, &mut terrain).expect("fill");
        for (idx, &d) in terrain.density.iter().enumerate() {
            let j = idx % generator.height();
            let y = generator.min_y() + j as i32;
            assert_eq!(terrain.categories[idx], category_for(d, y));
        }
    }
}
