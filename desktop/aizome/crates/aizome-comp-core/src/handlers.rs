//! The smaller protocols: seat, selections and clipboard, shm and dmabuf,
//! outputs, activation, idle, constraints, input methods and the rest.

use std::os::fd::OwnedFd;
use std::sync::Arc;

use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::desktop::PopupKind;
use smithay::input::pointer::{CursorImageStatus, PointerHandle};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::{
    zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
    zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2,
};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle};
use smithay::utils::{Logical, Point, Rectangle};
use smithay::wayland::compositor::{get_parent, with_states};
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::wayland::foreign_toplevel_list::{ForeignToplevelListHandler, ForeignToplevelListState};
use smithay::wayland::fractional_scale::{FractionalScaleHandler, with_fractional_scale};
use smithay::wayland::idle_inhibit::IdleInhibitHandler;
use smithay::wayland::idle_notify::{IdleNotifierHandler, IdleNotifierState};
use smithay::wayland::input_method::{
    InputMethodHandler, InputMethodManagerState, InputMethodUserData, PopupSurface as ImePopupSurface,
};
use smithay::wayland::keyboard_shortcuts_inhibit::{
    KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::pointer_constraints::{PointerConstraintsHandler, with_pointer_constraint};
use smithay::wayland::security_context::{SecurityContext, SecurityContextHandler, SecurityContextListenerSource};
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler, set_data_device_focus,
};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
};
use smithay::wayland::selection::{SelectionHandler, SelectionTarget};
use smithay::wayland::shell::xdg::ToplevelSurface;
use smithay::wayland::shell::xdg::dialog::XdgDialogHandler;
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::tablet_manager::TabletSeatHandler;
use smithay::wayland::text_input::TextInputSeat;
use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};
use smithay::wayland::xdg_foreign::{XdgForeignHandler, XdgForeignState};
use tracing::{info, warn};

use crate::focus::Focus;
use crate::state::{ClientState, State};

impl SeatHandler for State {
    type KeyboardFocus = Focus;
    type PointerFocus = Focus;
    type TouchFocus = Focus;

    fn seat_state(&mut self) -> &mut SeatState<State> {
        &mut self.az.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&Focus>) {
        let client = focused.and_then(|f| self.az.dh.get_client(f.0.id()).ok());
        set_data_device_focus(&self.az.dh, seat, client.clone());
        set_primary_focus(&self.az.dh, seat, client);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        // Called with the pointer's lock held: touch nothing that takes it.
        self.az.cursor_status = image;
        if let Some(o) = self.az.active_output() {
            self.az.queue_redraw(&o);
        }
    }

    fn led_state_changed(&mut self, _seat: &Seat<Self>, leds: smithay::input::keyboard::LedState) {
        if let crate::backend::Backend::Udev(u) = &mut self.backend {
            u.update_leds(leds);
        }
    }
}

impl TabletSeatHandler for State {
    fn tablet_tool_image(&mut self, _tool: &smithay::backend::input::TabletToolDescriptor, image: CursorImageStatus) {
        self.az.cursor_status = image;
    }
}

/// The compositor's own clipboard offers (screenshots): their bytes.
#[derive(Clone)]
pub enum SelectionData {
    Bytes(Arc<Vec<u8>>),
}

impl SelectionHandler for State {
    type SelectionUserData = SelectionData;

    fn send_selection(
        &mut self,
        _ty: SelectionTarget,
        _mime: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        data: &SelectionData,
    ) {
        let SelectionData::Bytes(bytes) = data.clone();
        // Write on a thread: the reader may be slow, and it mustn't stall us.
        std::thread::spawn(move || {
            use std::io::Write;
            let mut f = std::fs::File::from(fd);
            let _ = f.write_all(&bytes);
        });
    }
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.az.data_device
    }
}

impl ClientDndGrabHandler for State {
    fn started(&mut self, _source: Option<WlDataSource>, icon: Option<WlSurface>, _seat: Seat<Self>) {
        self.az.dnd_icon = icon;
        self.az.dnd_offset = (0, 0).into();
    }
    fn dropped(&mut self, _target: Option<WlSurface>, _validated: bool, _seat: Seat<Self>) {
        self.az.dnd_icon = None;
        self.az.queue_redraw_all();
    }
}

