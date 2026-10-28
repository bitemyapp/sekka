// SPDX-License-Identifier: MPL-2.0
// Copyright © 2024-2026 Chris Marchesi
// Copyright © 2002 University of Southern California
// Adapted from pinned z2d 40af3e84a4e15a9f31d126b3f281f23ce6a3dae5:
// Spline, Face, Slope, Pen, stroke_plotter, Polygon and multisample.
// Cairo-derived portions retain the upstream notices in LICENSE.z2d.
// This bounded subset supports identity-transform, butt-cap open strokes and
// closed inner-stroked corner triangles and nonzero polygon fills.
// No runtime Zig, foreign code, or unsafe Rust.
use crate::{Error, Raster};
const LIMIT: usize = 32768;
const TOLERANCE: f64 = 0.1;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y)
    }
    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y)
    }
    fn half(self, b: Self) -> Self {
        Self::new(self.x + (b.x - self.x) / 2., self.y + (b.y - self.y) / 2.)
    }
    fn normal(self) -> Self {
        if self.x == 0. {
            Self::new(0., if self.y > 0. { 1. } else { -1. })
        } else if self.y == 0. {
            Self::new(if self.x > 0. { 1. } else { -1. }, 0.)
        } else {
            let mag = crate::math::hypot(self.x, self.y);
            Self::new(self.x / mag, self.y / mag)
        }
    }
    fn compare(self, b: Self) -> i32 {
        let by = if (b.y - self.y).abs() > f64::EPSILON {
            b.y
        } else {
            self.y
        };
        let bx = if (b.x - self.x).abs() > f64::EPSILON {
            b.x
        } else {
            self.x
        };
        let value = self.y * bx - by * self.x;
        if value > 0. {
            return 1;
        }
        if value < 0. {
            return -1;
        }
        if self.x == 0. && self.y == 0. {
            return i32::from(bx != 0. || by != 0.);
        }
        if bx == 0. && by == 0. {
            return -1;
        }
        let sign = |x: f64| {
            if x > 0. {
                1
            } else if x < 0. {
                -1
            } else {
                0
            }
        };
        if sign(self.x) != sign(bx) || sign(self.y) != sign(by) {
            return if self.x > 0. || (self.x == 0. && self.y > 0.) {
                -1
            } else {
                1
            };
        }
        0
    }
}
fn push<T>(v: &mut Vec<T>, p: T) -> Result<(), Error> {
    if v.len() == LIMIT {
        return Err(Error::SizeLimit);
    }
    if v.len() == v.capacity() {
        v.try_reserve(1).map_err(|_| Error::Allocation)?;
    }
    v.push(p);
    Ok(())
}
#[derive(Clone, Copy)]
pub(crate) enum Node {
    Move(Point),
    Line(Point),
    Curve(Point, Point, Point),
}
#[derive(Clone, Copy)]
struct Face {
    slope: Point,
    p0_cw: Point,
    p0_ccw: Point,
    p1_cw: Point,
    p1_ccw: Point,
}
impl Face {
    fn new(p0: Point, p1: Point, t: f64) -> Self {
        let slope = p1.sub(p0).normal();
        let h = t / 2.;
        let offset = Point::new(-slope.y * h, slope.x * h);
        Self {
            slope,
            p0_cw: p0.add(offset),
            p0_ccw: p0.sub(offset),
            p1_cw: p1.add(offset),
            p1_ccw: p1.sub(offset),
        }
    }
    fn intersect(self, out: Self, cw: bool) -> Point {
        let a = if cw { self.p1_ccw } else { self.p1_cw };
        let b = if cw { out.p0_ccw } else { out.p0_cw };
        let i = self.slope.normal();
        let o = out.slope.normal();
        let y =
            ((b.x - a.x) * i.y * o.y - b.y * o.x * i.y + a.y * i.x * o.y) / (i.x * o.y - o.x * i.y);
        let x = if i.y.abs() >= o.y.abs() {
            (y - a.y) * i.x / i.y + a.x
        } else {
            (y - b.y) * o.x / o.y + b.x
        };
        Point::new(x, y)
    }
}
struct Pen {
    points: Vec<Point>,
}
impl Pen {
    fn new(t: f64) -> Result<Self, Error> {
        let r = t / 2.;
        let n = if TOLERANCE >= r * 4. {
            1
        } else if TOLERANCE >= r {
            4
        } else {
            let d = (1. - TOLERANCE / r).acos();
            if d == 0. {
                4
            } else {
                let n = (2. * std::f64::consts::PI / d).ceil() as usize;
                n.max(4).next_multiple_of(2)
            }
        };
        let mut points = Vec::new();
        for i in 0..n {
            let theta = 2. * std::f64::consts::PI * i as f64 / n as f64;
            push(&mut points, Point::new(r * theta.cos(), r * theta.sin()))?;
        }
        Ok(Self { points })
    }
    fn cw(&self, i: usize) -> Point {
        self.points[i].sub(self.points[if i == 0 { self.points.len() - 1 } else { i - 1 }])
    }
    fn ccw(&self, i: usize) -> Point {
        self.points[(i + 1) % self.points.len()].sub(self.points[i])
    }
    fn indices(&self, from: Point, to: Point, cw: bool) -> (usize, usize) {
        let n = self.points.len();
        let mut low = 0;
        let mut high = n;
        let mut i = (low + high) >> 1;
        if cw {
            while high - low > 1 {
                if self.cw(i).compare(from) < 0 {
                    low = i
                } else {
                    high = i
                }
                i = (low + high) >> 1;
            }
            if self.cw(i).compare(from) < 0 {
                i = (i + 1) % n;
            }
            let start = i;
            if to.compare(self.ccw(i)) >= 0 {
                low = i;
                high = i + n;
                i = (low + high) >> 1;
                while high - low > 1 {
                    let j = i % n;
                    if self.cw(j).compare(to) > 0 {
                        high = i
                    } else {
                        low = i
                    }
                    i = (low + high) >> 1;
                }
                i %= n;
            }
            (start, i)
        } else {
            while high - low > 1 {
                if from.compare(self.ccw(i)) < 0 {
                    low = i
                } else {
                    high = i
                }
                i = (low + high) >> 1;
            }
            if from.compare(self.ccw(i)) < 0 {
                i = (i + 1) % n;
            }
            let start = i;
            if self.cw(i).compare(to) <= 0 {
                low = i;
                high = i + n;
                i = (low + high) >> 1;
                while high - low > 1 {
                    let j = i % n;
                    if to.compare(self.ccw(j)) > 0 {
                        high = i
                    } else {
                        low = i
                    }
                    i = (low + high) >> 1;
                }
                i %= n;
            }
            (start, i)
        }
    }
}
#[derive(Clone, Copy)]
struct Knots {
    a: Point,
    b: Point,
    c: Point,
    d: Point,
}
impl Knots {
    fn error(self) -> f64 {
        let mut b = self.b.sub(self.a);
        let mut c = self.c.sub(self.a);
        let d = self.d.sub(self.a);
        if d.x != 0. || d.y != 0. {
            let ds = d.x * d.x + d.y * d.y;
            for p in [&mut b, &mut c] {
                let dot = p.x * d.x + p.y * d.y;
                if dot >= ds {
                    p.x -= d.x;
                    p.y -= d.y
                } else {
                    p.x -= dot / ds * d.x;
                    p.y -= dot / ds * d.y
                }
            }
        }
        (b.x * b.x + b.y * b.y).max(c.x * c.x + c.y * c.y)
    }
    fn split(self) -> (Self, Self) {
        let ab = self.a.half(self.b);
        let bc = self.b.half(self.c);
        let cd = self.c.half(self.d);
        let abbc = ab.half(bc);
        let bccd = bc.half(cd);
        let middle = abbc.half(bccd);
        (
            Self {
                a: self.a,
                b: ab,
                c: abbc,
                d: middle,
            },
            Self {
                a: middle,
                b: bccd,
                c: cd,
                d: self.d,
            },
        )
    }
}
fn flatten(k: Knots, start: Point, depth: u32, out: &mut Vec<(Point, bool)>) -> Result<(), Error> {
    if k.error() < TOLERANCE * TOLERANCE {
        if k.a != start {
            push(out, (k.a, true))?;
        }
        return Ok(());
    }
    if depth == 32 {
        return Err(Error::SizeLimit);
    }
    let (a, b) = k.split();
    flatten(a, start, depth + 1, out)?;
    flatten(b, start, depth + 1, out)
}
fn flatten_path(raster: &Raster, nodes: &[Node]) -> Result<Vec<(Point, bool)>, Error> {
    let translate =
        |p: Point| Point::new(p.x - raster.origin_x as f64, p.y - raster.origin_y as f64);
    let mut points = Vec::new();
    for node in nodes {
        match *node {
            Node::Move(p) => {
                let p = translate(p);
                if !points.is_empty() {
                    return Err(Error::Unsupported);
                }
                push(&mut points, (p, false))?;
            }
            Node::Line(p) => {
                let p = translate(p);
                if points.last().is_none_or(|&(old, _)| old != p) {
                    push(&mut points, (p, false))?;
                }
            }
            Node::Curve(b, c, d) => {
                let (b, c, d) = (translate(b), translate(c), translate(d));
                let a = points.last().ok_or(Error::Unsupported)?.0;
                if a != b || c != d {
                    flatten(Knots { a, b, c, d }, a, 0, &mut points)?;
                }
                push(&mut points, (d, true))?;
            }
        }
    }
    Ok(points)
}
pub(crate) fn fill_path(raster: &mut Raster, nodes: &[Node]) -> Result<(), Error> {
    let points = flatten_path(raster, nodes)?;
    let mut contour = Vec::new();
    for (p, _) in points {
        push(&mut contour, p)?;
    }
    fill_device(raster, &contour)
}
pub(crate) fn stroke(raster: &mut Raster, nodes: &[Node], t: f64) -> Result<(), Error> {
    let points = flatten_path(raster, nodes)?;
    stroke_points(raster, points, t)
}
fn stroke_points(raster: &mut Raster, mut points: Vec<(Point, bool)>, t: f64) -> Result<(), Error> {
    let curved = points.iter().any(|(_, round)| *round);
    points.dedup_by(|a, b| a.0 == b.0);
    if points.len() < 2 {
        return Ok(());
    }
    let pen = if curved { Some(Pen::new(t)?) } else { None };
    let mut outer = Vec::new();
    let mut inner = Vec::new();
    let mut direction = None;
    for triple in points.windows(3) {
        let a = Face::new(triple[0].0, triple[1].0, t);
        let b = Face::new(triple[1].0, triple[2].0, t);
        let cmp = a.slope.compare(b.slope);
        let cw = cmp < 0;
        let poly_cw = *direction.get_or_insert(cw);
        let (outside, inside) = if cw == poly_cw {
            (&mut outer, &mut inner)
        } else {
            (&mut inner, &mut outer)
        };
        let ai = if cw { a.p1_cw } else { a.p1_ccw };
        let ao = if cw { a.p1_ccw } else { a.p1_cw };
        let bi = if cw { b.p0_cw } else { b.p0_ccw };
        let bo = if cw { b.p0_ccw } else { b.p0_cw };
        if cmp == 0 {
            push(outside, ao)?;
            push(inside, ai)?;
            continue;
        }
        if triple[2].1 {
            let pen = pen.as_ref().ok_or(Error::Unsupported)?;
            let (mut index, end) = pen.indices(a.slope, b.slope, cw);
            push(outside, ao)?;
            while index != end {
                push(outside, triple[1].0.add(pen.points[index]))?;
                index = if cw {
                    (index + 1) % pen.points.len()
                } else {
                    (index + pen.points.len() - 1) % pen.points.len()
                };
            }
            push(outside, bo)?;
        } else {
            let i = a.slope.normal();
            let o = b.slope.normal();
            if 2. <= 100. * (1. + (i.x * o.x + i.y * o.y)) {
                push(outside, a.intersect(b, cw))?
            } else {
                push(outside, ao)?;
                push(outside, bo)?;
            }
        }
        push(inside, ai)?;
        push(inside, triple[1].0)?;
        push(inside, bi)?;
    }
    let cw = direction.unwrap_or(true);
    let first = Face::new(points[1].0, points[0].0, t);
    let last = Face::new(points[points.len() - 2].0, points[points.len() - 1].0, t);
    let mut contour = Vec::new();
    for p in if cw {
        [first.p1_ccw, first.p1_cw]
    } else {
        [first.p1_cw, first.p1_ccw]
    } {
        push(&mut contour, p)?;
    }
    for p in outer {
        push(&mut contour, p)?;
    }
    for p in if cw {
        [last.p1_ccw, last.p1_cw]
    } else {
        [last.p1_cw, last.p1_ccw]
    } {
        push(&mut contour, p)?;
    }
    for p in inner.into_iter().rev() {
        push(&mut contour, p)?;
    }
    fill_device(raster, &contour)
}
#[derive(Clone, Copy)]
struct Edge {
    top: f64,
    bottom: f64,
    x: f64,
    inc: f64,
    dir: i32,
}
pub(crate) fn fill_device(r: &mut Raster, points: &[Point]) -> Result<(), Error> {
    fill_contours(r, &[points])
}
fn fill_contours(r: &mut Raster, contours: &[&[Point]]) -> Result<(), Error> {
    let mut edges = Vec::new();
    for points in contours {
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            let a = Point::new(a.x * 4., a.y * 4.);
            let b = Point::new(b.x * 4., b.y * 4.);
            if a.y == b.y {
                continue;
            }
            let (top, bottom, dir) = if a.y < b.y { (a, b, -1) } else { (b, a, 1) };
            push(
                &mut edges,
                Edge {
                    top: top.y,
                    bottom: bottom.y,
                    x: top.x,
                    inc: (bottom.x - top.x) / (bottom.y - top.y),
                    dir,
                },
            )?;
        }
    }
    if edges.is_empty() {
        return Ok(());
    }
    let mut coverage = Vec::new();
    coverage
        .try_reserve_exact(r.width as usize)
        .map_err(|_| Error::Allocation)?;
    coverage.resize(r.width as usize, 0u8);
    let mut xs = Vec::new();
    xs.try_reserve_exact(edges.len())
        .map_err(|_| Error::Allocation)?;
    let start = (edges.iter().map(|e| e.top).fold(f64::INFINITY, f64::min) / 4.)
        .floor()
        .max(0.) as u32;
    let end = (edges
        .iter()
        .map(|e| e.bottom)
        .fold(f64::NEG_INFINITY, f64::max)
        / 4.)
        .ceil()
        .min(r.height as f64 - 1.) as u32;
    for y in start..=end {
        coverage.fill(0);
        for sub in 0..4 {
            let ym = (y * 4 + sub) as f64 + 0.5;
            xs.clear();
            for e in &edges {
                if e.top < ym && e.bottom >= ym {
                    xs.push(((e.x + e.inc * (ym - e.top)).round() as i32, e.dir));
                }
            }
            xs.sort_unstable_by_key(|p| p.0);
            let mut winding = 0;
            let mut begin = 0;
            let mut x_min = 0;
            for &(x, dir) in &xs {
                if winding == 0 {
                    begin = x;
                }
                winding += dir;
                if winding == 0 {
                    let a = begin.max(x_min).max(0).min(r.width as i32 * 4);
                    let b = x.max(a).min(r.width as i32 * 4);
                    for xx in a..b {
                        coverage[xx as usize / 4] += 1;
                    }
                    x_min = b;
                }
            }
        }
        for (x, &c) in coverage.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let alpha = (u32::from(c) * 16 - 1).min(255);
            let dst = &mut r.alpha[y as usize * r.width as usize + x];
            let old = u32::from(*dst);
            *dst = (alpha + old - alpha * old / 255) as u8;
        }
    }
    Ok(())
}

