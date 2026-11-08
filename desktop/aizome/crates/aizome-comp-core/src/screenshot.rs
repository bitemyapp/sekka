//! Screenshots: the whole screen, an output, an area or a window, saved as
//! PNG (in ~/Pictures/Screenshots by default), optionally put on the
//! clipboard as image/png, and announced to subscribers.

use std::path::PathBuf;
use std::sync::Arc;

use aizome_ipc::Event;
use smithay::output::Output;
use smithay::wayland::selection::data_device::set_data_device_selection;

use crate::backend::Capture;
use crate::handlers::SelectionData;
use crate::scene::Purpose;
use crate::state::State;
use crate::{Rect, WinId};

pub enum What {
    /// Every output, side by side as they are laid out.
    Screen,
    Output(Output),
    /// An area of the global layout.
    Area(Rect),
    /// A window with its tab and frame.
    Window(WinId),
}

/// An image in memory: straight RGBA.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Takes a screenshot; returns where it went.
pub fn take(state: &mut State, what: What, path: Option<PathBuf>, clipboard: bool) -> Result<PathBuf, String> {
    let image = capture(state, what)?;
    let png = encode_png(&image)?;
    let path = match path {
        Some(p) => p,
        None => default_path()?,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, &png).map_err(|e| format!("{}: {e}", path.display()))?;
    if clipboard {
        let seat = state.az.seat.clone();
        set_data_device_selection(&state.az.dh, &seat, vec!["image/png".into()], SelectionData::Bytes(Arc::new(png)));
    }
    let p = path.to_string_lossy().into_owned();
    tracing::info!("screenshot {p}");
    state.az.ipc.emit(&Event::ScreenshotTaken { path: p });
    Ok(path)
}

/// Renders what's asked for into an image.
pub fn capture(state: &mut State, what: What) -> Result<Image, String> {
    match what {
        What::Output(o) => {
            let c = state.capture_output(&o, false)?;
            Ok(unpremultiply(c))
        }
        What::Screen => stitch(state, None),
        What::Area(r) => stitch(state, Some(r)),
        What::Window(id) => {
            // With its tab, frame and shadow, as the logic draws them.
            let bounds = state.lg_call(None, |vt, st| (vt.window_bounds)(st, id));
            let rect = bounds.or_else(|| state.az.window_rect(id)).ok_or("no such window")?;
            stitch(state, Some(rect))
        }
    }
}

/// The outputs an area shows (every output without one), placed by the
/// layout, at the largest scale among them. An area on one display keeps
/// its pixels as they are; a display at a smaller scale beside a larger
/// one is enlarged to it, each of its pixels repeated, to fill its place.
fn stitch(state: &mut State, area: Option<Rect>) -> Result<Image, String> {
    let outputs = state.az.outputs.clone();
    if outputs.is_empty() {
        return Err("no outputs".into());
    }
    let mut bbox = Rect::default();
    for o in &outputs {
        bbox = bbox.union(state.az.output_rect(o));
    }
    let crop = match area {
        Some(a) => a.intersection(bbox).ok_or("the area is off every output")?,
        None => bbox,
    };
    let shown: Vec<(Output, Rect)> =
        outputs.iter().map(|o| (o.clone(), state.az.output_rect(o))).filter(|(_, r)| r.intersects(crop)).collect();
    let scale = shown.iter().map(|(o, _)| o.current_scale().fractional_scale()).reduce(f64::max).unwrap_or(1.0);
    let width = (crop.w as f64 * scale).round() as u32;
    let height = (crop.h as f64 * scale).round() as u32;
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for (o, r) in &shown {
        let cap = unpremultiply(state.capture_output(o, false)?);
        if cap.width == 0 || cap.height == 0 {
            continue;
        }
        let own = o.current_scale().fractional_scale();
        let columns: Vec<(usize, usize)> = place(r.x - crop.x, r.w, width, scale, cap.width, own).collect();
        // At the image's own scale the columns are one run, copied at once.
        let run = columns.first().zip(columns.last()).filter(|(a, b)| b.1 - a.1 == columns.len() - 1).map(|(a, _)| *a);
        let (stride, cap_stride) = (width as usize * 4, cap.width as usize * 4);
        for (dy, sy) in place(r.y - crop.y, r.h, height, scale, cap.height, own) {
            let src = &cap.rgba[sy * cap_stride..][..cap_stride];
            let dst = &mut rgba[dy * stride..][..stride];
            match run {
                Some((dx, sx)) => {
                    let n = columns.len() * 4;
                    dst[dx * 4..][..n].copy_from_slice(&src[sx * 4..][..n]);
                }
                None => {
                    for &(dx, sx) in &columns {
                        dst[dx * 4..][..4].copy_from_slice(&src[sx * 4..][..4]);
                    }
                }
            }
        }
    }
    Ok(Image { width, height, rgba })
}