impl ServerDndGrabHandler for State {}

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.az.primary_selection
    }
}

impl smithay::wayland::selection::wlr_data_control::DataControlHandler for State {
    fn data_control_state(&self) -> &smithay::wayland::selection::wlr_data_control::DataControlState {
        &self.az.wlr_data_control
    }
}

impl smithay::wayland::selection::ext_data_control::DataControlHandler for State {
    fn data_control_state(&self) -> &smithay::wayland::selection::ext_data_control::DataControlState {
        &self.az.ext_data_control
    }
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.az.shm
    }
}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.az.dmabuf
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        if self.backend.import_dmabuf(&dmabuf) {
            let _ = notifier.successful::<State>();
        } else {
            notifier.failed();
        }
    }
}

impl OutputHandler for State {}

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.az.xdg_activation
    }

    fn token_created(&mut self, token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        // Every token is kept; one may move focus only when made from an
        // input event of a client that had the keyboard or the pointer then
        // (a shell launching an app from a click). Others only mark the
        // window urgent.
        let valid = data.serial.as_ref().is_some_and(|(serial, seat)| {
            let Some(seat) = Seat::<State>::from_resource(seat) else { return false };
            let recent =
                seat.get_keyboard().is_some_and(|k| k.last_enter().is_some_and(|l| serial.is_no_older_than(&l)));
            let client = data.client_id.clone();
            let has = |s: Option<WlSurface>| s.and_then(|s| s.client()).map(|c| c.id()) == client && client.is_some();
            let kb_focus = seat.get_keyboard().and_then(|k| k.current_focus()).map(|f| f.0);
            let ptr_focus = seat.get_pointer().and_then(|p| p.current_focus()).map(|f| f.0);
            recent || has(kb_focus) || has(ptr_focus)
        });
        if valid {
            self.az.valid_tokens.insert(token.to_string());
        }
        true
    }

    fn request_activation(&mut self, token: XdgActivationToken, data: XdgActivationTokenData, surface: WlSurface) {
        let Some(id) = self.az.window_for_surface(&surface) else { return };
        let valid = self.az.valid_tokens.remove(&token.to_string()) && data.timestamp.elapsed().as_secs() < 30;
        if valid {
            info!(id, "activation granted");
        }
        self.toplevel_request(id, crate::logic::ToplevelRequest::Activate { valid });
        self.az.xdg_activation.remove_token(&token);
    }
}

impl XdgForeignHandler for State {
    fn xdg_foreign_state(&mut self) -> &mut XdgForeignState {
        &mut self.az.xdg_foreign
    }
}

impl IdleNotifierHandler for State {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.az.idle_notifier
    }
}

impl IdleInhibitHandler for State {
    fn inhibit(&mut self, surface: WlSurface) {
        self.az.idle_inhibitors.insert(surface);
        self.az.idle_notifier.set_is_inhibited(true);
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.drop_idle_inhibitor(&surface);
    }
}

impl KeyboardShortcutsInhibitHandler for State {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.az.shortcuts_inhibit
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        // Remote desktops and VMs may take the keys; the lock and VT keys
        // stay with the compositor regardless.
        inhibitor.activate();
    }
}

impl PointerConstraintsHandler for State {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        // Asked for anew: a reset no longer holds it off.
        crate::recovery::constraint_requested(&mut self.az, surface);
        if pointer.current_focus().is_some_and(|f| &f.0 == surface) {
            with_pointer_constraint(surface, pointer, |c| {
                if let Some(c) = c {
                    c.activate();
                }
            });
        }
    }

    fn cursor_position_hint(
        &mut self,
        surface: &WlSurface,
        pointer: &PointerHandle<Self>,
        location: Point<f64, Logical>,
    ) {
        let active = with_pointer_constraint(surface, pointer, |c| c.is_some_and(|c| c.is_active()));
        if !active {
            return;
        }
        if let Some(origin) = self.az.surface_origin(surface) {
            pointer.set_location(origin + location);
            self.az.pointer_pos = origin + location;
        }
    }
}

