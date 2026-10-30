use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::blended::{BlendedParams, OldBlendedNoise};
use crate::df::node::{
    Axis, Gradient, Metric, Node, Noise, Spline, SplinePoint, SplineValue, Tiling,
};
use crate::octaves::OctaveStack;
use crate::sources::PositionalFactory;

pub const F32_CONST_LIMIT: f32 = 1_000_000.0;
pub const MAX_INLINE_DEPTH: usize = 512;

#[derive(Clone, Debug, PartialEq)]
pub enum BuildError {
    NotADensityFunction(String),
    MissingField {
        ty: String,
        field: String,
    },
    BadField {
        ty: String,
        field: String,
        reason: String,
    },
    UnknownType(String),
    MissingReference(String),
    Cycle(String),
    ConstantOutOfRange(f64),
    UnknownNoise(String),
    DepthExceeded,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADensityFunction(s) => write!(f, "not a density function: {s}"),
            Self::MissingField { ty, field } => write!(f, "missing field {field} in {ty}"),
            Self::BadField { ty, field, reason } => {
                write!(f, "bad field {field} in {ty}: {reason}")
            }
            Self::UnknownType(t) => write!(f, "unknown density function type: {t}"),
            Self::MissingReference(r) => write!(f, "missing density_function reference: {r}"),
            Self::Cycle(r) => write!(f, "cyclic density_function reference: {r}"),
            Self::ConstantOutOfRange(v) => write!(f, "f32 constant out of range: {v}"),
            Self::UnknownNoise(n) => write!(f, "unknown noise reference: {n}"),
            Self::DepthExceeded => write!(f, "density function inline depth exceeded"),
        }
    }
}

impl std::error::Error for BuildError {}

pub struct Assets {
    pub noises: HashMap<String, Arc<OctaveStack>>,
    pub blended_params: BlendedParams,
    pub blended: Arc<OldBlendedNoise>,
    pub factory: PositionalFactory,
}

pub struct Builder<'a> {
    functions: &'a Map<String, Value>,
    assets: &'a Assets,
    stack: Vec<String>,
    pub minmax_eliminated: usize,
}

