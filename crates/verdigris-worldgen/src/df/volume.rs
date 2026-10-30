use std::collections::HashMap;
use std::fmt;

use crate::arith::{floor_div_i32, floor_mod_i32, jabs_f32, jclamp_f32, l};
use crate::df::eval::{Mode, eval};
use crate::df::node::{Axis, Node, Spline, SplinePoint, SplineValue, extrapolate, squeeze_value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume {
    pub origin: [i32; 3],
    pub steps: [i32; 3],
    pub sizes: [usize; 3],
}

impl Volume {
    pub fn new(origin: [i32; 3], steps: [i32; 3], sizes: [usize; 3]) -> Self {
        Self {
            origin,
            steps,
            sizes,
        }
    }

    pub fn len(&self) -> usize {
        self.sizes[0] * self.sizes[1] * self.sizes[2]
    }

    pub fn is_empty(&self) -> bool {
        self.sizes[0] == 0 || self.sizes[1] == 0 || self.sizes[2] == 0
    }

    pub fn linear_index(&self, i: usize, j: usize, k: usize) -> usize {
        let nx = self.sizes[0];
        let ny = self.sizes[1];
        j + (i + k * nx) * ny
    }

    pub fn block_coord(&self, i: usize, j: usize, k: usize) -> [i32; 3] {
        [
            self.origin[0].wrapping_add((i as i32).wrapping_mul(self.steps[0])),
            self.origin[1].wrapping_add((j as i32).wrapping_mul(self.steps[1])),
            self.origin[2].wrapping_add((k as i32).wrapping_mul(self.steps[2])),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeError {
    InterpolatedUnsupported,
}

impl fmt::Display for VolumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InterpolatedUnsupported => {
                write!(
                    f,
                    "interpolated density function is unsupported in volume mode"
                )
            }
        }
    }
}

impl std::error::Error for VolumeError {}

struct CacheSlot {
    key: Volume,
    values: Vec<f32>,
}

#[derive(Default)]
pub struct VolumeContext {
    caches: HashMap<usize, CacheSlot>,
    hits: usize,
    misses: usize,
}

impl VolumeContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cache_hits(&self) -> usize {
        self.hits
    }

    pub fn cache_misses(&self) -> usize {
        self.misses
    }
}

pub fn eval_volume(node: &Node, vol: &Volume) -> Result<Box<[f32]>, VolumeError> {
    let mut ctx = VolumeContext::new();
    eval_volume_with(&mut ctx, node, vol)
}

pub fn eval_volume_with(
    ctx: &mut VolumeContext,
    node: &Node,
    vol: &Volume,
) -> Result<Box<[f32]>, VolumeError> {
    Ok(ev(ctx, node, vol)?.into_boxed_slice())
}

