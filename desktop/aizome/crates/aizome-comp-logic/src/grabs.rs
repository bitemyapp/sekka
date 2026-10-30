//! Pointer grabs: moving windows (by the tab, ⌃❖-drag or the client's
//! request), dragging tabs into and out of groups, resizing by the frame,
//! sliding a tab along the top edge (⇧-drag), and the close and zoom boxes.
//!
//! Each ends when the button that started it comes up, whatever else the
//! seat thinks is held and whatever any client is doing ([`ends`]), and
//! if that button is pressed again (its release went missing).

use smithay::backend::input::ButtonState;
use smithay::input::Seat;
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, CursorIcon, Focus as PointerFocusMode, GestureHoldBeginEvent, GestureHoldEndEvent,
    GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent,
    GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab, PointerInnerHandle,
    RelativeMotionEvent,
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::utils::{Logical, Point, Serial};

use crate::ResizeAnchor;
use crate::decor::{Edges, Hit};
use crate::input::{InputExt, TabDrag};
use crate::prelude::*;
use crate::wm::{Rect, WinId};
use aizome_comp_core::focus::Focus;

/// How far a background tab moves before it tears out of its group.
const TEAR: f64 = 8.0;

macro_rules! forward_gestures {
    () => {
        fn gesture_swipe_begin(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeBeginEvent,
        ) {
            handle.gesture_swipe_begin(data, event)
        }
        fn gesture_swipe_update(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeUpdateEvent,
        ) {
            handle.gesture_swipe_update(data, event)
        }
        fn gesture_swipe_end(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeEndEvent,
        ) {
            handle.gesture_swipe_end(data, event)
        }
        fn gesture_pinch_begin(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchBeginEvent,
        ) {
            handle.gesture_pinch_begin(data, event)
        }
        fn gesture_pinch_update(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchUpdateEvent,
        ) {
            handle.gesture_pinch_update(data, event)
        }
        fn gesture_pinch_end(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchEndEvent,
        ) {
            handle.gesture_pinch_end(data, event)
        }
        fn gesture_hold_begin(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureHoldBeginEvent,
        ) {
            handle.gesture_hold_begin(data, event)
        }
        fn gesture_hold_end(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureHoldEndEvent,
        ) {
            handle.gesture_hold_end(data, event)
        }
        fn relative_motion(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            _focus: Option<(Focus, Point<f64, Logical>)>,
            event: &RelativeMotionEvent,
        ) {
            handle.relative_motion(data, None, event);
        }
        fn axis(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, details: AxisFrame) {
            handle.axis(data, details)
        }
        fn frame(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>) {
            handle.frame(data)
        }
        fn start_data(&self) -> &GrabStartData<State> {
            &self.start
        }
    };
}

fn start_data(pos: Point<f64, Logical>, button: u32) -> GrabStartData<State> {
    GrabStartData { focus: None, button, location: pos }
}

/// Whether a button event ends a grab the compositor holds: the release of
/// the button that started it (or of the last button held), or a press of
/// that button, which means its release never came. Grabs never wait for
/// every button the seat may wrongly think is held, nor for any client.
pub fn ends(start: &GrabStartData<State>, handle: &PointerInnerHandle<'_, State>, event: &ButtonEvent) -> bool {
    let own = event.button == start.button;
    match event.state {
        ButtonState::Released => own || handle.current_pressed().is_empty(),
        ButtonState::Pressed => own,
    }
}

// ---- move --------------------------------------------------------------

pub struct MoveGrab {
    start: GrabStartData<State>,
    win: WinId,
    start_rect: Rect,
    /// Started on a tab: stacking and tearing out apply.
    tab: bool,
    /// A background tab, not yet torn out (a click shows it instead).
    pending_tear: bool,
    moved: bool,
}

pub fn start_move(
    state: &mut State,
    win: WinId,
    pos: Point<f64, Logical>,
    button: u32,
    serial: Serial,
    _time: u32,
    tab: bool,
) {
    let Some(w) = state.lg.m().wm.get(win) else { return };
    let rect = w.rect;
    let grouped = w.group.is_some();
    let active = state.lg.m().wm.is_group_active(win);
    // ⌥-drag tears the showing tab out of its group too.
    let alt = state.az.seat.get_keyboard().is_some_and(|k| k.modifier_state().alt);
    let grab = MoveGrab {
        start: start_data(pos, button),
        win,
        start_rect: rect,
        tab,
        pending_tear: tab && grouped && (!active || alt),
        moved: false,
    };
    if tab {
        state.lg.m().interactive.drag = Some(TabDrag { win, target: None, at: 0, slot: None });
    }
    state.az.cursor_override = Some(CursorIcon::Grabbing);
    state.set_logic_pointer_grab(grab, serial, PointerFocusMode::Clear);
}

