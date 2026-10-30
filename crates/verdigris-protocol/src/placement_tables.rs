use std::sync::OnceLock;

use crate::blocks::{
    block_id_of_name, blocks_data, default_state_of_name, is_air, is_fluid, item_to_placed_state,
    property_of, state_to_block,
};
use crate::network::network_data;

pub const BLOCK_REGISTRY: &str = "minecraft:block";

pub const TALL_PLANTS: [&str; 6] = [
    "minecraft:tall_grass",
    "minecraft:large_fern",
    "minecraft:sunflower",
    "minecraft:lilac",
    "minecraft:rose_bush",
    "minecraft:peony",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SixFacingKind {
    OppositeLook,
    Look,
    Face,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizFacingKind {
    FenceGate,
    Anvil,
    OppositeHoriz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlabType {
    Top,
    Bottom,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementKind {
    Door,
    Bed,
    TallPlant,
    Simple,
}

struct Tables {
    stair_block: Box<[bool]>,
    slab_block: Box<[bool]>,
    six_facing: Box<[Option<SixFacingKind>]>,
    horiz_facing: Box<[Option<HorizFacingKind>]>,
    full_block: Box<[bool]>,
    replaceable_state: Box<[bool]>,
    door_block: Box<[bool]>,
    bed_block: Box<[bool]>,
    tall_plant_block: Box<[bool]>,
    item_kind: Box<[PlacementKind]>,
    water_source: i32,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
}

fn tag_block_table(name: &str, len: usize) -> Box<[bool]> {
    let mut table = vec![false; len].into_boxed_slice();
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
                        table[index] = true;
                    }
                }
            }
        }
    }
    table
}

fn classify_six(name: &str) -> SixFacingKind {
    const OPPOSITE_LOOK: [&str; 8] = [
        "minecraft:piston",
        "minecraft:sticky_piston",
        "minecraft:dispenser",
        "minecraft:dropper",
        "minecraft:barrel",
        "minecraft:command_block",
        "minecraft:chain_command_block",
        "minecraft:repeating_command_block",
    ];
    if OPPOSITE_LOOK.contains(&name) {
        return SixFacingKind::OppositeLook;
    }
    if name == "minecraft:observer" {
        return SixFacingKind::Look;
    }
    if name == "minecraft:end_rod"
        || name == "minecraft:lightning_rod"
        || name.ends_with("shulker_box")
        || name.ends_with("amethyst_cluster")
        || name.ends_with("_amethyst_bud")
    {
        return SixFacingKind::Face;
    }
    SixFacingKind::Default
}

fn classify_horiz(name: &str, is_fence_gate: bool) -> HorizFacingKind {
    if is_fence_gate {
        return HorizFacingKind::FenceGate;
    }
    if matches!(
        name,
        "minecraft:anvil" | "minecraft:chipped_anvil" | "minecraft:damaged_anvil"
    ) {
        return HorizFacingKind::Anvil;
    }
    HorizFacingKind::OppositeHoriz
}

fn excluded_from_full(name: &str) -> bool {
    matches!(
        name,
        "minecraft:lever"
            | "minecraft:end_portal_frame"
            | "minecraft:torch"
            | "minecraft:wall_torch"
    )
}

fn build_tables() -> Tables {
    let data = blocks_data();
    let id_len = data.blocks.iter().map(|block| block.id).max().unwrap_or(0) as usize + 1;
    let state_len = data.state_count as usize;

    let stair_block = tag_block_table("minecraft:stairs", id_len);
    let slab_block = tag_block_table("minecraft:slabs", id_len);
    let replaceable_block = tag_block_table("minecraft:replaceable", id_len);
    let fence_gate_block = tag_block_table("minecraft:fence_gates", id_len);

    let mut not_full_block = vec![false; id_len].into_boxed_slice();
    for tag in [
        "minecraft:slabs",
        "minecraft:stairs",
        "minecraft:doors",
        "minecraft:trapdoors",
        "minecraft:fence_gates",
        "minecraft:fences",
        "minecraft:walls",
        "minecraft:beds",
        "minecraft:buttons",
    ] {
        let members = tag_block_table(tag, id_len);
        for (index, member) in members.iter().enumerate() {
            if *member {
                not_full_block[index] = true;
            }
        }
    }

    let mut six_facing: Box<[Option<SixFacingKind>]> = vec![None; id_len].into_boxed_slice();
    let mut horiz_facing: Box<[Option<HorizFacingKind>]> = vec![None; id_len].into_boxed_slice();
    for block in &data.blocks {
        let id = block.id as usize;
        let Some(facing) = block.properties.iter().find(|p| p.name == "facing") else {
            continue;
        };
        let has_face = block.properties.iter().any(|p| p.name == "face");
        if facing.values.len() == 6 {
            six_facing[id] = Some(classify_six(&block.name));
        } else if facing.values.len() == 4 && !has_face {
            horiz_facing[id] = Some(classify_horiz(&block.name, fence_gate_block[id]));
        }
    }

    let mut full_block = vec![false; state_len].into_boxed_slice();
    let mut replaceable_state = vec![false; state_len].into_boxed_slice();
    for block in &data.blocks {
        let id = block.id as usize;
        let replaceable = replaceable_block[id];
        let blocked = not_full_block[id] || excluded_from_full(&block.name);
        for state in block.first_state..block.first_state + block.state_count {
            let index = state as usize;
            if index >= state_len {
                continue;
            }
            if replaceable {
                replaceable_state[index] = true;
            }
            full_block[index] = !is_air(state) && !is_fluid(state) && !replaceable && !blocked;
        }
    }

    let door_block = tag_block_table("minecraft:doors", id_len);
    let bed_block = tag_block_table("minecraft:beds", id_len);
    let mut tall_plant_block = vec![false; id_len].into_boxed_slice();
    for name in TALL_PLANTS {
        if let Some(block_id) = block_id_of_name(name)
            && let Ok(index) = usize::try_from(block_id)
            && index < id_len
        {
            tall_plant_block[index] = true;
        }
    }

    let item_len = data
        .items
        .iter()
        .map(|entry| entry.item)
        .max()
        .unwrap_or(-1) as usize
        + 1;
    let mut item_kind = vec![PlacementKind::Simple; item_len].into_boxed_slice();
    for entry in &data.items {
        let kind = classify_placement(
            state_to_block(entry.state).id,
            &door_block,
            &bed_block,
            &tall_plant_block,
        );
        if let Ok(index) = usize::try_from(entry.item)
            && index < item_len
        {
            item_kind[index] = kind;
        }
    }

    let water_source = default_state_of_name("minecraft:water").unwrap_or(-1);

    Tables {
        stair_block,
        slab_block,
        six_facing,
        horiz_facing,
        full_block,
        replaceable_state,
        door_block,
        bed_block,
        tall_plant_block,
        item_kind,
        water_source,
    }
}

