//! Optional retained paint follows the same capture authorization and committed
//! surface state as pixels. It never grants access to another output or client.
use crate::{
    scene::{Purpose, SceneItem},
    state::State,
};
use aizome_remote_protocol::server::{
    aizome_remote_paint_manager_v1 as manager, aizome_remote_paint_surface_v1 as paint,
};
use aizome_remote_protocol::{Frame, MAX_FRAME_BYTES};
use manager::AizomeRemotePaintManagerV1 as Manager;
use paint::AizomeRemotePaintSurfaceV1 as Paint;
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};
use smithay::utils::{Physical, Point, Rectangle, Transform};
use smithay::wayland::alpha_modifier::AlphaModifierSurfaceCachedState;
use smithay::wayland::compositor::{
    Cacheable, SurfaceAttributes, TraversalAction, add_pre_commit_hook, with_states, with_surface_tree_downward,
};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::sync::Arc;

#[derive(Debug)]
struct Update {
    serial: u32,
    full: bool,
    width: u32,
    height: u32,
    scale: f32,
    payload: String,
}
/// Smithay applies this together with the surface's buffer, including synchronized
/// subsurfaces and explicit fences. Several commits before dispatch stay ordered.
#[derive(Default, Debug)]
struct Committed {
    updates: Vec<Option<Arc<Update>>>,
}
impl Cacheable for Committed {
    fn commit(&mut self, _: &DisplayHandle) -> Self {
        std::mem::take(self)
    }
    fn merge_into(self, into: &mut Self, _: &DisplayHandle) {
        into.updates.extend(self.updates);
        if into.updates.len() > 64
            || into.updates.iter().flatten().map(|u| u.payload.len()).sum::<usize>() > MAX_FRAME_BYTES * 2
        {
            into.updates.clear();
            into.updates.push(None);
        }
    }
}
struct Producer {
    resource: Paint,
    surface: WlSurface,
    enabled: bool,
}
struct Watch {
    pid: u32,
    capture: u32,
    output: String,
    surface: Option<WlSurface>,
    serial: Option<u32>,
    status: String,
}
#[derive(Default)]
pub struct RemotePaint {
    producers: Vec<Producer>,
    watches: HashMap<u64, Watch>,
}
pub struct SurfaceData(WlSurface);
struct CommitHookInstalled;
pub fn create_global(dh: &DisplayHandle) {
    dh.create_global::<State, Manager, _>(1, ());
}
impl GlobalDispatch<Manager, ()> for State {
    fn bind(_: &mut Self, _: &DisplayHandle, _: &Client, r: New<Manager>, _: &(), init: &mut DataInit<'_, Self>) {
        init.init(r, ());
    }
}
impl Dispatch<Manager, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        object: &Manager,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let manager::Request::GetSurface { id, surface } = request {
            let producers = &mut state.az.remote_paint.producers;
            producers.retain(|p| p.resource.is_alive() && p.surface.is_alive());
            if producers.len() >= 1024 || producers.iter().any(|p| p.surface == surface) {
                object.post_error(1u32, "duplicate or excessive retained paint surfaces");
                return;
            }
            let resource = init.init(id, SurfaceData(surface.clone()));
            // Attaching new pixels without a corresponding paint invalidates replay.
            // Hooks belong to wl_surface, not the optional producer object.
            // Rebinding that object must not accumulate permanent callbacks.
            if with_states(&surface, |s| s.data_map.insert_if_missing(|| CommitHookInstalled)) {
                add_pre_commit_hook::<Self, _>(&surface, |_, _, surface| {
                    with_states(surface, |s| {
                        let changed = s.cached_state.get::<SurfaceAttributes>().pending().buffer.is_some();
                        let mut c = s.cached_state.get::<Committed>();
                        if changed && c.pending().updates.is_empty() {
                            c.pending().updates.push(None);
                        }
                    })
                });
            }
            resource.enabled(0);
            producers.push(Producer { resource, surface, enabled: false });
        }
    }
}
impl Dispatch<Paint, SurfaceData> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &Paint,
        request: paint::Request,
        data: &SurfaceData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let active = state.az.remote_paint.producers.iter().any(|p| p.resource == *resource && p.enabled);
        let update = match request {
            paint::Request::Paint { serial, fd, bytes, width, height, scale, full } if active => {
                let result = (|| -> Result<Update, String> {
                    if bytes as usize > MAX_FRAME_BYTES || bytes == 0 || full > 1 {
                        return Err("paint bounds".into());
                    }
                    let mut file = std::fs::File::from(fd);
                    let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
                    let required = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK;
                    if seals < 0
                        || seals & required != required
                        || file.metadata().map_err(|e| e.to_string())?.len() != u64::from(bytes)
                    {
                        return Err("paint must be a sealed memfd".into());
                    }
                    // The sender's file offset is immaterial; read the complete immutable object.
                    use std::io::{Seek, SeekFrom};
                    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                    let mut payload = String::with_capacity(bytes as usize);
                    file.take(u64::from(bytes)).read_to_string(&mut payload).map_err(|e| e.to_string())?;
                    let f = Frame::parse(payload.as_bytes())?;
                    if (f.serial, f.width, f.height, f.full) != (serial, width, height, full != 0)
                        || (f64::from(f.scale) - scale).abs() > 1.0 / 256.0
                    {
                        return Err("paint metadata mismatch".into());
                    }
                    Ok(Update { serial, width, height, scale: f.scale, full: f.full, payload })
                })();
                result.ok().map(Arc::new)
            }
            paint::Request::Unsupported | paint::Request::Destroy => None,
            _ => return,
        };
        with_states(&data.0, |s| {
            s.cached_state.get::<Committed>().pending().updates = vec![update];
        });
    }
}

