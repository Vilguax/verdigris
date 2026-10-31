pub mod compile;
pub mod special;

use std::sync::Arc;

use crate::arith::{floor_i32, jabs_f64, jmin_f64, l};
use crate::biome::BiomeSource;
use crate::chunk_fill::{CHUNK_WIDTH, Category, ChunkTerrain};
use crate::df::eval::eval_point;
use crate::df::node::Node;
use crate::df::volume::{Volume, VolumeError, eval_volume_with};
use crate::octaves::OctaveStack;
use crate::sources::PositionalFactory;

pub use compile::{CCond, CRule, MaterialError, OreKind, OreVein};
pub use special::{FrozenNoise, Simplex, SimplexStack, TemperatureModifier, zoom_seed};

const ZOOM_A: i64 = 6_364_136_223_846_793_005;
const ZOOM_B: i64 = 1_442_695_040_888_963_407;

#[derive(Clone, Debug)]
pub struct MatNoises {
    pub surface: Arc<OctaveStack>,
    pub surface_secondary: Arc<OctaveStack>,
    pub clay_bands_offset: Arc<OctaveStack>,
    pub badlands_surface: Arc<OctaveStack>,
    pub badlands_pillar: Arc<OctaveStack>,
    pub badlands_pillar_roof: Arc<OctaveStack>,
    pub iceberg_surface: Arc<OctaveStack>,
    pub iceberg_pillar: Arc<OctaveStack>,
    pub iceberg_pillar_roof: Arc<OctaveStack>,
}

pub struct MaterialRules {
    pub root: CRule,
    pub blocks: Vec<String>,
    pub air_idx: u16,
    pub water_idx: u16,
    pub lava_idx: u16,
    pub stone_idx: u16,
    pub copper_density: Node,
    pub iron_density: Node,
    pub richness: Node,
    pub gap: Node,
    pub prelim_surface: Node,
    pub ore_factory: PositionalFactory,
    pub factory: PositionalFactory,
    pub noises: MatNoises,
    pub bands: [u16; 192],
    pub frozen: FrozenNoise,
    pub temp_simplex: Simplex,
    pub biome_temps: Vec<(f32, TemperatureModifier)>,
    pub zoom_seed: i64,
    pub biome_source: BiomeSource,
    pub min_y: i32,
    pub height: usize,
    pub sea_level: i32,
    pub eroded_badlands_id: Option<u16>,
    pub frozen_ocean_id: Option<u16>,
    pub deep_frozen_ocean_id: Option<u16>,
    pub snow_block_idx: u16,
    pub packed_ice_idx: u16,
}

#[allow(clippy::too_many_arguments)]
pub fn build_bands(
    mut r: crate::sources::RootSource,
    terracotta: u16,
    orange: u16,
    yellow: u16,
    brown: u16,
    red: u16,
    white: u16,
    light_gray: u16,
) -> [u16; 192] {
    let mut t = [terracotta; 192];
    let mut i = 0i32;
    while i < 192 {
        i += r.next_int_bounded(5) + 1;
        if i < 192 {
            t[i as usize] = orange;
        }
        i += 1;
    }
    for (base, color) in [(1i32, yellow), (2, brown), (1, red)] {
        let n = r.next_int_bounded(10) + 6;
        for _ in 0..n {
            let w = base + r.next_int_bounded(3);
            let s = r.next_int_bounded(192);
            for p in 0..w {
                let idx = s + p;
                if idx < 192 {
                    t[idx as usize] = color;
                }
            }
        }
    }
    let n = r.next_int_bounded(7) + 9;
    let mut k = 0i32;
    let mut s = 0i32;
    while k < n && s < 192 {
        t[s as usize] = white;
        if s - 1 > 0 && r.next_boolean() {
            t[(s - 1) as usize] = light_gray;
        }
        if s + 1 < 192 && r.next_boolean() {
            t[(s + 1) as usize] = light_gray;
        }
        k += 1;
        s += r.next_int_bounded(16) + 4;
    }
    t
}

struct BlockCtx {
    x: i32,
    z: i32,
    y: i32,
    lx: usize,
    lz: usize,
    a: i32,
    bn: i32,
    w: Option<i32>,
    d: i32,
    s2: f64,
    m: i32,
    gx: i32,
    gz: i32,
    biome: u16,
}

