// MIT; adapted from Ghostty draw/powerline.zig at
// 6ba144158577d033ccb6ed4b574e4013957c9161. See LICENSE.ghostty.
use crate::{
    Error, Metrics, Raster, curves,
    vector::{self, Node, Point},
};
fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}
fn fill(r: &mut Raster, points: &[Point]) -> Result<(), Error> {
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(points.len())
        .map_err(|_| Error::Allocation)?;
    for (i, &p) in points.iter().enumerate() {
        nodes.push(if i == 0 { Node::Move(p) } else { Node::Line(p) });
    }
    vector::fill_path(r, &nodes)
}
pub(crate) fn draw(r: &mut Raster, m: Metrics, cp: u32) -> Result<(), Error> {
    let w = f64::from(m.cell_width);
    let h = f64::from(m.cell_height);
    let t = f64::from(m.box_thickness);
    match cp {
        0xe0b0 => fill(r, &[p(0., 0.), p(w, h / 2.), p(0., h)])?,
        0xe0b2 => fill(r, &[p(w, 0.), p(0., h / 2.), p(w, h)])?,
        0xe0b8 => fill(r, &[p(0., 0.), p(w, h), p(0., h)])?,
        0xe0ba => fill(r, &[p(w, 0.), p(w, h), p(0., h)])?,
        0xe0bc => fill(r, &[p(0., 0.), p(w, 0.), p(0., h)])?,
        0xe0be => fill(r, &[p(0., 0.), p(w, 0.), p(w, h)])?,
        0xe0b9 | 0xe0bf => curves::draw(r, m, 0x2572)?,
        0xe0bb | 0xe0bd => curves::draw(r, m, 0x2571)?,
        0xe0b1 | 0xe0b3 => vector::stroke(
            r,
            &[
                Node::Move(p(0., 0.)),
                Node::Line(p(w, h / 2.)),
                Node::Line(p(0., h)),
            ],
            t,
        )?,
        0xe0b4 | 0xe0b6 => {
            // Zig evaluates (sqrt2-1)*4/3 at comptime precision, then rounds to f64.
            let c = 0.552_284_749_830_793_5;
            let radius = w.min(h / 2.);
            vector::fill_path(
                r,
                &[
                    Node::Move(p(0., 0.)),
                    Node::Curve(
                        p(radius * c, 0.),
                        p(radius, radius - radius * c),
                        p(radius, radius),
                    ),
                    Node::Line(p(radius, h - radius)),
                    Node::Curve(
                        p(radius, h - radius + radius * c),
                        p(radius * c, h),
                        p(0., h),
                    ),
                ],
            )?;
        }
        0xe0b5 | 0xe0b7 => {
            let c = 0.552_284_749_830_793_5;
            let radius = w.min(h / 2.);
            vector::inner_stroke(
                r,
                &[
                    Node::Move(p(0., 0.)),
                    Node::Line(p(1., 0.)),
                    Node::Curve(
                        p(radius * c, 0.),
                        p(radius, radius - radius * c),
                        p(radius, radius),
                    ),
                    Node::Line(p(radius, h - radius)),
                    Node::Curve(
                        p(radius, h - radius + radius * c),
                        p(radius * c, h),
                        p(1., h),
                    ),
                    Node::Line(p(0., h)),
                ],
                t,
            )?;
        }
        0xe0d2 | 0xe0d4 => {
            fill(
                r,
                &[
                    p(0., 0.),
                    p(w, 0.),
                    p(w / 2., h / 2. - t / 2.),
                    p(0., h / 2. - t / 2.),
                ],
            )?;
            fill(
                r,
                &[
                    p(0., h),
                    p(w, h),
                    p(w / 2., h / 2. + t / 2.),
                    p(0., h / 2. + t / 2.),
                ],
            )?;
        }
        _ => return Err(Error::Unsupported),
    }
    if matches!(cp, 0xe0b3 | 0xe0b6 | 0xe0b7 | 0xe0d4) {
        for row in r.alpha.chunks_exact_mut(r.width as usize) {
            row.reverse();
        }
    }
    Ok(())
}
