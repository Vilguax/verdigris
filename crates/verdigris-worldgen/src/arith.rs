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

pub const C7: f64 = 1.0e-7_f32 as f64;
pub const WRAP_PERIOD: f64 = 3.3554432e7;

pub fn l(a: f32, p: f32, q: f32) -> f32 {
    p + a * (q - p)
}

pub fn l2(ax: f32, ay: f32, p00: f32, p10: f32, p01: f32, p11: f32) -> f32 {
    l(ay, l(ax, p00, p10), l(ax, p01, p11))
}

#[allow(clippy::too_many_arguments)]
pub fn l3(
    ax: f32,
    ay: f32,
    az: f32,
    p000: f32,
    p100: f32,
    p010: f32,
    p110: f32,
    p001: f32,
    p101: f32,
    p011: f32,
    p111: f32,
) -> f32 {
    l(
        az,
        l2(ax, ay, p000, p100, p010, p110),
        l2(ax, ay, p001, p101, p011, p111),
    )
}

pub fn smooth(t: f32) -> f32 {
    ((t * t) * t) * ((t * ((t * 6.0) - 15.0)) + 10.0)
}

pub fn floor_i32(v: f64) -> i32 {
    if v.is_nan() {
        return 0;
    }
    let f = v.floor();
    if f <= i32::MIN as f64 {
        i32::MIN
    } else if f >= i32::MAX as f64 {
        i32::MAX
    } else {
        f as i32
    }
}

pub fn wrap(c: f64) -> f64 {
    c - (c / WRAP_PERIOD + 0.5).floor() * WRAP_PERIOD
}

pub fn rotl(v: u64, k: u32) -> u64 {
    v.rotate_left(k)
}

pub fn trunc32(v: u64) -> i32 {
    v as u32 as i32
}

#[cfg(test)]
mod tests {
    use super::{floor_i32, l, smooth, wrap};

    #[test]
    fn wrap_is_identity_below_2p24() {
        for &c in &[0.0, 1.0, -1.0, 16_777_215.0, -16_777_215.0, 123456.789] {
            assert_eq!(wrap(c).to_bits(), c.to_bits());
        }
    }

    #[test]
    fn wrap_folds_beyond_period() {
        assert_eq!(wrap(33_554_432.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(wrap(33_554_433.0).to_bits(), 1.0_f64.to_bits());
    }

    #[test]
    fn smooth_at_characteristic_points() {
        assert_eq!(smooth(0.0).to_bits(), 0.0_f32.to_bits());
        assert_eq!(smooth(1.0).to_bits(), 1.0_f32.to_bits());
        assert_eq!(smooth(0.5).to_bits(), 0.5_f32.to_bits());
    }

    #[test]
    fn l_endpoints_and_midpoint() {
        assert_eq!(l(0.0, 2.0, 10.0).to_bits(), 2.0_f32.to_bits());
        assert_eq!(l(1.0, 2.0, 10.0).to_bits(), 10.0_f32.to_bits());
        assert_eq!(l(0.5, 0.0, 10.0).to_bits(), 5.0_f32.to_bits());
    }

    #[test]
    fn floor_i32_bounds_and_nan() {
        assert_eq!(floor_i32(f64::NAN), 0);
        assert_eq!(floor_i32(3.7), 3);
        assert_eq!(floor_i32(-0.5), -1);
        assert_eq!(floor_i32(1.0e300), i32::MAX);
        assert_eq!(floor_i32(-1.0e300), i32::MIN);
    }
}
