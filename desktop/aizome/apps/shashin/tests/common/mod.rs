//! Pictures to test (and try) Shashin with, made here rather than kept as
//! files: a landscape photo with a camera's EXIF (and its thumbnail), pixel
//! art with alpha, an animated GIF, an SVG, and one of every other kind.
//! And a rig that runs Shashin headlessly over them.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_kit::Appearance;
use aizome_kit::testing::Harness;
use aizome_kit::theme::ThemeChoice;
use aizome_shashin::{Msg, Shashin};

pub fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join(format!("shashin-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A landscape at dusk: sky in the dye ramp, a sun, two ridges, water.
pub fn landscape(w: u32, h: u32) -> image::RgbImage {
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|i| (a[i] + (b[i] - a[i]) * t.clamp(0.0, 1.0)) as u8);
    image::RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let sun = ((fx - 0.68).powi(2) + (fy - 0.42).powi(2) * 0.6).sqrt();
        let ridge1 = 0.58 + 0.08 * (fx * 7.0).sin() + 0.04 * (fx * 17.0).cos();
        let ridge2 = 0.66 + 0.05 * (fx * 4.0 + 1.0).sin();
        let px = if fy > 0.8 {
            lerp([36.0, 40.0, 59.0], [22.0, 22.0, 30.0], (fy - 0.8) * 5.0)
        } else if fy > ridge2 {
            [36, 40, 59]
        } else if fy > ridge1 {
            [61, 89, 161]
        } else if sun < 0.07 {
            [224, 175, 104]
        } else {
            lerp([169.0, 196.0, 255.0], [122.0, 162.0, 247.0], fy * 1.6)
        };
        // A little grain, as a photo has.
        let n = ((x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 7) as u8;
        image::Rgb([px[0].saturating_add(n), px[1].saturating_add(n), px[2].saturating_add(n)])
    })
}

/// A value of a TIFF entry.
enum V {
    Ascii(&'static str),
    Short(u16),
    Long(u32),
    Rational(u32, u32),
}

/// An IFD's bytes at `at` (with its values after it), pointing on to
/// `next`.
fn ifd(at: usize, entries: &[(u16, V)], next: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut data = Vec::new();
    let data_at = at + 2 + entries.len() * 12 + 4;
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for (tag, v) in entries {
        out.extend_from_slice(&tag.to_be_bytes());
        let (kind, count): (u16, u32) = match v {
            V::Ascii(s) => (2, s.len() as u32 + 1),
            V::Short(_) => (3, 1),
            V::Long(_) => (4, 1),
            V::Rational(..) => (5, 1),
        };
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        match v {
            V::Short(n) => out.extend_from_slice(&[(n >> 8) as u8, *n as u8, 0, 0]),
            V::Long(n) => out.extend_from_slice(&n.to_be_bytes()),
            V::Ascii(s) if s.len() < 4 => {
                let mut b = [0u8; 4];
                b[..s.len()].copy_from_slice(s.as_bytes());
                out.extend_from_slice(&b);
            }
            V::Ascii(s) => {
                out.extend_from_slice(&((data_at + data.len()) as u32).to_be_bytes());
                data.extend_from_slice(s.as_bytes());
                data.push(0);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
            }
            V::Rational(a, b) => {
                out.extend_from_slice(&((data_at + data.len()) as u32).to_be_bytes());
                data.extend_from_slice(&a.to_be_bytes());
                data.extend_from_slice(&b.to_be_bytes());
            }
        }
    }
    out.extend_from_slice(&next.to_be_bytes());
    out.extend_from_slice(&data);
    out
}

/// A camera's EXIF (a big-endian TIFF structure): make and model, the
/// settings in the EXIF sub-IFD, and, given one, a JPEG thumbnail in IFD1.
pub fn camera_exif(thumbnail: Option<&[u8]>) -> Vec<u8> {
    let exif_entries = |at| {
        ifd(
            at,
            &[
                (0x829a, V::Rational(1, 250)),
                (0x829d, V::Rational(2, 1)),
                (0x8827, V::Short(200)),
                (0x9003, V::Ascii("2026:10:17 06:12:00")),
                (0x920a, V::Rational(23, 1)),
            ],
            0,
        )
    };
    let ifd0 = |exif_at: u32, next: u32| {
        ifd(8, &[(0x010f, V::Ascii("Fujifilm")), (0x0110, V::Ascii("X100V")), (0x8769, V::Long(exif_at))], next)
    };
    // Sizes first (they don't depend on the offsets), then the real thing.
    let exif_at = 8 + ifd0(0, 0).len();
    let ifd1_at = exif_at + exif_entries(exif_at).len();
    let mut t = b"MM\0\x2a\0\0\0\x08".to_vec();
    t.extend(ifd0(exif_at as u32, if thumbnail.is_some() { ifd1_at as u32 } else { 0 }));
    t.extend(exif_entries(exif_at));
    if let Some(thumb) = thumbnail {
        let thumb_at = ifd1_at + 2 + 2 * 12 + 4;
        t.extend(ifd(ifd1_at, &[(0x0201, V::Long(thumb_at as u32)), (0x0202, V::Long(thumb.len() as u32))], 0));
        t.extend_from_slice(thumb);
    }
    t
}

pub fn jpeg(img: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    img.write_with_encoder(enc).unwrap();
    out.into_inner()
}

pub fn jpeg_with_exif(img: &image::RgbImage, exif: &[u8]) -> Vec<u8> {
    let plain = jpeg(img, 90);
    let mut out = plain[..2].to_vec();
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&((exif.len() + 8) as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(exif);
    out.extend_from_slice(&plain[2..]);
    out
}

/// A camera's JPEG: the picture, and its 160-pixel thumbnail in the EXIF.
pub fn camera_jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = landscape(w, h);
    let thumb = image::imageops::thumbnail(&img, 160, (160 * h / w).max(1));
    jpeg_with_exif(&img, &camera_exif(Some(&jpeg(&thumb, 80))))
}

/// Pixel art with alpha: a 16 × 16 sprite.
pub fn sprite() -> image::RgbaImage {
    const ART: [&str; 16] = [
        "................",
        "......####......",
        "....##aaaa##....",
        "...#aaaaaaaa#...",
        "..#aaaaaaaaaa#..",
        "..#aa##aa##aa#..",
        ".#aaa##aa##aaa#.",
        ".#aaaaaaaaaaaa#.",
        ".#aaaaaaaaaaaa#.",
        ".#aa#aaaaaa#aa#.",
        "..#aa######aa#..",
        "..#aaaaaaaaaa#..",
        "...#aaaaaaaa#...",
        "....##aaaa##....",
        "......####......",
        "................",
    ];
    image::RgbaImage::from_fn(16, 16, |x, y| match ART[y as usize].as_bytes()[x as usize] {
        b'#' => image::Rgba([36, 40, 59, 255]),
        b'a' => image::Rgba([224, 175, 104, 255]),
        _ => image::Rgba([0, 0, 0, 0]),
    })
}

pub fn gif(frames: usize) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
        let frames = (0..frames).map(|i| {
            let img = image::RgbaImage::from_fn(48, 32, |x, _| {
                if (x as usize / 8) % frames == i {
                    image::Rgba([122, 162, 247, 255])
                } else {
                    image::Rgba([36, 40, 59, 255])
                }
            });
            image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(200, 1))
        });
        enc.encode_frames(frames).unwrap();
    }
    out
}

