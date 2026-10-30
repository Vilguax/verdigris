use verdigris_protocol::{BlockPos, Direction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RayHit {
    pub pos: BlockPos,
    pub face: Option<Direction>,
}

pub fn look_direction(yaw: f32, pitch: f32) -> [f64; 3] {
    let yaw = (yaw as f64).to_radians();
    let pitch = (pitch as f64).to_radians();
    let cos_pitch = pitch.cos();
    [-yaw.sin() * cos_pitch, -pitch.sin(), yaw.cos() * cos_pitch]
}

struct Axis {
    step: i32,
    t_max: f64,
    t_delta: f64,
    enter_pos: Direction,
    enter_neg: Direction,
}

fn axis(origin: f64, voxel: i32, dir: f64, pos_face: Direction, neg_face: Direction) -> Axis {
    if dir > 0.0 {
        Axis {
            step: 1,
            t_max: (voxel as f64 + 1.0 - origin) / dir,
            t_delta: 1.0 / dir,
            enter_pos: pos_face,
            enter_neg: neg_face,
        }
    } else if dir < 0.0 {
        Axis {
            step: -1,
            t_max: (voxel as f64 - origin) / dir,
            t_delta: 1.0 / -dir,
            enter_pos: pos_face,
            enter_neg: neg_face,
        }
    } else {
        Axis {
            step: 0,
            t_max: f64::INFINITY,
            t_delta: f64::INFINITY,
            enter_pos: pos_face,
            enter_neg: neg_face,
        }
    }
}

pub fn cast_ray(
    origin: [f64; 3],
    dir: [f64; 3],
    max_dist: f64,
    read: impl Fn(BlockPos) -> i32,
    stop: impl Fn(i32) -> bool,
) -> Option<RayHit> {
    let [ox, oy, oz] = origin;
    let [dx, dy, dz] = dir;
    let norm = (dx * dx + dy * dy + dz * dz).sqrt();
    if !norm.is_finite() || norm < 1e-12 || !max_dist.is_finite() {
        return None;
    }
    let (dx, dy, dz) = (dx / norm, dy / norm, dz / norm);

    let mut ix = ox.floor() as i32;
    let mut iy = oy.floor() as i32;
    let mut iz = oz.floor() as i32;

    let mut ax = axis(ox, ix, dx, Direction::West, Direction::East);
    let mut ay = axis(oy, iy, dy, Direction::Down, Direction::Up);
    let mut az = axis(oz, iz, dz, Direction::North, Direction::South);

    let max_steps = (3.0 * max_dist.ceil() + 3.0) as i32;
    let mut face = None;

    for _ in 0..max_steps {
        let pos = BlockPos {
            x: ix,
            y: iy,
            z: iz,
        };
        if stop(read(pos)) {
            return Some(RayHit { pos, face });
        }
        if ax.t_max <= ay.t_max && ax.t_max <= az.t_max {
            if ax.t_max > max_dist {
                return None;
            }
            ix += ax.step;
            ax.t_max += ax.t_delta;
            face = Some(if ax.step > 0 {
                ax.enter_pos
            } else {
                ax.enter_neg
            });
        } else if ay.t_max <= az.t_max {
            if ay.t_max > max_dist {
                return None;
            }
            iy += ay.step;
            ay.t_max += ay.t_delta;
            face = Some(if ay.step > 0 {
                ay.enter_pos
            } else {
                ay.enter_neg
            });
        } else {
            if az.t_max > max_dist {
                return None;
            }
            iz += az.step;
            az.t_max += az.t_delta;
            face = Some(if az.step > 0 {
                az.enter_pos
            } else {
                az.enter_neg
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOLID: i32 = 1;
    const AIR: i32 = 0;

    fn single_block(target: BlockPos) -> impl Fn(BlockPos) -> i32 {
        move |p| if p == target { SOLID } else { AIR }
    }

    fn stop_solid(state: i32) -> bool {
        state != AIR
    }

    #[test]
    fn straight_down_hits_block_below_from_above() {
        let target = BlockPos { x: 0, y: -61, z: 0 };
        let hit = cast_ray(
            [0.5, -58.38, 0.5],
            look_direction(0.0, 90.0),
            5.0,
            single_block(target),
            stop_solid,
        )
        .unwrap();
        assert_eq!(hit.pos, target);
        assert_eq!(hit.face, Some(Direction::Up));
    }

    #[test]
    fn diagonal_hit_reports_entry_face() {
        let target = BlockPos { x: 2, y: 0, z: 0 };
        let hit = cast_ray(
            [0.5, 0.5, 0.5],
            look_direction(-90.0, 0.0),
            6.0,
            single_block(target),
            stop_solid,
        )
        .unwrap();
        assert_eq!(hit.pos, target);
        assert_eq!(
            hit.face,
            Some(Direction::West),
            "entrée par l'ouest vers l'est"
        );
    }

    #[test]
    fn misses_when_nothing_in_range() {
        let target = BlockPos {
            x: 0,
            y: -200,
            z: 0,
        };
        let hit = cast_ray(
            [0.5, -58.38, 0.5],
            look_direction(0.0, 90.0),
            5.0,
            single_block(target),
            stop_solid,
        );
        assert_eq!(hit, None, "cible hors portée: aucun impact");
    }

    #[test]
    fn non_finite_direction_yields_no_hit() {
        assert_eq!(
            cast_ray(
                [0.5, 0.5, 0.5],
                [f64::NAN, 0.0, 0.0],
                5.0,
                |_| SOLID,
                stop_solid,
            ),
            None
        );
    }
}
