use md5::{Digest, Md5};

use crate::arith::{F32_FACTOR, F64_FACTOR, GOLDEN, MIX13_M1, MIX13_M2, SILVER, rotl};
use crate::hash::hash_position;

pub fn mix13(z: u64) -> u64 {
    let z = (z ^ (z >> 30)).wrapping_mul(MIX13_M1);
    let z = (z ^ (z >> 27)).wrapping_mul(MIX13_M2);
    z ^ (z >> 31)
}

pub fn md5_words(name: &str) -> (u64, u64) {
    let digest = Md5::digest(name.as_bytes());
    let hlo = u64::from_be_bytes(digest[0..8].try_into().unwrap());
    let hhi = u64::from_be_bytes(digest[8..16].try_into().unwrap());
    (hlo, hhi)
}

#[derive(Clone, Debug)]
pub struct Xoroshiro {
    s0: u64,
    s1: u64,
}

impl Xoroshiro {
    pub fn from_seed(seed: i64) -> Self {
        let lo = (seed as u64) ^ SILVER;
        let hi = lo.wrapping_add(GOLDEN);
        Self {
            s0: mix13(lo),
            s1: mix13(hi),
        }
    }

    pub fn from_words(lo: u64, hi: u64) -> Self {
        if (lo | hi) == 0 {
            Self {
                s0: GOLDEN,
                s1: SILVER,
            }
        } else {
            Self { s0: lo, s1: hi }
        }
    }

    pub fn state(&self) -> (u64, u64) {
        (self.s0, self.s1)
    }

    pub fn next_long(&mut self) -> i64 {
        let s0 = self.s0;
        let mut s1 = self.s1;
        let result = rotl(s0.wrapping_add(s1), 17).wrapping_add(s0);
        s1 ^= s0;
        self.s0 = rotl(s0, 49) ^ s1 ^ (s1 << 21);
        self.s1 = rotl(s1, 28);
        result as i64
    }

    pub fn next_int(&mut self) -> i32 {
        self.next_long() as i32
    }

    pub fn next_int_bounded(&mut self, n: i32) -> i32 {
        assert!(n > 0, "bound must be positive");
        let bound = n as u64;
        let mut l = (self.next_int() as u32) as u64;
        let mut m = l.wrapping_mul(bound);
        let mut low = m & 0xFFFF_FFFF;
        if low < bound {
            let t = ((n as u32).wrapping_neg() % (n as u32)) as u64;
            while low < t {
                l = (self.next_int() as u32) as u64;
                m = l.wrapping_mul(bound);
                low = m & 0xFFFF_FFFF;
            }
        }
        (m >> 32) as i32
    }

    pub fn next_boolean(&mut self) -> bool {
        (self.next_long() & 1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        ((self.next_long() as u64) >> 40) as f32 * F32_FACTOR
    }

    pub fn next_double(&mut self) -> f64 {
        ((self.next_long() as u64) >> 11) as f64 * F64_FACTOR
    }

    pub fn consume(&mut self, n: u32) {
        for _ in 0..n {
            self.next_int();
        }
    }

    pub fn fork(&mut self) -> Xoroshiro {
        let lo = self.next_long() as u64;
        let hi = self.next_long() as u64;
        Xoroshiro::from_words(lo, hi)
    }

    pub fn fork_positional(&mut self) -> XoroshiroPositional {
        let plo = self.next_long() as u64;
        let phi = self.next_long() as u64;
        XoroshiroPositional { plo, phi }
    }
}

#[derive(Clone, Debug)]
pub struct XoroshiroPositional {
    plo: u64,
    phi: u64,
}

impl XoroshiroPositional {
    pub fn params(&self) -> (u64, u64) {
        (self.plo, self.phi)
    }

    pub fn at(&self, x: i32, y: i32, z: i32) -> Xoroshiro {
        Xoroshiro::from_words((hash_position(x, y, z) as u64) ^ self.plo, self.phi)
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn from_seed(&self, s: i64) -> Xoroshiro {
        Xoroshiro::from_words((s as u64) ^ self.plo, (s as u64) ^ self.phi)
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn from_hash_of(&self, name: &str) -> Xoroshiro {
        let (hlo, hhi) = md5_words(name);
        Xoroshiro::from_words(hlo ^ self.plo, hhi ^ self.phi)
    }
}