// Path.offset flattens curves and consolidates collinear segments before applying
// the offset. Its result contains only line nodes, so subsequent joins are miter.
pub(crate) fn inner_stroke(
    raster: &mut Raster,
    nodes: &[Node],
    thickness: f64,
) -> Result<(), Error> {
    #[derive(Clone, Copy)]
    struct Segment {
        p0: Point,
        p1: Point,
        slope: Point,
    }
    let points = flatten_path(raster, nodes)?;
    let mut segments: Vec<Segment> = Vec::new();
    let mut direction = None;
    let compare = |a: Point, b: Point| {
        let x = if (b.x - a.x).abs() > f64::EPSILON {
            b.x
        } else {
            a.x
        };
        let y = if (b.y - a.y).abs() > f64::EPSILON {
            b.y
        } else {
            a.y
        };
        let v = a.y * x - y * a.x;
        if v > 0. {
            1
        } else if v < 0. {
            -1
        } else {
            0
        }
    };
    for pair in points.windows(2) {
        let p0 = pair[0].0;
        let p1 = pair[1].0;
        if p0 == p1 {
            continue;
        }
        let seg = Segment {
            p0,
            p1,
            slope: p1.sub(p0).normal(),
        };
        if let Some(previous) = segments.last_mut() {
            let cmp = compare(previous.slope, seg.slope);
            if cmp == 0 {
                previous.p1 = p1;
                continue;
            }
            if direction.is_none() {
                direction = Some(cmp < 0);
            }
        }
        push(&mut segments, seg)?;
    }
    if segments.is_empty() {
        return Ok(());
    }
    let cw = direction.unwrap_or(true);
    let offset = -thickness / 2.;
    let shifted = |p: Point, s: Point| {
        let off = Point::new(-s.y * offset, s.x * offset);
        if cw { p.sub(off) } else { p.add(off) }
    };
    let mut output = Vec::new();
    push(
        &mut output,
        (shifted(segments[0].p0, segments[0].slope), false),
    )?;
    for pair in segments.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let p = shifted(a.p1, a.slope);
        let q = shifted(b.p0, b.slope);
        let div = a.slope.x * b.slope.y - a.slope.y * b.slope.x;
        if div == 0. {
            return Err(Error::Unsupported);
        }
        let t = ((q.x - p.x) * b.slope.y - (q.y - p.y) * b.slope.x) / div;
        push(
            &mut output,
            (Point::new(p.x + t * a.slope.x, p.y + t * a.slope.y), false),
        )?;
    }
    let last = segments[segments.len() - 1];
    push(&mut output, (shifted(last.p1, last.slope), false))?;
    stroke_points(raster, output, thickness)
}

