//! xdg-toplevel-icon-v1, in place of smithay 0.7's. Smithay's never
//! forgets an icon's buffers: removing a buffer's destruction hook drops
//! some other hook (its comparison is the wrong way round), so a buffer
//! destroyed after its icon, as the protocol allows and as Chromium and
//! Electron do (`set_icon`, destroy the icon, then destroy the buffer),
//! still raises `no_buffer` on the dead icon, and the client is
//! disconnected before its first frame.
//!
//! Here an icon remembers its buffers only to check them when it is set: a
//! buffer destroyed while its icon was still alive is the client's error
//! (`no_buffer`); one destroyed after `set_icon` or after the icon is fine.
//! Aizome shows the icon of the app's desktop entry, so nothing else is
//! kept.

use std::sync::Mutex;

use smithay::reexports::wayland_protocols::xdg::toplevel_icon::v1::server::{
    xdg_toplevel_icon_manager_v1, xdg_toplevel_icon_v1,
};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};
use smithay::wayland::shm::with_buffer_contents;

use crate::state::State;

use xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1 as Manager;
use xdg_toplevel_icon_v1::{Error, XdgToplevelIconV1 as Icon};

/// An icon being built: its buffers by size and scale, and whether it has
/// been set on a toplevel (then it can't change).
#[derive(Default)]
pub struct IconData(Mutex<IconState>);

#[derive(Default)]
struct IconState {
    buffers: Vec<IconBuffer>,
    immutable: bool,
}

struct IconBuffer {
    buffer: WlBuffer,
    size: i32,
    scale: i32,
}

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
        // No preferred sizes: clients pick theirs.
        init.init(resource, ()).done();
    }
}

impl Dispatch<Manager, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &Manager,
        request: xdg_toplevel_icon_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        match request {
            xdg_toplevel_icon_manager_v1::Request::CreateIcon { id } => {
                init.init(id, IconData::default());
            }
            xdg_toplevel_icon_manager_v1::Request::SetIcon { toplevel: _, icon: Some(icon) } => {
                let Some(data) = icon.data::<IconData>() else { return };
                let mut s = data.0.lock().unwrap();
                if s.buffers.iter().any(|b| !b.buffer.is_alive()) {
                    icon.post_error(Error::NoBuffer, "a buffer of the icon was destroyed before the icon");
                    return;
                }
                s.immutable = true;
                // Its buffers may go now: the icon has what it needs.
                s.buffers.clear();
            }
            _ => {}
        }
    }
}

impl Dispatch<Icon, IconData> for State {
    fn request(
        _: &mut State,
        _: &Client,
        icon: &Icon,
        request: xdg_toplevel_icon_v1::Request,
        data: &IconData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let mut s = data.0.lock().unwrap();
        match request {
            xdg_toplevel_icon_v1::Request::SetName { .. } if s.immutable => {
                icon.post_error(Error::Immutable, "the icon was already set on a toplevel");
            }
            xdg_toplevel_icon_v1::Request::AddBuffer { .. } if s.immutable => {
                icon.post_error(Error::Immutable, "the icon was already set on a toplevel");
            }
            xdg_toplevel_icon_v1::Request::AddBuffer { buffer, scale } => {
                match with_buffer_contents(&buffer, |_, _, d| (d.width, d.height)) {
                    Ok((w, h)) if w == h => {
                        // A later buffer of the same size and scale replaces
                        // the earlier one.
                        s.buffers.retain(|b| (b.size, b.scale) != (w, scale));
                        s.buffers.push(IconBuffer { buffer, size: w, scale });
                    }
                    Ok(_) => icon.post_error(Error::InvalidBuffer, "the icon's buffer isn't square"),
                    Err(_) => icon.post_error(Error::InvalidBuffer, "the icon's buffer isn't a wl_shm buffer"),
                }
            }
            _ => {}
        }
    }
}
