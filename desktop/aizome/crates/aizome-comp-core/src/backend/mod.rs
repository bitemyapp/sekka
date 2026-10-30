//! Where frames go: offscreen (headless), a window in another session
//! (winit), or the displays themselves (udev: DRM/KMS with libseat).

pub mod headless;
pub mod udev;
pub mod winit;

use std::time::{Duration, Instant};

use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::RenderElementStates;
use smithay::backend::renderer::{Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer};
use smithay::desktop::layer_map_for_output;
use smithay::desktop::utils::{
    OutputPresentationFeedback, surface_presentation_feedback_flags_from_states, surface_primary_scanout_output,
    update_surface_primary_scanout_output,
};
use smithay::input::pointer::CursorImageStatus;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Buffer as BufferCoords, Rectangle, Size, Transform};

use crate::render::{OutputElement, scene_elements};
use crate::scene::Scene;
use crate::state::Aizome;

pub enum Backend {
    Headless(headless::Headless),
    Winit(Box<winit::WinitBackend>),
    Udev(Box<udev::Udev>),
}

/// Pixels read back from a frame: RGBA, premultiplied, rows `width × 4`
/// bytes apart.
pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Backend {
    pub fn seat_name(&self) -> String {
        match self {
            Backend::Udev(u) => u.seat_name(),
            _ => "seat0".into(),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Backend::Headless(_) => "headless",
            Backend::Winit(_) => "winit",
            Backend::Udev(_) => "udev",
        }
    }

    pub fn renderer_name(&self) -> String {
        match self {
            Backend::Headless(h) => h.renderer_name().into(),
            Backend::Winit(_) => "gles".into(),
            Backend::Udev(u) => u.renderer_name(),
        }
    }

    /// Whether frames are drawn on the CPU (pixman): there, blending and
    /// scaling cost a pass over every pixel they touch.
    pub fn software(&self) -> bool {
        match self {
            Backend::Headless(h) => matches!(h.renderer, headless::HeadlessRenderer::Pixman(_)),
            Backend::Winit(_) => false,
            Backend::Udev(u) => u.software(),
        }
    }

    /// Whether an output can draw a frame now; if not, the backend comes
    /// back when it can (a vblank, the refresh timer).
    pub fn frame_due(&mut self, az: &mut Aizome, output: &Output) -> bool {
        match self {
            Backend::Headless(h) => h.frame_due(az, output),
            Backend::Winit(_) => true,
            Backend::Udev(u) => u.frame_due(az, output),
        }
    }

    /// Draws an output's scene (for a mirror, its source's picture).
    /// `started` is when collecting the scene began, for frame times.
    pub fn redraw(&mut self, az: &mut Aizome, output: &Output, scene: Scene, clear: [f32; 4], started: Instant) {
        match self {
            Backend::Headless(h) => h.redraw(az, output, scene, clear, started),
            Backend::Winit(w) => w.redraw(az, output, scene, clear, started),
            Backend::Udev(u) => u.redraw(az, output, scene, clear, started),
        }
    }

    /// Imports a surface's new buffer ahead of the next frame.
    pub fn early_import(&mut self, surface: &WlSurface) {
        if let Backend::Udev(u) = self {
            u.early_import(surface);
        }
    }

    /// Under GLES a wl_shm buffer's pixels are copied into a texture when
    /// it's imported, so the buffer can go back to the client then, not at
    /// its next commit: import the tree now and release the buffers it just
    /// attached (`fresh`). Pixman draws from the client's memory, and
    /// several GPUs may each read it, so there they are kept.
    ///
    /// The client hears `release` once more when the next commit replaces
    /// the buffer (smithay's own); by then its pixels are in the texture.
    pub fn release_shm_after_import(&mut self, surface: &WlSurface, fresh: &[WlBuffer]) {
        let imported = match self {
            Backend::Headless(h) => h.import_shm_copies(surface),
            Backend::Winit(w) => w.import_shm_copies(surface),
            Backend::Udev(u) => u.import_shm_copies(surface),
        };
        if imported {
            for b in fresh {
                b.release();
            }
        }
    }

    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> bool {
        match self {
            Backend::Headless(h) => h.import_dmabuf(dmabuf),
            Backend::Winit(w) => w.import_dmabuf(dmabuf),
            Backend::Udev(u) => u.import_dmabuf(dmabuf),
        }
    }

    /// Renders a surface (and what's on it) into memory at a size and
    /// scale of its own, over nothing (a layer surface's last frame).
    pub fn render_to_memory(
        &mut self,
        surface: &WlSurface,
        size: Size<i32, smithay::utils::Physical>,
        scale: f64,
    ) -> Result<Capture, String> {
        match self {
            Backend::Headless(h) => h.render_to_memory(surface, size, scale),
            Backend::Winit(w) => w.render_to_memory(surface, size, scale),
            Backend::Udev(u) => u.render_to_memory(surface, size, scale),
        }
    }

    /// Renders a scene afresh into memory, at an output's size, scale and
    /// transform (screenshots, screencopy, mirrors).
    pub fn capture(&mut self, az: &Aizome, output: &Output, scene: &Scene, clear: [f32; 4]) -> Result<Capture, String> {
        match self {
            Backend::Headless(h) => h.capture(az, output, scene, clear),
            Backend::Winit(w) => w.capture(az, output, scene, clear),
            Backend::Udev(u) => u.capture(az, output, scene, clear),
        }
    }

    /// Makes a capture copy with the renderer (protocols/capture), for
    /// wlr-screencopy and ext-image-copy-capture.
    pub fn screencopy(
        &mut self,
        az: &mut Aizome,
        job: &mut crate::protocols::capture::Job,
    ) -> Result<crate::protocols::capture::Outcome, String> {
        match self {
            Backend::Headless(h) => h.screencopy(az, job),
            Backend::Winit(w) => w.screencopy(az, job),
            Backend::Udev(u) => u.screencopy(az, job),
        }
    }

    /// Where capture dmabufs may be and what GLES renders them in, if any.
    pub fn screencopy_dmabuf(&mut self) -> Option<crate::protocols::capture::DmabufTarget> {
        match self {
            Backend::Headless(h) => h.screencopy_dmabuf(),
            Backend::Winit(w) => w.screencopy_dmabuf(),
            Backend::Udev(u) => u.screencopy_dmabuf(),
        }
    }

    /// Turns each display off or on (DPMS) as [`Aizome::wants_power`]
    /// says; off udev only the state changes.
    pub fn apply_power(&mut self, az: &mut Aizome) {
        let outputs: Vec<Output> = az.out.keys().cloned().collect();
        for output in outputs {
            let on = az.wants_power(&output);
            let Some(o) = az.out.get_mut(&output) else { continue };
            if o.powered == on {
                continue;
            }
            o.powered = on;
            o.redraw_queued |= on;
            if let Backend::Udev(u) = self {
                u.set_power(&output, on);
            }
        }
    }

    /// Applies a mode, scale, position, transform or enablement change to
    /// an output.
    pub fn configure_output(&mut self, az: &mut Aizome, change: &crate::outputs::OutputChange) -> Result<(), String> {
        match self {
            Backend::Udev(u) => u.configure_output(az, change),
            Backend::Headless(h) => h.configure_output(az, change),
            Backend::Winit(_) if change.enabled == Some(false) => Err("the window's output can't be turned off".into()),
            Backend::Winit(_) => crate::outputs::apply_logical(az, change),
        }
    }

    /// How many entries an output's gamma ramps have (0: none).
    pub fn gamma_size(&self, output: &Output) -> usize {
        match self {
            Backend::Udev(u) => u.gamma_size(output),
            _ => 0,
        }
    }

    /// Sets an output's gamma ramps (wlr-gamma-control).
    pub fn set_gamma(&mut self, output: &Output, r: &[u16], g: &[u16], b: &[u16]) -> Result<(), String> {
        match self {
            Backend::Udev(u) => u.set_gamma(output, r, g, b),
            _ => Err("no gamma here".into()),
        }
    }

    /// Night Light's temperature on the displays that take gamma ramps.
    pub fn set_temperature(&mut self, az: &mut Aizome, kelvin: u32) {
        if let Backend::Udev(u) = self {
            u.set_temperature(az, kelvin);
        }
    }

    pub fn headless_mut(&mut self) -> Option<&mut headless::Headless> {
        match self {
            Backend::Headless(h) => Some(h),
            _ => None,
        }
    }

    /// Switches virtual terminals (udev only).
    pub fn change_vt(&mut self, vt: i32) {
        if let Backend::Udev(u) = self {
            u.change_vt(vt);
        }
    }
}

