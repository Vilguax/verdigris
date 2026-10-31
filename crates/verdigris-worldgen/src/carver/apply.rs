use crate::aquifer::{AquiferFunctions, AquiferScratch, Filled};
use crate::carver::mask::{CARVE_MAX_Y, CARVE_MIN_Y, CarveMask};
use crate::chunk_fill::{CHUNK_WIDTH, Category};

pub struct CarveBlocks {
    pub air: u16,
    pub water: u16,
    pub lava: u16,
    pub dirt: Option<u16>,
    pub grass: Option<u16>,
    pub mycelium: Option<u16>,
}

pub struct CarveApplyCtx<'a> {
    pub mask: &'a CarveMask,
    pub funcs: &'a AquiferFunctions,
    pub uncarvable: &'a [bool],
    pub blocks: CarveBlocks,
    pub cx: i32,
    pub cz: i32,
    pub min_y: i32,
    pub height: usize,
}

pub struct CarveTarget<'a> {
    pub categories: &'a mut [Category],
    pub block_ids: &'a mut [u16],
    pub post: &'a mut [u64],
    pub surface: &'a mut [i32],
}

fn is_surface_trigger(block: u16, ctx: &CarveApplyCtx<'_>) -> Option<u16> {
    if ctx.blocks.grass == Some(block) || ctx.blocks.mycelium == Some(block) {
        Some(block)
    } else {
        None
    }
}

pub fn apply_carvers(
    aquifer: &mut AquiferScratch,
    t: &mut CarveTarget<'_>,
    ctx: &CarveApplyCtx<'_>,
) {
    let min_y = ctx.min_y;
    let height = ctx.height;
    let x0 = CHUNK_WIDTH as i32 * ctx.cx;
    let z0 = CHUNK_WIDTH as i32 * ctx.cz;
    let y_top = i32::min(CARVE_MAX_Y, min_y + height as i32 - 1);
    let y_bot = i32::max(CARVE_MIN_Y, min_y);

    for lx in 0..CHUNK_WIDTH {
        for lz in 0..CHUNK_WIDTH {
            let x = x0 + lx as i32;
            let z = z0 + lz as i32;
            let col = lx + CHUNK_WIDTH * lz;
            let base = col * height;
            let mut touched = false;

            let mut y = y_bot;
            while y <= y_top {
                if !ctx.mask.get(lx, y, lz) {
                    y += 1;
                    continue;
                }
                let s_lo = y;
                let mut s_hi = y;
                while s_hi < y_top && ctx.mask.get(lx, s_hi + 1, lz) {
                    s_hi += 1;
                }

                let mut reached_surface = false;
                let mut surface_block: u16 = ctx.blocks.grass.unwrap_or(ctx.blocks.air);
                let mut yy = s_hi;
                while yy >= s_lo {
                    let j = (yy - min_y) as usize;
                    let idx = base + j;
                    let cur = t.block_ids[idx];
                    if ctx.uncarvable.get(cur as usize).copied().unwrap_or(false) {
                        yy -= 1;
                        continue;
                    }
                    if let Some(b) = is_surface_trigger(cur, ctx) {
                        reached_surface = true;
                        surface_block = b;
                    }
                    let (filled, phi) = aquifer.decide(ctx.funcs, x, yy, z, 0.0);
                    let placed = match filled {
                        Filled::Nothing => None,
                        Filled::Air => Some((ctx.blocks.air, Category::Air)),
                        Filled::Water => Some((ctx.blocks.water, Category::Water)),
                        Filled::Lava => Some((ctx.blocks.lava, Category::Lava)),
                    };
                    if let Some((bi, cat)) = placed {
                        t.block_ids[idx] = bi;
                        t.categories[idx] = cat;
                        touched = true;
                        if phi && matches!(cat, Category::Water | Category::Lava) {
                            t.post[idx >> 6] |= 1u64 << (idx & 63);
                        }
                        if reached_surface
                            && cat == Category::Air
                            && yy > min_y
                            && Some(t.block_ids[idx - 1]) == ctx.blocks.dirt
                        {
                            t.block_ids[idx - 1] = surface_block;
                            t.categories[idx - 1] = Category::Stone;
                        }
                    }
                    yy -= 1;
                }

                y = s_hi + 1;
            }

            if touched {
                let mut top = min_y - 1;
                for j in (0..height).rev() {
                    if t.categories[base + j] != Category::Air {
                        top = min_y + j as i32;
                        break;
                    }
                }
                t.surface[col] = top;
            }
        }
    }
}
