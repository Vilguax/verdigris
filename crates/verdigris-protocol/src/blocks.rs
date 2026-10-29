use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const EXPECTED_STATE_COUNT: i32 = 35723;

#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    pub name: String,
    pub id: i32,
    pub first_state: i32,
    pub state_count: i32,
    pub default_state: i32,
    pub air: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ItemState {
    pub item: i32,
    pub state: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlocksData {
    pub game_version: String,
    pub server_jar_sha1: String,
    pub state_count: i32,
    pub blocks: Vec<Block>,
    pub fluid_states: Vec<i32>,
    pub items: Vec<ItemState>,
}

pub struct Blocks {
    data: BlocksData,
    by_name: HashMap<String, usize>,
    items: HashMap<i32, i32>,
}

static BLOCKS: OnceLock<Blocks> = OnceLock::new();

fn blocks() -> &'static Blocks {
    BLOCKS.get_or_init(|| {
        let data: BlocksData = serde_json::from_str(include_str!("../data/26.3/blocks.json"))
            .expect("embedded 26.3/blocks.json is valid BlocksData");

        debug_assert_eq!(data.state_count, EXPECTED_STATE_COUNT);
        debug_assert!(
            data.blocks
                .windows(2)
                .all(|w| w[0].first_state < w[1].first_state)
        );
        debug_assert!(data.fluid_states.windows(2).all(|w| w[0] < w[1]));

        let by_name = data
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.name.clone(), index))
            .collect();
        let items = data
            .items
            .iter()
            .map(|entry| (entry.item, entry.state))
            .collect();

        Blocks {
            data,
            by_name,
            items,
        }
    })
}

pub fn blocks_data() -> &'static BlocksData {
    &blocks().data
}

pub fn state_to_block(state: i32) -> &'static Block {
    let blocks = &blocks().data.blocks;
    let position = blocks.partition_point(|block| block.first_state <= state);
    &blocks[position - 1]
}

pub fn default_state_of_name(name: &str) -> Option<i32> {
    let handle = blocks();
    handle
        .by_name
        .get(name)
        .map(|&index| handle.data.blocks[index].default_state)
}

pub fn is_air(state: i32) -> bool {
    if state == 0 {
        return true;
    }
    state_to_block(state).air
}

pub fn is_fluid(state: i32) -> bool {
    blocks().data.fluid_states.binary_search(&state).is_ok()
}

pub fn item_to_placed_state(item: i32) -> Option<i32> {
    blocks().items.get(&item).copied()
}

pub fn block_item_of_state(state: i32) -> Option<i32> {
    let block = state_to_block(state);
    let lo = block.first_state;
    let hi = block.first_state + block.state_count;
    blocks()
        .data
        .items
        .iter()
        .find(|entry| entry.state >= lo && entry.state < hi)
        .map(|entry| entry.item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_state_count_matches_target() {
        assert_eq!(blocks_data().state_count, EXPECTED_STATE_COUNT);
    }

    #[test]
    fn blocks_sorted_by_first_state() {
        let blocks = &blocks_data().blocks;
        assert!(
            blocks
                .windows(2)
                .all(|w| w[0].first_state < w[1].first_state)
        );
    }

    #[test]
    fn loads_once() {
        let first = blocks_data() as *const BlocksData;
        let second = blocks_data() as *const BlocksData;
        assert_eq!(first, second);
    }

    #[test]
    fn default_states_of_known_names() {
        assert_eq!(default_state_of_name("minecraft:air"), Some(0));
        assert_eq!(default_state_of_name("minecraft:stone"), Some(1));
        assert_eq!(default_state_of_name("minecraft:grass_block"), Some(9));
        assert_eq!(default_state_of_name("minecraft:dirt"), Some(10));
        assert_eq!(default_state_of_name("minecraft:bedrock"), Some(88));
        assert_eq!(default_state_of_name("minecraft:water"), Some(89));
        assert_eq!(default_state_of_name("minecraft:not_a_block"), None);
    }

    #[test]
    fn states_map_to_owning_block() {
        assert_eq!(state_to_block(0).name, "minecraft:air");
        assert_eq!(state_to_block(1).name, "minecraft:stone");
        assert_eq!(state_to_block(9).name, "minecraft:grass_block");
        assert_eq!(state_to_block(10).name, "minecraft:dirt");
        assert_eq!(state_to_block(88).name, "minecraft:bedrock");
        assert_eq!(state_to_block(89).name, "minecraft:water");
    }

    #[test]
    fn air_detection() {
        assert!(is_air(0));
        assert!(!is_air(1));
        assert!(!is_air(89));
    }

    #[test]
    fn fluid_detection() {
        assert!(is_fluid(89));
        assert!(!is_fluid(1));
    }

    #[test]
    fn items_map_to_placed_states() {
        assert_eq!(item_to_placed_state(1), Some(1));
        assert_eq!(item_to_placed_state(55), Some(10));
    }

    #[test]
    fn block_items_resolve_from_state() {
        assert_eq!(block_item_of_state(9), Some(54));
        assert_eq!(block_item_of_state(10), Some(55));
        assert_eq!(block_item_of_state(88), Some(87));
    }

    #[test]
    fn blocks_without_item_resolve_to_none() {
        assert_eq!(block_item_of_state(0), None);
        assert_eq!(block_item_of_state(89), None);
    }
}
