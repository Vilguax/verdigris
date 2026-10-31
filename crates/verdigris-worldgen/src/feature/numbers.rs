use serde_json::Value;

use crate::feature::error::{FeatureError, strip_ns};
use crate::feature::rng::FeatureRng;

#[derive(Clone, Copy, Debug)]
pub struct HeightCtx {
    pub min_y: i32,
    pub height: i32,
}

impl HeightCtx {
    pub fn max_y(&self) -> i32 {
        self.min_y.wrapping_add(self.height).wrapping_sub(1)
    }

    pub fn resolve(&self, anchor: &Value) -> Result<i32, FeatureError> {
        let obj = anchor
            .as_object()
            .ok_or_else(|| FeatureError::BadField("vertical_anchor".to_string()))?;
        if let Some(k) = obj.get("absolute").and_then(Value::as_i64) {
            Ok(k as i32)
        } else if let Some(k) = obj.get("above_bottom").and_then(Value::as_i64) {
            Ok(self.min_y.wrapping_add(k as i32))
        } else if let Some(k) = obj.get("below_top").and_then(Value::as_i64) {
            Ok(self.max_y().wrapping_sub(k as i32))
        } else {
            Err(FeatureError::BadField("vertical_anchor kind".to_string()))
        }
    }
}

fn random_between_inclusive(rng: &mut FeatureRng, min: i32, max: i32) -> i32 {
    min.wrapping_add(rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1)))
}

#[derive(Clone, Debug)]
pub enum IntProvider {
    Constant(i32),
    Uniform {
        min: i32,
        max: i32,
    },
    BiasedToBottom {
        min: i32,
        max: i32,
    },
    Clamped {
        inner: Box<IntProvider>,
        min: i32,
        max: i32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: i32,
        max: i32,
    },
    Trapezoid {
        min: i32,
        max: i32,
        plateau: i32,
    },
    WeightedList {
        entries: Vec<(i32, IntProvider)>,
    },
}

impl IntProvider {
    pub fn parse(v: &Value) -> Result<IntProvider, FeatureError> {
        if let Some(n) = v.as_i64() {
            return Ok(IntProvider::Constant(n as i32));
        }
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("int_provider".to_string()))?;
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .unwrap_or("constant");
        match ty {
            "constant" => {
                let value = obj
                    .get("value")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| FeatureError::MissingField("value".to_string()))?;
                Ok(IntProvider::Constant(value as i32))
            }
            "uniform" => Ok(IntProvider::Uniform {
                min: int_field(obj, "min_inclusive")?,
                max: int_field(obj, "max_inclusive")?,
            }),
            "biased_to_bottom" => Ok(IntProvider::BiasedToBottom {
                min: int_field(obj, "min_inclusive")?,
                max: int_field(obj, "max_inclusive")?,
            }),
            "clamped" => Ok(IntProvider::Clamped {
                inner: Box::new(IntProvider::parse(
                    obj.get("source")
                        .ok_or_else(|| FeatureError::MissingField("source".to_string()))?,
                )?),
                min: int_field(obj, "min_inclusive")?,
                max: int_field(obj, "max_inclusive")?,
            }),
            "clamped_normal" => Ok(IntProvider::ClampedNormal {
                mean: float_field(obj, "mean")?,
                deviation: float_field(obj, "deviation")?,
                min: int_field(obj, "min_inclusive")?,
                max: int_field(obj, "max_inclusive")?,
            }),
            "trapezoid" => Ok(IntProvider::Trapezoid {
                min: int_field(obj, "min")?,
                max: int_field(obj, "max")?,
                plateau: obj
                    .get("plateau")
                    .and_then(Value::as_i64)
                    .map(|v| v as i32)
                    .unwrap_or(0),
            }),
            "weighted_list" => Ok(IntProvider::WeightedList {
                entries: parse_weighted(obj, IntProvider::parse)?,
            }),
            other => Err(FeatureError::UnknownType {
                namespace: "int_provider".to_string(),
                name: other.to_string(),
            }),
        }
    }

    pub fn sample(&self, rng: &mut FeatureRng) -> i32 {
        match self {
            IntProvider::Constant(v) => *v,
            IntProvider::Uniform { min, max } => random_between_inclusive(rng, *min, *max),
            IntProvider::BiasedToBottom { min, max } => {
                let span = max.wrapping_sub(*min).wrapping_add(1);
                let a = rng.next_int_bounded(span);
                let b = rng.next_int_bounded(a.wrapping_add(1));
                min.wrapping_add(b)
            }
            IntProvider::Clamped { inner, min, max } => inner.sample(rng).clamp(*min, *max),
            IntProvider::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let g = f64::from(*mean) + rng.next_gaussian() * f64::from(*deviation);
                let r = g.round() as i32;
                r.clamp(*min, *max)
            }
            IntProvider::Trapezoid { min, max, plateau } => {
                sample_trapezoid(rng, *min, *max, *plateau)
            }
            IntProvider::WeightedList { entries } => {
                let idx = weighted_pick(rng, entries);
                entries[idx].1.sample(rng)
            }
        }
    }
}