/// The outcome of drawing one frame.
pub struct Frame {
    pub damaged: bool,
    pub states: RenderElementStates,
    pub cpu: Duration,
    pub gpu: Option<Duration>,
}

/// Draws an output's elements through a damage tracker into a bound
/// framebuffer, timing it from `started` (when collecting the elements
/// began), and with `wait_gpu` the GPU's part too.
#[allow(clippy::too_many_arguments)] // what the damage tracker renders with, and the timing
pub fn render_frame<R>(
    renderer: &mut R,
    framebuffer: &mut R::Framebuffer<'_>,
    tracker: &mut OutputDamageTracker,
    age: usize,
    elements: &[OutputElement<R>],
    clear: [f32; 4],
    started: Instant,
    wait_gpu: bool,
) -> Result<Frame, String>
where
    R: Renderer + ImportAll + ImportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    let _trace = aizome_trace::Span::new(aizome_trace::Phase::Render, 0, 0);
    let res = tracker.render_output(renderer, framebuffer, age, elements, clear).map_err(|e| format!("{e:?}"))?;
    let cpu = started.elapsed();
    let damaged = res.damage.is_some_and(|d| !d.is_empty());
    let gpu = if wait_gpu && damaged {
        let t = Instant::now();
        let _ = res.sync.wait();
        Some(t.elapsed())
    } else {
        None
    };
    let states = res.states;
    Ok(Frame { damaged, states, cpu, gpu })
}