fn normalize_id(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

fn full_key(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

impl<'a> Builder<'a> {
    pub fn new(functions: &'a Map<String, Value>, assets: &'a Assets) -> Self {
        Self {
            functions,
            assets,
            stack: Vec::new(),
            minmax_eliminated: 0,
        }
    }

    pub fn build(&mut self, value: &Value) -> Result<Node, BuildError> {
        if self.stack.len() > MAX_INLINE_DEPTH {
            return Err(BuildError::DepthExceeded);
        }
        match value {
            Value::Number(_) => Ok(Node::Const(self.read_f32_value(value, "constant")?)),
            Value::String(s) => self.build_reference(s),
            Value::Object(obj) => self.build_object(obj),
            _ => Err(BuildError::NotADensityFunction(value.to_string())),
        }
    }

    fn build_reference(&mut self, id: &str) -> Result<Node, BuildError> {
        let key = full_key(id);
        if self.stack.iter().any(|k| k == &key) {
            return Err(BuildError::Cycle(key));
        }
        let target = self
            .functions
            .get(&key)
            .ok_or_else(|| BuildError::MissingReference(key.clone()))?;
        self.stack.push(key);
        let result = self.build(target);
        self.stack.pop();
        result
    }

    fn noise(&self, id: &str) -> Result<Arc<OctaveStack>, BuildError> {
        let key = full_key(id);
        self.assets
            .noises
            .get(&key)
            .cloned()
            .ok_or(BuildError::UnknownNoise(key))
    }

    fn build_object(&mut self, obj: &Map<String, Value>) -> Result<Node, BuildError> {
        let ty = obj
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| BuildError::NotADensityFunction("object without type".to_string()))?;
        let id = normalize_id(ty).to_string();
        match id.as_str() {
            "constant" => {
                let v = self.field(obj, &id, "value")?;
                Ok(Node::Const(self.read_f32_value(v, &id)?))
            }
            "add" => {
                let (l, r) = self.binary(obj, &id)?;
                Ok(Node::Add(Box::new(l), Box::new(r)))
            }
            "sub" => {
                let (l, r) = self.binary(obj, &id)?;
                Ok(Node::Sub(Box::new(l), Box::new(r)))
            }
            "mul" => {
                let (l, r) = self.binary(obj, &id)?;
                Ok(build_mul(l, r))
            }
            "div" => {
                let (l, r) = self.binary(obj, &id)?;
                Ok(build_div(l, r))
            }
            "min" => {
                let (l, r) = self.binary(obj, &id)?;
                Ok(self.build_min(l, r))
            }
            "max" => {
                let (l, r) = self.binary(obj, &id)?;
                Ok(self.build_max(l, r))
            }
            "abs" => Ok(Node::Abs(Box::new(self.input(obj, &id)?))),
            "square" => Ok(Node::Square(Box::new(self.input(obj, &id)?))),
            "cube" => Ok(Node::Cube(Box::new(self.input(obj, &id)?))),
            "half_negative" => Ok(Node::HalfNegative(Box::new(self.input(obj, &id)?))),
            "quarter_negative" => Ok(Node::QuarterNegative(Box::new(self.input(obj, &id)?))),
            "sqrt" => Ok(Node::Sqrt(Box::new(self.input(obj, &id)?))),
            "reciprocal" => Ok(Node::Reciprocal(Box::new(self.input(obj, &id)?))),
            "negate" => Ok(Node::Negate(Box::new(self.input(obj, &id)?))),
            "squeeze" => Ok(Node::Squeeze(Box::new(self.input(obj, &id)?))),
            "clamp" => {
                let input = self.input(obj, &id)?;
                let min = self.read_f32(obj, &id, "min")?;
                let max = self.read_f32(obj, &id, "max")?;
                Ok(Node::Clamp {
                    input: Box::new(input),
                    min,
                    max,
                })
            }
            "gradient" => self.build_gradient(obj, &id),
            "lerp" => {
                let alpha = self.sub(obj, &id, "alpha")?;
                let first = self.sub(obj, &id, "first")?;
                let second = self.sub(obj, &id, "second")?;
                Ok(Node::Lerp {
                    alpha: Box::new(alpha),
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
            "range_choice" => {
                let input = self.input(obj, &id)?;
                let min_inclusive = self.read_f32(obj, &id, "min_inclusive")?;
                let max_exclusive = self.read_f32(obj, &id, "max_exclusive")?;
                let in_range = self.sub(obj, &id, "when_in_range")?;
                let out_range = self.sub(obj, &id, "when_out_of_range")?;
                Ok(Node::RangeChoice {
                    input: Box::new(input),
                    min_inclusive,
                    max_exclusive,
                    in_range: Box::new(in_range),
                    out_range: Box::new(out_range),
                })
            }
            "interval_select" => self.build_interval_select(obj, &id),
            "spline" => {
                let spline_value = self.field(obj, &id, "spline")?.clone();
                Ok(Node::Spline(self.build_spline(&spline_value, &id)?))
            }
            "noise" => self.build_noise(obj, &id),
            "shift_a" => Ok(Node::ShiftA(self.noise(self.noise_ref(obj, &id)?)?)),
            "shift_b" => Ok(Node::ShiftB(self.noise(self.noise_ref(obj, &id)?)?)),
            "shift" => Ok(Node::Shift(self.noise(self.noise_ref(obj, &id)?)?)),
            "old_blended_noise" => self.build_old_blended(obj, &id),
            "blend_alpha" => Ok(Node::BlendAlpha),
            "blend_offset" => Ok(Node::BlendOffset),
            "blend_density" => Ok(Node::BlendDensity(Box::new(self.input(obj, &id)?))),
            "beardifier" => Ok(Node::Beardifier),
            "cache" | "flat_cache" | "cache_2d" | "cache_once" | "cache_all_in_cell" => {
                Ok(Node::Cache(Box::new(self.input(obj, &id)?)))
            }
            "interpolated" => self.build_interpolated(obj, &id),
            "slice" => self.build_slice(obj, &id),
            "find_top_surface" => self.build_find_top_surface(obj, &id),
            "distance_to_point" => self.build_distance_to_point(obj, &id),
            "end_outer_islands" => Ok(Node::EndOuterIslands),
            other => Err(BuildError::UnknownType(other.to_string())),
        }
    }

    fn field<'b>(
        &self,
        obj: &'b Map<String, Value>,
        ty: &str,
        name: &str,
    ) -> Result<&'b Value, BuildError> {
        obj.get(name).ok_or_else(|| BuildError::MissingField {
            ty: ty.to_string(),
            field: name.to_string(),
        })
    }

    fn binary(&mut self, obj: &Map<String, Value>, ty: &str) -> Result<(Node, Node), BuildError> {
        let l = self.sub(obj, ty, "left")?;
        let r = self.sub(obj, ty, "right")?;
        Ok((l, r))
    }

    fn input(&mut self, obj: &Map<String, Value>, ty: &str) -> Result<Node, BuildError> {
        self.sub(obj, ty, "input")
    }

    fn sub(&mut self, obj: &Map<String, Value>, ty: &str, name: &str) -> Result<Node, BuildError> {
        let v = self.field(obj, ty, name)?.clone();
        self.build(&v)
    }

    fn noise_ref<'b>(&self, obj: &'b Map<String, Value>, ty: &str) -> Result<&'b str, BuildError> {
        self.field(obj, ty, "noise")?
            .as_str()
            .ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: "noise".to_string(),
                reason: "expected string reference".to_string(),
            })
    }

    fn read_f32(&self, obj: &Map<String, Value>, ty: &str, name: &str) -> Result<f32, BuildError> {
        let v = self.field(obj, ty, name)?;
        self.read_f32_value(v, ty)
    }

    fn read_f32_value(&self, v: &Value, ty: &str) -> Result<f32, BuildError> {
        let d = v.as_f64().ok_or_else(|| BuildError::BadField {
            ty: ty.to_string(),
            field: "<number>".to_string(),
            reason: "expected number".to_string(),
        })?;
        if !(-(F32_CONST_LIMIT as f64)..=F32_CONST_LIMIT as f64).contains(&d) {
            return Err(BuildError::ConstantOutOfRange(d));
        }
        Ok(d as f32)
    }

    fn read_f64(&self, obj: &Map<String, Value>, ty: &str, name: &str) -> Result<f64, BuildError> {
        self.field(obj, ty, name)?
            .as_f64()
            .ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: name.to_string(),
                reason: "expected number".to_string(),
            })
    }

    fn read_i32(&self, obj: &Map<String, Value>, ty: &str, name: &str) -> Result<i32, BuildError> {
        let n = self
            .field(obj, ty, name)?
            .as_i64()
            .ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: name.to_string(),
                reason: "expected integer".to_string(),
            })?;
        i32::try_from(n).map_err(|_| BuildError::BadField {
            ty: ty.to_string(),
            field: name.to_string(),
            reason: "integer out of i32 range".to_string(),
        })
    }

    fn build_min(&mut self, left: Node, right: Node) -> Node {
        let li = left.interval();
        let ri = right.interval();
        if !li.is_indef() && !ri.is_indef() && li.max < ri.min {
            self.minmax_eliminated += 1;
            return left;
        }
        if !li.is_indef() && !ri.is_indef() && ri.max < li.min {
            self.minmax_eliminated += 1;
            return right;
        }
        if let Some(c) = right.as_const() {
            return Node::MinConst {
                input: Box::new(left),
                constant: c,
            };
        }
        if let Some(c) = left.as_const() {
            return Node::MinConst {
                input: Box::new(right),
                constant: c,
            };
        }
        Node::Min {
            left: Box::new(left),
            right: Box::new(right),
            right_interval: ri,
        }
    }

    fn build_max(&mut self, left: Node, right: Node) -> Node {
        let li = left.interval();
        let ri = right.interval();
        if !li.is_indef() && !ri.is_indef() && li.min > ri.max {
            self.minmax_eliminated += 1;
            return left;
        }
        if !li.is_indef() && !ri.is_indef() && ri.min > li.max {
            self.minmax_eliminated += 1;
            return right;
        }
        if let Some(c) = right.as_const() {
            return Node::MaxConst {
                input: Box::new(left),
                constant: c,
            };
        }
        if let Some(c) = left.as_const() {
            return Node::MaxConst {
                input: Box::new(right),
                constant: c,
            };
        }
        Node::Max {
            left: Box::new(left),
            right: Box::new(right),
            right_interval: ri,
        }
    }

    fn build_gradient(&self, obj: &Map<String, Value>, ty: &str) -> Result<Node, BuildError> {
        let axis = parse_axis(self.field(obj, ty, "axis")?, ty)?;
        let tiling = match obj.get("tiling").and_then(Value::as_str) {
            None | Some("clamp_to_edge") => Tiling::ClampToEdge,
            Some("repeat") => Tiling::Repeat,
            Some("mirrored_repeat") => Tiling::MirroredRepeat,
            Some(other) => {
                return Err(BuildError::BadField {
                    ty: ty.to_string(),
                    field: "tiling".to_string(),
                    reason: format!("unknown tiling {other}"),
                });
            }
        };
        let from_coordinate = self.read_i32(obj, ty, "from_coordinate")?;
        let to_coordinate = self.read_i32(obj, ty, "to_coordinate")?;
        let from_value = self.read_f32(obj, ty, "from_value")?;
        let to_value = self.read_f32(obj, ty, "to_value")?;
        let span = to_coordinate - from_coordinate;
        let slope = (to_value - from_value) / (span as f32);
        Ok(Node::Gradient(Gradient {
            axis,
            tiling,
            from_coordinate,
            to_coordinate,
            from_value,
            to_value,
            span,
            slope,
        }))
    }

    fn build_interval_select(
        &mut self,
        obj: &Map<String, Value>,
        ty: &str,
    ) -> Result<Node, BuildError> {
        let input = self.input(obj, ty)?;
        let thresholds = self
            .field(obj, ty, "thresholds")?
            .as_array()
            .ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: "thresholds".to_string(),
                reason: "expected array".to_string(),
            })?
            .iter()
            .map(|v| self.read_f32_value(v, ty))
            .collect::<Result<Vec<_>, _>>()?;
        let functions_values = self
            .field(obj, ty, "functions")?
            .as_array()
            .ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: "functions".to_string(),
                reason: "expected array".to_string(),
            })?
            .clone();
        let mut functions = Vec::with_capacity(functions_values.len());
        for v in &functions_values {
            functions.push(self.build(v)?);
        }
        if functions.is_empty() {
            return Err(BuildError::BadField {
                ty: ty.to_string(),
                field: "functions".to_string(),
                reason: "expected at least one function".to_string(),
            });
        }
        if functions.len() != thresholds.len() + 1 {
            return Err(BuildError::BadField {
                ty: ty.to_string(),
                field: "functions".to_string(),
                reason: format!(
                    "expected functions.len() == thresholds.len() + 1 ({} != {} + 1)",
                    functions.len(),
                    thresholds.len()
                ),
            });
        }
        Ok(Node::IntervalSelect {
            input: Box::new(input),
            thresholds,
            functions,
        })
    }

    fn build_spline(&mut self, spline: &Value, ty: &str) -> Result<Spline, BuildError> {
        let obj = spline.as_object().ok_or_else(|| BuildError::BadField {
            ty: ty.to_string(),
            field: "spline".to_string(),
            reason: "expected spline object".to_string(),
        })?;
        let coordinate = self.sub(obj, ty, "coordinate")?;
        let points_values = self
            .field(obj, ty, "points")?
            .as_array()
            .ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: "points".to_string(),
                reason: "expected array".to_string(),
            })?
            .clone();
        let mut points = Vec::with_capacity(points_values.len());
        for p in &points_values {
            let po = p.as_object().ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: "points".to_string(),
                reason: "expected point object".to_string(),
            })?;
            let location = self.read_f32(po, ty, "location")?;
            let derivative = self.read_f32(po, ty, "derivative")?;
            let value_field = self.field(po, ty, "value")?;
            let value = match value_field {
                Value::Number(_) => SplineValue::Const(self.read_f32_value(value_field, ty)?),
                Value::Object(_) => {
                    SplineValue::Nested(Box::new(self.build_spline(value_field, ty)?))
                }
                _ => {
                    return Err(BuildError::BadField {
                        ty: ty.to_string(),
                        field: "value".to_string(),
                        reason: "expected number or nested spline".to_string(),
                    });
                }
            };
            points.push(SplinePoint {
                location,
                value,
                derivative,
            });
        }
        if points.is_empty() {
            return Err(BuildError::BadField {
                ty: ty.to_string(),
                field: "points".to_string(),
                reason: "expected at least one point".to_string(),
            });
        }
        Ok(Spline {
            coordinate: Box::new(coordinate),
            points,
        })
    }

    fn build_noise(&mut self, obj: &Map<String, Value>, ty: &str) -> Result<Node, BuildError> {
        let stack = self.noise(self.noise_ref(obj, ty)?)?;
        let xz_scale = self.read_f64(obj, ty, "xz_scale")?;
        let y_scale = self.read_f64(obj, ty, "y_scale")?;
        let shift_x = self.optional_shift(obj, ty, "shift_x")?;
        let shift_y = self.optional_shift(obj, ty, "shift_y")?;
        let shift_z = self.optional_shift(obj, ty, "shift_z")?;
        Ok(Node::Noise(Noise {
            stack,
            xz_scale,
            y_scale,
            shift_x,
            shift_y,
            shift_z,
        }))
    }

    fn optional_shift(
        &mut self,
        obj: &Map<String, Value>,
        _ty: &str,
        name: &str,
    ) -> Result<Option<Box<Node>>, BuildError> {
        match obj.get(name).cloned() {
            None => Ok(None),
            Some(v) => {
                let node = self.build(&v)?;
                if matches!(node.as_const(), Some(c) if c == 0.0) {
                    Ok(None)
                } else {
                    Ok(Some(Box::new(node)))
                }
            }
        }
    }

    fn build_old_blended(&self, obj: &Map<String, Value>, ty: &str) -> Result<Node, BuildError> {
        let params = BlendedParams {
            xz_scale: self.read_f64(obj, ty, "xz_scale")?,
            y_scale: self.read_f64(obj, ty, "y_scale")?,
            xz_factor: self.read_f64(obj, ty, "xz_factor")?,
            y_factor: self.read_f64(obj, ty, "y_factor")?,
            smear_scale_multiplier: self.read_f64(obj, ty, "smear_scale_multiplier")?,
        };
        if same_blended(&params, &self.assets.blended_params) {
            Ok(Node::OldBlended(self.assets.blended.clone()))
        } else {
            Ok(Node::OldBlended(Arc::new(OldBlendedNoise::new(
                &self.assets.factory,
                &params,
            ))))
        }
    }

    fn build_interpolated(
        &mut self,
        obj: &Map<String, Value>,
        ty: &str,
    ) -> Result<Node, BuildError> {
        let input = self.input(obj, ty)?;
        let cell_size_xz = self.read_i32(obj, ty, "cell_size_xz")?;
        let cell_size_y = self.read_i32(obj, ty, "cell_size_y")?;
        if cell_size_xz <= 0 || cell_size_y <= 0 {
            return Err(BuildError::BadField {
                ty: ty.to_string(),
                field: "cell_size".to_string(),
                reason: "must be positive".to_string(),
            });
        }
        Ok(Node::Interpolated {
            input: Box::new(input),
            cell_size_xz,
            cell_size_y,
            inv_xz: 1.0 / (cell_size_xz as f32),
            inv_y: 1.0 / (cell_size_y as f32),
        })
    }

    fn build_slice(&mut self, obj: &Map<String, Value>, ty: &str) -> Result<Node, BuildError> {
        let axis = parse_axis(self.field(obj, ty, "axis")?, ty)?;
        let coordinate = self.read_i32(obj, ty, "coordinate")?;
        let input = self.input(obj, ty)?;
        Ok(Node::Slice {
            axis,
            coordinate,
            input: Box::new(input),
        })
    }

    fn build_find_top_surface(
        &mut self,
        obj: &Map<String, Value>,
        ty: &str,
    ) -> Result<Node, BuildError> {
        let density = self.sub(obj, ty, "density")?;
        let upper_bound = self.sub(obj, ty, "upper_bound")?;
        let lower_bound = self.read_i32(obj, ty, "lower_bound")?;
        let cell_height = self.read_i32(obj, ty, "cell_height")?;
        if cell_height <= 0 {
            return Err(BuildError::BadField {
                ty: ty.to_string(),
                field: "cell_height".to_string(),
                reason: "must be positive".to_string(),
            });
        }
        let ub = upper_bound.interval();
        let lower_f = lower_bound as f32;
        let hi = crate::arith::jmax_f32(lower_f, ub.max);
        let interval = crate::interval::Interval::new(lower_f, hi);
        Ok(Node::FindTopSurface {
            density: Box::new(density),
            upper_bound: Box::new(upper_bound),
            lower_bound,
            cell_height,
            interval,
        })
    }

    fn build_distance_to_point(
        &self,
        obj: &Map<String, Value>,
        ty: &str,
    ) -> Result<Node, BuildError> {
        let arr = self
            .field(obj, ty, "point")?
            .as_array()
            .ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: "point".to_string(),
                reason: "expected array".to_string(),
            })?;
        if arr.len() != 3 {
            return Err(BuildError::BadField {
                ty: ty.to_string(),
                field: "point".to_string(),
                reason: "expected 3 coordinates".to_string(),
            });
        }
        let mut point = [0.0f64; 3];
        for (i, slot) in point.iter_mut().enumerate() {
            *slot = arr[i].as_f64().ok_or_else(|| BuildError::BadField {
                ty: ty.to_string(),
                field: "point".to_string(),
                reason: "expected numbers".to_string(),
            })?;
        }
        let metric = match obj.get("metric").and_then(Value::as_str) {
            None | Some("euclidean") => Metric::Euclidean,
            Some("euclidean_squared") => Metric::EuclideanSquared,
            Some("manhattan") => Metric::Manhattan,
            Some("chebyshev") => Metric::Chebyshev,
            Some(other) => {
                return Err(BuildError::BadField {
                    ty: ty.to_string(),
                    field: "metric".to_string(),
                    reason: format!("unknown metric {other}"),
                });
            }
        };
        Ok(Node::DistanceToPoint { point, metric })
    }
}