impl PointerGrab<State> for MoveGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(Focus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let d = event.location - self.start.location;
        if !self.moved && d.x.abs() < 2.0 && d.y.abs() < 2.0 {
            return;
        }
        self.moved = true;
        if self.pending_tear {
            if d.x.abs() < TEAR && d.y.abs() < TEAR {
                return;
            }
            // Out of the group, following the pointer by its tab.
            self.pending_tear = false;
            let tab = data.lg.r().decor_tab_rect(&mut data.az.labels, self.win);
            data.unstack(self.win, Some((self.start_rect.x, self.start_rect.y)));
            if let (Some(t0), Some(t1)) = (tab, data.lg.r().decor_tab_rect(&mut data.az.labels, self.win)) {
                // Keep the grabbed point of the tab under the pointer.
                let dx = t0.x - t1.x;
                self.start_rect.x += dx;
            }
            if let Some(w) = data.lg.m().wm.get(self.win) {
                self.start_rect.w = w.rect.w;
                self.start_rect.h = w.rect.h;
            }
        }
        let mut x = self.start_rect.x + d.x.round() as i32;
        let mut y = self.start_rect.y + d.y.round() as i32;
        // The tab stays below the menu bar.
        let insets = data.lg.r().insets_for(self.win);
        let min_y = data.az.outputs.iter().map(|o| data.az.usable_area(o).y).min().unwrap_or(0) + insets.top;
        y = y.max(min_y);
        x = x.max(-self.start_rect.w + 48);
        data.move_window(self.win, x, y);
        if self.tab {
            update_drop_target(data, self.win, event.location);
        }
    }

    fn button(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, event: &ButtonEvent) {
        handle.button(data, event);
        if !ends(&self.start, handle, event) {
            return;
        }
        let drop = data.lg.m().interactive.drag.take();
        if event.state == ButtonState::Released {
            if self.pending_tear && !self.moved {
                // A click on a background tab shows it.
                data.show_group_member(self.win);
            }
            if let Some(TabDrag { target: Some(target), at, .. }) = drop {
                data.stack_at(target, self.win, Some(at));
            } else if self.moved {
                // Dropped on another display: it joins the Desk there.
                crate::actions::adopt_screen(data, self.win);
            }
        }
        data.az.cursor_override = None;
        data.az.queue_redraw_all();
        handle.unset_grab(self, data, event.serial, event.time, true);
    }

    fn unset(&mut self, data: &mut State) {
        // A strip parted for it closes up.
        if data.lg.m().interactive.drag.take().is_some_and(|d| d.slot.is_some()) {
            data.az.queue_redraw_all();
        }
        data.az.cursor_override = None;
    }

    forward_gestures!();
}

/// Which window's tabs a dragged tab hovers, and which of them it would go
/// before: the first whose middle is past the pointer, or after them all.
/// Their strip parts there for it (the slot).
fn update_drop_target(state: &mut State, win: WinId, pos: Point<f64, Logical>) {
    let dragged = state.lg.m().wm.group_members(win);
    let mut found = None;
    for id in state.lg.m().wm.shown_stacking().into_iter().rev() {
        if dragged.contains(&id) {
            continue;
        }
        // The tabs where they'd be without the drag, so parting them for
        // it doesn't move where it goes.
        let Some(l) = state.lg.r().decor_layout_unparted(&mut state.az.labels, id) else { continue };
        let strip = l.tabs.iter().fold(Rect::default(), |r, t| r.union(t.rect));
        // The strip, and a little beyond its end where the slot goes.
        let reach = Rect::new(strip.x, strip.y - 6, strip.w + 140, strip.h + 12);
        if reach.contains(pos.x, pos.y) {
            let at = l.tabs.iter().position(|t| pos.x < t.rect.x as f64 + t.rect.w as f64 / 2.0);
            found = Some((l.win, at.unwrap_or(l.tabs.len())));
            break;
        }
        // Windows above hide those below.
        if l.frame.grow(crate::decor::REACH).contains(pos.x, pos.y) {
            break;
        }
    }
    let Some(d) = state.lg.r().interactive.drag.as_ref() else { return };
    let (old, old_slot) = ((d.target, d.at), d.slot);
    let (target, at) = found.map_or((None, 0), |(t, at)| (Some(t), at));
    // The tabs that part for it or close up after it, where they are now.
    let mut moving = Vec::new();
    if old != (target, at) {
        for t in old.0.into_iter().chain(target) {
            for m in state.lg.r().wm.group_members(t) {
                if let Some(r) = state.lg.r().decor_tab_rect(&mut state.az.labels, m)
                    && !moving.iter().any(|(n, _)| *n == m)
                {
                    moving.push((m, r));
                }
            }
        }
    }
    if let Some(d) = &mut state.lg.m().interactive.drag {
        (d.target, d.at) = (target, at);
    }
    let slot = target.and_then(|t| state.lg.r().decor_layout(&mut state.az.labels, t)).and_then(|l| l.slot);
    if let Some(d) = &mut state.lg.m().interactive.drag {
        d.slot = slot;
    }
    // They slide there from where they show.
    let ms = state.az.motion_ms();
    for (m, r0) in moving {
        if let Some(r1) = state.lg.r().decor_tab_rect(&mut state.az.labels, m) {
            let anims = &mut state.lg.m().anims;
            let (dx, dy) = anims.tab_offset(m);
            anims.start_tab(m, (r0.x - r1.x) as f64 + dx, (r0.y - r1.y) as f64 + dy, ms);
        }
    }
    if old_slot != slot {
        for r in [old_slot, slot].into_iter().flatten() {
            state.az.queue_redraw_rect(r.grow(40));
        }
    }
}

