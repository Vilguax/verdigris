use serde_json::Value;

use crate::lcg::Lcg;

#[derive(Debug, Clone, PartialEq)]
pub enum CarverConfigError {
    MissingField(String),
    BadType(String),
    OutOfRange(String),
}

impl std::fmt::Display for CarverConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(s) => write!(f, "carver config missing field: {s}"),
            Self::BadType(s) => write!(f, "carver config bad type: {s}"),
            Self::OutOfRange(s) => write!(f, "carver config out of range: {s}"),
        }
    }
}

impl std::error::Error for CarverConfigError {}

type Res<T> = Result<T, CarverConfigError>;

fn obj<'a>(v: &'a Value, who: &str) -> Res<&'a serde_json::Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| CarverConfigError::BadType(who.to_string()))
}

fn field<'a>(o: &'a serde_json::Map<String, Value>, name: &str) -> Res<&'a Value> {
    o.get(name)
        .ok_or_else(|| CarverConfigError::MissingField(name.to_string()))
}

fn f32_of(v: &Value, who: &str) -> Res<f32> {
    v.as_f64()
        .map(|x| x as f32)
        .ok_or_else(|| CarverConfigError::BadType(who.to_string()))
}

fn f64_of(v: &Value, who: &str) -> Res<f64> {
    v.as_f64()
        .ok_or_else(|| CarverConfigError::BadType(who.to_string()))
}

