pub fn q(v: f32) -> i64 {
    (v * 10000.0f32) as i64
}

pub fn q_entry(decimal: f64) -> i64 {
    (decimal * 10000.0f64).round() as i64
}

#[cfg(test)]
mod tests {
    use super::{q, q_entry};

    #[test]
    fn truncates_toward_zero() {
        assert_eq!(q(1.23456), 12345);
        assert_eq!(q(-1.23456), -12345);
        assert_eq!(q(0.0), 0);
        assert_eq!(q(-0.0), 0);
    }

    #[test]
    fn nan_maps_to_zero() {
        assert_eq!(q(f32::NAN), 0);
    }

    #[test]
    fn saturates_on_overflow() {
        assert_eq!(q(f32::INFINITY), i64::MAX);
        assert_eq!(q(f32::NEG_INFINITY), i64::MIN);
    }

    #[test]
    fn intermediate_product_is_bit_exact() {
        let cases: [(f32, u32, i64); 6] = [
            (1.0, 0x461c_4000, 10000),
            (-1.0, 0xc61c_4000, -10000),
            (0.5, 0x459c_4000, 5000),
            (-0.103_910_65, 0xc481_e368, -1039),
            (1.23456, 0x4640_e667, 12345),
            (-1.2, 0xc63b_8000, -12000),
        ];
        for (v, product_bits, quantized) in cases {
            assert_eq!(
                (v * 10000.0f32).to_bits(),
                product_bits,
                "produit intermédiaire pour {v}"
            );
            assert_eq!(q(v), quantized, "quantification pour {v}");
        }
    }

    #[test]
    fn entry_matches_float_path_on_reference_values() {
        for &d in &[
            -1.2f64, -1.1, -1.05, -1.0, -0.9333, -0.85, -0.7799, -0.7666, -0.5666, -0.455, -0.45,
            -0.4, -0.375, -0.35, -0.2666, -0.2225, -0.19, -0.15, -0.11, -0.1, 0.0, 0.1, 0.15, 0.2,
            0.3, 0.4, 0.5, 0.55, 1.0,
        ] {
            assert_eq!(q_entry(d), q(d as f32), "mismatch at {d}");
        }
    }
}
