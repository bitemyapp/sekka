//! The pointer's image: xcursor themes at the output's scale, named by
//! cursor-shape or by the compositor, with a drawn arrow when no theme is
//! installed.
//!
//! Every image is whole points at the output's integer scale, and its
//! buffer is exactly that many pixels: smithay draws the buffer at that
//! size and, on a KMS cursor plane, copies it there row for row. A theme
//! without the size asked for has its nearest image resampled to it.
//! (Adwaita has 72 and 96 px images but no 64: a 72 px buffer drawn as
//! 64 pt had smithay copy 72 px rows into a 64 px cursor plane, which took
//! the compositor down at every login; #276.)

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::{Logical, Size, Transform};
use tracing::warn;

/// One frame of a cursor at a pixel size.
pub struct CursorImage {
    pub buffer: MemoryRenderBuffer,
    /// Hotspot in points.
    pub hot: (f64, f64),
    /// Size in points: the buffer is this at its scale, to the pixel.
    pub size: Size<i32, Logical>,
}

impl CursorImage {
    /// An image of `size` points at `scale` from as many premultiplied
    /// ARGB8888 pixels (B, G, R, A bytes).
    fn new(pixels: &[u8], size: Size<i32, Logical>, scale: u32, hot: (f64, f64)) -> CursorImage {
        let px = size.to_buffer(scale as i32, Transform::Normal);
        debug_assert_eq!(pixels.len(), (px.w * px.h * 4) as usize);
        let buffer =
            MemoryRenderBuffer::from_slice(pixels, Fourcc::Argb8888, px, scale as i32, Transform::Normal, None);
        CursorImage { buffer, hot, size }
    }
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
        fit(&images, self.size, scale)
    }
}

/// The theme's image nearest `size` points at `scale` (the first frame of
/// an animated cursor), made exactly that size.
fn fit(images: &[xcursor::parser::Image], size: u32, scale: u32) -> Option<CursorImage> {
    let want = size * scale;
    // The nominal size nearest the one wanted, its first frame.
    let best = images.iter().min_by_key(|i| (i.size as i64 - want as i64).abs())?.size;
    let img = images.iter().find(|i| i.size == best && i.width > 0 && i.height > 0)?;
    // Its size in points, whole: the nominal size is `size`, and the image
    // may be larger or smaller than its nominal size.
    let k = size as f64 / best as f64;
    let pt: Size<i32, Logical> =
        Size::from((((img.width as f64 * k).round() as i32).max(1), ((img.height as f64 * k).round() as i32).max(1)));
    let px = pt.to_buffer(scale as i32, Transform::Normal);
    // xcursor pixels are ARGB in native order (BGRA bytes): premultiplied.
    let mut pixels = img.pixels_argb_le();
    if (px.w as u32, px.h as u32) != (img.width, img.height) {
        pixels = resample(&pixels, (img.width as usize, img.height as usize), (px.w as usize, px.h as usize));
    }
    // The hotspot where its pixel went.
    let hot = (img.xhot as f64 * pt.w as f64 / img.width as f64, img.yhot as f64 * pt.h as f64 / img.height as f64);
    Some(CursorImage::new(&pixels, pt, scale, hot))
}

