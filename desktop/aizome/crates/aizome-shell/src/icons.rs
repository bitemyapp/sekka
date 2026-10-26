//! Apps' own icons, read from their files: PNG and the other formats the
//! kit reads as they are, SVG drawn at the size wanted with resvg (many
//! apps install only a scalable icon). For the Deskbar's apps that Aizome
//! has no glyph for, and apps' status items.

use std::path::Path;

use aizome_kit::Image;
use resvg::{tiny_skia, usvg};

/// The icon in `path`, for `pt` points square on a `scale`× screen.
pub fn read(path: &Path, pt: u32, scale: u32) -> Option<Image> {
    let vector = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    if !vector {
        return Image::open(path).ok();
    }
    let px = pt.checked_mul(scale.max(1))?;
    let (w, h, rgba) = rasterize(&std::fs::read(path).ok()?, px)?;
    Some(Image::from_rgba(w, h, &rgba)?.with_scale(1.0 / f64::from(scale.max(1))))
}

/// SVG source (or gzipped, `.svgz`) drawn `px` × `px`, centred and kept
/// in proportion, as straight RGBA.
pub fn rasterize(data: &[u8], px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    let size = tree.size();
    let k = (px as f32 / size.width()).min(px as f32 / size.height());
    let (dx, dy) = ((px as f32 - size.width() * k) / 2.0, (px as f32 - size.height() * k) / 2.0);
    resvg::render(&tree, tiny_skia::Transform::from_row(k, 0.0, 0.0, k, dx, dy), &mut pixmap.as_mut());
    // tiny-skia's pixels are premultiplied; the kit takes straight alpha.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some((px, px, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 24">
        <circle cx="24" cy="12" r="12" fill="#3060c0"/></svg>"##;

    #[test]
    fn an_svg_is_drawn_at_the_size_wanted() {
        let dir = std::env::temp_dir().join(format!("aizome-icons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("app.svg");
        std::fs::write(&file, CIRCLE).unwrap();
        let image = read(&file, 16, 2).expect("an SVG reads");
        assert_eq!(image.pixel_size(), (32, 32));
        assert_eq!(image.size(), aizome_kit::layout::Size::new(16.0, 16.0));
        // Wider than tall: centred, so the middle is drawn and the top row isn't.
        let (_, _, rgba) = rasterize(CIRCLE.as_bytes(), 32).unwrap();
        let at = |x: usize, y: usize| &rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(at(16, 16), [0x30, 0x60, 0xc0, 0xff]);
        assert_eq!(at(16, 0)[3], 0);
        assert!(read(&dir.join("missing.svg"), 16, 2).is_none());
        std::fs::write(dir.join("broken.svg"), "<svg").unwrap();
        assert!(read(&dir.join("broken.svg"), 16, 2).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
