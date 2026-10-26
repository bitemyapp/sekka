//! Foreign toplevels: ext-foreign-toplevel-list (through smithay) and
//! wlr-foreign-toplevel-management-unstable-v1 (version 3), so taskbars,
//! docks and scripts see the windows and can activate, minimise, fill,
//! full-screen and close them.
//!
//! Each loop iteration compares every window with what was last sent and
//! sends only the differences, followed by `done`.

use std::collections::HashMap;

use smithay::reexports::wayland_protocols::ext::foreign_toplevel_list::v1::server::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1;
use smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::{
    zwlr_foreign_toplevel_handle_v1 as handle, zwlr_foreign_toplevel_manager_v1 as manager,
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};
use smithay::wayland::foreign_toplevel_list::ForeignToplevelHandle;

use crate::WinId;
use crate::state::State;

use handle::ZwlrForeignToplevelHandleV1 as Handle;
use manager::ZwlrForeignToplevelManagerV1 as Manager;

pub struct ForeignToplevelManagerState {
    managers: Vec<Manager>,
    /// wlr handles per window.
    handles: HashMap<WinId, Vec<Handle>>,
    /// What was last sent, per window.
    sent: HashMap<WinId, Snapshot>,
    /// ext-foreign-toplevel-list handles.
    ext: HashMap<WinId, ForeignToplevelHandle>,
}

pub struct GlobalData {
    filter: Box<dyn Fn(&Client) -> bool + Send + Sync>,
}

/// The window a wlr handle stands for.
pub struct HandleData {
    win: WinId,
}

#[derive(Clone, Debug, PartialEq, Default)]
struct Snapshot {
    title: String,
    app_id: String,
    states: Vec<u32>,
    outputs: Vec<String>,
    parent: Option<WinId>,
}

impl ForeignToplevelManagerState {
    pub fn new<F>(dh: &DisplayHandle, filter: F) -> Self
    where
        F: Fn(&Client) -> bool + Send + Sync + 'static,
    {
        dh.create_global::<State, Manager, _>(3, GlobalData { filter: Box::new(filter) });
        ForeignToplevelManagerState {
            managers: Vec::new(),
            handles: HashMap::new(),
            sent: HashMap::new(),
            ext: HashMap::new(),
        }
    }
}

fn snapshot(state: &State, id: WinId) -> Option<Snapshot> {
    let m = state.az.windows.get(&id)?;
    let w = &m.view;
    let mut states = Vec::new();
    if w.fill {
        states.push(handle::State::Maximized as u32);
    }
    if w.minimised || w.hidden {
        states.push(handle::State::Minimized as u32);
    }
    if state.az.view.focused == Some(id) {
        states.push(handle::State::Activated as u32);
    }
    if w.fullscreen {
        states.push(handle::State::Fullscreen as u32);
    }
    let outputs = if w.shown {
        state.az.outputs.iter().filter(|o| state.az.output_rect(o).intersects(w.rect)).map(|o| o.name()).collect()
    } else {
        Vec::new()
    };
    let (app_id, title) = crate::shell::toplevel_ids(m.toplevel());
    Some(Snapshot { title, app_id, states, outputs, parent: w.parent })
}

fn states_bytes(states: &[u32]) -> Vec<u8> {
    states.iter().flat_map(|s| s.to_ne_bytes()).collect()
}

/// Announces a window to one manager.
fn announce(state: &mut State, id: WinId, m: &Manager, dh: &DisplayHandle) {
    let Ok(client) = dh.get_client(m.id()) else { return };
    let Ok(h) = client.create_resource::<Handle, _, State>(dh, m.version(), HandleData { win: id }) else { return };
    m.toplevel(&h);
    let Some(s) = snapshot(state, id) else { return };
    h.title(s.title.clone());
    h.app_id(s.app_id.clone());
    for o in &state.az.outputs {
        if s.outputs.contains(&o.name()) {
            for wo in o.client_outputs(&client) {
                h.output_enter(&wo);
            }
        }
    }
    h.state(states_bytes(&s.states));
    if h.version() >= 3 {
        let parent = s
            .parent
            .and_then(|p| state.az.foreign_toplevel_mgmt.handles.get(&p))
            .and_then(|hs| hs.iter().find(|ph| ph.id().same_client_as(&h.id())).cloned());
        h.parent(parent.as_ref());
    }
    h.done();
    state.az.foreign_toplevel_mgmt.handles.entry(id).or_default().push(h);
}

pub fn window_opened(state: &mut State, id: WinId) {
    let managers = state.az.foreign_toplevel_mgmt.managers.clone();
    let dh = state.az.dh.clone();
    for m in managers.iter().filter(|m| m.is_alive()) {
        announce(state, id, m, &dh);
    }
    if let Some(s) = snapshot(state, id) {
        let h = state.az.foreign_toplevel_list.new_toplevel::<State>(&s.title, &s.app_id);
        state.az.foreign_toplevel_mgmt.ext.insert(id, h);
        state.az.foreign_toplevel_mgmt.sent.insert(id, s);
    }
}

