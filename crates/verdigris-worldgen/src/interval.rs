use crate::arith::{jabs_f32, jmax_f32, jmin_f32};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub min: f32,
    pub max: f32,
}

fn pb(a: f32, b: f32) -> f32 {
    if a != 0.0 && b != 0.0 { a * b } else { 0.0 }
}

impl Interval {
    pub const INDEF: Interval = Interval {
        min: f32::NAN,
        max: f32::NAN,
    };

    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn point(v: f32) -> Self {
        Self { min: v, max: v }
    }

    pub fn symmetric(m: f32) -> Self {
        Self { min: -m, max: m }
    }

    pub fn is_indef(self) -> bool {
        self.min.is_nan() || self.max.is_nan() || self.max < self.min
    }

    pub fn plus(self, o: Interval) -> Interval {
        Interval {
            min: self.min + o.min,
            max: self.max + o.max,
        }
    }

    pub fn scale(self, s: f32) -> Interval {
        let a = self.min * s;
        let b = self.max * s;
        Interval {
            min: jmin_f32(a, b),
            max: jmax_f32(a, b),
        }
    }

    pub fn contains_zero(self) -> bool {
        self.min <= 0.0 && 0.0 <= self.max
    }

    pub fn hull(self, o: Interval) -> Interval {
        if self.is_indef() {
            return o;
        }
        if o.is_indef() {
            return self;
        }
        Interval {
            min: jmin_f32(self.min, o.min),
            max: jmax_f32(self.max, o.max),
        }
    }

    pub fn iadd(self, o: Interval) -> Interval {
        if self.is_indef() || o.is_indef() {
            return Interval::INDEF;
        }
        let lo = self.min + o.min;
        let hi = self.max + o.max;
        if lo.is_nan() || hi.is_nan() {
            Interval::INDEF
        } else {
            Interval { min: lo, max: hi }
        }
    }

    pub fn isub(self, o: Interval) -> Interval {
        if self.is_indef() || o.is_indef() {
            return Interval::INDEF;
        }
        Interval {
            min: self.min - o.max,
            max: self.max - o.min,
        }
    }

    pub fn imul(self, o: Interval) -> Interval {
        if self.is_indef() || o.is_indef() {
            return Interval::INDEF;
        }
        let a = pb(self.min, o.min);
        let b = pb(self.min, o.max);
        let c = pb(self.max, o.min);
        let d = pb(self.max, o.max);
        Interval {
            min: jmin_f32(jmin_f32(a, b), jmin_f32(c, d)),
            max: jmax_f32(jmax_f32(a, b), jmax_f32(c, d)),
        }
    }

    pub fn recip(self) -> Interval {
        if self.is_indef() {
            return Interval::INDEF;
        }
        if self.min == 0.0 && self.max == 0.0 {
            return Interval::INDEF;
        }
        if self.min > 0.0 || self.max < 0.0 {
            Interval {
                min: 1.0 / self.max,
                max: 1.0 / self.min,
            }
        } else if self.max == 0.0 {
            Interval {
                min: f32::NEG_INFINITY,
                max: 1.0 / self.min,
            }
        } else if self.min == 0.0 {
            Interval {
                min: 1.0 / self.max,
                max: f32::INFINITY,
            }
        } else {
            Interval {
                min: f32::NEG_INFINITY,
                max: f32::INFINITY,
            }
        }
    }

    pub fn idiv(self, o: Interval) -> Interval {
        self.imul(o.recip())
    }

    pub fn imin(self, o: Interval) -> Interval {
        if self.is_indef() || o.is_indef() {
            return Interval::INDEF;
        }
        Interval {
            min: jmin_f32(self.min, o.min),
            max: jmin_f32(self.max, o.max),
        }
    }

    pub fn imax(self, o: Interval) -> Interval {
        if self.is_indef() || o.is_indef() {
            return Interval::INDEF;
        }
        Interval {
            min: jmax_f32(self.min, o.min),
            max: jmax_f32(self.max, o.max),
        }
    }

