use crate::clientbound::{
    entity_position_sync, move_entity_pos, move_entity_pos_rot, move_entity_rot, rotate_head,
};
use crate::primitives::{fits_i16, pack_degrees, quantize};

pub const POSITION_EPSILON_SQ: f64 = 7.6293945e-6;
pub const FORCE_SYNC_TICKS: i32 = 60;
pub const MAX_TELEPORT_DELAY: i32 = 400;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntitySnapshot {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    pub on_ground: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackedState {
    entity_id: i32,
    base_q: [i64; 3],
    last_yrot: i8,
    last_xrot: i8,
    last_head: i8,
    was_on_ground: bool,
    tick_count: i32,
    teleport_delay: i32,
}

impl TrackedState {
    pub fn new(entity_id: i32, s: &EntitySnapshot) -> Self {
        TrackedState {
            entity_id,
            base_q: [quantize(s.x), quantize(s.y), quantize(s.z)],
            last_yrot: pack_degrees(s.yaw) as i8,
            last_xrot: pack_degrees(s.pitch) as i8,
            last_head: pack_degrees(s.head_yaw) as i8,
            was_on_ground: s.on_ground,
            tick_count: 0,
            teleport_delay: 0,
        }
    }

    pub fn entity_id(&self) -> i32 {
        self.entity_id
    }

    pub fn on_tick(&mut self, cur: &EntitySnapshot, out: &mut Vec<u8>) -> bool {
        if self.tick_count % 2 != 0 {
            self.tick_count += 1;
            return false;
        }
        self.run_pass(cur, out);
        self.tick_count += 1;
        !out.is_empty()
    }

    fn run_pass(&mut self, cur: &EntitySnapshot, out: &mut Vec<u8>) {
        let yrotn = pack_degrees(cur.yaw) as i8;
        let xrotn = pack_degrees(cur.pitch) as i8;
        let rot_changed = (i32::from(yrotn) - i32::from(self.last_yrot)).abs() >= 1
            || (i32::from(xrotn) - i32::from(self.last_xrot)).abs() >= 1;

        self.teleport_delay += 1;

        let qx = quantize(cur.x);
        let qy = quantize(cur.y);
        let qz = quantize(cur.z);

        let dx = cur.x - self.base_q[0] as f64 / 4096.0;
        let dy = cur.y - self.base_q[1] as f64 / 4096.0;
        let dz = cur.z - self.base_q[2] as f64 / 4096.0;
        let dist2 = dx * dx + dy * dy + dz * dz;
        let pos_changed = dist2 >= POSITION_EPSILON_SQ;
        let send_pos = pos_changed || self.tick_count % FORCE_SYNC_TICKS == 0;

        let dqx = qx - self.base_q[0];
        let dqy = qy - self.base_q[1];
        let dqz = qz - self.base_q[2];
        let delta_fits = fits_i16(dqx) && fits_i16(dqz) && fits_i16(dqy);

        let force_sync = self.teleport_delay > MAX_TELEPORT_DELAY
            || cur.on_ground != self.was_on_ground
            || (send_pos && !delta_fits);

        if force_sync {
            entity_position_sync(
                self.entity_id,
                cur.x,
                cur.y,
                cur.z,
                cur.yaw,
                cur.pitch,
                cur.on_ground,
                out,
            );
            self.reset_base(qx, qy, qz, yrotn, xrotn, cur.on_ground);
        } else if send_pos && rot_changed {
            move_entity_pos_rot(
                self.entity_id,
                dqx as i16,
                dqy as i16,
                dqz as i16,
                yrotn as u8,
                xrotn as u8,
                cur.on_ground,
                out,
            );
            self.base_q = [qx, qy, qz];
            self.last_yrot = yrotn;
            self.last_xrot = xrotn;
        } else if send_pos {
            move_entity_pos(
                self.entity_id,
                dqx as i16,
                dqy as i16,
                dqz as i16,
                cur.on_ground,
                out,
            );
            self.base_q = [qx, qy, qz];
        } else if rot_changed {
            move_entity_rot(self.entity_id, cur.on_ground, yrotn as u8, xrotn as u8, out);
            self.last_yrot = yrotn;
            self.last_xrot = xrotn;
        }

        let headn = pack_degrees(cur.head_yaw) as i8;
        if (i32::from(headn) - i32::from(self.last_head)).abs() >= 1 {
            rotate_head(self.entity_id, headn as u8, out);
            self.last_head = headn;
        }
    }

    fn reset_base(&mut self, qx: i64, qy: i64, qz: i64, yrotn: i8, xrotn: i8, on_ground: bool) {
        self.teleport_delay = 0;
        self.was_on_ground = on_ground;
        self.base_q = [qx, qy, qz];
        self.last_yrot = yrotn;
        self.last_xrot = xrotn;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::{
        ENTITY_POSITION_SYNC_ID, MOVE_ENTITY_POS_ID, MOVE_ENTITY_ROT_ID, ROTATE_HEAD_ID,
    };

    fn base_snapshot() -> EntitySnapshot {
        EntitySnapshot {
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
            on_ground: true,
        }
    }

    fn step(state: &mut TrackedState, snap: &EntitySnapshot) -> Vec<u8> {
        let mut out = Vec::new();
        state.on_tick(snap, &mut out);
        out
    }

    #[test]
    fn immobile_sends_zero_delta_pos_on_multiples_of_sixty() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(7, &snap);
        for tick in 0..=120 {
            let out = step(&mut state, &snap);
            if tick % 60 == 0 {
                assert_eq!(
                    out,
                    vec![MOVE_ENTITY_POS_ID as u8, 0x07, 0x01, 0, 0, 0, 0, 0, 0],
                    "tick {tick}"
                );
            } else {
                assert!(out.is_empty(), "tick {tick}");
            }
        }
    }

    #[test]
    fn one_block_x_on_ground_is_move_pos() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(42, &snap);
        let moved = EntitySnapshot { x: 1.0, ..snap };
        let out = step(&mut state, &moved);
        assert_eq!(
            out,
            vec![
                MOVE_ENTITY_POS_ID as u8,
                0x2A,
                0x01,
                0x10,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00
            ]
        );
    }

    #[test]
    fn rotation_only_is_move_rot() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(42, &snap);
        assert!(!step(&mut state, &snap).is_empty());
        assert!(step(&mut state, &snap).is_empty());
        let rotated = EntitySnapshot {
            yaw: 90.0,
            head_yaw: 0.0,
            ..snap
        };
        let out = step(&mut state, &rotated);
        assert_eq!(out, vec![MOVE_ENTITY_ROT_ID as u8, 0x2A, 0x01, 0x40, 0x00]);
    }

    #[test]
    fn delta_over_i16_falls_back_to_sync() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(42, &snap);
        let moved = EntitySnapshot { x: 9.0, ..snap };
        let out = step(&mut state, &moved);
        assert_eq!(out[0], ENTITY_POSITION_SYNC_ID as u8);
    }

    #[test]
    fn on_ground_change_forces_sync() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(42, &snap);
        assert!(!step(&mut state, &snap).is_empty());
        assert!(step(&mut state, &snap).is_empty());
        let airborne = EntitySnapshot {
            on_ground: false,
            ..snap
        };
        let out = step(&mut state, &airborne);
        assert_eq!(out[0], ENTITY_POSITION_SYNC_ID as u8);
    }

    #[test]
    fn odd_tick_without_change_emits_nothing_but_counts() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(42, &snap);
        assert!(!step(&mut state, &snap).is_empty());
        assert!(step(&mut state, &snap).is_empty());
        assert_eq!(state.tick_count, 2);
    }

    #[test]
    fn four_hundred_and_one_passes_force_sync() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(42, &snap);
        let tiny = EntitySnapshot { x: 0.001, ..snap };
        for _ in 0..800 {
            step(&mut state, &tiny);
        }
        let out = step(&mut state, &tiny);
        assert_eq!(out[0], ENTITY_POSITION_SYNC_ID as u8);
        assert_eq!(state.teleport_delay, 0);
    }

    #[test]
    fn head_yaw_change_sends_rotate_head_after_movement() {
        let snap = base_snapshot();
        let mut state = TrackedState::new(42, &snap);
        let moved = EntitySnapshot {
            x: 1.0,
            head_yaw: 90.0,
            ..snap
        };
        let out = step(&mut state, &moved);
        assert_eq!(out[0], MOVE_ENTITY_POS_ID as u8);
        let head_pos = out.iter().position(|&b| b == ROTATE_HEAD_ID as u8);
        assert!(head_pos.is_some());
        let idx = head_pos.unwrap();
        assert_eq!(&out[idx..], &[ROTATE_HEAD_ID as u8, 0x2A, 0x40]);
    }
}
