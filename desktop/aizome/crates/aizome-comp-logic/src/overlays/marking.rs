//! The marking menu on a tab's right button (C4-Marking in Aizome's
//! look): eight verbs around the pointer. Press and pause and the menu
//! shows; flick without pausing and the verb runs with nothing drawn.
//! "to Desk ▸" opens a second ring of Desk names. A quick right-click
//! (or a client's window-menu request) leaves the menu up until the next
//! click.

use std::f64::consts::PI;
use std::time::{Duration, Instant};

use smithay::input::keyboard::{Keysym, keysyms};
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, Focus as PointerFocusMode, GestureHoldBeginEvent, GestureHoldEndEvent,
    GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent,
    GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab, PointerInnerHandle,
    RelativeMotionEvent,
};
use smithay::output::Output;
use smithay::utils::{Logical, Point, Serial};

use crate::decor::DecorCache;
use crate::prelude::*;
use crate::wm::{DeskId, Side, WinId};
use aizome_comp_core::focus::Focus;
use aizome_comp_core::logic::KeyDecision;
use aizome_comp_core::painter::{Box2, Painter, mono};

/// The pause before the menu shows.
pub const PAUSE: Duration = Duration::from_millis(250);
/// How far a flick must go.
pub const FLICK: f64 = 36.0;
/// Where the verbs sit from the centre.
pub const RADIUS: f64 = 96.0;
/// Inside this, nothing is picked.
pub const DEAD: f64 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    SplitRight,
    Stack,
    Park,
    Close,
    SplitLeft,
    Float,
    Fill,
    ToDesk,
}

impl Verb {
    /// Clockwise from east, as screen angles (y down) go.
    pub const ALL: [Verb; 8] = [
        Verb::SplitRight,
        Verb::Stack,
        Verb::Park,
        Verb::Close,
        Verb::SplitLeft,
        Verb::Float,
        Verb::Fill,
        Verb::ToDesk,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Verb::Fill => "fill",
            Verb::ToDesk => "to Desk ▸",
            Verb::SplitRight => "split →",
            Verb::Stack => "stack",
            Verb::Park => "park",
            Verb::Close => "close",
            Verb::SplitLeft => "← split",
            Verb::Float => "float",
        }
    }

    /// The verb in the direction of (dx, dy).
    pub fn toward(dx: f64, dy: f64) -> Verb {
        let a = dy.atan2(dx); // −π..π, 0 east, π/2 south
        let i = ((a / (PI / 4.0)).round() as i64).rem_euclid(8) as usize;
        Verb::ALL[i]
    }

    fn angle(self) -> f64 {
        Verb::ALL.iter().position(|v| *v == self).unwrap() as f64 * PI / 4.0
    }
}

pub struct Marking {
    /// Which opening this is (for the pause's timer).
    pub serial: u32,
    pub win: WinId,
    pub center: (f64, f64),
    pub opened: Instant,
    /// Drawn: after the pause, or opened by a click or a client's request.
    pub shown: bool,
    /// Stays up until the next click (not a press-drag-release).
    pub sticky: bool,
    pub pos: (f64, f64),
    /// The Desk ring is open.
    pub sub: bool,
    pub hover: Option<Verb>,
    pub desk_hover: Option<DeskId>,
    pub desk_boxes: Vec<(DeskId, Box2)>,
    pub output: Option<Output>,
    pub cache: DecorCache,
}

/// Opens the menu around a point: drawn at once when `shown` (a client's
/// window-menu request), else after the pause unless flicked.
pub fn open(state: &mut State, win: WinId, at: (f64, f64), shown: bool) {
    let output = state.az.output_at(Point::from(at));
    let opened = Instant::now();
    state.lg.m().overlays.marking = Some(Marking {
        serial: 0,
        win,
        center: at,
        opened,
        shown,
        sticky: shown,
        pos: at,
        sub: false,
        hover: None,
        desk_hover: None,
        desk_boxes: Vec::new(),
        output: output.clone(),
        cache: DecorCache::default(),
    });
    if let Some(o) = output {
        state.az.queue_redraw(&o);
    }
    if shown {
        keep_on_output(state);
    }
    if !shown {
        // Show after the pause, if still held and not flicked.
        let n = {
            let l = state.lg.m();
            l.overlays.marking_opened = l.overlays.marking_opened.wrapping_add(1);
            l.overlays.marking_opened
        };
        if let Some(m) = &mut state.lg.m().overlays.marking {
            m.serial = n;
        }
        state.schedule(PAUSE, crate::hooks::TIMER_MARKING | n as u64);
    }
}

