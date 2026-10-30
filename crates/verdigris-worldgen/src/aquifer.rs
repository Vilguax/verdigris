use crate::arith::{floor_div_i32, floor_i32, jabs_f64, jmin_f64};
use crate::df::eval::eval_point;
use crate::df::node::Node;
use crate::df::volume::{Volume, eval_point_ctx};
use crate::sources::PositionalFactory;

pub const LAVA_LEVEL: i32 = -54;
pub const EMPTY_LEVEL: i32 = -32512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AquiferMode {
    Disabled,
    Noise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FluidType {
    Water,
    Lava,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blk {
    Air,
    Water,
    Lava,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filled {
    Nothing,
    Air,
    Water,
    Lava,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub level: i32,
    pub fluid: FluidType,
}

impl Status {
    fn at(self, y: i32) -> Blk {
        if y < self.level {
            match self.fluid {
                FluidType::Water => Blk::Water,
                FluidType::Lava => Blk::Lava,
            }
        } else {
            Blk::Air
        }
    }
}

fn global(y: i32, sea_level: i32) -> Status {
    if y < i32::min(LAVA_LEVEL, sea_level) {
        Status {
            level: LAVA_LEVEL,
            fluid: FluidType::Lava,
        }
    } else {
        Status {
            level: sea_level,
            fluid: FluidType::Water,
        }
    }
}

pub struct AquiferFunctions {
    pub barrier: Node,
    pub exclusion: Node,
    pub floodedness: Node,
    pub spread: Node,
    pub lava: Node,
    pub surface_level: Node,
    pub factory: PositionalFactory,
    pub sea_level: i32,
}

const SEARCH_OFFSETS: [(i32, i32); 13] = [
    (0, 0),
    (-2, -1),
    (-1, -1),
    (0, -1),
    (1, -1),
    (-3, 0),
    (-2, 0),
    (-1, 0),
    (1, 0),
    (-2, 1),
    (-1, 1),
    (0, 1),
    (1, 1),
];

const CANDIDATE_OFFSETS: [(i32, i32, i32); 12] = [
    (0, -1, 0),
    (0, -1, 1),
    (0, 0, 0),
    (0, 0, 1),
    (0, 1, 0),
    (0, 1, 1),
    (1, -1, 0),
    (1, -1, 1),
    (1, 0, 0),
    (1, 0, 1),
    (1, 1, 0),
    (1, 1, 1),
];

fn sim(p: i32, q: i32) -> f64 {
    1.0 - (f64::from(q.wrapping_sub(p))) / 25.0
}

fn sigma_ec() -> f64 {
    sim(100, 144)
}

pub fn anchor_cell(x: i32, y: i32, z: i32) -> [i32; 3] {
    [(x - 5) >> 4, floor_div_i32(y + 1, 12), (z - 5) >> 4]
}

pub fn y_saut_from_max(m: i32) -> i32 {
    12 * (floor_div_i32(m + 20, 12) + 1) + 10
}

pub struct AquiferScratch {
    gx_min: i32,
    gy_min: i32,
    gz_min: i32,
    nx: usize,
    ny: usize,
    nz: usize,
    centers: Vec<Option<[i32; 3]>>,
    statuses: Vec<Option<Status>>,
    qx_min: i32,
    qz_min: i32,
    sqx: usize,
    sqz: usize,
    surfaces: Vec<i32>,
    y_saut: i32,
    footprint: Volume,
}

impl Default for AquiferScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl AquiferScratch {
    pub fn new() -> Self {
        Self {
            gx_min: 0,
            gy_min: 0,
            gz_min: 0,
            nx: 0,
            ny: 0,
            nz: 0,
            centers: Vec::new(),
            statuses: Vec::new(),
            qx_min: 0,
            qz_min: 0,
            sqx: 0,
            sqz: 0,
            surfaces: Vec::new(),
            y_saut: 0,
            footprint: Volume::new([0, 0, 0], [1, 1, 1], [0, 0, 0]),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        f: &AquiferFunctions,
        x_min: i32,
        x_max: i32,
        y_min: i32,
        y_max: i32,
        z_min: i32,
        z_max: i32,
    ) {
        let gx_min = (x_min - 5) >> 4;
        let gx_max = ((x_max - 5) >> 4) + 1;
        let gz_min = (z_min - 5) >> 4;
        let gz_max = ((z_max - 5) >> 4) + 1;
        let gy_min = floor_div_i32(y_min + 1, 12) - 1;
        let gy_max = floor_div_i32(y_max + 1, 12) + 1;

        self.gx_min = gx_min;
        self.gy_min = gy_min;
        self.gz_min = gz_min;
        self.nx = (gx_max - gx_min + 1) as usize;
        self.ny = (gy_max - gy_min + 1) as usize;
        self.nz = (gz_max - gz_min + 1) as usize;
        let cells = self.nx * self.ny * self.nz;
        self.centers.clear();
        self.centers.resize(cells, None);
        self.statuses.clear();
        self.statuses.resize(cells, None);

        let nvx = (x_max - x_min + 1) as usize;
        let nvy = (y_max - y_min + 1) as usize;
        let nvz = (z_max - z_min + 1) as usize;
        self.footprint = Volume::new([x_min, y_min, z_min], [1, 1, 1], [nvx, nvy, nvz]);

        let cx0 = 16i32.wrapping_mul(gx_min);
        let cx1 = 16i32.wrapping_mul(gx_max) + 9;
        let cz0 = 16i32.wrapping_mul(gz_min);
        let cz1 = 16i32.wrapping_mul(gz_max) + 9;
        let sx0 = cx0 - 48;
        let sx1 = cx1 + 16;
        let sz0 = cz0 - 16;
        let sz1 = cz1 + 16;
        let qx_min = (sx0 >> 2) * 4;
        let qx_max = (sx1 >> 2) * 4;
        let qz_min = (sz0 >> 2) * 4;
        let qz_max = (sz1 >> 2) * 4;
        self.qx_min = qx_min;
        self.qz_min = qz_min;
        self.sqx = ((qx_max - qx_min) / 4 + 1) as usize;
        self.sqz = ((qz_max - qz_min) / 4 + 1) as usize;
        self.surfaces.clear();
        self.surfaces.resize(self.sqx * self.sqz, 0);
        for iz in 0..self.sqz {
            let qz = qz_min + 4 * iz as i32;
            for ix in 0..self.sqx {
                let qx = qx_min + 4 * ix as i32;
                let v = eval_point_ctx(&self.footprint, &f.surface_level, qx, 0, qz);
                let slot = ix + self.sqx * iz;
                self.surfaces[slot] = floor_i32(f64::from(v));
            }
        }

        let mqx0 = cx0 >> 2;
        let mqx1 = cx1 >> 2;
        let mqz0 = cz0 >> 2;
        let mqz1 = cz1 >> 2;
        let mut m = i32::MIN;
        let mut qz = mqz0;
        while qz <= mqz1 {
            let mut qx = mqx0;
            while qx <= mqx1 {
                let ix = ((4 * qx - qx_min) / 4) as usize;
                let iz = ((4 * qz - qz_min) / 4) as usize;
                let s = self.surfaces[ix + self.sqx * iz];
                if s > m {
                    m = s;
                }
                qx += 1;
            }
            qz += 1;
        }
        self.y_saut = y_saut_from_max(m);
    }

    fn grid_index(&self, gx: i32, gy: i32, gz: i32) -> usize {
        let ix = (gx - self.gx_min) as usize;
        let iy = (gy - self.gy_min) as usize;
        let iz = (gz - self.gz_min) as usize;
        ix + self.nx * (iy + self.ny * iz)
    }

    fn center(&mut self, f: &AquiferFunctions, gx: i32, gy: i32, gz: i32) -> [i32; 3] {
        let idx = self.grid_index(gx, gy, gz);
        if let Some(c) = self.centers[idx] {
            return c;
        }
        let mut r = f.factory.at(gx, gy, gz);
        let a = r.next_int_bounded(10);
        let b = r.next_int_bounded(9);
        let e = r.next_int_bounded(10);
        let c = [
            16i32.wrapping_mul(gx).wrapping_add(a),
            12i32.wrapping_mul(gy).wrapping_add(b),
            16i32.wrapping_mul(gz).wrapping_add(e),
        ];
        self.centers[idx] = Some(c);
        c
    }

    fn status(&mut self, f: &AquiferFunctions, gx: i32, gy: i32, gz: i32) -> Status {
        let idx = self.grid_index(gx, gy, gz);
        if let Some(s) = self.statuses[idx] {
            return s;
        }
        let c = self.center(f, gx, gy, gz);
        let s = self.compute_status(f, c);
        self.statuses[idx] = Some(s);
        s
    }

    fn ps(&self, f: &AquiferFunctions, x: i32, z: i32) -> i32 {
        let qx = 4 * (x >> 2);
        let qz = 4 * (z >> 2);
        let ix = (qx - self.qx_min) / 4;
        let iz = (qz - self.qz_min) / 4;
        if ix >= 0 && iz >= 0 && (ix as usize) < self.sqx && (iz as usize) < self.sqz {
            self.surfaces[ix as usize + self.sqx * iz as usize]
        } else {
            floor_i32(f64::from(eval_point_ctx(
                &self.footprint,
                &f.surface_level,
                qx,
                0,
                qz,
            )))
        }
    }

    fn compute_status(&mut self, f: &AquiferFunctions, c: [i32; 3]) -> Status {
        let [xc, yc, zc] = c;
        let g = global(yc, f.sea_level);
        let mut s_min = i32::MAX;
        let mut sous_mer = false;
        for (idx, (a, b)) in SEARCH_OFFSETS.iter().enumerate() {
            let depart = idx == 0;
            let xs = xc.wrapping_add(16i32.wrapping_mul(*a));
            let zs = zc.wrapping_add(16i32.wrapping_mul(*b));
            let s = self.ps(f, xs, zs);
            let sa = s + 8;
            if depart && yc - 12 > sa {
                return g;
            }
            let depasse = yc + 12 > sa;
            if depasse || depart {
                let h = global(sa, f.sea_level);
                if h.at(sa) != Blk::Air {
                    if depart {
                        sous_mer = true;
                    }
                    if depasse {
                        return h;
                    }
                }
            }
            if s < s_min {
                s_min = s;
            }
        }
        self.inondation(f, c, g, s_min, sous_mer)
    }

    fn inondation(
        &mut self,
        f: &AquiferFunctions,
        c: [i32; 3],
        g: Status,
        s_min: i32,
        sous_mer: bool,
    ) -> Status {
        let [xc, yc, zc] = c;
        let (w_tot, w_part) = if eval_point_ctx(&self.footprint, &f.exclusion, xc, yc, zc) > 0.0 {
            (-1.0f64, -1.0f64)
        } else {
            let delta = (s_min + 8) - yc;
            let kappa = if !sous_mer {
                0.0f64
            } else {
                let t = f64::from(delta) / 64.0;
                if t < 0.0 {
                    1.0
                } else if t > 1.0 {
                    0.0
                } else {
                    1.0 + t * (0.0 - 1.0)
                }
            };
            let mut v = f64::from(eval_point(&f.floodedness, xc, yc, zc));
            v = if v < -1.0 { -1.0 } else { jmin_f64(v, 1.0) };
            let u = (kappa - 1.0) / (0.0 - 1.0);
            let sigma_tot = -0.3 + u * (0.8 - (-0.3));
            let sigma_part = -0.8 + u * (0.4 - (-0.8));
            (v - sigma_tot, v - sigma_part)
        };
        let level = if w_tot > 0.0 {
            g.level
        } else if w_part > 0.0 {
            self.niveau_disperse(f, c, s_min)
        } else {
            EMPTY_LEVEL
        };
        let fluid = self.fluid_type(f, c, g, level);
        Status { level, fluid }
    }

    fn niveau_disperse(&mut self, f: &AquiferFunctions, c: [i32; 3], s_min: i32) -> i32 {
        let [xc, yc, zc] = c;
        let ux = floor_div_i32(xc, 16);
        let uy = floor_div_i32(yc, 40);
        let uz = floor_div_i32(zc, 16);
        let milieu = 40i32.wrapping_mul(uy).wrapping_add(20);
        let spread = eval_point(&f.spread, ux, uy, uz);
        let eps = f64::from(spread * 10.0f32);
        let q = floor_i32(eps / 3.0).wrapping_mul(3);
        i32::min(s_min, milieu.wrapping_add(q))
    }

    fn fluid_type(
        &mut self,
        f: &AquiferFunctions,
        c: [i32; 3],
        g: Status,
        level: i32,
    ) -> FluidType {
        let [xc, yc, zc] = c;
        if level <= -10 && level != EMPTY_LEVEL && g.fluid != FluidType::Lava {
            let lx = floor_div_i32(xc, 64);
            let ly = floor_div_i32(yc, 40);
            let lz = floor_div_i32(zc, 64);
            let lava = f64::from(eval_point(&f.lava, lx, ly, lz));
            if jabs_f64(lava) > 0.3 {
                return FluidType::Lava;
            }
        }
        g.fluid
    }

    fn barrier(
        &self,
        f: &AquiferFunctions,
        x: i32,
        y: i32,
        z: i32,
        cache: &mut Option<f64>,
    ) -> f64 {
        if let Some(v) = *cache {
            return v;
        }
        let v = f64::from(eval_point(&f.barrier, x, y, z));
        *cache = Some(v);
        v
    }

    #[allow(clippy::too_many_arguments)]
    fn pr(
        &self,
        f: &AquiferFunctions,
        a: Status,
        b: Status,
        x: i32,
        y: i32,
        z: i32,
        cache: &mut Option<f64>,
    ) -> f64 {
        let ta = a.at(y);
        let tb = b.at(y);
        let lava_water =
            (ta == Blk::Lava && tb == Blk::Water) || (ta == Blk::Water && tb == Blk::Lava);
        if lava_water {
            return 2.0;
        }
        let lam = a.level.wrapping_sub(b.level).abs();
        if lam == 0 {
            return 0.0;
        }
        let m = 0.5 * f64::from(a.level.wrapping_add(b.level));
        let h = (f64::from(y) + 0.5) - m;
        let beta = f64::from(lam) / 2.0;
        let eta = beta - jabs_f64(h);
        let gamma = if h > 0.0 {
            let cc = eta;
            if cc > 0.0 { cc / 1.5 } else { cc / 2.5 }
        } else {
            let cc = 3.0 + eta;
            if cc > 0.0 { cc / 3.0 } else { cc / 10.0 }
        };
        let n = if gamma < -2.0 || gamma > 2.0 {
            0.0
        } else {
            self.barrier(f, x, y, z, cache)
        };
        2.0 * (n + gamma)
    }

    pub fn decide(
        &mut self,
        f: &AquiferFunctions,
        x: i32,
        y: i32,
        z: i32,
        d: f64,
    ) -> (Filled, bool) {
        if d > 0.0 {
            return (Filled::Nothing, false);
        }
        let g = global(y, f.sea_level);
        if y > self.y_saut {
            return (blk_to_filled(g.at(y)), false);
        }
        if g.at(y) == Blk::Lava {
            return (Filled::Lava, false);
        }

        let [ax, ay, az] = anchor_cell(x, y, z);

        let mut dist = [i32::MAX; 4];
        let mut cells = [[0i32; 3]; 4];
        for (i, j, k) in CANDIDATE_OFFSETS {
            let gx = ax + i;
            let gy = ay + j;
            let gz = az + k;
            let c = self.center(f, gx, gy, gz);
            let dx = c[0].wrapping_sub(x);
            let dy = c[1].wrapping_sub(y);
            let dz = c[2].wrapping_sub(z);
            let dsq = dx
                .wrapping_mul(dx)
                .wrapping_add(dy.wrapping_mul(dy))
                .wrapping_add(dz.wrapping_mul(dz));
            let mut pos = 4usize;
            for (slot, &dd) in dist.iter().enumerate() {
                if dd >= dsq {
                    pos = slot;
                    break;
                }
            }
            if pos < 4 {
                let mut m = 3usize;
                while m > pos {
                    dist[m] = dist[m - 1];
                    cells[m] = cells[m - 1];
                    m -= 1;
                }
                dist[pos] = dsq;
                cells[pos] = [gx, gy, gz];
            }
        }

        let s1 = self.status(f, cells[0][0], cells[0][1], cells[0][2]);
        let s2 = self.status(f, cells[1][0], cells[1][1], cells[1][2]);
        let s3 = self.status(f, cells[2][0], cells[2][1], cells[2][2]);
        let s4 = self.status(f, cells[3][0], cells[3][1], cells[3][2]);

        let sig = sigma_ec();
        let fsel = blk_to_filled(s1.at(y));
        let s12 = sim(dist[0], dist[1]);
        if s12 <= 0.0 {
            let phi = s12 >= sig && s1 != s2;
            return (fsel, phi);
        }

        if s1.at(y) == Blk::Water && global(y - 1, f.sea_level).at(y - 1) == Blk::Lava {
            return (fsel, true);
        }

        let mut cache: Option<f64> = None;
        let b12 = s12 * self.pr(f, s1, s2, x, y, z, &mut cache);
        if d + b12 > 0.0 {
            return (Filled::Nothing, false);
        }
        let s13 = sim(dist[0], dist[2]);
        if s13 > 0.0 {
            let b13 = (s12 * s13) * self.pr(f, s1, s3, x, y, z, &mut cache);
            if d + b13 > 0.0 {
                return (Filled::Nothing, false);
            }
        }
        let s23 = sim(dist[1], dist[2]);
        if s23 > 0.0 {
            let b23 = (s12 * s23) * self.pr(f, s2, s3, x, y, z, &mut cache);
            if d + b23 > 0.0 {
                return (Filled::Nothing, false);
            }
        }

        let e12 = s1 != s2;
        let e23 = s23 >= sig && s2 != s3;
        let e13 = s13 >= sig && s1 != s3;
        let phi = if e12 || e23 || e13 {
            true
        } else {
            let s14 = sim(dist[0], dist[3]);
            s13 >= sig && s14 >= sig && s1 != s4
        };
        (fsel, phi)
    }

    pub fn column_base_height(
        &mut self,
        f: &AquiferFunctions,
        x: i32,
        z: i32,
        min_y: i32,
        height: i32,
        density: &[f32],
    ) -> Option<i32> {
        self.prepare(f, x, x, min_y, min_y + height - 1, z, z);
        let mut y = min_y + height - 1;
        while y >= min_y {
            let j = (y - min_y) as usize;
            let d = f64::from(density[j]);
            let (filled, _) = self.decide(f, x, y, z, d);
            let blk = match filled {
                Filled::Air => Blk::Air,
                Filled::Nothing | Filled::Water | Filled::Lava => Blk::Water,
            };
            if blk != Blk::Air {
                return Some(y);
            }
            y -= 1;
        }
        None
    }
}

fn blk_to_filled(b: Blk) -> Filled {
    match b {
        Blk::Air => Filled::Air,
        Blk::Water => Filled::Water,
        Blk::Lava => Filled::Lava,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigma_ec_is_minus_076() {
        assert_eq!(sigma_ec().to_bits(), (-0.76f64).to_bits());
    }

    #[test]
    fn sim_thresholds() {
        assert!(sim(0, 24) > 0.0);
        assert!(sim(0, 25) <= 0.0);
        assert!(sim(0, 44) >= sigma_ec());
        assert!(sim(0, 45) < sigma_ec());
    }

    #[test]
    fn sim_exact_values() {
        assert_eq!(sim(0, 0).to_bits(), 1.0f64.to_bits());
        assert_eq!(sim(0, 5).to_bits(), 0.8f64.to_bits());
        assert_eq!(sim(0, 8).to_bits(), 0.6799999999999999f64.to_bits());
        assert_eq!(sim(0, 14).to_bits(), 0.43999999999999995f64.to_bits());
        assert_eq!(sim(0, 20).to_bits(), 0.19999999999999996f64.to_bits());
        assert_eq!(sim(0, 24).to_bits(), 0.040000000000000036f64.to_bits());
        assert_eq!(sim(0, 25).to_bits(), 0.0f64.to_bits());
        assert_eq!(sim(0, 44).to_bits(), (-0.76f64).to_bits());
    }

    #[test]
    fn y_saut_formula() {
        assert_eq!(y_saut_from_max(64), 106);
        assert_eq!(y_saut_from_max(40), 82);
        assert_eq!(y_saut_from_max(-64), -26);
    }

    #[test]
    fn anchor_cells_8_1() {
        assert_eq!(anchor_cell(0, 0, 0), [-1, 0, -1]);
        assert_eq!(anchor_cell(5, -1, 21), [0, 0, 1]);
    }

    #[test]
    fn global_selector_overworld() {
        assert_eq!(
            global(-55, 63),
            Status {
                level: -54,
                fluid: FluidType::Lava
            }
        );
        assert_eq!(
            global(-54, 63),
            Status {
                level: 63,
                fluid: FluidType::Water
            }
        );
        assert_eq!(global(62, 63).at(62), Blk::Water);
        assert_eq!(global(63, 63).at(63), Blk::Air);
    }
}
