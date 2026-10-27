//! The seat's focus target: a surface. A newtype over `WlSurface` so
//! popup grabs (which need a target made from a popup) work.

use std::borrow::Cow;

use smithay::backend::input::KeyState;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::{
    LayerSurface, PopupKeyboardGrab, PopupKind, PopupUngrabStrategy, WindowSurfaceType, layer_map_for_output,
};
use smithay::input::Seat;
use smithay::input::keyboard::{KeyboardTarget, KeysymHandle, ModifiersState};
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent, GesturePinchEndEvent,
    GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
    MotionEvent, PointerGrab, PointerInnerHandle, PointerTarget, RelativeMotionEvent,
};
use smithay::input::touch::{
    DownEvent, MotionEvent as TouchMotionEvent, OrientationEvent, ShapeEvent, TouchTarget, UpEvent,
};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{IsAlive, Logical, Point, SERIAL_COUNTER, Serial};
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, Layer};

use crate::state::{Aizome, State};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Focus(pub WlSurface);

impl From<WlSurface> for Focus {
    fn from(s: WlSurface) -> Focus {
        Focus(s)
    }
}

impl From<&WlSurface> for Focus {
    fn from(s: &WlSurface) -> Focus {
        Focus(s.clone())
    }
}

impl From<PopupKind> for Focus {
    fn from(p: PopupKind) -> Focus {
        Focus(p.wl_surface().clone())
    }
}

impl IsAlive for Focus {
    fn alive(&self) -> bool {
        self.0.alive()
    }
}

impl WaylandFocus for Focus {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        Some(Cow::Borrowed(&self.0))
    }
    fn same_client_as(&self, object_id: &ObjectId) -> bool {
        self.0.id().same_client_as(object_id)
    }
}

impl KeyboardTarget<State> for Focus {
    fn enter(&self, seat: &Seat<State>, data: &mut State, keys: Vec<KeysymHandle<'_>>, serial: Serial) {
        KeyboardTarget::enter(&self.0, seat, data, keys, serial)
    }
    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial) {
        KeyboardTarget::leave(&self.0, seat, data, serial)
    }
    fn key(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        key: KeysymHandle<'_>,
        state: KeyState,
        serial: Serial,
        time: u32,
    ) {
        KeyboardTarget::key(&self.0, seat, data, key, state, serial, time)
    }
    fn modifiers(&self, seat: &Seat<State>, data: &mut State, modifiers: ModifiersState, serial: Serial) {
        KeyboardTarget::modifiers(&self.0, seat, data, modifiers, serial)
    }
}

impl PointerTarget<State> for Focus {
    fn enter(&self, seat: &Seat<State>, data: &mut State, event: &MotionEvent) {
        PointerTarget::enter(&self.0, seat, data, event)
    }
    fn motion(&self, seat: &Seat<State>, data: &mut State, event: &MotionEvent) {
        PointerTarget::motion(&self.0, seat, data, event)
    }
    fn relative_motion(&self, seat: &Seat<State>, data: &mut State, event: &RelativeMotionEvent) {
        PointerTarget::relative_motion(&self.0, seat, data, event)
    }
    fn button(&self, seat: &Seat<State>, data: &mut State, event: &ButtonEvent) {
        PointerTarget::button(&self.0, seat, data, event)
    }
    fn axis(&self, seat: &Seat<State>, data: &mut State, frame: AxisFrame) {
        PointerTarget::axis(&self.0, seat, data, frame)
    }
    fn frame(&self, seat: &Seat<State>, data: &mut State) {
        PointerTarget::frame(&self.0, seat, data)
    }
    fn gesture_swipe_begin(&self, seat: &Seat<State>, data: &mut State, event: &GestureSwipeBeginEvent) {
        PointerTarget::gesture_swipe_begin(&self.0, seat, data, event)
    }
    fn gesture_swipe_update(&self, seat: &Seat<State>, data: &mut State, event: &GestureSwipeUpdateEvent) {
        PointerTarget::gesture_swipe_update(&self.0, seat, data, event)
    }
    fn gesture_swipe_end(&self, seat: &Seat<State>, data: &mut State, event: &GestureSwipeEndEvent) {
        PointerTarget::gesture_swipe_end(&self.0, seat, data, event)
    }
    fn gesture_pinch_begin(&self, seat: &Seat<State>, data: &mut State, event: &GesturePinchBeginEvent) {
        PointerTarget::gesture_pinch_begin(&self.0, seat, data, event)
    }
    fn gesture_pinch_update(&self, seat: &Seat<State>, data: &mut State, event: &GesturePinchUpdateEvent) {
        PointerTarget::gesture_pinch_update(&self.0, seat, data, event)
    }
    fn gesture_pinch_end(&self, seat: &Seat<State>, data: &mut State, event: &GesturePinchEndEvent) {
        PointerTarget::gesture_pinch_end(&self.0, seat, data, event)
    }
    fn gesture_hold_begin(&self, seat: &Seat<State>, data: &mut State, event: &GestureHoldBeginEvent) {
        PointerTarget::gesture_hold_begin(&self.0, seat, data, event)
    }
    fn gesture_hold_end(&self, seat: &Seat<State>, data: &mut State, event: &GestureHoldEndEvent) {
        PointerTarget::gesture_hold_end(&self.0, seat, data, event)
    }
    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial, time: u32) {
        PointerTarget::leave(&self.0, seat, data, serial, time)
    }
}

