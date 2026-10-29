use crate::arith::{jmax_f32, jmin_f32};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub min: f32,
    pub max: f32,
}

impl Interval {
    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn point(v: f32) -> Self {
        Self { min: v, max: v }
    }

    pub fn symmetric(m: f32) -> Self {
        Self { min: -m, max: m }
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
