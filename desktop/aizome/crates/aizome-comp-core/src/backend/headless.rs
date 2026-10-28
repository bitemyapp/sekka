//! Outputs that exist only in memory: frames render offscreen with GLES
//! (EGL on a DRM render node through GBM) or pixman, paced by a 60 Hz
//! timer that runs only while something is damaged.

use std::collections::HashMap;
use std::time::Instant;

use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::drm::{DrmDeviceFd, DrmNode, NodeType};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::{GlesRenderbuffer, GlesRenderer};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ImportDma, ImportMemWl, Offscreen};
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::RegistrationToken;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::utils::{DeviceFd, Size};
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;
use tracing::{info, warn};

use super::{Capture, capture_with, post_frame, render_frame, scene_for};
use crate::cli::{HeadlessOutput, RendererKind};
use crate::state::{Aizome, State};

pub enum HeadlessRenderer {
    Gles(Box<GlesRenderer>),
    Pixman(PixmanRenderer),
}

enum Target {
    Gles(GlesRenderbuffer),
    Pixman(smithay::reexports::pixman::Image<'static, 'static>),
}

struct HeadlessOutputState {
    target: Target,
    tracker: OutputDamageTracker,
    /// What changed under blurs since the last frame ([`crate::blur`]).
    blur_probe: crate::blur::Probe,
    rendered_once: bool,
    timer: Option<RegistrationToken>,
}

pub struct Headless {
    pub renderer: HeadlessRenderer,
    /// Thumbnails drawn once and kept (pixman).
    thumbs: crate::thumbs::ThumbCache,
    outputs: HashMap<Output, HeadlessOutputState>,
    render_node: Option<DrmNode>,
    configs: Vec<HeadlessOutput>,
}

impl Headless {
    /// Sets up the renderer: GLES on the first usable render node unless
    /// pixman is asked for, and pixman when GLES fails.
    pub fn new(renderer: Option<RendererKind>, outputs: Vec<HeadlessOutput>) -> Headless {
        let (renderer, render_node) = match renderer {
            Some(RendererKind::Pixman) => (HeadlessRenderer::Pixman(PixmanRenderer::new().expect("pixman")), None),
            _ => match gles_on_render_node() {
                Ok((r, node)) => (HeadlessRenderer::Gles(Box::new(r)), Some(node)),
                Err(e) => {
                    warn!("GLES unavailable ({e}); rendering with pixman");
                    (HeadlessRenderer::Pixman(PixmanRenderer::new().expect("pixman")), None)
                }
            },
        };
        Headless {
            renderer,
            thumbs: crate::thumbs::ThumbCache::new(Fourcc::Abgr8888),
            outputs: HashMap::new(),
            render_node,
            configs: outputs,
        }
    }

