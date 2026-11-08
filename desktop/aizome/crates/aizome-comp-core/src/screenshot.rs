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

mod pixels;
mod placement;

pub use pixels::Image;
use placement::{CapturedOutput, Screenshot, layout_rect};

pub enum What {
    /// Every output, side by side as they are laid out.
    Screen,
    Output(Output),
    /// An area of the global layout.
    Area(Rect),
    /// A window with its tab and frame.
    Window(WinId),
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
            Image::from_capture(c)
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

/// The outputs an area shows, at their largest scale. Geometry planning and
/// validated pixel buffers keep placement arithmetic out of the capture loop.
fn stitch(state: &mut State, area: Option<Rect>) -> Result<Image, String> {
    let outputs = state.az.outputs.clone();
    let outputs = outputs
        .into_iter()
        .map(|output| Ok((output.clone(), layout_rect(state.az.output_rect(&output))?)))
        .collect::<Result<Vec<_>, String>>()?;
    let bounds = outputs.iter().map(|(_, rect)| *rect).reduce(|a, b| a.merge(b)).ok_or("no outputs")?;
    let crop = match area {
        Some(area) => layout_rect(area)?.intersection(bounds).ok_or("the area is off every output")?,
        None => bounds,
    };
    let shown: Vec<_> = outputs.into_iter().filter(|(_, rect)| rect.overlaps(crop)).collect();
    let scale =
        shown.iter().map(|(output, _)| output.current_scale().fractional_scale()).reduce(f64::max).unwrap_or(1.0);
    let mut screenshot = Screenshot::new(crop, scale)?;
    for (output, bounds) in shown {
        let captured = CapturedOutput::new(
            bounds,
            output.current_scale().fractional_scale(),
            state.capture_output(&output, false)?,
        )?;
        screenshot.plan(&captured)?.execute();
    }
    Ok(screenshot.into_image())
}

pub fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, image.width(), image.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(image.rgba()).map_err(|e| e.to_string())?;
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
