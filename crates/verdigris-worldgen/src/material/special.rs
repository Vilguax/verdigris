use crate::arith::floor_i32;
use crate::gradient::GRAD;
use crate::lcg::Lcg;
use crate::sources::RootSource;

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

#[allow(clippy::needless_range_loop)]
fn sha256_32(message: &[u8; 8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut block = [0u8; 64];
    block[..8].copy_from_slice(message);
    block[8] = 0x80;
    let bits = (message.len() as u64) * 8;
    block[56..64].copy_from_slice(&bits.to_be_bytes());

    let mut w = [0u32; 64];
    for (i, word) in w.iter_mut().enumerate().take(16) {
        let b = i * 4;
        *word = u32::from_be_bytes([block[b], block[b + 1], block[b + 2], block[b + 3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut a = h[0];
    let mut b = h[1];
    let mut c = h[2];
    let mut d = h[3];
    let mut e = h[4];
    let mut f = h[5];
    let mut g = h[6];
    let mut hh = h[7];
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn zoom_seed(world_seed: i64) -> i64 {
    let digest = sha256_32(&world_seed.to_le_bytes());
    i64::from_le_bytes(digest[0..8].try_into().unwrap())
}

const F2: f64 = 0.5 * (1.732_050_807_568_877_2 - 1.0);
const G2: f64 = (3.0 - 1.732_050_807_568_877_2) / 6.0;

#[derive(Clone, Debug)]
pub struct Simplex {
    perm: [u8; 256],
}

impl Simplex {
    pub fn new(r: &mut RootSource) -> Self {
        r.next_double();
        r.next_double();
        r.next_double();
        let mut p = [0u8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        for i in 0..256usize {
            let drawn = r.next_int_bounded(256 - i as i32) as usize;
            p.swap(i, i + drawn);
        }
        Self { perm: p }
    }

    pub fn from_lcg(seed: i64) -> Self {
        let mut r = RootSource::Lcg(Lcg::from_seed(seed));
        Self::new(&mut r)
    }

    pub fn permutation(&self) -> &[u8; 256] {
        &self.perm
    }

    fn h(&self, v: i32) -> i32 {
        self.perm[(v & 255) as usize] as i32
    }

    fn corner(&self, q: i32, u: f64, v: f64) -> f64 {
        let mut c = ((0.5 - u * u) - v * v) - 0.0;
        if c < 0.0 {
            0.0
        } else {
            c = c * c;
            let g = GRAD[(q & 15) as usize];
            (c * c) * (g.0 as f64 * u + g.1 as f64 * v + g.2 as f64 * 0.0)
        }
    }

    pub fn value(&self, bx: f64, bz: f64) -> f32 {
        let s = (bx + bz) * F2;
        let i = floor_i32(bx + s);
        let j = floor_i32(bz + s);
        let t = f64::from(i + j) * G2;
        let x0 = bx - (f64::from(i) - t);
        let y0 = bz - (f64::from(j) - t);
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let x1 = (x0 - i1 as f64) + G2;
        let y1 = (y0 - j1 as f64) + G2;
        let x2 = (x0 - 1.0) + 2.0 * G2;
        let y2 = (y0 - 1.0) + 2.0 * G2;
        let ii = i & 255;
        let jj = j & 255;
        let q0 = self.h(ii + self.h(jj)) % 12;
        let q1 = self.h(ii + i1 + self.h(jj + j1)) % 12;
        let q2 = self.h(ii + 1 + self.h(jj + 1)) % 12;
        let n0 = self.corner(q0, x0, y0);
        let n1 = self.corner(q1, x1, y1);
        let n2 = self.corner(q2, x2, y2);
        (70.0 * ((n0 + n1) + n2)) as f32
    }
}

#[derive(Clone, Debug)]
pub struct SimplexStack {
    layers: Vec<(f64, f32, Simplex)>,
}

impl SimplexStack {
    pub fn gf() -> Self {
        let mut r = RootSource::Lcg(Lcg::from_seed(3456));
        let specs = [
            (1.0f64, 0.142_857_15f32),
            (0.5, 0.285_714_3),
            (0.25, 0.571_428_6),
        ];
        let mut layers = Vec::with_capacity(3);
        for (freq, amp) in specs {
            let s = Simplex::new(&mut r);
            layers.push((freq, amp, s));
        }
        Self { layers }
    }

    pub fn first(&self) -> &Simplex {
        &self.layers[0].2
    }

    pub fn value(&self, bx: f64, bz: f64) -> f32 {
        let mut v = 0.0f32;
        for (freq, amp, s) in &self.layers {
            v += amp * s.value(bx * freq, bz * freq);
        }
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemperatureModifier {
    None,
    Frozen,
}

#[derive(Clone, Debug)]
pub struct FrozenNoise {
    gi: Simplex,
    gf: SimplexStack,
}

impl FrozenNoise {
    pub fn new() -> Self {
        Self {
            gi: Simplex::from_lcg(2345),
            gf: SimplexStack::gf(),
        }
    }

    pub fn gi(&self) -> &Simplex {
        &self.gi
    }

    pub fn gf(&self) -> &SimplexStack {
        &self.gf
    }

    pub fn temperature(&self, base: f32, modifier: TemperatureModifier, x: i32, z: i32) -> f32 {
        match modifier {
            TemperatureModifier::None => base,
            TemperatureModifier::Frozen => {
                let fx = f64::from(x);
                let fz = f64::from(z);
                let g1 = f64::from(self.gf.value(fx * 0.05, fz * 0.05) * 7.0);
                let g2 = f64::from(self.gi.value(fx * 0.2, fz * 0.2));
                if g1 + g2 < 0.3 {
                    let g3 = f64::from(self.gi.value(fx * 0.09, fz * 0.09));
                    if g3 < 0.8 {
                        return 0.2;
                    }
                }
                base
            }
        }
    }
}

impl Default for FrozenNoise {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Simplex, SimplexStack};

    #[test]
    fn simplex_is_zero_at_origin() {
        let s = Simplex::from_lcg(2345);
        assert_eq!(s.value(0.0, 0.0).to_bits() & 0x7fff_ffff, 0);
        let g = Simplex::from_lcg(1234);
        assert_eq!(g.value(0.0, 0.0).to_bits() & 0x7fff_ffff, 0);
    }

    #[test]
    #[ignore = "§8.3 INCERTAIN: permutation non confrontée à vanilla (pas de tels biomes dans oracle-1a)"]
    fn gi_permutation_prefix_seed_2345() {
        let gi = Simplex::from_lcg(2345);
        assert_eq!(
            &gi.permutation()[0..8],
            &[64, 175, 124, 148, 10, 239, 244, 91]
        );
    }

    #[test]
    #[ignore = "§8.3 INCERTAIN: permutation non confrontée à vanilla"]
    fn gf_first_permutation_prefix_seed_3456() {
        let gf = SimplexStack::gf();
        assert_eq!(
            &gf.first().permutation()[0..8],
            &[25, 120, 42, 41, 130, 59, 217, 175]
        );
    }
}