pub fn window_closed(state: &mut State, id: WinId) {
    let fm = &mut state.az.foreign_toplevel_mgmt;
    for h in fm.handles.remove(&id).unwrap_or_default() {
        if h.is_alive() {
            h.closed();
        }
    }
    fm.sent.remove(&id);
    if let Some(h) = fm.ext.remove(&id) {
        state.az.foreign_toplevel_list.remove_toplevel(&h);
    }
}

/// The window an ext-foreign-toplevel-list handle stands for (for window
/// capture sources).
pub fn window_of(state: &State, handle: &ExtForeignToplevelHandleV1) -> Option<WinId> {
    let identifier = ForeignToplevelHandle::from_resource(handle)?.identifier();
    let ext = &state.az.foreign_toplevel_mgmt.ext;
    ext.iter().find(|(_, h)| h.identifier() == identifier).map(|(id, _)| *id)
}

/// A window's ext-foreign-toplevel-list identifier (how
/// xdg-desktop-portal-wlr names windows to its chooser).
pub fn identifier(az: &crate::state::Aizome, id: WinId) -> Option<String> {
    az.foreign_toplevel_mgmt.ext.get(&id).map(|h| h.identifier())
}

/// Changes are picked up by [`refresh`].
pub fn window_changed(_state: &mut State, _id: WinId) {}

/// Sends what changed since the last iteration.
pub fn refresh(state: &mut State) {
    let ids: Vec<WinId> = state.az.foreign_toplevel_mgmt.sent.keys().copied().collect();
    for id in ids {
        let Some(now) = snapshot(state, id) else { continue };
        let before = state.az.foreign_toplevel_mgmt.sent.get(&id).cloned().unwrap_or_default();
        if now == before {
            continue;
        }
        let handles = state.az.foreign_toplevel_mgmt.handles.get(&id).cloned().unwrap_or_default();
        for h in handles.iter().filter(|h| h.is_alive()) {
            let Ok(client) = state.az.dh.get_client(h.id()) else { continue };
            if now.title != before.title {
                h.title(now.title.clone());
            }
            if now.app_id != before.app_id {
                h.app_id(now.app_id.clone());
            }
            for o in &state.az.outputs {
                let (was, is) = (before.outputs.contains(&o.name()), now.outputs.contains(&o.name()));
                if was != is {
                    for wo in o.client_outputs(&client) {
                        if is {
                            h.output_enter(&wo);
                        } else {
                            h.output_leave(&wo);
                        }
                    }
                }
            }
            if now.states != before.states {
                h.state(states_bytes(&now.states));
            }
            h.done();
        }
        if let Some(e) = state.az.foreign_toplevel_mgmt.ext.get(&id) {
            if now.title != before.title {
                e.send_title(&now.title);
            }
            if now.app_id != before.app_id {
                e.send_app_id(&now.app_id);
            }
            if now.title != before.title || now.app_id != before.app_id {
                e.send_done();
            }
        }
        state.az.foreign_toplevel_mgmt.sent.insert(id, now);
    }
    let fm = &mut state.az.foreign_toplevel_mgmt;
    fm.handles.values_mut().for_each(|hs| hs.retain(|h| h.is_alive()));
    fm.managers.retain(|m| m.is_alive());
}

impl GlobalDispatch<Manager, GlobalData> for State {
    fn bind(
        state: &mut State,
        dh: &DisplayHandle,
        _: &Client,
        resource: New<Manager>,
        _: &GlobalData,
        init: &mut DataInit<'_, State>,
    ) {
        let m = init.init(resource, ());
        let mut ids: Vec<WinId> = state.az.windows.keys().copied().collect();
        ids.sort();
        for id in ids {
            announce(state, id, &m, dh);
        }
        state.az.foreign_toplevel_mgmt.managers.push(m);
    }

    fn can_view(client: Client, data: &GlobalData) -> bool {
        (data.filter)(&client)
    }
}

impl Dispatch<Manager, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        m: &Manager,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        if let manager::Request::Stop = request {
            m.finished();
            state.az.foreign_toplevel_mgmt.managers.retain(|x| x != m);
        }
    }
}

impl Dispatch<Handle, HandleData> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &Handle,
        request: handle::Request,
        data: &HandleData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let id = data.win;
        if !state.az.windows.contains_key(&id) {
            return;
        }
        // The window manager's verbs, as IPC asks for them.
        use aizome_ipc::Request as R;
        let req = match request {
            handle::Request::SetMaximized => R::Fill { id: Some(id), state: Some(true) },
            handle::Request::UnsetMaximized => R::Fill { id: Some(id), state: Some(false) },
            handle::Request::SetMinimized => R::Minimise { id: Some(id) },
            handle::Request::UnsetMinimized => R::Unminimise { id },
            handle::Request::Activate { .. } => R::Focus { id },
            handle::Request::Close => R::Close { id: Some(id) },
            handle::Request::SetFullscreen { .. } => R::Fullscreen { id: Some(id), state: Some(true) },
            handle::Request::UnsetFullscreen => R::Fullscreen { id: Some(id), state: Some(false) },
            _ => return,
        };
        let _ = state.handle_request(req);
    }

    fn destroyed(state: &mut State, _: ClientId, h: &Handle, _: &HandleData) {
        for hs in state.az.foreign_toplevel_mgmt.handles.values_mut() {
            hs.retain(|x| x != h);
        }
    }
}
