use crate::biome::quantize::q;
use crate::biome::search::{BiomeTree, SearchState};
use crate::df::eval::eval_point;
use crate::df::node::Node;

pub const QUART: i32 = 4;

pub struct BiomeSource {
    temperature: Node,
    vegetation: Node,
    continents: Node,
    erosion: Node,
    depth: Node,
    ridges: Node,
    tree: &'static BiomeTree,
    min_y: i32,
    height: usize,
}

impl BiomeSource {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        temperature: Node,
        vegetation: Node,
        continents: Node,
        erosion: Node,
        depth: Node,
        ridges: Node,
        min_y: i32,
        height: usize,
    ) -> Self {
        BiomeSource {
            temperature,
            vegetation,
            continents,
            erosion,
            depth,
            ridges,
            tree: BiomeTree::embedded(),
            min_y,
            height,
        }
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn quart_y_count(&self) -> usize {
        self.height / QUART as usize
    }

    pub fn cell_count(&self) -> usize {
        self.quart_y_count() * 16
    }

    fn target(&self, x: i32, y: i32, z: i32) -> [i64; 7] {
        [
            q(eval_point(&self.temperature, x, y, z)),
            q(eval_point(&self.vegetation, x, y, z)),
            q(eval_point(&self.continents, x, y, z)),
            q(eval_point(&self.erosion, x, y, z)),
            q(eval_point(&self.depth, x, y, z)),
            q(eval_point(&self.ridges, x, y, z)),
            0,
        ]
    }

    pub fn targets(&self, cx: i32, cz: i32) -> Vec<[i64; 7]> {
        let x0 = 16i32.wrapping_mul(cx);
        let z0 = 16i32.wrapping_mul(cz);
        let qy_count = self.quart_y_count();
        let mut out = vec![[0i64; 7]; qy_count * 16];
        for qy in 0..qy_count {
            for qz in 0..4usize {
                for qx in 0..4usize {
                    let bx = x0 + QUART * qx as i32;
                    let by = self.min_y + QUART * qy as i32;
                    let bz = z0 + QUART * qz as i32;
                    out[qy * 16 + qz * 4 + qx] = self.target(bx, by, bz);
                }
            }
        }
        out
    }

    pub fn fill_chunk(&self, cx: i32, cz: i32, state: &mut SearchState, out: &mut [u16]) {
        state.reset();
        let x0 = 16i32.wrapping_mul(cx);
        let z0 = 16i32.wrapping_mul(cz);
        let qy_count = self.quart_y_count();
        let sections = qy_count.div_ceil(4);
        for section in 0..sections {
            for qx in 0..4usize {
                for yl in 0..4usize {
                    let qy = section * 4 + yl;
                    if qy >= qy_count {
                        continue;
                    }
                    for qz in 0..4usize {
                        let bx = x0 + QUART * qx as i32;
                        let by = self.min_y + QUART * qy as i32;
                        let bz = z0 + QUART * qz as i32;
                        let target = self.target(bx, by, bz);
                        let biome = self.tree.search(state, &target);
                        out[qy * 16 + qz * 4 + qx] = biome;
                    }
                }
            }
        }
    }
}
