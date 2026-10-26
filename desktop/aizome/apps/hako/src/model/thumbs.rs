//! Thumbnails by the freedesktop.org Thumbnail Managing Standard (0.9),
//! shared with every other program that keeps them:
//!
//! - in `$XDG_CACHE_HOME/thumbnails/normal` (at most 128 pixels square)
//!   and `large` (256), each named by the MD5 of the file's URI, as a PNG
//!   whose text chunks say whose thumbnail it is (`Thumb::URI`) and of
//!   which version (`Thumb::MTime`, the file's modification time, and
//!   `Thumb::Size`); a thumbnail whose file has changed since is made
//!   again;
//! - written beside and renamed into place, readable only by the person
//!   (the folders `0700`, the files `0600`);
//! - a file that can't be read gets a mark in `fail/aizome-hako`, so it
//!   isn't tried again until it changes.
//!
//! Thumbnails aren't made of files in the thumbnail folders themselves,
//! nor of very large images.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::uri::file_uri;

/// The sizes, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    Normal,
    Large,
}

impl ThumbSize {
    pub fn pixels(self) -> u32 {
        match self {
            ThumbSize::Normal => 128,
            ThumbSize::Large => 256,
        }
    }

    fn dir(self) -> &'static str {
        match self {
            ThumbSize::Normal => "normal",
            ThumbSize::Large => "large",
        }
    }
}

/// Images larger than this many bytes aren't thumbnailed.
pub const MAX_FILE: u64 = 64 << 20;
/// Nor of more pixels than this.
pub const MAX_PIXELS: u64 = 80_000_000;

const PROGRAM: &str = "aizome-hako";

/// A thumbnail cache.
#[derive(Clone, Debug)]
pub struct Thumbnails {
    pub root: PathBuf,
}

impl Thumbnails {
    /// The person's: `$XDG_CACHE_HOME/thumbnails`.
    pub fn from_env() -> Thumbnails {
        Thumbnails { root: crate::xdg_dir("XDG_CACHE_HOME", ".cache").join("thumbnails") }
    }

    fn name(uri: &str) -> String {
        format!("{:x}.png", md5::compute(uri.as_bytes()))
    }

    /// Where `uri`'s thumbnail of `size` is (or would be).
    pub fn path(&self, uri: &str, size: ThumbSize) -> PathBuf {
        self.root.join(size.dir()).join(Self::name(uri))
    }

    fn fail_path(&self, uri: &str) -> PathBuf {
        self.root.join("fail").join(PROGRAM).join(Self::name(uri))
    }

    /// Whether a file is one of the cache's own.
    pub fn is_cached_file(&self, file: &Path) -> bool {
        file.starts_with(&self.root)
    }

    /// A thumbnail of `file` (modified at `mtime`) that's up to date, if
    /// there is one.
    pub fn lookup(&self, file: &Path, mtime: i64, size: ThumbSize) -> Option<PathBuf> {
        let uri = file_uri(file);
        let p = self.path(&uri, size);
        is_valid(&p, &uri, mtime).then_some(p)
    }

    /// Whether making `file`'s thumbnail failed since it last changed.
    pub fn failed(&self, file: &Path, mtime: i64) -> bool {
        let uri = file_uri(file);
        is_valid(&self.fail_path(&uri), &uri, mtime)
    }

    /// Keep a thumbnail made from straight RGBA pixels (scaled down to
    /// fit `size`).
    #[allow(clippy::too_many_arguments)]
    pub fn save(
        &self,
        file: &Path,
        mtime: i64,
        file_size: u64,
        mime: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
        size: ThumbSize,
    ) -> io::Result<PathBuf> {
        let uri = file_uri(file);
        let (px, w, h) = scale_to_fit(rgba, width, height, size.pixels());
        let p = self.path(&uri, size);
        let mut text = vec![
            ("Thumb::URI", uri.clone()),
            ("Thumb::MTime", mtime.to_string()),
            ("Thumb::Size", file_size.to_string()),
            ("Thumb::Image::Width", width.to_string()),
            ("Thumb::Image::Height", height.to_string()),
            ("Software", "Aizome Hako".to_owned()),
        ];
        if !mime.is_empty() {
            text.push(("Thumb::Mime", mime.to_owned()));
        }
        write_png(&p, &px, w, h, &text)?;
        Ok(p)
    }