fn sample_trapezoid(rng: &mut FeatureRng, min: i32, max: i32, plateau: i32) -> i32 {
    let span = max.wrapping_sub(min);
    let lower = span.wrapping_sub(plateau) / 2;
    let upper = span.wrapping_sub(lower);
    min.wrapping_add(random_between_inclusive(rng, 0, upper))
        .wrapping_add(random_between_inclusive(rng, 0, lower))
}

#[derive(Clone, Debug)]
pub enum HeightProvider {
    Constant(i32),
    Uniform { min: i32, max: i32 },
    BiasedToBottom { min: i32, max: i32 },
    VeryBiasedToBottom { min: i32, max: i32, inner: i32 },
    Trapezoid { min: i32, max: i32, plateau: i32 },
    WeightedList { entries: Vec<(i32, HeightProvider)> },
}

impl HeightProvider {
    pub fn parse(v: &Value, ctx: &HeightCtx) -> Result<HeightProvider, FeatureError> {
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("height_provider".to_string()))?;
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .unwrap_or("uniform");
        match ty {
            "constant" => Ok(HeightProvider::Constant(
                ctx.resolve(
                    obj.get("value")
                        .ok_or_else(|| FeatureError::MissingField("value".to_string()))?,
                )?,
            )),
            "uniform" => Ok(HeightProvider::Uniform {
                min: anchor_field(obj, "min_inclusive", ctx)?,
                max: anchor_field(obj, "max_inclusive", ctx)?,
            }),
            "biased_to_bottom" => Ok(HeightProvider::BiasedToBottom {
                min: anchor_field(obj, "min_inclusive", ctx)?,
                max: anchor_field(obj, "max_inclusive", ctx)?,
            }),
            "very_biased_to_bottom" => Ok(HeightProvider::VeryBiasedToBottom {
                min: anchor_field(obj, "min_inclusive", ctx)?,
                max: anchor_field(obj, "max_inclusive", ctx)?,
                inner: obj
                    .get("inner")
                    .and_then(Value::as_i64)
                    .map(|v| v as i32)
                    .unwrap_or(1),
            }),
            "trapezoid" => Ok(HeightProvider::Trapezoid {
                min: anchor_field(obj, "min_inclusive", ctx)?,
                max: anchor_field(obj, "max_inclusive", ctx)?,
                plateau: obj
                    .get("plateau")
                    .and_then(Value::as_i64)
                    .map(|v| v as i32)
                    .unwrap_or(0),
            }),
            "weighted_list" => {
                let arr = obj
                    .get("distribution")
                    .and_then(Value::as_array)
                    .ok_or_else(|| FeatureError::MissingField("distribution".to_string()))?;
                let mut entries = Vec::with_capacity(arr.len());
                for e in arr {
                    let eo = e
                        .as_object()
                        .ok_or_else(|| FeatureError::BadField("distribution entry".to_string()))?;
                    let weight = eo
                        .get("weight")
                        .and_then(Value::as_i64)
                        .ok_or_else(|| FeatureError::MissingField("weight".to_string()))?
                        as i32;
                    let data = eo
                        .get("data")
                        .ok_or_else(|| FeatureError::MissingField("data".to_string()))?;
                    entries.push((weight, HeightProvider::parse(data, ctx)?));
                }
                Ok(HeightProvider::WeightedList { entries })
            }
            other => Err(FeatureError::UnknownType {
                namespace: "height_provider".to_string(),
                name: other.to_string(),
            }),
        }
    }

    pub fn sample(&self, rng: &mut FeatureRng) -> i32 {
        match self {
            HeightProvider::Constant(v) => *v,
            HeightProvider::Uniform { min, max } => random_between_inclusive(rng, *min, *max),
            HeightProvider::BiasedToBottom { min, max } => {
                let span = max.wrapping_sub(*min).wrapping_add(1);
                let a = rng.next_int_bounded(span);
                let b = rng.next_int_bounded(a.wrapping_add(1));
                min.wrapping_add(b)
            }
            HeightProvider::VeryBiasedToBottom { min, max, inner } => {
                let span = max.wrapping_sub(*min).wrapping_sub(*inner).wrapping_add(1);
                if span <= 0 {
                    return *min;
                }
                let a = rng.next_int_bounded(span);
                let b = rng.next_int_bounded(a.wrapping_add(*inner).wrapping_add(1));
                min.wrapping_add(b)
            }
            HeightProvider::Trapezoid { min, max, plateau } => {
                sample_trapezoid(rng, *min, *max, *plateau)
            }
            HeightProvider::WeightedList { entries } => {
                let idx = weighted_pick(rng, entries);
                entries[idx].1.sample(rng)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum FloatProvider {
    Constant(f32),
    Uniform {
        min: f32,
        max: f32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    },
    Trapezoid {
        min: f32,
        max: f32,
        plateau: f32,
    },
}

impl FloatProvider {
    pub fn parse(v: &Value) -> Result<FloatProvider, FeatureError> {
        if let Some(n) = v.as_f64() {
            return Ok(FloatProvider::Constant(n as f32));
        }
        let obj = v
            .as_object()
            .ok_or_else(|| FeatureError::BadField("float_provider".to_string()))?;
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .map(strip_ns)
            .unwrap_or("constant");
        match ty {
            "constant" => Ok(FloatProvider::Constant(float_field(obj, "value")?)),
            "uniform" => Ok(FloatProvider::Uniform {
                min: float_field(obj, "min_inclusive")?,
                max: float_field(obj, "max_exclusive")?,
            }),
            "clamped_normal" => Ok(FloatProvider::ClampedNormal {
                mean: float_field(obj, "mean")?,
                deviation: float_field(obj, "deviation")?,
                min: float_field(obj, "min")?,
                max: float_field(obj, "max")?,
            }),
            "trapezoid" => Ok(FloatProvider::Trapezoid {
                min: float_field(obj, "min")?,
                max: float_field(obj, "max")?,
                plateau: float_field(obj, "plateau")?,
            }),
            other => Err(FeatureError::UnknownType {
                namespace: "float_provider".to_string(),
                name: other.to_string(),
            }),
        }
    }

    pub fn sample(&self, rng: &mut FeatureRng) -> f32 {
        match self {
            FloatProvider::Constant(v) => *v,
            FloatProvider::Uniform { min, max } => min + rng.next_float() * (max - min),
            FloatProvider::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let g = mean + rng.next_gaussian() as f32 * deviation;
                g.clamp(*min, *max)
            }
            FloatProvider::Trapezoid { min, max, plateau } => {
                let span = max - min;
                let base = span - plateau;
                min + rng.next_float() * base / 2.0 + rng.next_float() * (plateau + base / 2.0)
            }
        }
    }
}

fn weighted_pick<T>(rng: &mut FeatureRng, entries: &[(i32, T)]) -> usize {
    let total: i32 = entries.iter().map(|(w, _)| *w).sum();
    if total <= 0 {
        return 0;
    }
    let mut roll = rng.next_int_bounded(total);
    for (i, (w, _)) in entries.iter().enumerate() {
        roll -= *w;
        if roll < 0 {
            return i;
        }
    }
    entries.len() - 1
}

fn int_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<i32, FeatureError> {
    obj.get(key)
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .ok_or_else(|| FeatureError::MissingField(key.to_string()))
}

fn float_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<f32, FeatureError> {
    obj.get(key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .ok_or_else(|| FeatureError::MissingField(key.to_string()))
}

fn anchor_field(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    ctx: &HeightCtx,
) -> Result<i32, FeatureError> {
    let v = obj
        .get(key)
        .ok_or_else(|| FeatureError::MissingField(key.to_string()))?;
    ctx.resolve(v)
}

fn parse_weighted<T>(
    obj: &serde_json::Map<String, Value>,
    mut parse_one: impl FnMut(&Value) -> Result<T, FeatureError>,
) -> Result<Vec<(i32, T)>, FeatureError> {
    let arr = obj
        .get("distribution")
        .and_then(Value::as_array)
        .ok_or_else(|| FeatureError::MissingField("distribution".to_string()))?;
    let mut entries = Vec::with_capacity(arr.len());
    for e in arr {
        let eo = e
            .as_object()
            .ok_or_else(|| FeatureError::BadField("distribution entry".to_string()))?;
        let weight = eo
            .get("weight")
            .and_then(Value::as_i64)
            .ok_or_else(|| FeatureError::MissingField("weight".to_string()))?
            as i32;
        let data = eo
            .get("data")
            .ok_or_else(|| FeatureError::MissingField("data".to_string()))?;
        entries.push((weight, parse_one(data)?));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trapezoid_min_max_zero_consumes_two_draws_and_is_zero() {
        let mut r = FeatureRng::new(42);
        let before = r.clone();
        let v = sample_trapezoid(&mut r, 0, 0, 0);
        assert_eq!(v, 0);
        let mut check = before;
        check.next_int_bounded(1);
        check.next_int_bounded(1);
        assert_eq!(r.next_bits(32), check.next_bits(32));
    }

    #[test]
    fn uniform_int_draws_once() {
        let p = IntProvider::Uniform { min: 3, max: 7 };
        let mut r = FeatureRng::new(1);
        let v = p.sample(&mut r);
        assert!((3..=7).contains(&v));
    }

    #[test]
    fn clamped_normal_uses_gaussian() {
        let p = IntProvider::ClampedNormal {
            mean: 0.0,
            deviation: 1.0,
            min: -2,
            max: 2,
        };
        let mut r = FeatureRng::new(7);
        let v = p.sample(&mut r);
        assert!((-2..=2).contains(&v));
    }
}
