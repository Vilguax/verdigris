use crate::arith::{C7, floor_i32, l3, smooth};
use crate::sources::RootSource;

pub const GRAD: [(i8, i8, i8); 16] = [
    (1, 1, 0),
    (-1, 1, 0),
    (1, -1, 0),
    (-1, -1, 0),
    (1, 0, 1),
    (-1, 0, 1),
    (1, 0, -1),
    (-1, 0, -1),
    (0, 1, 1),
    (0, -1, 1),
    (0, 1, -1),
    (0, -1, -1),
    (1, 1, 0),
    (0, -1, 1),
    (-1, 1, 0),
    (0, -1, -1),
];

#[derive(Clone, Debug)]
pub struct GradientNoise {
    ox: f64,
    oy: f64,
    oz: f64,
    p: [u8; 256],
}

impl GradientNoise {
    pub fn new(r: &mut RootSource) -> Self {
        let ox = r.next_double() * 256.0;
        let oy = r.next_double() * 256.0;
        let oz = r.next_double() * 256.0;
        let mut p = [0u8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        for i in 0..256usize {
            let drawn = r.next_int_bounded(256 - i as i32) as usize;
            p.swap(i, i + drawn);
        }
        Self { ox, oy, oz, p }
    }

    pub fn origin(&self) -> (f64, f64, f64) {
        (self.ox, self.oy, self.oz)
    }

    pub fn permutation(&self) -> &[u8; 256] {
        &self.p
    }

    fn hash(&self, v: i32) -> i32 {
        self.p[(v & 255) as usize] as i32
    }

    fn grad_dot(&self, h: i32, u: f32, v: f32, w: f32) -> f32 {
        let g = GRAD[(h & 15) as usize];
        (g.0 as f32 * u + g.1 as f32 * v) + g.2 as f32 * w
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_and_lerp(
        &self,
        gx: i32,
        gy: i32,
        gz: i32,
        dx: f32,
        dy: f32,
        dz: f32,
        fade_y: f32,
    ) -> f32 {
        let a = self.hash(gx);
        let b = self.hash(gx.wrapping_add(1));
        let aa = self.hash(a.wrapping_add(gy));
        let ab = self.hash(a.wrapping_add(gy).wrapping_add(1));
        let ba = self.hash(b.wrapping_add(gy));
        let bb = self.hash(b.wrapping_add(gy).wrapping_add(1));

        let d000 = self.grad_dot(self.hash(aa.wrapping_add(gz)), dx, dy, dz);
        let d100 = self.grad_dot(self.hash(ba.wrapping_add(gz)), dx - 1.0, dy, dz);
        let d010 = self.grad_dot(self.hash(ab.wrapping_add(gz)), dx, dy - 1.0, dz);
        let d110 = self.grad_dot(self.hash(bb.wrapping_add(gz)), dx - 1.0, dy - 1.0, dz);
        let d001 = self.grad_dot(
            self.hash(aa.wrapping_add(gz).wrapping_add(1)),
            dx,
            dy,
            dz - 1.0,
        );
        let d101 = self.grad_dot(
            self.hash(ba.wrapping_add(gz).wrapping_add(1)),
            dx - 1.0,
            dy,
            dz - 1.0,
        );
        let d011 = self.grad_dot(
            self.hash(ab.wrapping_add(gz).wrapping_add(1)),
            dx,
            dy - 1.0,
            dz - 1.0,
        );
        let d111 = self.grad_dot(
            self.hash(bb.wrapping_add(gz).wrapping_add(1)),
            dx - 1.0,
            dy - 1.0,
            dz - 1.0,
        );

        l3(
            smooth(dx),
            smooth(fade_y),
            smooth(dz),
            d000,
            d100,
            d010,
            d110,
            d001,
            d101,
            d011,
            d111,
        )
    }

    pub fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        let xx = x + self.ox;
        let yy = y + self.oy;
        let zz = z + self.oz;
        let gx = floor_i32(xx);
        let gy = floor_i32(yy);
        let gz = floor_i32(zz);
        let rx = xx - gx as f64;
        let ry = yy - gy as f64;
        let rz = zz - gz as f64;
        let dx = rx as f32;
        let dy = ry as f32;
        let dz = rz as f32;
        self.sample_and_lerp(gx, gy, gz, dx, dy, dz, dy)
    }

    pub fn sample_smeared(&self, x: f64, y: f64, z: f64, e: f64) -> f32 {
        let xx = x + self.ox;
        let yy = y + self.oy;
        let zz = z + self.oz;
        let gx = floor_i32(xx);
        let gy = floor_i32(yy);
        let gz = floor_i32(zz);
        let rx = xx - gx as f64;
        let ry = yy - gy as f64;
        let rz = zz - gz as f64;
        let lambda = if y >= 0.0 && y < ry { y } else { ry };
        let kappa = if e != 0.0 {
            (floor_i32(lambda / e + C7) as f64) * e
        } else {
            0.0
        };
        let dx = rx as f32;
        let dyc = (ry - kappa) as f32;
        let dz = rz as f32;
        self.sample_and_lerp(gx, gy, gz, dx, dyc, dz, ry as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::GradientNoise;
    use crate::sources::positional_factory;

    fn first_gradient(seed: i64, key: &str, octave: &str) -> GradientNoise {
        let p = positional_factory(seed, false);
        let mut r = p.from_hash_of(key);
        let f1 = r.fork_positional();
        let mut s = f1.from_hash_of(octave);
        GradientNoise::new(&mut s)
    }

    #[test]
    fn extreme_coordinates_never_panic() {
        let g = first_gradient(0, "minecraft:offset", "octave_-3");
        let extremes = [
            0.0,
            i32::MAX as f64,
            i32::MIN as f64,
            -(i32::MAX as f64) - 2.0,
            1.0e300,
            -1.0e300,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ];
        for &x in &extremes {
            for &y in &extremes {
                for &z in &extremes {
                    let _ = g.sample(x, y, z);
                    let _ = g.sample_smeared(x, y, z, 1.0);
                }
            }
        }
    }

    #[test]
    fn offset_octave_minus3_origin_and_permutation_seed0() {
        let g = first_gradient(0, "minecraft:offset", "octave_-3");
        let (ox, oy, oz) = g.origin();
        assert_eq!(ox.to_bits(), 83.9670092904692_f64.to_bits());
        assert_eq!(oy.to_bits(), 84.088390992294_f64.to_bits());
        assert_eq!(oz.to_bits(), 215.14810353054168_f64.to_bits());
        assert_eq!(&g.permutation()[0..8], &[28, 135, 232, 8, 15, 19, 177, 255]);
    }

    #[test]
    fn continentalness_octave_minus9_origin_and_permutation_seed0() {
        let g = first_gradient(0, "minecraft:continentalness", "octave_-9");
        let (ox, oy, oz) = g.origin();
        assert_eq!(ox.to_bits(), 107.83518441372479_f64.to_bits());
        assert_eq!(oy.to_bits(), 250.9820076676574_f64.to_bits());
        assert_eq!(oz.to_bits(), 65.88805370474378_f64.to_bits());
        assert_eq!(&g.permutation()[0..4], &[254, 118, 111, 215]);
    }

    #[test]
    fn offset_octave_minus3_point_values_seed0() {
        let g = first_gradient(0, "minecraft:offset", "octave_-3");
        assert_eq!(
            g.sample(0.0, 0.0, 0.0).to_bits(),
            (-0.20457279682159424_f64 as f32).to_bits()
        );
        assert_eq!(
            g.sample(10.5, -3.25, 7.75).to_bits(),
            (-0.12707149982452393_f64 as f32).to_bits()
        );
    }

    #[test]
    fn permutation_is_a_bijection_for_any_seed() {
        let g = first_gradient(12345, "minecraft:continentalness", "octave_-9");
        let p = g.permutation();
        let mut counts = [0u16; 256];
        for &v in p.iter() {
            counts[v as usize] += 1;
        }
        assert!(counts.iter().all(|&c| c == 1));
    }

    #[test]
    fn sample_is_deterministic() {
        let g = first_gradient(0, "minecraft:offset", "octave_-3");
        let a = g.sample(3.5, 1.25, -2.0);
        let b = g.sample(3.5, 1.25, -2.0);
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn smeared_large_scale_matches_plain_sample() {
        let g = first_gradient(0, "minecraft:offset", "octave_-3");
        let plain = g.sample(1.5, 0.5, 2.5);
        let smeared = g.sample_smeared(1.5, 0.5, 2.5, 1.0e9);
        assert_eq!(plain.to_bits(), smeared.to_bits());
    }
}
