// Adapted from Ghostty draw/box.zig at 6ba144158577d033ccb6ed4b574e4013957c9161.
// MIT; see LICENSE.ghostty. Raster primitives live in the MPL-2.0 vector module.
use crate::{
    Error, Metrics, Raster,
    vector::{self, Node, Point},
};
fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}
pub(crate) fn draw(r: &mut Raster, m: Metrics, cp: u32) -> Result<(), Error> {
    let w = f64::from(m.cell_width);
    let h = f64::from(m.cell_height);
    let t = f64::from(m.box_thickness);
    if cp >= 0x2571 {
        let sx = (w / h).min(1.);
        let sy = (h / w).min(1.);
        if cp == 0x2571 || cp == 0x2573 {
            vector::stroke(
                r,
                &[
                    Node::Move(p(w + 0.5 * sx, -0.5 * sy)),
                    Node::Line(p(-0.5 * sx, h + 0.5 * sy)),
                ],
                t,
            )?;
        }
        if cp == 0x2572 || cp == 0x2573 {
            vector::stroke(
                r,
                &[
                    Node::Move(p(-0.5 * sx, -0.5 * sy)),
                    Node::Line(p(w + 0.5 * sx, h + 0.5 * sy)),
                ],
                t,
            )?;
        }
    } else {
        let x = f64::from(m.cell_width.saturating_sub(m.box_thickness) / 2) + t / 2.;
        let y = f64::from(m.cell_height.saturating_sub(m.box_thickness) / 2) + t / 2.;
        let radius = w.min(h) / 2.;
        let s = 0.25;
        let nodes = match cp {
            0x256d => [
                Node::Move(p(x, h)),
                Node::Line(p(x, y + radius)),
                Node::Curve(p(x, y + s * radius), p(x + s * radius, y), p(x + radius, y)),
                Node::Line(p(w, y)),
            ],
            0x256e => [
                Node::Move(p(x, h)),
                Node::Line(p(x, y + radius)),
                Node::Curve(p(x, y + s * radius), p(x - s * radius, y), p(x - radius, y)),
                Node::Line(p(0., y)),
            ],
            0x256f => [
                Node::Move(p(x, 0.)),
                Node::Line(p(x, y - radius)),
                Node::Curve(p(x, y - s * radius), p(x - s * radius, y), p(x - radius, y)),
                Node::Line(p(0., y)),
            ],
            0x2570 => [
                Node::Move(p(x, 0.)),
                Node::Line(p(x, y - radius)),
                Node::Curve(p(x, y - s * radius), p(x + s * radius, y), p(x + radius, y)),
                Node::Line(p(w, y)),
            ],
            _ => return Err(Error::Unsupported),
        };
        vector::stroke(r, &nodes, t)?;
    }
    Ok(())
}