pub fn contains_interpolated(node: &Node) -> bool {
    match node {
        Node::Interpolated { .. } => true,
        Node::Const(_)
        | Node::Gradient(_)
        | Node::ShiftA(_)
        | Node::ShiftB(_)
        | Node::Shift(_)
        | Node::OldBlended(_)
        | Node::BlendAlpha
        | Node::BlendOffset
        | Node::Beardifier
        | Node::DistanceToPoint { .. }
        | Node::EndOuterIslands => false,
        Node::Noise(n) => {
            n.shift_x.as_deref().is_some_and(contains_interpolated)
                || n.shift_y.as_deref().is_some_and(contains_interpolated)
                || n.shift_z.as_deref().is_some_and(contains_interpolated)
        }
        Node::Add(a, b) | Node::Sub(a, b) | Node::Mul(a, b) | Node::Div(a, b) => {
            contains_interpolated(a) || contains_interpolated(b)
        }
        Node::Min { left, right, .. } | Node::Max { left, right, .. } => {
            contains_interpolated(left) || contains_interpolated(right)
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
        | Node::Slice { input, .. } => contains_interpolated(input),
        Node::Lerp {
            alpha,
            first,
            second,
        } => {
            contains_interpolated(alpha)
                || contains_interpolated(first)
                || contains_interpolated(second)
        }
        Node::RangeChoice {
            input,
            in_range,
            out_range,
            ..
        } => {
            contains_interpolated(input)
                || contains_interpolated(in_range)
                || contains_interpolated(out_range)
        }
        Node::IntervalSelect {
            input, functions, ..
        } => contains_interpolated(input) || functions.iter().any(contains_interpolated),
        Node::Spline(s) => spline_contains_interpolated(s),
        Node::FindTopSurface {
            density,
            upper_bound,
            ..
        } => contains_interpolated(density) || contains_interpolated(upper_bound),
    }
}

fn spline_contains_interpolated(spline: &Spline) -> bool {
    if contains_interpolated(&spline.coordinate) {
        return true;
    }
    spline.points.iter().any(|p| match &p.value {
        SplineValue::Const(_) => false,
        SplineValue::Nested(s) => spline_contains_interpolated(s),
    })
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

fn write_cells<F: FnMut(i32, i32, i32) -> f32>(out: &mut [f32], vol: &Volume, mut f: F) {
    let [nx, ny, nz] = vol.sizes;
    for k in 0..nz {
        for i in 0..nx {
            for j in 0..ny {
                let idx = vol.linear_index(i, j, k);
                let [x, y, z] = vol.block_coord(i, j, k);
                out[idx] = f(x, y, z);
            }
        }
    }
}

fn leaf_mode(node: &Node, vol: &Volume, mode: Mode) -> Vec<f32> {
    let mut out = vec![0.0f32; vol.len()];
    write_cells(&mut out, vol, |x, y, z| eval(node, x, y, z, mode));
    out
}

fn map1(mut a: Vec<f32>, f: impl Fn(f32) -> f32) -> Vec<f32> {
    for x in &mut a {
        *x = f(*x);
    }
    a
}

fn map2(mut a: Vec<f32>, b: &[f32], f: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    for (x, y) in a.iter_mut().zip(b.iter()) {
        *x = f(*x, *y);
    }
    a
}

#[allow(clippy::float_cmp)]
fn ev(ctx: &mut VolumeContext, node: &Node, vol: &Volume) -> Result<Vec<f32>, VolumeError> {
    match node {
        Node::Interpolated {
            input,
            cell_size_xz,
            cell_size_y,
            inv_xz,
            inv_y,
        } => eval_interpolated_volume(
            ctx,
            input,
            *cell_size_xz,
            *cell_size_y,
            *inv_xz,
            *inv_y,
            vol,
        ),
        Node::Const(c) => Ok(vec![*c; vol.len()]),
        Node::Add(a, b) => {
            let va = ev(ctx, a, vol)?;
            let vb = ev(ctx, b, vol)?;
            Ok(map2(va, &vb, |x, y| x + y))
        }
        Node::Sub(a, b) => {
            let va = ev(ctx, a, vol)?;
            let vb = ev(ctx, b, vol)?;
            Ok(map2(va, &vb, |x, y| x - y))
        }
        Node::Mul(a, b) => {
            let va = ev(ctx, a, vol)?;
            let vb = ev(ctx, b, vol)?;
            Ok(map2(va, &vb, |x, y| x * y))
        }
        Node::MulConst { input, factor } => {
            let v = ev(ctx, input, vol)?;
            Ok(map1(v, |x| x * *factor))
        }
        Node::Div(a, b) => {
            let va = ev(ctx, a, vol)?;
            let vb = ev(ctx, b, vol)?;
            Ok(map2(va, &vb, |x, y| x / y))
        }
        Node::DivConst { input, reciprocal } => {
            let v = ev(ctx, input, vol)?;
            Ok(map1(v, |x| x * *reciprocal))
        }
        Node::Min { left, right, .. } => {
            let va = ev(ctx, left, vol)?;
            let vb = ev(ctx, right, vol)?;
            Ok(map2(va, &vb, |x, y| if y < x { y } else { x }))
        }
        Node::Max { left, right, .. } => {
            let va = ev(ctx, left, vol)?;
            let vb = ev(ctx, right, vol)?;
            Ok(map2(va, &vb, |x, y| if y > x { y } else { x }))
        }
        Node::MinConst { input, constant } => {
            let v = ev(ctx, input, vol)?;
            let c = *constant;
            Ok(map1(v, |x| if c < x { c } else { x }))
        }
        Node::MaxConst { input, constant } => {
            let v = ev(ctx, input, vol)?;
            let c = *constant;
            Ok(map1(v, |x| if c > x { c } else { x }))
        }
        Node::Abs(a) => Ok(map1(ev(ctx, a, vol)?, jabs_f32)),
        Node::Square(a) => Ok(map1(ev(ctx, a, vol)?, |x| x * x)),
        Node::Cube(a) => Ok(map1(ev(ctx, a, vol)?, |x| (x * x) * x)),
        Node::HalfNegative(a) => Ok(map1(
            ev(ctx, a, vol)?,
            |x| if x > 0.0 { x } else { x * 0.5 },
        )),
        Node::QuarterNegative(a) => Ok(map1(
            ev(ctx, a, vol)?,
            |x| {
                if x > 0.0 { x } else { x * 0.25 }
            },
        )),
        Node::Sqrt(a) => Ok(map1(ev(ctx, a, vol)?, |x| f64::from(x).sqrt() as f32)),
        Node::Reciprocal(a) => Ok(map1(ev(ctx, a, vol)?, |x| 1.0 / x)),
        Node::Negate(a) => Ok(map1(ev(ctx, a, vol)?, |x| -x)),
        Node::Squeeze(a) => Ok(map1(ev(ctx, a, vol)?, squeeze_value)),
        Node::Clamp { input, min, max } => {
            let v = ev(ctx, input, vol)?;
            let (lo, hi) = (*min, *max);
            Ok(map1(v, |x| jclamp_f32(x, lo, hi)))
        }
        Node::Lerp {
            alpha,
            first,
            second,
        } => {
            let a = ev(ctx, alpha, vol)?;
            let f = ev(ctx, first, vol)?;
            let s = ev(ctx, second, vol)?;
            let mut out = f;
            for (cell, out_v) in out.iter_mut().enumerate() {
                let av = a[cell];
                *out_v = if av == 0.0 {
                    *out_v
                } else if av == 1.0 {
                    s[cell]
                } else {
                    l(av, *out_v, s[cell])
                };
            }
            Ok(out)
        }
        Node::RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            in_range,
            out_range,
        } => {
            let v = ev(ctx, input, vol)?;
            let ir = ev(ctx, in_range, vol)?;
            let or = ev(ctx, out_range, vol)?;
            let mut out = ir;
            for (cell, out_v) in out.iter_mut().enumerate() {
                let vv = v[cell];
                if !(vv >= *min_inclusive && vv < *max_exclusive) {
                    *out_v = or[cell];
                }
            }
            Ok(out)
        }
        Node::IntervalSelect {
            input,
            thresholds,
            functions,
        } => {
            let v = ev(ctx, input, vol)?;
            let mut fns = Vec::with_capacity(functions.len());
            for f in functions {
                fns.push(ev(ctx, f, vol)?);
            }
            let mut out = vec![0.0f32; vol.len()];
            for (cell, out_v) in out.iter_mut().enumerate() {
                let vv = v[cell];
                let mut idx = functions.len() - 1;
                for (ti, t) in thresholds.iter().enumerate() {
                    if vv < *t {
                        idx = ti;
                        break;
                    }
                }
                *out_v = fns[idx][cell];
            }
            Ok(out)
        }
        Node::Spline(s) => eval_spline_volume(ctx, s, vol),
        Node::BlendDensity(input) => ev(ctx, input, vol),
        Node::Cache(inner) => {
            let addr = std::ptr::from_ref::<Node>(node) as usize;
            let hit = match ctx.caches.get(&addr) {
                Some(slot) if slot.key == *vol => Some(slot.values.clone()),
                _ => None,
            };
            if let Some(values) = hit {
                ctx.hits += 1;
                return Ok(values);
            }
            let values = ev(ctx, inner, vol)?;
            ctx.misses += 1;
            ctx.caches.insert(
                addr,
                CacheSlot {
                    key: *vol,
                    values: values.clone(),
                },
            );
            Ok(values)
        }
        Node::Slice {
            axis,
            coordinate,
            input,
        } => {
            let ax = axis_index(*axis);
            let mut sub = *vol;
            sub.origin[ax] = *coordinate;
            sub.steps[ax] = 0;
            sub.sizes[ax] = 1;
            let reduced = ev(ctx, input, &sub)?;
            let mut out = vec![0.0f32; vol.len()];
            let [nx, ny, nz] = vol.sizes;
            for k in 0..nz {
                for i in 0..nx {
                    for j in 0..ny {
                        let idx = vol.linear_index(i, j, k);
                        let (si, sj, sk) = match ax {
                            0 => (0, j, k),
                            1 => (i, 0, k),
                            _ => (i, j, 0),
                        };
                        out[idx] = reduced[sub.linear_index(si, sj, sk)];
                    }
                }
            }
            Ok(out)
        }
        Node::FindTopSurface { .. } => Ok(leaf_mode(node, vol, Mode::Point)),
        Node::Gradient(_)
        | Node::Noise(_)
        | Node::ShiftA(_)
        | Node::ShiftB(_)
        | Node::Shift(_)
        | Node::OldBlended(_)
        | Node::BlendAlpha
        | Node::BlendOffset
        | Node::Beardifier
        | Node::DistanceToPoint { .. }
        | Node::EndOuterIslands => Ok(leaf_mode(node, vol, Mode::Volume)),
    }
}

fn interpolated_aligned(vol: &Volume, w: i32, h: i32) -> bool {
    let axes = [
        (vol.origin[0], vol.steps[0], vol.sizes[0], w),
        (vol.origin[1], vol.steps[1], vol.sizes[1], h),
        (vol.origin[2], vol.steps[2], vol.sizes[2], w),
    ];
    axes.iter().all(|&(origin, step, size, cell)| {
        floor_mod_i32(origin, cell) == 0 && (size == 1 || step == cell)
    })
}

fn full_size(n: usize, s: i32) -> usize {
    if n <= 1 {
        n
    } else {
        (n - 1).saturating_mul(s.max(0) as usize).saturating_add(1)
    }
}

fn corner_count(c_min: i32, c_max: i32, max_coord: i32, cell: i32) -> usize {
    let base = (c_max - c_min + 1) as usize;
    let delta = usize::from(floor_mod_i32(max_coord, cell) != 0);
    base + delta
}

#[allow(clippy::too_many_arguments)]
fn eval_interpolated_volume(
    ctx: &mut VolumeContext,
    input: &Node,
    w: i32,
    h: i32,
    iw: f32,
    ih: f32,
    vol: &Volume,
) -> Result<Vec<f32>, VolumeError> {
    if interpolated_aligned(vol, w, h) {
        return ev(ctx, input, vol);
    }
    if vol.steps == [1, 1, 1] {
        return interpolated_unit_volume(ctx, input, w, h, iw, ih, vol);
    }
    let [nx, ny, nz] = vol.sizes;
    let [sx, sy, sz] = vol.steps;
    let full = Volume::new(
        vol.origin,
        [1, 1, 1],
        [full_size(nx, sx), full_size(ny, sy), full_size(nz, sz)],
    );
    let dense = interpolated_unit_volume(ctx, input, w, h, iw, ih, &full)?;
    let mut out = vec![0.0f32; vol.len()];
    for k in 0..nz {
        let fk = k.saturating_mul(sz.max(0) as usize);
        for i in 0..nx {
            let fi = i.saturating_mul(sx.max(0) as usize);
            for j in 0..ny {
                let fj = j.saturating_mul(sy.max(0) as usize);
                out[vol.linear_index(i, j, k)] = dense[full.linear_index(fi, fj, fk)];
            }
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn interpolated_unit_volume(
    ctx: &mut VolumeContext,
    input: &Node,
    w: i32,
    h: i32,
    iw: f32,
    ih: f32,
    vol: &Volume,
) -> Result<Vec<f32>, VolumeError> {
    let [nx, ny, nz] = vol.sizes;
    let [x0, y0, z0] = vol.origin;
    if nx == 0 || ny == 0 || nz == 0 {
        return Ok(vec![0.0f32; vol.len()]);
    }

    let cx_min = floor_div_i32(x0, w);
    let cy_min = floor_div_i32(y0, h);
    let cz_min = floor_div_i32(z0, w);
    let x_max = x0.wrapping_add(nx as i32 - 1);
    let y_max = y0.wrapping_add(ny as i32 - 1);
    let z_max = z0.wrapping_add(nz as i32 - 1);
    let gx = corner_count(cx_min, floor_div_i32(x_max, w), x_max, w);
    let gy = corner_count(cy_min, floor_div_i32(y_max, h), y_max, h);
    let gz = corner_count(cz_min, floor_div_i32(z_max, w), z_max, w);

    let corner_vol = Volume::new(
        [
            cx_min.wrapping_mul(w),
            cy_min.wrapping_mul(h),
            cz_min.wrapping_mul(w),
        ],
        [w, h, w],
        [gx, gy, gz],
    );
    let corners = ev(ctx, input, &corner_vol)?;
    let corner = |gi: usize, gj: usize, gk: usize| corners[corner_vol.linear_index(gi, gj, gk)];

    let mut out = vec![0.0f32; vol.len()];
    for k in 0..nz {
        let z = z0.wrapping_add(k as i32);
        let gz0 = (floor_div_i32(z, w) - cz_min) as usize;
        let gz1 = (gz0 + 1).min(gz - 1);
        let az = (floor_mod_i32(z, w) as f32) * iw;
        for i in 0..nx {
            let x = x0.wrapping_add(i as i32);
            let gx0 = (floor_div_i32(x, w) - cx_min) as usize;
            let gx1 = (gx0 + 1).min(gx - 1);
            let ax = (floor_mod_i32(x, w) as f32) * iw;
            let mut j = 0usize;
            while j < ny {
                let y = y0.wrapping_add(j as i32);
                let gy0 = (floor_div_i32(y, h) - cy_min) as usize;
                let gy1 = (gy0 + 1).min(gy - 1);
                let p00 = l(az, corner(gx0, gy0, gz0), corner(gx0, gy0, gz1));
                let p10 = l(az, corner(gx1, gy0, gz0), corner(gx1, gy0, gz1));
                let p01 = l(az, corner(gx0, gy1, gz0), corner(gx0, gy1, gz1));
                let p11 = l(az, corner(gx1, gy1, gz0), corner(gx1, gy1, gz1));
                let q0 = l(ax, p00, p10);
                let q1 = l(ax, p01, p11);
                let step = (q1 - q0) * ih;
                let oy0 = floor_mod_i32(y, h);
                let mut val = q0 + step * (oy0 as f32);
                let mut oy = oy0;
                loop {
                    out[vol.linear_index(i, j, k)] = val;
                    j += 1;
                    oy += 1;
                    if j >= ny || oy >= h {
                        break;
                    }
                    val += step;
                }
            }
        }
    }
    Ok(out)
}

fn eval_spline_volume(
    ctx: &mut VolumeContext,
    spline: &Spline,
    vol: &Volume,
) -> Result<Vec<f32>, VolumeError> {
    let coord = ev(ctx, &spline.coordinate, vol)?;
    let mut values = Vec::with_capacity(spline.points.len());
    for p in &spline.points {
        let arr = match &p.value {
            SplineValue::Const(c) => vec![*c; vol.len()],
            SplineValue::Nested(s) => eval_spline_volume(ctx, s, vol)?,
        };
        values.push(arr);
    }
    let mut out = vec![0.0f32; vol.len()];
    for (cell, out_v) in out.iter_mut().enumerate() {
        *out_v = spline_scalar(&spline.points, |p| values[p][cell], coord[cell]);
    }
    Ok(out)
}

#[allow(clippy::float_cmp)]
fn spline_scalar(points: &[SplinePoint], val: impl Fn(usize) -> f32, u: f32) -> f32 {
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
        return extrapolate(u, val(0), p.location, p.derivative);
    }
    if j as usize == n - 1 {
        let p = &points[n - 1];
        return extrapolate(u, val(n - 1), p.location, p.derivative);
    }
    let jl = j as usize;
    let p1 = &points[jl];
    let p2 = &points[jl + 1];
    let x1 = p1.location;
    let x2 = p2.location;
    let y1 = val(jl);
    let y2 = val(jl + 1);
    let d1 = p1.derivative;
    let d2 = p2.derivative;
    let delta = x2 - x1;
    let t = (u - x1) / delta;
    let a = d1 * delta - (y2 - y1);
    let b = (-d2) * delta + (y2 - y1);
    l(t, y1, y2) + (t * (1.0 - t)) * l(t, a, b)
}

#[cfg(test)]
mod tests {
    use crate::arith::l;

    fn accumulate(q0: f32, q1: f32, h: usize) -> Vec<f32> {
        let ih = 1.0 / (h as f32);
        let step = (q1 - q0) * ih;
        let mut val = q0;
        let mut out = Vec::with_capacity(h);
        for _ in 0..h {
            out.push(val);
            val += step;
        }
        out
    }

    #[test]
    fn vertical_accumulation_exact_bits_0_1_to_0_7() {
        let out = accumulate(0.1, 0.7, 8);
        let expected: [u32; 8] = [
            1036831949, 1043542835, 1048576000, 1051092582, 1053609164, 1056125746, 1057803468,
            1059061759,
        ];
        for (oy, (v, e)) in out.iter().zip(expected.iter()).enumerate() {
            assert_eq!(v.to_bits(), *e, "accumulation mismatch at oy={oy}");
        }
        let forbidden = [1057803469u32, 1059061760u32];
        for v in &out {
            assert!(
                !forbidden.contains(&v.to_bits()),
                "direct-lerp value leaked into accumulation"
            );
        }
    }

    #[test]
    fn vertical_accumulation_differs_from_direct_lerp() {
        let out = accumulate(0.123, 0.987, 8);
        let mut direct = Vec::with_capacity(8);
        for oy in 0..8usize {
            direct.push(l((oy as f32) / 8.0, 0.123, 0.987));
        }
        let differs = out
            .iter()
            .zip(direct.iter())
            .any(|(a, b)| a.to_bits() != b.to_bits());
        assert!(differs);
    }
}
