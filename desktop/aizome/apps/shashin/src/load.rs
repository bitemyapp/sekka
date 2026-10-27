//! Reading pictures, off the interface thread: what kind of file, its
//! size, a quick preview (a JPEG's EXIF thumbnail), the pixels upright,
//! animation frames, SVGs rendered at any zoom, and copies scaled and
//! turned for drawing one to one.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use aizome_kit::Image;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use resvg::{tiny_skia, usvg};

/// The kinds of picture Shashin reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Tiff,
    Ico,
    Svg,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Png => "PNG",
            Kind::Jpeg => "JPEG",
            Kind::Gif => "GIF",
            Kind::WebP => "WebP",
            Kind::Bmp => "BMP",
            Kind::Tiff => "TIFF",
            Kind::Ico => "ICO",
            Kind::Svg => "SVG",
        }
    }

    fn of_format(f: ImageFormat) -> Option<Kind> {
        Some(match f {
            ImageFormat::Png => Kind::Png,
            ImageFormat::Jpeg => Kind::Jpeg,
            ImageFormat::Gif => Kind::Gif,
            ImageFormat::WebP => Kind::WebP,
            ImageFormat::Bmp => Kind::Bmp,
            ImageFormat::Tiff => Kind::Tiff,
            ImageFormat::Ico => Kind::Ico,
            _ => return None,
        })
    }

    pub fn format(self) -> Option<ImageFormat> {
        Some(match self {
            Kind::Png => ImageFormat::Png,
            Kind::Jpeg => ImageFormat::Jpeg,
            Kind::Gif => ImageFormat::Gif,
            Kind::WebP => ImageFormat::WebP,
            Kind::Bmp => ImageFormat::Bmp,
            Kind::Tiff => ImageFormat::Tiff,
            Kind::Ico => ImageFormat::Ico,
            Kind::Svg => return None,
        })
    }
}

/// A picture's file, read.
#[derive(Clone)]
pub struct Source {
    pub path: PathBuf,
    pub bytes: Arc<[u8]>,
    pub kind: Kind,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Whether `bytes` look like an SVG (or `path` is named as one).
fn is_svg(bytes: &[u8], path: &Path) -> bool {
    let named = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    if named && bytes.starts_with(&[0x1f, 0x8b]) {
        return true;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.contains("<svg") && (head.starts_with('<'))
}

/// Read `path` and tell what it is.
pub fn read(path: &Path) -> Result<Source, String> {
    let meta = std::fs::metadata(path).map_err(|e| describe(&e))?;
    if meta.is_dir() {
        return Err("It's a folder.".into());
    }
    let bytes: Arc<[u8]> = std::fs::read(path).map_err(|e| describe(&e))?.into();
    let kind = if is_svg(&bytes, path) {
        Kind::Svg
    } else {
        image::guess_format(&bytes).ok().and_then(Kind::of_format).ok_or("It isn't a picture Shashin can read.")?
    };
    Ok(Source { path: path.to_path_buf(), bytes, kind, modified: meta.modified().ok(), created: meta.created().ok() })
}

/// A file error in a sentence.
pub fn describe(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "It isn't there any more.".into(),
        std::io::ErrorKind::PermissionDenied => "You don't have permission to read it.".into(),
        _ => format!("{e}."),
    }
}

fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, String> {
    ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())
}

/// A raster picture's size upright, and its EXIF orientation.
pub fn header(src: &Source) -> Result<((u32, u32), u8), String> {
    let mut decoder = reader(&src.bytes)?.into_decoder().map_err(|e| e.to_string())?;
    let (w, h) = decoder.dimensions();
    let o = decoder.orientation().map(|o| o.to_exif()).unwrap_or(1);
    Ok((if (5..=8).contains(&o) { (h, w) } else { (w, h) }, o))
}

/// The picture's pixels, upright.
pub fn decode(src: &Source) -> Result<RgbaImage, String> {
    decode_opaque(src).map(|(img, _)| img)
}

/// The picture's pixels, upright, and whether its kind has no alpha (so
/// every pixel is opaque).
pub fn decode_opaque(src: &Source) -> Result<(RgbaImage, bool), String> {
    let mut decoder = reader(&src.bytes)?.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let opaque = !img.color().has_alpha();
    Ok((img.into_rgba8(), opaque))
}

/// The picture's EXIF, if it has some.
pub fn exif(src: &Source) -> Option<exif::Exif> {
    if !matches!(src.kind, Kind::Jpeg | Kind::Tiff | Kind::WebP | Kind::Png) {
        return None;
    }
    exif::Reader::new().read_from_container(&mut Cursor::new(&src.bytes[..])).ok()
}

