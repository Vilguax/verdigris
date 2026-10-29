use crate::arith::{F32_FACTOR, F64_FACTOR, LCG_ADDEND, LCG_MASK, LCG_MULTIPLIER};
use crate::hash::{hash_position, java_string_hash};

#[derive(Clone, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn from_seed(s: i64) -> Self {
        Self {
            state: ((s as u64) ^ LCG_MULTIPLIER) & LCG_MASK,
        }
    }

    fn next_bits(&mut self, bits: u32) -> i32 {
        self.state = self
            .state
            .wrapping_mul(LCG_MULTIPLIER)
            .wrapping_add(LCG_ADDEND)
            & LCG_MASK;
        (self.state >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next_bits(32)
    }

    pub fn next_int_bounded(&mut self, n: i32) -> i32 {
        assert!(n > 0, "bound must be positive");
        let m = n - 1;
        if (n & m) == 0 {
            ((n as i64).wrapping_mul(self.next_bits(31) as i64) >> 31) as i32
        } else {
            let mut u = self.next_bits(31);
            loop {
                let r = u % n;
                if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                    return r;
                }
                u = self.next_bits(31);
            }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        let hi = self.next_bits(32) as i64;
        let lo = self.next_bits(32) as i64;
        (hi << 32).wrapping_add(lo)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next_bits(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * F32_FACTOR
    }

    pub fn next_double(&mut self) -> f64 {
        let hi = (self.next_bits(26) as i64) << 27;
        let lo = self.next_bits(27) as i64;
        hi.wrapping_add(lo) as f64 * F64_FACTOR
    }

    pub fn consume(&mut self, n: u32) {
        for _ in 0..n {
            self.next_bits(32);
        }
    }

    pub fn fork(&mut self) -> Lcg {
        Lcg::from_seed(self.next_long())
    }

    pub fn fork_positional(&mut self) -> LcgPositional {
        LcgPositional {
            g: self.next_long(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LcgPositional {
    g: i64,
}

impl LcgPositional {
    pub fn at(&self, x: i32, y: i32, z: i32) -> Lcg {
        Lcg::from_seed(hash_position(x, y, z) ^ self.g)
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn from_hash_of(&self, name: &str) -> Lcg {
        Lcg::from_seed(java_string_hash(name) as i64 ^ self.g)
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn from_seed(&self, s: i64) -> Lcg {
        Lcg::from_seed(s)
    }
}