/// After a frame: statistics, the surfaces' primary outputs, frame
/// callbacks, fifo barriers and presentation feedback.
pub fn post_frame(az: &mut Aizome, output: &Output, frame: &Frame, presented: bool) {
    if aizome_trace::enabled() && frame.damaged {
        let id = az.outputs.iter().position(|o| o == output).map_or(0, |i| i as u64 + 1);
        aizome_trace::instant(aizome_trace::Phase::FrameQueued, id, az.out.get(output).map_or(0, |o| o.frame_seq + 1));
        if presented {
            aizome_trace::instant(aizome_trace::Phase::PresentationEstimate, id, 0);
        }
    }
    if let Some(o) = az.out.get_mut(output) {
        if frame.damaged {
            o.stats.record(frame.cpu, frame.gpu);
        } else {
            o.stats.skip();
        }
        o.frame_seq += 1;
        if frame.damaged {
            o.damaged_frames += 1;
        }
        o.last_frame = Some(Instant::now());
    }
    if az.mirror_source(output).is_some() {
        // A mirror shows a copy: its source's frame did the rest.
        return;
    }
    if frame.damaged {
        let mirrors: Vec<Output> = az.mirrors.iter().filter(|(_, s)| s == output).map(|(m, _)| m.clone()).collect();
        for m in mirrors {
            az.queue_redraw(&m);
        }
    }
    update_primary_outputs(az, output, &frame.states);
    send_frames(az, output);
    crate::pacing::frame_drawn(az, output);
    if presented && frame.damaged {
        let mut feedback = take_feedback(az, output, &frame.states);
        let refresh = az.out.get(output).map(|o| o.refresh).unwrap_or(Duration::from_millis(16));
        let seq = az.out.get(output).map(|o| o.frame_seq).unwrap_or(0);
        feedback.presented::<_, smithay::utils::Monotonic>(
            az.clock.now(),
            smithay::wayland::presentation::Refresh::fixed(refresh),
            seq,
            wp_presentation_feedback::Kind::empty(),
        );
    }
    // Keep animating.
    if az.view.animating {
        az.queue_redraw(output);
    }
}

fn update_primary_outputs(az: &Aizome, output: &Output, states: &RenderElementStates) {
    let compare = smithay::backend::renderer::element::default_primary_scanout_output_compare;
    for m in az.windows.values() {
        m.window.with_surfaces(|surface, data| {
            update_surface_primary_scanout_output(surface, output, data, states, compare);
        });
    }
    let map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.with_surfaces(|surface, data| {
            update_surface_primary_scanout_output(surface, output, data, states, compare);
        });
    }
}

/// Frame callbacks for everything visible on an output (and, now and
/// then, for hidden windows so they don't stall forever).
pub fn send_frames(az: &Aizome, output: &Output) {
    let time = az.now();
    let throttle = Some(Duration::from_millis(995));
    // The render states account for occlusion, subsurfaces, thumbnails and
    // other outputs. A logically shown window may be fully covered by a
    // fullscreen client, as may the shell's top/background layers.
    for m in az.windows.values() {
        m.window.send_frame(output, time, throttle, surface_primary_scanout_output);
    }
    let map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.send_frame(output, time, throttle, surface_primary_scanout_output);
    }
    drop(map);
    if let Some(lock) = az.out.get(output).and_then(|o| o.lock_surface.as_ref()) {
        smithay::desktop::utils::send_frames_surface_tree(lock.wl_surface(), output, time, throttle, |_, _| {
            Some(output.clone())
        });
    }
    if let CursorImageStatus::Surface(s) = &az.cursor_status {
        smithay::desktop::utils::send_frames_surface_tree(s, output, time, throttle, |_, _| Some(output.clone()));
    }
    if let Some(icon) = &az.dnd_icon {
        smithay::desktop::utils::send_frames_surface_tree(icon, output, time, throttle, |_, _| Some(output.clone()));
    }
    for u in &az.unmapped {
        if let Some(t) = u.window.toplevel() {
            smithay::desktop::utils::send_frames_surface_tree(t.wl_surface(), output, time, throttle, |_, _| {
                Some(output.clone())
            });
        }
    }
}

