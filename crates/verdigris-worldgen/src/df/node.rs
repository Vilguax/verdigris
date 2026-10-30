use std::sync::Arc;

use crate::blended::OldBlendedNoise;
use crate::interval::Interval;
use crate::octaves::OctaveStack;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tiling {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

#[derive(Clone, Debug)]
pub struct Gradient {
    pub axis: Axis,
    pub tiling: Tiling,
    pub from_coordinate: i32,
    pub to_coordinate: i32,
    pub from_value: f32,
    pub to_value: f32,
    pub span: i32,
    pub slope: f32,
}

#[derive(Clone, Debug)]
pub struct Noise {
    pub stack: Arc<OctaveStack>,
    pub xz_scale: f64,
    pub y_scale: f64,
    pub shift_x: Option<Box<Node>>,
    pub shift_y: Option<Box<Node>>,
    pub shift_z: Option<Box<Node>>,
}

#[derive(Clone, Debug)]
pub enum SplineValue {
    Const(f32),
    Nested(Box<Spline>),
}

#[derive(Clone, Debug)]
pub struct SplinePoint {
    pub location: f32,
    pub value: SplineValue,
    pub derivative: f32,
}

#[derive(Clone, Debug)]
pub struct Spline {
    pub coordinate: Box<Node>,
    pub points: Vec<SplinePoint>,
}

#[derive(Clone, Debug)]
pub enum Node {
    Const(f32),
    Add(Box<Node>, Box<Node>),
    Sub(Box<Node>, Box<Node>),
    Mul(Box<Node>, Box<Node>),
    MulConst {
        input: Box<Node>,
        factor: f32,
    },
    Div(Box<Node>, Box<Node>),
    DivConst {
        input: Box<Node>,
        reciprocal: f32,
    },
    Min {
        left: Box<Node>,
        right: Box<Node>,
        right_interval: Interval,
    },
    Max {
        left: Box<Node>,
        right: Box<Node>,
        right_interval: Interval,
    },
    MinConst {
        input: Box<Node>,
        constant: f32,
    },
    MaxConst {
        input: Box<Node>,
        constant: f32,
    },
    Abs(Box<Node>),
    Square(Box<Node>),
    Cube(Box<Node>),
    HalfNegative(Box<Node>),
    QuarterNegative(Box<Node>),
    Sqrt(Box<Node>),
    Reciprocal(Box<Node>),
    Negate(Box<Node>),
    Squeeze(Box<Node>),
    Clamp {
        input: Box<Node>,
        min: f32,
        max: f32,
    },
    Gradient(Gradient),
    Lerp {
        alpha: Box<Node>,
        first: Box<Node>,
        second: Box<Node>,
    },
    RangeChoice {
        input: Box<Node>,
        min_inclusive: f32,
        max_exclusive: f32,
        in_range: Box<Node>,
        out_range: Box<Node>,
    },
    IntervalSelect {
        input: Box<Node>,
        thresholds: Vec<f32>,
        functions: Vec<Node>,
    },
    Spline(Spline),
    Noise(Noise),
    ShiftA(Arc<OctaveStack>),
    ShiftB(Arc<OctaveStack>),
    Shift(Arc<OctaveStack>),
    OldBlended(Arc<OldBlendedNoise>),
    BlendAlpha,
    BlendOffset,
    BlendDensity(Box<Node>),
    Beardifier,
    Cache(Box<Node>),
    Interpolated {
        input: Box<Node>,
        cell_size_xz: i32,
        cell_size_y: i32,
        inv_xz: f32,
        inv_y: f32,
    },
    Slice {
        axis: Axis,
        coordinate: i32,
        input: Box<Node>,
    },
    FindTopSurface {
        density: Box<Node>,
        upper_bound: Box<Node>,
        lower_bound: i32,
        cell_height: i32,
        interval: Interval,
    },
    DistanceToPoint {
        point: [f64; 3],
        metric: Metric,
    },
    EndOuterIslands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Euclidean,
    EuclideanSquared,
    Manhattan,
    Chebyshev,
}

impl Node {
    pub fn as_const(&self) -> Option<f32> {
        match self {
            Node::Const(c) => Some(*c),
            _ => None,
        }
    }

    pub fn find_top_surface_parts(&self) -> Option<(&Node, &Node, i32, i32)> {
        match self {
            Node::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
                ..
            } => Some((density, upper_bound, *lower_bound, *cell_height)),
            Node::Cache(inner) => inner.find_top_surface_parts(),
            _ => None,
        }
    }

    pub fn interval(&self) -> Interval {
        match self {
            Node::Const(c) => Interval::point(*c),
            Node::Add(l, r) => l.interval().iadd(r.interval()),
            Node::Sub(l, r) => l.interval().isub(r.interval()),
            Node::Mul(l, r) => l.interval().imul(r.interval()),
            Node::MulConst { input, factor } => input.interval().imul(Interval::point(*factor)),
            Node::Div(l, r) => l.interval().idiv(r.interval()),
            Node::DivConst { input, reciprocal } => {
                input.interval().imul(Interval::point(*reciprocal))
            }
            Node::Min { left, right, .. } => left.interval().imin(right.interval()),
            Node::Max { left, right, .. } => left.interval().imax(right.interval()),
            Node::MinConst { input, constant } => input.interval().imin(Interval::point(*constant)),
            Node::MaxConst { input, constant } => input.interval().imax(Interval::point(*constant)),
            Node::Abs(x) => x.interval().iabs(),
            Node::Square(x) => x.interval().isquare(),
            Node::Cube(x) => x.interval().monotone(|v| (v * v) * v),
            Node::HalfNegative(x) => x.interval().monotone(|v| if v > 0.0 { v } else { v * 0.5 }),
            Node::QuarterNegative(x) => x
                .interval()
                .monotone(|v| if v > 0.0 { v } else { v * 0.25 }),
            Node::Squeeze(x) => x.interval().monotone(squeeze_value),
            Node::Sqrt(x) => x.interval().monotone(|v| (f64::from(v).sqrt()) as f32),
            Node::Reciprocal(x) => x.interval().recip(),
            Node::Negate(x) => x.interval().imul(Interval::point(-1.0)),
            Node::Clamp { input, min, max } => input.interval().clamp(*min, *max),
            Node::Gradient(g) => Interval::point(g.from_value).hull(Interval::point(g.to_value)),
            Node::Lerp {
                alpha,
                first,
                second,
            } => Interval::lerp(alpha.interval(), first.interval(), second.interval()),
            Node::RangeChoice {
                in_range,
                out_range,
                ..
            } => in_range.interval().hull(out_range.interval()),
            Node::IntervalSelect { functions, .. } => {
                let mut acc = Interval::INDEF;
                for f in functions {
                    acc = acc.hull(f.interval());
                }
                acc
            }
            Node::Spline(s) => spline_interval(s),
            Node::Noise(n) => n.stack.interval(),
            Node::ShiftA(s) | Node::ShiftB(s) | Node::Shift(s) => {
                s.interval().imul(Interval::new(4.0, 4.0))
            }
            Node::OldBlended(b) => b.interval(),
            Node::BlendAlpha => Interval::new(0.0, 1.0),
            Node::BlendOffset | Node::Beardifier => Interval::new(f32::NEG_INFINITY, f32::INFINITY),
            Node::BlendDensity(x) | Node::Cache(x) | Node::Slice { input: x, .. } => x.interval(),
            Node::Interpolated { input, .. } => input.interval(),
            Node::FindTopSurface { interval, .. } => *interval,
            Node::DistanceToPoint { .. } => Interval::new(0.0, f32::INFINITY),
            Node::EndOuterIslands => Interval::new(f32::NEG_INFINITY, f32::INFINITY),
        }
    }
}

