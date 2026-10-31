use crate::arith::floor_i32;
use crate::carver::mask::{CARVE_MAX_Y, CARVE_MIN_Y, CarveMask};

pub fn middle_block(c: i32) -> i32 {
    16i32.wrapping_mul(c).wrapping_add(8)
}

pub fn can_reach(cx: i32, cz: i32, x: f64, z: f64, current: i32, total: i32, width: f32) -> bool {
    let mx = f64::from(middle_block(cx));
    let mz = f64::from(middle_block(cz));
    let dx = x - mx;
    let dz = z - mz;
    let remaining = f64::from(total - current);
    let reach = f64::from((width + 2.0f32) + 16.0f32);
    dx * dx + dz * dz - remaining * remaining <= reach * reach
}

#[allow(clippy::too_many_arguments)]
pub fn ellipsoid<F>(
    mask: &mut CarveMask,
    cx: i32,
    cz: i32,
    x: f64,
    y: f64,
    z: f64,
    horizontal_radius: f64,
    vertical_radius: f64,
    mut should_carve: F,
) where
    F: FnMut(f64, f64, f64, i32) -> bool,
{
    let min_bx = 16i32.wrapping_mul(cx);
    let min_bz = 16i32.wrapping_mul(cz);
    let mid_x = f64::from(middle_block(cx));
    let mid_z = f64::from(middle_block(cz));
    let delta = 16.0 + horizontal_radius * 2.0;
    if (x - mid_x).abs() > delta || (z - mid_z).abs() > delta {
        return;
    }

    let px_lo = i32::max(floor_i32(x - horizontal_radius) - min_bx - 1, 0);
    let px_hi = i32::min(floor_i32(x + horizontal_radius) - min_bx, 15);
    let pz_lo = i32::max(floor_i32(z - horizontal_radius) - min_bz - 1, 0);
    let pz_hi = i32::min(floor_i32(z + horizontal_radius) - min_bz, 15);
    let y_lo = i32::max(floor_i32(y - vertical_radius) - 1, CARVE_MIN_Y);
    let y_hi = i32::min(floor_i32(y + vertical_radius) + 1, CARVE_MAX_Y);

    let mut p = px_lo;
    while p <= px_hi {
        let bx = min_bx.wrapping_add(p);
        let g = (f64::from(bx) + 0.5 - x) / horizontal_radius;
        let mut rr = pz_lo;
        while rr <= pz_hi {
            let bz = min_bz.wrapping_add(rr);
            let hh = (f64::from(bz) + 0.5 - z) / horizontal_radius;
            if g * g + hh * hh >= 1.0 {
                rr += 1;
                continue;
            }
            let mut t = y_hi;
            while t > y_lo {
                let d2 = (f64::from(t) - 0.5 - y) / vertical_radius;
                if should_carve(g, d2, hh, t) {
                    mask.set(p as usize, t, rr as usize);
                }
                t -= 1;
            }
            rr += 1;
        }
        p += 1;
    }
}

pub fn cave_should_carve(g: f64, d2: f64, h: f64, floor_level: f64) -> bool {
    d2 > floor_level && (g * g + d2 * d2 + h * h) < 1.0
}

pub fn canyon_should_carve(
    g: f64,
    d2: f64,
    h: f64,
    width_factor: f32,
    vertical_divisor: f64,
) -> bool {
    (g * g + h * h) * f64::from(width_factor) + (d2 * d2) / vertical_divisor < 1.0
}

pub fn width_factor_index(t: i32) -> usize {
    (t - CARVE_MIN_Y) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_reach_true_at_center() {
        assert!(can_reach(0, 0, 8.0, 8.0, 0, 100, 1.0));
    }

    #[test]
    fn can_reach_false_when_far_and_no_budget() {
        assert!(!can_reach(0, 0, 10_000.0, 0.0, 99, 100, 1.0));
    }

    #[test]
    fn cave_floor_predicate_rejects_below_floor() {
        assert!(!cave_should_carve(0.0, -0.9, 0.0, -0.8));
        assert!(cave_should_carve(0.0, 0.0, 0.0, -0.8));
    }

    #[test]
    fn ellipsoid_carves_central_cell() {
        let mut m = CarveMask::new();
        ellipsoid(&mut m, 0, 0, 8.0, 10.0, 8.0, 3.0, 3.0, |g, d2, h, _| {
            cave_should_carve(g, d2, h, -1.0)
        });
        assert!(m.any());
        assert!(m.get(8, 10, 8));
    }

    #[test]
    fn ellipsoid_outside_chunk_sets_nothing() {
        let mut m = CarveMask::new();
        ellipsoid(&mut m, 0, 0, 200.0, 10.0, 8.0, 3.0, 3.0, |g, d2, h, _| {
            cave_should_carve(g, d2, h, -1.0)
        });
        assert!(!m.any());
    }
}
