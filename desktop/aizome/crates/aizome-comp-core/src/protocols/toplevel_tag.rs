//! xdg-toplevel-tag-v1: an app's own name for a window among its windows
//! ("main window", "settings"), the same from one run to the next, which
//! the client sets with the toplevel's first commit. The logic reads it
//! when it first sizes the window and when the window maps: Aizome's kit
//! tags its utility windows, which the logic keeps by themselves.
//!
//! A tag lasts as long as the mapping it was given for. It is kept with the
//! toplevel's `wl_surface`, which outlives both: an unmapped toplevel is as
//! it was when it was made (xdg-shell discards its attributes), and a
//! surface whose toplevel is destroyed may be given another. So the shell
//! forgets the tag ([`clear`]) when the window is unmapped and when its
//! toplevel goes, and what opens next on that surface says what it is anew.
//!
//! Smithay 0.7 speaks the protocol. Its own record of the tag
//! (`XdgToplevelTagSurfaceData`) isn't read here, because 0.7.0 writes a
//! window's description over its tag (Smithay#2045 fixed that on `master`:
//! see docs/upstream/smithay). The tag is kept from the handler instead.

use std::sync::Mutex;

use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::XdgToplevel;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::ToplevelSurface;
use smithay::wayland::xdg_toplevel_tag::{XdgToplevelTagHandler, XdgToplevelTagManager};

use crate::state::State;

/// A toplevel's tag, kept with its surface.
#[derive(Default)]
struct Tag(Mutex<Option<String>>);

pub fn create_global(dh: &DisplayHandle) {
    XdgToplevelTagManager::new::<State>(dh);
}

/// The tag the client gave the toplevel, if it gave one.
pub fn tag(toplevel: &ToplevelSurface) -> Option<String> {
    with_states(toplevel.wl_surface(), |s| s.data_map.get::<Tag>().and_then(|t| t.0.lock().unwrap().clone()))
}

/// Forget the toplevel's tag: it was unmapped or destroyed, and its surface
/// may show another window next.
pub fn clear(toplevel: &ToplevelSurface) {
    let surface = toplevel.wl_surface();
    // A surface that is gone takes its tag with it.
    if !surface.is_alive() {
        return;
    }
    with_states(surface, |s| {
        if let Some(tag) = s.data_map.get::<Tag>() {
            *tag.0.lock().unwrap() = None;
        }
    });
}

impl XdgToplevelTagHandler for State {
    fn set_tag(&mut self, toplevel: XdgToplevel, tag: String) {
        let Some(toplevel) = self.az.xdg_shell.get_toplevel(&toplevel) else { return };
        with_states(toplevel.wl_surface(), |s| {
            *s.data_map.get_or_insert_threadsafe(Tag::default).0.lock().unwrap() = Some(tag);
        });
    }
}

smithay::delegate_xdg_toplevel_tag!(State);