struct OreBufs<'a> {
    copper: &'a [f32],
    iron: &'a [f32],
    rich: &'a [f32],
}

fn zoom_n(r: i64, c: i64) -> i64 {
    let inner = r.wrapping_mul(ZOOM_A).wrapping_add(ZOOM_B);
    r.wrapping_mul(inner).wrapping_add(c)
}

fn zoom_f(r: i64) -> f64 {
    let shifted = r >> 24;
    let m = shifted.rem_euclid(1024);
    (m as f64 / 1024.0 - 0.5) * 0.9
}

impl MaterialRules {
    fn block_index(&self, cat: Category) -> u16 {
        match cat {
            Category::Air => self.air_idx,
            Category::Water => self.water_idx,
            Category::Lava => self.lava_idx,
            Category::Stone => self.stone_idx,
        }
    }

    pub fn apply(&self, cx: i32, cz: i32, out: &mut ChunkTerrain) -> Result<(), VolumeError> {
        let min_y = self.min_y;
        let height = self.height;
        let max_y = min_y + height as i32 - 1;
        let x0 = CHUNK_WIDTH as i32 * cx;
        let z0 = CHUNK_WIDTH as i32 * cz;
        let qy_count = height / 4;
        let cells = qy_count * 16;

        out.ensure_material(min_y, height);

        for idx in 0..out.categories.len() {
            out.blocks[idx] = self.block_index(out.categories[idx]);
        }

        let vol = Volume::new(
            [x0, min_y, z0],
            [1, 1, 1],
            [CHUNK_WIDTH, height, CHUNK_WIDTH],
        );
        let cd = eval_volume_with(&mut out.ctx, &self.copper_density, &vol)?;
        out.ore_copper.clear();
        out.ore_copper.extend_from_slice(&cd);
        let id = eval_volume_with(&mut out.ctx, &self.iron_density, &vol)?;
        out.ore_iron.clear();
        out.ore_iron.extend_from_slice(&id);
        let rd = eval_volume_with(&mut out.ctx, &self.richness, &vol)?;
        out.ore_rich.clear();
        out.ore_rich.extend_from_slice(&rd);

        out.biome_grids.resize(9 * cells, 0);
        for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                let goff = (((dz + 1) * 3 + (dx + 1)) as usize) * cells;
                self.biome_source.fill_chunk(
                    cx + dx,
                    cz + dz,
                    &mut out.bsearch,
                    &mut out.biome_grids[goff..goff + cells],
                );
            }
        }

        let c00 = eval_point(&self.prelim_surface, x0, 0, z0);
        let c01 = eval_point(&self.prelim_surface, x0, 0, z0 + 16);
        let c10 = eval_point(&self.prelim_surface, x0 + 16, 0, z0);
        let c11 = eval_point(&self.prelim_surface, x0 + 16, 0, z0 + 16);

        for lz in 0..CHUNK_WIDTH {
            for lx in 0..CHUNK_WIDTH {
                let col = lx + CHUNK_WIDTH * lz;
                let mut top = min_y - 1;
                let base = (lx + CHUNK_WIDTH * lz) * height;
                for j in (0..height).rev() {
                    if out.categories[base + j] != Category::Air {
                        top = min_y + j as i32;
                        break;
                    }
                }
                out.ws[col] = top;
            }
        }

        for lx in 0..CHUNK_WIDTH {
            for lz in 0..CHUNK_WIDTH {
                let col = lx + CHUNK_WIDTH * lz;
                let x = x0 + lx as i32;
                let z = z0 + lz as i32;

                let h0 = out.ws[col];
                let s0 = h0 + 1;
                let bs = self.biome_at(&out.biome_grids, cells, cx, cz, qy_count, x, s0, z);

                if Some(bs) == self.eroded_badlands_id {
                    self.eroded_badlands_ext(x, z, s0, lx, lz, out);
                }

                let h = out.ws[col] + 1;
                let col_r = usize::min(lx + 1, 15) + CHUNK_WIDTH * lz;
                let col_l = lx.saturating_sub(1) + CHUNK_WIDTH * lz;
                let col_f = lx + CHUNK_WIDTH * usize::min(lz + 1, 15);
                let col_b = lx + CHUNK_WIDTH * lz.saturating_sub(1);
                let gx = out.ws[col_r] - out.ws[col_l];
                let gz = out.ws[col_f] - out.ws[col_b];

                let sn = self
                    .noises
                    .surface
                    .sample_point(f64::from(x), 0.0, f64::from(z), None);
                let r = self.factory.at(x, 0, z).next_double();
                let d = surface_depth_offset(sn, r);
                let s2 = f64::from(self.noises.surface_secondary.sample_point(
                    f64::from(x),
                    0.0,
                    f64::from(z),
                    None,
                ));

                let az = (lz as f32) * 0.0625;
                let ax = (lx as f32) * 0.0625;
                let p0 = l(az, c00, c01);
                let p1 = l(az, c10, c11);
                let v = l(ax, p0, p1);
                let m = floor_i32(f64::from(v)) + d - 8;

                let base = col * height;
                {
                    let mut below = min_y - 1;
                    for j in 0..height {
                        let cat = out.categories[base + j];
                        if cat == Category::Stone {
                            out.bn[j] = (min_y + j as i32) - below;
                        } else {
                            below = min_y + j as i32;
                            out.bn[j] = 0;
                        }
                    }
                }

                let mut a = 0i32;
                let mut w: Option<i32> = None;
                let mut y = i32::min(h, max_y);
                while y >= min_y {
                    let j = (y - min_y) as usize;
                    let idx = base + j;
                    match out.categories[idx] {
                        Category::Air => {
                            a = 0;
                            w = None;
                        }
                        Category::Water | Category::Lava => {
                            if w.is_none() {
                                w = Some(y + 1);
                            }
                        }
                        Category::Stone => {
                            a += 1;
                            let bn = out.bn[j];
                            let biome =
                                self.biome_at(&out.biome_grids, cells, cx, cz, qy_count, x, y, z);
                            let ctx = BlockCtx {
                                x,
                                z,
                                y,
                                lx,
                                lz,
                                a,
                                bn,
                                w,
                                d,
                                s2,
                                m,
                                gx,
                                gz,
                                biome,
                            };
                            let result = {
                                let bufs = OreBufs {
                                    copper: &out.ore_copper,
                                    iron: &out.ore_iron,
                                    rich: &out.ore_rich,
                                };
                                self.eval_rule(&self.root, &ctx, &bufs)
                            };
                            if let Some(bi) = result {
                                out.blocks[idx] = bi;
                                if bi == self.water_idx || bi == self.lava_idx {
                                    out.post[idx >> 6] |= 1u64 << (idx & 63);
                                }
                                if bi == self.air_idx && y == out.ws[col] {
                                    let mut yy = y - 1;
                                    while yy >= min_y
                                        && out.blocks[base + (yy - min_y) as usize] == self.air_idx
                                    {
                                        yy -= 1;
                                    }
                                    out.ws[col] = yy;
                                }
                            }
                        }
                    }
                    y -= 1;
                }

                if Some(bs) == self.frozen_ocean_id || Some(bs) == self.deep_frozen_ocean_id {
                    self.iceberg_ext(x, z, s0, m, bs, lx, lz, out);
                }
            }
        }

        Ok(())
    }

    fn eroded_badlands_ext(
        &self,
        x: i32,
        z: i32,
        s0: i32,
        lx: usize,
        lz: usize,
        out: &mut ChunkTerrain,
    ) {
        let min_y = self.min_y;
        let height = self.height;
        let col = lx + CHUNK_WIDTH * lz;
        let base = col * height;

        let n_bs = f64::from(self.noises.badlands_surface.sample_point(
            f64::from(x),
            0.0,
            f64::from(z),
            None,
        ));
        let aa = jabs_f64(n_bs * 8.25);
        let np = self.noises.badlands_pillar.sample_point(
            f64::from(x) * 0.2,
            0.0,
            f64::from(z) * 0.2,
            None,
        );
        let bb = f64::from(np * 15.0);
        let beta = jmin_f64(aa, bb);
        if beta <= 0.0 {
            return;
        }
        let n_roof = f64::from(self.noises.badlands_pillar_roof.sample_point(
            f64::from(x) * 0.75,
            0.0,
            f64::from(z) * 0.75,
            None,
        ));
        let phi = jabs_f64(n_roof * 1.5);
        let top = 64.0 + jmin_f64((beta * beta) * 2.5, (phi * 50.0).ceil() + 24.0);
        let ys = floor_i32(top);
        if s0 > ys {
            return;
        }
        let max_y = min_y + height as i32 - 1;
        let scan_top = i32::min(ys, max_y);

        let mut y = scan_top;
        while y >= min_y {
            let j = (y - min_y) as usize;
            match out.categories[base + j] {
                Category::Stone => break,
                Category::Water => return,
                _ => {}
            }
            y -= 1;
        }

        let mut y = scan_top;
        while y >= min_y {
            let j = (y - min_y) as usize;
            let idx = base + j;
            if out.categories[idx] == Category::Air {
                out.categories[idx] = Category::Stone;
                out.blocks[idx] = self.stone_idx;
                if y > out.ws[col] {
                    out.ws[col] = y;
                }
            } else {
                break;
            }
            y -= 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn iceberg_ext(
        &self,
        x: i32,
        z: i32,
        s0: i32,
        m: i32,
        bs: u16,
        lx: usize,
        lz: usize,
        out: &mut ChunkTerrain,
    ) {
        let min_y = self.min_y;
        let height = self.height;
        let col = lx + CHUNK_WIDTH * lz;
        let base = col * height;

        let snow_idx = self.snow_block_idx;
        let packed_idx = self.packed_ice_idx;

        let i_surf = f64::from(self.noises.iceberg_surface.sample_point(
            f64::from(x),
            0.0,
            f64::from(z),
            None,
        ));
        let aa = jabs_f64(i_surf * 8.25);
        let ip = self.noises.iceberg_pillar.sample_point(
            f64::from(x) * 1.28,
            0.0,
            f64::from(z) * 1.28,
            None,
        );
        let bb = f64::from(ip * 15.0);
        let iota = jmin_f64(aa, bb);
        if iota <= 1.8 {
            return;
        }
        let roof = f64::from(self.noises.iceberg_pillar_roof.sample_point(
            f64::from(x) * 1.17,
            0.0,
            f64::from(z) * 1.17,
            None,
        ));
        let kappa = jabs_f64(roof * 1.5);
        let mut t = jmin_f64((iota * iota) * 1.2, (kappa * 40.0).ceil() + 14.0);

        let (t0, modi) = self.biome_temps[bs as usize];
        let temp = self.frozen.temperature(t0, modi, x, z);
        if temp > 0.1f32 {
            t -= 2.0;
        }
        if t <= 2.0 {
            return;
        }

        let bas = (63.0 - t) - 7.0;
        let haut = t + 63.0;
        let hb = bas as i32;
        let hh = haut as i32;

        let mut r = self.factory.at(x, 0, z);
        let ns = 2 + r.next_int_bounded(4);
        let hn = 63 + 18 + r.next_int_bounded(10);
        let mut k = 0i32;

        let mut y = i32::max(s0, hh + 1);
        while y >= m {
            if y < min_y || y > min_y + height as i32 - 1 {
                y -= 1;
                continue;
            }
            let j = (y - min_y) as usize;
            let idx = base + j;
            let cat = out.categories[idx];
            let mut convert = false;
            if cat == Category::Air && y < hh && r.next_double() > 0.01 {
                convert = true;
            }
            if !convert && cat == Category::Water && y > hb && y < 63 && r.next_double() > 0.15 {
                convert = true;
            }
            if convert {
                let bi = if k <= ns && y > hn {
                    k += 1;
                    snow_idx
                } else {
                    packed_idx
                };
                out.blocks[idx] = bi;
                out.categories[idx] = Category::Stone;
                if y > out.ws[col] {
                    out.ws[col] = y;
                }
            }
            y -= 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn biome_at(
        &self,
        grids: &[u16],
        cells: usize,
        cx: i32,
        cz: i32,
        qy_count: usize,
        x: i32,
        y: i32,
        z: i32,
    ) -> u16 {
        let seed = self.zoom_seed;
        let xp = x - 2;
        let yp = y - 2;
        let zp = z - 2;
        let px = xp >> 2;
        let py = yp >> 2;
        let pz = zp >> 2;
        let fx = f64::from(xp & 3) / 4.0;
        let fy = f64::from(yp & 3) / 4.0;
        let fz = f64::from(zp & 3) / 4.0;
        let mut best = 0usize;
        let mut best_s = f64::INFINITY;
        for i in 0..8usize {
            let (cxx, dx) = if i & 4 != 0 {
                (px + 1, fx - 1.0)
            } else {
                (px, fx)
            };
            let (cyy, dy) = if i & 2 != 0 {
                (py + 1, fy - 1.0)
            } else {
                (py, fy)
            };
            let (czz, dz) = if i & 1 != 0 {
                (pz + 1, fz - 1.0)
            } else {
                (pz, fz)
            };
            let mut r = seed;
            r = zoom_n(r, i64::from(cxx));
            r = zoom_n(r, i64::from(cyy));
            r = zoom_n(r, i64::from(czz));
            r = zoom_n(r, i64::from(cxx));
            r = zoom_n(r, i64::from(cyy));
            r = zoom_n(r, i64::from(czz));
            let phix = zoom_f(r);
            r = zoom_n(r, seed);
            let phiy = zoom_f(r);
            r = zoom_n(r, seed);
            let phiz = zoom_f(r);
            let score =
                ((dz + phiz) * (dz + phiz) + (dy + phiy) * (dy + phiy)) + (dx + phix) * (dx + phix);
            if score < best_s {
                best_s = score;
                best = i;
            }
        }
        let cxx = if best & 4 != 0 { px + 1 } else { px };
        let cyy = if best & 2 != 0 { py + 1 } else { py };
        let czz = if best & 1 != 0 { pz + 1 } else { pz };
        let qy = cyy.clamp(-16, (qy_count as i32) - 1 - 16);
        let qy_idx = (qy + 16) as usize;
        let ccx = cxx >> 2;
        let ccz = czz >> 2;
        let gdx = ccx - cx;
        let gdz = ccz - cz;
        let gx = (cxx & 3) as usize;
        let gz = (czz & 3) as usize;
        let goff = (((gdz + 1) * 3 + (gdx + 1)) as usize) * cells;
        grids[goff + qy_idx * 16 + gz * 4 + gx]
    }

    fn eval_rule(&self, rule: &CRule, ctx: &BlockCtx, bufs: &OreBufs) -> Option<u16> {
        match rule {
            CRule::Sequence(seq) => {
                for r in seq {
                    if let Some(b) = self.eval_rule(r, ctx, bufs) {
                        return Some(b);
                    }
                }
                None
            }
            CRule::Condition { cond, then } => {
                if self.eval_cond(cond, ctx) {
                    self.eval_rule(then, ctx, bufs)
                } else {
                    None
                }
            }
            CRule::Block(b) => Some(*b),
            CRule::Bandlands => {
                let n = self.noises.clay_bands_offset.sample_point(
                    f64::from(ctx.x),
                    0.0,
                    f64::from(ctx.z),
                    None,
                );
                let v = n * 4.0f32;
                let o = floor_i32(f64::from(v) + 0.5);
                let idx = ctx.y.wrapping_add(o).wrapping_add(192).rem_euclid(192) as usize;
                Some(self.bands[idx])
            }
            CRule::OreVein(ov) => self.eval_ore(ov, ctx, bufs),
        }
    }

    fn eval_ore(&self, ov: &OreVein, ctx: &BlockCtx, bufs: &OreBufs) -> Option<u16> {
        let idx = (ctx.y - self.min_y) as usize + (ctx.lx + CHUNK_WIDTH * ctx.lz) * self.height;
        let density = match ov.kind {
            OreKind::Copper => bufs.copper[idx],
            OreKind::Iron => bufs.iron[idx],
        };
        if density <= 0.0 {
            return None;
        }
        let mut r = self.ore_factory.at(ctx.x, ctx.y, ctx.z);
        let u1 = r.next_float();
        if u1 > density {
            return None;
        }
        let rich = bufs.rich[idx];
        let u2 = r.next_float();
        if u2 < rich {
            let gap = eval_point(&self.gap, ctx.x, ctx.y, ctx.z);
            if gap < 0.0 {
                let u3 = r.next_float();
                return Some(ore_raw_or_block(u3, ov.raw_ore_chance, ov.raw_ore, ov.ore));
            }
        }
        Some(ov.filler)
    }

    fn eval_cond(&self, cond: &CCond, ctx: &BlockCtx) -> bool {
        match cond {
            CCond::Biome(mask) => (*mask >> ctx.biome) & 1 != 0,
            CCond::Not(inner) => !self.eval_cond(inner, ctx),
            CCond::NoiseThreshold {
                stack,
                min,
                max,
                is_3d,
            } => {
                let yy = if *is_3d { ctx.y } else { 0 };
                let v = f64::from(stack.sample_point(
                    f64::from(ctx.x),
                    f64::from(yy),
                    f64::from(ctx.z),
                    None,
                ));
                if v.is_nan() {
                    false
                } else {
                    (*min..=*max).contains(&v)
                }
            }
            CCond::VerticalGradient { factory, a, c } => {
                if ctx.y <= *a {
                    true
                } else if ctx.y >= *c {
                    false
                } else {
                    let p = vertical_gradient_probability(ctx.y, *a, *c);
                    let u = factory.at(ctx.x, ctx.y, ctx.z).next_float();
                    f64::from(u) < p
                }
            }
            CCond::YAbove {
                anchor,
                mult,
                add_stone_depth,
            } => {
                let add = if *add_stone_depth { ctx.a } else { 0 };
                ctx.y.wrapping_add(add) >= anchor.wrapping_add(ctx.d.wrapping_mul(*mult))
            }
            CCond::Water {
                offset,
                mult,
                add_stone_depth,
            } => condition_water(ctx.w, ctx.y, ctx.a, *offset, *mult, ctx.d, *add_stone_depth),
            CCond::StoneDepth {
                offset,
                add_surface_depth,
                secondary,
                ceiling,
            } => condition_stone_depth(
                ctx.a,
                ctx.bn,
                ctx.d,
                ctx.s2,
                *offset,
                *add_surface_depth,
                *secondary,
                *ceiling,
            ),
            CCond::Steep => condition_steep(ctx.gx, ctx.gz),
            CCond::Hole => ctx.d <= 0,
            CCond::AbovePreliminarySurface => ctx.y >= ctx.m,
            CCond::Temperature => {
                let (t0, modi) = self.biome_temps[ctx.biome as usize];
                let mut t = self.frozen.temperature(t0, modi, ctx.x, ctx.z);
                if ctx.y > 63 + 17 {
                    let gx = (ctx.x as f32) / 8.0f32;
                    let gz = (ctx.z as f32) / 8.0f32;
                    let vv = self.temp_simplex.value(f64::from(gx), f64::from(gz)) * 8.0f32;
                    t -= ((vv + ctx.y as f32) - 80.0) * 0.05f32 / 40.0f32;
                }
                t < 0.15f32
            }
        }
    }
}

pub(crate) fn surface_depth_offset(sn: f32, r: f64) -> i32 {
    ((f64::from(sn) * 2.75 + 3.0) + r * 0.25) as i32
}

pub(crate) fn vertical_gradient_probability(y: i32, a: i32, c: i32) -> f64 {
    let t = f64::from(y - a) / f64::from(c - a);
    1.0 - t
}

pub(crate) fn condition_steep(gx: i32, gz: i32) -> bool {
    gx <= -4 || gz >= 4
}

pub(crate) fn condition_water(
    w: Option<i32>,
    y: i32,
    a: i32,
    offset: i32,
    mult: i32,
    d: i32,
    add_stone_depth: bool,
) -> bool {
    match w {
        None => true,
        Some(w) => {
            let add = if add_stone_depth { a } else { 0 };
            y.wrapping_add(add) >= w.wrapping_add(offset).wrapping_add(d.wrapping_mul(mult))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn condition_stone_depth(
    a: i32,
    bn: i32,
    d: i32,
    s2: f64,
    offset: i32,
    add_surface_depth: bool,
    secondary: i32,
    ceiling: bool,
) -> bool {
    let big_d = if ceiling { bn } else { a };
    let e = if add_surface_depth { d } else { 0 };
    let sigma = if secondary == 0 {
        0
    } else {
        let rr = f64::from(secondary);
        (((s2 + 1.0) / 2.0) * rr) as i32
    };
    big_d
        <= 1i32
            .wrapping_add(offset)
            .wrapping_add(e)
            .wrapping_add(sigma)
}

pub(crate) fn ore_raw_or_block(u3: f32, raw_ore_chance: f32, raw_ore: u16, ore: u16) -> u16 {
    if u3 < raw_ore_chance { raw_ore } else { ore }
}

#[cfg(test)]
mod tests {
    use super::{
        condition_steep, condition_stone_depth, condition_water, ore_raw_or_block,
        surface_depth_offset, vertical_gradient_probability,
    };

    #[test]
    fn vertical_gradient_bedrock_probabilities() {
        let (a, c) = (-64, -59);
        assert_eq!(
            vertical_gradient_probability(-63, a, c).to_bits(),
            (1.0_f64 - 1.0_f64 / 5.0_f64).to_bits()
        );
        assert_eq!(
            vertical_gradient_probability(-62, a, c).to_bits(),
            (1.0_f64 - 2.0_f64 / 5.0_f64).to_bits()
        );
        assert_eq!(
            vertical_gradient_probability(-61, a, c).to_bits(),
            (1.0_f64 - 3.0_f64 / 5.0_f64).to_bits()
        );
        assert_eq!(
            vertical_gradient_probability(-60, a, c).to_bits(),
            (1.0_f64 - 4.0_f64 / 5.0_f64).to_bits()
        );
    }

    #[test]
    fn vertical_gradient_deepslate_probabilities() {
        let (a, c) = (0, 8);
        assert_eq!(
            vertical_gradient_probability(1, a, c).to_bits(),
            0.875_f64.to_bits()
        );
        assert_eq!(
            vertical_gradient_probability(2, a, c).to_bits(),
            0.75_f64.to_bits()
        );
        assert_eq!(
            vertical_gradient_probability(4, a, c).to_bits(),
            0.5_f64.to_bits()
        );
        assert_eq!(
            vertical_gradient_probability(7, a, c).to_bits(),
            0.125_f64.to_bits()
        );
    }

    #[test]
    fn vertical_gradient_endpoints_are_zero_and_one() {
        assert_eq!(
            vertical_gradient_probability(-64, -64, -59).to_bits(),
            1.0_f64.to_bits()
        );
        assert_eq!(
            vertical_gradient_probability(-59, -64, -59).to_bits(),
            0.0_f64.to_bits()
        );
    }

    #[test]
    fn steep_threshold_boundaries() {
        assert!(condition_steep(-4, 0));
        assert!(!condition_steep(-3, 0));
        assert!(condition_steep(0, 4));
        assert!(!condition_steep(0, 3));
        assert!(!condition_steep(-3, 3));
        assert!(condition_steep(-5, 5));
    }

    #[test]
    fn surface_depth_truncates_toward_zero() {
        assert_eq!(surface_depth_offset(0.0, 0.0), 3);
        assert_eq!(surface_depth_offset(-2.0, 0.0), -2);
        assert_eq!(surface_depth_offset(0.0, 4.0), 4);
        assert_eq!(surface_depth_offset(0.0, 0.0).max(0), 3);
        assert_eq!(surface_depth_offset(-2.0, 0.0).max(0), 0);
    }

    #[test]
    fn stone_depth_on_floor_within_one() {
        assert!(condition_stone_depth(1, 0, 0, 0.0, 0, false, 0, false));
        assert!(!condition_stone_depth(2, 0, 0, 0.0, 0, false, 0, false));
        assert!(condition_stone_depth(0, 1, 0, 0.0, 0, false, 0, true));
        assert!(!condition_stone_depth(0, 2, 0, 0.0, 0, false, 0, true));
    }

    #[test]
    fn water_condition_vectors() {
        assert!(condition_water(None, 70, 0, 0, 0, 0, false));
        assert!(!condition_water(Some(63), 60, 0, 0, 0, 0, false));
        assert!(condition_water(Some(63), 63, 0, 0, 0, 0, false));
    }

    #[test]
    fn raw_ore_chance_copper_is_f32_rounded() {
        let stored = 0.02_f64 as f32;
        assert_eq!(stored.to_bits(), 0.02_f32.to_bits());
        assert_ne!(f64::from(stored).to_bits(), 0.02_f64.to_bits());
        assert_eq!(ore_raw_or_block(0.01, 0.02, 7, 9), 7);
        assert_eq!(ore_raw_or_block(0.02, 0.02, 7, 9), 9);
        assert_eq!(ore_raw_or_block(0.03, 0.02, 7, 9), 9);
    }
}