    pub fn renderer_name(&self) -> &'static str {
        match self.renderer {
            HeadlessRenderer::Gles(_) => "gles",
            HeadlessRenderer::Pixman(_) => "pixman",
        }
    }

    /// Creates the outputs and the dmabuf global (GLES only).
    pub fn init(state: &mut State) {
        let crate::backend::Backend::Headless(h) = &mut state.backend else { return };
        let configs = h.configs.clone();
        // Every wl_shm format the renderer reads (ABGR and XBGR besides the
        // two every compositor has).
        let formats: Vec<_> = match &h.renderer {
            HeadlessRenderer::Gles(r) => r.shm_formats().collect(),
            HeadlessRenderer::Pixman(r) => r.shm_formats().collect(),
        };
        state.az.shm.update_formats(formats);
        if let (HeadlessRenderer::Gles(r), Some(node)) = (&mut h.renderer, h.render_node) {
            // A render-node-only compositor has real GPU fences too. This
            // also lets protocol tests exercise NVIDIA without taking a VT.
            if let Some(path) = node.dev_path()
                && let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(path)
            {
                crate::explicit_sync::init(
                    &mut state.az,
                    DrmDeviceFd::new(DeviceFd::from(std::os::fd::OwnedFd::from(file))),
                );
            }
            let formats = r.dmabuf_formats();
            match DmabufFeedbackBuilder::new(node.dev_id(), formats).build() {
                Ok(feedback) => {
                    let global = state.az.dmabuf.create_global_with_default_feedback::<State>(&state.az.dh, &feedback);
                    state.az.dmabuf_global = Some(global);
                }
                Err(e) => warn!("no dmabuf feedback: {e}"),
            }
        }
        let mut x = 0;
        for (i, c) in configs.iter().enumerate() {
            let name = c.name.clone().unwrap_or_else(|| format!("HEADLESS-{}", i + 1));
            let output = Output::new(
                name,
                PhysicalProperties {
                    size: ((c.width as f64 / 5.0) as i32, (c.height as f64 / 5.0) as i32).into(),
                    subpixel: Subpixel::Unknown,
                    make: "Aizome".into(),
                    model: "Headless".into(),
                },
            );
            let mode = Mode { size: (c.width, c.height).into(), refresh: 60_000 };
            output.add_mode(mode);
            output.set_preferred(mode);
            let pos = (x, 0);
            crate::outputs::add_output(&mut state.az, &output, mode, c.scale, pos, "");
            x += (c.width as f64 / c.scale).round() as i32;
            let h = state.backend.headless_mut().unwrap();
            h.add_target(&output);
        }
    }

    fn add_target(&mut self, output: &Output) {
        let Some(mode) = output.current_mode() else { return };
        let size = Size::from((mode.size.w, mode.size.h));
        let target = match &mut self.renderer {
            HeadlessRenderer::Gles(r) => Target::Gles(
                Offscreen::<GlesRenderbuffer>::create_buffer(r.as_mut(), Fourcc::Abgr8888, size)
                    .expect("an offscreen renderbuffer"),
            ),
            HeadlessRenderer::Pixman(r) => {
                Target::Pixman(r.create_buffer(Fourcc::Abgr8888, size).expect("an offscreen image"))
            }
        };
        self.outputs.insert(
            output.clone(),
            HeadlessOutputState {
                target,
                tracker: OutputDamageTracker::from_output(output),
                blur_probe: Default::default(),
                rendered_once: false,
                timer: None,
            },
        );
    }

    /// Mode, scale, position, transform, and turning outputs off and on
    /// (an output off has no buffer and no place in the layout).
    pub fn configure_output(&mut self, az: &mut Aizome, change: &crate::outputs::OutputChange) -> Result<(), String> {
        let output = az.known_output(&change.name).ok_or_else(|| format!("no output {}", change.name))?;
        let off = az.disabled_outputs.contains(&output);
        if change.enabled == Some(false) {
            if !off {
                crate::outputs::disable(az, &output);
                self.outputs.remove(&output);
            }
            return Ok(());
        }
        if let Some((w, h, _)) = change.mode {
            let mode = Mode { size: (w, h).into(), refresh: 60_000 };
            output.add_mode(mode);
            output.change_current_state(Some(mode), None, None, None);
        }
        if off {
            if change.enabled != Some(true) {
                return Ok(());
            }
            let refresh = output.current_mode().map(|m| m.refresh).unwrap_or(60_000);
            az.out.insert(output.clone(), crate::state::OutputState::new(refresh));
            let pos = crate::outputs::free_position(az, change.position);
            output.change_current_state(None, None, None, Some(pos.into()));
            crate::outputs::attach(az, &output);
        }
        crate::outputs::apply_logical(az, change)?;
        self.outputs.remove(&output);
        self.add_target(&output);
        az.queue_redraw(&output);
        Ok(())
    }

    /// Imports a surface tree when the renderer copies wl_shm pixels (GLES).
    pub fn import_shm_copies(
        &mut self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> bool {
        match &mut self.renderer {
            HeadlessRenderer::Gles(r) => {
                smithay::backend::renderer::utils::import_surface_tree(r.as_mut(), surface).is_ok()
            }
            HeadlessRenderer::Pixman(_) => false,
        }
    }

    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> bool {
        match &mut self.renderer {
            HeadlessRenderer::Gles(r) => r.import_dmabuf(dmabuf, None).is_ok(),
            HeadlessRenderer::Pixman(r) => r.import_dmabuf(dmabuf, None).is_ok(),
        }
    }

    /// Whether an output's next frame is due: paced to the refresh rate
    /// (a timer brings the loop back when it is).
    pub fn frame_due(&mut self, az: &mut Aizome, output: &Output) -> bool {
        let Some(hs) = self.outputs.get_mut(output) else { return false };
        let Some(os) = az.out.get(output) else { return false };
        if !os.powered {
            if let Some(o) = az.out.get_mut(output) {
                o.redraw_queued = false;
            }
            return false;
        }
        if let Some(last) = os.last_frame {
            let due = last + os.refresh;
            if Instant::now() < due {
                if hs.timer.is_none() {
                    let o = output.clone();
                    let token = az
                        .lh
                        .insert_source(Timer::from_deadline(due), move |_, _, state: &mut State| {
                            if let Some(h) = state.backend.headless_mut()
                                && let Some(hs) = h.outputs.get_mut(&o)
                            {
                                hs.timer = None;
                            }
                            TimeoutAction::Drop
                        })
                        .ok();
                    hs.timer = token;
                }
                return false;
            }
        }
        true
    }

    pub fn redraw(
        &mut self,
        az: &mut Aizome,
        output: &Output,
        mut scene: crate::scene::Scene,
        clear: [f32; 4],
        started: Instant,
    ) {
        let Some(hs) = self.outputs.get_mut(output) else { return };
        if let Some(o) = az.out.get_mut(output) {
            o.redraw_queued = false;
        }
        let age = if hs.rendered_once { 1 } else { 0 };
        let result = match (&mut self.renderer, &mut hs.target) {
            (HeadlessRenderer::Gles(r), Target::Gles(rb)) => {
                let r = r.as_mut();
                let (mut elements, clear) = scene_for::<_, GlesRenderbuffer>(r, az, output, &scene, clear);
                crate::blur::prepare(&mut hs.blur_probe, output, &mut elements, None);
                match r.bind(rb) {
                    Ok(mut fb) => render_frame(r, &mut fb, &mut hs.tracker, age, &elements, clear, started, true),
                    Err(e) => Err(format!("bind: {e:?}")),
                }
            }
            (HeadlessRenderer::Pixman(r), Target::Pixman(img)) => {
                self.thumbs.bake(r, &mut scene, output.current_scale().fractional_scale());
                let (elements, clear) =
                    scene_for::<_, smithay::reexports::pixman::Image<'static, 'static>>(r, az, output, &scene, clear);
                match r.bind(img) {
                    Ok(mut fb) => render_frame(r, &mut fb, &mut hs.tracker, age, &elements, clear, started, false),
                    Err(e) => Err(format!("bind: {e:?}")),
                }
            }
            _ => Err("renderer and target disagree".into()),
        };
        match result {
            Ok(frame) => {
                hs.rendered_once = true;
                post_frame(az, output, &frame, true);
            }
            Err(e) => warn!("rendering {} failed: {e}", output.name()),
        }
    }

    pub fn screencopy(
        &mut self,
        az: &mut Aizome,
        job: &mut crate::protocols::capture::Job,
    ) -> Result<crate::protocols::capture::Outcome, String> {
        use crate::protocols::capture::{copy_gpu, copy_pixman};
        match &mut self.renderer {
            HeadlessRenderer::Gles(r) => copy_gpu::<GlesRenderer, GlesRenderbuffer>(r.as_mut(), az, job),
            HeadlessRenderer::Pixman(r) => copy_pixman(r, az, job),
        }
    }

    pub fn screencopy_dmabuf(&mut self) -> Option<crate::protocols::capture::DmabufTarget> {
        match (&self.renderer, self.render_node) {
            // Only with a render node clients can allocate on.
            (HeadlessRenderer::Gles(r), Some(node)) => {
                crate::protocols::capture::dmabuf_target(node.dev_id(), r.as_ref())
            }
            _ => None,
        }
    }

    pub fn render_to_memory(
        &mut self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        size: smithay::utils::Size<i32, smithay::utils::Physical>,
        scale: f64,
    ) -> Result<Capture, String> {
        match &mut self.renderer {
            HeadlessRenderer::Gles(r) => {
                super::memory_with::<GlesRenderer, GlesRenderbuffer>(r.as_mut(), surface, size, scale)
            }
            HeadlessRenderer::Pixman(r) => super::memory_with::<
                PixmanRenderer,
                smithay::reexports::pixman::Image<'static, 'static>,
            >(r, surface, size, scale),
        }
    }

    pub fn capture(
        &mut self,
        az: &Aizome,
        output: &Output,
        scene: &crate::scene::Scene,
        clear: [f32; 4],
    ) -> Result<Capture, String> {
        match &mut self.renderer {
            HeadlessRenderer::Gles(r) => {
                capture_with::<GlesRenderer, GlesRenderbuffer>(r.as_mut(), az, output, scene, clear)
            }
            HeadlessRenderer::Pixman(r) => {
                // As the screen shows it.
                let mut scene = scene.clone();
                self.thumbs.bake(r, &mut scene, output.current_scale().fractional_scale());
                capture_with::<PixmanRenderer, smithay::reexports::pixman::Image<'static, 'static>>(
                    r, az, output, &scene, clear,
                )
            }
        }
    }
}