/// Premultiplied pixels (4 bytes each) resampled by area: each new pixel is
/// the old ones under it, weighted by how much of each it covers.
fn resample(pixels: &[u8], (w, h): (usize, usize), (to_w, to_h): (usize, usize)) -> Vec<u8> {
    // For each new column (or row), the old ones under it and their weights.
    let spans = |from: usize, to: usize| -> Vec<Vec<(usize, f32)>> {
        let r = from as f64 / to as f64;
        (0..to)
            .map(|i| {
                let (a, b) = (i as f64 * r, (i + 1) as f64 * r);
                (a.floor() as usize..(b.ceil() as usize).min(from))
                    .map(|j| (j, ((b.min(j as f64 + 1.0) - a.max(j as f64)) / r) as f32))
                    .filter(|&(_, weight)| weight > 0.0)
                    .collect()
            })
            .collect()
    };
    let (cols, rows) = (spans(w, to_w), spans(h, to_h));
    let mut out = vec![0u8; to_w * to_h * 4];
    for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let mut sum = [0f32; 4];
        for &(y, wy) in &rows[i / to_w] {
            for &(x, wx) in &cols[i % to_w] {
                let from = &pixels[(y * w + x) * 4..][..4];
                for (s, c) in sum.iter_mut().zip(from) {
                    *s += *c as f32 * wx * wy;
                }
            }
        }
        for (c, s) in px.iter_mut().zip(sum) {
            *c = s.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
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
    CursorImage::new(&buf, Size::from((size as i32, size as i32)), scale, (0.0, 0.0))
}

#[cfg(test)]
mod tests {
    use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
    use smithay::backend::renderer::element::{Element, Kind, RenderElement, UnderlyingStorage};
    use smithay::backend::renderer::pixman::PixmanRenderer;
    use smithay::utils::{Rectangle, Scale};

    use super::*;

    /// Settings' pointer sizes (Accessibility › Pointer size).
    const SIZES: [u32; 5] = [24, 32, 40, 48, 64];

    /// Adwaita 50's sizes: 72 and 96 but no 64, 30 and 36 but no 32 or 40.
    const ADWAITA: [u32; 6] = [24, 30, 36, 48, 72, 96];

    /// A theme's images at these nominal sizes, square, `k` times as wide
    /// as their nominal size, opaque white, the hotspot an eighth of the
    /// way in.
    fn theme(nominal: &[u32], k: f64) -> Vec<xcursor::parser::Image> {
        nominal
            .iter()
            .map(|&size| {
                let n = (size as f64 * k).round() as u32;
                xcursor::parser::Image {
                    size,
                    width: n,
                    height: n,
                    xhot: n / 8,
                    yhot: n / 8,
                    delay: 0,
                    pixels_rgba: vec![0xff; (n * n * 4) as usize],
                    pixels_argb: vec![0xff; (n * n * 4) as usize],
                }
            })
            .collect()
    }

    /// Where the element the pointer becomes (render.rs) differs from what
    /// smithay's cursor plane copy takes it to be, on an output at the
    /// image's own scale: its source is the whole buffer, drawn 1:1.
    fn mismatch(r: &mut PixmanRenderer, img: &CursorImage, scale: u32) -> Option<String> {
        let e = MemoryRenderBufferRenderElement::from_buffer(
            r,
            (0.0, 0.0),
            &img.buffer,
            None,
            None,
            Some(img.size),
            Kind::Cursor,
        )
        .unwrap();
        let Some(UnderlyingStorage::Memory(mem)) = e.underlying_storage(r) else { unreachable!() };
        let whole = Rectangle::from_size(mem.size().to_f64());
        let drawn = e.geometry(Scale::from(scale as f64)).size;
        (e.src() != whole || (drawn.w, drawn.h) != (mem.size().w, mem.size().h))
            .then(|| format!("source {:?} of a {:?} buffer, drawn {drawn:?}", e.src().size, mem.size()))
    }

    #[test]
    fn every_pointer_size_is_its_whole_buffer() {
        // smithay copies a cursor element to a KMS cursor plane row for row
        // when its source is at the origin and drawn 1:1, taking the whole
        // buffer's rows for the source's: a 72 px Adwaita image given as
        // 64 pt overran the 64 px plane and took the compositor down (#276).
        let mut r = PixmanRenderer::new().unwrap();
        let mut bad = Vec::new();
        for (name, images) in [("Adwaita", theme(&ADWAITA, 1.0)), ("wider than nominal", theme(&ADWAITA, 4.0 / 3.0))] {
            for scale in 1..=3 {
                for size in SIZES {
                    let img = fit(&images, size, scale).unwrap();
                    if let Some(m) = mismatch(&mut r, &img, scale) {
                        bad.push(format!("{name}, {size} pt at {scale}×: {m}"));
                    }
                    let (hx, hy) = img.hot;
                    assert!(
                        hx < img.size.w as f64 && hy < img.size.h as f64,
                        "{name}, {size} pt: hotspot {:?}",
                        img.hot
                    );
                }
            }
        }
        for scale in 1..=3 {
            for size in SIZES {
                if let Some(m) = mismatch(&mut r, &arrow(size, scale), scale) {
                    bad.push(format!("the arrow, {size} pt at {scale}×: {m}"));
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn the_nearest_image_is_made_the_size_asked_for() {
        let images = theme(&ADWAITA, 1.0);
        // 64 pt: Adwaita's 72 px image, made 64 px; at 2×, its 96 made 128.
        assert_eq!(fit(&images, 64, 1).unwrap().size, Size::from((64, 64)));
        assert_eq!(fit(&images, 64, 2).unwrap().size, Size::from((64, 64)));
        // 24 pt at 2×: its 48 px image as it is.
        let img = fit(&images, 24, 2).unwrap();
        assert_eq!(img.size, Size::from((24, 24)));
        assert_eq!(img.hot, (3.0, 3.0));
        // A theme's images larger than their nominal size stay larger.
        assert_eq!(fit(&theme(&[24], 4.0 / 3.0), 24, 1).unwrap().size, Size::from((32, 32)));
    }

    #[test]
    fn resampling_keeps_an_opaque_image_opaque() {
        for (from, to) in [(72, 64), (36, 40), (96, 128), (96, 24), (30, 32)] {
            let out = resample(&vec![0xff; from * from * 4], (from, from), (to, to));
            assert_eq!(out.len(), to * to * 4);
            assert!(out.iter().all(|&b| b == 0xff), "{from} → {to}");
        }
        // Half covered by an opaque pixel, half by a clear one: half alpha,
        // premultiplied.
        let two = [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
        assert_eq!(resample(&two, (2, 1), (1, 1)), vec![0x80; 4]);
    }
}
