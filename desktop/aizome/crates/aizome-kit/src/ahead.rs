//! Windows drawn ahead of their showing ([`Cx::prepare_window`]): a
//! hidden window's view is laid out and drawn offscreen at idle, in bands
//! of whole device pixels a frame-slice each, into pictures the window
//! keeps; its next showing's first frame copies them in, pixel for pixel
//! (Sidestep copies pictures that land one to one on device pixels row by
//! row), and draws over them only what changed since. A panel whose first
//! opening would rasterise all of it (the shell's palette) costs a copy
//! instead.
//!
//! [`Cx::prepare_window`]: crate::Cx::prepare_window

use sidestep_ui::{BlendMode, Canvas, Color, Image};

use crate::layout::{Rect, Size};

/// About how many device pixels a band holds: a millisecond or so of
/// drawing, so a band drawn at idle holds the loop up little.
const BAND_PIXELS: f64 = 256.0 * 1024.0;

/// A hidden window drawn ahead: its pixels at `scale`, in bands across it
/// (rectangles in points, on whole device pixels), for its next showing.
#[derive(Clone, Debug)]
pub struct Picture {
    pub size: Size,
    pub scale: f64,
    pub bands: Vec<(Rect, Image)>,
}

impl Picture {
    /// Whether a frame drawing all of a window `size` points at `scale`
    /// can start from it.
    pub fn fits(&self, size: Size, scale: f64, damage: &[Rect]) -> bool {
        let whole = Rect::from_origin_size(crate::layout::Point::ZERO, size);
        self.size == size && self.scale == scale && damage.iter().any(|d| d.contains_rect(whole))
    }
}

/// A drawing ahead under way, in the engine: the scale, and the bands
/// drawn so far.
pub(crate) struct Drawing {
    pub scale: f64,
    pub bands: Vec<(Rect, Image)>,
    /// A band's timer is on its way.
    pub armed: bool,
}

impl Drawing {
    pub fn new(scale: f64) -> Drawing {
        Drawing { scale, bands: Vec::new(), armed: false }
    }
}

/// The bands a window `size` points big is drawn ahead in at `scale`:
/// full-width strips of whole device pixels, about [`BAND_PIXELS`] each
/// (one, at a scale that isn't whole: a strip's edge then falls between
/// device pixels).
pub(crate) fn bands(size: Size, scale: f64) -> Vec<Rect> {
    let (w, h) = ((size.width * scale).ceil().max(1.0), (size.height * scale).ceil().max(1.0));
    if scale.fract() != 0.0 {
        return vec![Rect::from_origin_size(crate::layout::Point::ZERO, size)];
    }
    let rows = (BAND_PIXELS / w).floor().clamp(1.0, h);
    let mut out = Vec::new();
    let mut y = 0.0;
    while y < h {
        let next = (y + rows).min(h);
        out.push(Rect::new(0.0, y / scale, size.width, (next / scale).min(size.height)));
        y = next;
    }
    out
}

/// The first frame of a window drawn ahead: `picture` copied in whole,
/// then `changed` (what changed since, in points) cleared and drawn
/// again with `paint`.
pub(crate) fn draw(picture: &Picture, changed: &[Rect], canvas: &mut Canvas, mut paint: impl FnMut(&mut Canvas)) {
    canvas.save();
    canvas.set_blend_mode(BlendMode::Copy);
    for (rect, image) in &picture.bands {
        canvas.draw_image(image, *rect);
    }
    canvas.restore();
    for r in changed {
        let r = r.expand();
        canvas.save();
        canvas.clip_rect(r);
        canvas.set_blend_mode(BlendMode::Clear);
        canvas.fill_rect(r, Color::TRANSPARENT);
        canvas.set_blend_mode(BlendMode::Normal);
        paint(canvas);
        canvas.restore();
    }
}

/// What changed in a window since it was drawn ahead, kept to a few
/// rectangles (past four, their union), or `None` once that's most of it
/// (it's drawn whole then).
pub(crate) fn changed(rects: &[Rect], size: Size) -> Option<Vec<Rect>> {
    let rects: Vec<Rect> = rects.iter().filter(|r| r.area() > 0.0).copied().collect();
    let rects = if rects.len() > 4 { rects.into_iter().reduce(|a, b| a.union(b)).into_iter().collect() } else { rects };
    let area: f64 = rects.iter().map(|r| r.area()).sum();
    (area < size.width * size.height / 2.0).then_some(rects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_cover_the_window_on_whole_device_pixels() {
        let size = Size::new(723.0, 483.0);
        let b = bands(size, 2.0);
        assert!(b.len() > 1, "{b:?}");
        assert_eq!(b.first().map(|r| r.y0), Some(0.0));
        assert_eq!(b.last().map(|r| r.y1), Some(483.0));
        for pair in b.windows(2) {
            assert_eq!(pair[0].y1, pair[1].y0);
            assert_eq!((pair[0].y1 * 2.0).fract(), 0.0);
        }
        assert_eq!(bands(size, 1.5).len(), 1);
    }

    #[test]
    fn what_changed_stays_a_few_rectangles_or_is_all_of_it() {
        let size = Size::new(100.0, 100.0);
        let few = [Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(50.0, 50.0, 60.0, 60.0)];
        assert_eq!(changed(&few, size), Some(few.to_vec()));
        let many: Vec<Rect> = (0..6).map(|i| Rect::new(i as f64, 0.0, i as f64 + 1.0, 1.0)).collect();
        assert_eq!(changed(&many, size), Some(vec![Rect::new(0.0, 0.0, 6.0, 1.0)]));
        assert_eq!(changed(&[Rect::new(0.0, 0.0, 100.0, 60.0)], size), None);
    }
}
