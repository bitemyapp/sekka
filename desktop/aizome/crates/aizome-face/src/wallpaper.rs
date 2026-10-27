//! Wallpapers made ready to blit: each output's picture decoded, scaled to
//! cover its size in pixels and cropped about its centre, on a thread,
//! once per output size and theme. The desktop shows its colour until the
//! picture is ready, and every redraw after copies pixels one to one.

use std::path::{Path, PathBuf};

use aizome_kit::Image;

/// A wallpaper at one output's size.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WallKey {
    pub path: PathBuf,
    /// The output's size in pixels.
    pub width: u32,
    pub height: u32,
}

/// Read, decode and scale `path` to cover `width × height` pixels: straight
/// RGBA, rows `width × 4` bytes apart. `None` if it doesn't read.
pub fn prepare(path: &Path, width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    let (rgba, w, h) = decode(path).ok()?;
    Some(cover(&rgba, w, h, width, height))
}

/// Check the actual pixels with the same decoder the desktop uses, before
/// replacing a wallpaper setting. A readable header alone is not enough.
/// This checks the file now; callers still need it at this path afterwards.
pub fn validate(path: &Path) -> Result<(), String> {
    decode(path).map(|_| ())
}

fn decode(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
    let image = Image::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::InvalidData {
            "Unsupported or damaged image (SVG isn't supported).".to_owned()
        } else {
            e.to_string()
        }
    })?;
    let (w, h) = image.pixel_size();
    if w == 0 || h == 0 {
        return Err("The wallpaper has no pixels.".into());
    }
    let rgba = image.to_rgba8().ok_or("The wallpaper's image data couldn't be decoded.")?;
    Ok((rgba, w, h))
}

/// The image ready to draw at `scale` pixels a point.
pub fn image(rgba: &[u8], width: u32, height: u32, scale: f64) -> Option<Image> {
    Some(Image::from_rgba(width, height, rgba)?.with_scale(1.0 / scale.max(0.25)))
}

/// One output pixel's sources along an axis: the first source pixel and
/// each one's weight from there.
struct Taps {
    first: usize,
    weights: Vec<f32>,
}

/// How each of `out` pixels samples the source span `from .. from + len`
/// (of a source `size` long): the area it covers when shrinking, a linear
/// blend of the two nearest when growing.
fn taps(out: u32, from: f64, len: f64, size: u32) -> Vec<Taps> {
    let step = len / out as f64;
    let last = size as isize - 1;
    (0..out)
        .map(|i| {
            let (a, b) = (from + i as f64 * step, from + (i + 1) as f64 * step);
            if step >= 1.0 {
                let first = a.floor().max(0.0) as usize;
                let end = (b.ceil() as usize).min(size as usize);
                let mut weights: Vec<f32> =
                    (first..end).map(|p| ((b.min(p as f64 + 1.0) - a.max(p as f64)).max(0.0)) as f32).collect();
                let sum: f32 = weights.iter().sum();
                if sum > 0.0 {
                    weights.iter_mut().for_each(|w| *w /= sum);
                }
                Taps { first, weights }
            } else {
                let c = (a + b) / 2.0 - 0.5;
                let i0 = c.floor();
                let t = (c - i0) as f32;
                let i0 = (i0 as isize).clamp(0, last);
                let i1 = (i0 + 1).min(last);
                if i1 == i0 {
                    Taps { first: i0 as usize, weights: vec![1.0] }
                } else {
                    Taps { first: i0 as usize, weights: vec![1.0 - t, t] }
                }
            }
        })
        .collect()
}

/// Scale `src` (`sw × sh`) to cover `dw × dh`, cropped about its centre.
pub fn cover(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let k = (dw as f64 / sw as f64).max(dh as f64 / sh as f64);
    let (cw, ch) = (dw as f64 / k, dh as f64 / k);
    let (x0, y0) = ((sw as f64 - cw) / 2.0, (sh as f64 - ch) / 2.0);
    let cols = taps(dw, x0, cw, sw);
    let rows = taps(dh, y0, ch, sh);
    let stride = sw as usize * 4;
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    // A row at a time: the source rows blended down into one, then across.
    let mut line = vec![0f32; stride];
    for (y, r) in rows.iter().enumerate() {
        line.iter_mut().for_each(|v| *v = 0.0);
        for (j, w) in r.weights.iter().enumerate() {
            let row = &src[(r.first + j) * stride..(r.first + j + 1) * stride];
            for (acc, s) in line.iter_mut().zip(row) {
                *acc += *s as f32 * w;
            }
        }
        let dst = &mut out[y * dw as usize * 4..(y + 1) * dw as usize * 4];
        for (x, c) in cols.iter().enumerate() {
            let mut px = [0f32; 4];
            for (j, w) in c.weights.iter().enumerate() {
                let at = (c.first + j) * 4;
                for (ch, p) in px.iter_mut().enumerate() {
                    *p += line[at + ch] * w;
                }
            }
            for (ch, p) in px.iter().enumerate() {
                dst[x * 4 + ch] = p.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// How to draw a prepared wallpaper into `bounds` (points): pixel for
/// pixel when it was made for them (the cheapest copy), else smoothly
/// (the output changed and a new one is on its way).
pub fn options(image: &Image, bounds: aizome_kit::layout::Rect, scale: f64) -> aizome_kit::sidestep_ui::ImageOptions {
    let (w, h) = image.pixel_size();
    let exact = (w as f64 - bounds.width() * scale).abs() < 0.5 && (h as f64 - bounds.height() * scale).abs() < 0.5;
    aizome_kit::sidestep_ui::ImageOptions { smooth: !exact, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_covers_and_crops_about_its_centre() {
        // 4 × 2: left half black, right half white; covering 2 × 2 keeps
        // the middle 2 × 2.
        let mut src = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0 } else { 255 };
                src.extend([v, v, v, 255]);
            }
        }
        let out = cover(&src, 4, 2, 2, 2);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        assert_eq!(&out[4..8], &[255, 255, 255, 255]);
        // Shrunk by half: each pixel the average of the four it covers.
        let mut src = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let v = if (x + y) % 2 == 0 { 0 } else { 200 };
                src.extend([v, v, v, 255]);
            }
        }
        let out = cover(&src, 4, 4, 2, 2);
        assert!(out.chunks(4).all(|p| p[0] == 100 && p[3] == 255), "{out:?}");
        // Grown: smooth, within the source's range, the right size.
        let out = cover(&src, 4, 4, 10, 10);
        assert_eq!(out.len(), 400);
        assert!(out.chunks(4).all(|p| p[0] <= 200 && p[3] == 255));
    }
}