fn classify_placement(
    block_id: i32,
    door_block: &[bool],
    bed_block: &[bool],
    tall_plant_block: &[bool],
) -> PlacementKind {
    if by_block_id(door_block, block_id, false) {
        PlacementKind::Door
    } else if by_block_id(bed_block, block_id, false) {
        PlacementKind::Bed
    } else if by_block_id(tall_plant_block, block_id, false) {
        PlacementKind::TallPlant
    } else {
        PlacementKind::Simple
    }
}

fn by_block_id<T: Copy>(table: &[T], block_id: i32, fallback: T) -> T {
    usize::try_from(block_id)
        .ok()
        .and_then(|index| table.get(index).copied())
        .unwrap_or(fallback)
}

fn by_state<T: Copy>(table: &[T], state: i32, fallback: T) -> T {
    usize::try_from(state)
        .ok()
        .and_then(|index| table.get(index).copied())
        .unwrap_or(fallback)
}

pub fn is_stair_block(block_id: i32) -> bool {
    by_block_id(&tables().stair_block, block_id, false)
}

pub fn is_slab_block(block_id: i32) -> bool {
    by_block_id(&tables().slab_block, block_id, false)
}

pub fn state_in_replaceable(state: i32) -> bool {
    by_state(&tables().replaceable_state, state, false)
}

pub fn full_block(state: i32) -> bool {
    by_state(&tables().full_block, state, false)
}

pub fn six_facing_kind(block_id: i32) -> Option<SixFacingKind> {
    by_block_id(&tables().six_facing, block_id, None)
}

pub fn horiz_facing_kind(block_id: i32) -> Option<HorizFacingKind> {
    by_block_id(&tables().horiz_facing, block_id, None)
}

pub fn water_source_state() -> i32 {
    tables().water_source
}

pub fn slab_type(state: i32) -> Option<SlabType> {
    match property_of(state, "type")? {
        "top" => Some(SlabType::Top),
        "bottom" => Some(SlabType::Bottom),
        "double" => Some(SlabType::Double),
        _ => None,
    }
}

pub fn slab_block_of_item(item: i32) -> Option<i32> {
    let state = item_to_placed_state(item)?;
    let id = state_to_block(state).id;
    if is_slab_block(id) { Some(id) } else { None }
}

pub fn placement_kind_of_item(item: i32) -> PlacementKind {
    usize::try_from(item)
        .ok()
        .and_then(|index| tables().item_kind.get(index).copied())
        .unwrap_or(PlacementKind::Simple)
}

