use tracing::warn;
use verdigris_protocol::{
    AIR_STATE, BlockPos, BucketIds, Direction, GameMode, ItemStack, PickItemFromBlock,
    PlayerAction, START_DESTROY_BLOCK, STOP_DESTROY_BLOCK, UseItem, UseItemOn, World,
    block_item_of_state, bucket_ids, horizontal_direction, is_air, is_filled_frame,
    is_liquid_block, is_source_fluid, is_waterloggable, lava_source_state, property_of,
    state_in_replaceable, state_to_block, water_source_state, with_property,
};

use crate::interaction::resolve_interaction;
use crate::placement::{
    PlaceContext, Placement, SmallPositions, offset, resolve_placement, twin_to_break,
};
use crate::portal;
use crate::raycast::{cast_ray, look_direction};

pub const GAMEMODE_ENV: &str = "VERDIGRIS_GAMEMODE";
pub const OPS_ENV: &str = "VERDIGRIS_OPS";
pub const DEFAULT_PERMISSION_ENV: &str = "VERDIGRIS_DEFAULT_PERMISSION";
pub const DEFAULT_PERMISSION_LEVEL: u8 = 4;
pub const OP_PERMISSION_LEVEL: u8 = 4;

pub const EYE_HEIGHT: f64 = 1.62;
pub const SNEAKING_EYE_HEIGHT: f64 = 1.27;
pub const REACH_SQUARED: f64 = 64.0;
pub const PICK_REACH_SQUARED: f64 = 81.0;
pub const SURVIVAL_REACH: f64 = 4.5;
pub const CREATIVE_REACH: f64 = 5.0;
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