impl FractionalScaleHandler for State {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let mut root = surface.clone();
        while let Some(p) = get_parent(&root) {
            root = p;
        }
        let output = self
            .az
            .window_for_surface(&root)
            .and_then(|id| self.az.window_rect(id))
            .and_then(|r| self.az.output_for_rect(r))
            .or_else(|| self.az.active_output());
        if let Some(o) = output {
            with_states(&surface, |states| {
                with_fractional_scale(states, |fs| fs.set_preferred_scale(o.current_scale().fractional_scale()));
            });
        }
    }
}

impl SecurityContextHandler for State {
    fn context_created(&mut self, source: SecurityContextListenerSource, context: SecurityContext) {
        let res = self.az.lh.insert_source(source, move |stream, _, state| {
            let data = ClientState { security_context: Some(context.clone()), ..Default::default() };
            if let Err(e) = state.az.dh.insert_client(stream, Arc::new(data)) {
                warn!("sandboxed client: {e}");
            }
        });
        if let Err(e) = res {
            warn!("security context listener: {e}");
        }
    }
}

impl InputMethodHandler for State {
    fn new_popup(&mut self, surface: ImePopupSurface) {
        if let Err(e) = self.az.popups.track_popup(PopupKind::from(surface)) {
            warn!("input method popup: {e}");
        }
    }

    fn dismiss_popup(&mut self, surface: ImePopupSurface) {
        if let Some(parent) = surface.get_parent().map(|p| p.surface.clone()) {
            let _ = smithay::desktop::PopupManager::dismiss_popup(&parent, &PopupKind::from(surface));
        }
    }

    fn popup_repositioned(&mut self, _surface: ImePopupSurface) {}

    /// The window geometry of the surface with the text field: a window's,
    /// or a menu's for a field in a menu (GTK's emoji search).
    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        if let Some(m) = self.az.window_for_surface(parent).and_then(|id| self.az.windows.get(&id)) {
            return m.window.geometry();
        }
        self.az.popups.find_popup(parent).map(|p| p.geometry()).unwrap_or_default()
    }
}

/// An input method's requests go to smithay, but for `commit`: what it
/// committed reaches the text input whatever serial it gives, as under
/// wlroots. smithay discards a commit whose serial isn't the number of
/// `done` events the input method was sent (by sending the text input
/// `done(0)`), and IBus 1.5's Wayland frontend counts from zero again on
/// every global the registry announces: after a display is plugged in,
/// every commit of it is stale. Foot drops a `done` whose serial isn't its
/// own and so lost every letter typed through IBus (Return, which IBus
/// passes through its virtual keyboard, still came); GTK applies them
/// regardless.
impl Dispatch<ZwpInputMethodV2, InputMethodUserData<State>> for State {
    fn request(
        state: &mut State,
        client: &Client,
        resource: &ZwpInputMethodV2,
        request: zwp_input_method_v2::Request,
        data: &InputMethodUserData<State>,
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        if let zwp_input_method_v2::Request::Commit { serial } = request {
            tracing::trace!(serial, "input method commit");
            state.az.seat.text_input().done(false);
            return;
        }
        <InputMethodManagerState as Dispatch<ZwpInputMethodV2, InputMethodUserData<State>, State>>::request(
            state, client, resource, request, data, dh, data_init,
        )
    }

    fn destroyed(state: &mut State, client: ClientId, resource: &ZwpInputMethodV2, data: &InputMethodUserData<State>) {
        <InputMethodManagerState as Dispatch<ZwpInputMethodV2, InputMethodUserData<State>, State>>::destroyed(
            state, client, resource, data,
        )
    }
}

impl XdgDialogHandler for State {
    fn modal_changed(&mut self, toplevel: ToplevelSurface, _is_modal: bool) {
        if let Some(id) = self.az.window_for_surface(toplevel.wl_surface()) {
            let parent = toplevel.parent().and_then(|p| self.az.window_for_surface(&p));
            self.toplevel_request(id, crate::logic::ToplevelRequest::ParentChanged(parent));
        }
    }
}

