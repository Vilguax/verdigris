use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const EXPECTED_STATE_COUNT: i32 = 35723;

#[derive(Debug, Clone, Deserialize)]
pub struct Property {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    pub name: String,
    pub id: i32,
    pub first_state: i32,
    pub state_count: i32,
    pub default_state: i32,
    pub air: bool,
    pub properties: Vec<Property>,
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
    #[serde(default)]
    pub item_ids: HashMap<String, i32>,
}

pub struct Blocks {
    data: BlocksData,
    by_name: HashMap<String, usize>,
    by_id: HashMap<i32, usize>,
    items: HashMap<i32, i32>,
    steps: Vec<Vec<i32>>,
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
        debug_assert!(data.blocks.iter().all(|block| {
            let product: i32 = block
                .properties
                .iter()
                .map(|property| property.values.len() as i32)
                .product();
            product == block.state_count
        }));

        let by_name = data
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.name.clone(), index))
            .collect();
        let by_id = data
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.id, index))
            .collect();
        let items = data
            .items
            .iter()
            .map(|entry| (entry.item, entry.state))
            .collect();
        let steps = data.blocks.iter().map(mixed_radix_steps).collect();

        Blocks {
            data,
            by_name,
            by_id,
            items,
            steps,
        }
    })
}

pub fn blocks_data() -> &'static BlocksData {
    &blocks().data
}

fn mixed_radix_steps(block: &Block) -> Vec<i32> {
    let cardinalities: Vec<i32> = block
        .properties
        .iter()
        .map(|property| property.values.len() as i32)
        .collect();
    let count = cardinalities.len();
    let mut steps = vec![1i32; count];
    for index in (0..count.saturating_sub(1)).rev() {
        steps[index] = steps[index + 1] * cardinalities[index + 1];
    }
    steps
}

fn locate(state: i32) -> Option<(usize, i32)> {
    if state < 0 {
        return None;
    }
    let blocks = &blocks().data.blocks;
    let position = blocks.partition_point(|block| block.first_state <= state);
    if position == 0 {
        return None;
    }
    let index = position - 1;
    let offset = state - blocks[index].first_state;
    if offset >= blocks[index].state_count {
        return None;
    }
    Some((index, offset))
}

pub fn state_to_block(state: i32) -> &'static Block {
    let blocks = &blocks().data.blocks;
    let position = blocks.partition_point(|block| block.first_state <= state);
    &blocks[position - 1]
}

pub fn property_of(state: i32, name: &str) -> Option<&'static str> {
    let (index, offset) = locate(state)?;
    let handle = blocks();
    let block = &handle.data.blocks[index];
    let steps = &handle.steps[index];
    for (position, property) in block.properties.iter().enumerate() {
        if property.name == name {
            let cardinality = property.values.len() as i32;
            let value_index = ((offset / steps[position]) % cardinality) as usize;
            return Some(property.values[value_index].as_str());
        }
    }
    None
}

pub fn with_property(state: i32, name: &str, value: &str) -> Option<i32> {
    let (index, offset) = locate(state)?;
    let handle = blocks();
    let block = &handle.data.blocks[index];
    let steps = &handle.steps[index];
    for (position, property) in block.properties.iter().enumerate() {
        if property.name == name {
            let cardinality = property.values.len() as i32;
            let target = property.values.iter().position(|v| v.as_str() == value)? as i32;
            let current = (offset / steps[position]) % cardinality;
            return Some(state - current * steps[position] + target * steps[position]);
        }
    }
    None
}

pub fn state_with(block_id: i32, properties: &[(&str, &str)]) -> Option<i32> {
    let handle = blocks();
    let &index = handle.by_id.get(&block_id)?;
    let mut state = handle.data.blocks[index].default_state;
    for &(name, value) in properties {
        state = with_property(state, name, value)?;
    }
    Some(state)
}

pub fn block_has_property(block_id: i32, name: &str) -> bool {
    let handle = blocks();
    match handle.by_id.get(&block_id) {
        Some(&index) => handle.data.blocks[index]
            .properties
            .iter()
            .any(|property| property.name == name),
        None => false,
    }
}

pub fn default_state_of_name(name: &str) -> Option<i32> {
    let handle = blocks();
    handle
        .by_name
        .get(name)
        .map(|&index| handle.data.blocks[index].default_state)
}

pub fn block_id_of_name(name: &str) -> Option<i32> {
    let handle = blocks();
    handle
        .by_name
        .get(name)
        .map(|&index| handle.data.blocks[index].id)
}

