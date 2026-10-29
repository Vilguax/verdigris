use tracing::warn;
use verdigris_protocol::{
    AIR_STATE, BlockPos, GameMode, ItemStack, PlayerAction, START_DESTROY_BLOCK,
    STOP_DESTROY_BLOCK, UseItemOn, WORLD_MAX_Y, WORLD_MIN_Y, World, is_air, item_to_placed_state,
    state_to_block,
};

pub const GAMEMODE_ENV: &str = "VERDIGRIS_GAMEMODE";

pub const EYE_HEIGHT: f64 = 1.62;
pub const REACH_SQUARED: f64 = 64.0;
pub const BEDROCK: &str = "minecraft:bedrock";

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hotbar {
    pub slots: [Option<ItemSlot>; 9],
    pub selected: usize,
}

impl Hotbar {
    pub fn new() -> Self {
        Hotbar::default()
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
        if (36..=44).contains(&slot) {
            self.slots[(slot - 36) as usize] = match stack {
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

fn face_offset(face: i32) -> (i32, i32, i32) {
    match face {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => (0, 0, 0),
    }
}

pub struct PlayerState {
    pub name: String,
    pub mode: GameMode,
    pub flying: bool,
    pub pos: (f64, f64, f64),
    pub hotbar: Hotbar,
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
            hotbar: Hotbar::new(),
        }
    }

    pub fn eye_pos(&self) -> (f64, f64, f64) {
        (self.pos.0, self.pos.1 + EYE_HEIGHT, self.pos.2)
    }

    pub fn within_reach(&self, pos: BlockPos) -> bool {
        let (ex, ey, ez) = self.eye_pos();
        let dx = ex - (pos.x as f64 + 0.5);
        let dy = ey - (pos.y as f64 + 0.5);
        let dz = ez - (pos.z as f64 + 0.5);
        dx * dx + dy * dy + dz * dz <= REACH_SQUARED
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

        let (dx, dy, dz) = face_offset(packet.face);
        let target = packet.pos.offset(dx, dy, dz);

        if packet.hand == 0 {
            let can_place = matches!(self.mode, GameMode::Creative | GameMode::Survival);
            if can_place
                && let Some(slot) = self.hotbar.selected_item()
                && let Some(state) = item_to_placed_state(slot.item)
                && self.within_reach(packet.pos)
                && (WORLD_MIN_Y..=WORLD_MAX_Y).contains(&target.y)
                && is_air(world.state_at(target.x, target.y, target.z) as i32)
            {
                world.set_state(target.x, target.y, target.z, state as u16);
                if self.mode == GameMode::Survival {
                    self.hotbar.consume_one(self.hotbar.selected);
                }
            }
        }

        let pos_state = world.state_at(packet.pos.x, packet.pos.y, packet.pos.z);
        let target_state = world.state_at(target.x, target.y, target.z);
        ActionResult::Respond {
            updates: vec![
                (packet.pos, pos_state as i32),
                (target, target_state as i32),
            ],
            ack: packet.sequence,
        }
    }

    #[cfg(test)]
    pub fn with_hotbar(mode: GameMode, pos: (f64, f64, f64), hotbar: Hotbar) -> Self {
        PlayerState {
            name: "test".to_owned(),
            mode,
            flying: false,
            pos,
            hotbar,
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

    fn dirt_hotbar() -> Hotbar {
        let mut hotbar = Hotbar::new();
        hotbar.slots[0] = Some(ItemSlot {
            item: DIRT_ITEM,
            count: 1,
        });
        hotbar
    }

    fn player(mode: GameMode) -> PlayerState {
        PlayerState::with_hotbar(mode, SPAWN, dirt_hotbar())
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
            player.hotbar.selected_item(),
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
        assert_eq!(player.hotbar.selected_item(), None, "la survie consomme");
    }

    #[test]
    fn empty_slot_refuses_placement() {
        let world = World::flat().unwrap();
        let mut player = PlayerState::with_hotbar(GameMode::Creative, SPAWN, Hotbar::new());
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
    fn hotbar_carried_and_creative_slots() {
        let mut hotbar = Hotbar::new();
        hotbar.set_carried(8);
        assert_eq!(hotbar.selected, 8);
        hotbar.set_carried(9);
        assert_eq!(hotbar.selected, 8, "hors borne ignoré");
        hotbar.set_carried(-1);
        assert_eq!(hotbar.selected, 8);

        hotbar.set_creative_slot(
            GameMode::Creative,
            36,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        assert_eq!(
            hotbar.slots[0],
            Some(ItemSlot {
                item: DIRT_ITEM,
                count: 1
            })
        );

        hotbar.set_creative_slot(GameMode::Creative, 36, ItemStack::Empty);
        assert_eq!(hotbar.slots[0], None);

        hotbar.set_creative_slot(
            GameMode::Survival,
            37,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        assert_eq!(hotbar.slots[1], None, "hors créatif ignoré");

        hotbar.set_creative_slot(
            GameMode::Creative,
            45,
            ItemStack::Item {
                item: DIRT_ITEM,
                count: 1,
            },
        );
        assert!(
            hotbar.slots.iter().filter(|slot| slot.is_some()).count() == 0,
            "slot hors 36..=44 ignoré"
        );
    }
}
