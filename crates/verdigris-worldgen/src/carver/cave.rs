use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::carver::config::CaveConfig;
use crate::carver::geom::{can_reach, cave_should_carve, ellipsoid};
use crate::carver::mask::{CARVE_GEN_MIN_Y, CARVE_GEN_TOP_Y, CarveMask};
use crate::carver::sincos::{SinTable, sin_table};

use crate::lcg::Lcg;

pub const CAVE_RANGE: i32 = 4;

pub fn tunnel_bound() -> i32 {
    (CAVE_RANGE * 2 - 1) * 16
}

fn get_thickness(cfg: &CaveConfig, r: &mut Lcg) -> f32 {
    let mut f = cfg.thickness.sample(r);
    if cfg.weird_thickness_bias && r.next_int_bounded(10) == 0 {
        f *= r.next_float() * r.next_float() * 3.0 + 1.0;
    }
    f
}

#[allow(clippy::too_many_arguments)]
pub fn carve_cave(
    mask: &mut CarveMask,
    cfg: &CaveConfig,
    r: &mut Lcg,
    sx: i32,
    sz: i32,
    cx: i32,
    cz: i32,
) {
    let t = sin_table();
    let bound = tunnel_bound();
    let count = cfg.count.sample(r);
    for _ in 0..count {
        let x = f64::from(16i32.wrapping_mul(sx).wrapping_add(r.next_int_bounded(16)));
        let y = f64::from(cfg.y.sample(r, CARVE_GEN_MIN_Y, CARVE_GEN_TOP_Y));
        let z = f64::from(16i32.wrapping_mul(sz).wrapping_add(r.next_int_bounded(16)));
        let hmul = f64::from(cfg.horizontal_radius_multiplier.sample(r));
        let vmul = f64::from(cfg.vertical_radius_multiplier.sample(r));
        let floor = f64::from(cfg.floor_level.sample(r));

        let mut branches = 1i32;
        if r.next_int_bounded(4) == 0 {
            let room_mul = f64::from(cfg.room_vertical_radius_multiplier.sample(r));
            let width = 1.0 + r.next_float() * 6.0;
            create_room(mask, t, cx, cz, x, y, z, width, room_mul, floor);
            branches += r.next_int_bounded(4);
        }

        for _ in 0..branches {
            let yaw = r.next_float() * TAU;
            let pitch = (r.next_float() - 0.5) / 4.0;
            let thickness = get_thickness(cfg, r);
            let length = bound - r.next_int_bounded(bound / 4);
            let seed = r.next_long();
            create_tunnel(
                mask,
                t,
                seed,
                cx,
                cz,
                x,
                y,
                z,
                hmul,
                vmul,
                thickness,
                yaw,
                pitch,
                0,
                length,
                f64::from(cfg.start_vertical_radius_multiplier),
                floor,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_room(
    mask: &mut CarveMask,
    t: &SinTable,
    cx: i32,
    cz: i32,
    x: f64,
    y: f64,
    z: f64,
    width: f32,
    room_mul: f64,
    floor: f64,
) {
    let d = 1.5 + f64::from(t.sin_t(FRAC_PI_2) * width);
    let e = d * room_mul;
    ellipsoid(mask, cx, cz, x + 1.0, y, z, d, e, |g, d2, h, _| {
        cave_should_carve(g, d2, h, floor)
    });
}

#[allow(clippy::too_many_arguments)]
fn create_tunnel(
    mask: &mut CarveMask,
    t: &SinTable,
    seed: i64,
    cx: i32,
    cz: i32,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    hmul: f64,
    vmul: f64,
    thickness: f32,
    mut yaw: f32,
    mut pitch: f32,
    branch_index: i32,
    branch_count: i32,
    vertical_ratio: f64,
    floor: f64,
) {
    if branch_count <= 0 {
        return;
    }
    let mut r = Lcg::from_seed(seed);
    let branch_at = r.next_int_bounded(branch_count / 2) + branch_count / 4;
    let wide = r.next_int_bounded(6) == 0;
    let mut dyaw = 0.0f32;
    let mut dpitch = 0.0f32;

    let mut j = branch_index;
    while j < branch_count {
        let d = 1.5 + f64::from(t.sin_t(PI * j as f32 / branch_count as f32) * thickness);
        let e = d * vertical_ratio;
        let cos_p = t.cos_t(pitch);
        x += f64::from(t.cos_t(yaw) * cos_p);
        y += f64::from(t.sin_t(pitch));
        z += f64::from(t.sin_t(yaw) * cos_p);
        pitch *= if wide { 0.92 } else { 0.7 };
        pitch += dpitch * 0.1;
        yaw += dyaw * 0.1;
        dpitch *= 0.9;
        dyaw *= 0.75;
        dpitch += (r.next_float() - r.next_float()) * r.next_float() * 2.0;
        dyaw += (r.next_float() - r.next_float()) * r.next_float() * 4.0;

        if j == branch_at && thickness > 1.0 {
            let seed1 = r.next_long();
            let thick1 = r.next_float() * 0.5 + 0.5;
            create_tunnel(
                mask,
                t,
                seed1,
                cx,
                cz,
                x,
                y,
                z,
                hmul,
                vmul,
                thick1,
                yaw - FRAC_PI_2,
                pitch / 3.0,
                j,
                branch_count,
                1.0,
                floor,
            );
            let seed2 = r.next_long();
            let thick2 = r.next_float() * 0.5 + 0.5;
            create_tunnel(
                mask,
                t,
                seed2,
                cx,
                cz,
                x,
                y,
                z,
                hmul,
                vmul,
                thick2,
                yaw + FRAC_PI_2,
                pitch / 3.0,
                j,
                branch_count,
                1.0,
                floor,
            );
            return;
        }

        if r.next_int_bounded(4) == 0 {
            j += 1;
            continue;
        }
        if !can_reach(cx, cz, x, z, j, branch_count, thickness) {
            return;
        }
        ellipsoid(mask, cx, cz, x, y, z, d * hmul, e * vmul, |g, d2, h, _| {
            cave_should_carve(g, d2, h, floor)
        });
        j += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carver::rng::seed_large_feature;

    #[test]
    fn tunnel_bound_is_112() {
        assert_eq!(tunnel_bound(), 112);
    }

    #[test]
    fn carve_is_deterministic_for_same_source() {
        let cfg = CaveConfig {
            probability: 0.15,
            count: crate::carver::config::IntProvider::Constant(2),
            y: crate::carver::config::HeightProvider {
                min: crate::carver::config::VerticalAnchor::AboveBottom(8),
                max: crate::carver::config::VerticalAnchor::Absolute(180),
            },
            horizontal_radius_multiplier: crate::carver::config::FloatProvider::Uniform {
                min: 0.7,
                max: 1.4,
            },
            vertical_radius_multiplier: crate::carver::config::FloatProvider::Uniform {
                min: 0.8,
                max: 1.3,
            },
            floor_level: crate::carver::config::FloatProvider::Uniform {
                min: -1.0,
                max: -0.4,
            },
            room_vertical_radius_multiplier: crate::carver::config::FloatProvider::Uniform {
                min: 0.1,
                max: 0.9,
            },
            thickness: crate::carver::config::FloatProvider::Trapezoid {
                min: 0.0,
                max: 3.0,
                plateau: 1.0,
            },
            weird_thickness_bias: true,
            start_vertical_radius_multiplier: 1.0,
        };
        let mut m1 = CarveMask::new();
        let mut r1 = seed_large_feature(20_261_029, -10, -5);
        carve_cave(&mut m1, &cfg, &mut r1, -10, -5, -8, -8);
        let mut m2 = CarveMask::new();
        let mut r2 = seed_large_feature(20_261_029, -10, -5);
        carve_cave(&mut m2, &cfg, &mut r2, -10, -5, -8, -8);
        assert_eq!(m1.count(), m2.count());
    }
}
