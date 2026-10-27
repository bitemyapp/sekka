//! A window inside another session, for development (never on the live
//! session of whoever runs the tests). Modelled on anvil's winit backend
//! (smithay, MIT licence: Copyright (c) 2017 Victor Berger and Victor
//! Brekenfeld).

use std::time::Instant;

use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::egl::EGLDevice;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::{GlesRenderbuffer, GlesRenderer};
use smithay::backend::renderer::{ImportDma, ImportMemWl};
use smithay::backend::winit::{self, WinitEvent, WinitGraphicsBackend};
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::EventLoop;
use smithay::utils::Transform;
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;
use tracing::warn;

use super::{Capture, capture_with, post_frame, render_frame};
use crate::state::{Aizome, State};

pub struct WinitBackend {
    backend: WinitGraphicsBackend<GlesRenderer>,
    tracker: Option<OutputDamageTracker>,
    /// What changed under blurs since the last frame ([`crate::blur`]).
    blur_probe: crate::blur::Probe,
    output: Option<Output>,
    full_redraw: u8,
}

impl WinitBackend {
    /// Opens the window and puts its events on the loop.
    pub fn new(event_loop: &EventLoop<'static, State>) -> Result<WinitBackend, String> {
        let (backend, winit) = winit::init::<GlesRenderer>().map_err(|e| e.to_string())?;
        event_loop
            .handle()
            .insert_source(winit, |event, _, state: &mut State| match event {
                WinitEvent::Resized { size, .. } => {
                    if let crate::backend::Backend::Winit(w) = &mut state.backend
                        && let Some(o) = &w.output
                    {
                        let mode = Mode { size, refresh: 60_000 };
                        o.change_current_state(Some(mode), None, None, None);
                        o.set_preferred(mode);
                        w.full_redraw = 4;
                    }
                    smithay::desktop::layer_map_for_output(&state.az.outputs[0]).arrange();
                    crate::outputs::outputs_changed(&mut state.az);
                    state.az.queue_redraw_all();
                }
                WinitEvent::Input(event) => state.process_input(event),
                WinitEvent::Redraw => state.az.queue_redraw_all(),
                WinitEvent::CloseRequested => state.quit(),
                WinitEvent::Focus(_) => {}
            })
            .map_err(|e| e.to_string())?;
        Ok(WinitBackend { backend, tracker: None, blur_probe: Default::default(), output: None, full_redraw: 4 })
    }

    /// Adds the output and the dmabuf global.
    pub fn init(state: &mut State) {
        let crate::backend::Backend::Winit(w) = &mut state.backend else { return };
        let size = w.backend.window_size();
        let mode = Mode { size, refresh: 60_000 };
        let output = Output::new(
            "WINIT-1".into(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Aizome".into(),
                model: "Winit".into(),
            },
        );
        output.add_mode(mode);
        output.set_preferred(mode);
        let node = EGLDevice::device_for_display(w.backend.renderer().egl_context().display())
            .and_then(|d| d.try_get_render_node());
        let formats = w.backend.renderer().dmabuf_formats();
        match node {
            Ok(Some(node)) => {
                if let Ok(feedback) = DmabufFeedbackBuilder::new(node.dev_id(), formats).build() {
                    let g = state.az.dmabuf.create_global_with_default_feedback::<State>(&state.az.dh, &feedback);
                    state.az.dmabuf_global = Some(g);
                }
            }
            _ => {
                let g = state.az.dmabuf.create_global::<State>(&state.az.dh, formats);
                state.az.dmabuf_global = Some(g);
            }
        }
        state.az.shm.update_formats(w.backend.renderer().shm_formats());
        w.tracker = Some(OutputDamageTracker::from_output(&output));
        w.output = Some(output.clone());
        let scale = w.backend.window().scale_factor().round().max(1.0);
        crate::outputs::add_output(&mut state.az, &output, mode, scale, (0, 0), "");
        output.change_current_state(None, Some(Transform::Flipped180), None, None);
    }

    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> bool {
        self.backend.renderer().import_dmabuf(dmabuf, None).is_ok()
    }

    /// Imports a surface tree (GLES copies wl_shm pixels).
    pub fn import_shm_copies(
        &mut self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> bool {
        smithay::backend::renderer::utils::import_surface_tree(self.backend.renderer(), surface).is_ok()
    }

    pub fn redraw(
        &mut self,
        az: &mut Aizome,
        output: &Output,
        scene: crate::scene::Scene,
        clear: [f32; 4],
        started: Instant,
    ) {
        if let Some(o) = az.out.get_mut(output) {
            o.redraw_queued = false;
        }
        let Some(tracker) = self.tracker.as_mut() else { return };
        self.full_redraw = self.full_redraw.saturating_sub(1);
        let age = if self.full_redraw > 0 { 0 } else { self.backend.buffer_age().unwrap_or(0) };
        let result = match self.backend.bind() {
            Ok((renderer, mut fb)) => {
                let scale = output.current_scale().fractional_scale();
                let mut elements = crate::render::scene_elements(renderer, scale, &scene);
                crate::blur::prepare(&mut self.blur_probe, output, &mut elements, None);
                render_frame(renderer, &mut fb, tracker, age, &elements, clear, started, false)
            }
            Err(e) => Err(format!("bind: {e}")),
        };
        match result {
            Ok(frame) => {
                if frame.damaged
                    && let Err(e) = self.backend.submit(None)
                {
                    warn!("submit: {e}");
                }
                post_frame(az, output, &frame, true);
            }
            Err(e) => warn!("rendering failed: {e}"),
        }
    }

    pub fn screencopy(
        &mut self,
        az: &mut Aizome,
        job: &mut crate::protocols::capture::Job,
    ) -> Result<crate::protocols::capture::Outcome, String> {
        crate::protocols::capture::copy_gpu::<GlesRenderer, GlesRenderbuffer>(self.backend.renderer(), az, job)
    }

    pub fn screencopy_dmabuf(&mut self) -> Option<crate::protocols::capture::DmabufTarget> {
        let node = EGLDevice::device_for_display(self.backend.renderer().egl_context().display())
            .and_then(|d| d.try_get_render_node())
            .ok()??;
        crate::protocols::capture::dmabuf_target(node.dev_id(), self.backend.renderer())
    }

    pub fn render_to_memory(
        &mut self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        size: smithay::utils::Size<i32, smithay::utils::Physical>,
        scale: f64,
    ) -> Result<Capture, String> {
        super::memory_with::<GlesRenderer, GlesRenderbuffer>(self.backend.renderer(), surface, size, scale)
    }

    pub fn capture(
        &mut self,
        az: &Aizome,
        output: &Output,
        scene: &crate::scene::Scene,
        clear: [f32; 4],
    ) -> Result<Capture, String> {
        capture_with::<GlesRenderer, GlesRenderbuffer>(self.backend.renderer(), az, output, scene, clear)
    }
}