fn i32_of(v: &Value, who: &str) -> Res<i32> {
    v.as_i64()
        .and_then(|x| i32::try_from(x).ok())
        .ok_or_else(|| CarverConfigError::BadType(who.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatProvider {
    Constant(f32),
    Uniform { min: f32, max: f32 },
    Trapezoid { min: f32, max: f32, plateau: f32 },
}

impl FloatProvider {
    pub fn parse(v: &Value, who: &str) -> Res<Self> {
        if let Some(x) = v.as_f64() {
            return Ok(FloatProvider::Constant(x as f32));
        }
        let o = obj(v, who)?;
        let kind = field(o, "type")?
            .as_str()
            .ok_or_else(|| CarverConfigError::BadType(format!("{who}.type")))?;
        match kind {
            "minecraft:constant" => {
                let value = f32_of(field(o, "value")?, &format!("{who}.value"))?;
                Ok(FloatProvider::Constant(value))
            }
            "minecraft:uniform" => {
                let min = f32_of(field(o, "min_inclusive")?, &format!("{who}.min_inclusive"))?;
                let max = f32_of(field(o, "max_exclusive")?, &format!("{who}.max_exclusive"))?;
                if max <= min {
                    return Err(CarverConfigError::OutOfRange(format!(
                        "{who}: max_exclusive<=min_inclusive"
                    )));
                }
                Ok(FloatProvider::Uniform { min, max })
            }
            "minecraft:trapezoid" => {
                let min = f32_of(field(o, "min")?, &format!("{who}.min"))?;
                let max = f32_of(field(o, "max")?, &format!("{who}.max"))?;
                let plateau = f32_of(field(o, "plateau")?, &format!("{who}.plateau"))?;
                if max < min {
                    return Err(CarverConfigError::OutOfRange(format!("{who}: max<min")));
                }
                if plateau > max - min {
                    return Err(CarverConfigError::OutOfRange(format!(
                        "{who}: plateau>max-min"
                    )));
                }
                Ok(FloatProvider::Trapezoid { min, max, plateau })
            }
            other => Err(CarverConfigError::BadType(format!("{who}.type={other}"))),
        }
    }

    pub fn min_value(&self) -> f32 {
        match *self {
            FloatProvider::Constant(v) => v,
            FloatProvider::Uniform { min, .. } => min,
            FloatProvider::Trapezoid { min, .. } => min,
        }
    }

    pub fn max_value(&self) -> f32 {
        match *self {
            FloatProvider::Constant(v) => v,
            FloatProvider::Uniform { max, .. } => max,
            FloatProvider::Trapezoid { max, .. } => max,
        }
    }

    pub fn sample(&self, r: &mut Lcg) -> f32 {
        match *self {
            FloatProvider::Constant(v) => v,
            FloatProvider::Uniform { min, max } => min + r.next_float() * (max - min),
            FloatProvider::Trapezoid { min, max, plateau } => {
                let span = max - min;
                let rise = (span - plateau) / 2.0;
                let flat = span - rise;
                min + r.next_float() * flat + r.next_float() * rise
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntProvider {
    Constant(i32),
    VeryBiasedToBottom { min: i32, max: i32 },
}

impl IntProvider {
    pub fn parse(v: &Value, who: &str) -> Res<Self> {
        if let Some(x) = v.as_i64() {
            return Ok(IntProvider::Constant(i32::try_from(x).map_err(|_| {
                CarverConfigError::OutOfRange(format!("{who}: int"))
            })?));
        }
        let o = obj(v, who)?;
        let kind = field(o, "type")?
            .as_str()
            .ok_or_else(|| CarverConfigError::BadType(format!("{who}.type")))?;
        match kind {
            "minecraft:constant" => Ok(IntProvider::Constant(i32_of(
                field(o, "value")?,
                &format!("{who}.value"),
            )?)),
            "minecraft:very_biased_to_bottom" => {
                let min = i32_of(field(o, "min_inclusive")?, &format!("{who}.min_inclusive"))?;
                let max = i32_of(field(o, "max_inclusive")?, &format!("{who}.max_inclusive"))?;
                if max < min {
                    return Err(CarverConfigError::OutOfRange(format!(
                        "{who}: max_inclusive<min_inclusive"
                    )));
                }
                Ok(IntProvider::VeryBiasedToBottom { min, max })
            }
            other => Err(CarverConfigError::BadType(format!("{who}.type={other}"))),
        }
    }

    pub fn min_value(&self) -> i32 {
        match *self {
            IntProvider::Constant(v) => v,
            IntProvider::VeryBiasedToBottom { min, .. } => min,
        }
    }

    pub fn sample(&self, r: &mut Lcg) -> i32 {
        match *self {
            IntProvider::Constant(v) => v,
            IntProvider::VeryBiasedToBottom { min, max } => {
                let span = max - min + 1;
                let a = r.next_int_bounded(span);
                let b = r.next_int_bounded(a + 1);
                let c = r.next_int_bounded(b + 1);
                min + c
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl VerticalAnchor {
    pub fn parse(v: &Value, who: &str) -> Res<Self> {
        let o = obj(v, who)?;
        if let Some(x) = o.get("absolute") {
            return Ok(VerticalAnchor::Absolute(i32_of(
                x,
                &format!("{who}.absolute"),
            )?));
        }
        if let Some(x) = o.get("above_bottom") {
            return Ok(VerticalAnchor::AboveBottom(i32_of(
                x,
                &format!("{who}.above_bottom"),
            )?));
        }
        if let Some(x) = o.get("below_top") {
            return Ok(VerticalAnchor::BelowTop(i32_of(
                x,
                &format!("{who}.below_top"),
            )?));
        }
        Err(CarverConfigError::MissingField(format!("{who}.anchor")))
    }

    pub fn resolve(&self, min_y: i32, max_y: i32) -> i32 {
        match *self {
            VerticalAnchor::Absolute(n) => n,
            VerticalAnchor::AboveBottom(n) => min_y + n,
            VerticalAnchor::BelowTop(n) => max_y - n,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightProvider {
    pub min: VerticalAnchor,
    pub max: VerticalAnchor,
}

impl HeightProvider {
    pub fn parse(v: &Value, who: &str) -> Res<Self> {
        let o = obj(v, who)?;
        let min = VerticalAnchor::parse(field(o, "min_inclusive")?, &format!("{who}.min"))?;
        let max = VerticalAnchor::parse(field(o, "max_inclusive")?, &format!("{who}.max"))?;
        Ok(HeightProvider { min, max })
    }

    pub fn sample(&self, r: &mut Lcg, min_y: i32, max_y: i32) -> i32 {
        let lo = self.min.resolve(min_y, max_y);
        let hi = self.max.resolve(min_y, max_y);
        if hi < lo {
            lo
        } else {
            lo + r.next_int_bounded(hi - lo + 1)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaveConfig {
    pub probability: f32,
    pub count: IntProvider,
    pub y: HeightProvider,
    pub horizontal_radius_multiplier: FloatProvider,
    pub vertical_radius_multiplier: FloatProvider,
    pub floor_level: FloatProvider,
    pub room_vertical_radius_multiplier: FloatProvider,
    pub thickness: FloatProvider,
    pub weird_thickness_bias: bool,
    pub start_vertical_radius_multiplier: f32,
}

impl CaveConfig {
    pub fn parse(v: &Value) -> Res<Self> {
        let o = obj(v, "cave")?;
        let probability = f32_of(field(o, "probability")?, "cave.probability")?;
        if !(0.0..=1.0).contains(&probability) {
            return Err(CarverConfigError::OutOfRange(
                "cave.probability".to_string(),
            ));
        }
        let count = IntProvider::parse(field(o, "count")?, "cave.count")?;
        if count.min_value() < 0 {
            return Err(CarverConfigError::OutOfRange("cave.count".to_string()));
        }
        let thickness = FloatProvider::parse(field(o, "thickness")?, "cave.thickness")?;
        if thickness.min_value() < 0.0 {
            return Err(CarverConfigError::OutOfRange("cave.thickness".to_string()));
        }
        let floor_level = FloatProvider::parse(field(o, "floor_level")?, "cave.floor_level")?;
        if floor_level.min_value() < -1.0 || floor_level.max_value() > 1.0 {
            return Err(CarverConfigError::OutOfRange(
                "cave.floor_level".to_string(),
            ));
        }
        Ok(CaveConfig {
            probability,
            count,
            y: HeightProvider::parse(field(o, "y")?, "cave.y")?,
            horizontal_radius_multiplier: FloatProvider::parse(
                field(o, "horizontal_radius_multiplier")?,
                "cave.horizontal_radius_multiplier",
            )?,
            vertical_radius_multiplier: FloatProvider::parse(
                field(o, "vertical_radius_multiplier")?,
                "cave.vertical_radius_multiplier",
            )?,
            floor_level,
            room_vertical_radius_multiplier: FloatProvider::parse(
                field(o, "room_vertical_radius_multiplier")?,
                "cave.room_vertical_radius_multiplier",
            )?,
            thickness,
            weird_thickness_bias: o
                .get("weird_thickness_bias")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            start_vertical_radius_multiplier: o
                .get("start_vertical_radius_multiplier")
                .map(|v| f32_of(v, "cave.start_vertical_radius_multiplier"))
                .transpose()?
                .unwrap_or(1.0),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanyonConfig {
    pub probability: f32,
    pub y: HeightProvider,
    pub vertical_rotation: FloatProvider,
    pub thickness: FloatProvider,
    pub width_smoothness: i32,
    pub horizontal_radius_factor: FloatProvider,
    pub vertical_radius_default_factor: f32,
    pub vertical_radius_center_factor: f32,
    pub distance_factor: FloatProvider,
    pub y_scale: f32,
}

impl CanyonConfig {
    pub fn parse(v: &Value) -> Res<Self> {
        let o = obj(v, "canyon")?;
        let probability = f32_of(field(o, "probability")?, "canyon.probability")?;
        if !(0.0..=1.0).contains(&probability) {
            return Err(CarverConfigError::OutOfRange(
                "canyon.probability".to_string(),
            ));
        }
        let shape = obj(field(o, "shape")?, "canyon.shape")?;
        let width_smoothness = i32_of(
            field(shape, "width_smoothness")?,
            "canyon.shape.width_smoothness",
        )?;
        if width_smoothness <= 0 {
            return Err(CarverConfigError::OutOfRange(
                "canyon.shape.width_smoothness".to_string(),
            ));
        }
        Ok(CanyonConfig {
            probability,
            y: HeightProvider::parse(field(o, "y")?, "canyon.y")?,
            vertical_rotation: FloatProvider::parse(
                field(o, "vertical_rotation")?,
                "canyon.vertical_rotation",
            )?,
            thickness: FloatProvider::parse(field(shape, "thickness")?, "canyon.shape.thickness")?,
            width_smoothness,
            horizontal_radius_factor: FloatProvider::parse(
                field(shape, "horizontal_radius_factor")?,
                "canyon.shape.horizontal_radius_factor",
            )?,
            vertical_radius_default_factor: f32_of(
                field(shape, "vertical_radius_default_factor")?,
                "canyon.shape.vertical_radius_default_factor",
            )?,
            vertical_radius_center_factor: f32_of(
                field(shape, "vertical_radius_center_factor")?,
                "canyon.shape.vertical_radius_center_factor",
            )?,
            distance_factor: FloatProvider::parse(
                field(shape, "distance_factor")?,
                "canyon.shape.distance_factor",
            )?,
            y_scale: f64_of(field(shape, "y_scale")?, "canyon.shape.y_scale")? as f32,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CarverKind {
    Cave(CaveConfig),
    Canyon(CanyonConfig),
}

impl CarverKind {
    pub fn parse(v: &Value) -> Res<Self> {
        let o = obj(v, "carver")?;
        let kind = field(o, "type")?
            .as_str()
            .ok_or_else(|| CarverConfigError::BadType("carver.type".to_string()))?;
        match kind {
            "minecraft:cave" => Ok(CarverKind::Cave(CaveConfig::parse(v)?)),
            "minecraft:canyon" => Ok(CarverKind::Canyon(CanyonConfig::parse(v)?)),
            other => Err(CarverConfigError::BadType(format!("carver.type={other}"))),
        }
    }

    pub fn probability(&self) -> f32 {
        match self {
            CarverKind::Cave(c) => c.probability,
            CarverKind::Canyon(c) => c.probability,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uniform_sample_is_min_plus_fraction() {
        let p = FloatProvider::Uniform { min: 0.7, max: 1.4 };
        let mut r = Lcg::from_seed(1);
        let v = p.sample(&mut r);
        assert!((0.7..1.4).contains(&v));
    }

    #[test]
    fn trapezoid_rejects_plateau_too_large() {
        let v = json!({"type":"minecraft:trapezoid","min":0.0,"max":3.0,"plateau":4.0});
        assert!(matches!(
            FloatProvider::parse(&v, "t"),
            Err(CarverConfigError::OutOfRange(_))
        ));
    }

    #[test]
    fn uniform_rejects_inverted_bounds() {
        let v = json!({"type":"minecraft:uniform","min_inclusive":1.4,"max_exclusive":0.7});
        assert!(matches!(
            FloatProvider::parse(&v, "u"),
            Err(CarverConfigError::OutOfRange(_))
        ));
    }

    #[test]
    fn anchor_resolution() {
        assert_eq!(VerticalAnchor::Absolute(180).resolve(-63, 312), 180);
        assert_eq!(VerticalAnchor::AboveBottom(8).resolve(-63, 312), -55);
        assert_eq!(VerticalAnchor::BelowTop(1).resolve(-63, 312), 311);
    }

    #[test]
    fn height_uniform_no_draw_when_inverted() {
        let h = HeightProvider {
            min: VerticalAnchor::Absolute(100),
            max: VerticalAnchor::Absolute(10),
        };
        let mut r = Lcg::from_seed(0);
        assert_eq!(h.sample(&mut r, -63, 312), 100);
    }
}