impl ForeignToplevelListHandler for State {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.az.foreign_toplevel_list
    }
}

impl crate::state::Aizome {
    /// Where a surface's origin is in the global layout (windows and
    /// layer surfaces).
    pub fn surface_origin(&self, surface: &WlSurface) -> Option<Point<f64, Logical>> {
        let mut root = surface.clone();
        while let Some(p) = get_parent(&root) {
            root = p;
        }
        if let Some(id) = self.window_for_surface(&root) {
            let m = self.windows.get(&id)?;
            let (rect, geo) = (m.view.rect, m.window.geometry());
            return Some(Point::from(((rect.x - geo.loc.x) as f64, (rect.y - geo.loc.y) as f64)));
        }
        for o in &self.outputs {
            let map = smithay::desktop::layer_map_for_output(o);
            if let Some(l) = map.layer_for_surface(&root, smithay::desktop::WindowSurfaceType::TOPLEVEL) {
                let g = map.layer_geometry(l)?;
                return Some((g.loc + o.current_location()).to_f64());
            }
        }
        None
    }
}

smithay::delegate_seat!(State);
smithay::delegate_tablet_manager!(State);
smithay::delegate_data_device!(State);
smithay::delegate_primary_selection!(State);
smithay::delegate_data_control!(State);
smithay::delegate_ext_data_control!(State);
smithay::delegate_shm!(State);
smithay::delegate_dmabuf!(State);
smithay::delegate_output!(State);
smithay::delegate_viewporter!(State);
smithay::delegate_presentation!(State);
smithay::delegate_fractional_scale!(State);
smithay::delegate_xdg_activation!(State);
smithay::delegate_xdg_foreign!(State);
smithay::delegate_idle_notify!(State);
smithay::delegate_idle_inhibit!(State);
smithay::delegate_keyboard_shortcuts_inhibit!(State);
smithay::delegate_pointer_constraints!(State);
smithay::delegate_relative_pointer!(State);
smithay::delegate_pointer_gestures!(State);
smithay::delegate_cursor_shape!(State);
smithay::delegate_single_pixel_buffer!(State);
smithay::delegate_text_input_manager!(State);
// smithay::delegate_input_method_manager! but for zwp_input_method_v2,
// which State dispatches itself (above).
smithay::reexports::wayland_server::delegate_global_dispatch!(State: [
    ZwpInputMethodManagerV2: smithay::wayland::input_method::InputMethodManagerGlobalData
] => InputMethodManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(State: [ZwpInputMethodManagerV2: ()] => InputMethodManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(State: [
    ZwpInputMethodKeyboardGrabV2: smithay::wayland::input_method::InputMethodKeyboardUserData<State>
] => InputMethodManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(State: [
    ZwpInputPopupSurfaceV2: smithay::wayland::input_method::InputMethodPopupSurfaceUserData
] => InputMethodManagerState);
smithay::delegate_virtual_keyboard_manager!(State);
smithay::delegate_security_context!(State);
smithay::delegate_content_type!(State);
smithay::delegate_xdg_dialog!(State);
smithay::delegate_xdg_system_bell!(State);

/// xdg-system-bell: the shell rings it (flashes the screen edge or plays
/// the alert sound), told as a `bell` binding with the output.
impl smithay::wayland::xdg_system_bell::XdgSystemBellHandler for State {
    fn ring(&mut self, surface: Option<WlSurface>) {
        let output = surface
            .as_ref()
            .and_then(|s| self.az.window_for_surface(s))
            .and_then(|id| self.az.window_rect(id))
            .and_then(|r| self.az.output_for_rect(r))
            .or_else(|| self.az.active_output())
            .map(|o| o.name());
        self.az.ipc.emit(&aizome_ipc::Event::Binding { action: "bell".into(), pointer: None, output });
    }
}
smithay::delegate_alpha_modifier!(State);
smithay::delegate_fifo!(State);
smithay::delegate_commit_timing!(State);
smithay::delegate_foreign_toplevel_list!(State);