impl TouchTarget<State> for Focus {
    fn down(&self, seat: &Seat<State>, data: &mut State, event: &DownEvent, seq: Serial) {
        TouchTarget::down(&self.0, seat, data, event, seq)
    }
    fn up(&self, seat: &Seat<State>, data: &mut State, event: &UpEvent, seq: Serial) {
        TouchTarget::up(&self.0, seat, data, event, seq)
    }
    fn motion(&self, seat: &Seat<State>, data: &mut State, event: &TouchMotionEvent, seq: Serial) {
        TouchTarget::motion(&self.0, seat, data, event, seq)
    }
    fn frame(&self, seat: &Seat<State>, data: &mut State, seq: Serial) {
        TouchTarget::frame(&self.0, seat, data, seq)
    }
    fn cancel(&self, seat: &Seat<State>, data: &mut State, seq: Serial) {
        TouchTarget::cancel(&self.0, seat, data, seq)
    }
    fn shape(&self, seat: &Seat<State>, data: &mut State, event: &ShapeEvent, seq: Serial) {
        TouchTarget::shape(&self.0, seat, data, event, seq)
    }
    fn orientation(&self, seat: &Seat<State>, data: &mut State, event: &OrientationEvent, seq: Serial) {
        TouchTarget::orientation(&self.0, seat, data, event, seq)
    }
}

/// The popup grab in force (an xdg_popup.grab): its root surface (the
/// toplevel or layer surface the popups hang from), and whether Aizome has
/// dismissed its popups already (it waits for the client to destroy them).
pub struct PopupGrabbed {
    pub grab: smithay::desktop::PopupGrab<State>,
    pub root: WlSurface,
    pub dismissed: bool,
}

impl PopupGrabbed {
    /// The grab holds the keyboard: its popups are open and not dismissed.
    pub fn live(&self) -> bool {
        !self.dismissed && self.menu().is_some()
    }

    /// The topmost popup still open. None once the chain is closed, by
    /// Aizome or by smithay (which closes it for a press outside, or a
    /// nested grab refused, without Aizome hearing).
    pub fn menu(&self) -> Option<Focus> {
        if self.grab.has_ended() {
            return None;
        }
        self.grab.current_grab().filter(|f| f.0 != self.root)
    }

    pub fn owns_serial(&self, has: impl Fn(Serial) -> bool) -> bool {
        has(self.grab.serial()) || self.grab.previous_serial().is_some_and(&has)
    }
}