    /// Mark `file` as failing (until it changes).
    pub fn save_failure(&self, file: &Path, mtime: i64) -> io::Result<()> {
        let uri = file_uri(file);
        write_png(
            &self.fail_path(&uri),
            &[0, 0, 0, 0],
            1,
            1,
            &[("Thumb::URI", uri.clone()), ("Thumb::MTime", mtime.to_string()), ("Software", "Aizome Hako".into())],
        )
    }
}

/// Whether the PNG at `p` is `uri`'s at `mtime`.
fn is_valid(p: &Path, uri: &str, mtime: i64) -> bool {
    let text = read_text(p);
    text.get("Thumb::URI").is_some_and(|u| u == uri)
        && text.get("Thumb::MTime").and_then(|m| m.parse::<i64>().ok()) == Some(mtime)
}

/// A PNG's text chunks (those before its pixels).
pub fn read_text(p: &Path) -> HashMap<String, String> {
    let Ok(f) = fs::File::open(p) else { return HashMap::new() };
    let Ok(reader) = png::Decoder::new(BufReader::new(f)).read_info() else { return HashMap::new() };
    let info = reader.info();
    let mut out: HashMap<String, String> =
        info.uncompressed_latin1_text.iter().map(|c| (c.keyword.clone(), c.text.clone())).collect();
    for c in &info.utf8_text {
        if let Ok(t) = c.get_text() {
            out.insert(c.keyword.clone(), t);
        }
    }
    out
}

