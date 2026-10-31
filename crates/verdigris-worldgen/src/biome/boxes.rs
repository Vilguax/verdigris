use crate::biome::data::{DIMENSIONS, RawEntry};
use crate::biome::quantize::q;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamBox {
    pub a: [i64; DIMENSIONS],
    pub b: [i64; DIMENSIONS],
    pub biome: u16,
}

impl ParamBox {
    pub fn from_raw(entry: &RawEntry) -> Self {
        let mut a = [0i64; DIMENSIONS];
        let mut b = [0i64; DIMENSIONS];
        for (d, &(lo, hi)) in entry.bounds.iter().enumerate() {
            a[d] = q(lo);
            b[d] = q(hi);
        }
        ParamBox {
            a,
            b,
            biome: entry.biome_id,
        }
    }

    pub fn center(&self, d: usize) -> i64 {
        (self.a[d] + self.b[d]) / 2
    }
}

pub fn dist(bx: &ParamBox, target: &[i64; DIMENSIONS]) -> i64 {
    let mut sum = 0i64;
    for (d, &t) in target.iter().enumerate() {
        let gap = if t < bx.a[d] {
            bx.a[d] - t
        } else if t > bx.b[d] {
            t - bx.b[d]
        } else {
            0
        };
        sum += gap * gap;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::{ParamBox, dist};

    fn point(v: [i64; 7]) -> ParamBox {
        ParamBox {
            a: v,
            b: v,
            biome: 0,
        }
    }

    #[test]
    fn distance_is_zero_inside_box() {
        let bx = ParamBox {
            a: [-10, -10, -10, -10, -10, -10, -10],
            b: [10, 10, 10, 10, 10, 10, 10],
            biome: 0,
        };
        assert_eq!(dist(&bx, &[0, 0, 0, 0, 0, 0, 0]), 0);
        assert_eq!(dist(&bx, &[10, -10, 0, 0, 0, 0, 0]), 0);
    }

    #[test]
    fn distance_sums_squared_gaps() {
        let bx = point([0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(dist(&bx, &[3, 4, 0, 0, 0, 0, 0]), 25);
        assert_eq!(dist(&bx, &[0, 0, 0, 0, 0, 0, 12]), 144);
    }
}
