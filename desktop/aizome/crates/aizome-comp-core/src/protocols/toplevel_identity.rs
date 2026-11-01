//! aizome-toplevel-identity-v1, Aizome's own: a client hears the
//! ext-foreign-toplevel-list identifier of each of its toplevels as it
//! maps ([`foreign_toplevel::identifier`](super::foreign_toplevel::identifier)),
//! the name the compositor gives other processes for exactly that window.
//! ❖Return names a Hotaru window to the `aizome-hotaru` it starts by it
//! (`terminal.rs`), and Hotaru finds the window by the same name. The XML
//! is Sukumo's, whose windows ask for it.

use std::collections::HashMap;

use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use crate::WinId;
use crate::state::State;

#[allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#[allow(non_upper_case_globals, non_snake_case, unused_imports, missing_docs, clippy::all)]
mod generated {
    use smithay::reexports::wayland_protocols::xdg::shell::server::*;
    use smithay::reexports::wayland_server;
    use wayland_server::protocol::*;

    pub mod __interfaces {
        use smithay::reexports::wayland_protocols::xdg::shell::server::__interfaces::*;
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../sukumo/crates/sidestep-engine/src/backend/aizome-toplevel-identity-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../sukumo/crates/sidestep-engine/src/backend/aizome-toplevel-identity-v1.xml"
    );
}

use generated::aizome_toplevel_identity_manager_v1::{self as manager, AizomeToplevelIdentityManagerV1 as Manager};
use generated::aizome_toplevel_identity_v1::{self as identity, AizomeToplevelIdentityV1 as Identity};

/// The identity objects clients asked for, by their toplevel's surface.
#[derive(Default)]
pub struct ToplevelIdentities {
    watching: HashMap<WlSurface, Vec<Identity>>,
}

/// Every client may ask: it only ever hears its own windows' names.
pub fn create_global(dh: &DisplayHandle) {
    dh.create_global::<State, Manager, _>(1, ());
}

impl GlobalDispatch<Manager, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<Manager>,
        _: &(),
        init: &mut DataInit<'_, State>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<Manager, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &Manager,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        let manager::Request::GetToplevelIdentity { id, toplevel } = request else { return };
        let identity = init.init(id, ());
        // A toplevel already gone has nothing to be named.
        let Some(surface) = state.az.xdg_shell.get_toplevel(&toplevel).map(|t| t.wl_surface().clone()) else { return };
        // Mapped already: named at once.
        let mapped = state.az.window_for_surface(&surface);
        if let Some(name) = mapped.and_then(|id| super::foreign_toplevel::identifier(&state.az, id)) {
            identity.identifier(name);
        }
        let watching = state.az.toplevel_identities.watching.entry(surface).or_default();
        watching.retain(Resource::is_alive);
        watching.push(identity);
    }
}

impl Dispatch<Identity, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &Identity,
        _: identity::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }

    fn destroyed(state: &mut State, _: ClientId, identity: &Identity, _: &()) {
        state.az.toplevel_identities.watching.retain(|_, all| {
            all.retain(|other| other != identity);
            !all.is_empty()
        });
    }
}

/// Window `id` mapped (its ext-foreign-toplevel-list handle just made):
/// whoever asked for its toplevel's identifier hears it.
pub fn window_opened(state: &mut State, id: WinId) {
    let Some(name) = super::foreign_toplevel::identifier(&state.az, id) else { return };
    let Some(surface) = state.az.windows.get(&id).map(|m| m.toplevel().wl_surface().clone()) else { return };
    if let Some(all) = state.az.toplevel_identities.watching.get_mut(&surface) {
        all.retain(Resource::is_alive);
        for identity in all.iter() {
            identity.identifier(name.clone());
        }
    }
}
