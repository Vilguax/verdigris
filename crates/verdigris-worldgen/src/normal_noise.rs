use crate::arith::jabs_f64;
use crate::gradient::GradientNoise;
use crate::interval::Interval;
use crate::octaves::{Octave, OctaveStack};
use crate::registry::WorldgenError;
use crate::sources::PositionalFactory;

pub const AMP_MIN: f64 = 9.999999747378752e-6;
pub const AMP_MAX: f64 = 1_000_000.0;
pub const MOD_MAX: f64 = 1_000_000.0;
pub const MAX_MODIFIERS: usize = 32;

pub const C_DEV: f64 = 0.2702247831245211;
pub const THIRD: f64 = 0.3333333333333333;
pub const SQRT2: f64 = std::f64::consts::SQRT_2;
pub const SECOND_NOISE_FREQ_FACTOR: f64 = 1.0181268882175227;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Normalize {
    Enabled,
    Disabled,
    Legacy,
}

#[derive(Clone, Debug)]
pub struct NormalNoiseDef {
    pub base_amplitude: f64,
    pub base_octave: i32,
    pub octave_count: i32,
    pub amplitude_modifiers: Vec<f64>,
    pub normalize: Normalize,
}

#[derive(Clone, Copy, Debug)]
pub struct PresentOctave {
    pub index: i32,
    pub freq: f64,
    pub amp: f64,
}

impl NormalNoiseDef {
    pub fn validate(&self, entry: &str) -> Result<(), WorldgenError> {
        if !(AMP_MIN..=AMP_MAX).contains(&self.base_amplitude) {
            return Err(WorldgenError::InvalidField {
                entry: entry.to_string(),
                field: "base_amplitude".to_string(),
                reason: "out of range [1e-5, 1e6]".to_string(),
            });
        }
        if !(-32..=32).contains(&self.base_octave) {
            return Err(WorldgenError::OctaveOutOfRange {
                entry: entry.to_string(),
                value: self.base_octave,
            });
        }
        if !(1..=32).contains(&self.octave_count) {
            return Err(WorldgenError::OctaveCountOutOfRange {
                entry: entry.to_string(),
                value: self.octave_count,
            });
        }
        if self.amplitude_modifiers.len() > MAX_MODIFIERS {
            return Err(WorldgenError::TooManyModifiers {
                entry: entry.to_string(),
                value: self.amplitude_modifiers.len(),
            });
        }
        for m in &self.amplitude_modifiers {
            if !(0.0..=MOD_MAX).contains(m) {
                return Err(WorldgenError::ModifierOutOfRange {
                    entry: entry.to_string(),
                    value: *m,
                });
            }
        }
        if !self.amplitude_modifiers.is_empty()
            && self.amplitude_modifiers.len() as i32 != self.octave_count
        {
            return Err(WorldgenError::AmplitudeLenMismatch {
                entry: entry.to_string(),
                expected: self.octave_count,
                got: self.amplitude_modifiers.len(),
            });
        }
        Ok(())
    }

    fn effective_modifiers(&self) -> Vec<f64> {
        if self.amplitude_modifiers.is_empty() {
            vec![1.0; self.octave_count as usize]
        } else {
            self.amplitude_modifiers.clone()
        }
    }

    pub fn octaves_present(&self) -> Vec<PresentOctave> {
        let n = self.octave_count;
        let mods = self.effective_modifiers();
        let mut freq = (2.0f64).powi(self.base_octave);
        let mut a = self.base_amplitude;
        if self.normalize != Normalize::Disabled {
            a *= (2.0f64).powi(n - 1) / ((2.0f64).powi(n) - 1.0);
        }
        let mut out = Vec::new();
        for (i, &mu) in mods.iter().enumerate() {
            if mu != 0.0 {
                out.push(PresentOctave {
                    index: self.base_octave + i as i32,
                    freq,
                    amp: a * mu,
                });
            }
            freq *= 2.0;
            a *= 0.5;
        }
        out
    }

