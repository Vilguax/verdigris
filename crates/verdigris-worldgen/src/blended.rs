use crate::arith::{jclamp_f32, l};
use crate::gradient::GradientNoise;
use crate::interval::Interval;
use crate::octaves::{Octave, OctaveStack};
use crate::sources::{PositionalFactory, RootSource};

pub const NOISE_SCALE: f64 = 684.412;

pub const SCALE_MIN: f64 = 0.001;
pub const SCALE_MAX: f64 = 1000.0;
pub const SMEAR_MIN: f64 = 1.0;
pub const SMEAR_MAX: f64 = 8.0;

pub const LIMIT_OCTAVE: i32 = -15;
pub const MAIN_OCTAVE: i32 = -7;
pub const LIMIT_V: f64 = 0.9999847412109375;
pub const MAIN_V: f64 = 12.75;

#[derive(Clone, Copy, Debug)]
pub struct BlendedParams {
    pub xz_scale: f64,
    pub y_scale: f64,
    pub xz_factor: f64,
    pub y_factor: f64,
    pub smear_scale_multiplier: f64,
}

#[derive(Clone, Debug)]
pub struct OldBlendedNoise {
    lo: OctaveStack,
    hi: OctaveStack,
    main: OctaveStack,
    m_xz: f64,
    m_y: f64,
    main_sxz: f64,
    main_sy: f64,
    interval: Interval,
}

fn fbm_stack(r: &mut RootSource, q: i32, e_base: f64, v: f64) -> OctaveStack {
    let k = -q + 1;
    let mut vf = v / ((2.0f64).powi(k) - 1.0);
    let mut nu = 1.0f64;
    let mut layers = Vec::with_capacity(k as usize);
    for _ in 0..k {
        let b = GradientNoise::new(r);
        let e = e_base * nu;
        layers.push(Octave::smeared(b, nu, vf as f32, e));
        nu /= 2.0;
        vf *= 2.0;
    }
    OctaveStack::new(layers)
}

impl OldBlendedNoise {
    pub fn new(p: &PositionalFactory, params: &BlendedParams) -> Self {
        let m_xz = NOISE_SCALE * params.xz_scale;
        let m_y = NOISE_SCALE * params.y_scale;
        let e_lim = m_y * params.smear_scale_multiplier;
        let e_main = e_lim / params.y_factor;
        let main_sxz = m_xz / params.xz_factor;
        let main_sy = m_y / params.y_factor;

        let mut r = p.from_hash_of("minecraft:terrain");
        let lo = fbm_stack(&mut r, LIMIT_OCTAVE, e_lim, LIMIT_V);
        let hi = fbm_stack(&mut r, LIMIT_OCTAVE, e_lim, LIMIT_V);
        let main = fbm_stack(&mut r, MAIN_OCTAVE, e_main, MAIN_V);

        let interval = lo.interval();

        Self {
            lo,
            hi,
            main,
            m_xz,
            m_y,
            main_sxz,
            main_sy,
            interval,
        }
    }

    pub fn interval(&self) -> Interval {
        self.interval
    }

    pub fn limit(&self) -> &OctaveStack {
        &self.lo
    }

    pub fn main(&self) -> &OctaveStack {
        &self.main
    }

    pub fn scales(&self) -> (f64, f64, f64, f64) {
        (self.m_xz, self.m_y, self.main_sxz, self.main_sy)
    }

    #[allow(clippy::float_cmp)]
    pub fn sample_point(&self, x: f64, y: f64, z: f64) -> f32 {
        let m =
            self.main
                .sample_point(x * self.main_sxz, y * self.main_sy, z * self.main_sxz, None);
        let t = jclamp_f32(m + 0.5, 0.0, 1.0);
        if t == 0.0 {
            self.lo
                .sample_point(x * self.m_xz, y * self.m_y, z * self.m_xz, None)
        } else if t == 1.0 {
            self.hi
                .sample_point(x * self.m_xz, y * self.m_y, z * self.m_xz, None)
        } else {
            let lo = self
                .lo
                .sample_point(x * self.m_xz, y * self.m_y, z * self.m_xz, None);
            let hi = self
                .hi
                .sample_point(x * self.m_xz, y * self.m_y, z * self.m_xz, None);
            l(t, lo, hi)
        }
    }