/// The pause is over: the menu shows, if it's the same one, still held
/// and not flicked.
pub fn pause_over(state: &mut State, serial: u64) {
    if let Some(m) = &mut state.lg.m().overlays.marking
        && m.serial as u64 == serial
        && !m.shown
    {
        m.shown = true;
        let o = m.output.clone();
        if let Some(o) = o {
            state.az.queue_redraw(&o);
        }
        keep_on_output(state);
    }
}

/// The right button went down on a tab.
pub fn press(state: &mut State, win: WinId, pos: Point<f64, Logical>, serial: Serial, _time: u32) {
    open(state, win, (pos.x, pos.y), false);
    let grab = MarkingGrab {
        start: GrabStartData { focus: None, button: aizome_comp_core::input::BTN_RIGHT, location: pos },
        moved: false,
    };
    state.set_logic_pointer_grab(grab, serial, PointerFocusMode::Clear);
}

fn close(state: &mut State) {
    if let Some(m) = state.lg.m().overlays.marking.take()
        && let Some(o) = m.output
    {
        state.az.queue_redraw(&o);
    }
}

/// Runs the picked verb (or Desk).
fn pick(state: &mut State) {
    let Some(m) = state.lg.m().overlays.marking.take() else { return };
    if let Some(o) = &m.output {
        state.az.queue_redraw(o);
    }
    let w = m.win;
    if !state.lg.m().wm.contains(w) {
        return;
    }
    if let Some(d) = m.desk_hover {
        state.move_to_desk(w, d, false);
        return;
    }
    run(state, w, m.hover);
}

pub fn run(state: &mut State, w: WinId, verb: Option<Verb>) {
    match verb {
        Some(Verb::Fill) => state.set_fill(w, None),
        Some(Verb::Park) => state.minimise(w),
        Some(Verb::SplitLeft) => state.set_split(w, Side::Left),
        Some(Verb::SplitRight) => state.set_split(w, Side::Right),
        Some(Verb::Close) => state.close_window(w),
        Some(Verb::Stack) => state.stack_with_behind(w),
        Some(Verb::Float) => state.set_float(w, None),
        Some(Verb::ToDesk) | None => {}
    }
}

/// Where the pointer is now: hover, or a flick.
pub fn moved(state: &mut State, x: f64, y: f64) {
    let Some(m) = state.lg.m().overlays.marking.as_mut() else { return };
    m.pos = (x, y);
    let (dx, dy) = (x - m.center.0, y - m.center.1);
    let dist = (dx * dx + dy * dy).sqrt();
    if !m.shown && !m.sticky {
        if dist >= FLICK {
            let v = Verb::toward(dx, dy);
            if v == Verb::ToDesk {
                // The second flick picks a Desk: show the ring.
                m.shown = true;
                m.sub = true;
                m.hover = Some(v);
            } else {
                m.hover = Some(v);
                pick(state);
                return;
            }
        }
    } else {
        // Desk chips take precedence once the ring is open.
        m.desk_hover = if m.sub { m.desk_boxes.iter().find(|(_, b)| b.contains(x, y)).map(|(d, _)| *d) } else { None };
        if m.desk_hover.is_none() {
            m.hover = if dist < DEAD { None } else { Some(Verb::toward(dx, dy)) };
            if m.hover == Some(Verb::ToDesk) && dist > RADIUS * 0.6 {
                m.sub = true;
            }
        }
    }
    if let Some(o) = m.output.clone() {
        state.az.queue_redraw(&o);
    }
}

pub struct MarkingGrab {
    start: GrabStartData<State>,
    moved: bool,
}

