//! SVG icons, rasterized with resvg.

use std::path::Path;

use resvg::{tiny_skia, usvg};

/// An SVG file at `px` × `px`, as straight RGBA.
pub fn rasterize(path: &Path, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    rasterize_data(&std::fs::read(path).ok()?, px)
}

/// SVG source at `px` × `px`, as straight RGBA.
pub fn rasterize_data(data: &[u8], px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    let size = tree.size();
    let k = (px as f32 / size.width()).min(px as f32 / size.height());
    let dx = (px as f32 - size.width() * k) / 2.0;
    let dy = (px as f32 - size.height() * k) / 2.0;
    resvg::render(&tree, tiny_skia::Transform::from_row(k, 0.0, 0.0, k, dx, dy), &mut pixmap.as_mut());
    // tiny-skia is premultiplied; hand back straight alpha.
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some((px, px, rgba))
}