    #[allow(clippy::float_cmp)]
    pub fn sample_volume(&self, bx: i32, by: i32, bz: i32) -> f32 {
        let lo = self.lo.sample_volume(bx, by, bz, self.m_xz, self.m_y, None);
        let hi = self.hi.sample_volume(bx, by, bz, self.m_xz, self.m_y, None);
        let m = self
            .main
            .sample_volume(bx, by, bz, self.main_sxz, self.main_sy, None);
        let t = jclamp_f32(m + 0.5, 0.0, 1.0);
        if t == 0.0 {
            lo
        } else if t == 1.0 {
            hi
        } else {
            l(t, lo, hi)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BlendedParams, OldBlendedNoise};
    use crate::sources::positional_factory;

    fn overworld_params() -> BlendedParams {
        BlendedParams {
            xz_scale: 0.25,
            y_scale: 0.125,
            xz_factor: 80.0,
            y_factor: 160.0,
            smear_scale_multiplier: 8.0,
        }
    }

    #[test]
    fn derived_scales_match_plan() {
        let p = positional_factory(20261029, false);
        let b = OldBlendedNoise::new(&p, &overworld_params());
        let (m_xz, m_y, main_sxz, main_sy) = b.scales();
        assert_eq!(m_xz.to_bits(), 171.103_f64.to_bits());
        assert_eq!(m_y.to_bits(), 85.5515_f64.to_bits());
        assert_eq!(main_sxz.to_bits(), (171.103_f64 / 80.0).to_bits());
        assert_eq!(main_sy.to_bits(), (85.5515_f64 / 160.0).to_bits());
    }

    #[test]
    fn limit_stack_amplitudes_are_powers_of_two() {
        let p = positional_factory(20261029, false);
        let b = OldBlendedNoise::new(&p, &overworld_params());
        let layers = b.limit().layers();
        assert_eq!(layers.len(), 16);
        for (i, layer) in layers.iter().enumerate() {
            let expected = (2.0f64).powi(-16 + i as i32) as f32;
            assert_eq!(layer.amp().to_bits(), expected.to_bits());
        }
    }

    #[test]
    fn main_stack_amplitudes_match_plan() {
        let p = positional_factory(20261029, false);
        let b = OldBlendedNoise::new(&p, &overworld_params());
        let layers = b.main().layers();
        assert_eq!(layers.len(), 8);
        for (i, layer) in layers.iter().enumerate() {
            let expected = (0.05f64 * (2.0f64).powi(i as i32)) as f32;
            assert_eq!(layer.amp().to_bits(), expected.to_bits());
        }
    }

    #[test]
    fn limit_smear_tracks_frequency() {
        let p = positional_factory(20261029, false);
        let b = OldBlendedNoise::new(&p, &overworld_params());
        let layers = b.limit().layers();
        let e0 = layers[0].smear().unwrap();
        assert_eq!(e0.to_bits(), 684.412_f64.to_bits());
    }

    #[test]
    fn declared_interval_matches_plan() {
        let p = positional_factory(20261029, false);
        let b = OldBlendedNoise::new(&p, &overworld_params());
        let iv = b.interval();
        assert_eq!(iv.max.to_bits(), (2.167062282562256_f64 as f32).to_bits());
        assert_eq!(iv.min.to_bits(), (-2.167062282562256_f64 as f32).to_bits());
    }

    #[test]
    fn point_and_volume_agree_at_block_origin() {
        let p = positional_factory(20261029, false);
        let b = OldBlendedNoise::new(&p, &overworld_params());
        let point = b.sample_point(0.0, 0.0, 0.0);
        let volume = b.sample_volume(0, 0, 0);
        assert_eq!(point.to_bits(), volume.to_bits());
    }
}