impl PointerGrab<State> for MarkingGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _f: Option<(Focus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let d = event.location - self.start.location;
        if d.x.abs() > 3.0 || d.y.abs() > 3.0 {
            self.moved = true;
        }
        moved(data, event.location.x, event.location.y);
        if data.lg.m().overlays.marking.is_none() {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn button(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>, event: &ButtonEvent) {
        handle.button(data, event);
        let pressed = event.state == smithay::backend::input::ButtonState::Pressed;
        let Some(m) = data.lg.m().overlays.marking.as_mut() else {
            handle.unset_grab(self, data, event.serial, event.time, true);
            return;
        };
        if !pressed && handle.current_pressed().is_empty() {
            if !self.moved && !m.sticky && m.hover.is_none() {
                // A quick right-click: the menu stays up until the next click.
                m.shown = true;
                m.sticky = true;
                if let Some(o) = m.output.clone() {
                    data.az.queue_redraw(&o);
                }
                return;
            }
            if m.sticky && !self.moved && m.hover.is_none() && m.desk_hover.is_none() {
                return;
            }
            if m.hover == Some(Verb::ToDesk) && m.desk_hover.is_none() {
                // Opened the ring: wait for a Desk.
                m.sub = true;
                m.sticky = true;
                self.moved = false;
                return;
            }
            pick(data);
            handle.unset_grab(self, data, event.serial, event.time, true);
        } else if pressed && m.sticky {
            self.moved = false;
            let loc = handle.current_location();
            moved(data, loc.x, loc.y);
            if data.lg.m().overlays.marking.as_ref().is_some_and(|m| m.hover.is_none() && m.desk_hover.is_none()) {
                close(data);
                handle.unset_grab(self, data, event.serial, event.time, true);
            }
        }
    }

    fn relative_motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _f: Option<(Focus, Point<f64, Logical>)>,
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
    fn start_data(&self) -> &GrabStartData<State> {
        &self.start
    }
    fn unset(&mut self, data: &mut State) {
        if data.lg.m().overlays.marking.as_ref().is_some_and(|m| !m.sticky || !m.shown) {
            close(data);
        }
    }
}

/// Esc closes the menu.
pub fn key(state: &mut State, syms: &[Keysym], pressed: bool) -> Option<KeyDecision> {
    state.lg.m().overlays.marking.as_ref()?;
    if pressed && syms.iter().any(|s| s.raw() == keysyms::KEY_Escape) {
        close(state);
    }
    Some(KeyDecision::Swallow)
}

pub fn items(st: &mut State, output: &Output, scale: f64) -> aizome_comp_core::scene::Scene {
    let (az, l) = (&mut st.az, st.lg.m());
    let Some(mut m) = l.overlays.marking.take() else { return Vec::new() };
    if m.output.as_ref() != Some(output) || !m.shown {
        l.overlays.marking = Some(m);
        return Vec::new();
    }
    let geo = az.output_geometry(output);
    let origin = (geo.loc.x as f64, geo.loc.y as f64);
    let mut pt: Painter = Painter::new(origin, scale, 1.0);
    let p = az.palette;
    let shadows = az.settings.appearance.effects.shadows();
    let (cx, cy) = m.center;

    // A dotted ring, and the trail from the centre to the pointer.
    for i in 0..48 {
        let a = i as f64 * 2.0 * PI / 48.0;
        let r = RADIUS + 44.0;
        pt.fill(Box2::new((cx + r * a.cos()).round() - 1.0, (cy + r * a.sin()).round() - 1.0, 2.0, 2.0), p.line2);
    }
    let (dx, dy) = (m.pos.0 - cx, m.pos.1 - cy);
    let dist = (dx * dx + dy * dy).sqrt();
    if dist > DEAD {
        let steps = (dist / 5.0) as usize;
        for i in 1..steps {
            let t = i as f64 / steps as f64;
            pt.fill(Box2::new((cx + dx * t).round() - 1.0, (cy + dy * t).round() - 1.0, 2.0, 2.0), p.focus);
        }
        pt.fill(Box2::new(m.pos.0.round() - 3.0, m.pos.1.round() - 3.0, 6.0, 6.0), p.focus);
    }
    // The centre.
    let centre = Box2::new(cx - 28.0, cy - 11.0, 56.0, 22.0);
    pt.panel(centre, p.chrome, &p, shadows);
    pt.text_center(&mut az.labels, "window", mono(10.5, p.text3), centre);
    // The verbs.
    let mut todesk_box = None;
    for v in Verb::ALL {
        let a = v.angle();
        let label = v.label();
        let (tw, _) = az.labels.measure(label, aizome_comp_core::text::Face::Mono, 12.0);
        let w = tw.ceil() + 20.0;
        let (vx, vy) = (cx + RADIUS * a.cos(), cy + RADIUS * a.sin());
        let b = Box2::new((vx - w / 2.0).round(), (vy - 12.0).round(), w, 24.0);
        let hot = m.hover == Some(v) && m.desk_hover.is_none();
        let (fill, ink) = if hot { (p.selection, p.on_selection) } else { (p.raised, p.text) };
        pt.panel(b, fill, &p, shadows);
        pt.text_center(&mut az.labels, label, mono(12.0, ink), b);
        if v == Verb::ToDesk {
            todesk_box = Some(b);
        }
    }
    // The Desk ring: a column beside "to Desk ▸".
    m.desk_boxes.clear();
    if m.sub
        && let Some(tb) = todesk_box
    {
        let desks: Vec<(DeskId, String)> = l.wm.desks().iter().map(|d| (d.id, d.name.clone())).collect();
        let current = l.wm.get(m.win).map(|w| w.desk);
        let n = desks.len() as f64;
        let mut y = (tb.y + tb.h / 2.0 - n * 28.0 / 2.0).round();
        let x = tb.right() + 24.0;
        for (i, (d, name)) in desks.iter().enumerate() {
            let label = format!("{} {}", i + 1, name);
            let (tw, _) = az.labels.measure(&label, aizome_comp_core::text::Face::Mono, 12.0);
            let b = Box2::new(x, y, tw.ceil() + 24.0, 24.0);
            let hot = m.desk_hover == Some(*d);
            let (fill, ink) = if hot {
                (p.selection, p.on_selection)
            } else if current == Some(*d) {
                (p.sunk, p.text3)
            } else {
                (p.raised, p.text)
            };
            pt.panel(b, fill, &p, shadows);
            pt.text_center(&mut az.labels, &label, mono(12.0, ink), b);
            m.desk_boxes.push((*d, b));
            y += 28.0;
        }
    }
    let out = pt.finish(&mut m.cache);
    l.overlays.marking = Some(m);
    out
}

/// A click while a sticky menu (opened by a client's window-menu request)
/// is up: picks what's under the pointer, or closes it.
pub fn click(state: &mut State, x: f64, y: f64) {
    moved(state, x, y);
    if state.lg.m().overlays.marking.as_ref().is_some_and(|m| m.hover.is_some() || m.desk_hover.is_some()) {
        if state
            .lg
            .m()
            .overlays
            .marking
            .as_ref()
            .is_some_and(|m| m.hover == Some(Verb::ToDesk) && m.desk_hover.is_none())
        {
            if let Some(m) = &mut state.lg.m().overlays.marking {
                m.sub = true;
            }
            return;
        }
        pick(state);
    } else {
        close(state);
    }
}

/// A menu opened near an output's edge (a tab under the menu bar) moves
/// in so all its verbs show, and the pointer moves with it.
fn keep_on_output(state: &mut State) {
    let Some(o) = state.lg.m().overlays.marking.as_ref().and_then(|m| m.output.clone()) else { return };
    let g = state.az.output_geometry(&o).to_f64();
    let Some(m) = state.lg.m().overlays.marking.as_mut() else { return };
    let reach = RADIUS + 48.0;
    let cx = m.center.0.clamp(g.loc.x + reach, (g.loc.x + g.size.w - reach).max(g.loc.x + reach));
    let cy = m.center.1.clamp(g.loc.y + reach, (g.loc.y + g.size.h - reach).max(g.loc.y + reach));
    if (cx, cy) == m.center {
        return;
    }
    let (dx, dy) = (cx - m.center.0, cy - m.center.1);
    m.center = (cx, cy);
    m.pos = (m.pos.0 + dx, m.pos.1 + dy);
    let p = Point::from(m.pos);
    state.az.pointer_pos = p;
    if let Some(pointer) = state.az.seat.get_pointer() {
        pointer.set_location(p);
    }
    state.az.queue_redraw(&o);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions() {
        assert_eq!(Verb::toward(0.0, -50.0), Verb::Fill);
        assert_eq!(Verb::toward(0.0, 50.0), Verb::Park);
        assert_eq!(Verb::toward(-50.0, 0.0), Verb::SplitLeft);
        assert_eq!(Verb::toward(50.0, 0.0), Verb::SplitRight);
        assert_eq!(Verb::toward(40.0, -40.0), Verb::ToDesk);
        assert_eq!(Verb::toward(40.0, 40.0), Verb::Stack);
        assert_eq!(Verb::toward(-40.0, 40.0), Verb::Close);
        assert_eq!(Verb::toward(-40.0, -40.0), Verb::Float);
    }
}