impl State {
    /// Gives keyboard focus to what should have it: the lock surface, an
    /// exclusive layer surface, a layer surface clicked on, or the focused
    /// window, and to the topmost popup of a menu open on it.
    pub fn update_keyboard_focus(&mut self) {
        // The logic's latest word on which window has focus.
        self.lg_call((), |vt, st| (vt.publish)(st));
        let target = self.az.keyboard_target().map(Focus);
        let Some(keyboard) = self.az.seat.get_keyboard() else { return };
        // A popup grab gives way to whatever else must have the keyboard
        // (the lock, an exclusive layer surface, the switcher): its popups
        // close.
        if self.az.popup_grab.as_ref().is_some_and(|g| g.live() && target.as_ref().map(|f| &f.0) != Some(&g.root)) {
            self.dismiss_popup_grab();
        }
        if self.keep_keys_in_menu() {
            return;
        }
        if keyboard.current_focus() != target {
            tracing::debug!(
                target: "aizome::focus",
                from = ?keyboard.current_focus().map(|f| f.0.id()),
                to = ?target.as_ref().map(|f| f.0.id()),
                grabbed = keyboard.is_grabbed(),
                "keyboard focus"
            );
            keyboard.set_focus(self, target, SERIAL_COUNTER.next_serial());
        }
    }

    /// While a menu is open its topmost popup has the keys, whichever grab
    /// the keyboard has: the menu's, or an input method's (ibus holds the
    /// keyboard while a field has it; the field's context menu opens over
    /// that, and a field in the menu has it grab the keyboard again). The
    /// menu's grab takes the keyboard only while no other grab has it, so
    /// it never displaces the input method's. Whether a menu is open.
    fn keep_keys_in_menu(&mut self) -> bool {
        let Some(g) = self.az.popup_grab.as_ref().filter(|g| !g.dismissed) else { return false };
        let Some(menu) = g.menu() else { return false };
        let grab = g.grab.clone();
        let Some(keyboard) = self.az.seat.get_keyboard() else { return true };
        if !keyboard.is_grabbed() {
            tracing::debug!(target: "aizome::focus", "popup grab takes the keyboard");
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), grab.serial());
        }
        if keyboard.current_focus().as_ref() != Some(&menu) {
            tracing::debug!(
                target: "aizome::focus",
                from = ?keyboard.current_focus().map(|f| f.0.id()),
                to = ?menu.0.id(),
                grabbed = keyboard.is_grabbed(),
                "keyboard focus to the topmost popup"
            );
            keyboard.set_focus(self, Some(menu), SERIAL_COUNTER.next_serial());
        }
        true
    }

    /// Closes the popups of the grab in force (popup_done to each) and
    /// lets go of the keyboard; the pointer grab ends once the client has
    /// destroyed them (`check_popup_grab`).
    pub fn dismiss_popup_grab(&mut self) {
        let Some(g) = self.az.popup_grab.as_mut() else { return };
        if g.dismissed {
            return;
        }
        g.dismissed = true;
        let _ = g.grab.ungrab(PopupUngrabStrategy::All);
        tracing::debug!(target: "aizome::focus", "popup grab dismissed");
        let Some(keyboard) = self.az.seat.get_keyboard() else { return };
        if keyboard.is_grabbed() && self.az.popup_grab.as_ref().is_some_and(|g| g.owns_serial(|s| keyboard.has_grab(s)))
        {
            keyboard.unset_grab(self);
        }
    }

    /// Once a popup grab has ended (its popups destroyed), its keyboard and
    /// pointer grabs go and the keyboard returns to what should have it
    /// (not necessarily the grab's root: a menu of the menu bar opened by a
    /// shortcut gives the keys back to the window that had them). While it
    /// lasts, the keys follow its topmost popup, past an input method
    /// taking and letting go of the keyboard (`keep_keys_in_menu`).
    pub fn check_popup_grab(&mut self) {
        if self.keep_keys_in_menu() {
            return;
        }
        let Some(g) = self.az.popup_grab.as_ref() else { return };
        let ended = g.grab.has_ended();
        // Closed by smithay rather than Aizome: the keyboard is let go of
        // as when Aizome closes it, and goes where it should.
        if !g.dismissed && !ended {
            self.dismiss_popup_grab();
            self.update_keyboard_focus();
            return;
        }
        if !ended {
            return;
        }
        let Some(g) = self.az.popup_grab.take() else { return };
        tracing::debug!(target: "aizome::focus", "popup grab ended");
        // A layer surface clicked to open its menu (the bar, for a menu or
        // a systray panel) doesn't keep the keys once the menu has gone:
        // they go back to the focused window.
        if self.az.layer_focus.as_ref() == Some(&g.root) {
            self.az.layer_focus = None;
        }
        if let Some(keyboard) = self.az.seat.get_keyboard()
            && keyboard.is_grabbed()
            && g.owns_serial(|s| keyboard.has_grab(s))
        {
            keyboard.unset_grab(self);
        }
        if let Some(pointer) = self.az.seat.get_pointer()
            && pointer.is_grabbed()
            && g.owns_serial(|s| pointer.has_grab(s))
        {
            let time = self.az.start.elapsed().as_millis() as u32;
            pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), time);
        }
        self.update_keyboard_focus();
    }

    /// A press outside the client whose menus are open (a grabbing popup)
    /// closes them; the press then does what a press there does (focusing
    /// a window, say), rather than only closing them. `under` is the client
    /// surface under the pointer, if any.
    pub fn press_outside_menus(&mut self, under: Option<&WlSurface>, serial: Serial, time: u32) {
        let Some(root) = self.az.popup_grab.as_ref().filter(|g| g.live()).map(|g| g.root.clone()) else { return };
        let same_client = under.is_some_and(|s| s.id().same_client_as(&root.id()));
        if same_client {
            return;
        }
        self.dismiss_popup_grab();
        let Some(pointer) = self.az.seat.get_pointer() else { return };
        if pointer.is_grabbed() && self.az.popup_grab.as_ref().is_some_and(|g| g.owns_serial(|s| pointer.has_grab(s))) {
            pointer.unset_grab(self, serial, time);
        }
    }

    /// Sets a pointer grab made by the logic, guarded: a panic in it is
    /// caught inside the grab (it can't unwind through smithay's locks),
    /// and the core can end it at a reload.
    pub fn set_logic_pointer_grab<G: PointerGrab<State> + 'static>(
        &mut self,
        grab: G,
        serial: Serial,
        focus: smithay::input::pointer::Focus,
    ) {
        let Some(pointer) = self.az.seat.get_pointer() else { return };
        pointer.set_grab(self, Guarded { inner: grab, failed: false }, serial, focus);
        if pointer.has_grab(serial) {
            self.lg.pointer_grab = Some(serial);
        }
    }
}