// ---- resize ------------------------------------------------------------

pub struct ResizeGrab {
    start: GrabStartData<State>,
    win: WinId,
    edges: Edges,
    start_rect: Rect,
}

pub fn start_resize(
    state: &mut State,
    win: WinId,
    edges: Edges,
    pos: Point<f64, Logical>,
    button: u32,
    serial: Serial,
    _time: u32,
) {
    let Some(w) = state.lg.m().wm.get(win) else { return };
    if w.fullscreen() {
        return;
    }
    let start_rect = w.rect;
    for m in state.lg.m().wm.group_members(win) {
        if let Some(mm) = state.az.windows.get(&m) {
            mm.toplevel().with_pending_state(|s| s.states.set(xdg_toplevel::State::Resizing));
            state.lg.m().data(m).resize_anchor = Some(ResizeAnchor {
                left: edges.contains(Edges::LEFT),
                top: edges.contains(Edges::TOP),
                start: start_rect,
            });
        }
        if let Some(w) = state.lg.m().wm.get_mut(m) {
            // A size of its own now: where it was on another display goes.
            w.home = None;
            if w.placement != crate::wm::Placement::Free {
                w.placement = crate::wm::Placement::Free;
                w.restore = None;
            }
        }
    }
    let grab = ResizeGrab { start: start_data(pos, button), win, edges, start_rect };
    state.az.cursor_override = Some(edges.cursor());
    state.set_logic_pointer_grab(grab, serial, PointerFocusMode::Clear);
}

/// The size limits a client set.
pub fn size_limits(state: &State, win: WinId) -> ((i32, i32), (i32, i32)) {
    let Some(m) = state.az.windows.get(&win) else { return ((64, 32), (i32::MAX, i32::MAX)) };
    let (min, max) = smithay::wayland::compositor::with_states(m.surface(), |s| {
        let c = *s.cached_state.get::<smithay::wayland::shell::xdg::SurfaceCachedState>().current();
        (c.min_size, c.max_size)
    });
    let min = (min.w.max(64), min.h.max(32));
    let max = (if max.w > 0 { max.w } else { i32::MAX }, if max.h > 0 { max.h } else { i32::MAX });
    (min, max)
}

impl PointerGrab<State> for ResizeGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(Focus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let d = event.location - self.start.location;
        let (dx, dy) = (d.x.round() as i32, d.y.round() as i32);
        let r = self.start_rect;
        let (mut w, mut h) = (r.w, r.h);
        if self.edges.contains(Edges::LEFT) {
            w -= dx;
        } else if self.edges.contains(Edges::RIGHT) {
            w += dx;
        }
        if self.edges.contains(Edges::TOP) {
            h -= dy;
        } else if self.edges.contains(Edges::BOTTOM) {
            h += dy;
        }
        let (min, max) = size_limits(data, self.win);
        w = w.clamp(min.0, max.0);
        h = h.clamp(min.1, max.1);
        for m in data.lg.m().wm.group_members(self.win) {
            data.request_size(m, w, h);
            data.configure(m);
        }
    }

    fn button(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, event: &ButtonEvent) {
        handle.button(data, event);
        if !ends(&self.start, handle, event) {
            return;
        }
        handle.unset_grab(self, data, event.serial, event.time, true);
    }

    fn unset(&mut self, data: &mut State) {
        for m in data.lg.m().wm.group_members(self.win) {
            if let Some(mm) = data.az.windows.get(&m) {
                mm.toplevel().with_pending_state(|s| s.states.unset(xdg_toplevel::State::Resizing));
            }
            data.configure(m);
        }
        data.az.cursor_override = None;
    }

    forward_gestures!();
}

// ---- tab slide (⇧-drag) ------------------------------------------------