    fn normalization(&self, present: &[PresentOctave]) -> (f64, f64) {
        let t = kahan_abs_sum(present);
        let mut d2 = 0.0f64;
        for o in present {
            let term = C_DEV * jabs_f64(o.amp);
            d2 += term * term;
        }
        let d = d2.sqrt();
        let n = if d == 0.0 {
            0.0
        } else {
            (t * THIRD) / (d * SQRT2)
        };
        if self.normalize == Normalize::Legacy && n != 0.0 {
            let i_min = present[0].index - self.base_octave;
            let i_max = present[present.len() - 1].index - self.base_octave;
            let span = (i_max - i_min + 1) as f64;
            let n_p = self.base_amplitude * 0.5 * THIRD / (0.1 * (1.0 + 1.0 / span));
            (n_p, t * n_p / n)
        } else {
            (n, t)
        }
    }

    pub fn normalization_factor(&self, present: &[PresentOctave]) -> f64 {
        self.normalization(present).0
    }

    pub fn rho(&self, present: &[PresentOctave]) -> f32 {
        (self.normalization(present).1 * THIRD * 6.0) as f32
    }

    pub fn instantiate(
        &self,
        p: &PositionalFactory,
        key: &str,
    ) -> Result<OctaveStack, WorldgenError> {
        if key.is_empty() {
            return Err(WorldgenError::InlineNoiseWithoutKey);
        }
        let present = self.octaves_present();
        let (n_factor, t_eff) = self.normalization(&present);
        let rho = (t_eff * THIRD * 6.0) as f32;

        let mut r = p.from_hash_of(key);
        let f1 = r.fork_positional();
        let f2 = r.fork_positional();

        let mut layers = Vec::with_capacity(present.len() * 2);
        for oct in &present {
            let name = format!("octave_{}", oct.index);
            let mut s1 = f1.from_hash_of(&name);
            let b1 = GradientNoise::new(&mut s1);
            let mut s2 = f2.from_hash_of(&name);
            let b2 = GradientNoise::new(&mut s2);
            let alpha = (n_factor * oct.amp) as f32;
            layers.push(Octave::standard(b1, oct.freq, alpha));
            layers.push(Octave::standard(
                b2,
                oct.freq * SECOND_NOISE_FREQ_FACTOR,
                alpha,
            ));
        }

        Ok(OctaveStack::with_interval(layers, Interval::symmetric(rho)))
    }
}

fn kahan_abs_sum(present: &[PresentOctave]) -> f64 {
    let mut s = 0.0f64;
    let mut c = 0.0f64;
    let mut naive = 0.0f64;
    for o in present {
        let x = jabs_f64(o.amp);
        let y = x - c;
        let t = s + y;
        c = (t - s) - y;
        s = t;
        naive += x;
    }
    let r = s - c;
    if r.is_finite() { r } else { naive }
}

#[cfg(test)]
mod tests {
    use super::{NormalNoiseDef, Normalize};
    use crate::sources::positional_factory;

    fn def(
        base_amplitude: f64,
        base_octave: i32,
        octave_count: i32,
        mods: &[f64],
    ) -> NormalNoiseDef {
        NormalNoiseDef {
            base_amplitude,
            base_octave,
            octave_count,
            amplitude_modifiers: mods.to_vec(),
            normalize: Normalize::Enabled,
        }
    }

    struct TableVector {
        key: &'static str,
        rho_bits: u32,
        alpha_bits: &'static [u32],
    }