impl Aizome {
    /// What takes the keyboard from everything else: the lock surface while
    /// locked, else the topmost mapped layer surface on the overlay or top
    /// layer with exclusive keyboard interactivity.
    pub fn exclusive_keyboard_target(&self) -> Option<Option<WlSurface>> {
        // Ctrl+Alt+Del's overlay has the keys while it shows.
        if self.recovery.is_open() {
            return Some(None);
        }
        if self.lock.is_locked() {
            let out = self.active_output();
            let lock = out
                .as_ref()
                .and_then(|o| self.out.get(o))
                .and_then(|o| o.lock_surface.clone())
                .or_else(|| self.out.values().find_map(|o| o.lock_surface.clone()));
            return Some(lock.map(|l| l.wl_surface().clone()));
        }
        // The active output's first, then the others.
        let active = self.active_output();
        let outputs = active.iter().chain(self.outputs.iter().filter(|o| Some(*o) != active.as_ref()));
        for o in outputs {
            let map = layer_map_for_output(o);
            for layer in [Layer::Overlay, Layer::Top] {
                for l in map.layers_on(layer).rev() {
                    if l.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive && layer_mapped(l) {
                        return Some(Some(l.wl_surface().clone()));
                    }
                }
            }
        }
        None
    }

    /// The surface that should have the keyboard: the lock surface, an
    /// exclusive layer surface, the root of a popup grab (the grab hands
    /// the keys on to its topmost popup), a layer surface clicked on, or
    /// the focused window.
    pub fn keyboard_target(&self) -> Option<WlSurface> {
        if let Some(t) = self.exclusive_keyboard_target() {
            return t;
        }
        let overlay_keys = self.view.grabs_keyboard;
        if let Some(g) = &self.popup_grab
            && g.live()
            && !overlay_keys
        {
            return Some(g.root.clone());
        }
        if let Some(l) = &self.layer_focus {
            let still = self.outputs.iter().any(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(l, WindowSurfaceType::TOPLEVEL)
                    .is_some_and(|ls| ls.cached_state().keyboard_interactivity != KeyboardInteractivity::None)
            });
            if still {
                return Some(l.clone());
            }
        }
        if overlay_keys {
            return None;
        }
        let id = self.view.focused?;
        let m = self.windows.get(&id)?;
        if !m.view.shown {
            return None;
        }
        Some(m.surface().clone())
    }
}

