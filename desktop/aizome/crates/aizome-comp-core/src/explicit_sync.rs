//! linux-drm-syncobj-v1: wait before importing a client's buffer. Smithay
//! retains the release point with its renderer buffer, including scanout,
//! and signals it only when the last reference is retired.

use smithay::backend::drm::DrmDeviceFd;
use std::collections::HashMap;

use smithay::reexports::calloop::{Interest, RegistrationToken};
use smithay::reexports::wayland_server::backend::DisconnectReason;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Resource, Weak};
use smithay::wayland::compositor::{BufferAssignment, CompositorHandler, SurfaceAttributes, add_blocker, with_states};
use smithay::wayland::dmabuf::get_dmabuf;
use smithay::wayland::drm_syncobj::{
    DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState, supports_syncobj_eventfd,
};

use crate::state::{Aizome, State};

#[derive(Default)]
pub struct Waiters {
    next: u64,
    sources: HashMap<u64, (Weak<WlSurface>, RegistrationToken)>,
}

/// A vanished client may never signal its timeline. Drop its eventfd and
/// event-loop registration rather than retaining one for every abandoned frame.
pub fn surface_destroyed(state: &mut State, surface: &WlSurface) {
    let weak = surface.downgrade();
    state.az.syncobj_waiters.sources.retain(|_, (s, token)| {
        if s == &weak {
            state.az.lh.remove(*token);
            false
        } else {
            true
        }
    });
}

/// Offered by default only on NVIDIA's driver, where it has been validated;
/// `[performance] explicit_sync` turns it on or off for any GPU.
pub fn init(az: &mut Aizome, device: DrmDeviceFd) {
    use smithay::reexports::drm::Device as _;
    let driver = device.get_driver().map(|d| d.name().to_string_lossy().into_owned()).unwrap_or_default();
    let choice = az.settings.explicit_sync_choice();
    if !choice.offered_on(&driver) {
        tracing::info!(
            %driver,
            ?choice,
            "explicit synchronization off; [performance] explicit_sync = \"on\" offers linux-drm-syncobj-v1"
        );
    } else if supports_syncobj_eventfd(&device) {
        az.drm_syncobj = Some(DrmSyncobjState::new::<State>(&az.dh, device));
        tracing::info!(%driver, "explicit synchronization available (linux-drm-syncobj-v1)");
    } else {
        tracing::info!(%driver, "DRM device cannot wait on syncobj eventfds; using implicit synchronization");
    }
}

impl DrmSyncobjHandler for State {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.az.drm_syncobj.as_mut()
    }
}

smithay::delegate_drm_syncobj!(State);

pub fn commit_requested(state: &mut State, surface: &WlSurface) {
    let (dmabuf, acquire) = with_states(surface, |data| {
        let dmabuf = data.cached_state.get::<SurfaceAttributes>().pending().buffer.as_ref().and_then(|a| match a {
            BufferAssignment::NewBuffer(b) => get_dmabuf(b).cloned().ok(),
            _ => None,
        });
        let acquire = data.cached_state.get::<DrmSyncobjCachedState>().pending().acquire_point.clone();
        (dmabuf, acquire)
    });
    let Some(dmabuf) = dmabuf else { return };
    let Some(client) = surface.client() else { return };
    if let Some(acquire) = acquire {
        // An explicit acquire point replaces the implicit DMA-BUF fence.
        // NVIDIA need not attach an implicit fence at all; falling back
        // after a failed explicit wait would sample unfinished pixels.
        let result = acquire.generate_blocker().map_err(|e| e.to_string()).and_then(|(blocker, source)| {
            let id = state.az.syncobj_waiters.next;
            state.az.syncobj_waiters.next = id.wrapping_add(1);
            let client = client.clone();
            let token = state
                .az
                .lh
                .insert_source(source, move |_, _, state| {
                    state.az.syncobj_waiters.sources.remove(&id);
                    let dh = state.az.dh.clone();
                    state.client_compositor_state(&client).blocker_cleared(state, &dh);
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            state.az.syncobj_waiters.sources.insert(id, (surface.downgrade(), token));
            add_blocker(surface, blocker);
            Ok(())
        });
        if let Err(error) = result {
            tracing::error!(%error, "cannot wait for client's explicit acquire point; disconnecting client");
            state.az.dh.backend_handle().kill_client(client.id(), DisconnectReason::ConnectionClosed);
        }
        return;
    }
    // Legacy clients still use implicit synchronization.
    if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ) {
        let res = state.az.lh.insert_source(source, move |_, _, state| {
            let dh = state.az.dh.clone();
            state.client_compositor_state(&client).blocker_cleared(state, &dh);
            Ok(())
        });
        if res.is_ok() {
            add_blocker(surface, blocker);
        }
    }
}
