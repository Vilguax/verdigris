use crate::arith::{floor_i32, jabs_f32, jclamp_f32, jmax_f32, jmin_f32, l, l3};
use crate::df::node::{
    Axis, Gradient, Node, Noise, Spline, SplinePoint, SplineValue, Tiling, extrapolate,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Point,
    Volume,
}

pub fn eval_point(node: &Node, x: i32, y: i32, z: i32) -> f32 {
    eval(node, x, y, z, Mode::Point)
}

#[allow(clippy::float_cmp)]
pub fn eval(node: &Node, x: i32, y: i32, z: i32, mode: Mode) -> f32 {
    match node {
        Node::Const(c) => *c,
        Node::Add(a, b) => eval(a, x, y, z, mode) + eval(b, x, y, z, mode),
        Node::Sub(a, b) => eval(a, x, y, z, mode) - eval(b, x, y, z, mode),
        Node::Mul(a, b) => {
            let va = eval(a, x, y, z, mode);
            if mode == Mode::Point && va == 0.0 {
                0.0
            } else {
                va * eval(b, x, y, z, mode)
            }
        }
        Node::MulConst { input, factor } => eval(input, x, y, z, mode) * factor,
        Node::Div(a, b) => {
            let va = eval(a, x, y, z, mode);
            if mode == Mode::Point && va == 0.0 {
                0.0
            } else {
                va / eval(b, x, y, z, mode)
            }
        }
        Node::DivConst { input, reciprocal } => eval(input, x, y, z, mode) * reciprocal,
        Node::Min {
            left,
            right,
            right_interval,
        } => {
            let a = eval(left, x, y, z, mode);
            if mode == Mode::Point {
                if a <= right_interval.min {
                    a
                } else {
                    jmin_f32(a, eval(right, x, y, z, mode))
                }
            } else {
                let b = eval(right, x, y, z, mode);
                if b < a { b } else { a }
            }
        }
        Node::Max {
            left,
            right,
            right_interval,
        } => {
            let a = eval(left, x, y, z, mode);
            if mode == Mode::Point {
                if a >= right_interval.max {
                    a
                } else {
                    jmax_f32(a, eval(right, x, y, z, mode))
                }
            } else {
                let b = eval(right, x, y, z, mode);
                if b > a { b } else { a }
            }
        }
        Node::MinConst { input, constant } => jmin_f32(eval(input, x, y, z, mode), *constant),
        Node::MaxConst { input, constant } => jmax_f32(eval(input, x, y, z, mode), *constant),
        Node::Abs(a) => jabs_f32(eval(a, x, y, z, mode)),
        Node::Square(a) => {
            let v = eval(a, x, y, z, mode);
            v * v
        }
        Node::Cube(a) => {
            let v = eval(a, x, y, z, mode);
            (v * v) * v
        }
        Node::HalfNegative(a) => {
            let v = eval(a, x, y, z, mode);
            if v > 0.0 { v } else { v * 0.5 }
        }
        Node::QuarterNegative(a) => {
            let v = eval(a, x, y, z, mode);
            if v > 0.0 { v } else { v * 0.25 }
        }
        Node::Sqrt(a) => {
            let v = eval(a, x, y, z, mode);
            (f64::from(v).sqrt()) as f32
        }
        Node::Reciprocal(a) => 1.0 / eval(a, x, y, z, mode),
        Node::Negate(a) => -eval(a, x, y, z, mode),
        Node::Squeeze(a) => crate::df::node::squeeze_value(eval(a, x, y, z, mode)),
        Node::Clamp { input, min, max } => jclamp_f32(eval(input, x, y, z, mode), *min, *max),
        Node::Gradient(g) => eval_gradient(g, x, y, z),
        Node::Lerp {
            alpha,
            first,
            second,
        } => {
            let a = eval(alpha, x, y, z, mode);
            if a == 0.0 {
                eval(first, x, y, z, mode)
            } else if a == 1.0 {
                eval(second, x, y, z, mode)
            } else {
                l(a, eval(first, x, y, z, mode), eval(second, x, y, z, mode))
            }
        }
        Node::RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            in_range,
            out_range,
        } => {
            let v = eval(input, x, y, z, mode);
            if v >= *min_inclusive && v < *max_exclusive {
                eval(in_range, x, y, z, mode)
            } else {
                eval(out_range, x, y, z, mode)
            }
        }
        Node::IntervalSelect {
            input,
            thresholds,
            functions,
        } => {
            let v = eval(input, x, y, z, mode);
            let mut idx = functions.len() - 1;
            for (i, t) in thresholds.iter().enumerate() {
                if v < *t {
                    idx = i;
                    break;
                }
            }
            eval(&functions[idx], x, y, z, mode)
        }
        Node::Spline(s) => eval_spline(s, x, y, z, mode),
        Node::Noise(n) => eval_noise(n, x, y, z, mode),
        Node::ShiftA(stack) => {
            4.0 * stack.sample_point(0.25 * f64::from(x), 0.0, 0.25 * f64::from(z), None)
        }
        Node::ShiftB(stack) => {
            4.0 * stack.sample_point(0.25 * f64::from(z), 0.25 * f64::from(x), 0.0, None)
        }
        Node::Shift(stack) => {
            4.0 * stack.sample_point(
                0.25 * f64::from(x),
                0.25 * f64::from(y),
                0.25 * f64::from(z),
                None,
            )
        }
        Node::OldBlended(b) => match mode {
            Mode::Point => b.sample_point(f64::from(x), f64::from(y), f64::from(z)),
            Mode::Volume => b.sample_volume(x, y, z),
        },
        Node::BlendAlpha => 1.0,
        Node::BlendOffset => 0.0,
        Node::BlendDensity(input) => eval(input, x, y, z, mode),
        Node::Beardifier => 0.0,
        Node::Cache(input) => eval(input, x, y, z, mode),
        Node::Interpolated {
            input,
            cell_size_xz,
            cell_size_y,
            ..
        } => eval_interpolated(input, *cell_size_xz, *cell_size_y, x, y, z, mode),
        Node::Slice {
            axis,
            coordinate,
            input,
        } => {
            let (nx, ny, nz) = match axis {
                Axis::X => (*coordinate, y, z),
                Axis::Y => (x, *coordinate, z),
                Axis::Z => (x, y, *coordinate),
            };
            eval(input, nx, ny, nz, mode)
        }
        Node::FindTopSurface {
            density,
            upper_bound,
            lower_bound,
            cell_height,
            ..
        } => eval_find_top_surface(density, upper_bound, *lower_bound, *cell_height, x, z),
        Node::DistanceToPoint { .. } | Node::EndOuterIslands => f32::NAN,
    }
}

