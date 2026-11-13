//! Wallpapers made ready to blit: each output's picture decoded, scaled to
//! cover its size in pixels and cropped about its centre, on a thread,
//! once per output size and theme. The desktop shows its colour until the
//! picture is ready, and every redraw after copies pixels one to one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aizome_kit::Image;
use image::codecs::jpeg::JpegDecoder;
use image::metadata::Orientation;
use image::{ColorType, DynamicImage, ImageDecoder, ImageFormat};

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
    let (pixels, w, h) = decode(path).ok()?;
    Some(match pixels {
        Decoded::Rgb(rgb) => cover_pixels::<3>(&rgb, w, h, width, height),
        Decoded::Rgba(rgba) => cover(&rgba, w, h, width, height),
    })
}

/// Check the actual pixels with the same decoder the desktop uses, before
/// replacing a wallpaper setting. A readable header alone is not enough.
/// This checks the file now; callers still need it at this path afterwards.
pub fn validate(path: &Path) -> Result<(), String> {
    decode(path).map(|_| ())
}

/// A wallpaper's pixels, upright, as its file's decoder leaves them.
enum Decoded {
    /// A color JPEG's: opaque, three bytes a pixel.
    Rgb(Vec<u8>),
    /// Any other file's: straight RGBA.
    Rgba(Vec<u8>),
}

fn decode(path: &Path) -> Result<(Decoded, u32, u32), String> {
    let bytes: Arc<[u8]> = std::fs::read(path).map_err(|e| e.to_string())?.into();
    // Sidestep reads the header: whether it's a format the desktop draws,
    // of a size its codecs decode, and that size upright.
    let image = Image::decode(bytes.clone()).ok_or("Unsupported or damaged image (SVG isn't supported).")?;
    let (w, h) = image.pixel_size();
    if w == 0 || h == 0 {
        return Err("The wallpaper has no pixels.".into());
    }
    let undecoded = "The wallpaper's image data couldn't be decoded.";
    // A photo is megapixels, and most wallpapers are JPEG photos (all of
    // Sekka's are). `to_rgba8` would hold a photo's RGB pixels and their
    // RGBA copy at once, seven bytes a pixel, to scale them down to an
    // output: here they stay as the decoder Sidestep uses leaves them,
    // and only the scaled picture gets its alpha.
    if image::guess_format(&bytes).ok() == Some(ImageFormat::Jpeg) {
        let mut decoder = JpegDecoder::new(std::io::Cursor::new(&bytes[..])).map_err(|_| undecoded)?;
        if decoder.color_type() == ColorType::Rgb8 {
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            let mut decoded = DynamicImage::from_decoder(decoder).map_err(|_| undecoded)?;
            decoded.apply_orientation(orientation);
            return match decoded {
                DynamicImage::ImageRgb8(rgb) if rgb.dimensions() == (w, h) => Ok((Decoded::Rgb(rgb.into_raw()), w, h)),
                _ => Err(undecoded.into()),
            };
        }
    }
    let rgba = image.to_rgba8().ok_or(undecoded)?;
    Ok((Decoded::Rgba(rgba), w, h))
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
    cover_pixels::<4>(src, sw, sh, dw, dh)
}

/// [`cover`] for a source of `CHANNELS` bytes a pixel: RGBA's four, or
/// RGB's three, which come out opaque RGBA, as their RGBA would.
fn cover_pixels<const CHANNELS: usize>(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let k = (dw as f64 / sw as f64).max(dh as f64 / sh as f64);
    let (cw, ch) = (dw as f64 / k, dh as f64 / k);
    let (x0, y0) = ((sw as f64 - cw) / 2.0, (sh as f64 - ch) / 2.0);
    let cols = taps(dw, x0, cw, sw);
    let rows = taps(dh, y0, ch, sh);
    let stride = sw as usize * CHANNELS;
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
            let mut px = [0f32; CHANNELS];
            for (j, w) in c.weights.iter().enumerate() {
                let at = (c.first + j) * CHANNELS;
                for (ch, p) in px.iter_mut().enumerate() {
                    *p += line[at + ch] * w;
                }
            }
            for (ch, p) in px.iter().enumerate() {
                dst[x * 4 + ch] = p.round().clamp(0.0, 255.0) as u8;
            }
            if CHANNELS == 3 {
                dst[x * 4 + 3] = 255;
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
    fn rgb_scales_to_the_picture_its_opaque_rgba_does() {
        let rgb: Vec<u8> = (0..7 * 5 * 3).map(|i| (i * 37) as u8).collect();
        let rgba: Vec<u8> = rgb.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        // As it is, shrunk and cropped either way, grown, and down to a pixel.
        for (w, h) in [(7, 5), (3, 2), (2, 7), (11, 9), (1, 1)] {
            assert_eq!(cover_pixels::<3>(&rgb, 7, 5, w, h), cover(&rgba, 7, 5, w, h), "{w} × {h}");
        }
    }

    /// A 7 × 5 JPEG file of `color` samples, to be turned upright as EXIF
    /// orientation `orientation` (1 to 8) says.
    fn jpeg(color: image::ExtendedColorType, orientation: u8) -> PathBuf {
        use image::ImageEncoder;
        let samples: Vec<u8> = (0..7 * 5 * 3).map(|i| (i * 37) as u8).collect();
        // A little-endian TIFF header and one entry, the orientation.
        let mut exif =
            b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x12\x01\x03\x00\x01\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00"
                .to_vec();
        exif[18] = orientation;
        let mut file = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut file);
        encoder.set_exif_metadata(exif).unwrap();
        encoder.write_image(&samples[..7 * 5 * usize::from(color.channel_count())], 7, 5, color).unwrap();
        let path = std::env::temp_dir().join(format!("aizome-wallpaper-{}-{color:?}.jpg", std::process::id()));
        std::fs::write(&path, file).unwrap();
        path
    }

    #[test]
    fn a_color_jpeg_stays_rgb_and_makes_the_picture_its_rgba_would() {
        for orientation in 1..=8 {
            let path = jpeg(image::ExtendedColorType::Rgb8, orientation);
            let image = Image::open(&path).unwrap();
            let (w, h) = image.pixel_size();
            assert_eq!((w, h), if orientation < 5 { (7, 5) } else { (5, 7) }, "orientation {orientation}");
            let rgba = image.to_rgba8().unwrap();
            // Three bytes a pixel, upright: the pixels the desktop's
            // decoder gives, less their alpha.
            let Ok((Decoded::Rgb(rgb), ..)) = decode(&path) else { panic!("orientation {orientation} isn't RGB") };
            let opaque: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            assert_eq!(rgb, opaque, "orientation {orientation}");
            for (dw, dh) in [(3, 2), (9, 11)] {
                assert_eq!(prepare(&path, dw, dh), Some(cover(&rgba, w, h, dw, dh)), "orientation {orientation}");
            }
            std::fs::remove_file(path).unwrap();
        }
        // A gray one has no RGB to keep: it goes as any other file does.
        let path = jpeg(image::ExtendedColorType::L8, 1);
        assert!(matches!(decode(&path), Ok((Decoded::Rgba(_), 7, 5))));
        let rgba = Image::open(&path).unwrap().to_rgba8().unwrap();
        assert_eq!(prepare(&path, 3, 2), Some(cover(&rgba, 7, 5, 3, 2)));
        std::fs::remove_file(path).unwrap();
    }

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
