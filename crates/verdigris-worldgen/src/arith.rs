pub const LCG_MULTIPLIER: u64 = 0x5DEECE66D;
pub const LCG_ADDEND: u64 = 0xB;
pub const LCG_MASK: u64 = (1 << 48) - 1;

pub const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
pub const SILVER: u64 = 0x6A09_E667_F3BC_C909;

pub const MIX13_M1: u64 = 0xBF58_476D_1CE4_E5B9;
pub const MIX13_M2: u64 = 0x94D0_49BB_1331_11EB;

pub const F32_FACTOR: f32 = 1.0 / 16_777_216.0;
pub const F64_FACTOR: f64 = 1.0 / 9_007_199_254_740_992.0;

const NEG_ZERO_F64_BITS: u64 = 0x8000_0000_0000_0000;
const NEG_ZERO_F32_BITS: u32 = 0x8000_0000;

fn is_zero_f64(v: f64) -> bool {
    (v.to_bits() & !NEG_ZERO_F64_BITS) == 0
}

fn is_zero_f32(v: f32) -> bool {
    (v.to_bits() & !NEG_ZERO_F32_BITS) == 0
}

pub fn jmin_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if is_zero_f64(a) && is_zero_f64(b) && b.to_bits() == NEG_ZERO_F64_BITS {
        return b;
    }
    if a <= b { a } else { b }
}

pub fn jmax_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if is_zero_f64(a) && is_zero_f64(b) && a.to_bits() == NEG_ZERO_F64_BITS {
        return b;
    }
    if a >= b { a } else { b }
}

pub fn jabs_f64(a: f64) -> f64 {
    if a <= 0.0 { 0.0 - a } else { a }
}

#[allow(clippy::manual_clamp)]
pub fn jclamp_f64(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

pub fn jmin_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if is_zero_f32(a) && is_zero_f32(b) && b.to_bits() == NEG_ZERO_F32_BITS {
        return b;
    }
    if a <= b { a } else { b }
}

pub fn jmax_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if is_zero_f32(a) && is_zero_f32(b) && a.to_bits() == NEG_ZERO_F32_BITS {
        return b;
    }
    if a >= b { a } else { b }
}

pub fn jabs_f32(a: f32) -> f32 {
    if a <= 0.0 { 0.0 - a } else { a }
}

#[allow(clippy::manual_clamp)]
pub fn jclamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

pub fn rotl(v: u64, k: u32) -> u64 {
    v.rotate_left(k)
}

pub fn trunc32(v: u64) -> i32 {
    v as u32 as i32
}
