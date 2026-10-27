//! Thumbnails for the filmstrip, kept where every freedesktop program keeps
//! them (`$XDG_CACHE_HOME/thumbnails/normal`, 128 pixels, named by the MD5
//! of the file's URI and carrying its URI and modification time), so Hako's
//! and Shashin's are the same files.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use image::RgbaImage;

/// The size of a "normal" thumbnail's longer side.
pub const NORMAL: u32 = 128;

/// `$XDG_CACHE_HOME/thumbnails/normal`.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    base.join("thumbnails/normal")
}

/// `file:///…`, percent-encoded as the spec wants.
pub fn uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// The cache's file for `path`.
pub fn thumb_path(dir: &Path, path: &Path) -> PathBuf {
    dir.join(format!("{:x}.png", md5::compute(uri(path).as_bytes())))
}

fn mtime(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// The cached thumbnail of `path`, if there's one made since it last
/// changed.
pub fn cached(dir: &Path, path: &Path) -> Option<RgbaImage> {
    let file = std::fs::File::open(thumb_path(dir, path)).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().ok()?;
    let stamp = reader
        .info()
        .uncompressed_latin1_text
        .iter()
        .find(|t| t.keyword == "Thumb::MTime")
        .and_then(|t| t.text.trim().parse::<u64>().ok());
    if stamp != mtime(path) {
        return None;
    }
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    buf.truncate(frame.buffer_size());
    match frame.color_type {
        png::ColorType::Rgba => RgbaImage::from_raw(frame.width, frame.height, buf),
        png::ColorType::GrayscaleAlpha => {
            let px = buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect();
            RgbaImage::from_raw(frame.width, frame.height, px)
        }
        _ => None,
    }
}

/// Keep `thumb` as `path`'s thumbnail (written to a temporary file and
/// renamed, as the spec asks).
pub fn store(dir: &Path, path: &Path, thumb: &RgbaImage) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let target = thumb_path(dir, path);
    let tmp = target.with_extension(format!("shashin-{}.tmp", std::process::id()));
    {
        let file = std::fs::File::create(&tmp)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), thumb.width(), thumb.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let text = |k: &str, v: String| (k.to_owned(), v);
        let mut chunks = vec![text("Thumb::URI", uri(path)), text("Software", "Shashin".into())];
        if let Some(m) = mtime(path) {
            chunks.push(text("Thumb::MTime", m.to_string()));
        }
        if let Ok(meta) = std::fs::metadata(path) {
            chunks.push(text("Thumb::Size", meta.len().to_string()));
        }
        for (k, v) in chunks {
            enc.add_text_chunk(k, v).map_err(std::io::Error::other)?;
        }
        let mut w = enc.write_header().map_err(std::io::Error::other)?;
        w.write_image_data(thumb.as_raw()).map_err(std::io::Error::other)?;
        w.finish().map_err(std::io::Error::other)?;
    }
    std::fs::rename(&tmp, &target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// `image` made no larger than a normal thumbnail.
pub fn shrink(image: &RgbaImage) -> RgbaImage {
    let (w, h) = image.dimensions();
    if w <= NORMAL && h <= NORMAL {
        return image.clone();
    }
    let s = f64::from(NORMAL) / f64::from(w.max(h));
    let (tw, th) = (((f64::from(w) * s).round() as u32).max(1), ((f64::from(h) * s).round() as u32).max(1));
    image::imageops::thumbnail(image, tw, th)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_round_trips() {
        assert_eq!(uri(Path::new("/home/a/b c.png")), "file:///home/a/b%20c.png");
        let dir = std::env::temp_dir().join(format!("shashin-thumbs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pic = dir.join("p.png");
        std::fs::write(&pic, b"not really").unwrap();
        let thumb = RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]));
        store(&dir.join("cache"), &pic, &thumb).unwrap();
        let back = cached(&dir.join("cache"), &pic).expect("cached");
        assert_eq!(back, thumb);
        assert_eq!(shrink(&RgbaImage::new(512, 256)).dimensions(), (128, 64));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