pub fn placement_kind_of_block(block_id: i32) -> PlacementKind {
    let tables = tables();
    classify_placement(
        block_id,
        &tables.door_block,
        &tables.bed_block,
        &tables.tall_plant_block,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_id(name: &str) -> i32 {
        let state = default_state_of_name(name).unwrap();
        state_to_block(state).id
    }

    #[test]
    fn stairs_and_slabs_are_tagged() {
        assert!(is_stair_block(block_id("minecraft:oak_stairs")));
        assert!(is_slab_block(block_id("minecraft:oak_slab")));
        assert!(!is_stair_block(block_id("minecraft:stone")));
        assert!(!is_slab_block(block_id("minecraft:oak_stairs")));
    }

    #[test]
    fn replaceable_states_follow_tag() {
        assert!(state_in_replaceable(0));
        assert!(state_in_replaceable(
            default_state_of_name("minecraft:short_grass").unwrap()
        ));
        assert!(state_in_replaceable(
            default_state_of_name("minecraft:water").unwrap()
        ));
        assert!(!state_in_replaceable(
            default_state_of_name("minecraft:stone").unwrap()
        ));
    }

    #[test]
    fn full_block_excludes_fluids_and_partial_shapes() {
        assert!(full_block(
            default_state_of_name("minecraft:stone").unwrap()
        ));
        assert!(full_block(
            default_state_of_name("minecraft:grass_block").unwrap()
        ));
        assert!(!full_block(0));
        assert!(!full_block(
            default_state_of_name("minecraft:water").unwrap()
        ));
        assert!(!full_block(
            default_state_of_name("minecraft:oak_slab").unwrap()
        ));
        assert!(!full_block(
            default_state_of_name("minecraft:lever").unwrap()
        ));
        assert!(!full_block(
            default_state_of_name("minecraft:end_portal_frame").unwrap()
        ));
    }

    #[test]
    fn six_facing_families() {
        assert_eq!(
            six_facing_kind(block_id("minecraft:piston")),
            Some(SixFacingKind::OppositeLook)
        );
        assert_eq!(
            six_facing_kind(block_id("minecraft:observer")),
            Some(SixFacingKind::Look)
        );
        assert_eq!(
            six_facing_kind(block_id("minecraft:end_rod")),
            Some(SixFacingKind::Face)
        );
        assert_eq!(
            six_facing_kind(block_id("minecraft:shulker_box")),
            Some(SixFacingKind::Face)
        );
        assert_eq!(six_facing_kind(block_id("minecraft:stone")), None);
    }

    #[test]
    fn horizontal_facing_families() {
        assert_eq!(
            horiz_facing_kind(block_id("minecraft:oak_fence_gate")),
            Some(HorizFacingKind::FenceGate)
        );
        assert_eq!(
            horiz_facing_kind(block_id("minecraft:anvil")),
            Some(HorizFacingKind::Anvil)
        );
        assert_eq!(
            horiz_facing_kind(block_id("minecraft:furnace")),
            Some(HorizFacingKind::OppositeHoriz)
        );
        assert_eq!(
            horiz_facing_kind(block_id("minecraft:end_portal_frame")),
            Some(HorizFacingKind::OppositeHoriz)
        );
        assert_eq!(horiz_facing_kind(block_id("minecraft:lever")), None);
    }

    #[test]
    fn slab_type_reads_property() {
        let bottom = default_state_of_name("minecraft:oak_slab").unwrap();
        assert_eq!(slab_type(bottom), Some(SlabType::Bottom));
        assert_eq!(slab_type(0), None);
    }

    #[test]
    fn slab_block_of_item_resolves_slab_items() {
        let slab_state = default_state_of_name("minecraft:oak_slab").unwrap();
        let slab_item = crate::blocks::block_item_of_state(slab_state).unwrap();
        assert_eq!(
            slab_block_of_item(slab_item),
            Some(block_id("minecraft:oak_slab"))
        );

        let stone_item = crate::blocks::block_item_of_state(1).unwrap();
        assert_eq!(slab_block_of_item(stone_item), None);
    }

    #[test]
    fn water_source_is_default_water_state() {
        assert_eq!(
            water_source_state(),
            default_state_of_name("minecraft:water").unwrap()
        );
    }

    fn item_of(name: &str) -> i32 {
        let state = default_state_of_name(name).unwrap();
        crate::blocks::block_item_of_state(state).unwrap()
    }

    #[test]
    fn placement_kind_classifies_families() {
        assert_eq!(
            placement_kind_of_item(item_of("minecraft:oak_door")),
            PlacementKind::Door
        );
        assert_eq!(
            placement_kind_of_item(item_of("minecraft:white_bed")),
            PlacementKind::Bed
        );
        assert_eq!(
            placement_kind_of_item(item_of("minecraft:sunflower")),
            PlacementKind::TallPlant
        );
        assert_eq!(
            placement_kind_of_item(item_of("minecraft:tall_grass")),
            PlacementKind::TallPlant
        );
        assert_eq!(
            placement_kind_of_item(item_of("minecraft:dirt")),
            PlacementKind::Simple
        );
    }

    #[test]
    fn placement_kind_from_block_matches_families() {
        assert_eq!(
            placement_kind_of_block(block_id("minecraft:oak_door")),
            PlacementKind::Door
        );
        assert_eq!(
            placement_kind_of_block(block_id("minecraft:white_bed")),
            PlacementKind::Bed
        );
        assert_eq!(
            placement_kind_of_block(block_id("minecraft:peony")),
            PlacementKind::TallPlant
        );
        assert_eq!(
            placement_kind_of_block(block_id("minecraft:stone")),
            PlacementKind::Simple
        );
        assert_eq!(placement_kind_of_block(-1), PlacementKind::Simple);
    }
}