/// GLES on the first render node that takes an EGL context.
fn gles_on_render_node() -> Result<(GlesRenderer, DrmNode), String> {
    let mut nodes: Vec<std::path::PathBuf> = std::fs::read_dir("/dev/dri")
        .map_err(|e| format!("/dev/dri: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("renderD")))
        .collect();
    nodes.sort();
    if let Ok(p) = std::env::var("AIZOME_RENDER_NODE") {
        nodes.insert(0, p.into());
    }
    let mut last = String::from("no render nodes");
    for path in nodes {
        match open_gles(&path) {
            Ok(r) => {
                info!("rendering with GLES on {}", path.display());
                return Ok(r);
            }
            Err(e) => last = format!("{}: {e}", path.display()),
        }
    }
    Err(last)
}

fn open_gles(path: &std::path::Path) -> Result<(GlesRenderer, DrmNode), String> {
    use std::os::fd::OwnedFd;
    let file = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| e.to_string())?;
    let node = DrmNode::from_path(path).map_err(|e| e.to_string())?;
    let node = match node.ty() {
        NodeType::Render => node,
        _ => node.node_with_type(NodeType::Render).and_then(|n| n.ok()).unwrap_or(node),
    };
    let fd = DrmDeviceFd::new(DeviceFd::from(OwnedFd::from(file)));
    let gbm = GbmDevice::new(fd).map_err(|e| e.to_string())?;
    // Safety: the GBM device outlives the display (it's moved into it).
    let display = unsafe { EGLDisplay::new(gbm) }.map_err(|e| e.to_string())?;
    let context = EGLContext::new(&display).map_err(|e| e.to_string())?;
    // Safety: the context is current on this thread only.
    let renderer = unsafe { GlesRenderer::new(context) }.map_err(|e| e.to_string())?;
    Ok((renderer, node))
}
