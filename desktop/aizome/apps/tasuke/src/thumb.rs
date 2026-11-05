//! Screenshots as thumbnails. A screenshot is the screen's size (3840 ×
//! 2160 at 2×); drawn smaller, the kit would scale it bicubic on one
//! thread, hundreds of milliseconds at that size. So a thumbnail is made
//! once, on a worker, at exactly the pixels it shows at on a 2× screen
//! (averaging each block of the picture's pixels), and the window copies
//! it one to one. On a 1× screen the kit halves that small picture once.

use std::path::Path;
use std::sync::Arc;

use aizome_kit::Image;

/// How wide a thumbnail shows, in points.
pub const WIDTH: u32 = 280;
/// Pixels per point it's made for.
const DENSITY: u32 = 2;

#[derive(Clone, Debug)]
pub enum Thumb {
    Loading,
    Ready(Image),
    /// The file is gone or isn't a picture: why, in words.
    Missing(String),
}

/// A thumbnail of the PNG at `path`.
pub fn make(path: &Path) -> Thumb {
    match read_png(path) {
        Ok((w, h, rgba)) => match shrink(w, h, &rgba, WIDTH * DENSITY) {
            Some((tw, th, px)) => match Image::from_premultiplied_rgba(tw, th, Arc::<[u8]>::from(px)) {
                Some(img) => Thumb::Ready(img.with_scale(1.0 / f64::from(DENSITY))),
                None => Thumb::Missing("The picture couldn't be shown.".into()),
            },
            None => Thumb::Missing("The picture is empty.".into()),
        },
        Err(e) => Thumb::Missing(e),
    }
}

/// The PNG's size and its pixels as straight RGBA.
fn read_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "The screenshot is no longer there.".to_owned(),
        _ => format!("The screenshot can't be read: {e}"),
    })?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("Not a picture Tasuke can read: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("The picture is too large.")?];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("The picture is damaged: {e}"))?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.as_chunks::<2>().0.iter().flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("The picture's colours weren't expanded.".into()),
    };
    Ok((w, h, rgba))
}

/// `rgba` (straight alpha) shrunk to `width` across, as premultiplied
/// RGBA: each pixel the average of the block it covers. Never enlarges.
pub fn shrink(w: u32, h: u32, rgba: &[u8], width: u32) -> Option<(u32, u32, Vec<u8>)> {
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return None;
    }
    let tw = width.min(w).max(1);
    let th = ((u64::from(h) * u64::from(tw) / u64::from(w)) as u32).max(1);
    let mut out = vec![0u8; (tw * th * 4) as usize];
    for ty in 0..th {
        let y0 = (u64::from(ty) * u64::from(h) / u64::from(th)) as u32;
        let y1 = ((u64::from(ty + 1) * u64::from(h) / u64::from(th)) as u32).max(y0 + 1);
        for tx in 0..tw {
            let x0 = (u64::from(tx) * u64::from(w) / u64::from(tw)) as u32;
            let x1 = ((u64::from(tx + 1) * u64::from(w) / u64::from(tw)) as u32).max(x0 + 1);
            let mut sum = [0u64; 4];
            for y in y0..y1 {
                let row = (y * w) as usize * 4;
                for x in x0..x1 {
                    let p = &rgba[row + x as usize * 4..row + x as usize * 4 + 4];
                    let a = u64::from(p[3]);
                    // Premultiplied as it's summed, so edges don't fringe.
                    sum[0] += u64::from(p[0]) * a;
                    sum[1] += u64::from(p[1]) * a;
                    sum[2] += u64::from(p[2]) * a;
                    sum[3] += a;
                }
            }
            let n = u64::from((y1 - y0) * (x1 - x0));
            let o = ((ty * tw + tx) * 4) as usize;
            out[o] = (sum[0] / (n * 255)) as u8;
            out[o + 1] = (sum[1] / (n * 255)) as u8;
            out[o + 2] = (sum[2] / (n * 255)) as u8;
            out[o + 3] = (sum[3] / n) as u8;
        }
    }
    Some((tw, th, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_shrinks_to_the_width_keeping_its_shape() {
        // 8×4, left half red, right half opaque blue.
        let mut px = Vec::new();
        for _y in 0..4 {
            for x in 0..8 {
                px.extend(if x < 4 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
            }
        }
        let (w, h, out) = shrink(8, 4, &px, 2).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(&out[..4], &[255, 0, 0, 255]);
        assert_eq!(&out[4..], &[0, 0, 255, 255]);
        // Never larger than it is.
        assert_eq!(shrink(8, 4, &px, 100).map(|(w, h, _)| (w, h)), Some((8, 4)));
    }

    #[test]
    fn transparency_is_premultiplied() {
        let px = [255, 255, 255, 0, 255, 255, 255, 255];
        let (_, _, out) = shrink(2, 1, &px, 1).unwrap();
        assert_eq!(out, [127, 127, 127, 127]);
    }

    #[test]
    fn a_missing_file_says_so() {
        let Thumb::Missing(why) = make(Path::new("/nonexistent/shot.png")) else { panic!() };
        assert_eq!(why, "The screenshot is no longer there.");
    }
}
