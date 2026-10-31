use std::sync::OnceLock;

use crate::biome::boxes::{ParamBox, dist};
use crate::biome::data::{DIMENSIONS, biome_entries};
use crate::biome::tree::build_tree;

pub struct BiomeTree {
    boxes: Vec<ParamBox>,
    rank: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SearchState {
    pub last: Option<usize>,
}

impl SearchState {
    pub fn new() -> Self {
        SearchState { last: None }
    }

    pub fn reset(&mut self) {
        self.last = None;
    }
}

impl BiomeTree {
    pub fn build() -> Self {
        let boxes: Vec<ParamBox> = biome_entries().iter().map(ParamBox::from_raw).collect();
        let built = build_tree(&boxes);
        BiomeTree {
            boxes,
            rank: built.rank,
        }
    }

    pub fn embedded() -> &'static BiomeTree {
        static TREE: OnceLock<BiomeTree> = OnceLock::new();
        TREE.get_or_init(BiomeTree::build)
    }

    pub fn from_parts(boxes: Vec<ParamBox>, rank: Vec<u16>) -> Self {
        assert_eq!(boxes.len(), rank.len(), "boxes and rank must align");
        BiomeTree { boxes, rank }
    }

    pub fn boxes(&self) -> &[ParamBox] {
        &self.boxes
    }

    pub fn rank(&self) -> &[u16] {
        &self.rank
    }

    pub fn min_info(&self, target: &[i64; DIMENSIONS]) -> (i64, Vec<u16>) {
        let mut min_dist = i64::MAX;
        for bx in &self.boxes {
            let d = dist(bx, target);
            if d < min_dist {
                min_dist = d;
            }
        }
        let mut biomes = Vec::new();
        for bx in &self.boxes {
            if dist(bx, target) == min_dist && !biomes.contains(&bx.biome) {
                biomes.push(bx.biome);
            }
        }
        (min_dist, biomes)
    }

    pub fn search(&self, state: &mut SearchState, target: &[i64; DIMENSIONS]) -> u16 {
        let mut min_dist = i64::MAX;
        let mut best = usize::MAX;
        let mut best_rank = u16::MAX;
        for (entry, bx) in self.boxes.iter().enumerate() {
            let d = dist(bx, target);
            if d < min_dist {
                min_dist = d;
                best = entry;
                best_rank = self.rank[entry];
            } else if d == min_dist && self.rank[entry] < best_rank {
                best = entry;
                best_rank = self.rank[entry];
            }
        }
        let chosen = match state.last {
            Some(last) if dist(&self.boxes[last], target) == min_dist => last,
            _ => best,
        };
        state.last = Some(chosen);
        self.boxes[chosen].biome
    }
}

#[cfg(test)]
mod tests {
    use super::{BiomeTree, SearchState};

    #[test]
    fn search_prefers_smaller_rank_on_ties() {
        let tree = BiomeTree::embedded();
        let mut state = SearchState::new();
        let far = [1_000_000i64; 7];
        let biome = tree.search(&mut state, &far);
        assert!(biome < 67);
        assert!(state.last.is_some());
    }

    #[test]
    fn sticky_last_result_is_kept_on_equal_distance() {
        let tree = BiomeTree::embedded();
        let mut state = SearchState::new();
        let entry0 = &tree.boxes()[0];
        let mut target = [0i64; 7];
        for (d, slot) in target.iter_mut().enumerate() {
            *slot = entry0.center(d);
        }
        let first = tree.search(&mut state, &target);
        let last_after = state.last;
        let second = tree.search(&mut state, &target);
        assert_eq!(first, second);
        assert_eq!(last_after, state.last);
    }
}