pub fn take_feedback(az: &Aizome, output: &Output, states: &RenderElementStates) -> OutputPresentationFeedback {
    let mut feedback = OutputPresentationFeedback::new(output);
    for m in az.windows.values() {
        if m.view.shown {
            m.window.take_presentation_feedback(&mut feedback, surface_primary_scanout_output, |surface, _| {
                surface_presentation_feedback_flags_from_states(surface, states)
            });
        }
    }
    let map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.take_presentation_feedback(&mut feedback, surface_primary_scanout_output, |surface, _| {
            surface_presentation_feedback_flags_from_states(surface, states)
        });
    }
    feedback
}

/// Renders a scene for an output into a fresh offscreen buffer and reads
/// it back, upright (the output's transform undone). For a mirror, the
/// scene is its source's, fitted.
pub fn capture_with<R, T>(
    renderer: &mut R,
    az: &Aizome,
    output: &Output,
    scene: &Scene,
    clear: [f32; 4],
) -> Result<Capture, String>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    let mode = output.current_mode().ok_or("the output has no mode")?;
    let size = output.current_transform().transform_size(mode.size);
    let scale = output.current_scale().fractional_scale();
    let (elements, clear) = scene_for::<R, T>(renderer, az, output, scene, clear);
    let buffer_size: Size<i32, BufferCoords> = Size::from((size.w, size.h));
    let mut target: T =
        renderer.create_buffer(Fourcc::Abgr8888, buffer_size).map_err(|e| format!("offscreen buffer: {e:?}"))?;
    let mut tracker = OutputDamageTracker::new(size, scale, Transform::Normal);
    let rgba = {
        let mut fb = renderer.bind(&mut target).map_err(|e| format!("bind: {e:?}"))?;
        let res =
            tracker.render_output(renderer, &mut fb, 0, &elements, clear).map_err(|e| format!("render: {e:?}"))?;
        let _ = res.sync.wait();
        let mapping = renderer
            .copy_framebuffer(&fb, Rectangle::from_size(buffer_size), Fourcc::Abgr8888)
            .map_err(|e| format!("read back: {e:?}"))?;
        renderer.map_texture(&mapping).map_err(|e| format!("map: {e:?}"))?.to_vec()
    };
    Ok(Capture { width: size.w as u32, height: size.h as u32, rgba })
}

/// Renders a surface tree into a fresh buffer `size` pixels big, at
/// `scale`, over nothing, and reads it back.
pub fn memory_with<R, T>(
    renderer: &mut R,
    surface: &WlSurface,
    size: Size<i32, smithay::utils::Physical>,
    scale: f64,
) -> Result<Capture, String>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    use smithay::backend::renderer::element::Kind;
    use smithay::backend::renderer::element::surface::{
        WaylandSurfaceRenderElement, render_elements_from_surface_tree,
    };
    let mut elements: Vec<WaylandSurfaceRenderElement<R>> =
        render_elements_from_surface_tree(renderer, surface, (0, 0), scale, 1.0, Kind::Unspecified);
    if elements.is_empty() {
        // A surface being destroyed is no longer in its own tree: just it.
        let root = smithay::wayland::compositor::with_states(surface, |states| {
            WaylandSurfaceRenderElement::from_surface(
                renderer,
                surface,
                states,
                (0.0, 0.0).into(),
                1.0,
                Kind::Unspecified,
            )
        });
        elements.extend(root.ok().flatten());
    }
    let buffer_size: Size<i32, BufferCoords> = Size::from((size.w, size.h));
    let mut target: T =
        renderer.create_buffer(Fourcc::Abgr8888, buffer_size).map_err(|e| format!("offscreen buffer: {e:?}"))?;
    let mut tracker = OutputDamageTracker::new(size, scale, Transform::Normal);
    let rgba = {
        let mut fb = renderer.bind(&mut target).map_err(|e| format!("bind: {e:?}"))?;
        let res =
            tracker.render_output(renderer, &mut fb, 0, &elements, [0.0; 4]).map_err(|e| format!("render: {e:?}"))?;
        let _ = res.sync.wait();
        let mapping = renderer
            .copy_framebuffer(&fb, Rectangle::from_size(buffer_size), Fourcc::Abgr8888)
            .map_err(|e| format!("read back: {e:?}"))?;
        renderer.map_texture(&mapping).map_err(|e| format!("map: {e:?}"))?.to_vec()
    };
    Ok(Capture { width: size.w as u32, height: size.h as u32, rgba })
}

