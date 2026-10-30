use std::sync::OnceLock;

use crate::blocks::{block_id_of_name, blocks_data, item_id_of_name};
use crate::network::network_data;
use crate::placement_tables::BLOCK_REGISTRY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionKind {
    WoodenDoor,
    WoodenTrapdoor,
    FenceGate,
    Lever,
    EndPortalFrame,
}

struct Tables {
    by_block: Box<[Option<InteractionKind>]>,
    ender_eye: i32,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
}

fn apply_tag(table: &mut [Option<InteractionKind>], name: &str, kind: InteractionKind) {
    for registry in &network_data().tags {
        if registry.registry != BLOCK_REGISTRY {
            continue;
        }
        for tag in &registry.tags {
            if tag.name == name {
                for &id in &tag.ids {
                    if let Ok(index) = usize::try_from(id)
                        && index < table.len()
                    {
                        table[index] = Some(kind);
                    }
                }
            }
        }
    }
}

fn apply_block(table: &mut [Option<InteractionKind>], name: &str, kind: InteractionKind) {
    if let Some(id) = block_id_of_name(name)
        && let Ok(index) = usize::try_from(id)
        && index < table.len()
    {
        table[index] = Some(kind);
    }
}

fn build_tables() -> Tables {
    let data = blocks_data();
    let id_len = data.blocks.iter().map(|block| block.id).max().unwrap_or(0) as usize + 1;
    let mut by_block: Box<[Option<InteractionKind>]> = vec![None; id_len].into_boxed_slice();

    apply_tag(
        &mut by_block,
        "minecraft:wooden_doors",
        InteractionKind::WoodenDoor,
    );
    apply_tag(
        &mut by_block,
        "minecraft:wooden_trapdoors",
        InteractionKind::WoodenTrapdoor,
    );
    apply_tag(
        &mut by_block,
        "minecraft:fence_gates",
        InteractionKind::FenceGate,
    );
    apply_block(&mut by_block, "minecraft:lever", InteractionKind::Lever);
    apply_block(
        &mut by_block,
        "minecraft:end_portal_frame",
        InteractionKind::EndPortalFrame,
    );

    let ender_eye = item_id_of_name("minecraft:ender_eye").unwrap_or(-1);

    Tables {
        by_block,
        ender_eye,
    }
}

pub fn interaction_kind(block_id: i32) -> Option<InteractionKind> {
    usize::try_from(block_id)
        .ok()
        .and_then(|index| tables().by_block.get(index).copied())
        .flatten()
}

pub fn ender_eye_item() -> i32 {
    tables().ender_eye
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{default_state_of_name, state_to_block};

    fn block_id(name: &str) -> i32 {
        state_to_block(default_state_of_name(name).unwrap()).id
    }

    #[test]
    fn wooden_doors_and_trapdoors_classify() {
        assert_eq!(
            interaction_kind(block_id("minecraft:oak_door")),
            Some(InteractionKind::WoodenDoor)
        );
        assert_eq!(
            interaction_kind(block_id("minecraft:spruce_door")),
            Some(InteractionKind::WoodenDoor)
        );
        assert_eq!(
            interaction_kind(block_id("minecraft:oak_trapdoor")),
            Some(InteractionKind::WoodenTrapdoor)
        );
    }

    #[test]
    fn fence_gates_lever_and_frame_classify() {
        assert_eq!(
            interaction_kind(block_id("minecraft:oak_fence_gate")),
            Some(InteractionKind::FenceGate)
        );
        assert_eq!(
            interaction_kind(block_id("minecraft:lever")),
            Some(InteractionKind::Lever)
        );
        assert_eq!(
            interaction_kind(block_id("minecraft:end_portal_frame")),
            Some(InteractionKind::EndPortalFrame)
        );
    }

    #[test]
    fn iron_variants_are_excluded() {
        assert_eq!(interaction_kind(block_id("minecraft:iron_door")), None);
        assert_eq!(interaction_kind(block_id("minecraft:iron_trapdoor")), None);
    }

    #[test]
    fn plain_blocks_have_no_interaction() {
        assert_eq!(interaction_kind(block_id("minecraft:stone")), None);
        assert_eq!(interaction_kind(block_id("minecraft:dirt")), None);
        assert_eq!(interaction_kind(-1), None);
        assert_eq!(interaction_kind(i32::MAX), None);
    }

    #[test]
    fn ender_eye_item_matches_registry() {
        assert_eq!(
            ender_eye_item(),
            item_id_of_name("minecraft:ender_eye").unwrap()
        );
    }
}
