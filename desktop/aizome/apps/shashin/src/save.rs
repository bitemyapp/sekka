//! Saving a turned picture: a JPEG by its EXIF orientation alone, so not a
//! pixel changes; anything else written again in its own format.

use std::io::Cursor;
use std::path::Path;

use image::AnimationDecoder;

use crate::load::{self, Kind};
use crate::orient;

/// How a picture was saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    /// Only the orientation tag changed.
    Lossless,
    /// The pixels were turned and written again.
    Rewritten,
}

/// Turn the picture at `path` `turns` quarter turns clockwise, in its file.
pub fn save_turned(path: &Path, turns: u8) -> Result<Saved, String> {
    let turns = turns % 4;
    if turns == 0 {
        return Ok(Saved::Lossless);
    }
    let src = load::read(path)?;
    match src.kind {
        Kind::Jpeg => {
            let now = orient::jpeg_orientation(&src.bytes);
            match orient::set_jpeg_orientation(&src.bytes, orient::compose(now, turns)) {
                Ok(bytes) => {
                    write_atomic(path, &bytes)?;
                    Ok(Saved::Lossless)
                }
                Err(e) => {
                    log::info!("{}: writing the orientation: {e:?}; encoding again", path.display());
                    rewrite(&src, turns)
                }
            }
        }
        Kind::Svg => Err("Shashin can't save a turned SVG yet.".into()),
        Kind::Gif if load::frames(&src).is_some() => rewrite_gif(&src, turns),
        Kind::WebP | Kind::Png if load::frames(&src).is_some() => {
            Err("Shashin can't write animated WebP or PNG pictures.".into())
        }
        _ => rewrite(&src, turns),
    }
}

/// Decode, turn and encode again in the same format.
fn rewrite(src: &load::Source, turns: u8) -> Result<Saved, String> {
    let img = load::turn(load::decode(src)?, turns);
    let format = src.kind.format().ok_or("Shashin can't write this kind of picture.")?;
    let mut out = Cursor::new(Vec::new());
    match src.kind {
        Kind::Jpeg => {
            let rgb = image::DynamicImage::ImageRgba8(img).into_rgb8();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
            rgb.write_with_encoder(enc).map_err(|e| e.to_string())?;
        }
        Kind::Ico if img.width() > 256 || img.height() > 256 => {
            return Err("An icon can't be larger than 256 pixels.".into());
        }
        Kind::Bmp | Kind::Gif => {
            // Neither keeps an alpha channel's shades: write what shows.
            image::DynamicImage::ImageRgba8(img).write_to(&mut out, format).map_err(|e| e.to_string())?;
        }
        _ => img.write_to(&mut out, format).map_err(|e| e.to_string())?,
    }
    write_atomic(&src.path, &out.into_inner())?;
    Ok(Saved::Rewritten)
}

/// An animated GIF with every frame turned.
fn rewrite_gif(src: &load::Source, turns: u8) -> Result<Saved, String> {
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&src.bytes[..])).map_err(|e| e.to_string())?;
    let frames = decoder.into_frames().collect_frames().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).map_err(|e| e.to_string())?;
        let turned = frames.into_iter().map(|f| {
            let delay = f.delay();
            image::Frame::from_parts(load::turn(f.into_buffer(), turns), 0, 0, delay)
        });
        enc.encode_frames(turned).map_err(|e| e.to_string())?;
    }
    write_atomic(&src.path, &out)?;
    Ok(Saved::Rewritten)
}

/// Write `bytes` over `path` through a temporary file beside it, keeping
/// its permissions.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.shashin-{}", std::process::id()));
    let fail = |e: std::io::Error| load::describe(&e);
    std::fs::write(&tmp, bytes).map_err(fail)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        load::describe(&e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("shashin-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn jpegs_turn_by_their_tag_and_pngs_by_their_pixels() {
        let d = dir("turn");
        let img = image::RgbImage::from_fn(
            6,
            4,
            |x, _| if x < 3 { image::Rgb([250, 0, 0]) } else { image::Rgb([0, 0, 250]) },
        );
        let jpeg = d.join("a.jpg");
        img.save(&jpeg).unwrap();
        let scan = std::fs::read(&jpeg).unwrap();
        assert_eq!(save_turned(&jpeg, 1).unwrap(), Saved::Lossless);
        let after = std::fs::read(&jpeg).unwrap();
        assert!(after.ends_with(&scan[scan.len() - 64..]));
        let shown = load::decode(&load::read(&jpeg).unwrap()).unwrap();
        assert_eq!(shown.dimensions(), (4, 6));
        assert_eq!(save_turned(&jpeg, 3).unwrap(), Saved::Lossless);
        assert_eq!(orient::jpeg_orientation(&std::fs::read(&jpeg).unwrap()), 1);

        let png = d.join("b.png");
        img.save(&png).unwrap();
        assert_eq!(save_turned(&png, 1).unwrap(), Saved::Rewritten);
        let shown = load::decode(&load::read(&png).unwrap()).unwrap();
        assert_eq!(shown.dimensions(), (4, 6));
        // Turned clockwise: the left (red) half is on top.
        assert_eq!(shown.get_pixel(1, 0).0[0], 250);
        let _ = std::fs::remove_dir_all(&d);
    }
}