pub fn item_id_of_name(name: &str) -> Option<i32> {
    blocks().data.item_ids.get(name).copied()
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
    fn block_ids_resolve_from_name() {
        assert_eq!(block_id_of_name("minecraft:air"), Some(0));
        assert_eq!(block_id_of_name("minecraft:stone"), Some(1));
        assert_eq!(block_id_of_name("minecraft:oak_door"), Some(265));
        assert_eq!(block_id_of_name("minecraft:not_a_block"), None);
    }

    #[test]
    fn item_ids_resolve_from_name() {
        assert_eq!(item_id_of_name("minecraft:ender_eye"), Some(1278));
        assert_eq!(item_id_of_name("minecraft:not_an_item"), None);
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

    fn block_by_name(name: &str) -> &'static Block {
        let handle = blocks();
        &handle.data.blocks[handle.by_name[name]]
    }

    #[test]
    fn chest_state_decodes_properties() {
        assert_eq!(property_of(5545, "facing"), Some("north"));
        assert_eq!(property_of(5545, "type"), Some("left"));
        assert_eq!(property_of(5545, "waterlogged"), Some("true"));
    }

    #[test]
    fn oak_stairs_facing_roundtrips() {
        let stairs = block_by_name("minecraft:oak_stairs");
        let default = stairs.default_state;
        assert_eq!(property_of(default, "facing"), Some("north"));
        let east = with_property(default, "facing", "east").unwrap();
        assert_eq!(property_of(east, "facing"), Some("east"));
        assert_eq!(property_of(east, "half"), property_of(default, "half"));
        assert_eq!(property_of(east, "shape"), property_of(default, "shape"));
        assert_eq!(
            property_of(east, "waterlogged"),
            property_of(default, "waterlogged")
        );
    }

    #[test]
    fn oak_log_axes_are_reconstructible() {
        let log = block_by_name("minecraft:oak_log");
        for axis in ["x", "y", "z"] {
            let state = state_with(log.id, &[("axis", axis)]).unwrap();
            assert_eq!(property_of(state, "axis"), Some(axis));
        }
    }

    #[test]
    fn observer_covers_all_facings() {
        let observer = block_by_name("minecraft:observer");
        for facing in ["north", "east", "south", "west", "up", "down"] {
            let state = state_with(observer.id, &[("facing", facing)]).unwrap();
            assert_eq!(property_of(state, "facing"), Some(facing));
        }
    }

    #[test]
    fn unknown_name_or_value_yields_none() {
        assert_eq!(property_of(5545, "nonexistent"), None);
        assert_eq!(with_property(5545, "nonexistent", "north"), None);
        assert_eq!(with_property(5545, "facing", "diagonal"), None);
        let chest = block_by_name("minecraft:chest");
        assert_eq!(state_with(chest.id, &[("facing", "diagonal")]), None);
        assert_eq!(state_with(chest.id, &[("nonexistent", "north")]), None);
        assert_eq!(state_with(-1, &[]), None);
    }

    #[test]
    fn out_of_range_states_yield_none() {
        assert_eq!(property_of(-1, "facing"), None);
        assert_eq!(with_property(-1, "facing", "north"), None);
        let last = blocks_data().state_count;
        assert_eq!(property_of(last, "facing"), None);
        assert_eq!(with_property(last, "facing", "north"), None);
        assert_eq!(property_of(last + 1000, "facing"), None);
    }

    #[test]
    fn block_has_property_reflects_registry() {
        let chest = block_by_name("minecraft:chest");
        assert!(block_has_property(chest.id, "facing"));
        assert!(block_has_property(chest.id, "waterlogged"));
        assert!(!block_has_property(chest.id, "axis"));
        let stone = block_by_name("minecraft:stone");
        assert!(!block_has_property(stone.id, "facing"));
        assert!(!block_has_property(i32::MAX, "facing"));
    }

    #[test]
    fn every_state_roundtrips_from_default() {
        let handle = blocks();
        for block in &handle.data.blocks {
            for state in block.first_state..block.first_state + block.state_count {
                let mut rebuilt = block.default_state;
                for property in &block.properties {
                    let value = property_of(state, &property.name).unwrap();
                    rebuilt = with_property(rebuilt, &property.name, value).unwrap();
                }
                assert_eq!(rebuilt, state);
            }
        }
    }

    #[test]
    fn cardinality_product_matches_state_count() {
        for block in &blocks_data().blocks {
            let product: i32 = block
                .properties
                .iter()
                .map(|property| property.values.len() as i32)
                .product();
            assert_eq!(product, block.state_count, "block {}", block.name);
        }
    }
}