    const TABLE: &[TableVector] = &[
        TableVector {
            key: "minecraft:offset",
            rho_bits: 0x3fe0292e,
            alpha_bits: &[0x3f2aaaab, 0x3eaaaaab, 0x3e2aaaab],
        },
        TableVector {
            key: "minecraft:continentalness",
            rho_bits: 0x400a96f4,
            alpha_bits: &[
                0x3f406030, 0x3ec06030, 0x3ec06030, 0x3e406030, 0x3dc06030, 0x3cc06030, 0x3c406030,
                0x3bc06030, 0x3b406030,
            ],
        },
        TableVector {
            key: "minecraft:erosion",
            rho_bits: 0x3fed0e0c,
            alpha_bits: &[0x3f378336, 0x3eb78336, 0x3db78336, 0x3d378336],
        },
        TableVector {
            key: "minecraft:ridge",
            rho_bits: 0x4005cf4d,
            alpha_bits: &[0x3f228a29, 0x3f228a29, 0x3e228a29],
        },
        TableVector {
            key: "minecraft:jagged",
            rho_bits: 0x4004e75c,
            alpha_bits: &[
                0x3f48c992, 0x3ec8c992, 0x3e48c992, 0x3dc8c992, 0x3d48c992, 0x3cc8c992, 0x3c48c992,
                0x3bc8c992, 0x3b48c992, 0x3ac8c992, 0x3a48c992, 0x39c8c992, 0x3948c992, 0x38c8c992,
                0x3848c992, 0x37c8c992,
            ],
        },
        TableVector {
            key: "minecraft:temperature",
            rho_bits: 0x400db004,
            alpha_bits: &[0x3f73cf3d, 0x3e228a29],
        },
        TableVector {
            key: "minecraft:vegetation",
            rho_bits: 0x3fb93146,
            alpha_bits: &[0x3f107acf, 0x3e907acf],
        },
        TableVector {
            key: "minecraft:cave_cheese",
            rho_bits: 0x3fc0afae,
            alpha_bits: &[
                0x3ebe0030, 0x3ebe0030, 0x3ebe0030, 0x3dbe0030, 0x3dbe0030, 0x3cbe0030, 0x3c3e0030,
            ],
        },
        TableVector {
            key: "minecraft:cave_entrance",
            rho_bits: 0x3f5fd578,
            alpha_bits: &[0x3e924925, 0x3e36db6e, 0x3e36db6e],
        },
        TableVector {
            key: "minecraft:pillar",
            rho_bits: 0x3ff310ab,
            alpha_bits: &[0x3f3da12f, 0x3ebda12f],
        },
    ];

    #[test]
    fn table_alpha_per_octave_and_rho_bits() {
        let reg = crate::registry::NoiseRegistry::embedded().unwrap();
        for tv in TABLE {
            let d = reg.noises().get(tv.key).unwrap();
            let present = d.octaves_present();
            assert_eq!(
                present.len(),
                tv.alpha_bits.len(),
                "{} present octave count",
                tv.key
            );
            let n = d.normalization_factor(&present);
            assert_eq!(d.rho(&present).to_bits(), tv.rho_bits, "{} rho", tv.key);
            for (i, (oct, &want)) in present.iter().zip(tv.alpha_bits).enumerate() {
                let alpha = (n * oct.amp) as f32;
                assert_eq!(alpha.to_bits(), want, "{} alpha[{i}]", tv.key);
            }
        }
    }

    #[test]
    fn table_instantiated_layer_amplitudes_and_interval_bits() {
        let reg = crate::registry::NoiseRegistry::embedded().unwrap();
        let p = positional_factory(0, false);
        for tv in TABLE {
            let d = reg.noises().get(tv.key).unwrap();
            let stack = d.instantiate(&p, tv.key).unwrap();
            let layers = stack.layers();
            assert_eq!(
                layers.len(),
                tv.alpha_bits.len() * 2,
                "{} layer count",
                tv.key
            );
            for (i, &want) in tv.alpha_bits.iter().enumerate() {
                assert_eq!(
                    layers[2 * i].amp().to_bits(),
                    want,
                    "{} layer {}",
                    tv.key,
                    2 * i
                );
                assert_eq!(
                    layers[2 * i + 1].amp().to_bits(),
                    want,
                    "{} layer {}",
                    tv.key,
                    2 * i + 1
                );
            }
            assert_eq!(
                stack.interval().max.to_bits(),
                tv.rho_bits,
                "{} interval max",
                tv.key
            );
            assert_eq!(
                stack.interval().min.to_bits(),
                (-f32::from_bits(tv.rho_bits)).to_bits(),
                "{} interval min",
                tv.key
            );
        }
    }

    #[test]
    fn one_octave_normalization_factor() {
        let d = def(0.955388882960065, -3, 1, &[]);
        let present = d.octaves_present();
        let n = d.normalization_factor(&present);
        assert_eq!(n.to_bits(), 0.8722451644522289_f64.to_bits());
    }

    #[test]
    fn offset_normalization_factor() {
        let d = def(0.9381732587751005, -3, 4, &[1.0, 1.0, 1.0, 0.0]);
        let present = d.octaves_present();
        assert_eq!(present.len(), 3);
        let n = d.normalization_factor(&present);
        assert_eq!(n.to_bits(), 1.3323764968871814_f64.to_bits());
    }