fn jfloor_div(x: i32, y: i32) -> i32 {
    let q = x.wrapping_div(y);
    let r = x.wrapping_rem(y);
    if r != 0 && (r < 0) != (y < 0) {
        q - 1
    } else {
        q
    }
}

fn jfloor_mod(x: i32, y: i32) -> i32 {
    x.wrapping_sub(jfloor_div(x, y).wrapping_mul(y))
}

fn eval_gradient(g: &Gradient, x: i32, y: i32, z: i32) -> f32 {
    let c = match g.axis {
        Axis::X => x,
        Axis::Y => y,
        Axis::Z => z,
    };
    let r = match g.tiling {
        Tiling::ClampToEdge => {
            let lo = g.from_coordinate.min(g.to_coordinate);
            let hi = g.from_coordinate.max(g.to_coordinate);
            c.clamp(lo, hi) - g.from_coordinate
        }
        Tiling::Repeat => jfloor_mod(c - g.from_coordinate, g.span),
        Tiling::MirroredRepeat => {
            let rr = c - g.from_coordinate;
            let t = jfloor_div(rr, g.span);
            let ll = rr - t.wrapping_mul(g.span);
            if t & 1 == 0 { ll } else { g.span - ll }
        }
    };
    g.from_value + (r as f32) * g.slope
}

fn spline_point_value(sp: &SplinePoint, x: i32, y: i32, z: i32, mode: Mode) -> f32 {
    match &sp.value {
        SplineValue::Const(c) => *c,
        SplineValue::Nested(s) => eval_spline(s, x, y, z, mode),
    }
}

