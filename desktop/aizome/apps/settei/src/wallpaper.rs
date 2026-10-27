//! Wallpapers to choose from: the system's backgrounds and the pictures in
//! `~/Pictures`, as small thumbnails made off the main thread (the full
//! image is read, drawn small with Sidestep's offscreen renderer, and let
//! go, so a grid of 4K photos costs a grid of thumbnails).

use std::path::{Path, PathBuf};

use aizome_kit::Image;
use aizome_kit::layout::{Rect, Size};
use aizome_kit::sidestep_ui::{Appearance as Look, offscreen};

/// A thumbnail's size, in points (drawn at 2×).
pub const THUMB: Size = Size::new(140.0, 88.0);

/// Where wallpapers are looked for, in order.
pub fn dirs() -> Vec<PathBuf> {
    let home = aizome_config::home_dir();
    vec![
        PathBuf::from("/run/current-system/sw/share/backgrounds"),
        PathBuf::from("/run/current-system/sw/share/wallpapers"),
        home.join("Pictures/Wallpapers"),
        home.join("Pictures"),
    ]
}

/// Whether a file's name says it's a picture.
pub fn is_picture(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp"))
}

/// The pictures under `dirs` (two levels deep), at most `limit`, by name
/// within each folder.
pub fn find(dirs: &[PathBuf], limit: usize) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in dirs {
        walk(d, 2, &mut out, limit);
        if out.len() >= limit {
            break;
        }
    }
    out.dedup();
    out
}

fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>, limit: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in &entries {
        if out.len() >= limit {
            return;
        }
        if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
            continue;
        }
        if p.is_file() && is_picture(p) && !out.contains(p) {
            out.push(p.clone());
        }
    }
    if depth > 1 {
        for p in entries {
            if p.is_dir() {
                walk(&p, depth - 1, out, limit);
            }
        }
    }
}

/// A picture's thumbnail, cropped to cover [`THUMB`], or `None` if it
/// can't be read.
pub fn thumbnail(path: &Path) -> Option<Image> {
    let image = Image::open(path).ok()?;
    let s = image.size();
    if s.width <= 0.0 || s.height <= 0.0 {
        return None;
    }
    let k = (THUMB.width / s.width).max(THUMB.height / s.height);
    let (w, h) = (s.width * k, s.height * k);
    let dst =
        Rect::new((THUMB.width - w) / 2.0, (THUMB.height - h) / 2.0, (THUMB.width + w) / 2.0, (THUMB.height + h) / 2.0);
    Some(offscreen::render(THUMB, 2.0, Look::DARK, |c| c.draw_image(&image, dst)))
}

/// A wallpaper's name from its file: `aizome-dusk.png` → "aizome dusk".
pub fn name(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("").replace(['-', '_'], " ")
}

/// The sample system's wallpapers: gradients of the dye ramp, made here,
/// at paths that don't exist.
pub fn sample() -> Vec<(PathBuf, Image)> {
    let make = |top: u32, bottom: u32, glow: u32| -> Image {
        let (w, h) = (280u32, 176u32);
        let px: Vec<u8> = (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    let t = y as f32 / (h - 1) as f32;
                    let mix = |a: u32, b: u32, s: u32| -> u8 {
                        let (ca, cb) = (((a >> s) & 0xff) as f32, ((b >> s) & 0xff) as f32);
                        (ca + (cb - ca) * t) as u8
                    };
                    // A low sun: a soft band of the glow colour.
                    let d = ((x as f32 / w as f32 - 0.62).powi(2) * 3.0 + (t - 0.78).powi(2) * 9.0).sqrt();
                    let g = (1.0 - d * 2.2).clamp(0.0, 1.0) * 0.55;
                    let add = |c: u8, s: u32| -> u8 { (c as f32 + (((glow >> s) & 0xff) as f32 - c as f32) * g) as u8 };
                    let (r, gg, b) = (mix(top, bottom, 16), mix(top, bottom, 8), mix(top, bottom, 0));
                    [add(r, 16), add(gg, 8), add(b, 0), 255]
                })
            })
            .collect();
        Image::from_rgba(w, h, &px).expect("pixels").with_scale(0.5)
    };
    vec![
        (
            PathBuf::from("/run/current-system/sw/share/backgrounds/aizome/ai-dusk.png"),
            make(0x16161e, 0x3d59a1, 0xe0af68),
        ),
        (
            PathBuf::from("/run/current-system/sw/share/backgrounds/aizome/hanada-morning.png"),
            make(0xa9c4ff, 0xf4f5fa, 0xffffff),
        ),
        (
            PathBuf::from("/run/current-system/sw/share/backgrounds/aizome/kon-night.png"),
            make(0x0b0c12, 0x24283b, 0x7aa2f7),
        ),
        (
            PathBuf::from("/run/current-system/sw/share/backgrounds/aizome/kohaku.png"),
            make(0x24283b, 0x9a6a12, 0xf5c542),
        ),
        (PathBuf::from("/home/aiko/Pictures/kamenozoki.jpg"), make(0x7aa2f7, 0xdde0ec, 0xffffff)),
        (PathBuf::from("/home/aiko/Pictures/violet-hour.jpg"), make(0x1a1b26, 0x7247c9, 0xf7768e)),
    ]
}