// Closed offset and miter stroke from z2d InputSet::alignSegments,
// offset::runClosed and stroke_plotter::plotClosedJoined. Only the fixed,
// nondegenerate cell-corner triangles call this entry point. Their first
// vertex is already the first leftmost point, so alignment preserves order.
pub(crate) fn inner_stroke_triangle(
    raster: &mut Raster,
    vertices: [Point; 3],
    thickness: f64,
) -> Result<(), Error> {
    let points = vertices.map(|p| {
        Point::new(
            p.x - f64::from(raster.origin_x),
            p.y - f64::from(raster.origin_y),
        )
    });
    let slopes = [
        points[1].sub(points[0]).normal(),
        points[2].sub(points[1]).normal(),
        points[0].sub(points[2]).normal(),
    ];
    let clockwise = slopes[2].compare(slopes[0]) < 0;
    let offset = -thickness / 2.;
    let shifted = |p: Point, slope: Point| {
        let adjust = Point::new(-slope.y * offset, slope.x * offset);
        if clockwise {
            p.sub(adjust)
        } else {
            p.add(adjust)
        }
    };
    let mut inset = Vec::new();
    for out in 0..3 {
        let incoming = (out + 2) % 3;
        let a = slopes[incoming];
        let b = slopes[out];
        let p = shifted(points[out], a);
        let q = shifted(points[out], b);
        let divisor = a.x * b.y - a.y * b.x;
        if divisor == 0. {
            return Err(Error::Unsupported);
        }
        let t = ((q.x - p.x) * b.y - (q.y - p.y) * b.x) / divisor;
        push(
            &mut inset,
            (Point::new(p.x + t * a.x, p.y + t * a.y), false),
        )?;
    }
    inset.dedup_by(|a, b| a.0 == b.0);
    if inset.len() < 3 {
        return stroke_points(raster, inset, thickness);
    }
    // z2d's close-path special case joins the penultimate point back through
    // the initial point when the final line already returned there. Retain
    // both joins even if floating rounding leaves only two distinct points.
    if inset.last().unwrap().0 == inset[0].0 {
        inset.pop();
    }
    let mut outer = Vec::new();
    let mut inner = Vec::new();
    let mut direction = None;
    // Normal line processing starts at the second vertex; closing supplies
    // the final and initial joins. Inner plotting reverses contour order.
    for i in 1..=inset.len() {
        let p0 = inset[(i - 1) % inset.len()].0;
        let p1 = inset[i % inset.len()].0;
        let p2 = inset[(i + 1) % inset.len()].0;
        if p0 == p1 || p1 == p2 {
            direction.get_or_insert(false);
            continue;
        }
        let a = Face::new(p0, p1, thickness);
        let b = Face::new(p1, p2, thickness);
        let compare = a.slope.compare(b.slope);
        let clockwise = compare < 0;
        let polygon_clockwise = *direction.get_or_insert(clockwise);
        let (outside, inside) = if clockwise == polygon_clockwise {
            (&mut outer, &mut inner)
        } else {
            (&mut inner, &mut outer)
        };
        let ai = if clockwise { a.p1_cw } else { a.p1_ccw };
        let ao = if clockwise { a.p1_ccw } else { a.p1_cw };
        let bi = if clockwise { b.p0_cw } else { b.p0_ccw };
        let bo = if clockwise { b.p0_ccw } else { b.p0_cw };
        if compare == 0 {
            push(outside, ao)?;
            push(inside, ai)?;
            continue;
        }
        let incoming = a.slope.normal();
        let outgoing = b.slope.normal();
        if 2. <= 100. * (1. + (incoming.x * outgoing.x + incoming.y * outgoing.y)) {
            push(outside, a.intersect(b, clockwise))?;
        } else {
            push(outside, ao)?;
            push(outside, bo)?;
        }
        push(inside, ai)?;
        push(inside, p1)?;
        push(inside, bi)?;
    }
    inner.reverse();
    fill_contours(raster, &[&outer, &inner])
}
