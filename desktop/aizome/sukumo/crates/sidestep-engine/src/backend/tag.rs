//! Windows' tags (xdg-toplevel-tag v1): the program's own name for a
//! window among its windows ("main window", "settings"), which the
//! compositor may keep its rules for the window by. A tag is the same
//! from one run to the next and isn't translated.

use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use smithay_client_toolkit::reexports::protocols::xdg::toplevel_tag::v1::client::xdg_toplevel_tag_manager_v1::{
    self, XdgToplevelTagManagerV1,
};

use super::State;

/// The tag manager, where the compositor offers one.
pub(super) struct Tags {
    manager: Option<XdgToplevelTagManagerV1>,
}

impl Tags {
    pub fn new(globals: &GlobalList, qh: &QueueHandle<State>) -> Tags {
        Tags { manager: globals.bind::<XdgToplevelTagManagerV1, _, _>(qh, 1..=1, ()).ok() }
    }

    /// Tag `toplevel`. For the compositor to know the tag when it first
    /// sizes and places the window, this comes before the toplevel's first
    /// commit. Nothing without a manager: the window is an untagged one.
    pub fn set(&self, toplevel: &XdgToplevel, tag: String) {
        if let Some(manager) = &self.manager {
            manager.set_toplevel_tag(toplevel, tag);
        }
    }
}

impl Dispatch<XdgToplevelTagManagerV1, ()> for State {
    fn event(
        _: &mut State,
        _: &XdgToplevelTagManagerV1,
        _: xdg_toplevel_tag_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}