fn eval_spline(spline: &Spline, x: i32, y: i32, z: i32, mode: Mode) -> f32 {
    let u = eval(&spline.coordinate, x, y, z, mode);
    let points = &spline.points;
    let n = points.len();
    let mut idx = None;
    for (i, p) in points.iter().enumerate() {
        if u < p.location {
            idx = Some(i);
            break;
        }
    }
    let j: isize = match idx {
        Some(i) => i as isize - 1,
        None => n as isize - 1,
    };
    if j < 0 {
        let p = &points[0];
        return extrapolate(
            u,
            spline_point_value(p, x, y, z, mode),
            p.location,
            p.derivative,
        );
    }
    if j as usize == n - 1 {
        let p = &points[n - 1];
        return extrapolate(
            u,
            spline_point_value(p, x, y, z, mode),
            p.location,
            p.derivative,
        );
    }
    let jl = j as usize;
    let p1 = &points[jl];
    let p2 = &points[jl + 1];
    let x1 = p1.location;
    let x2 = p2.location;
    let y1 = spline_point_value(p1, x, y, z, mode);
    let y2 = spline_point_value(p2, x, y, z, mode);
    let d1 = p1.derivative;
    let d2 = p2.derivative;
    let delta = x2 - x1;
    let t = (u - x1) / delta;
    let a = d1 * delta - (y2 - y1);
    let b = (-d2) * delta + (y2 - y1);
    l(t, y1, y2) + (t * (1.0 - t)) * l(t, a, b)
}

fn eval_noise(n: &Noise, x: i32, y: i32, z: i32, mode: Mode) -> f32 {
    let shifted = n.shift_x.is_some() || n.shift_y.is_some() || n.shift_z.is_some();
    if !shifted {
        match mode {
            Mode::Point => n.stack.sample_point(
                f64::from(x) * n.xz_scale,
                f64::from(y) * n.y_scale,
                f64::from(z) * n.xz_scale,
                None,
            ),
            Mode::Volume => n.stack.sample_volume(x, y, z, n.xz_scale, n.y_scale, None),
        }
    } else {
        let sx = n
            .shift_x
            .as_ref()
            .map(|s| f64::from(eval(s, x, y, z, mode)))
            .unwrap_or(0.0);
        let sy = n
            .shift_y
            .as_ref()
            .map(|s| f64::from(eval(s, x, y, z, mode)))
            .unwrap_or(0.0);
        let sz = n
            .shift_z
            .as_ref()
            .map(|s| f64::from(eval(s, x, y, z, mode)))
            .unwrap_or(0.0);
        let cx = f64::from(x) * n.xz_scale + sx;
        let cy = f64::from(y) * n.y_scale + sy;
        let cz = f64::from(z) * n.xz_scale + sz;
        n.stack.sample_point(cx, cy, cz, None)
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_interpolated(input: &Node, w: i32, h: i32, x: i32, y: i32, z: i32, mode: Mode) -> f32 {
    if mode == Mode::Volume {
        return eval(input, x, y, z, Mode::Volume);
    }
    let ox = jfloor_mod(x, w);
    let oy = jfloor_mod(y, h);
    let oz = jfloor_mod(z, w);
    if ox == 0 && oy == 0 && oz == 0 {
        return eval(input, x, y, z, Mode::Volume);
    }
    let cx = x - ox;
    let cy = y - oy;
    let cz = z - oz;
    let v000 = eval(input, cx, cy, cz, Mode::Volume);
    let v100 = eval(input, cx + w, cy, cz, Mode::Volume);
    let v010 = eval(input, cx, cy + h, cz, Mode::Volume);
    let v110 = eval(input, cx + w, cy + h, cz, Mode::Volume);
    let v001 = eval(input, cx, cy, cz + w, Mode::Volume);
    let v101 = eval(input, cx + w, cy, cz + w, Mode::Volume);
    let v011 = eval(input, cx, cy + h, cz + w, Mode::Volume);
    let v111 = eval(input, cx + w, cy + h, cz + w, Mode::Volume);
    let ax = (ox as f32) / (w as f32);
    let ay = (oy as f32) / (h as f32);
    let az = (oz as f32) / (w as f32);
    l3(ax, ay, az, v000, v100, v010, v110, v001, v101, v011, v111)
}

fn eval_find_top_surface(
    density: &Node,
    upper_bound: &Node,
    lower_bound: i32,
    cell_height: i32,
    x: i32,
    z: i32,
) -> f32 {
    let u = eval(upper_bound, x, 0, z, Mode::Point);
    let scaled = f64::from(u / (cell_height as f32));
    let top = floor_i32(scaled).wrapping_mul(cell_height);
    if top <= lower_bound {
        return lower_bound as f32;
    }
    let mut y = top;
    while y >= lower_bound {
        if eval(density, x, y, z, Mode::Point) > 0.0 {
            return y as f32;
        }
        y = y.wrapping_sub(cell_height);
    }
    lower_bound as f32
}
