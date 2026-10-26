//! The pointer's image: xcursor themes at the output's scale, named by
//! cursor-shape or by the compositor, with a drawn arrow when no theme is
//! installed.

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::Transform;
use tracing::warn;

/// One frame of a cursor at a pixel size.
pub struct CursorImage {
    pub buffer: MemoryRenderBuffer,
    /// Hotspot in points.
    pub hot: (f64, f64),
    /// Size in points.
    pub size: (f64, f64),
}

pub struct CursorTheme {
    name: String,
    /// Nominal size in points.
    pub size: u32,
    theme: xcursor::CursorTheme,
    cache: HashMap<(String, u32), Option<CursorImage>>,
}

impl CursorTheme {
    pub fn load(name: &str, size: u32) -> CursorTheme {
        CursorTheme {
            name: name.to_string(),
            size: size.clamp(8, 256),
            theme: xcursor::CursorTheme::load(name),
            cache: HashMap::new(),
        }
    }

    /// The image for a cursor name at an integer scale (the first frame of
    /// animated cursors).
    pub fn get(&mut self, name: &str, scale: u32) -> &CursorImage {
        let key = (name.to_string(), scale);
        if !self.cache.contains_key(&key) {
            let img = self.load_image(name, scale);
            if img.is_none() && name != "default" {
                warn!("no cursor {name} in theme {}", self.name);
            }
            self.cache.insert(key.clone(), img);
        }
        if self.cache[&key].is_none() {
            let fallback_key = ("__arrow".to_string(), scale);
            self.cache.entry(fallback_key.clone()).or_insert_with(|| Some(arrow(self.size, scale)));
            return self.cache[&fallback_key].as_ref().unwrap();
        }
        self.cache[&key].as_ref().unwrap()
    }

    fn load_image(&self, name: &str, scale: u32) -> Option<CursorImage> {
        let names: Vec<&str> = std::iter::once(name).chain(alternatives(name).iter().copied()).collect();
        let path = names.iter().find_map(|n| self.theme.load_icon(n))?;
        let bytes = std::fs::read(path).ok()?;
        let images = xcursor::parser::parse_xcursor(&bytes)?;
        let want = self.size * scale;
        // The nominal size nearest the one wanted, its first frame.
        let best = images.iter().min_by_key(|i| (i.size as i64 - want as i64).abs())?.size;
        let img = images.iter().find(|i| i.size == best)?;
        // xcursor pixels are ARGB in native order (BGRA bytes): premultiplied.
        let buffer = MemoryRenderBuffer::from_slice(
            &img.pixels_argb_le(),
            Fourcc::Argb8888,
            (img.width as i32, img.height as i32),
            scale as i32,
            Transform::Normal,
            None,
        );
        let s = best as f64 / self.size as f64;
        Some(CursorImage {
            buffer,
            hot: (img.xhot as f64 / s, img.yhot as f64 / s),
            size: (img.width as f64 / s, img.height as f64 / s),
        })
    }
}

trait ArgbBytes {
    fn pixels_argb_le(&self) -> Vec<u8>;
}

impl ArgbBytes for xcursor::parser::Image {
    /// Little-endian ARGB8888: B, G, R, A in memory, premultiplied (as
    /// xcursor files store them).
    fn pixels_argb_le(&self) -> Vec<u8> {
        self.pixels_rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()
    }
}

/// Other names themes use for the same cursor.
fn alternatives(name: &str) -> &'static [&'static str] {
    match name {
        "default" => &["left_ptr", "arrow"],
        "pointer" => &["hand2", "hand1", "pointing_hand"],
        "text" => &["xterm", "ibeam"],
        "grab" => &["openhand", "hand1"],
        "grabbing" => &["closedhand", "fleur"],
        "move" | "all-scroll" => &["fleur"],
        "n-resize" => &["top_side"],
        "s-resize" => &["bottom_side"],
        "e-resize" => &["right_side"],
        "w-resize" => &["left_side"],
        "ne-resize" => &["top_right_corner"],
        "nw-resize" => &["top_left_corner"],
        "se-resize" => &["bottom_right_corner"],
        "sw-resize" => &["bottom_left_corner"],
        "crosshair" => &["cross", "tcross"],
        "wait" => &["watch"],
        "progress" => &["left_ptr_watch"],
        "not-allowed" => &["crossed_circle"],
        "context-menu" => &["left_ptr"],
        _ => &[],
    }
}

/// A plain arrow when no cursor theme is installed: Aizome's text colour
/// with a dark edge.
fn arrow(size: u32, scale: u32) -> CursorImage {
    let px = (size * scale) as usize;
    let mut buf = vec![0u8; px * px * 4];
    let h = px as f64 * 0.75;
    for y in 0..px {
        for x in 0..px {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            // A triangle from the tip down and to the right.
            let inside = fy < h && fx < fy * 0.62 + 0.5;
            let edge = inside
                && (fx < 1.5 * scale as f64 || fx > fy * 0.62 - 1.0 * scale as f64 || fy > h - 1.5 * scale as f64);
            if inside {
                let (r, g, b) = if edge { (0x0b, 0x0c, 0x12) } else { (0xc0, 0xca, 0xf5) };
                let i = (y * px + x) * 4;
                buf[i..i + 4].copy_from_slice(&[b, g, r, 0xff]);
            }
        }
    }
    let buffer = MemoryRenderBuffer::from_slice(
        &buf,
        Fourcc::Argb8888,
        (px as i32, px as i32),
        scale as i32,
        Transform::Normal,
        None,
    );
    CursorImage { buffer, hot: (0.0, 0.0), size: (size as f64, size as f64) }
}