/// The wl_shm buffers a commit of this surface tree is about to make
/// current (call before `on_commit_buffer_handler`, which takes them).
pub fn fresh_shm_buffers(surface: &WlSurface) -> Vec<WlBuffer> {
    use smithay::backend::renderer::{BufferType, buffer_type};
    use smithay::wayland::compositor::{
        BufferAssignment, SurfaceAttributes, TraversalAction, with_surface_tree_upward,
    };
    let mut out = Vec::new();
    with_surface_tree_upward(
        surface,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_, states, _| {
            if let Some(BufferAssignment::NewBuffer(b)) =
                states.cached_state.get::<SurfaceAttributes>().current().buffer.as_ref()
                && matches!(buffer_type(b), Some(BufferType::Shm))
            {
                out.push(b.clone());
            }
        },
        |_, _, _| true,
    );
    out
}

/// What an output shows and the colour behind it: its scene, or for a
/// mirror its source's scene (`scene`) rendered afresh, scaled to fit and
/// letterboxed in black. (A mirror costs a second render of its source and
/// a read-back each frame; it redraws whenever its source does.)
pub fn scene_for<R, T>(
    renderer: &mut R,
    az: &Aizome,
    output: &Output,
    scene: &Scene,
    clear: [f32; 4],
) -> (Vec<OutputElement<R>>, [f32; 4])
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    let Some(source) = az.mirror_source(output) else {
        let scale = output.current_scale().fractional_scale();
        return (scene_elements(renderer, scale, scene), clear);
    };
    let black = [0.0, 0.0, 0.0, 1.0];
    match mirror_elements::<R, T>(renderer, az, output, &source, scene, clear) {
        Ok(e) => (e, black),
        Err(e) => {
            tracing::warn!("mirroring {} on {}: {e}", source.name(), output.name());
            (Vec::new(), black)
        }
    }
}

fn mirror_elements<R, T>(
    renderer: &mut R,
    az: &Aizome,
    mirror: &Output,
    source: &Output,
    scene: &Scene,
    clear: [f32; 4],
) -> Result<Vec<OutputElement<R>>, String>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    use smithay::backend::renderer::element::Kind;
    use smithay::backend::renderer::element::memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement};
    let cap = capture_with::<R, T>(renderer, az, source, scene, clear)?;
    let mode = mirror.current_mode().ok_or("the mirror has no mode")?;
    let size = mirror.current_transform().transform_size(mode.size);
    let (fit, at) = letterbox((cap.width as i32, cap.height as i32), (size.w, size.h));
    let scale = mirror.current_scale().fractional_scale();
    let buffer = MemoryRenderBuffer::from_slice(
        &cap.rgba,
        Fourcc::Abgr8888,
        (cap.width as i32, cap.height as i32),
        1,
        Transform::Normal,
        None,
    );
    let logical =
        Size::<f64, smithay::utils::Logical>::from((fit.0 as f64 / scale, fit.1 as f64 / scale)).to_i32_round();
    // All of the picture (src), into the fitted size.
    let src = smithay::utils::Rectangle::<f64, smithay::utils::Logical>::from_size(
        (cap.width as f64, cap.height as f64).into(),
    );
    let e = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        (at.0 as f64, at.1 as f64),
        &buffer,
        None,
        Some(src),
        Some(logical),
        Kind::Unspecified,
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(vec![OutputElement::Memory(e)])
}

/// A picture of size `src` fitted into `dst`: its size and where it goes.
pub fn letterbox(src: (i32, i32), dst: (i32, i32)) -> ((i32, i32), (i32, i32)) {
    if src.0 <= 0 || src.1 <= 0 {
        return ((0, 0), (0, 0));
    }
    let k = (dst.0 as f64 / src.0 as f64).min(dst.1 as f64 / src.1 as f64);
    let w = (src.0 as f64 * k).round() as i32;
    let h = (src.1 as f64 * k).round() as i32;
    ((w, h), ((dst.0 - w) / 2, (dst.1 - h) / 2))
}

#[cfg(test)]
mod tests {
    #[test]
    fn letterboxing() {
        assert_eq!(super::letterbox((2880, 1800), (1920, 1080)), ((1728, 1080), (96, 0)));
        assert_eq!(super::letterbox((1920, 1080), (1920, 1200)), ((1920, 1080), (0, 60)));
    }
}
