//! Eight corner triangles from Ghostty's MIT-licensed geometric_shapes.zig,
//! commit 6ba144158577d033ccb6ed4b574e4013957c9161. See LICENSE.ghostty.
use crate::{
    Error, Metrics, Raster,
    vector::{self, Node, Point},
};

pub(super) fn draw(raster: &mut Raster, metrics: Metrics, cp: u32) -> Result<(), Error> {
    let w = f64::from(metrics.cell_width);
    let h = f64::from(metrics.cell_height);
    let vertices = match cp {
        0x25e4 | 0x25f8 => [Point::new(0., 0.), Point::new(0., h), Point::new(w, 0.)],
        0x25e5 | 0x25f9 => [Point::new(0., 0.), Point::new(w, h), Point::new(w, 0.)],
        0x25e3 | 0x25fa => [Point::new(0., 0.), Point::new(0., h), Point::new(w, h)],
        0x25e2 | 0x25ff => [Point::new(0., h), Point::new(w, h), Point::new(w, 0.)],
        _ => return Err(Error::Unsupported),
    };
    if cp <= 0x25e5 {
        vector::fill_path(
            raster,
            &[
                Node::Move(vertices[0]),
                Node::Line(vertices[1]),
                Node::Line(vertices[2]),
            ],
        )
    } else {
        vector::inner_stroke_triangle(raster, vertices, f64::from(metrics.box_thickness))
    }
}
