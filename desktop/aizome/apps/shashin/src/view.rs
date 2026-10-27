//! Where the picture sits in its view: the zoom, the picture's point at the
//! view's centre, fitting, zooming about a point and panning.
//!
//! Everything here is in device pixels: a zoom of 1 (100%) shows one of the
//! picture's pixels on one of the screen's, so 100% is crisp at any scale.
//! The picture's corner always lands on a whole device pixel, so a picture
//! scaled to the zoom ahead of time is drawn one to one.

/// How large the picture shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// As large as fits the view, but never above 100%.
    Fit,
    /// Device pixels per picture pixel.
    Fixed(f64),
}

impl Zoom {
    /// The zoom in device pixels per picture pixel, for a picture of
    /// `image` pixels in a view of `view` device pixels.
    pub fn resolve(self, image: (f64, f64), view: (f64, f64)) -> f64 {
        match self {
            Zoom::Fit => fit(image, view),
            Zoom::Fixed(z) => z,
        }
    }

    /// Bits that compare equal when the zoom is the same (for hashing).
    pub fn bits(self) -> u64 {
        match self {
            Zoom::Fit => u64::MAX,
            Zoom::Fixed(z) => z.to_bits(),
        }
    }
}

pub const MIN_ZOOM: f64 = 1.0 / 64.0;
pub const MAX_ZOOM: f64 = 32.0;

/// The zooms ❖+ and ❖− step through.
pub const STEPS: [f64; 21] = [
    1.0 / 32.0,
    1.0 / 24.0,
    1.0 / 16.0,
    1.0 / 12.0,
    1.0 / 8.0,
    1.0 / 6.0,
    0.25,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    1.0,
    1.5,
    2.0,
    3.0,
    4.0,
    6.0,
    8.0,
    12.0,
    16.0,
    24.0,
    32.0,
];

/// The zoom that fits `image` into `view`, no larger than 100%.
pub fn fit(image: (f64, f64), view: (f64, f64)) -> f64 {
    if image.0 <= 0.0 || image.1 <= 0.0 || view.0 <= 0.0 || view.1 <= 0.0 {
        return 1.0;
    }
    (view.0 / image.0).min(view.1 / image.1).clamp(MIN_ZOOM, 1.0)
}

/// The next step in from `z`.
pub fn step_in(z: f64) -> f64 {
    STEPS.iter().copied().find(|s| *s > z * 1.0001).unwrap_or(MAX_ZOOM)
}

/// The next step out from `z`.
pub fn step_out(z: f64) -> f64 {
    STEPS.iter().rev().copied().find(|s| *s < z / 1.0001).unwrap_or(MIN_ZOOM.max(z.min(STEPS[0])))
}

pub fn clamp_zoom(z: f64) -> f64 {
    z.clamp(MIN_ZOOM, MAX_ZOOM)
}

fn axis_origin(image: f64, view: f64, z: f64, center: f64) -> f64 {
    let size = image * z;
    if size <= view { ((view - size) / 2.0).round() } else { (view / 2.0 - center * z).round().clamp(view - size, 0.0) }
}

/// The picture's top left corner in the view, in whole device pixels:
/// centred along an axis it doesn't fill, else placed by `center`.
pub fn origin(image: (f64, f64), view: (f64, f64), z: f64, center: (f64, f64)) -> (f64, f64) {
    (axis_origin(image.0, view.0, z, center.0), axis_origin(image.1, view.1, z, center.1))
}

fn axis_center(image: f64, view: f64, z: f64, center: f64) -> f64 {
    let size = image * z;
    if size <= view {
        image / 2.0
    } else {
        let half = view / 2.0 / z;
        center.clamp(half, image - half)
    }
}

/// `center` kept where the picture still covers the view along each axis
/// it's larger than (and in the middle along the others).
pub fn clamp_center(image: (f64, f64), view: (f64, f64), z: f64, center: (f64, f64)) -> (f64, f64) {
    (axis_center(image.0, view.0, z, center.0), axis_center(image.1, view.1, z, center.1))
}

/// The centre after zooming from `z` to `to` about `at` (a point of the
/// view): the picture's point under it stays under it.
pub fn zoom_about(
    image: (f64, f64),
    view: (f64, f64),
    z: f64,
    center: (f64, f64),
    to: f64,
    at: (f64, f64),
) -> (f64, f64) {
    let o = origin(image, view, z, center);
    let p = ((at.0 - o.0) / z, (at.1 - o.1) / z);
    let o2 = (at.0 - p.0 * to, at.1 - p.1 * to);
    let c2 = ((view.0 / 2.0 - o2.0) / to, (view.1 / 2.0 - o2.1) / to);
    clamp_center(image, view, to, c2)
}