pub const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240" viewBox="0 0 320 240">
<rect width="320" height="240" fill="#24283b"/>
<rect x="40" y="40" width="80" height="40" fill="#e0af68"/>
<rect x="40" y="80" width="160" height="40" fill="#a9c4ff"/>
<rect x="40" y="120" width="160" height="40" fill="#7aa2f7"/>
<rect x="40" y="160" width="160" height="40" fill="#4a67b3"/>
<circle cx="250" cy="120" r="44" fill="none" stroke="#e0af68" stroke-width="6"/>
</svg>"##;

/// Write pictures of every kind (and a text file) into `d`, as Hako sorts
/// them: app.ico, beach.png, dance.gif, IMG_2.jpg, IMG_10.jpg, logo.svg,
/// old.bmp, scan.tiff, sticker.webp.
pub fn write_folder(d: &Path) {
    std::fs::write(d.join("IMG_2.jpg"), jpeg_with_exif(&landscape(1600, 1066), &camera_exif(None))).unwrap();
    image::DynamicImage::ImageRgb8(landscape(800, 1200)).save(d.join("IMG_10.jpg")).unwrap();
    sprite().save(d.join("beach.png")).unwrap();
    std::fs::write(d.join("dance.gif"), gif(3)).unwrap();
    std::fs::write(d.join("logo.svg"), SVG).unwrap();
    image::DynamicImage::ImageRgb8(landscape(64, 48)).save(d.join("old.bmp")).unwrap();
    image::DynamicImage::ImageRgb8(landscape(96, 64)).save(d.join("scan.tiff")).unwrap();
    sprite().save(d.join("sticker.webp")).unwrap();
    image::imageops::resize(&sprite(), 32, 32, image::imageops::FilterType::Nearest).save(d.join("app.ico")).unwrap();
    std::fs::write(d.join("notes.txt"), "not a picture").unwrap();
}

pub fn folder(name: &str) -> PathBuf {
    let d = scratch(name);
    write_folder(&d);
    d
}

pub struct Rig {
    pub h: Harness<Shashin>,
    pub dir: PathBuf,
}

/// Shashin over a fresh folder, `open` shown, settled.
pub fn rig(name: &str, theme: ThemeChoice, open: &str) -> Rig {
    rig_in(folder(name), theme, open)
}

pub fn rig_in(dir: PathBuf, theme: ThemeChoice, open: &str) -> Rig {
    let mut app = Shashin::new();
    app.trash = aizome_shashin::trash::Trash { home: dir.join(".trash-can"), uid: 1000 };
    app.settings_path = Some(dir.join("settings.toml"));
    app.thumb_dir = dir.join(".thumbnails");
    app.dry_run = true;
    if !open.is_empty() {
        app.initial = vec![dir.join(open)];
    }
    let h = Harness::with(app, Appearance { theme, ..Appearance::default() }, false);
    let mut r = Rig { h, dir };
    settle(&mut r, 1.0);
    r
}

/// Draw, take in what the background sent, and again, until nothing is
/// being read or drawn.
pub fn settle(r: &mut Rig, scale: f64) {
    let w = r.h.window();
    let mut quiet = 0;
    for _ in 0..2000 {
        let _ = r.h.render(w, scale);
        r.h.send(Msg::Nothing);
        if r.h.program().idle() {
            quiet += 1;
            if quiet >= 3 {
                return;
            }
        } else {
            quiet = 0;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    panic!("Shashin didn't settle");
}