/// Along one axis, for an output `len` points long from `at` points into
/// the image: each image pixel it covers (of `size`, at `scale`), with an
/// output pixel (of `pixels`, at its own scale `own`). At the same scale,
/// translate by the rounded pixel offset; otherwise sample under its centre.
fn place(at: i32, len: i32, size: u32, scale: f64, pixels: u32, own: f64) -> impl Iterator<Item = (usize, usize)> {
    let edge = |p: i32| (p as f64 * scale).round().clamp(0.0, size as f64) as usize;
    let origin = (at as f64 * scale).round() as i64;
    let last = pixels as usize - 1;
    (edge(at)..edge(at + len)).map(move |d| {
        let source = if own == scale {
            // Stay in pixels: a divide/multiply round trip at fractional
            // scales can land just below an integer and repeat a row.
            (d as i64 - origin) as usize
        } else {
            let point = (d as f64 + 0.5) / scale - at as f64;
            (point * own) as usize
        };
        (d, source.min(last))
    })
}

fn unpremultiply(c: Capture) -> Image {
    let mut rgba = c.rgba;
    for p in rgba.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for ch in &mut p[..3] {
                *ch = ((*ch as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Image { width: c.width, height: c.height, rgba }
}

pub fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, image.width, image.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&image.rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// `~/Pictures/Screenshots/Screenshot YYYY-MM-DD at HH.MM.SS.png`.
fn default_path() -> Result<PathBuf, String> {
    let pictures = std::env::var_os("XDG_PICTURES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| aizome_config::home_dir().join("Pictures"));
    let dir = pictures.join("Screenshots");
    // Safety: localtime_r writes only into `tm`.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    let name = format!(
        "Screenshot {:04}-{:02}-{:02} at {:02}.{:02}.{:02}.png",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    );
    let mut path = dir.join(&name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(name.replace(".png", &format!(" ({n}).png")));
        n += 1;
    }
    Ok(path)
}

impl State {
    /// Renders an output afresh into memory (screenshots, screencopy), with
    /// or without the pointer.
    pub fn capture_output(&mut self, output: &Output, cursor: bool) -> Result<Capture, String> {
        // A mirror shows its source's picture (pointer and all), fitted.
        let (shown, cursor) = match self.az.mirror_source(output) {
            Some(s) => (s, true),
            None => (output.clone(), cursor),
        };
        let scene = self.scene(&shown, Purpose::Capture { cursor });
        let clear = crate::render::clear_color(&self.az);
        self.backend.capture(&self.az, output, &scene, clear)
    }
}

#[cfg(test)]
mod tests {
    use super::place;

    #[test]
    fn an_output_at_the_images_scale_is_copied_as_it_is() {
        // A 2× output 100 points wide, 30 points left of the image's edge,
        // in an image 120 pixels wide: its pixels 60 to 179 (not 0 to 119).
        let p: Vec<_> = place(-30, 100, 120, 2.0, 200, 2.0).collect();
        assert_eq!(p.len(), 120);
        assert!(p.iter().enumerate().all(|(i, &(d, s))| d == i && s == i + 60), "{p:?}");
    }

    #[test]
    fn fractional_scale_crops_keep_every_native_pixel_in_order() {
        // At 150%, a crop one point down starts at source row 2. A
        // floating-point round trip made row 5 copy row 6 again, not 7.
        // Also cover 175% and outputs placed at positive half-pixel
        // offsets: rounding the origin must not drop their first pixel.
        for (scale, at, first, source) in [(1.5, -1, 0, 2), (1.75, -2, 0, 4), (1.25, 2, 3, 0), (1.5, 1, 2, 0)] {
            let placed: Vec<_> = place(at, 720, 150, scale, (720.0 * scale) as u32, scale).collect();
            let expected: Vec<_> = (first..150).zip(source..).collect();
            assert_eq!(placed, expected, "scale {scale}, offset {at}");
        }
    }

    #[test]
    fn an_output_at_a_smaller_scale_is_enlarged_into_its_place() {
        // A 1× output 960 points wide, 1440 points into a 2× image (beside
        // a 2× display): image pixels 2880 to 4799, each of its pixels twice.
        let p: Vec<_> = place(1440, 960, 4800, 2.0, 960, 1.0).collect();
        assert_eq!((p[0], p[1], p[2], p[1919]), ((2880, 0), (2881, 0), (2882, 1), (4799, 959)));
        // A 2× image of a band 600 points down it: its rows 600 to 677
        // (stepping a row at its own scale went from 1200, past its 1080).
        let p: Vec<_> = place(-600, 1080, 156, 2.0, 1080, 1.0).collect();
        assert_eq!(p.len(), 156);
        assert_eq!((p[0], p[1], p[2], p[155]), ((0, 600), (1, 600), (2, 601), (155, 677)));
        // At 1.25 in a 2× image: 1.6 image pixels to each of its own.
        let p: Vec<_> = place(0, 8, 16, 2.0, 10, 1.25).collect();
        assert_eq!(p.iter().map(|&(_, s)| s).collect::<Vec<_>>(), [0, 0, 1, 2, 2, 3, 4, 4, 5, 5, 6, 7, 7, 8, 9, 9]);
    }

    #[test]
    fn an_output_off_the_image_covers_none_of_it() {
        assert_eq!(place(200, 100, 120, 1.0, 100, 1.0).count(), 0);
        assert_eq!(place(-300, 100, 120, 1.0, 100, 1.0).count(), 0);
    }
}
