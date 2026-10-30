//! Immutable, bounded Alpha8 masks for worker-rasterized grid artwork.
//! No graphics object, recoloring allocation, or mutable cross-thread cache.
use super::{Canvas, blend, premultiplied};
use crate::protocol::{Color, Rect};

#[derive(Debug)]
pub struct AlphaMask {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
}
impl AlphaMask {
    pub const MAX_BYTES: usize = 4 * 1024 * 1024;
    pub fn new(width: u32, height: u32, alpha: Vec<u8>) -> Option<Self> {
        let len = (width as usize).checked_mul(height as usize)?;
        if width == 0
            || height == 0
            || len > Self::MAX_BYTES
            || alpha.len() != len
            || alpha.capacity() > Self::MAX_BYTES
        {
            return None;
        }
        Some(Self { width, height, alpha })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn alpha(&self) -> &[u8] {
        &self.alpha
    }
    pub fn storage_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.alpha.capacity()
    }
}

/// Nearest sampling preserves final-scale device pixels exactly. Resampling is
/// used for a coherent retained frame while the output scale changes; workers
/// replace it with a newly rasterized mask at the target physical dimensions.
pub(super) fn draw(canvas: &mut Canvas, mask: &AlphaMask, dst: &Rect, clip: &Rect, color: Color) {
    let Some((x0, y0, x1, y1)) = canvas.pixels(&dst.intersect(clip)) else { return };
    let left = dst.x0 as f64 * canvas.scale as f64 - canvas.x0 as f64;
    let top = (dst.y0 as f64 - canvas.origin_y as f64) * canvas.scale as f64 - canvas.y0 as f64;
    let width = (dst.x1 as f64 - dst.x0 as f64) * canvas.scale as f64;
    let height = (dst.y1 as f64 - dst.y0 as f64) * canvas.scale as f64;
    if !left.is_finite()
        || !top.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
    {
        return;
    }
    let alpha = (color[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    if alpha == 0 {
        return;
    }
    let solid = premultiplied([color[0], color[1], color[2], 1.0]);
    // Final grid geometry is one device pixel per coverage sample. Avoid
    // per-pixel floating point division in that common case; all slice bounds
    // are still checked. The tolerance only absorbs f32 protocol conversion.
    let exact = (width - f64::from(mask.width)).abs() < 0.001 && (height - f64::from(mask.height)).abs() < 0.001;
    if exact {
        // Match Canvas::pixels exactly, including half-pixel widget origins.
        // Repeating the f32 transform in f64 and floor-sampling can choose the
        // other side of a pixel-center tie and shift an internal stroke.
        let left = (dst.x0 * canvas.scale - canvas.x0 as f32 - 0.5).ceil() as i64;
        let top = ((dst.y0 - canvas.origin_y) * canvas.scale - canvas.y0 as f32 - 0.5).ceil() as i64;
        for y in y0..y1 {
            let sy = y as i64 - top;
            if !(0..i64::from(mask.height)).contains(&sy) {
                continue;
            }
            let sy = sy as usize;
            let sx = (x0 as i64 - left).max(0) as usize;
            if sx >= mask.width as usize {
                continue;
            }
            let count = (x1 - x0).min(mask.width as usize - sx);
            let source = sy * mask.width as usize + sx;
            for (pixel, &cov) in canvas.px[y * canvas.stride + x0..y * canvas.stride + x0 + count]
                .iter_mut()
                .zip(&mask.alpha[source..source + count])
            {
                let a = if alpha == 255 { u32::from(cov) } else { (u32::from(cov) * alpha * 257 + 32896) >> 16 };
                if a != 0 {
                    *pixel = if a == 255 { solid } else { blend(*pixel, solid, a) };
                }
            }
        }
        return;
    }
    for y in y0..y1 {
        let sy =
            (((y as f64 + 0.5 - top) * mask.height as f64 / height).floor() as usize).min(mask.height as usize - 1);
        for x in x0..x1 {
            let sx =
                (((x as f64 + 0.5 - left) * mask.width as f64 / width).floor() as usize).min(mask.width as usize - 1);
            let cov = u32::from(mask.alpha[sy * mask.width as usize + sx]);
            let a = if alpha == 255 { cov } else { (cov * alpha * 257 + 32896) >> 16 };
            if a != 0 {
                let pixel = &mut canvas.px[y * canvas.stride + x];
                *pixel = if a == 255 { solid } else { blend(*pixel, solid, a) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validated_owned_mask_and_premultiplied_blend() {
        assert!(AlphaMask::new(2, 2, vec![0; 3]).is_none());
        assert!(AlphaMask::new(0, 1, vec![]).is_none());
        assert!(AlphaMask::new(u32::MAX, u32::MAX, vec![]).is_none());
        let m = AlphaMask::new(2, 2, vec![0, 64, 128, 255]).unwrap();
        let mut pixels = vec![0; 4];
        draw(
            &mut Canvas::new(&mut pixels, 2, 2, 0.0, 1.0),
            &m,
            &Rect::new(0.0, 0.0, 2.0, 2.0),
            &Rect::new(0.0, 0.0, 2.0, 2.0),
            [1.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(
            pixels.iter().map(|p| p.to_ne_bytes()).collect::<Vec<_>>(),
            [[0, 0, 0, 0], [64, 0, 0, 64], [128, 0, 0, 128], [255, 0, 0, 255]]
        );
    }
    #[test]
    fn final_scale_fractional_widget_origins_preserve_each_mask_sample() {
        let mask = AlphaMask::new(3, 2, vec![255, 0, 64, 128, 192, 255]).unwrap();
        for scale in [1.0f32, 1.25, 2.0] {
            for (x, y) in [(4, 7), (71, 103)] {
                let left = (x as f32 + 0.5) / scale;
                let top = (y as f32 + 0.5) / scale;
                let dst = Rect::new(left, top, left + 3.0 / scale, top + 2.0 / scale);
                let clip = Rect::new(0.0, 0.0, 128.0 / scale, 128.0 / scale);
                let mut pixels = vec![0u32; 128 * 128];
                draw(&mut Canvas::new(&mut pixels, 128, 128, 0.0, scale), &mask, &dst, &clip, [1.0; 4]);
                for yy in 0..128 {
                    for xx in 0..128 {
                        let expected = if (x..x + 3).contains(&xx) && (y..y + 2).contains(&yy) {
                            mask.alpha[((yy - y) * 3 + xx - x) as usize]
                        } else {
                            0
                        };
                        assert_eq!(
                            pixels[(yy * 128 + xx) as usize].to_ne_bytes(),
                            [expected; 4],
                            "scale{scale} origin{x},{y} pixel{xx},{yy}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn half_pixel_epsilon_negative_origins_and_tile_offsets_preserve_samples() {
        let mask = AlphaMask::new(3, 2, vec![255, 17, 64, 128, 192, 237]).unwrap();
        for scale in [1.0f32, 1.25, 2.0] {
            for start in [-2.5f32, -0.5, 0.5, 5.5, 12.5] {
                for epsilon in [-0.001f32, 0.0, 0.001] {
                    let x = (start + epsilon) / scale;
                    let y = (start - epsilon) / scale;
                    let dst = Rect::new(x, y, x + 3.0 / scale, y + 2.0 / scale);
                    let clip = Rect::new(0.0, 0.0, 16.0 / scale, 16.0 / scale);
                    let mut full = vec![0u32; 16 * 16];
                    draw(&mut Canvas::new(&mut full, 16, 16, 0.0, scale), &mask, &dst, &clip, [1.0; 4]);
                    let first_x = (x * scale - 0.5).ceil() as i32;
                    let first_y = (y * scale - 0.5).ceil() as i32;
                    for yy in 0..16 {
                        for xx in 0..16 {
                            let sx = xx - first_x;
                            let sy = yy - first_y;
                            let expected = if (0..3).contains(&sx) && (0..2).contains(&sy) {
                                mask.alpha[(sy * 3 + sx) as usize]
                            } else {
                                0
                            };
                            assert_eq!(
                                full[(yy * 16 + xx) as usize].to_ne_bytes(),
                                [expected; 4],
                                "scale{scale} start{start} epsilon{epsilon} at{xx},{yy}"
                            );
                        }
                    }
                    for y0 in 0..16 {
                        for x0 in [0, 3, 7] {
                            let width = 16 - x0;
                            let mut tile = vec![0u32; width as usize];
                            let mut canvas = Canvas::new(&mut tile, width, 1, 0.0, scale);
                            canvas.x0 = x0 as i32;
                            canvas.y0 = y0;
                            draw(&mut canvas, &mask, &dst, &clip, [1.0; 4]);
                            assert_eq!(
                                tile,
                                full[(y0 as usize * 16 + x0 as usize)..((y0 + 1) as usize * 16)],
                                "tile origin{x0},{y0}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn arbitrary_damage_bands_equal_full_and_transition_is_deliberate() {
        let m = AlphaMask::new(3, 2, vec![0, 64, 255, 128, 192, 0]).unwrap();
        for scale in [1.0, 1.25, 2.0] {
            let dst = Rect::new(2.0, 3.0, 8.0, 7.0);
            let mut all = vec![0xff202020; 20 * 20];
            let mut split = all.clone();
            draw(
                &mut Canvas::new(&mut all, 20, 20, 0.0, scale),
                &m,
                &dst,
                &Rect::new(0.0, 0.0, 20.0, 20.0),
                [0.1, 0.7, 1.0, 0.6],
            );
            for y in 0..20 {
                let clip = Rect::new(0.0, y as f32 / scale, 20.0, (y + 1) as f32 / scale);
                let mut band = Canvas::new(&mut split[y * 20..(y + 1) * 20], 20, 1, 0.0, scale);
                band.y0 = y as i32;
                draw(&mut band, &m, &dst, &clip, [0.1, 0.7, 1.0, 0.6]);
            }
            assert_eq!(all, split);
        }
    }
}