pub fn squeeze_value(x: f32) -> f32 {
    let c = crate::arith::jclamp_f32(x, -1.0, 1.0);
    (c / 2.0) - (((c * c) * c) / 24.0)
}

pub fn extrapolate(u: f32, w: f32, location: f32, derivative: f32) -> f32 {
    if derivative == 0.0 {
        w
    } else {
        w + derivative * (u - location)
    }
}

pub fn spline_interval(spline: &Spline) -> Interval {
    let coord = spline.coordinate.interval();
    if coord.is_indef() {
        return Interval::INDEF;
    }
    let n = spline.points.len();
    let ranges: Vec<Interval> = spline
        .points
        .iter()
        .map(|p| match &p.value {
            SplineValue::Const(c) => Interval::point(*c),
            SplineValue::Nested(s) => spline_interval(s),
        })
        .collect();

    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;

    if coord.min < spline.points[0].location {
        let x0 = spline.points[0].location;
        let d0 = spline.points[0].derivative;
        let e1 = extrapolate(coord.min, ranges[0].min, x0, d0);
        let e2 = extrapolate(coord.min, ranges[0].max, x0, d0);
        lo = crate::arith::jmin_f32(lo, crate::arith::jmin_f32(e1, e2));
        hi = crate::arith::jmax_f32(hi, crate::arith::jmax_f32(e1, e2));
    }
    if coord.max > spline.points[n - 1].location {
        let xl = spline.points[n - 1].location;
        let dl = spline.points[n - 1].derivative;
        let e1 = extrapolate(coord.max, ranges[n - 1].min, xl, dl);
        let e2 = extrapolate(coord.max, ranges[n - 1].max, xl, dl);
        lo = crate::arith::jmin_f32(lo, crate::arith::jmin_f32(e1, e2));
        hi = crate::arith::jmax_f32(hi, crate::arith::jmax_f32(e1, e2));
    }
    for r in &ranges {
        lo = crate::arith::jmin_f32(lo, r.min);
        hi = crate::arith::jmax_f32(hi, r.max);
    }
    for i in 0..n.saturating_sub(1) {
        let d1 = spline.points[i].derivative;
        let d2 = spline.points[i + 1].derivative;
        if d1 != 0.0 || d2 != 0.0 {
            let delta = spline.points[i + 1].location - spline.points[i].location;
            let p1 = d1 * delta;
            let p2 = d2 * delta;
            let (m1, big1) = (ranges[i].min, ranges[i].max);
            let (m2, big2) = (ranges[i + 1].min, ranges[i + 1].max);
            let a_min = (p1 - big2) + m1;
            let a_max = (p1 - m2) + big1;
            let b_min = (-p2 + m2) - big1;
            let b_max = (-p2 + big2) - m1;
            let base_lo = crate::arith::jmin_f32(m1, m2);
            let base_hi = crate::arith::jmax_f32(big1, big2);
            lo = crate::arith::jmin_f32(lo, base_lo + 0.25 * crate::arith::jmin_f32(a_min, b_min));
            hi = crate::arith::jmax_f32(hi, base_hi + 0.25 * crate::arith::jmax_f32(a_max, b_max));
        }
    }
    Interval::new(lo, hi)
}