pub fn resolve_permission(name: &str, ops: Option<&str>, default: Option<&str>) -> u8 {
    if let Some(list) = ops {
        let is_op = list
            .split(',')
            .map(|entry| entry.trim())
            .filter(|entry| !entry.is_empty())
            .any(|entry| entry.eq_ignore_ascii_case(name));
        if is_op {
            return OP_PERMISSION_LEVEL;
        }
    }
    match default {
        Some(raw) => match raw.trim().parse::<u8>() {
            Ok(level) if level <= 4 => level,
            _ => {
                warn!(
                    value = raw,
                    "VERDIGRIS_DEFAULT_PERMISSION invalide, repli sur 4"
                );
                DEFAULT_PERMISSION_LEVEL
            }
        },
        None => DEFAULT_PERMISSION_LEVEL,
    }
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

    fn first_empty(&self) -> Option<usize> {
        (0..INVENTORY_SLOTS).find(|&i| self.slots[i].is_none())
    }

    pub fn fill_bucket(&mut self, mode: GameMode, filled_item: i32) -> Vec<usize> {
        if mode == GameMode::Creative {
            if self
                .slots
                .iter()
                .any(|slot| slot_has_item(*slot, filled_item))
            {
                return Vec::new();
            }
            return match self.first_empty() {
                Some(empty) => {
                    self.slots[empty] = Some(ItemSlot {
                        item: filled_item,
                        count: 1,
                    });
                    vec![empty]
                }
                None => Vec::new(),
            };
        }

        let sel = self.selected;
        let count = self.slots[sel].map(|slot| slot.count).unwrap_or(1);
        if count <= 1 {
            self.slots[sel] = Some(ItemSlot {
                item: filled_item,
                count: 1,
            });
            return vec![sel];
        }

        if let Some(slot) = self.slots[sel].as_mut() {
            slot.count -= 1;
        }
        match self.first_empty() {
            Some(empty) => {
                self.slots[empty] = Some(ItemSlot {
                    item: filled_item,
                    count: 1,
                });
                let mut changed = vec![sel, empty];
                changed.sort_unstable();
                changed
            }
            None => vec![sel],
        }
    }

    pub fn empty_bucket(&mut self, mode: GameMode, empty_item: i32) -> Vec<usize> {
        if mode == GameMode::Creative {
            return Vec::new();
        }
        let sel = self.selected;
        let count = self.slots[sel].map(|slot| slot.count).unwrap_or(1);
        self.slots[sel] = Some(ItemSlot {
            item: empty_item,
            count,
        });
        vec![sel]
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

#[derive(Debug, Clone, PartialEq)]
pub struct UseItemResponse {
    pub updates: Vec<(BlockPos, i32)>,
    pub slots: Vec<SlotUpdate>,
    pub ack: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UseItemResult {
    ProtocolError,
    Respond(UseItemResponse),
}

fn is_bedrock(state: u16) -> bool {
    state_to_block(state as i32).name == BEDROCK
}

fn ack_only(ack: i32) -> UseItemResult {
    UseItemResult::Respond(UseItemResponse {
        updates: Vec::new(),
        slots: Vec::new(),
        ack,
    })
}

fn empty_bucket_stops(state: i32) -> bool {
    if is_air(state) {
        return false;
    }
    if is_liquid_block(state) && !is_source_fluid(state) {
        return false;
    }
    true
}

fn full_bucket_stops(state: i32) -> bool {
    !is_air(state) && !is_liquid_block(state)
}

pub struct PlayerState {
    pub name: String,
    pub mode: GameMode,
    pub flying: bool,
    pub pos: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
    pub sneaking: bool,
    pub skin: u8,
    pub main_hand_left: bool,
    pub view_distance: i8,
    pub permission_level: u8,
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
            skin: 0,
            main_hand_left: false,
            view_distance: 0,
            permission_level: DEFAULT_PERMISSION_LEVEL,
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
            let mut twin = None;
            if breakable {
                world.set_state(action.pos.x, action.pos.y, action.pos.z, AIR_STATE);
                if let Some(twin_pos) = twin_to_break(world, action.pos, state) {
                    world.set_state(twin_pos.x, twin_pos.y, twin_pos.z, AIR_STATE);
                    twin = Some(twin_pos);
                }
            }
            let resync = world.state_at(action.pos.x, action.pos.y, action.pos.z);
            updates.push((action.pos, resync as i32));
            if let Some(twin_pos) = twin {
                let twin_state = world.state_at(twin_pos.x, twin_pos.y, twin_pos.z);
                updates.push((twin_pos, twin_state as i32));
            }
        }

        ActionResult::Respond {
            updates,
            ack: action.sequence,
        }
    }

    fn try_interact(
        &mut self,
        world: &World,
        packet: &UseItemOn,
        p: BlockPos,
    ) -> Option<ActionResult> {
        if packet.hand != 0 || self.mode == GameMode::Spectator || !self.within_reach(p) {
            return None;
        }
        let held = self.inventory.selected_item().map(|slot| slot.item);
        if self.sneaking && held.is_some() {
            return None;
        }

        let state_p = world.state_at(p.x, p.y, p.z) as i32;
        let other_half = match property_of(state_p, "half") {
            Some("lower") => Some(world.state_at(p.x, p.y + 1, p.z) as i32),
            Some("upper") => Some(world.state_at(p.x, p.y - 1, p.z) as i32),
            _ => None,
        };

        let outcome = resolve_interaction(
            state_p,
            other_half,
            horizontal_direction(self.yaw),
            self.mode,
            held,
        )?;

        let mut updates = Vec::with_capacity(outcome.changes().len());
        for &(delta, new_state) in outcome.changes() {
            let pos = p.offset(delta.x, delta.y, delta.z);
            world.set_state(pos.x, pos.y, pos.z, new_state as u16);
            let written = world.state_at(pos.x, pos.y, pos.z) as i32;
            updates.push((pos, written));
        }
        if outcome.consume_eye() && self.mode == GameMode::Survival {
            self.inventory.consume_one(self.inventory.selected);
        }

        let completed = updates.first().copied();
        if let Some((pos, new_state)) = completed
            && pos == p
            && is_filled_frame(new_state)
        {
            portal::try_complete(world, p, &mut updates);
        }

        Some(ActionResult::Respond {
            updates,
            ack: packet.sequence,
        })
    }

    pub fn handle_use_item_on(&mut self, world: &World, packet: UseItemOn) -> ActionResult {
        if packet.sequence < 0 {
            return ActionResult::ProtocolError;
        }

        let p = packet.pos;
        let face = Direction::from_face(packet.face);

        if let Some(result) = self.try_interact(world, &packet, p) {
            return result;
        }

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
            Placement::PlacePair { writes, resync } => {
                for (pos, state) in writes {
                    world.set_state(pos.x, pos.y, pos.z, state as u16);
                }
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

    fn bucket_eye(&self) -> [f64; 3] {
        let height = if self.sneaking {
            SNEAKING_EYE_HEIGHT
        } else {
            EYE_HEIGHT
        };
        [self.pos.0, self.pos.1 + height, self.pos.2]
    }

    fn slot_updates(&self, changed: Vec<usize>) -> Vec<SlotUpdate> {
        changed
            .into_iter()
            .map(|index| SlotUpdate {
                menu_slot: index_to_menu_slot(index),
                stack: slot_to_stack(self.inventory.slots[index]),
            })
            .collect()
    }

    pub fn handle_use_item(&mut self, world: &World, packet: UseItem) -> UseItemResult {
        if packet.sequence < 0 {
            return UseItemResult::ProtocolError;
        }
        let ack = packet.sequence;
        if packet.hand != 0 || !(packet.yaw.is_finite() && packet.pitch.is_finite()) {
            return ack_only(ack);
        }
        let reach = match self.mode {
            GameMode::Creative => CREATIVE_REACH,
            GameMode::Survival => SURVIVAL_REACH,
            GameMode::Adventure | GameMode::Spectator => return ack_only(ack),
        };
        let Some(held) = self.inventory.selected_item().map(|slot| slot.item) else {
            return ack_only(ack);
        };
        let bids = bucket_ids();
        if held == bids.empty {
            self.fill_from_world(world, packet, reach, bids, ack)
        } else if held == bids.water || held == bids.lava {
            self.empty_into_world(world, packet, reach, held, bids, ack)
        } else {
            ack_only(ack)
        }
    }

    fn fill_from_world(
        &mut self,
        world: &World,
        packet: UseItem,
        reach: f64,
        bids: BucketIds,
        ack: i32,
    ) -> UseItemResult {
        let origin = self.bucket_eye();
        let dir = look_direction(packet.yaw, packet.pitch);
        let read = |p: BlockPos| world.state_at(p.x, p.y, p.z) as i32;
        let Some(hit) = cast_ray(origin, dir, reach, read, empty_bucket_stops) else {
            return ack_only(ack);
        };
        let state = world.state_at(hit.pos.x, hit.pos.y, hit.pos.z) as i32;
        let (new_state, filled) = if is_source_fluid(state) {
            let filled = if state == lava_source_state() {
                bids.lava
            } else {
                bids.water
            };
            (AIR_STATE as i32, filled)
        } else if property_of(state, "waterlogged") == Some("true") {
            (
                with_property(state, "waterlogged", "false").unwrap_or(state),
                bids.water,
            )
        } else {
            return ack_only(ack);
        };
        world.set_state(hit.pos.x, hit.pos.y, hit.pos.z, new_state as u16);
        let written = world.state_at(hit.pos.x, hit.pos.y, hit.pos.z) as i32;
        let changed = self.inventory.fill_bucket(self.mode, filled);
        UseItemResult::Respond(UseItemResponse {
            updates: vec![(hit.pos, written)],
            slots: self.slot_updates(changed),
            ack,
        })
    }

    fn empty_into_world(
        &mut self,
        world: &World,
        packet: UseItem,
        reach: f64,
        held: i32,
        bids: BucketIds,
        ack: i32,
    ) -> UseItemResult {
        let origin = self.bucket_eye();
        let dir = look_direction(packet.yaw, packet.pitch);
        let read = |p: BlockPos| world.state_at(p.x, p.y, p.z) as i32;
        let Some(hit) = cast_ray(origin, dir, reach, read, full_bucket_stops) else {
            return ack_only(ack);
        };
        let hit_state = world.state_at(hit.pos.x, hit.pos.y, hit.pos.z) as i32;
        let is_water = held == bids.water;
        let source = if is_water {
            water_source_state()
        } else {
            lava_source_state()
        };

        let target = if is_water
            && is_waterloggable(hit_state)
            && property_of(hit_state, "waterlogged") == Some("false")
        {
            match with_property(hit_state, "waterlogged", "true") {
                Some(state) => (hit.pos, state),
                None => return ack_only(ack),
            }
        } else {
            let Some(face) = hit.face else {
                return ack_only(ack);
            };
            let (dx, dy, dz) = face.delta();
            let cell = hit.pos.offset(dx, dy, dz);
            let cell_state = world.state_at(cell.x, cell.y, cell.z) as i32;
            if is_air(cell_state) || state_in_replaceable(cell_state) {
                (cell, source)
            } else {
                return ack_only(ack);
            }
        };

        let (pos, new_state) = target;
        world.set_state(pos.x, pos.y, pos.z, new_state as u16);
        let written = world.state_at(pos.x, pos.y, pos.z) as i32;
        let changed = self.inventory.empty_bucket(self.mode, bids.empty);
        UseItemResult::Respond(UseItemResponse {
            updates: vec![(pos, written)],
            slots: self.slot_updates(changed),
            ack,
        })
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
            skin: 0,
            main_hand_left: false,
            view_distance: 0,
            permission_level: DEFAULT_PERMISSION_LEVEL,
            inventory,
            container_state_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verdigris_protocol::{
        block_id_of_name, default_state_of_name, item_id_of_name, property_of, state_with,
        with_property,
    };

    #[test]
    fn permission_ops_list_grants_level_four() {
        assert_eq!(resolve_permission("Alice", Some("Alice,Bob"), Some("0")), 4);
        assert_eq!(resolve_permission("bob", Some("Alice,Bob"), Some("0")), 4);
        assert_eq!(resolve_permission("Eve", Some("Alice,Bob"), Some("0")), 0);
    }

    #[test]
    fn permission_default_used_without_ops() {
        assert_eq!(resolve_permission("Any", None, None), 4);
        assert_eq!(resolve_permission("Any", None, Some("2")), 2);
        assert_eq!(resolve_permission("Any", None, Some("9")), 4);
        assert_eq!(resolve_permission("Any", None, Some("x")), 4);
    }

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

    fn door_halves() -> (i32, i32, i32) {
        let door = block_id_of_name("minecraft:oak_door").unwrap();
        let lower = state_with(door, &[("half", "lower")]).unwrap();
        let upper = state_with(door, &[("half", "upper")]).unwrap();
        (door, lower, upper)
    }

    #[test]
    fn breaking_door_removes_both_halves_from_below() {
        let world = World::flat().unwrap();
        let (_, lower, upper) = door_halves();
        world.set_state(0, -60, 0, lower as u16);
        world.set_state(0, -59, 0, upper as u16);
        let mut player = player(GameMode::Creative);
        let lower_pos = BlockPos { x: 0, y: -60, z: 0 };
        let upper_pos = BlockPos { x: 0, y: -59, z: 0 };
        let (updates, ack) = respond(player.handle_player_action(&world, action(0, lower_pos, 4)));
        assert_eq!(updates, vec![(lower_pos, 0), (upper_pos, 0)]);
        assert_eq!(ack, 4);
        assert_eq!(world.state_at(0, -60, 0), 0);
        assert_eq!(world.state_at(0, -59, 0), 0);
    }

    #[test]
    fn breaking_door_removes_both_halves_from_above() {
        let world = World::flat().unwrap();
        let (_, lower, upper) = door_halves();
        world.set_state(0, -60, 0, lower as u16);
        world.set_state(0, -59, 0, upper as u16);
        let mut player = player(GameMode::Creative);
        let lower_pos = BlockPos { x: 0, y: -60, z: 0 };
        let upper_pos = BlockPos { x: 0, y: -59, z: 0 };
        let (updates, _) = respond(player.handle_player_action(&world, action(0, upper_pos, 1)));
        assert_eq!(updates, vec![(upper_pos, 0), (lower_pos, 0)]);
        assert_eq!(world.state_at(0, -60, 0), 0);
        assert_eq!(world.state_at(0, -59, 0), 0);
    }

    #[test]
    fn breaking_bed_removes_both_parts() {
        let world = World::flat().unwrap();
        let bed = block_id_of_name("minecraft:white_bed").unwrap();
        let foot = state_with(bed, &[("facing", "south"), ("part", "foot")]).unwrap();
        let head = state_with(bed, &[("facing", "south"), ("part", "head")]).unwrap();
        world.set_state(0, -60, 0, foot as u16);
        world.set_state(0, -60, 1, head as u16);
        let mut player = player(GameMode::Creative);
        let foot_pos = BlockPos { x: 0, y: -60, z: 0 };
        let head_pos = BlockPos { x: 0, y: -60, z: 1 };
        let (updates, _) = respond(player.handle_player_action(&world, action(0, head_pos, 1)));
        assert_eq!(updates, vec![(head_pos, 0), (foot_pos, 0)]);
        assert_eq!(world.state_at(0, -60, 0), 0);
        assert_eq!(world.state_at(0, -60, 1), 0);
    }

    #[test]
    fn breaking_tall_plant_removes_both_halves() {
        let world = World::flat().unwrap();
        let plant = block_id_of_name("minecraft:tall_grass").unwrap();
        let lower = state_with(plant, &[("half", "lower")]).unwrap();
        let upper = state_with(plant, &[("half", "upper")]).unwrap();
        world.set_state(0, -60, 0, lower as u16);
        world.set_state(0, -59, 0, upper as u16);
        let mut player = player(GameMode::Creative);
        let lower_pos = BlockPos { x: 0, y: -60, z: 0 };
        let upper_pos = BlockPos { x: 0, y: -59, z: 0 };
        let (updates, _) = respond(player.handle_player_action(&world, action(0, lower_pos, 1)));
        assert_eq!(updates, vec![(lower_pos, 0), (upper_pos, 0)]);
    }

    #[test]
    fn breaking_lonely_half_leaves_twin_untouched() {
        let world = World::flat().unwrap();
        let (_, lower, _) = door_halves();
        world.set_state(0, -60, 0, lower as u16);
        world.set_state(0, -59, 0, lower as u16);
        let mut player = player(GameMode::Creative);
        let lower_pos = BlockPos { x: 0, y: -60, z: 0 };
        let (updates, _) = respond(player.handle_player_action(&world, action(0, lower_pos, 1)));
        assert_eq!(updates, vec![(lower_pos, 0)], "jumeau incoherent non casse");
        assert_eq!(world.state_at(0, -59, 0), lower as u16);
    }

    #[test]
    fn survival_door_consumes_a_single_item() {
        let world = World::flat().unwrap();
        let (door, _, _) = door_halves();
        let door_item = block_item_of_state(default_state_of_name("minecraft:oak_door").unwrap())
            .expect("objet porte");
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: door_item,
            count: 2,
        });
        let mut player = PlayerState::with_inventory(GameMode::Survival, SPAWN, inventory);
        let ground = BlockPos { x: 0, y: -61, z: 0 };
        let (updates, _) = respond(player.handle_use_item_on(&world, use_item(ground, 1, 1)));
        assert_eq!(updates.len(), 2, "deux block_update pour les deux moities");
        assert_eq!(
            player.inventory.selected_item(),
            Some(ItemSlot {
                item: door_item,
                count: 1
            }),
            "une paire consomme exactement un objet"
        );
        assert_eq!(state_to_block(world.state_at(0, -60, 0) as i32).id, door);
        assert_eq!(
            property_of(world.state_at(0, -59, 0) as i32, "half"),
            Some("upper")
        );
    }

    fn closed_door(world: &World) -> (BlockPos, BlockPos) {
        let door = block_id_of_name("minecraft:oak_door").unwrap();
        let lower = state_with(door, &[("half", "lower"), ("open", "false")]).unwrap();
        let upper = state_with(door, &[("half", "upper"), ("open", "false")]).unwrap();
        world.set_state(0, -60, 0, lower as u16);
        world.set_state(0, -59, 0, upper as u16);
        (
            BlockPos { x: 0, y: -60, z: 0 },
            BlockPos { x: 0, y: -59, z: 0 },
        )
    }

    fn door_open(world: &World) -> Option<&'static str> {
        property_of(world.state_at(0, -60, 0) as i32, "open")
    }

    #[test]
    fn creative_right_click_opens_then_closes_wooden_door() {
        let world = World::flat().unwrap();
        let (lower_pos, upper_pos) = closed_door(&world);
        let mut player = PlayerState::with_inventory(GameMode::Creative, SPAWN, Inventory::new());

        let (updates, ack) = respond(player.handle_use_item_on(&world, use_item(lower_pos, 1, 1)));
        assert_eq!(updates.len(), 2, "une porte modifie ses deux moities");
        assert_eq!(ack, 1);
        let positions: Vec<BlockPos> = updates.iter().map(|(pos, _)| *pos).collect();
        assert!(positions.contains(&lower_pos) && positions.contains(&upper_pos));
        assert_eq!(door_open(&world), Some("true"));
        assert_eq!(
            property_of(world.state_at(0, -59, 0) as i32, "open"),
            Some("true"),
            "les deux moities s'ouvrent"
        );

        let (updates, _) = respond(player.handle_use_item_on(&world, use_item(upper_pos, 1, 2)));
        assert_eq!(updates.len(), 2);
        assert_eq!(door_open(&world), Some("false"));
        assert_eq!(
            property_of(world.state_at(0, -59, 0) as i32, "open"),
            Some("false")
        );
    }

    #[test]
    fn sneaking_with_item_skips_interaction() {
        let world = World::flat().unwrap();
        let (lower_pos, _) = closed_door(&world);
        let mut player = player(GameMode::Creative);
        player.sneaking = true;
        let _ = respond(player.handle_use_item_on(&world, use_item(lower_pos, 1, 1)));
        assert_eq!(
            door_open(&world),
            Some("false"),
            "accroupi avec objet: aucune interaction"
        );
    }

    #[test]
    fn spectator_never_interacts() {
        let world = World::flat().unwrap();
        let (lower_pos, _) = closed_door(&world);
        let mut player = PlayerState::with_inventory(GameMode::Spectator, SPAWN, Inventory::new());
        let _ = respond(player.handle_use_item_on(&world, use_item(lower_pos, 1, 1)));
        assert_eq!(
            door_open(&world),
            Some("false"),
            "spectateur: aucune interaction"
        );
    }

    #[test]
    fn empty_hand_opens_wooden_door_without_sneak_guard() {
        let world = World::flat().unwrap();
        let (lower_pos, _) = closed_door(&world);
        let mut player = player(GameMode::Creative);
        player.sneaking = true;
        player.inventory = Inventory::new();
        let (updates, _) = respond(player.handle_use_item_on(&world, use_item(lower_pos, 1, 1)));
        assert_eq!(
            updates.len(),
            2,
            "accroupi mais mains vides: interaction permise"
        );
        assert_eq!(door_open(&world), Some("true"));
    }

    const DOWN: f32 = 90.0;
    const UP: f32 = -90.0;

    fn water_src() -> i32 {
        default_state_of_name("minecraft:water").unwrap()
    }

    fn lava_src() -> i32 {
        default_state_of_name("minecraft:lava").unwrap()
    }

    fn bucket() -> i32 {
        item_id_of_name("minecraft:bucket").unwrap()
    }

    fn water_bucket() -> i32 {
        item_id_of_name("minecraft:water_bucket").unwrap()
    }

    fn lava_bucket() -> i32 {
        item_id_of_name("minecraft:lava_bucket").unwrap()
    }

    fn stairs(waterlogged: &str) -> u16 {
        state_with(
            block_id_of_name("minecraft:oak_stairs").unwrap(),
            &[("waterlogged", waterlogged)],
        )
        .unwrap() as u16
    }

    fn holding(mode: GameMode, item: i32, count: i32) -> PlayerState {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot { item, count });
        PlayerState::with_inventory(mode, SPAWN, inventory)
    }

    fn ui(sequence: i32, yaw: f32, pitch: f32) -> UseItem {
        UseItem {
            hand: 0,
            sequence,
            yaw,
            pitch,
        }
    }

    fn use_respond(result: UseItemResult) -> UseItemResponse {
        match result {
            UseItemResult::Respond(response) => response,
            other => panic!("attendu Respond, obtenu {other:?}"),
        }
    }

    #[test]
    fn fill_bucket_survival_rules() {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: bucket(),
            count: 1,
        });
        assert_eq!(
            inventory.fill_bucket(GameMode::Survival, water_bucket()),
            vec![0]
        );
        assert_eq!(
            inventory.slots[0],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );

        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: bucket(),
            count: 2,
        });
        assert_eq!(
            inventory.fill_bucket(GameMode::Survival, water_bucket()),
            vec![0, 1]
        );
        assert_eq!(
            inventory.slots[0],
            Some(ItemSlot {
                item: bucket(),
                count: 1
            })
        );
        assert_eq!(
            inventory.slots[1],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );

        let mut inventory = Inventory::new();
        for cell in inventory.slots.iter_mut() {
            *cell = Some(ItemSlot { item: 55, count: 1 });
        }
        inventory.slots[0] = Some(ItemSlot {
            item: bucket(),
            count: 2,
        });
        assert_eq!(
            inventory.fill_bucket(GameMode::Survival, water_bucket()),
            vec![0]
        );
        assert_eq!(
            inventory.slots[0],
            Some(ItemSlot {
                item: bucket(),
                count: 1
            })
        );
        assert!(
            inventory
                .slots
                .iter()
                .all(|slot| !matches!(slot, Some(s) if s.item == water_bucket())),
            "inventaire plein: le seau plein est perdu"
        );
    }

    #[test]
    fn fill_bucket_creative_rules() {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: bucket(),
            count: 1,
        });
        assert_eq!(
            inventory.fill_bucket(GameMode::Creative, water_bucket()),
            vec![1]
        );
        assert_eq!(
            inventory.slots[0],
            Some(ItemSlot {
                item: bucket(),
                count: 1
            }),
            "le seau tenu reste inchangé en créatif"
        );
        assert_eq!(
            inventory.slots[1],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );
        assert!(
            inventory
                .fill_bucket(GameMode::Creative, water_bucket())
                .is_empty(),
            "objet déjà présent: aucun ajout"
        );

        let mut inventory = Inventory::new();
        for cell in inventory.slots.iter_mut() {
            *cell = Some(ItemSlot { item: 55, count: 1 });
        }
        assert!(
            inventory
                .fill_bucket(GameMode::Creative, water_bucket())
                .is_empty(),
            "inventaire plein en créatif: aucun ajout"
        );
    }

    #[test]
    fn empty_bucket_inventory_rules() {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: water_bucket(),
            count: 1,
        });
        assert_eq!(
            inventory.empty_bucket(GameMode::Survival, bucket()),
            vec![0]
        );
        assert_eq!(
            inventory.slots[0],
            Some(ItemSlot {
                item: bucket(),
                count: 1
            })
        );

        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: water_bucket(),
            count: 1,
        });
        assert!(
            inventory
                .empty_bucket(GameMode::Creative, bucket())
                .is_empty(),
            "créatif: pas de mutation d'inventaire"
        );
        assert_eq!(
            inventory.slots[0],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );
    }

    #[test]
    fn empty_bucket_picks_water_source_survival() {
        let world = World::flat().unwrap();
        world.set_state(0, -61, 0, water_src() as u16);
        let mut player = holding(GameMode::Survival, bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        assert_eq!(response.ack, 1);
        assert_eq!(response.updates, vec![(BlockPos { x: 0, y: -61, z: 0 }, 0)]);
        assert_eq!(world.state_at(0, -61, 0), 0);
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );
        assert_eq!(
            response.slots,
            vec![SlotUpdate {
                menu_slot: 36,
                stack: ItemStack::Item {
                    item: water_bucket(),
                    count: 1
                }
            }]
        );
    }

    #[test]
    fn empty_bucket_picks_lava_source_survival() {
        let world = World::flat().unwrap();
        world.set_state(0, -61, 0, lava_src() as u16);
        let mut player = holding(GameMode::Survival, bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        assert_eq!(response.updates, vec![(BlockPos { x: 0, y: -61, z: 0 }, 0)]);
        assert_eq!(world.state_at(0, -61, 0), 0);
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: lava_bucket(),
                count: 1
            })
        );
    }

    #[test]
    fn empty_bucket_creative_adds_filled_without_consuming_held() {
        let world = World::flat().unwrap();
        world.set_state(0, -61, 0, water_src() as u16);
        let mut player = holding(GameMode::Creative, bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(2, 0.0, DOWN)));
        assert_eq!(world.state_at(0, -61, 0), 0);
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: bucket(),
                count: 1
            })
        );
        assert_eq!(
            player.inventory.slots[1],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );
        assert_eq!(response.ack, 2);
    }

    #[test]
    fn empty_bucket_drains_waterlogged_block() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, stairs("true"));
        let mut player = holding(GameMode::Survival, bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        let (pos, state) = response.updates[0];
        assert_eq!(pos, BlockPos { x: 0, y: -60, z: 0 });
        assert_eq!(property_of(state, "waterlogged"), Some("false"));
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );
    }

    #[test]
    fn empty_bucket_traverses_flowing_fluid_to_source() {
        let world = World::flat().unwrap();
        let flowing = with_property(water_src(), "level", "1").unwrap();
        world.set_state(0, -60, 0, flowing as u16);
        world.set_state(0, -61, 0, water_src() as u16);
        let mut player = holding(GameMode::Survival, bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        assert_eq!(
            response.updates,
            vec![(BlockPos { x: 0, y: -61, z: 0 }, 0)],
            "la source sous l'eau courante est prise"
        );
        assert_eq!(
            world.state_at(0, -60, 0),
            flowing as u16,
            "l'eau courante n'est pas touchée"
        );
    }

    #[test]
    fn empty_bucket_without_source_only_acks() {
        let world = World::flat().unwrap();
        let mut player = holding(GameMode::Creative, bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(7, 0.0, UP)));
        assert!(response.updates.is_empty() && response.slots.is_empty());
        assert_eq!(response.ack, 7);
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: bucket(),
                count: 1
            })
        );
    }

    #[test]
    fn water_bucket_waterlogs_targeted_block() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, stairs("false"));
        let mut player = holding(GameMode::Creative, water_bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        assert_eq!(response.updates.len(), 1);
        let (pos, state) = response.updates[0];
        assert_eq!(pos, BlockPos { x: 0, y: -60, z: 0 });
        assert_eq!(property_of(state, "waterlogged"), Some("true"));
        assert_eq!(
            property_of(world.state_at(0, -60, 0) as i32, "waterlogged"),
            Some("true")
        );
        assert!(
            response.slots.is_empty(),
            "créatif ne change pas l'inventaire"
        );
    }

    #[test]
    fn water_bucket_on_waterlogged_block_places_on_face() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, stairs("true"));
        let mut player = holding(GameMode::Creative, water_bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        let (pos, state) = response.updates[0];
        assert_eq!(
            pos,
            BlockPos { x: 0, y: -59, z: 0 },
            "déjà waterlogged: source posée sur la face (au-dessus)"
        );
        assert_eq!(state, water_src());
    }

    #[test]
    fn lava_bucket_never_waterlogs() {
        let world = World::flat().unwrap();
        world.set_state(0, -60, 0, stairs("false"));
        let mut player = holding(GameMode::Creative, lava_bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        let (pos, state) = response.updates[0];
        assert_eq!(pos, BlockPos { x: 0, y: -59, z: 0 });
        assert_eq!(state, lava_src());
        assert_eq!(
            property_of(world.state_at(0, -60, 0) as i32, "waterlogged"),
            Some("false"),
            "le bloc visé reste non waterlogged"
        );
    }

    #[test]
    fn full_bucket_survival_success_empties_bucket() {
        let world = World::flat().unwrap();
        let mut player = holding(GameMode::Survival, water_bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        let (pos, state) = response.updates[0];
        assert_eq!(pos, BlockPos { x: 0, y: -60, z: 0 });
        assert_eq!(state, water_src());
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: bucket(),
                count: 1
            })
        );
        assert_eq!(
            response.slots,
            vec![SlotUpdate {
                menu_slot: 36,
                stack: ItemStack::Item {
                    item: bucket(),
                    count: 1
                }
            }]
        );
    }

    #[test]
    fn full_bucket_without_hit_only_acks() {
        let world = World::flat().unwrap();
        let mut player = holding(GameMode::Survival, water_bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, UP)));
        assert!(response.updates.is_empty() && response.slots.is_empty());
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            }),
            "échec: le seau reste plein"
        );
    }

    #[test]
    fn full_bucket_without_valid_target_keeps_world_and_inventory() {
        let world = World::flat().unwrap();
        world.set_state(
            0,
            -59,
            0,
            default_state_of_name("minecraft:stone").unwrap() as u16,
        );
        let mut player = holding(GameMode::Survival, water_bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(1, 0.0, DOWN)));
        assert!(response.updates.is_empty() && response.slots.is_empty());
        assert_eq!(
            player.inventory.slots[0],
            Some(ItemSlot {
                item: water_bucket(),
                count: 1
            })
        );
        assert_eq!(
            world.state_at(0, -59, 0),
            default_state_of_name("minecraft:stone").unwrap() as u16
        );
    }

    #[test]
    fn non_bucket_item_only_acks() {
        let world = World::flat().unwrap();
        world.set_state(0, -61, 0, water_src() as u16);
        let mut player = holding(GameMode::Creative, 55, 1);
        let response = use_respond(player.handle_use_item(&world, ui(3, 0.0, DOWN)));
        assert!(response.updates.is_empty() && response.slots.is_empty());
        assert_eq!(response.ack, 3);
        assert_eq!(world.state_at(0, -61, 0), water_src() as u16);
    }

    #[test]
    fn off_hand_bucket_only_acks() {
        let world = World::flat().unwrap();
        world.set_state(0, -61, 0, water_src() as u16);
        let mut player = holding(GameMode::Creative, bucket(), 1);
        let mut packet = ui(4, 0.0, DOWN);
        packet.hand = 1;
        let response = use_respond(player.handle_use_item(&world, packet));
        assert!(response.updates.is_empty() && response.slots.is_empty());
        assert_eq!(response.ack, 4);
        assert_eq!(world.state_at(0, -61, 0), water_src() as u16);
    }

    #[test]
    fn non_finite_orientation_acks_without_changes() {
        let world = World::flat().unwrap();
        world.set_state(0, -61, 0, water_src() as u16);
        let mut player = holding(GameMode::Creative, bucket(), 1);
        let response = use_respond(player.handle_use_item(&world, ui(5, f32::NAN, 0.0)));
        assert_eq!(response.ack, 5);
        assert!(response.updates.is_empty() && response.slots.is_empty());
        assert_eq!(world.state_at(0, -61, 0), water_src() as u16);
    }

    #[test]
    fn negative_sequence_use_item_is_protocol_error() {
        let world = World::flat().unwrap();
        let mut player = holding(GameMode::Creative, bucket(), 1);
        assert_eq!(
            player.handle_use_item(&world, ui(-1, 0.0, DOWN)),
            UseItemResult::ProtocolError
        );
    }

    #[test]
    fn reach_differs_between_survival_and_creative() {
        let origin = (0.0, -60.0, 0.0);

        let world = World::flat().unwrap();
        world.set_state(5, -59, 0, water_src() as u16);
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: bucket(),
            count: 1,
        });
        let mut survival = PlayerState::with_inventory(GameMode::Survival, origin, inventory);
        let response = use_respond(survival.handle_use_item(&world, ui(1, -90.0, 0.0)));
        assert!(
            response.updates.is_empty(),
            "survie 4.5: cible à 5.0 hors portée"
        );
        assert_eq!(world.state_at(5, -59, 0), water_src() as u16);

        let world = World::flat().unwrap();
        world.set_state(5, -59, 0, water_src() as u16);
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(ItemSlot {
            item: bucket(),
            count: 1,
        });
        let mut creative = PlayerState::with_inventory(GameMode::Creative, origin, inventory);
        let response = use_respond(creative.handle_use_item(&world, ui(1, -90.0, 0.0)));
        assert_eq!(
            response.updates,
            vec![(BlockPos { x: 5, y: -59, z: 0 }, 0)],
            "créatif 5.0: cible à 5.0 atteinte"
        );
    }
}
