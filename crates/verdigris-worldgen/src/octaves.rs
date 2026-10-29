use crate::arith::{jabs_f64, wrap};
use crate::gradient::GradientNoise;
use crate::interval::Interval;

#[derive(Clone, Debug)]
pub struct Octave {
    noise: GradientNoise,
    freq: f64,
    amp: f32,
    smear: Option<f64>,
}

impl Octave {
    pub fn standard(noise: GradientNoise, freq: f64, amp: f32) -> Self {
        Self {
            noise,
            freq,
            amp,
            smear: None,
        }
    }

    pub fn smeared(noise: GradientNoise, freq: f64, amp: f32, e: f64) -> Self {
        Self {
            noise,
            freq,
            amp,
            smear: Some(e),
        }
    }

    pub fn freq(&self) -> f64 {
        self.freq
    }

    pub fn amp(&self) -> f32 {
        self.amp
    }

    pub fn smear(&self) -> Option<f64> {
        self.smear
    }

    fn base_interval(&self) -> Interval {
        match self.smear {
            None => Interval::symmetric(2.0),
            Some(e) => Interval::symmetric((jabs_f64(e) + 2.0) as f32),
        }
    }

    fn evaluate(&self, x: f64, y: f64, z: f64) -> f32 {
        match self.smear {
            None => self.noise.sample(x, y, z),
            Some(e) => self.noise.sample_smeared(x, y, z, e),
        }
    }
}

#[derive(Clone, Debug)]
pub struct OctaveStack {
    layers: Vec<Octave>,
    interval: Interval,
}

impl OctaveStack {
    pub fn new(layers: Vec<Octave>) -> Self {
        let interval = layers.iter().fold(Interval::point(0.0), |acc, layer| {
            acc.plus(layer.base_interval().scale(layer.amp))
        });
        Self { layers, interval }
    }

    pub fn with_interval(layers: Vec<Octave>, interval: Interval) -> Self {
        Self { layers, interval }
    }

    pub fn layers(&self) -> &[Octave] {
        &self.layers
    }

    pub fn interval(&self) -> Interval {
        self.interval
    }

    pub fn sample_point(&self, x: f64, y: f64, z: f64, amp_global: Option<f32>) -> f32 {
        let mut sum = 0.0f32;
        for layer in &self.layers {
            let nu = layer.freq;
            let sx = wrap(x * nu);
            let sy = wrap(y * nu);
            let sz = wrap(z * nu);
            let v = layer.evaluate(sx, sy, sz);
            let a = match amp_global {
                Some(g) => g * layer.amp,
                None => layer.amp,
            };
            sum += a * v;
        }
        sum
    }

    pub fn sample_volume(
        &self,
        bx: i32,
        by: i32,
        bz: i32,
        sxz: f64,
        sy: f64,
        amp_global: Option<f32>,
    ) -> f32 {
        let mut sum = 0.0f32;
        for layer in &self.layers {
            let nu = layer.freq;
            let cx = (sxz * nu) * (bx as f64);
            let cy = (sy * nu) * (by as f64);
            let cz = (sxz * nu) * (bz as f64);
            let v = layer.evaluate(wrap(cx), wrap(cy), wrap(cz));
            let a = match amp_global {
                Some(g) => g * layer.amp,
                None => layer.amp,
            };
            sum += a * v;
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::{Octave, OctaveStack};
    use crate::gradient::GradientNoise;
    use crate::sources::positional_factory;

    fn grad(seed: i64, key: &str, octave: &str) -> GradientNoise {
        let p = positional_factory(seed, false);
        let mut r = p.from_hash_of(key);
        let f1 = r.fork_positional();
        let mut s = f1.from_hash_of(octave);
        GradientNoise::new(&mut s)
    }

    #[test]
    fn interval_sums_scaled_base_intervals() {
        let a = grad(1, "minecraft:offset", "octave_-3");
        let b = grad(1, "minecraft:offset", "octave_-2");
        let stack = OctaveStack::new(vec![
            Octave::standard(a, 1.0, 0.5),
            Octave::standard(b, 2.0, 0.25),
        ]);
        let iv = stack.interval();
        assert_eq!(iv.max.to_bits(), 1.5_f32.to_bits());
        assert_eq!(iv.min.to_bits(), (-1.5_f32).to_bits());
    }

    #[test]
    fn point_and_volume_agree_at_block_origin_when_scales_match() {
        let a = grad(7, "minecraft:offset", "octave_-3");
        let stack = OctaveStack::new(vec![Octave::standard(a, 1.0, 1.0)]);
        let p = stack.sample_point(0.0, 0.0, 0.0, None);
        let v = stack.sample_volume(0, 0, 0, 171.103, 85.5515, None);
        assert_eq!(p.to_bits(), v.to_bits());
    }

    #[test]
    fn amp_global_scales_every_layer() {
        let a = grad(3, "minecraft:offset", "octave_-3");
        let stack = OctaveStack::new(vec![Octave::standard(a, 1.0, 0.5)]);
        let base = stack.sample_point(2.0, 3.0, 4.0, None);
        let scaled = stack.sample_point(2.0, 3.0, 4.0, Some(2.0));
        assert_eq!((base * 2.0).to_bits(), scaled.to_bits());
    }
}
