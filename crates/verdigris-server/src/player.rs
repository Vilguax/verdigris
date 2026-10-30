use tracing::warn;
use verdigris_protocol::{
    AIR_STATE, BlockPos, Direction, GameMode, ItemStack, PickItemFromBlock, PlayerAction,
    START_DESTROY_BLOCK, STOP_DESTROY_BLOCK, UseItemOn, World, block_item_of_state, state_to_block,
};

use crate::placement::{PlaceContext, Placement, SmallPositions, offset, resolve_placement};

pub const GAMEMODE_ENV: &str = "VERDIGRIS_GAMEMODE";

pub const EYE_HEIGHT: f64 = 1.62;
pub const REACH_SQUARED: f64 = 64.0;
pub const PICK_REACH_SQUARED: f64 = 81.0;
pub const BEDROCK: &str = "minecraft:bedrock";

pub const INVENTORY_SLOTS: usize = 36;
pub const HOTBAR_SLOTS: usize = 9;

pub fn resolve_gamemode(value: Option<&str>) -> GameMode {
    match value {
        None => GameMode::Creative,
        Some(raw) => match GameMode::parse(raw) {
            Some(mode) => mode,
            None => {
                warn!(
                    value = raw,
                    "VERDIGRIS_GAMEMODE inconnu, repli sur creative"
                );
                GameMode::Creative
            }
        },
    }
}

pub fn read_gamemode() -> GameMode {
    resolve_gamemode(std::env::var(GAMEMODE_ENV).ok().as_deref())
}