    #[test]
    fn continentalness_normalization_factor() {
        let d = def(
            0.8880832896205223,
            -9,
            9,
            &[1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0],
        );
        let present = d.octaves_present();
        let n = d.normalization_factor(&present);
        assert_eq!(n.to_bits(), 1.6890307671940872_f64.to_bits());
    }

    #[test]
    fn base_amplitude_one_octave_alpha_and_rho() {
        let d = def(0.955388882960065, -3, 1, &[]);
        let p = positional_factory(0, false);
        let stack = d.instantiate(&p, "minecraft:aquifer_barrier").unwrap();
        let present = d.octaves_present();
        let alpha = (d.normalization_factor(&present) * present[0].amp) as f32;
        assert_eq!(alpha.to_bits(), (0.8333333134651184_f64 as f32).to_bits());
        assert_eq!(
            stack.layers()[0].amp().to_bits(),
            (0.8333333134651184_f64 as f32).to_bits()
        );
        let rho = d.rho(&present);
        assert_eq!(rho.to_bits(), (1.9107778072357178_f64 as f32).to_bits());
        assert_eq!(
            stack.interval().max.to_bits(),
            (1.9107778072357178_f64 as f32).to_bits()
        );
    }

    #[test]
    fn instantiate_doubles_present_octaves_and_offsets_second_frequency() {
        let d = def(0.9381732587751005, -3, 4, &[1.0, 1.0, 1.0, 0.0]);
        let p = positional_factory(0, false);
        let stack = d.instantiate(&p, "minecraft:offset").unwrap();
        assert_eq!(stack.layers().len(), 6);
        let layers = stack.layers();
        assert_eq!(layers[0].amp().to_bits(), layers[1].amp().to_bits());
        let ratio = layers[1].freq() / layers[0].freq();
        assert_eq!(ratio.to_bits(), super::SECOND_NOISE_FREQ_FACTOR.to_bits());
    }

    #[test]
    fn skipped_octaves_consume_no_layers() {
        let d = def(1.4659491761370222, -3, 4, &[1.0, 0.0, 0.0, 0.35]);
        let present = d.octaves_present();
        assert_eq!(present.len(), 2);
        assert_eq!(present[0].index, -3);
        assert_eq!(present[1].index, 0);
    }

    #[test]
    fn empty_modifiers_behaves_as_all_ones() {
        let d = def(0.939546696581729, -2, 4, &[]);
        let present = d.octaves_present();
        assert_eq!(present.len(), 4);
    }

    #[test]
    fn disabled_skips_prescale_but_still_applies_n() {
        let mut d = def(0.5, -3, 1, &[]);
        d.normalize = Normalize::Disabled;
        let present = d.octaves_present();
        assert_eq!(present[0].amp.to_bits(), 0.5_f64.to_bits());

        let t = super::kahan_abs_sum(&present);
        let d2: f64 = present
            .iter()
            .map(|o| {
                let term = super::C_DEV * o.amp.abs();
                term * term
            })
            .sum();
        let expected = (t * super::THIRD) / (d2.sqrt() * super::SQRT2);

        let n = d.normalization_factor(&present);
        assert_ne!(n.to_bits(), 1.0_f64.to_bits());
        assert_eq!(n.to_bits(), expected.to_bits());
    }

    #[test]
    fn legacy_replaces_n_and_t_with_prime() {
        let mut d = def(0.5, -3, 4, &[1.0, 0.0, 1.0, 1.0]);
        d.normalize = Normalize::Legacy;
        let present = d.octaves_present();
        assert_eq!(present.len(), 3);

        let t = super::kahan_abs_sum(&present);
        let d2: f64 = present
            .iter()
            .map(|o| {
                let term = super::C_DEV * o.amp.abs();
                term * term
            })
            .sum();
        let n_enabled = (t * super::THIRD) / (d2.sqrt() * super::SQRT2);

        let i_min = present[0].index - (-3);
        let i_max = present[present.len() - 1].index - (-3);
        let span = (i_max - i_min + 1) as f64;
        let n_p = 0.5 * 0.5 * super::THIRD / (0.1 * (1.0 + 1.0 / span));

        assert_eq!(d.normalization_factor(&present).to_bits(), n_p.to_bits());
        let t_eff = t * n_p / n_enabled;
        assert_eq!(
            d.rho(&present).to_bits(),
            ((t_eff * super::THIRD * 6.0) as f32).to_bits()
        );
    }
}
