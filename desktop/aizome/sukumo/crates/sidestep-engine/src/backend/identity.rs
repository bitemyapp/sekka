//! The compositor's identifiers for our toplevels (Aizome's
//! aizome-toplevel-identity-v1, `aizome-toplevel-identity-v1.xml` here):
//! the name another process is given for exactly one of our windows, as
//! Aizome's ❖Return names a terminal's window to the command it starts.
//! Each reaches the main thread as `FromRender::Identifier`. Under other
//! compositors there's no such global, and windows go unnamed.
//!
//! The protocol is Aizome's own, so its code is generated here; the
//! compositor generates its side from the same file.

use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;

use super::State;
use crate::protocol::{FromRender, WindowId};

#[allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#[allow(non_upper_case_globals, non_snake_case, unused_imports, missing_docs, clippy::all)]
mod generated {
    use smithay_client_toolkit::reexports::client as wayland_client;
    use smithay_client_toolkit::reexports::protocols::xdg::shell::client::*;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use smithay_client_toolkit::reexports::client::backend as wayland_backend;
        use smithay_client_toolkit::reexports::client::protocol::__interfaces::*;
        use smithay_client_toolkit::reexports::protocols::xdg::shell::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("src/backend/aizome-toplevel-identity-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("src/backend/aizome-toplevel-identity-v1.xml");
}

use generated::aizome_toplevel_identity_manager_v1::{self as manager, AizomeToplevelIdentityManagerV1 as Manager};
use generated::aizome_toplevel_identity_v1::{self as identity, AizomeToplevelIdentityV1};

/// One toplevel's identity object, destroyed with the window.
pub(super) struct Identity(AizomeToplevelIdentityV1);

impl Drop for Identity {
    fn drop(&mut self) {
        self.0.destroy();
    }
}

/// The manager, where the compositor offers one.
pub(super) struct Identities {
    manager: Option<Manager>,
}

impl Identities {
    pub fn new(globals: &GlobalList, qh: &QueueHandle<State>) -> Identities {
        Identities { manager: globals.bind::<Manager, _, _>(qh, 1..=1, ()).ok() }
    }

    /// Ask for `toplevel`'s identifier, for window `window`; `None`
    /// without the global.
    pub fn watch(&self, toplevel: &XdgToplevel, window: WindowId, qh: &QueueHandle<State>) -> Option<Identity> {
        Some(Identity(self.manager.as_ref()?.get_toplevel_identity(toplevel, qh, window)))
    }
}

impl Dispatch<Manager, ()> for State {
    fn event(_: &mut State, _: &Manager, _: manager::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {}
}

impl Dispatch<AizomeToplevelIdentityV1, WindowId> for State {
    fn event(
        state: &mut State,
        _: &AizomeToplevelIdentityV1,
        event: identity::Event,
        window: &WindowId,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        let identity::Event::Identifier { identifier } = event;
        state.send(FromRender::Identifier { window: *window, identifier });
    }
}
