use std::sync::OnceLock;

use crate::blocks::{
    block_has_property, blocks_data, default_state_of_name, item_id_of_name, state_to_block,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketIds {
    pub empty: i32,
    pub water: i32,
    pub lava: i32,
}

struct Tables {
    bucket_ids: BucketIds,
    lava_source: i32,
    source_fluid: Box<[bool]>,
    liquid_block: Box<[bool]>,
    waterloggable: Box<[bool]>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
}

fn build_tables() -> Tables {
    let data = blocks_data();
    let state_len = data.state_count as usize;
    let id_len = data.blocks.iter().map(|block| block.id).max().unwrap_or(0) as usize + 1;

    let water_source = default_state_of_name("minecraft:water").unwrap_or(-1);
    let lava_source = default_state_of_name("minecraft:lava").unwrap_or(-1);

    let mut source_fluid = vec![false; state_len].into_boxed_slice();
    for state in [water_source, lava_source] {
        if let Ok(index) = usize::try_from(state)
            && index < source_fluid.len()
        {
            source_fluid[index] = true;
        }
    }

    let mut liquid_block = vec![false; state_len].into_boxed_slice();
    for block in &data.blocks {
        if block.name == "minecraft:water" || block.name == "minecraft:lava" {
            for state in block.first_state..block.first_state + block.state_count {
                if let Ok(index) = usize::try_from(state)
                    && index < liquid_block.len()
                {
                    liquid_block[index] = true;
                }
            }
        }
    }

    let mut waterloggable = vec![false; id_len].into_boxed_slice();
    for block in &data.blocks {
        if block_has_property(block.id, "waterlogged")
            && let Ok(index) = usize::try_from(block.id)
            && index < waterloggable.len()
        {
            waterloggable[index] = true;
        }
    }

    let bucket_ids = BucketIds {
        empty: item_id_of_name("minecraft:bucket").unwrap_or(-1),
        water: item_id_of_name("minecraft:water_bucket").unwrap_or(-1),
        lava: item_id_of_name("minecraft:lava_bucket").unwrap_or(-1),
    };

    Tables {
        bucket_ids,
        lava_source,
        source_fluid,
        liquid_block,
        waterloggable,
    }
}

pub fn bucket_ids() -> BucketIds {
    tables().bucket_ids
}

#[inline]
pub fn lava_source_state() -> i32 {
    tables().lava_source
}

#[inline]
pub fn is_source_fluid(state: i32) -> bool {
    usize::try_from(state)
        .ok()
        .and_then(|index| tables().source_fluid.get(index).copied())
        .unwrap_or(false)
}

#[inline]
pub fn is_liquid_block(state: i32) -> bool {
    usize::try_from(state)
        .ok()
        .and_then(|index| tables().liquid_block.get(index).copied())
        .unwrap_or(false)
}

#[inline]
pub fn is_waterloggable(state: i32) -> bool {
    if state < 0 {
        return false;
    }
    let block_id = state_to_block(state).id;
    usize::try_from(block_id)
        .ok()
        .and_then(|index| tables().waterloggable.get(index).copied())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_items_resolve_from_registry() {
        let ids = bucket_ids();
        assert_eq!(ids.empty, item_id_of_name("minecraft:bucket").unwrap());
        assert_eq!(
            ids.water,
            item_id_of_name("minecraft:water_bucket").unwrap()
        );
        assert_eq!(ids.lava, item_id_of_name("minecraft:lava_bucket").unwrap());
    }

    #[test]
    fn only_default_water_and_lava_are_sources() {
        let water = default_state_of_name("minecraft:water").unwrap();
        let lava = default_state_of_name("minecraft:lava").unwrap();
        assert!(is_source_fluid(water));
        assert!(is_source_fluid(lava));
        assert_eq!(lava_source_state(), lava);
        assert!(
            !is_source_fluid(water + 1),
            "l'eau coulante n'est pas source"
        );
        assert!(!is_source_fluid(
            default_state_of_name("minecraft:stone").unwrap()
        ));
        assert!(!is_source_fluid(0));
        assert!(!is_source_fluid(-1));
    }

    #[test]
    fn liquid_block_excludes_waterlogged_solids() {
        assert!(is_liquid_block(
            default_state_of_name("minecraft:water").unwrap()
        ));
        assert!(is_liquid_block(
            default_state_of_name("minecraft:lava").unwrap()
        ));
        let flowing = crate::blocks::with_property(
            default_state_of_name("minecraft:water").unwrap(),
            "level",
            "1",
        )
        .unwrap();
        assert!(
            is_liquid_block(flowing),
            "l'eau courante reste un bloc liquide"
        );
        let waterlogged_stairs = crate::blocks::state_with(
            crate::blocks::block_id_of_name("minecraft:oak_stairs").unwrap(),
            &[("waterlogged", "true")],
        )
        .unwrap();
        assert!(
            !is_liquid_block(waterlogged_stairs),
            "un bloc waterlogged n'est pas un bloc liquide"
        );
    }

    #[test]
    fn waterloggable_follows_property() {
        assert!(is_waterloggable(
            default_state_of_name("minecraft:oak_stairs").unwrap()
        ));
        assert!(is_waterloggable(
            default_state_of_name("minecraft:chest").unwrap()
        ));
        assert!(!is_waterloggable(
            default_state_of_name("minecraft:stone").unwrap()
        ));
        assert!(!is_waterloggable(
            default_state_of_name("minecraft:dirt").unwrap()
        ));
        assert!(!is_waterloggable(0));
    }
}