pub const SPAWN: (f64, f64, f64) = (0.5, -60.0, 0.5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSlot {
    pub item: i32,
    pub count: i32,
}

pub fn menu_slot_to_index(menu: i16) -> Option<usize> {
    if (9..=35).contains(&menu) {
        Some(menu as usize)
    } else if (36..=44).contains(&menu) {
        Some((menu - 36) as usize)
    } else {
        None
    }
}

pub fn index_to_menu_slot(index: usize) -> i16 {
    if index < HOTBAR_SLOTS {
        36 + index as i16
    } else {
        index as i16
    }
}

fn slot_has_item(slot: Option<ItemSlot>, item: i32) -> bool {
    matches!(slot, Some(s) if s.item == item)
}

fn slot_to_stack(slot: Option<ItemSlot>) -> ItemStack {
    match slot {
        Some(ItemSlot { item, count }) => ItemStack::Item { item, count },
        None => ItemStack::Empty,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub slots: [Option<ItemSlot>; INVENTORY_SLOTS],
    pub selected: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Inventory {
            slots: [None; INVENTORY_SLOTS],
            selected: 0,
        }
    }
}

impl Inventory {
    pub fn new() -> Self {
        Inventory::default()
    }

    pub fn set_carried(&mut self, slot: i16) {
        if (0..=8).contains(&slot) {
            self.selected = slot as usize;
        }
    }

    pub fn set_creative_slot(&mut self, mode: GameMode, slot: i16, stack: ItemStack) {
        if mode != GameMode::Creative {
            return;
        }
        if let Some(index) = menu_slot_to_index(slot) {
            self.slots[index] = match stack {
                ItemStack::Empty => None,
                ItemStack::Item { item, count } => Some(ItemSlot { item, count }),
            };
        }
    }

    pub fn selected_item(&self) -> Option<ItemSlot> {
        self.slots[self.selected]
    }

    pub fn consume_one(&mut self, index: usize) {
        if let Some(slot) = self.slots[index].as_mut() {
            slot.count -= 1;
            if slot.count <= 0 {
                self.slots[index] = None;
            }
        }
    }

    pub fn adapted_hotbar_slot(&self) -> usize {
        for k in 0..HOTBAR_SLOTS {
            let index = (self.selected + k) % HOTBAR_SLOTS;
            if self.slots[index].is_none() {
                return index;
            }
        }
        self.selected
    }

    pub fn pick_block_item(&mut self, mode: GameMode, item: i32) -> Vec<usize> {
        if let Some(found) = (0..INVENTORY_SLOTS).find(|&i| slot_has_item(self.slots[i], item)) {
            if found < HOTBAR_SLOTS {
                self.selected = found;
                return Vec::new();
            }
            let target = self.adapted_hotbar_slot();
            self.slots.swap(target, found);
            self.selected = target;
            let mut changed = vec![target, found];
            changed.sort_unstable();
            return changed;
        }

        if mode != GameMode::Creative {
            return Vec::new();
        }

        let target = self.adapted_hotbar_slot();
        let mut changed = Vec::new();
        if self.slots[target].is_some() {
            let empty = (0..INVENTORY_SLOTS).find(|&i| i != target && self.slots[i].is_none());
            if let Some(empty) = empty {
                self.slots[empty] = self.slots[target].take();
                changed.push(empty);
            }
        }
        self.slots[target] = Some(ItemSlot { item, count: 1 });
        self.selected = target;
        changed.push(target);
        changed.sort_unstable();
        changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotUpdate {
    pub menu_slot: i16,
    pub stack: ItemStack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickResult {
    Ignored,
    Respond {
        slots: Vec<SlotUpdate>,
        selected: i16,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionResult {
    ProtocolError,
    Ignored,
    Respond {
        updates: Vec<(BlockPos, i32)>,
        ack: i32,
    },
}

fn is_bedrock(state: u16) -> bool {
    state_to_block(state as i32).name == BEDROCK
}

pub struct PlayerState {
    pub name: String,
    pub mode: GameMode,
    pub flying: bool,
    pub pos: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
    pub sneaking: bool,
    pub inventory: Inventory,
    pub container_state_id: i32,
}

impl PlayerState {
    pub fn connect(name: &str) -> Self {
        PlayerState::with_mode(name, read_gamemode())
    }

    pub fn with_mode(name: &str, mode: GameMode) -> Self {
        let flying = (mode.abilities_flags() & 0x02) != 0;
        PlayerState {
            name: name.to_owned(),
            mode,
            flying,
            pos: SPAWN,
            yaw: 0.0,
            pitch: 0.0,
            sneaking: false,
            inventory: Inventory::new(),
            container_state_id: 0,
        }
    }

    pub fn orientation(&self) -> (f32, f32, bool) {
        (self.yaw, self.pitch, self.sneaking)
    }

    pub fn eye_pos(&self) -> (f64, f64, f64) {
        (self.pos.0, self.pos.1 + EYE_HEIGHT, self.pos.2)
    }

    pub fn next_container_state_id(&mut self) -> i32 {
        self.container_state_id += 1;
        self.container_state_id
    }

    fn within_reach_squared(&self, pos: BlockPos, max_squared: f64) -> bool {
        let (ex, ey, ez) = self.eye_pos();
        let dx = ex - (pos.x as f64 + 0.5);
        let dy = ey - (pos.y as f64 + 0.5);
        let dz = ez - (pos.z as f64 + 0.5);
        dx * dx + dy * dy + dz * dz <= max_squared
    }

    pub fn within_reach(&self, pos: BlockPos) -> bool {
        self.within_reach_squared(pos, REACH_SQUARED)
    }

    pub fn within_pick_reach(&self, pos: BlockPos) -> bool {
        self.within_reach_squared(pos, PICK_REACH_SQUARED)
    }

    pub fn handle_player_action(&mut self, world: &World, action: PlayerAction) -> ActionResult {
        if action.sequence < 0 {
            return ActionResult::ProtocolError;
        }
        if !(0..=3).contains(&action.action) {
            return ActionResult::Ignored;
        }

        let break_action = match self.mode {
            GameMode::Creative => Some(START_DESTROY_BLOCK),
            GameMode::Survival | GameMode::Adventure => Some(STOP_DESTROY_BLOCK),
            GameMode::Spectator => None,
        };

        let mut updates = Vec::new();
        if break_action == Some(action.action) {
            let in_reach = self.within_reach(action.pos);
            let state = world.state_at(action.pos.x, action.pos.y, action.pos.z);
            let breakable = in_reach && (self.mode == GameMode::Creative || !is_bedrock(state));
            if breakable {
                world.set_state(action.pos.x, action.pos.y, action.pos.z, AIR_STATE);
            }
            let resync = world.state_at(action.pos.x, action.pos.y, action.pos.z);
            updates.push((action.pos, resync as i32));
        }

        ActionResult::Respond {
            updates,
            ack: action.sequence,
        }
    }

    pub fn handle_use_item_on(&mut self, world: &World, packet: UseItemOn) -> ActionResult {
        if packet.sequence < 0 {
            return ActionResult::ProtocolError;
        }

        let p = packet.pos;
        let face = Direction::from_face(packet.face);

        let placement = if packet.hand == 0
            && matches!(self.mode, GameMode::Creative | GameMode::Survival)
            && self.within_reach(p)
            && let Some(dir) = face
            && let Some(slot) = self.inventory.selected_item()
        {
            let ctx = PlaceContext {
                pos: p,
                face: dir,
                cursor: (packet.cursor[0], packet.cursor[1], packet.cursor[2]),
                yaw: self.yaw,
                pitch: self.pitch,
            };
            resolve_placement(world, ctx, slot.item)
        } else {
            Placement::Nothing
        };

        let resync = match placement {
            Placement::Place {
                write,
                state,
                resync,
            }
            | Placement::Fuse {
                write,
                state,
                resync,
            } => {
                world.set_state(write.x, write.y, write.z, state as u16);
                if self.mode == GameMode::Survival {
                    self.inventory.consume_one(self.inventory.selected);
                }
                resync
            }
            Placement::Nothing => {
                let target = match face {
                    Some(dir) => offset(p, dir),
                    None => p,
                };
                SmallPositions::pair(p, target)
            }
        };

        let mut updates = Vec::with_capacity(resync.as_slice().len());
        for &pos in resync.as_slice() {
            let state = world.state_at(pos.x, pos.y, pos.z) as i32;
            updates.push((pos, state));
        }

        ActionResult::Respond {
            updates,
            ack: packet.sequence,
        }
    }

    pub fn handle_pick_item_from_block(
        &mut self,
        world: &World,
        packet: PickItemFromBlock,
    ) -> PickResult {
        if !self.within_pick_reach(packet.pos) {
            return PickResult::Ignored;
        }
        let chunk_x = packet.pos.x.div_euclid(16);
        let chunk_z = packet.pos.z.div_euclid(16);
        if !world.is_chunk_loaded(chunk_x, chunk_z) {
            return PickResult::Ignored;
        }
        let state = world.state_at(packet.pos.x, packet.pos.y, packet.pos.z) as i32;
        let Some(item) = block_item_of_state(state) else {
            return PickResult::Ignored;
        };
        let changed = self.inventory.pick_block_item(self.mode, item);
        let slots = changed
            .into_iter()
            .map(|index| SlotUpdate {
                menu_slot: index_to_menu_slot(index),
                stack: slot_to_stack(self.inventory.slots[index]),
            })
            .collect();
        PickResult::Respond {
            slots,
            selected: self.inventory.selected as i16,
        }
    }

    #[cfg(test)]
    pub fn with_inventory(mode: GameMode, pos: (f64, f64, f64), inventory: Inventory) -> Self {
        PlayerState {
            name: "test".to_owned(),
            mode,
            flying: false,
            pos,
            yaw: 0.0,
            pitch: 0.0,
            sneaking: false,
            inventory,
            container_state_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_env_is_creative() {
        assert_eq!(resolve_gamemode(None), GameMode::Creative);
    }

    #[test]
    fn known_values_are_mapped() {
        assert_eq!(resolve_gamemode(Some("survival")), GameMode::Survival);
        assert_eq!(resolve_gamemode(Some("creative")), GameMode::Creative);
        assert_eq!(resolve_gamemode(Some("adventure")), GameMode::Adventure);
        assert_eq!(resolve_gamemode(Some("spectator")), GameMode::Spectator);
    }

    #[test]
    fn unknown_value_falls_back_to_creative() {
        assert_eq!(resolve_gamemode(Some("xxx")), GameMode::Creative);
        assert_eq!(resolve_gamemode(Some("")), GameMode::Creative);
    }

    #[test]
    fn flying_tracks_the_abilities_flag() {
        assert!(!PlayerState::with_mode("Verdi", GameMode::Survival).flying);
        assert!(!PlayerState::with_mode("Verdi", GameMode::Creative).flying);
        assert!(!PlayerState::with_mode("Verdi", GameMode::Adventure).flying);
        assert!(PlayerState::with_mode("Verdi", GameMode::Spectator).flying);
    }

    const GRASS_STATE: i32 = 9;
    const DIRT_STATE: i32 = 10;
    const BEDROCK_STATE: i32 = 88;
    const DIRT_ITEM: i32 = 55;

    const GRASS_ITEM: i32 = 54;

    fn dirt_inventory() -> Inventory {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: DIRT_ITEM,
            count: 1,
        });
        inventory
    }

    fn player(mode: GameMode) -> PlayerState {
        PlayerState::with_inventory(mode, SPAWN, dirt_inventory())
    }

    fn slot(item: i32) -> Option<ItemSlot> {
        Some(ItemSlot { item, count: 1 })
    }

    fn action(action: i32, pos: BlockPos, sequence: i32) -> PlayerAction {
        PlayerAction {
            action,
            pos,
            face: 1,
            sequence,
        }
    }

    fn use_item(pos: BlockPos, face: i32, sequence: i32) -> UseItemOn {
        UseItemOn {
            hand: 0,
            pos,
            face,
            cursor: [0.5, 0.5, 0.5],
            inside: false,
            world_border_hit: false,
            sequence,
        }
    }

    fn respond(result: ActionResult) -> (Vec<(BlockPos, i32)>, i32) {
        match result {
            ActionResult::Respond { updates, ack } => (updates, ack),
            other => panic!("attendu Respond, obtenu {other:?}"),
        }
    }

    #[test]
    fn creative_breaks_on_start() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        let (updates, ack) = respond(player.handle_player_action(&world, action(0, pos, 3)));
        assert_eq!(updates, vec![(pos, 0)]);
        assert_eq!(ack, 3);
        assert_eq!(world.state_at(0, -61, 0), 0);
    }

    #[test]
    fn creative_start_outside_reach_resyncs_without_breaking() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos {
            x: 40,
            y: -61,
            z: 0,
        };
        let (updates, ack) = respond(player.handle_player_action(&world, action(0, pos, 1)));
        assert_eq!(updates, vec![(pos, GRASS_STATE)]);
        assert_eq!(ack, 1);
        assert_eq!(world.state_at(40, -61, 0), GRASS_STATE as u16);
    }

    #[test]
    fn survival_breaks_on_stop_not_start() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Survival);
        let pos = BlockPos { x: 0, y: -61, z: 0 };

        let (updates, ack) = respond(player.handle_player_action(&world, action(0, pos, 1)));
        assert!(updates.is_empty(), "START en survie ne touche pas le bloc");
        assert_eq!(ack, 1);
        assert_eq!(world.state_at(0, -61, 0), GRASS_STATE as u16);

        let (updates, ack) = respond(player.handle_player_action(&world, action(3, pos, 2)));
        assert_eq!(updates, vec![(pos, 0)]);
        assert_eq!(ack, 2);
        assert_eq!(world.state_at(0, -61, 0), 0);
    }

    #[test]
    fn adventure_breaks_on_stop() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Adventure);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        let (updates, _) = respond(player.handle_player_action(&world, action(3, pos, 1)));
        assert_eq!(updates, vec![(pos, 0)]);
        assert_eq!(world.state_at(0, -61, 0), 0);
    }

    #[test]
    fn spectator_never_breaks_but_acks() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Spectator);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        for a in 0..=3 {
            let (updates, ack) = respond(player.handle_player_action(&world, action(a, pos, a)));
            assert!(updates.is_empty(), "spectateur ne resynchronise pas");
            assert_eq!(ack, a);
        }
        assert_eq!(world.state_at(0, -61, 0), GRASS_STATE as u16);
    }

    #[test]
    fn abort_and_change_only_ack() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        for a in [1, 2] {
            let (updates, ack) = respond(player.handle_player_action(&world, action(a, pos, a)));
            assert!(updates.is_empty());
            assert_eq!(ack, a);
        }
        assert_eq!(world.state_at(0, -61, 0), GRASS_STATE as u16);
    }

    #[test]
    fn bedrock_survives_in_survival() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Survival);
        let pos = BlockPos { x: 0, y: -64, z: 0 };
        let (updates, ack) = respond(player.handle_player_action(&world, action(3, pos, 5)));
        assert_eq!(updates, vec![(pos, BEDROCK_STATE)]);
        assert_eq!(ack, 5);
        assert_eq!(world.state_at(0, -64, 0), BEDROCK_STATE as u16);
    }

    #[test]
    fn bedrock_breaks_in_creative() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -64, z: 0 };
        let (updates, _) = respond(player.handle_player_action(&world, action(0, pos, 1)));
        assert_eq!(updates, vec![(pos, 0)]);
        assert_eq!(world.state_at(0, -64, 0), 0);
    }

    #[test]
    fn unknown_actions_are_ignored() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        for a in [4, 5, 8, 9, 100] {
            assert_eq!(
                player.handle_player_action(&world, action(a, pos, 1)),
                ActionResult::Ignored
            );
        }
        assert_eq!(world.state_at(0, -61, 0), GRASS_STATE as u16);
    }

    #[test]
    fn negative_sequence_is_protocol_error() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        assert_eq!(
            player.handle_player_action(&world, action(0, pos, -1)),
            ActionResult::ProtocolError
        );
        assert_eq!(
            player.handle_use_item_on(&world, use_item(pos, 1, -1)),
            ActionResult::ProtocolError
        );
    }

    #[test]
    fn creative_places_selected_item() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        let target = BlockPos { x: 0, y: -60, z: 0 };
        let (updates, ack) = respond(player.handle_use_item_on(&world, use_item(pos, 1, 7)));
        assert_eq!(updates, vec![(pos, GRASS_STATE), (target, DIRT_STATE)]);
        assert_eq!(ack, 7);
        assert_eq!(world.state_at(0, -60, 0), DIRT_STATE as u16);
        assert_eq!(
            player.inventory.selected_item(),
            Some(ItemSlot {
                item: DIRT_ITEM,
                count: 1
            }),
            "le créatif ne consomme pas"
        );
    }

    #[test]
    fn survival_places_and_consumes_one() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Survival);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        let (updates, _) = respond(player.handle_use_item_on(&world, use_item(pos, 1, 1)));
        assert_eq!(
            updates,
            vec![
                (pos, GRASS_STATE),
                (BlockPos { x: 0, y: -60, z: 0 }, DIRT_STATE)
            ]
        );
        assert_eq!(world.state_at(0, -60, 0), DIRT_STATE as u16);
        assert_eq!(player.inventory.selected_item(), None, "la survie consomme");
    }

    #[test]
    fn empty_slot_refuses_placement() {
        let world = World::flat().unwrap();
        let mut player = PlayerState::with_inventory(GameMode::Creative, SPAWN, Inventory::new());
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        let (updates, _) = respond(player.handle_use_item_on(&world, use_item(pos, 1, 1)));
        assert_eq!(
            updates,
            vec![(pos, GRASS_STATE), (BlockPos { x: 0, y: -60, z: 0 }, 0)]
        );
        assert_eq!(world.state_at(0, -60, 0), 0);
    }

    #[test]
    fn non_air_target_refuses_placement() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -62, z: 0 };
        let target = BlockPos { x: 0, y: -61, z: 0 };
        let (updates, _) = respond(player.handle_use_item_on(&world, use_item(pos, 1, 1)));
        assert_eq!(updates, vec![(pos, DIRT_STATE), (target, GRASS_STATE)]);
        assert_eq!(world.state_at(0, -61, 0), GRASS_STATE as u16);
    }

    #[test]
    fn placement_outside_reach_resyncs_both_sides() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos {
            x: 40,
            y: -61,
            z: 0,
        };
        let target = BlockPos {
            x: 40,
            y: -60,
            z: 0,
        };
        let (updates, ack) = respond(player.handle_use_item_on(&world, use_item(pos, 1, 9)));
        assert_eq!(updates, vec![(pos, GRASS_STATE), (target, 0)]);
        assert_eq!(ack, 9);
        assert_eq!(world.state_at(40, -60, 0), 0, "hors de portée: aucune pose");
    }

    #[test]
    fn off_hand_resyncs_without_placing() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -61, z: 0 };
        let target = BlockPos { x: 0, y: -60, z: 0 };
        let mut packet = use_item(pos, 1, 4);
        packet.hand = 1;
        let (updates, ack) = respond(player.handle_use_item_on(&world, packet));
        assert_eq!(updates, vec![(pos, GRASS_STATE), (target, 0)]);
        assert_eq!(ack, 4);
        assert_eq!(world.state_at(0, -60, 0), 0);
    }

    #[test]
    fn target_below_world_resyncs_as_air() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        let pos = BlockPos { x: 0, y: -64, z: 0 };
        let target = BlockPos { x: 0, y: -65, z: 0 };
        let (updates, ack) = respond(player.handle_use_item_on(&world, use_item(pos, 0, 2)));
        assert_eq!(updates, vec![(pos, BEDROCK_STATE), (target, 0)]);
        assert_eq!(ack, 2);
        assert_eq!(world.state_at(0, -64, 0), BEDROCK_STATE as u16);
    }

    #[test]
    fn inventory_carried_and_creative_slots() {
        let mut inventory = Inventory::new();
        inventory.set_carried(8);
        assert_eq!(inventory.selected, 8);
        inventory.set_carried(9);
        assert_eq!(inventory.selected, 8, "hors borne ignoré");
        inventory.set_carried(-1);
        assert_eq!(inventory.selected, 8);

        inventory.set_creative_slot(
            GameMode::Creative,
            36,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        assert_eq!(
            inventory.slots[0],
            Some(ItemSlot {
                item: DIRT_ITEM,
                count: 1
            })
        );

        inventory.set_creative_slot(GameMode::Creative, 36, ItemStack::Empty);
        assert_eq!(inventory.slots[0], None);

        inventory.set_creative_slot(
            GameMode::Survival,
            37,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        assert_eq!(inventory.slots[1], None, "hors créatif ignoré");

        inventory.set_creative_slot(
            GameMode::Creative,
            45,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        assert!(
            inventory.slots.iter().filter(|slot| slot.is_some()).count() == 0,
            "slot hors 9..=44 ignoré"
        );
    }

    #[test]
    fn creative_slot_fills_reserve_cases() {
        let mut inventory = Inventory::new();
        inventory.set_creative_slot(
            GameMode::Creative,
            9,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        inventory.set_creative_slot(
            GameMode::Creative,
            35,
            ItemStack::Item {
                item: GRASS_ITEM,
                count: 1,
            },
        );
        assert_eq!(inventory.slots[9], slot(DIRT_ITEM));
        assert_eq!(inventory.slots[35], slot(GRASS_ITEM));
        inventory.set_creative_slot(
            GameMode::Creative,
            8,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        assert!(
            inventory.slots.iter().filter(|s| s.is_some()).count() == 2,
            "slot menu 8 hors de la plage 9..=44"
        );
    }

    #[test]
    fn menu_slot_mapping_bounds() {
        assert_eq!(menu_slot_to_index(9), Some(9));
        assert_eq!(menu_slot_to_index(35), Some(35));
        assert_eq!(menu_slot_to_index(36), Some(0));
        assert_eq!(menu_slot_to_index(44), Some(8));
        assert_eq!(menu_slot_to_index(8), None);
        assert_eq!(menu_slot_to_index(45), None);

        assert_eq!(index_to_menu_slot(0), 36);
        assert_eq!(index_to_menu_slot(8), 44);
        assert_eq!(index_to_menu_slot(9), 9);
        assert_eq!(index_to_menu_slot(35), 35);
    }

    #[test]
    fn adapted_hotbar_slot_rules() {
        let mut inventory = Inventory::new();
        inventory.selected = 3;
        assert_eq!(inventory.adapted_hotbar_slot(), 3, "selected vide renvoyée");

        for cell in inventory.slots.iter_mut().take(HOTBAR_SLOTS) {
            *cell = slot(DIRT_ITEM);
        }
        inventory.slots[6] = None;
        inventory.selected = 3;
        assert_eq!(
            inventory.adapted_hotbar_slot(),
            6,
            "première vide en tournant depuis selected"
        );

        for cell in inventory.slots.iter_mut().take(HOTBAR_SLOTS) {
            *cell = slot(DIRT_ITEM);
        }
        inventory.selected = 2;
        assert_eq!(
            inventory.adapted_hotbar_slot(),
            2,
            "aucune vide renvoie selected"
        );
    }

    fn pick(pos: BlockPos, include_data: bool) -> PickItemFromBlock {
        PickItemFromBlock { pos, include_data }
    }

    fn pick_respond(result: PickResult) -> (Vec<SlotUpdate>, i16) {
        match result {
            PickResult::Respond { slots, selected } => (slots, selected),
            other => panic!("attendu Respond, obtenu {other:?}"),
        }
    }

    const GRASS_POS: BlockPos = BlockPos { x: 0, y: -61, z: 0 };

    fn loaded_flat() -> World {
        let world = World::flat().unwrap();
        let _ = world.state_at(GRASS_POS.x, GRASS_POS.y, GRASS_POS.z);
        world
    }

    #[test]
    fn pick_found_in_hotbar_only_changes_selection() {
        let world = loaded_flat();
        let mut inventory = Inventory::new();
        inventory.slots[4] = slot(GRASS_ITEM);
        inventory.selected = 0;
        let mut player = PlayerState::with_inventory(GameMode::Creative, SPAWN, inventory);
        let result = player.handle_pick_item_from_block(&world, pick(GRASS_POS, false));
        let (slots, selected) = pick_respond(result);
        assert!(slots.is_empty(), "objet en barre: aucune case modifiée");
        assert_eq!(selected, 4);
        assert_eq!(player.inventory.selected, 4);
    }

    #[test]
    fn pick_found_in_reserve_swaps_two_slots() {
        let world = loaded_flat();
        let mut inventory = Inventory::new();
        inventory.slots[0] = slot(DIRT_ITEM);
        inventory.slots[20] = slot(GRASS_ITEM);
        inventory.selected = 0;
        let mut player = PlayerState::with_inventory(GameMode::Survival, SPAWN, inventory);
        let result = player.handle_pick_item_from_block(&world, pick(GRASS_POS, false));
        let (slots, selected) = pick_respond(result);
        assert_eq!(selected, 1, "barre adaptée = première vide depuis 0");
        assert_eq!(
            slots,
            vec![
                SlotUpdate {
                    menu_slot: 37,
                    stack: ItemStack::Item {
                        item: GRASS_ITEM,
                        count: 1
                    },
                },
                SlotUpdate {
                    menu_slot: 20,
                    stack: ItemStack::Empty,
                },
            ],
            "ordre croissant des index: barre (index 1) puis réserve (index 20)"
        );
        assert_eq!(player.inventory.slots[1], slot(GRASS_ITEM));
        assert_eq!(player.inventory.slots[20], None);
    }

    #[test]
    fn pick_creative_not_found_with_empty_hotbar_places_without_displacement() {
        let world = loaded_flat();
        let mut inventory = Inventory::new();
        inventory.slots[0] = slot(DIRT_ITEM);
        inventory.selected = 0;
        let mut player = PlayerState::with_inventory(GameMode::Creative, SPAWN, inventory);
        let result = player.handle_pick_item_from_block(&world, pick(GRASS_POS, false));
        let (slots, selected) = pick_respond(result);
        assert_eq!(selected, 1);
        assert_eq!(
            slots,
            vec![SlotUpdate {
                menu_slot: 37,
                stack: ItemStack::Item {
                    item: GRASS_ITEM,
                    count: 1
                },
            }]
        );
        assert_eq!(player.inventory.slots[0], slot(DIRT_ITEM));
        assert_eq!(player.inventory.slots[1], slot(GRASS_ITEM));
    }

    #[test]
    fn pick_creative_not_found_with_full_hotbar_displaces_to_reserve() {
        let world = loaded_flat();
        let mut inventory = Inventory::new();
        for cell in inventory.slots.iter_mut().take(HOTBAR_SLOTS) {
            *cell = slot(DIRT_ITEM);
        }
        inventory.selected = 0;
        let mut player = PlayerState::with_inventory(GameMode::Creative, SPAWN, inventory);
        let result = player.handle_pick_item_from_block(&world, pick(GRASS_POS, false));
        let (slots, selected) = pick_respond(result);
        assert_eq!(selected, 0);
        assert_eq!(
            slots,
            vec![
                SlotUpdate {
                    menu_slot: 36,
                    stack: ItemStack::Item {
                        item: GRASS_ITEM,
                        count: 1
                    },
                },
                SlotUpdate {
                    menu_slot: 9,
                    stack: ItemStack::Item {
                        item: DIRT_ITEM,
                        count: 1
                    },
                },
            ],
            "déplacement: herbe en index 0, terre en première vide (index 9)"
        );
        assert_eq!(player.inventory.slots[0], slot(GRASS_ITEM));
        assert_eq!(player.inventory.slots[9], slot(DIRT_ITEM));
    }

    #[test]
    fn pick_creative_not_found_full_inventory_overwrites_single_slot() {
        let world = loaded_flat();
        let mut inventory = Inventory::new();
        for cell in inventory.slots.iter_mut() {
            *cell = slot(DIRT_ITEM);
        }
        inventory.selected = 2;
        let mut player = PlayerState::with_inventory(GameMode::Creative, SPAWN, inventory);
        let result = player.handle_pick_item_from_block(&world, pick(GRASS_POS, false));
        let (slots, selected) = pick_respond(result);
        assert_eq!(selected, 2);
        assert_eq!(
            slots,
            vec![SlotUpdate {
                menu_slot: 38,
                stack: ItemStack::Item {
                    item: GRASS_ITEM,
                    count: 1
                },
            }],
            "inventaire plein: écrasement d'une seule case"
        );
        assert_eq!(player.inventory.slots[2], slot(GRASS_ITEM));
    }

    #[test]
    fn pick_survival_not_found_only_sets_held_slot() {
        let world = loaded_flat();
        let mut inventory = Inventory::new();
        inventory.slots[0] = slot(DIRT_ITEM);
        inventory.selected = 0;
        let mut player = PlayerState::with_inventory(GameMode::Survival, SPAWN, inventory);
        let result = player.handle_pick_item_from_block(&world, pick(GRASS_POS, false));
        let (slots, selected) = pick_respond(result);
        assert!(slots.is_empty(), "survie introuvable: aucune case modifiée");
        assert_eq!(selected, 0, "case choisie inchangée");
        assert_eq!(player.inventory.slots[0], slot(DIRT_ITEM));
    }

    #[test]
    fn pick_on_air_block_is_ignored() {
        let world = loaded_flat();
        let mut player = player(GameMode::Creative);
        let air = BlockPos { x: 0, y: -59, z: 0 };
        assert_eq!(
            player.handle_pick_item_from_block(&world, pick(air, false)),
            PickResult::Ignored
        );
    }

    #[test]
    fn pick_on_water_block_is_ignored() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, 89);
        let _ = world.state_at(0, -60, 0);
        let mut player = player(GameMode::Creative);
        let water = BlockPos { x: 0, y: -60, z: 0 };
        assert_eq!(
            player.handle_pick_item_from_block(&world, pick(water, false)),
            PickResult::Ignored
        );
    }

    #[test]
    fn pick_outside_reach_is_ignored() {
        let world = loaded_flat();
        let mut player = player(GameMode::Creative);
        let far = BlockPos {
            x: 40,
            y: -61,
            z: 0,
        };
        assert_eq!(
            player.handle_pick_item_from_block(&world, pick(far, false)),
            PickResult::Ignored
        );
    }

    #[test]
    fn pick_on_unloaded_chunk_is_ignored() {
        let world = World::flat().unwrap();
        let mut player = player(GameMode::Creative);
        assert_eq!(
            player.handle_pick_item_from_block(&world, pick(GRASS_POS, false)),
            PickResult::Ignored
        );
    }

    #[test]
    fn container_state_id_increments_per_player() {
        let mut player = player(GameMode::Creative);
        assert_eq!(player.next_container_state_id(), 1);
        assert_eq!(player.next_container_state_id(), 2);
        assert_eq!(player.next_container_state_id(), 3);
    }
}