    pub fn clamp(self, a: f32, b: f32) -> Interval {
        if self.is_indef() {
            return Interval::INDEF;
        }
        if self.min >= b {
            Interval { min: b, max: b }
        } else if self.max <= a {
            Interval { min: a, max: a }
        } else {
            Interval {
                min: jmax_f32(self.min, a),
                max: jmin_f32(self.max, b),
            }
        }
    }

    pub fn iabs(self) -> Interval {
        if self.is_indef() {
            return Interval::INDEF;
        }
        let lo = jabs_f32(self.min);
        let hi = jabs_f32(self.max);
        let m = jmax_f32(lo, hi);
        if self.contains_zero() {
            Interval { min: 0.0, max: m }
        } else {
            Interval {
                min: jmin_f32(lo, hi),
                max: m,
            }
        }
    }

    pub fn isquare(self) -> Interval {
        if self.is_indef() {
            return Interval::INDEF;
        }
        let lo = self.min * self.min;
        let hi = self.max * self.max;
        let m = jmax_f32(lo, hi);
        if self.contains_zero() {
            Interval { min: 0.0, max: m }
        } else {
            Interval {
                min: jmin_f32(lo, hi),
                max: m,
            }
        }
    }

    pub fn monotone(self, f: impl Fn(f32) -> f32) -> Interval {
        if self.is_indef() {
            return Interval::INDEF;
        }
        let a = f(self.min);
        let b = f(self.max);
        Interval {
            min: jmin_f32(a, b),
            max: jmax_f32(a, b),
        }
    }

    pub fn lerp(alpha: Interval, first: Interval, second: Interval) -> Interval {
        if alpha.is_indef() || first.is_indef() || second.is_indef() {
            return Interval::INDEF;
        }
        let mut acc = Interval::INDEF;
        for &p in &[first.min, first.max] {
            for &q in &[second.min, second.max] {
                acc = acc.hull(Self::lerp_bound(alpha, p, q));
            }
        }
        acc
    }

    #[allow(clippy::float_cmp)]
    fn lerp_bound(alpha: Interval, p: f32, q: f32) -> Interval {
        if p.is_finite() && q.is_finite() {
            let a = p + pb(alpha.min, q - p);
            let b = p + pb(alpha.max, q - p);
            Interval {
                min: jmin_f32(a, b),
                max: jmax_f32(a, b),
            }
        } else if p == q {
            Interval { min: p, max: p }
        } else {
            let a = Self::beta(alpha.min, p, q);
            let b = Self::beta(alpha.max, p, q);
            if a.is_nan() || b.is_nan() {
                Interval::INDEF
            } else {
                Interval {
                    min: jmin_f32(a, b),
                    max: jmax_f32(a, b),
                }
            }
        }
    }

    fn beta(a: f32, p: f32, q: f32) -> f32 {
        let u = pb(1.0 - a, p);
        let v = pb(a, q);
        if !(u.is_infinite() && v.is_infinite()) {
            u + v
        } else if a <= 0.0 {
            if q > p {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            }
        } else if a >= 1.0 {
            if q > p {
                f32::INFINITY
            } else {
                f32::NEG_INFINITY
            }
        } else {
            f32::NAN
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Interval;

    #[test]
    fn symmetric_and_point() {
        let s = Interval::symmetric(2.0);
        assert_eq!(s.min.to_bits(), (-2.0_f32).to_bits());
        assert_eq!(s.max.to_bits(), 2.0_f32.to_bits());
        let p = Interval::point(0.0);
        assert_eq!(p.min.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn add_and_scale() {
        let base = Interval::symmetric(2.0);
        let scaled = base.scale(0.5);
        assert_eq!(scaled.min.to_bits(), (-1.0_f32).to_bits());
        assert_eq!(scaled.max.to_bits(), 1.0_f32.to_bits());
        let sum = scaled.plus(scaled);
        assert_eq!(sum.min.to_bits(), (-2.0_f32).to_bits());
        assert_eq!(sum.max.to_bits(), 2.0_f32.to_bits());
    }

    #[test]
    fn scale_negative_swaps_bounds() {
        let base = Interval::new(-2.0, 2.0);
        let scaled = base.scale(-3.0);
        assert_eq!(scaled.min.to_bits(), (-6.0_f32).to_bits());
        assert_eq!(scaled.max.to_bits(), 6.0_f32.to_bits());
    }
}
