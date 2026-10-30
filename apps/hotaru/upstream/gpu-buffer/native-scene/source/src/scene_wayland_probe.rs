//! Private complete-scene DMA-BUF presentation diagnostic; no desktop discovery.
mod egl;
mod scene;
use hotaru_gpu_transport_prototype::{
    Committed, EventOwner, Import, Limits, WindowId, spawn_with_wake,
};
use sctk::{
    compositor::{CompositorHandler, CompositorState},
    dmabuf::{DmabufFeedback, DmabufHandler, DmabufState},
    output::{OutputHandler, OutputState},
    reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource},
    registry::{ProvidesRegistryState, RegistryState},
    shell::{
        WaylandSurface,
        wlr_layer::{
            KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use std::{
    io::{self, BufRead},
    net::Shutdown,
    os::fd::AsFd,
    os::unix::net::UnixStream,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_buffer, wl_output, wl_surface},
};
use wayland_protocols::wp::{
    linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1 as params, zwp_linux_dmabuf_feedback_v1 as feedback,
    },
    presentation_time::client::{wp_presentation, wp_presentation_feedback},
};

struct State {
    registry: RegistryState,
    output: OutputState,
    dmabuf: DmabufState,
    layer: LayerSurface,
    owner: EventOwner<scene::Scene>,
    window: WindowId,
    extent: (u32, u32),
    configured: bool,
    pairs: Vec<(u32, u64)>,
    feedback_ready: bool,
    importing: Option<(params::ZwpLinuxBufferParamsV1, Import<scene::Scene>)>,
    buffer: Option<wl_buffer::WlBuffer>,
    lease: Option<Committed>,
    frame: bool,
    detached: bool,
    released: bool,
    error: Option<String>,
    gpu_ready: bool,
    presentation: Option<wp_presentation::WpPresentation>,
    clock: Option<u32>,
    presented: bool,
}
impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    sctk::registry_handlers![OutputState];
}
impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        n: i32,
    ) {
        if n != 1 {
            self.error = Some("unexpected scale".into());
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        t: wl_output::Transform,
    ) {
        if t != wl_output::Transform::Normal {
            self.error = Some("unexpected transform".into());
        }
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        time: u32,
    ) {
        self.frame = true;
        println!(
            "{}",
            serde_json::json!({"event":"rust_surface_frame","callback_ms_unspecified_epoch":time})
        );
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.error = Some("layer closed".into());
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        c: LayerSurfaceConfigure,
        _: u32,
    ) {
        if c.new_size != self.extent {
            self.error = Some(format!("unexpected configure {:?}", c.new_size));
        }
        self.configured = true;
    }
}
impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf
    }
    fn dmabuf_feedback(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &feedback::ZwpLinuxDmabufFeedbackV1,
        f: DmabufFeedback,
    ) {
        let table = f.format_table();
        if table.len() > 4096 || f.tranches().len() > 64 {
            self.error = Some("feedback cap".into());
            return;
        }
        // A format-table entry is not itself advertised for this surface.
        // Only validated tranche references authorize import, regardless of flags.
        let table: Vec<_> = table.iter().map(|p| (p.format, p.modifier)).collect();
        self.pairs =
            match advertised_pairs(&table, f.tranches().iter().map(|t| t.formats.as_slice())) {
                Ok(pairs) => pairs,
                Err(error) => {
                    self.error = Some(error.into());
                    return;
                }
            };
        let tranches: Vec<_> = f.tranches().iter().map(|t|
            serde_json::json!({"device":t.device,"flags":format!("{:?}",t.flags),"format_indices":t.formats})
        ).collect();
        self.feedback_ready = true;
        println!(
            "{}",
            serde_json::json!({"event":"rust_dmabuf_feedback","main_device":f.main_device(),"pairs":self.pairs.len(),"tranches":tranches})
        );
    }
    fn created(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        p: &params::ZwpLinuxBufferParamsV1,
        b: wl_buffer::WlBuffer,
    ) {
        let Some((pending, import)) = self.importing.take() else {
            b.destroy();
            self.error = Some("unknown created".into());
            return;
        };
        if pending.id() != p.id() {
            b.destroy();
            self.error = Some("stale params identity".into());
            return;
        }
        p.destroy();
        match self.owner.commit(import) {
            Ok(lease) => {
                self.lease = Some(lease);
                self.layer.wl_surface().attach(Some(&b), 0, 0);
                self.layer.wl_surface().damage_buffer(
                    0,
                    0,
                    self.extent.0 as i32,
                    self.extent.1 as i32,
                );
                self.layer
                    .wl_surface()
                    .frame(qh, self.layer.wl_surface().clone());
                if let Some(p) = &self.presentation {
                    p.feedback(self.layer.wl_surface(), qh, ());
                }
                self.buffer = Some(b);
                self.layer.commit();
                println!(
                    "{}",
                    serde_json::json!({"event":"rust_buffer_committed","async_created":true,"flags":0})
                );
            }
            Err(e) => {
                b.destroy();
                self.error = Some(format!("commit: {e:?}"));
            }
        }
    }
    fn failed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        p: &params::ZwpLinuxBufferParamsV1,
    ) {
        p.destroy();
        self.importing.take();
        self.error = Some("async import failed".into());
    }
    fn released(&mut self, _: &Connection, _: &QueueHandle<Self>, b: &wl_buffer::WlBuffer) {
        if self.buffer.as_ref().map(Proxy::id) != Some(b.id()) {
            self.error = Some("unknown release".into());
            return;
        }
        if !self.detached {
            self.error = Some("diagnostic requires retained buffer until detach".into());
        }
        if let Some(lease) = self.lease.take() {
            self.owner.released(lease);
        }
        self.released = true;
        println!(
            "{}",
            serde_json::json!({"event":"rust_buffer_release","after_detach":self.detached})
        );
    }
}
impl Dispatch<wp_presentation::WpPresentation, ()> for State {
    fn event(
        s: &mut Self,
        _: &wp_presentation::WpPresentation,
        e: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation::Event::ClockId { clk_id } = e {
            s.clock = Some(clk_id);
        }
    }
}
impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, ()> for State {
    fn event(
        s: &mut Self,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        e: wp_presentation_feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                seq_hi,
                seq_lo,
                flags,
            } => {
                s.presented = true;
                println!(
                    "{}",
                    serde_json::json!({"event":"rust_presentation","clock_id":s.clock,"seconds_hi":tv_sec_hi,"seconds_lo":tv_sec_lo,"nanoseconds":tv_nsec,"refresh":refresh,"sequence_hi":seq_hi,"sequence_lo":seq_lo,"flags":format!("{:?}",flags),"physical_scanout_claim":false})
                );
            }
            wp_presentation_feedback::Event::Discarded => {
                s.error = Some("presentation discarded".into());
            }
            _ => {}
        }
    }
}
sctk::delegate_registry!(State);
sctk::delegate_output!(State);
sctk::delegate_compositor!(State);
sctk::delegate_layer!(State);
sctk::delegate_dmabuf!(State);