/// The small JPEG a camera keeps in the EXIF, upright: a preview to show
/// while the picture decodes.
pub fn exif_thumbnail(exif: &exif::Exif, orientation: u8) -> Option<RgbaImage> {
    let field = |t| exif.get_field(t, exif::In::THUMBNAIL).and_then(|f| f.value.get_uint(0));
    let at = field(exif::Tag::JPEGInterchangeFormat)? as usize;
    let len = field(exif::Tag::JPEGInterchangeFormatLength)? as usize;
    let bytes = exif.buf().get(at..at.checked_add(len)?)?;
    let mut img = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg).ok()?;
    img.apply_orientation(image::metadata::Orientation::from_exif(orientation)?);
    Some(img.into_rgba8())
}

/// What Get Info shows of a picture's EXIF: label and value.
pub fn exif_details(exif: &exif::Exif) -> Vec<(String, String)> {
    use exif::{In, Tag};
    let get = |t| exif.get_field(t, In::PRIMARY);
    let text =
        |t| get(t).map(|f| f.display_value().to_string().trim_matches('"').trim().to_owned()).filter(|s| !s.is_empty());
    let mut out = Vec::new();
    let camera = match (text(Tag::Make), text(Tag::Model)) {
        (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => Some(model),
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (a, b) => a.or(b),
    };
    if let Some(c) = camera {
        out.push(("Camera".into(), c));
    }
    if let Some(l) = text(Tag::LensModel) {
        out.push(("Lens".into(), l));
    }
    let mut settings = Vec::new();
    if let Some(f) = get(Tag::FocalLength) {
        settings.push(format!("{} mm", f.display_value()));
    }
    if let Some(f) = get(Tag::FNumber) {
        settings.push(format!("ƒ/{}", f.display_value()));
    }
    if let Some(f) = get(Tag::ExposureTime) {
        settings.push(format!("{} s", f.display_value()));
    }
    if let Some(f) = get(Tag::PhotographicSensitivity) {
        settings.push(format!("ISO {}", f.display_value()));
    }
    if !settings.is_empty() {
        out.push(("Settings".into(), settings.join("  ·  ")));
    }
    if let Some(t) = text(Tag::DateTimeOriginal) {
        out.push(("Taken".into(), t));
    }
    if let Some(s) = text(Tag::Software) {
        out.push(("Software".into(), s));
    }
    out
}

/// Frames of an animated GIF, WebP or PNG, each with how long it shows, if
/// it has more than one and they're few enough to keep (256 MiB).
pub fn frames(src: &Source) -> Option<Vec<(RgbaImage, Duration)>> {
    let bytes = Cursor::new(&src.bytes[..]);
    let frames = match src.kind {
        Kind::Gif => image::codecs::gif::GifDecoder::new(bytes).ok()?.into_frames(),
        Kind::WebP => {
            let d = image::codecs::webp::WebPDecoder::new(bytes).ok()?;
            if !d.has_animation() {
                return None;
            }
            d.into_frames()
        }
        Kind::Png => {
            let d = image::codecs::png::PngDecoder::new(bytes).ok()?;
            if !d.is_apng().ok()? {
                return None;
            }
            d.apng().ok()?.into_frames()
        }
        _ => return None,
    };
    let mut out = Vec::new();
    let mut total = 0usize;
    for f in frames {
        let f = f.ok()?;
        let (n, d) = f.delay().numer_denom_ms();
        let ms = n.checked_div(d).unwrap_or(100);
        // Browsers' rule: delays under 20 ms play as 100 ms.
        let ms = if ms < 20 { 100 } else { ms };
        let buf = f.into_buffer();
        total += buf.len();
        if total > 256 << 20 {
            return None;
        }
        out.push((buf, Duration::from_millis(u64::from(ms))));
    }
    (out.len() > 1).then_some(out)
}

/// Fonts for text in SVGs: the system's, loaded once, on first need.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        db.set_sans_serif_family("Archivo");
        Arc::new(db)
    })
    .clone()
}

/// An SVG's drawing.
pub fn parse_svg(src: &Source) -> Result<Arc<usvg::Tree>, String> {
    let mut opt = usvg::Options { resources_dir: src.path.parent().map(Path::to_path_buf), ..usvg::Options::default() };
    // Fonts only when there's text to set.
    if memfind(&src.bytes, b"<text") || src.bytes.starts_with(&[0x1f, 0x8b]) {
        opt.fontdb = fonts();
    }
    usvg::Tree::from_data(&src.bytes, &opt).map(Arc::new).map_err(|e| format!("The SVG doesn't read: {e}."))
}