pub struct TabSlideGrab {
    start: GrabStartData<State>,
    win: WinId,
    start_offset: i32,
}

pub fn start_tab_slide(state: &mut State, win: WinId, pos: Point<f64, Logical>, serial: Serial, _time: u32) {
    // The offset the tab shows at (the layout clamps the stored one).
    let start_offset = state
        .lg
        .r()
        .decor_layout(&mut state.az.labels, win)
        .map(|l| l.tabs.first().map(|t| t.rect.x - l.frame.x).unwrap_or(0))
        .unwrap_or(0);
    let grab = TabSlideGrab { start: start_data(pos, aizome_comp_core::input::BTN_LEFT), win, start_offset };
    state.az.cursor_override = Some(CursorIcon::EwResize);
    state.set_logic_pointer_grab(grab, serial, PointerFocusMode::Clear);
}

impl PointerGrab<State> for TabSlideGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(Focus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let dx = (event.location.x - self.start.location.x).round() as i32;
        let offset = (self.start_offset + dx).max(0);
        for m in data.lg.m().wm.group_members(self.win) {
            if let Some(w) = data.lg.m().wm.get_mut(m) {
                w.tab_offset = offset;
            }
        }
        // Store what the layout will show, so it never drifts past the end.
        let shown = data
            .lg
            .r()
            .decor_layout(&mut data.az.labels, self.win)
            .and_then(|l| l.tabs.first().map(|t| t.rect.x - l.frame.x));
        if let Some(s) = shown {
            for m in data.lg.m().wm.group_members(self.win) {
                if let Some(w) = data.lg.m().wm.get_mut(m) {
                    w.tab_offset = s;
                }
            }
        }
        data.redraw_window(self.win);
    }

    fn button(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, event: &ButtonEvent) {
        handle.button(data, event);
        if ends(&self.start, handle, event) {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn unset(&mut self, data: &mut State) {
        data.az.cursor_override = None;
        data.lg.m().window_changed(self.win);
    }

    forward_gestures!();
}

// ---- close and zoom boxes ----------------------------------------------

pub struct BoxGrab {
    start: GrabStartData<State>,
    hit: Hit,
}

pub fn start_box_click(state: &mut State, hit: Hit, pos: Point<f64, Logical>, serial: Serial, _time: u32) {
    let grab = BoxGrab { start: start_data(pos, aizome_comp_core::input::BTN_LEFT), hit };
    state.set_logic_pointer_grab(grab, serial, PointerFocusMode::Clear);
}

impl PointerGrab<State> for BoxGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(Focus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
    }

    fn button(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, event: &ButtonEvent) {
        handle.button(data, event);
        if !ends(&self.start, handle, event) {
            return;
        }
        let pos = handle.current_location();
        handle.unset_grab(self, data, event.serial, event.time, true);
        // Only if released over the same box.
        let still = event.state == ButtonState::Released
            && matches!(data.under(pos), crate::input::Under::Decor(_, h) if h == self.hit);
        if still {
            match self.hit {
                Hit::Close(w) => data.close_window(w),
                Hit::Zoom(w) => data.set_fill(w, None),
                _ => {}
            }
        }
    }

    fn unset(&mut self, _data: &mut State) {}

    forward_gestures!();
}

// ---- requests from clients (client-side decorations) -----------------------

pub fn start_client_move(state: &mut State, seat: &Seat<State>, win: WinId, serial: Serial) {
    let Some(pointer) = seat.get_pointer() else { return };
    if !pointer.has_grab(serial) {
        return;
    }
    let Some(start) = pointer.grab_start_data() else { return };
    // The client's own title bar: a move like the tab's (stacking too).
    start_move(state, win, start.location, start.button, serial, 0, true);
}

pub fn start_client_resize(state: &mut State, seat: &Seat<State>, win: WinId, serial: Serial, edges: Edges) {
    let Some(pointer) = seat.get_pointer() else { return };
    if !pointer.has_grab(serial) {
        return;
    }
    let Some(start) = pointer.grab_start_data() else { return };
    start_resize(state, win, edges, start.location, start.button, serial, 0);
}

pub fn edges_from_xdg(e: xdg_toplevel::ResizeEdge) -> Edges {
    use xdg_toplevel::ResizeEdge as R;
    match e {
        R::Top => Edges::TOP,
        R::Bottom => Edges::BOTTOM,
        R::Left => Edges::LEFT,
        R::Right => Edges::RIGHT,
        R::TopLeft => Edges::TOP | Edges::LEFT,
        R::TopRight => Edges::TOP | Edges::RIGHT,
        R::BottomLeft => Edges::BOTTOM | Edges::LEFT,
        R::BottomRight => Edges::BOTTOM | Edges::RIGHT,
        _ => Edges::BOTTOM | Edges::RIGHT,
    }
}