pub fn subscribe(
    state: &mut State,
    connection: u64,
    pid: u32,
    capture: u32,
    output: String,
    enabled: bool,
) -> aizome_ipc::Response {
    state.az.remote_paint.watches.remove(&connection);
    if !enabled {
        return aizome_ipc::Response::Ok;
    }
    if super::image_copy_capture::owned_output(&state.az, pid, capture, &output).is_none() {
        return aizome_ipc::Response::error("retained replay requires this process's active output capture");
    }
    state
        .az
        .remote_paint
        .watches
        .insert(connection, Watch { pid, capture, output, surface: None, serial: None, status: String::new() });
    aizome_ipc::Response::Ok
}

/// The very first visible surface must cover the output opaquely, untransformed,
/// at its native scale. Popups, decorations, tiles, overlays, thumbnails and
/// other toolkits automatically select pixels. Items behind it are occluded.
fn candidate(state: &mut State, output: &Output) -> Option<WlSurface> {
    if state.az.lock.is_locked() || output.current_transform() != Transform::Normal {
        return None;
    }
    let size = super::capture::output_size(output)?;
    let scale = output.current_scale().fractional_scale();
    let scene = state.scene(output, Purpose::Capture { cursor: false });
    let SceneItem::Surface { surface, loc, alpha, .. } = scene.first()? else { return None };
    if *alpha != 1.0 {
        return None;
    }
    let mut first = None;
    with_surface_tree_downward(
        surface,
        loc.to_f64(),
        |_, s, loc| {
            let view = s.data_map.get::<RendererSurfaceStateUserData>().and_then(|d| d.lock().unwrap().view());
            match view {
                Some(v) => TraversalAction::DoChildren(*loc + v.offset.to_f64().to_physical(scale)),
                None => TraversalAction::SkipChildren,
            }
        },
        |surface, s, loc| {
            if first.is_some() {
                return;
            }
            let Some(data) = s.data_map.get::<RendererSurfaceStateUserData>() else { return };
            let data = data.lock().unwrap();
            let Some(v) = data.view().filter(|_| data.buffer().is_some()) else { return };
            let origin: Point<f64, Physical> = *loc + v.offset.to_f64().to_physical(scale);
            let full = Rectangle::from_size(v.dst);
            let opaque = data.opaque_regions().is_some_and(|r| r.iter().any(|r| r.contains_rect(full)));
            let alpha =
                s.cached_state.get::<AlphaModifierSurfaceCachedState>().current().multiplier_f32().unwrap_or(1.0);
            let native = origin == (0.0, 0.0).into()
                && v.dst.to_f64().to_physical(scale).to_i32_round() == size
                && data.buffer_transform() == Transform::Normal
                && data.buffer_size().is_some_and(|b| v.src == Rectangle::from_size(b).to_f64())
                && opaque
                && alpha == 1.0;
            first = Some(native.then(|| surface.clone()));
        },
        |_, _, _| true,
    );
    first.flatten().filter(|s| state.az.remote_paint.producers.iter().any(|p| p.surface == *s && p.resource.is_alive()))
}
fn status(state: &mut State, id: u64, watch: &mut Watch, reason: &str) {
    if watch.status != reason {
        state.az.ipc.emit_to(
            id,
            &aizome_ipc::Event::RemoteScene { capture: watch.capture, payload: None, reason: Some(reason.into()) },
        );
        watch.status = reason.into();
    }
}
pub fn flush(state: &mut State) {
    let mut watches = std::mem::take(&mut state.az.remote_paint.watches);
    let mut desired = HashSet::new();
    let mut refresh = HashSet::new();
    watches.retain(|id, w| {
        if !state.az.ipc.is_subscribed(*id) {
            return false;
        }
        let Some(output) = super::image_copy_capture::owned_output(&state.az, w.pid, w.capture, &w.output) else {
            status(state, *id, w, "capture-stopped");
            return false;
        };
        let next = candidate(state, &output);
        if next != w.surface {
            w.surface = next.clone();
            w.serial = None;
            if let Some(s) = &next {
                refresh.insert(s.id());
            }
            status(state, *id, w, "waiting-for-full-frame");
        }
        if let Some(surface) = next {
            desired.insert(surface.id());
        } else {
            w.serial = None;
            status(state, *id, w, "scene-requires-pixels");
        }
        true
    });
    let mut updates = HashMap::new();
    state.az.remote_paint.producers.retain(|p| p.resource.is_alive() && p.surface.is_alive());
    for p in &mut state.az.remote_paint.producers {
        let wanted = desired.contains(&p.surface.id());
        let request = wanted && (!p.enabled || refresh.contains(&p.surface.id()));
        if request || wanted != p.enabled {
            p.resource.enabled(u32::from(wanted));
            p.enabled = wanted;
        }
        let mut pending =
            with_states(&p.surface, |s| std::mem::take(&mut s.cached_state.get::<Committed>().current().updates));
        if request {
            pending.clear();
            refresh.insert(p.surface.id());
        }
        updates.insert(p.surface.id(), pending);
    }
    for (id, w) in &mut watches {
        let Some(surface) = w.surface.clone() else { continue };
        if refresh.contains(&surface.id()) {
            w.serial = None;
            status(state, *id, w, "waiting-for-full-frame");
        }
        let Some(paints) = updates.get(&surface.id()) else { continue };
        for update in paints {
            let Some(u) = update else {
                w.serial = None;
                status(state, *id, w, "paint-requires-pixels");
                continue;
            };
            let output = state.az.outputs.iter().find(|o| o.name() == w.output);
            let geometry = output.is_some_and(|o| {
                super::capture::output_size(o).is_some_and(|s| (s.w as u32, s.h as u32) == (u.width, u.height))
                    && (o.current_scale().fractional_scale() - f64::from(u.scale)).abs() < 0.001
            });
            if !geometry || (!u.full && w.serial.map(|n| n.wrapping_add(1)) != Some(u.serial)) {
                w.serial = None;
                status(state, *id, w, "paint-state-mismatch");
                continue;
            }
            state.az.ipc.emit_to(
                *id,
                &aizome_ipc::Event::RemoteScene { capture: w.capture, payload: Some(u.payload.clone()), reason: None },
            );
            w.serial = Some(u.serial);
            w.status.clear();
        }
    }
    state.az.remote_paint.watches = watches;
}