fn build_mul(left: Node, right: Node) -> Node {
    if let Some(c) = right.as_const() {
        return Node::MulConst {
            input: Box::new(left),
            factor: c,
        };
    }
    if let Some(c) = left.as_const() {
        return Node::MulConst {
            input: Box::new(right),
            factor: c,
        };
    }
    Node::Mul(Box::new(left), Box::new(right))
}

fn build_div(left: Node, right: Node) -> Node {
    if let Some(c) = right.as_const() {
        return Node::DivConst {
            input: Box::new(left),
            reciprocal: 1.0 / c,
        };
    }
    Node::Div(Box::new(left), Box::new(right))
}

fn same_blended(a: &BlendedParams, b: &BlendedParams) -> bool {
    a.xz_scale.to_bits() == b.xz_scale.to_bits()
        && a.y_scale.to_bits() == b.y_scale.to_bits()
        && a.xz_factor.to_bits() == b.xz_factor.to_bits()
        && a.y_factor.to_bits() == b.y_factor.to_bits()
        && a.smear_scale_multiplier.to_bits() == b.smear_scale_multiplier.to_bits()
}

fn parse_axis(v: &Value, ty: &str) -> Result<Axis, BuildError> {
    match v.as_str() {
        Some("x") => Ok(Axis::X),
        Some("y") => Ok(Axis::Y),
        Some("z") => Ok(Axis::Z),
        _ => Err(BuildError::BadField {
            ty: ty.to_string(),
            field: "axis".to_string(),
            reason: "expected x, y or z".to_string(),
        }),
    }
}
