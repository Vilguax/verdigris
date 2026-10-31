use crate::arith::{F32_FACTOR, F64_FACTOR};
use crate::xoroshiro::Xoroshiro;

#[derive(Clone, Debug)]
pub struct FeatureRng {
    core: Xoroshiro,
    next_gaussian: f64,
    have_gaussian: bool,
}

impl FeatureRng {
    pub fn new(seed: i64) -> Self {
        Self {
            core: Xoroshiro::from_seed(seed),
            next_gaussian: 0.0,
            have_gaussian: false,
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.core = Xoroshiro::from_seed(seed);
    }

    pub fn next_bits(&mut self, bits: u32) -> i32 {
        let raw = self.core.next_long() as u64;
        (raw >> (64 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next_bits(32)
    }

    pub fn next_int_bounded(&mut self, n: i32) -> i32 {
        if n <= 0 {
            return 0;
        }
        let m = n - 1;
        if (n & m) == 0 {
            let r = i64::from(n).wrapping_mul(i64::from(self.next_bits(31)));
            (r >> 31) as i32
        } else {
            let mut bits = self.next_bits(31);
            let mut val = bits % n;
            while bits.wrapping_sub(val).wrapping_add(m) < 0 {
                bits = self.next_bits(31);
                val = bits % n;
            }
            val
        }
    }

    pub fn next_int_between(&mut self, min: i32, max: i32) -> i32 {
        min.wrapping_add(self.next_int_bounded(max.wrapping_sub(min).wrapping_add(1)))
    }

    pub fn next_long(&mut self) -> i64 {
        let hi = i64::from(self.next_bits(32));
        let lo = i64::from(self.next_bits(32));
        (hi << 32).wrapping_add(lo)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next_bits(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * F32_FACTOR
    }

    pub fn next_double(&mut self) -> f64 {
        let hi = i64::from(self.next_bits(26)) << 27;
        let lo = i64::from(self.next_bits(27));
        hi.wrapping_add(lo) as f64 * F64_FACTOR
    }

    pub fn next_gaussian(&mut self) -> f64 {
        if self.have_gaussian {
            self.have_gaussian = false;
            return self.next_gaussian;
        }
        loop {
            let v1 = 2.0 * self.next_double() - 1.0;
            let v2 = 2.0 * self.next_double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let multiplier = (-2.0 * s.ln() / s).sqrt();
                self.next_gaussian = v2 * multiplier;
                self.have_gaussian = true;
                return v1 * multiplier;
            }
        }
    }

    pub fn consume(&mut self, n: u32) {
        for _ in 0..n {
            self.next_bits(32);
        }
    }

    pub fn set_decoration_seed(
        &mut self,
        level_seed: i64,
        min_block_x: i32,
        min_block_z: i32,
    ) -> i64 {
        self.set_seed(level_seed);
        let l = self.next_long() | 1;
        let m = self.next_long() | 1;
        let population = i64::from(min_block_x)
            .wrapping_mul(l)
            .wrapping_add(i64::from(min_block_z).wrapping_mul(m))
            ^ level_seed;
        self.set_seed(population);
        population
    }

    pub fn set_feature_seed(&mut self, population_seed: i64, index: i32, step: i32) {
        let seed = population_seed
            .wrapping_add(i64::from(index))
            .wrapping_add(10000i64.wrapping_mul(i64::from(step)));
        self.set_seed(seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_bits_sequence_11_2() {
        let mut r = FeatureRng::new(0);
        assert_eq!(r.next_bits(32), 707568776);
        assert_eq!(r.next_bits(32), -860085545);
        assert_eq!(r.next_bits(32), 1078879416);
    }

    #[test]
    fn next_long_11_2() {
        let mut r = FeatureRng::new(0);
        assert_eq!(r.next_long(), 3038984751730664151);
    }

    #[test]
    fn next_float_11_2() {
        let mut r = FeatureRng::new(0);
        let _ = r.next_long();
        let expected = 4214372f32 * F32_FACTOR;
        assert_eq!(r.next_float().to_bits(), expected.to_bits());
    }

    #[test]
    fn next_double_11_2() {
        let mut r = FeatureRng::new(0);
        let _ = r.next_long();
        let _ = r.next_float();
        let expected = 1054967193155086_f64 * F64_FACTOR;
        assert_eq!(r.next_double().to_bits(), expected.to_bits());
    }

    #[test]
    fn decoration_seed_equals_world_seed_at_origin_11_1() {
        let mut r = FeatureRng::new(0);
        let d = r.set_decoration_seed(20261029, 0, 0);
        assert_eq!(d, 20261029);
    }

    #[test]
    fn feature_seed_matches_formula_3_3() {
        let mut a = FeatureRng::new(0);
        let d = a.set_decoration_seed(20261029, 16, -32);
        a.set_feature_seed(d, 5, 3);
        let mut b = FeatureRng::new(0);
        b.set_seed(d.wrapping_add(5).wrapping_add(10000i64.wrapping_mul(3)));
        for _ in 0..4 {
            assert_eq!(a.next_int_bounded(16), b.next_int_bounded(16));
        }
    }

    #[test]
    fn set_seed_keeps_gaussian_buffer() {
        let mut r = FeatureRng::new(1);
        let _ = r.next_gaussian();
        assert!(r.have_gaussian);
        r.set_seed(999);
        assert!(r.have_gaussian);
        let buffered = r.next_gaussian;
        assert_eq!(r.next_gaussian().to_bits(), buffered.to_bits());
    }
}