/// A layer surface is mapped once it has a buffer (a null one unmaps it).
pub fn layer_mapped(l: &LayerSurface) -> bool {
    with_renderer_surface_state(l.wl_surface(), |s| s.buffer().is_some()).unwrap_or(false)
}

/// A pointer grab of the logic's, run under `catch_unwind`: smithay holds
/// the pointer's lock while a grab runs, so a panic must not unwind
/// through it (the lock would be poisoned and the pointer dead). A grab
/// that panics marks the logic for rollback and lets go.
pub struct Guarded<G> {
    inner: G,
    failed: bool,
}

macro_rules! guard {
    ($self:ident, $data:ident, $handle:ident, $call:expr, $unset:expr) => {{
        if $self.failed {
            return;
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $call));
        if let Err(p) = r {
            $self.failed = true;
            let message = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic in a pointer grab".into());
            $data.mark_logic_panicked(message);
            #[allow(clippy::redundant_closure_call)]
            ($unset)($self, $data, $handle);
        }
    }};
}

fn unset_now<G: PointerGrab<State> + 'static>(
    g: &mut Guarded<G>,
    data: &mut State,
    handle: &mut PointerInnerHandle<'_, State>,
) {
    let time = data.az.start.elapsed().as_millis() as u32;
    handle.unset_grab(g, data, SERIAL_COUNTER.next_serial(), time, true);
}

fn keep<G>(_: &mut Guarded<G>, _: &mut State, _: &mut PointerInnerHandle<'_, State>) {}

impl<G: PointerGrab<State> + 'static> PointerGrab<State> for Guarded<G> {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        focus: Option<(Focus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        guard!(self, data, handle, self.inner.motion(data, handle, focus, event), unset_now)
    }
    fn relative_motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        focus: Option<(Focus, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        guard!(self, data, handle, self.inner.relative_motion(data, handle, focus, event), unset_now)
    }
    fn button(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, event: &ButtonEvent) {
        guard!(self, data, handle, self.inner.button(data, handle, event), unset_now)
    }
    fn axis(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, details: AxisFrame) {
        guard!(self, data, handle, self.inner.axis(data, handle, details), unset_now)
    }
    fn frame(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>) {
        guard!(self, data, handle, self.inner.frame(data, handle), keep)
    }
    fn gesture_swipe_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeBeginEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_swipe_begin(data, handle, event), keep)
    }
    fn gesture_swipe_update(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeUpdateEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_swipe_update(data, handle, event), keep)
    }
    fn gesture_swipe_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeEndEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_swipe_end(data, handle, event), keep)
    }
    fn gesture_pinch_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchBeginEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_pinch_begin(data, handle, event), keep)
    }
    fn gesture_pinch_update(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchUpdateEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_pinch_update(data, handle, event), keep)
    }
    fn gesture_pinch_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchEndEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_pinch_end(data, handle, event), keep)
    }
    fn gesture_hold_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureHoldBeginEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_hold_begin(data, handle, event), keep)
    }
    fn gesture_hold_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureHoldEndEvent,
    ) {
        guard!(self, data, handle, self.inner.gesture_hold_end(data, handle, event), keep)
    }
    fn start_data(&self) -> &GrabStartData<State> {
        self.inner.start_data()
    }
    fn unset(&mut self, data: &mut State) {
        data.lg.pointer_grab = None;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.inner.unset(data)));
        if r.is_err() {
            data.mark_logic_panicked("a panic ending a pointer grab".into());
        }
    }
}
