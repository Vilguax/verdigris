use crate::lcg::Lcg;

pub fn seed_large_feature(seed: i64, sx: i32, sz: i32) -> Lcg {
    let mut r = Lcg::from_seed(seed);
    let a = r.next_long();
    let b = r.next_long();
    let n = (i64::from(sx).wrapping_mul(a)) ^ (i64::from(sz).wrapping_mul(b)) ^ seed;
    Lcg::from_seed(n)
}

pub fn is_start(r: &mut Lcg, probability: f32) -> bool {
    r.next_float() <= probability
}

pub fn tunnel_source(r: &mut Lcg) -> Lcg {
    Lcg::from_seed(r.next_long())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_feature_is_deterministic() {
        let a = seed_large_feature(20_261_029, -5, 3);
        let b = seed_large_feature(20_261_029, -5, 3);
        let mut a2 = a;
        let mut b2 = b;
        assert_eq!(a2.next_long(), b2.next_long());
    }

    #[test]
    fn large_feature_varies_with_position() {
        let mut a = seed_large_feature(0, 0, 0);
        let mut b = seed_large_feature(0, 1, 0);
        assert_ne!(a.next_long(), b.next_long());
    }

    #[test]
    fn start_test_is_a_single_float_compare() {
        let mut r = seed_large_feature(0, 0, 0);
        let mut probe = r.clone();
        let f = probe.next_float();
        assert_eq!(is_start(&mut r, 1.0), f <= 1.0);
    }
}