/// Write a PNG beside `p`, private, and rename it into place.
fn write_png(p: &Path, rgba: &[u8], w: u32, h: u32, text: &[(&str, String)]) -> io::Result<()> {
    let dir = p.parent().unwrap_or(Path::new("/"));
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}",
        p.file_name().unwrap_or_default().to_string_lossy(),
        crate::fsops::sys::random_word()
    ));
    let result = (|| -> io::Result<()> {
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            for (k, v) in text {
                enc.add_text_chunk((*k).to_owned(), v.clone()).map_err(io::Error::other)?;
            }
            let mut writer = enc.write_header().map_err(io::Error::other)?;
            writer.write_image_data(rgba).map_err(io::Error::other)?;
            writer.finish().map_err(io::Error::other)?;
        }
        let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(&bytes)?;
        Ok(())
    })();
    match result {
        Ok(()) => fs::rename(&tmp, p),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Straight RGBA pixels scaled down (by averaging boxes of them, alpha
/// weighted) to fit `max` square; never up.
pub fn scale_to_fit(rgba: &[u8], w: u32, h: u32, max: u32) -> (Vec<u8>, u32, u32) {
    if w <= max && h <= max {
        return (rgba.to_vec(), w, h);
    }
    let k = (max as f64 / w as f64).min(max as f64 / h as f64);
    let (nw, nh) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        let (y0, y1) =
            ((y as u64 * h as u64 / nh as u64) as u32, (((y + 1) as u64 * h as u64).div_ceil(nh as u64)) as u32);
        for x in 0..nw {
            let (x0, x1) =
                ((x as u64 * w as u64 / nw as u64) as u32, (((x + 1) as u64 * w as u64).div_ceil(nw as u64)) as u32);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1.min(h) {
                let row = (sy * w) as usize * 4;
                for sx in x0..x1.min(w) {
                    let i = row + sx as usize * 4;
                    let al = rgba[i + 3] as u64;
                    r += rgba[i] as u64 * al;
                    g += rgba[i + 1] as u64 * al;
                    b += rgba[i + 2] as u64 * al;
                    a += al;
                    n += 1;
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            if let (Some(r), Some(g), Some(b)) = (r.checked_div(a), g.checked_div(a), b.checked_div(a)) {
                out[o] = r as u8;
                out[o + 1] = g as u8;
                out[o + 2] = b as u8;
            }
            out[o + 3] = (a / n.max(1)) as u8;
        }
    }
    (out, nw, nh)
}

/// Whether Hako makes thumbnails of files of `mime`.
pub fn can_thumbnail(mime: &str) -> bool {
    matches!(
        mime,
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/tiff"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
            | "image/apng"
    )
}

/// Make (or find) `file`'s thumbnails, decoding it with Sidestep's
/// codecs: the large one's path, or `None` if it can't be made.
#[cfg(feature = "gui")]
pub fn make(cache: &Thumbnails, file: &Path, mime: &str) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(file).ok()?;
    let mtime = meta.mtime();
    if let Some(p) = cache.lookup(file, mtime, ThumbSize::Large) {
        return Some(p);
    }
    if cache.is_cached_file(file) || meta.len() > MAX_FILE || cache.failed(file, mtime) {
        return None;
    }
    let decoded = aizome_kit::Image::open(file).ok().and_then(|img| {
        let (w, h) = img.pixel_size();
        if u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        img.to_rgba8().map(|px| (px, w, h))
    });
    let Some((px, w, h)) = decoded else {
        let _ = cache.save_failure(file, mtime);
        return None;
    };
    let large = cache.save(file, mtime, meta.len(), mime, &px, w, h, ThumbSize::Large).ok();
    let _ = cache.save(file, mtime, meta.len(), mime, &px, w, h, ThumbSize::Normal);
    large
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::TempDir;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn names_by_the_md5_of_the_uri() {
        let c = Thumbnails { root: PathBuf::from("/c/thumbnails") };
        // The spec's own example.
        let p = c.path("file:///home/jens/photos/me.png", ThumbSize::Normal);
        assert_eq!(p, PathBuf::from("/c/thumbnails/normal/c6ee772d9e49320e97ec29a7eb5b1697.png"));
    }

    #[test]
    fn saves_finds_and_invalidates() {
        let t = TempDir::new("thumbs");
        let c = Thumbnails { root: t.join("cache/thumbnails") };
        let file = t.file("pic.png", "not really a png");
        let mtime = fs::metadata(&file).unwrap().mtime();
        // A 300 × 150 gradient.
        let (w, h) = (300u32, 150u32);
        let px: Vec<u8> = (0..w * h).flat_map(|i| [(i % w) as u8, (i / w) as u8, 128, 255]).collect();
        let saved = c.save(&file, mtime, 16, "image/png", &px, w, h, ThumbSize::Large).unwrap();
        assert_eq!(c.lookup(&file, mtime, ThumbSize::Large), Some(saved.clone()));
        assert_eq!(c.lookup(&file, mtime + 1, ThumbSize::Large), None, "a changed file's thumbnail is stale");
        assert_eq!(c.lookup(&file, mtime, ThumbSize::Normal), None);
        let text = read_text(&saved);
        assert_eq!(text.get("Thumb::Size").map(String::as_str), Some("16"));
        assert_eq!(fs::metadata(&saved).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(saved.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        let img = png::Decoder::new(BufReader::new(fs::File::open(&saved).unwrap())).read_info().unwrap();
        assert_eq!((img.info().width, img.info().height), (256, 128));
        assert!(!c.failed(&file, mtime));
        c.save_failure(&file, mtime).unwrap();
        assert!(c.failed(&file, mtime));
        assert!(!c.failed(&file, mtime + 5));
    }

    #[test]
    fn scales_by_averaging() {
        // 4 × 2: left half black, right half white; to 2 × 1.
        let px: Vec<u8> = (0..8).flat_map(|i| if i % 4 < 2 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        let (out, w, h) = scale_to_fit(&px, 4, 2, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, vec![0, 0, 0, 255, 255, 255, 255, 255]);
        let (same, w, h) = scale_to_fit(&px, 4, 2, 128);
        assert_eq!((w, h, same.len()), (4, 2, 32), "never up");
        // Transparent pixels don't darken their neighbours.
        let px = vec![255, 0, 0, 255, 0, 0, 0, 0];
        let (out, _, _) = scale_to_fit(&px, 2, 1, 1);
        assert_eq!(&out[..3], &[255, 0, 0]);
        assert_eq!(out[3], 127);
    }
}