/// The centre after the content moves by `by` device pixels (positive:
/// towards the picture's end, as a scroll moves it).
pub fn pan(image: (f64, f64), view: (f64, f64), z: f64, center: (f64, f64), by: (f64, f64)) -> (f64, f64) {
    clamp_center(image, view, z, (center.0 + by.0 / z, center.1 + by.1 / z))
}

/// The part of the picture scaled by `z` that shows, in its device
/// pixels: `(x, y, w, h)`.
pub fn visible(image: (f64, f64), view: (f64, f64), z: f64, center: (f64, f64)) -> (f64, f64, f64, f64) {
    let o = origin(image, view, z, center);
    let (w, h) = (image.0 * z, image.1 * z);
    let x0 = (-o.0).clamp(0.0, w);
    let y0 = (-o.1).clamp(0.0, h);
    let x1 = (view.0 - o.0).clamp(0.0, w);
    let y1 = (view.1 - o.1).clamp(0.0, h);
    (x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// A picture's size after `turns` quarter turns.
pub fn turned(dims: (f64, f64), turns: u8) -> (f64, f64) {
    if turns % 2 == 1 { (dims.1, dims.0) } else { dims }
}

/// A rectangle `(x, y, w, h)` of a picture turned `turns` quarter turns
/// clockwise, as the same pixels of the picture before turning (`dims` its
/// size before turning).
pub fn unturn_rect(rect: (f64, f64, f64, f64), dims: (f64, f64), turns: u8) -> (f64, f64, f64, f64) {
    let (x, y, w, h) = rect;
    let (bw, bh) = dims;
    match turns % 4 {
        // Turned once: a pixel (x, y) came from (y, bh − x).
        1 => (y, bh - x - w, h, w),
        2 => (bw - x - w, bh - y - h, w, h),
        // Three times: (x, y) came from (bw − y, x).
        3 => (bw - y - h, x, h, w),
        _ => rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_without_enlarging() {
        assert_eq!(fit((4000.0, 3000.0), (2000.0, 1000.0)), 1.0 / 3.0);
        assert_eq!(fit((100.0, 50.0), (2000.0, 1000.0)), 1.0);
        assert_eq!(origin((100.0, 50.0), (2000.0, 1000.0), 1.0, (50.0, 25.0)), (950.0, 475.0));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let (img, view) = ((4000.0, 3000.0), (1000.0, 800.0));
        let z = 0.5;
        let c = clamp_center(img, view, z, (2000.0, 1500.0));
        let at = (700.0, 300.0);
        let o = origin(img, view, z, c);
        let p = ((at.0 - o.0) / z, (at.1 - o.1) / z);
        let c2 = zoom_about(img, view, z, c, 1.0, at);
        let o2 = origin(img, view, 1.0, c2);
        assert!((o2.0 + p.0 - at.0).abs() <= 1.0 && (o2.1 + p.1 - at.1).abs() <= 1.0, "{o2:?} {p:?}");
    }

    #[test]
    fn panning_stops_at_the_edges() {
        let (img, view) = ((4000.0, 3000.0), (1000.0, 800.0));
        let c = pan(img, view, 1.0, (2000.0, 1500.0), (-1e6, 1e6));
        assert_eq!(c, (500.0, 2600.0));
        assert_eq!(origin(img, view, 1.0, c), (0.0, -2200.0));
        assert_eq!(visible(img, view, 1.0, c), (0.0, 2200.0, 1000.0, 800.0));
    }

    #[test]
    fn steps_go_through_100_percent() {
        assert_eq!(step_in(0.9), 1.0);
        assert_eq!(step_out(1.0), 2.0 / 3.0);
        assert_eq!(step_in(1.0), 1.5);
        assert_eq!(step_in(32.0), MAX_ZOOM);
    }

    #[test]
    fn rectangles_turn_back() {
        // A 4 × 2 picture turned once is 2 × 4; its top row (y 0) is the
        // original's left column, read bottom up.
        let dims = (4.0, 2.0);
        assert_eq!(unturn_rect((0.0, 0.0, 2.0, 1.0), dims, 1), (0.0, 0.0, 1.0, 2.0));
        assert_eq!(unturn_rect((0.0, 3.0, 2.0, 1.0), dims, 1), (3.0, 0.0, 1.0, 2.0));
        assert_eq!(unturn_rect((0.0, 0.0, 1.0, 1.0), dims, 2), (3.0, 1.0, 1.0, 1.0));
        assert_eq!(unturn_rect((0.0, 0.0, 2.0, 1.0), dims, 3), (3.0, 0.0, 1.0, 2.0));
        assert_eq!(turned(dims, 3), (2.0, 4.0));
    }
}