fn memfind(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// An SVG's size in pixels (its user units).
pub fn svg_size(tree: &usvg::Tree) -> (u32, u32) {
    let s = tree.size();
    ((s.width().ceil() as u32).max(1), (s.height().ceil() as u32).max(1))
}

/// Part of an SVG drawn at zoom `z`: `region` is `(x, y, w, h)` of the
/// drawing scaled by `z`, in its device pixels.
pub fn render_svg(tree: &usvg::Tree, z: f64, region: (u32, u32, u32, u32)) -> Option<RgbaImage> {
    let (x, y, w, h) = region;
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let t = tiny_skia::Transform::from_row(z as f32, 0.0, 0.0, z as f32, -(x as f32), -(y as f32));
    resvg::render(tree, t, &mut pixmap.as_mut());
    // Straight alpha, as `Image::from_rgba` takes it.
    let mut px = pixmap.take();
    for p in px.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(w.max(1), h.max(1), px)
}

/// `img` at `w` × `h`: averaged down, or smoothly up.
pub fn scale_to(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    if (w, h) == img.dimensions() {
        img.clone()
    } else if w <= img.width() && h <= img.height() {
        image::imageops::thumbnail(img, w, h)
    } else {
        image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle)
    }
}

/// `img` turned `turns` quarter turns clockwise.
pub fn turn(img: RgbaImage, turns: u8) -> RgbaImage {
    match turns % 4 {
        1 => image::imageops::rotate90(&img),
        2 => image::imageops::rotate180(&img),
        3 => image::imageops::rotate270(&img),
        _ => img,
    }
}

/// The picture `base` turned `turns` times and scaled by `z`: the part
/// `(x, y, w, h)` of it, in its device pixels, scaled smoothly.
pub fn region(base: &RgbaImage, turns: u8, z: f64, rect: (u32, u32, u32, u32)) -> RgbaImage {
    let (bw, bh) = (f64::from(base.width()), f64::from(base.height()));
    let (x, y, w, h) = (f64::from(rect.0), f64::from(rect.1), f64::from(rect.2), f64::from(rect.3));
    // The region in the picture's pixels before turning and scaling.
    let r = crate::view::unturn_rect((x / z, y / z, w / z, h / z), (bw, bh), turns);
    let x0 = r.0.floor().clamp(0.0, bw - 1.0) as u32;
    let y0 = r.1.floor().clamp(0.0, bh - 1.0) as u32;
    let x1 = (r.0 + r.2).ceil().clamp(f64::from(x0) + 1.0, bw) as u32;
    let y1 = (r.1 + r.3).ceil().clamp(f64::from(y0) + 1.0, bh) as u32;
    let crop = image::imageops::crop_imm(base, x0, y0, x1 - x0, y1 - y0).to_image();
    let (tw, th) = if turns % 2 == 1 { (rect.3, rect.2) } else { (rect.2, rect.3) };
    let scaled = image::imageops::resize(&crop, tw.max(1), th.max(1), image::imageops::FilterType::CatmullRom);
    turn(scaled, turns)
}

/// Pixels to draw.
pub fn to_image(img: &RgbaImage) -> Option<Image> {
    Image::from_rgba(img.width(), img.height(), img.as_raw())
}

/// How many pixels a picture has before it's too big to keep a second,
/// premultiplied copy of for drawing at 100% and over (the copy is made
/// on demand).
pub const FULL_COPY_LIMIT: u64 = 120_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_come_from_the_right_place() {
        // Left half red, right half blue, 4 × 2.
        let base =
            RgbaImage::from_fn(
                4,
                2,
                |x, _| if x < 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) },
            );
        // Unturned at 2×: the region at x 0..4 is red.
        let r = region(&base, 0, 2.0, (0, 0, 4, 4));
        assert_eq!(r.get_pixel(1, 1).0, [255, 0, 0, 255]);
        // Turned once (2 × 4, red on top), at 1×: the top half is red.
        let r = region(&base, 1, 1.0, (0, 0, 2, 2));
        assert_eq!(r.dimensions(), (2, 2));
        assert_eq!(r.get_pixel(0, 0).0, [255, 0, 0, 255]);
        let r = region(&base, 1, 1.0, (0, 2, 2, 2));
        assert_eq!(r.get_pixel(1, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn tells_svg_from_xml() {
        assert!(is_svg(b"<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>", Path::new("a")));
        assert!(!is_svg(b"\x89PNG", Path::new("a.svg")));
    }
}