fn main() -> Result<(), String> {
    let library = std::env::args()
        .nth(1)
        .ok_or("explicit EGL library path required")?;
    let path = std::env::args()
        .nth(2)
        .ok_or("explicit scene path required")?;
    let source = Arc::new(scene::Scene::load(std::path::Path::new(&path))?);
    let extent = (source.width(), source.height());
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    // A duplicated fd permits explicit safe shutdown before releasing all leases
    // on an exceptional disconnect; no host/current-desktop connection is opened.
    let socket = UnixStream::from(
        conn.backend()
            .poll_fd()
            .try_clone_to_owned()
            .map_err(|e| e.to_string())?,
    );
    let (globals, queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let mut event_loop = EventLoop::<State>::try_new().map_err(|e| e.to_string())?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| e.to_string())?;
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let shell = LayerShell::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let layer = shell.create_layer_surface(
        &qh,
        compositor.create_surface(&qh),
        Layer::Overlay,
        Some("hotaru-rust-gpu-probe"),
        None,
    );
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(extent.0, extent.1);
    layer.commit();
    let dmabuf = DmabufState::new(&globals, &qh);
    let feedback = if dmabuf.version().is_some_and(|v| v >= 4) {
        Some(
            dmabuf
                .get_surface_feedback(layer.wl_surface(), &qh)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let (ping, ping_source) =
        sctk::reexports::calloop::ping::make_ping().map_err(|e| e.to_string())?;
    event_loop
        .handle()
        .insert_source(ping_source, |_, _, state: &mut State| {
            state.gpu_ready = true
        })
        .map_err(|e| e.to_string())?;
    let gpu_ping = ping.clone();
    let (done_tx, done) = mpsc::channel();
    let mut owner = spawn_with_wake(
        Limits {
            bytes: 128 << 20,
            ..Limits::default()
        },
        move || egl::SceneDriver::new(&library, "/dev/dri/renderD128", Some(done_tx)),
        move || gpu_ping.ping(),
    )
    .map_err(|e| e.to_string())?;
    let window = owner.open().map_err(|e| format!("open: {e:?}"))?;
    owner
        .submit(
            window,
            source.clone(),
            source.bytes(),
            source.width() as usize * source.height() as usize * 4 + 65536,
        )
        .map_err(|e| format!("submit: {e:?}"))?;
    let presentation = globals
        .bind::<wp_presentation::WpPresentation, _, _>(&qh, 1..=1, ())
        .ok();
    let mut state = State {
        registry: RegistryState::new(&globals),
        output: OutputState::new(&globals, &qh),
        dmabuf,
        layer,
        owner,
        window,
        extent,
        configured: false,
        pairs: Vec::new(),
        feedback_ready: false,
        importing: None,
        buffer: None,
        lease: None,
        frame: false,
        detached: false,
        released: false,
        error: None,
        presentation,
        clock: None,
        presented: false,
        gpu_ready: false,
    };
    let (control_tx, control_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            if let Ok(line) = line {
                let _ = control_tx.try_send(line);
                ping.ping();
            } else {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let result = (|| {
        while !state.released {
            if Instant::now() >= deadline {
                return Err("Wayland phase deadline".into());
            }
            event_loop
                .dispatch(Duration::from_millis(100), &mut state)
                .map_err(|e| e.to_string())?;
            if let Some(error) = state.error.take() {
                return Err(error);
            }
            if state.owner.stats().failed {
                return Err("GPU worker failed".into());
            }
            if state.configured && state.importing.is_none() && state.buffer.is_none() {
                if state.dmabuf.version() == Some(3) && !state.dmabuf.modifiers().is_empty() {
                    if state.dmabuf.modifiers().len() > 4096 {
                        return Err("modifier cap".into());
                    }
                    state.pairs = state
                        .dmabuf
                        .modifiers()
                        .iter()
                        .map(|p| (p.format, p.modifier))
                        .collect();
                    state.feedback_ready = true;
                }
                if state.feedback_ready && state.gpu_ready {
                    state.gpu_ready = false;
                    if let Some(import) = state.owner.poll_ready() {
                        let d = import.descriptor();
                        if !state.pairs.contains(&(d.fourcc, d.modifier)) {
                            return Err("export pair not advertised".into());
                        }
                        let p = state.dmabuf.create_params(&qh).map_err(|e| e.to_string())?;
                        p.add(d.fd.as_fd(), 0, d.offset, d.stride, d.modifier);
                        let p = p.create(
                            d.width as i32,
                            d.height as i32,
                            d.fourcc,
                            params::Flags::empty(),
                        );
                        state.importing = Some((p, import));
                        println!(
                            "{}",
                            serde_json::json!({"event":"rust_import_pending","exact_pair_advertised":true})
                        );
                    }
                }
            }
            if let Ok(command) = control_rx.try_recv() {
                if command != "detach" || !state.frame || state.detached {
                    return Err("invalid fixture control".into());
                }
                state.detached = true;
                state.layer.wl_surface().attach(None, 0, 0);
                state.layer.commit();
                state.owner.close(state.window);
            }
        }
        if !state.presented {
            return Err("missing presentation feedback".into());
        }
        Ok(())
    })();
    if let Some(b) = state.buffer.take() {
        b.destroy();
    }
    if let Some((p, _)) = state.importing.take() {
        p.destroy();
    }
    if let Some(feedback) = feedback {
        feedback.destroy();
    }
    // Shutdown the actual owned Wayland socket before globally retiring resources.
    socket.shutdown(Shutdown::Both).map_err(|e| e.to_string())?;
    state.owner.connection_lost();
    // Diagnostic-only bounded completion wait. No production event path may wait.
    let clean = done
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    if !clean {
        return Err("EGL cleanup failed".into());
    }
    result?;
    println!(
        "{}",
        serde_json::json!({"event":"rust_wayland_probe_complete","source_immutable_until_release":true})
    );
    Ok(())
}

fn advertised_pairs<'a>(
    table: &[(u32, u64)],
    tranches: impl IntoIterator<Item = &'a [u16]>,
) -> Result<Vec<(u32, u64)>, &'static str> {
    if table.len() > 4096 {
        return Err("format table cap");
    }
    let mut referenced = vec![false; table.len()];
    let mut entries = 0;
    for (count, tranche) in tranches.into_iter().enumerate() {
        if count >= 64 || tranche.len() > 4096 {
            return Err("tranche cap");
        }
        entries += tranche.len();
        if entries > 8192 {
            return Err("total feedback work cap");
        }
        for &index in tranche {
            let slot = referenced
                .get_mut(usize::from(index))
                .ok_or("invalid tranche index")?;
            *slot = true;
        }
    }
    let mut result: Vec<_> = table
        .iter()
        .zip(referenced)
        .filter_map(|(pair, used)| used.then_some(*pair))
        .collect();
    result.sort_unstable();
    result.dedup();
    Ok(result)
}

#[cfg(test)]
mod feedback_tests {
    use super::*;
    #[test]
    fn only_tranche_references_advertise_supported_pairs() {
        let rgba = (u32::from_le_bytes(*b"AB24"), 0);
        let other = (u32::from_le_bytes(*b"XR24"), 0);
        let table = [rgba, other, other];
        let pairs = advertised_pairs(&table, [&[1u16, 2, 1][..]]).unwrap();
        assert_eq!(pairs, vec![other]);
        assert!(
            !pairs.contains(&rgba),
            "unreferenced AB24 must not authorize import"
        );
        assert_eq!(
            advertised_pairs(&table, [&[0u16][..], &[0u16, 1][..]])
                .unwrap()
                .len(),
            2
        );
        assert!(advertised_pairs(&table, [&[3u16][..]]).is_err());
        assert!(advertised_pairs(&table, [&[][..]]).unwrap().is_empty());
    }
    #[test]
    fn feedback_work_is_bounded_before_deduplication() {
        let indices = vec![0u16; 4096];
        assert!(
            advertised_pairs(
                &[(1, 0)],
                [indices.as_slice(), indices.as_slice(), &[0][..]]
            )
            .is_err()
        );
        assert!(advertised_pairs(&vec![(1, 0); 4097], [&[][..]]).is_err());
        assert!(advertised_pairs(&[(1, 0)], std::iter::repeat_n(&[][..], 65)).is_err());
    }
}
