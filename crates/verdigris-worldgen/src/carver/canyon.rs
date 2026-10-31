use std::f32::consts::{PI, TAU};

use crate::carver::config::CanyonConfig;
use crate::carver::geom::{can_reach, canyon_should_carve, ellipsoid, width_factor_index};
use crate::carver::mask::{CARVE_GEN_MIN_Y, CARVE_GEN_TOP_Y, CarveMask};
use crate::carver::sincos::{SinTable, sin_table};
use crate::lcg::Lcg;

pub const CANYON_GEN_DEPTH: usize = 384;
pub const CANYON_LENGTH_BASE: f32 = 112.0;
pub const CANYON_VERTICAL_DIVISOR: f64 = 6.0;

pub fn init_width_factors(cfg: &CanyonConfig, r: &mut Lcg) -> Vec<f32> {
    let mut fs = vec![0.0f32; CANYON_GEN_DEPTH];
    let mut f = 1.0f32;
    for (j, slot) in fs.iter_mut().enumerate() {
        if j == 0 || r.next_int_bounded(cfg.width_smoothness) == 0 {
            f = 1.0 + r.next_float() * r.next_float();
        }
        *slot = f * f;
    }
    fs
}

#[allow(clippy::too_many_arguments)]
pub fn carve_canyon(
    mask: &mut CarveMask,
    cfg: &CanyonConfig,
    r: &mut Lcg,
    sx: i32,
    sz: i32,
    cx: i32,
    cz: i32,
) {
    let t = sin_table();
    let x = f64::from(16i32.wrapping_mul(sx).wrapping_add(r.next_int_bounded(16)));
    let y = f64::from(cfg.y.sample(r, CARVE_GEN_MIN_Y, CARVE_GEN_TOP_Y));
    let z = f64::from(16i32.wrapping_mul(sz).wrapping_add(r.next_int_bounded(16)));
    let yaw = r.next_float() * TAU;
    let pitch = cfg.vertical_rotation.sample(r);
    let width = cfg.thickness.sample(r);
    let df = cfg.distance_factor.sample(r);
    let length = (CANYON_LENGTH_BASE * df) as i32;
    let seed = r.next_long();
    do_carve(
        mask, t, cfg, seed, cx, cz, x, y, z, yaw, pitch, width, length,
    );
}

#[allow(clippy::too_many_arguments)]
fn do_carve(
    mask: &mut CarveMask,
    t: &SinTable,
    cfg: &CanyonConfig,
    seed: i64,
    cx: i32,
    cz: i32,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    mut yaw: f32,
    mut pitch: f32,
    width: f32,
    length: i32,
) {
    let mut r = Lcg::from_seed(seed);
    let fs = init_width_factors(cfg, &mut r);
    let e = f64::from(cfg.y_scale);
    let default_factor = cfg.vertical_radius_default_factor;
    let center_factor = cfg.vertical_radius_center_factor;
    let mut dyaw = 0.0f32;
    let mut dpitch = 0.0f32;

    let mut s = 0i32;
    while s < length {
        let base = 1.5 + f64::from(t.sin_t(s as f32 * PI / length as f32) * width);
        let mut rv = base * e;
        let hrf = cfg.horizontal_radius_factor.sample(&mut r);
        let rh = base * f64::from(hrf);
        let frac = s as f32 / length as f32;
        let mu = 1.0 - (0.5 - frac).abs() * 2.0;
        let kappa = default_factor + center_factor * mu;
        let u = r.next_float() * 0.25 + 0.75;
        rv = (f64::from(kappa) * rv) * f64::from(u);

        let cos_p = t.cos_t(pitch);
        x += f64::from(t.cos_t(yaw) * cos_p);
        y += f64::from(t.sin_t(pitch));
        z += f64::from(t.sin_t(yaw) * cos_p);
        pitch *= 0.7;
        pitch += dpitch * 0.05;
        yaw += dyaw * 0.05;
        dpitch *= 0.8;
        dyaw *= 0.5;
        dpitch += (r.next_float() - r.next_float()) * r.next_float() * 2.0;
        dyaw += (r.next_float() - r.next_float()) * r.next_float() * 4.0;

        if r.next_int_bounded(4) != 0 {
            if !can_reach(cx, cz, x, z, s, length, width) {
                return;
            }
            ellipsoid(mask, cx, cz, x, y, z, rh, rv, |g, d2, h, yy| {
                let fl = fs[width_factor_index(yy)];
                canyon_should_carve(g, d2, h, fl, CANYON_VERTICAL_DIVISOR)
            });
        }
        s += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carver::config::{CanyonConfig, FloatProvider, HeightProvider, VerticalAnchor};
    use crate::carver::rng::seed_large_feature;

    fn cfg() -> CanyonConfig {
        CanyonConfig {
            probability: 0.01,
            y: HeightProvider {
                min: VerticalAnchor::Absolute(10),
                max: VerticalAnchor::Absolute(67),
            },
            vertical_rotation: FloatProvider::Uniform {
                min: -0.125,
                max: 0.125,
            },
            thickness: FloatProvider::Trapezoid {
                min: 0.0,
                max: 6.0,
                plateau: 2.0,
            },
            width_smoothness: 3,
            horizontal_radius_factor: FloatProvider::Uniform {
                min: 0.75,
                max: 1.0,
            },
            vertical_radius_default_factor: 1.0,
            vertical_radius_center_factor: 0.0,
            distance_factor: FloatProvider::Uniform {
                min: 0.75,
                max: 1.0,
            },
            y_scale: 3.0,
        }
    }

    #[test]
    fn width_factors_length_is_gen_depth() {
        let mut r = Lcg::from_seed(1);
        let fs = init_width_factors(&cfg(), &mut r);
        assert_eq!(fs.len(), CANYON_GEN_DEPTH);
        assert!(fs[0] >= 1.0);
    }

    #[test]
    fn carve_is_deterministic() {
        let c = cfg();
        let mut m1 = CarveMask::new();
        let mut r1 = seed_large_feature(20_261_029, -5, -8);
        carve_canyon(&mut m1, &c, &mut r1, -5, -8, -6, -8);
        let mut m2 = CarveMask::new();
        let mut r2 = seed_large_feature(20_261_029, -5, -8);
        carve_canyon(&mut m2, &c, &mut r2, -5, -8, -6, -8);
        assert_eq!(m1.count(), m2.count());
    }
}
