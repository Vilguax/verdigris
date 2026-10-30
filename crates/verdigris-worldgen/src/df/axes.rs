use crate::df::node::{Axis, Node, Spline, SplineValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axes(u8);

impl Axes {
    pub const X: u8 = 1;
    pub const Y: u8 = 2;
    pub const Z: u8 = 4;

    pub fn empty() -> Self {
        Axes(0)
    }

    pub fn all() -> Self {
        Axes(Self::X | Self::Y | Self::Z)
    }

    pub fn of(bits: u8) -> Self {
        Axes(bits)
    }

    pub fn has_x(self) -> bool {
        self.0 & Self::X != 0
    }

    pub fn has_y(self) -> bool {
        self.0 & Self::Y != 0
    }

    pub fn has_z(self) -> bool {
        self.0 & Self::Z != 0
    }

    pub fn union(self, o: Axes) -> Axes {
        Axes(self.0 | o.0)
    }

    pub fn without(self, axis: Axis) -> Axes {
        let bit = match axis {
            Axis::X => Self::X,
            Axis::Y => Self::Y,
            Axis::Z => Self::Z,
        };
        Axes(self.0 & !bit)
    }

    pub fn bits(self) -> u8 {
        self.0
    }
}

pub fn axes(node: &Node) -> Axes {
    match node {
        Node::Const(_) => Axes::empty(),
        Node::BlendAlpha | Node::BlendOffset => Axes::of(Axes::X | Axes::Z),
        Node::Beardifier | Node::OldBlended(_) => Axes::all(),
        Node::ShiftA(_) | Node::ShiftB(_) => Axes::of(Axes::X | Axes::Z),
        Node::Shift(_) => Axes::all(),
        Node::Gradient(g) => match g.axis {
            Axis::X => Axes::of(Axes::X),
            Axis::Y => Axes::of(Axes::Y),
            Axis::Z => Axes::of(Axes::Z),
        },
        Node::Noise(n) => {
            let mut a = Axes::of(Axes::X | Axes::Z);
            if n.y_scale != 0.0 {
                a = a.union(Axes::of(Axes::Y));
            }
            if n.xz_scale == 0.0 {
                a = a.without(Axis::X).without(Axis::Z);
            }
            if let Some(s) = &n.shift_x {
                a = a.union(axes(s));
            }
            if let Some(s) = &n.shift_y {
                a = a.union(axes(s));
            }
            if let Some(s) = &n.shift_z {
                a = a.union(axes(s));
            }
            a
        }
        Node::Spline(s) => spline_axes(s),
        Node::Slice { axis, input, .. } => axes(input).without(*axis),
        Node::FindTopSurface {
            density,
            upper_bound,
            ..
        } => axes(density).union(axes(upper_bound)).without(Axis::Y),
        Node::DistanceToPoint { .. } => Axes::all(),
        Node::EndOuterIslands => Axes::of(Axes::X | Axes::Z),
        Node::Add(l, r) | Node::Sub(l, r) | Node::Mul(l, r) | Node::Div(l, r) => {
            axes(l).union(axes(r))
        }
        Node::Min { left, right, .. } | Node::Max { left, right, .. } => {
            axes(left).union(axes(right))
        }
        Node::MulConst { input, .. }
        | Node::DivConst { input, .. }
        | Node::MinConst { input, .. }
        | Node::MaxConst { input, .. }
        | Node::Abs(input)
        | Node::Square(input)
        | Node::Cube(input)
        | Node::HalfNegative(input)
        | Node::QuarterNegative(input)
        | Node::Sqrt(input)
        | Node::Reciprocal(input)
        | Node::Negate(input)
        | Node::Squeeze(input)
        | Node::Clamp { input, .. }
        | Node::BlendDensity(input)
        | Node::Cache(input)
        | Node::Interpolated { input, .. } => axes(input),
        Node::Lerp {
            alpha,
            first,
            second,
        } => axes(alpha).union(axes(first)).union(axes(second)),
        Node::RangeChoice {
            input,
            in_range,
            out_range,
            ..
        } => axes(input).union(axes(in_range)).union(axes(out_range)),
        Node::IntervalSelect {
            input, functions, ..
        } => {
            let mut a = axes(input);
            for f in functions {
                a = a.union(axes(f));
            }
            a
        }
    }
}

fn spline_axes(spline: &Spline) -> Axes {
    let mut a = axes(&spline.coordinate);
    for p in &spline.points {
        if let SplineValue::Nested(s) = &p.value {
            a = a.union(spline_axes(s));
        }
    }
    a
}
