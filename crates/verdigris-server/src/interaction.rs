use verdigris_protocol::{
    BlockPos, Direction, GameMode, InteractionKind, ender_eye_item, interaction_kind, property_of,
    state_to_block, with_property,
};

const ZERO: BlockPos = BlockPos { x: 0, y: 0, z: 0 };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractionOutcome {
    changes: [(BlockPos, i32); 2],
    len: usize,
    consume_eye: bool,
}

impl InteractionOutcome {
    fn single(state: i32, consume_eye: bool) -> Self {
        InteractionOutcome {
            changes: [(ZERO, state), (ZERO, state)],
            len: 1,
            consume_eye,
        }
    }

    fn pair(primary: i32, other_offset: BlockPos, other: i32) -> Self {
        InteractionOutcome {
            changes: [(ZERO, primary), (other_offset, other)],
            len: 2,
            consume_eye: false,
        }
    }

    pub fn changes(&self) -> &[(BlockPos, i32)] {
        &self.changes[..self.len]
    }

    pub fn consume_eye(&self) -> bool {
        self.consume_eye
    }
}

fn facing_to_direction(value: &str) -> Option<Direction> {
    match value {
        "north" => Some(Direction::North),
        "south" => Some(Direction::South),
        "west" => Some(Direction::West),
        "east" => Some(Direction::East),
        _ => None,
    }
}

fn toggle(state: i32, name: &str) -> Option<i32> {
    let current = property_of(state, name)?;
    let next = if current == "true" { "false" } else { "true" };
    with_property(state, name, next)
}

fn toggle_single(state: i32, name: &str) -> Option<InteractionOutcome> {
    Some(InteractionOutcome::single(toggle(state, name)?, false))
}

fn wooden_door(state_at_p: i32, other_half_state: Option<i32>) -> Option<InteractionOutcome> {
    let other = other_half_state?;
    let offset = match property_of(state_at_p, "half")? {
        "lower" => BlockPos { x: 0, y: 1, z: 0 },
        "upper" => BlockPos { x: 0, y: -1, z: 0 },
        _ => return None,
    };
    let target = if property_of(state_at_p, "open")? == "true" {
        "false"
    } else {
        "true"
    };
    let new_p = with_property(state_at_p, "open", target)?;
    let new_other = with_property(other, "open", target)?;
    Some(InteractionOutcome::pair(new_p, offset, new_other))
}

fn fence_gate(state: i32, player_facing: Direction) -> Option<InteractionOutcome> {
    let next = if property_of(state, "open")? == "true" {
        with_property(state, "open", "false")?
    } else {
        let gate = facing_to_direction(property_of(state, "facing")?)?;
        let turned = if gate == player_facing.opposite() {
            with_property(state, "facing", player_facing.facing_str())?
        } else {
            state
        };
        with_property(turned, "open", "true")?
    };
    Some(InteractionOutcome::single(next, false))
}

fn end_portal_frame(
    state: i32,
    gamemode: GameMode,
    held_item: Option<i32>,
) -> Option<InteractionOutcome> {
    if property_of(state, "eye")? == "true" {
        return None;
    }
    if held_item != Some(ender_eye_item()) {
        return None;
    }
    let next = with_property(state, "eye", "true")?;
    Some(InteractionOutcome::single(
        next,
        gamemode == GameMode::Survival,
    ))
}

