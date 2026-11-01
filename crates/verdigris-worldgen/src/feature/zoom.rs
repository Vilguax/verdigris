use crate::feature::error::FeatureError;
use crate::feature::region::Region;

const ZOOM_A: i64 = 6_364_136_223_846_793_005;
const ZOOM_B: i64 = 1_442_695_040_888_963_407;

const QY_MIN: i32 = -16;
const QY_MAX: i32 = 79;

fn zoom_n(r: i64, c: i64) -> i64 {
    let inner = r.wrapping_mul(ZOOM_A).wrapping_add(ZOOM_B);
    r.wrapping_mul(inner).wrapping_add(c)
}

fn zoom_f(r: i64) -> f64 {
    let shifted = r >> 24;
    let m = shifted.rem_euclid(1024);
    (m as f64 / 1024.0 - 0.5) * 0.9
}

pub(crate) fn fuzzy_cell(seed: i64, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    let xp = x - 2;
    let yp = y - 2;
    let zp = z - 2;
    let px = xp >> 2;
    let py = yp >> 2;
    let pz = zp >> 2;
    let fx = f64::from(xp & 3) / 4.0;
    let fy = f64::from(yp & 3) / 4.0;
    let fz = f64::from(zp & 3) / 4.0;
    let mut best = 0usize;
    let mut best_s = f64::INFINITY;
    for i in 0..8usize {
        let (cxx, dx) = if i & 4 != 0 {
            (px + 1, fx - 1.0)
        } else {
            (px, fx)
        };
        let (cyy, dy) = if i & 2 != 0 {
            (py + 1, fy - 1.0)
        } else {
            (py, fy)
        };
        let (czz, dz) = if i & 1 != 0 {
            (pz + 1, fz - 1.0)
        } else {
            (pz, fz)
        };
        let mut r = seed;
        r = zoom_n(r, i64::from(cxx));
        r = zoom_n(r, i64::from(cyy));
        r = zoom_n(r, i64::from(czz));
        r = zoom_n(r, i64::from(cxx));
        r = zoom_n(r, i64::from(cyy));
        r = zoom_n(r, i64::from(czz));
        let phix = zoom_f(r);
        r = zoom_n(r, seed);
        let phiy = zoom_f(r);
        r = zoom_n(r, seed);
        let phiz = zoom_f(r);
        let score =
            ((dz + phiz) * (dz + phiz) + (dy + phiy) * (dy + phiy)) + (dx + phix) * (dx + phix);
        if score < best_s {
            best_s = score;
            best = i;
        }
    }
    let cxx = if best & 4 != 0 { px + 1 } else { px };
    let cyy = if best & 2 != 0 { py + 1 } else { py };
    let czz = if best & 1 != 0 { pz + 1 } else { pz };
    (cxx, cyy, czz)
}

pub(crate) fn biome_at_block<R: Region>(
    region: &R,
    seed: i64,
    x: i32,
    y: i32,
    z: i32,
) -> Result<u16, FeatureError> {
    let (cxx, cyy, czz) = fuzzy_cell(seed, x, y, z);
    let qy = cyy.clamp(QY_MIN, QY_MAX);
    region.stored_biome(cxx, qy, czz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_cell_is_deterministic() {
        let a = fuzzy_cell(123456, 7, 40, -13);
        let b = fuzzy_cell(123456, 7, 40, -13);
        assert_eq!(a, b);
    }

    #[test]
    fn fuzzy_cell_origin_selects_a_neighbor_cell() {
        let (cx, cy, cz) = fuzzy_cell(0, 0, 0, 0);
        assert!((cx == -1 || cx == 0) && (cy == -1 || cy == 0) && (cz == -1 || cz == 0));
    }
}
