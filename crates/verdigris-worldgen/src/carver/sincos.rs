use std::sync::OnceLock;

pub const SIN_TABLE_LEN: usize = 65_536;
pub const SIN_CONV: f64 = 10_430.378_350_470_453;
pub const COS_OFFSET: f64 = 16_384.0;

pub struct SinTable {
    table: Vec<f32>,
}

impl SinTable {
    fn build() -> Self {
        let mut table = vec![0.0f32; SIN_TABLE_LEN];
        for (k, slot) in table.iter_mut().enumerate() {
            *slot = (k as f64 / SIN_CONV).sin() as f32;
        }
        SinTable { table }
    }

    pub fn get(&self) -> &[f32] {
        &self.table
    }

    pub fn at(&self, i: usize) -> f32 {
        self.table[i & (SIN_TABLE_LEN - 1)]
    }

    pub fn sin_t(&self, a: f32) -> f32 {
        let idx = (f64::from(a) * SIN_CONV) as i64 & 65_535;
        self.table[idx as usize]
    }

    pub fn cos_t(&self, a: f32) -> f32 {
        let idx = (f64::from(a) * SIN_CONV + COS_OFFSET) as i64 & 65_535;
        self.table[idx as usize]
    }
}

pub fn sin_table() -> &'static SinTable {
    static TABLE: OnceLock<SinTable> = OnceLock::new();
    TABLE.get_or_init(SinTable::build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_anchor_values_match_spec_8_1() {
        let t = sin_table();
        assert_eq!(t.at(0).to_bits(), 0.0f32.to_bits());
        assert_eq!(t.at(1).to_bits(), 952_700_891);
        assert_eq!(t.at(100).to_bits(), 0.009_587_233_f32.to_bits());
        assert_eq!(t.at(12_345).to_bits(), 0.925_957_f32.to_bits());
        assert_eq!(t.at(16_384).to_bits(), 1.0f32.to_bits());
        assert_eq!(t.at(32_768).to_bits(), 1.224_646_9e-16_f32.to_bits());
        assert_eq!(t.at(49_152).to_bits(), (-1.0f32).to_bits());
        assert_eq!(t.at(65_535).to_bits(), (-9.587_38e-5_f32).to_bits());
    }

    #[test]
    fn sin_t_cos_t_match_spec_8_1() {
        let t = sin_table();
        let pi_f = std::f32::consts::PI;
        let two_pi_f = std::f32::consts::TAU;
        assert_eq!(t.sin_t(-0.5).to_bits(), (-0.479_409_64_f32).to_bits());
        assert_eq!(t.cos_t(-0.5).to_bits(), 0.877_545_3_f32.to_bits());
        assert_eq!(t.sin_t(0.5).to_bits(), 0.479_409_64_f32.to_bits());
        assert_eq!(t.cos_t(0.5).to_bits(), 0.877_591_25_f32.to_bits());
        assert_eq!(t.cos_t(pi_f).to_bits(), (-1.0f32).to_bits());
        assert_eq!(t.cos_t(two_pi_f).to_bits(), 1.0f32.to_bits());
        assert_eq!(
            t.sin_t(std::f32::consts::FRAC_PI_2).to_bits(),
            1.0f32.to_bits()
        );
    }

    #[test]
    fn table_values_stay_in_unit_range() {
        let t = sin_table();
        assert!(t.get().iter().all(|&v| (-1.0..=1.0).contains(&v)));
    }

    #[test]
    fn table_fingerprint_sha256_matches_spec_8_1() {
        let t = sin_table();
        let mut bytes = Vec::with_capacity(SIN_TABLE_LEN * 4);
        for &v in t.get() {
            bytes.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        let digest = sha256(&bytes);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "87a67a39fff59636acb04edcda4ef622831037291f11fb61586c57596771dac2"
        );
    }

    fn sha256(message: &[u8]) -> [u8; 32] {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        let mut data = message.to_vec();
        let bitlen = (message.len() as u64).wrapping_mul(8);
        data.push(0x80);
        while data.len() % 64 != 56 {
            data.push(0);
        }
        data.extend_from_slice(&bitlen.to_be_bytes());
        for block in data.as_chunks::<64>().0 {
            let mut w = [0u32; 64];
            for (i, slot) in w.iter_mut().take(16).enumerate() {
                *slot = u32::from_be_bytes([
                    block[i * 4],
                    block[i * 4 + 1],
                    block[i * 4 + 2],
                    block[i * 4 + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let mut v = h;
            for i in 0..64 {
                let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
                let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
                let t1 = v[7]
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
                let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
                let t2 = s0.wrapping_add(maj);
                v[7] = v[6];
                v[6] = v[5];
                v[5] = v[4];
                v[4] = v[3].wrapping_add(t1);
                v[3] = v[2];
                v[2] = v[1];
                v[1] = v[0];
                v[0] = t1.wrapping_add(t2);
            }
            for (hi, vi) in h.iter_mut().zip(v.iter()) {
                *hi = hi.wrapping_add(*vi);
            }
        }
        let mut out = [0u8; 32];
        for (i, word) in h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}