pub fn resolve_interaction(
    state_at_p: i32,
    other_half_state: Option<i32>,
    player_facing: Direction,
    gamemode: GameMode,
    held_item: Option<i32>,
) -> Option<InteractionOutcome> {
    let block_id = state_to_block(state_at_p).id;
    match interaction_kind(block_id)? {
        InteractionKind::WoodenDoor => wooden_door(state_at_p, other_half_state),
        InteractionKind::WoodenTrapdoor => toggle_single(state_at_p, "open"),
        InteractionKind::FenceGate => fence_gate(state_at_p, player_facing),
        InteractionKind::Lever => toggle_single(state_at_p, "powered"),
        InteractionKind::EndPortalFrame => end_portal_frame(state_at_p, gamemode, held_item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verdigris_protocol::{block_id_of_name, default_state_of_name, state_with};

    fn state(name: &str, props: &[(&str, &str)]) -> i32 {
        let id = block_id_of_name(name).unwrap();
        state_with(id, props).unwrap()
    }

    #[test]
    fn wooden_trapdoor_toggles_open() {
        let closed = state("minecraft:oak_trapdoor", &[("open", "false")]);
        let outcome = resolve_interaction(closed, None, Direction::South, GameMode::Creative, None)
            .expect("trappe interactive");
        let changes = outcome.changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, ZERO);
        assert_eq!(property_of(changes[0].1, "open"), Some("true"));
        assert!(!outcome.consume_eye());

        let open = state("minecraft:oak_trapdoor", &[("open", "true")]);
        let outcome =
            resolve_interaction(open, None, Direction::South, GameMode::Creative, None).unwrap();
        assert_eq!(property_of(outcome.changes()[0].1, "open"), Some("false"));
    }

    #[test]
    fn lever_toggles_powered() {
        let off = state("minecraft:lever", &[("powered", "false")]);
        let outcome =
            resolve_interaction(off, None, Direction::South, GameMode::Survival, None).unwrap();
        assert_eq!(property_of(outcome.changes()[0].1, "powered"), Some("true"));
        assert!(!outcome.consume_eye());
    }

    #[test]
    fn wooden_door_toggles_both_halves_consistently() {
        let lower = state(
            "minecraft:oak_door",
            &[("half", "lower"), ("open", "false")],
        );
        let upper = state(
            "minecraft:oak_door",
            &[("half", "upper"), ("open", "false")],
        );
        let outcome = resolve_interaction(
            lower,
            Some(upper),
            Direction::South,
            GameMode::Creative,
            None,
        )
        .expect("porte interactive");
        let changes = outcome.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].0, ZERO);
        assert_eq!(changes[1].0, BlockPos { x: 0, y: 1, z: 0 });
        assert_eq!(property_of(changes[0].1, "open"), Some("true"));
        assert_eq!(property_of(changes[1].1, "open"), Some("true"));
        assert_eq!(property_of(changes[1].1, "half"), Some("upper"));
    }

    #[test]
    fn wooden_door_from_upper_targets_lower_below() {
        let upper = state("minecraft:oak_door", &[("half", "upper"), ("open", "true")]);
        let lower = state("minecraft:oak_door", &[("half", "lower"), ("open", "true")]);
        let outcome = resolve_interaction(
            upper,
            Some(lower),
            Direction::North,
            GameMode::Creative,
            None,
        )
        .unwrap();
        let changes = outcome.changes();
        assert_eq!(changes[1].0, BlockPos { x: 0, y: -1, z: 0 });
        assert_eq!(property_of(changes[0].1, "open"), Some("false"));
        assert_eq!(property_of(changes[1].1, "open"), Some("false"));
    }

    #[test]
    fn wooden_door_without_other_half_is_none() {
        let lower = state(
            "minecraft:oak_door",
            &[("half", "lower"), ("open", "false")],
        );
        assert_eq!(
            resolve_interaction(lower, None, Direction::South, GameMode::Creative, None),
            None
        );
    }

    #[test]
    fn fence_gate_closes_when_open() {
        let open = state(
            "minecraft:oak_fence_gate",
            &[("facing", "north"), ("open", "true")],
        );
        let outcome =
            resolve_interaction(open, None, Direction::South, GameMode::Creative, None).unwrap();
        assert_eq!(property_of(outcome.changes()[0].1, "open"), Some("false"));
        assert_eq!(property_of(outcome.changes()[0].1, "facing"), Some("north"));
    }

    #[test]
    fn fence_gate_turns_to_player_when_facing_away() {
        let closed = state(
            "minecraft:oak_fence_gate",
            &[("facing", "north"), ("open", "false")],
        );
        let outcome =
            resolve_interaction(closed, None, Direction::South, GameMode::Creative, None).unwrap();
        let s = outcome.changes()[0].1;
        assert_eq!(property_of(s, "open"), Some("true"));
        assert_eq!(property_of(s, "facing"), Some("south"));
    }

    #[test]
    fn fence_gate_keeps_facing_when_aligned() {
        let closed = state(
            "minecraft:oak_fence_gate",
            &[("facing", "east"), ("open", "false")],
        );
        let outcome =
            resolve_interaction(closed, None, Direction::South, GameMode::Creative, None).unwrap();
        let s = outcome.changes()[0].1;
        assert_eq!(property_of(s, "open"), Some("true"));
        assert_eq!(property_of(s, "facing"), Some("east"));
    }

    #[test]
    fn end_portal_frame_sets_eye_and_consumes_in_survival() {
        let empty = state("minecraft:end_portal_frame", &[("eye", "false")]);
        let eye = ender_eye_item();
        let outcome =
            resolve_interaction(empty, None, Direction::South, GameMode::Survival, Some(eye))
                .unwrap();
        assert_eq!(property_of(outcome.changes()[0].1, "eye"), Some("true"));
        assert!(outcome.consume_eye());
    }

    #[test]
    fn end_portal_frame_does_not_consume_in_creative() {
        let empty = state("minecraft:end_portal_frame", &[("eye", "false")]);
        let outcome = resolve_interaction(
            empty,
            None,
            Direction::South,
            GameMode::Creative,
            Some(ender_eye_item()),
        )
        .unwrap();
        assert!(!outcome.consume_eye());
    }

    #[test]
    fn end_portal_frame_needs_eye_item() {
        let empty = state("minecraft:end_portal_frame", &[("eye", "false")]);
        assert_eq!(
            resolve_interaction(empty, None, Direction::South, GameMode::Survival, None),
            None
        );
        assert_eq!(
            resolve_interaction(
                empty,
                None,
                Direction::South,
                GameMode::Survival,
                Some(ender_eye_item() + 1)
            ),
            None
        );
    }

    #[test]
    fn end_portal_frame_with_eye_present_is_none() {
        let filled = state("minecraft:end_portal_frame", &[("eye", "true")]);
        assert_eq!(
            resolve_interaction(
                filled,
                None,
                Direction::South,
                GameMode::Survival,
                Some(ender_eye_item())
            ),
            None
        );
    }

    #[test]
    fn iron_door_has_no_interaction() {
        let lower = state(
            "minecraft:iron_door",
            &[("half", "lower"), ("open", "false")],
        );
        let upper = state(
            "minecraft:iron_door",
            &[("half", "upper"), ("open", "false")],
        );
        assert_eq!(
            resolve_interaction(
                lower,
                Some(upper),
                Direction::South,
                GameMode::Creative,
                None
            ),
            None
        );
    }

    #[test]
    fn plain_block_has_no_interaction() {
        let stone = default_state_of_name("minecraft:stone").unwrap();
        assert_eq!(
            resolve_interaction(stone, None, Direction::South, GameMode::Creative, None),
            None
        );
    }
}
