use std::sync::OnceLock;

use crate::blocks::{block_id_of_name, default_state_of_name, property_of, state_to_block};

pub const RING_SIZE: usize = 12;
pub const INTERIOR_SIZE: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingSlot {
    pub dx: i32,
    pub dz: i32,
    pub facing: &'static str,
}

pub const RING_TEMPLATE: [RingSlot; RING_SIZE] = [
    RingSlot {
        dx: 0,
        dz: -1,
        facing: "south",
    },
    RingSlot {
        dx: 1,
        dz: -1,
        facing: "south",
    },
    RingSlot {
        dx: 2,
        dz: -1,
        facing: "south",
    },
    RingSlot {
        dx: 0,
        dz: 3,
        facing: "north",
    },
    RingSlot {
        dx: 1,
        dz: 3,
        facing: "north",
    },
    RingSlot {
        dx: 2,
        dz: 3,
        facing: "north",
    },
    RingSlot {
        dx: -1,
        dz: 0,
        facing: "east",
    },
    RingSlot {
        dx: -1,
        dz: 1,
        facing: "east",
    },
    RingSlot {
        dx: -1,
        dz: 2,
        facing: "east",
    },
    RingSlot {
        dx: 3,
        dz: 0,
        facing: "west",
    },
    RingSlot {
        dx: 3,
        dz: 1,
        facing: "west",
    },
    RingSlot {
        dx: 3,
        dz: 2,
        facing: "west",
    },
];

pub const INTERIOR_OFFSETS: [(i32, i32); INTERIOR_SIZE] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (0, 1),
    (1, 1),
    (2, 1),
    (0, 2),
    (1, 2),
    (2, 2),
];

struct Tables {
    frame_block_id: i32,
    end_portal_state: i32,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
}

fn build_tables() -> Tables {
    let frame_block_id = block_id_of_name("minecraft:end_portal_frame").unwrap_or(-1);
    let end_portal_state = default_state_of_name("minecraft:end_portal").unwrap_or(-1);
    Tables {
        frame_block_id,
        end_portal_state,
    }
}

pub fn frame_block_id() -> i32 {
    tables().frame_block_id
}

pub fn end_portal_state() -> i32 {
    tables().end_portal_state
}

pub fn is_filled_frame(state: i32) -> bool {
    state_to_block(state).id == frame_block_id() && property_of(state, "eye") == Some("true")
}

pub fn is_frame_facing(state: i32, expected_facing: &str) -> bool {
    is_filled_frame(state) && property_of(state, "facing") == Some(expected_facing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::state_with;

    fn frame(eye: &str, facing: &str) -> i32 {
        let id = block_id_of_name("minecraft:end_portal_frame").unwrap();
        state_with(id, &[("eye", eye), ("facing", facing)]).unwrap()
    }

    #[test]
    fn ids_resolve_from_registry() {
        assert_eq!(
            frame_block_id(),
            block_id_of_name("minecraft:end_portal_frame").unwrap()
        );
        assert_eq!(
            end_portal_state(),
            default_state_of_name("minecraft:end_portal").unwrap()
        );
    }

    #[test]
    fn filled_frame_requires_eye() {
        assert!(is_filled_frame(frame("true", "south")));
        assert!(!is_filled_frame(frame("false", "south")));
        assert!(!is_filled_frame(
            default_state_of_name("minecraft:stone").unwrap()
        ));
    }

    #[test]
    fn frame_facing_matches_property() {
        assert!(is_frame_facing(frame("true", "east"), "east"));
        assert!(!is_frame_facing(frame("true", "east"), "west"));
        assert!(!is_frame_facing(frame("false", "east"), "east"));
    }

    #[test]
    fn ring_template_has_twelve_inward_frames() {
        assert_eq!(RING_TEMPLATE.len(), RING_SIZE);
        let corners = [(-1, -1), (3, -1), (-1, 3), (3, 3)];
        for slot in RING_TEMPLATE {
            assert!(!corners.contains(&(slot.dx, slot.dz)), "pas de coin");
        }
        assert_eq!(INTERIOR_OFFSETS.len(), INTERIOR_SIZE);
    }
}
