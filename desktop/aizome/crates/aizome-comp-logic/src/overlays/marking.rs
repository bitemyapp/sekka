//! The marking menu on a tab's right button (C4-Marking in Aizome's
//! look): eight verbs around the pointer. Press and pause and the menu
//! shows; flick without pausing and the verb runs with nothing drawn.
//! "to Desk ▸" opens a second ring of Desk names. A quick right-click
//! (or a client's window-menu request) leaves the menu up until the next
//! click. ⌃❖Space opens it around the focused window's tab, showing its
//! keys: the arrows pick the verbs that way, letters the others, digits
//! a Desk.

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

    /// Its key, when the menu came from the keyboard: an arrow the way it
    /// lies, or the first letter for those between. The splits' labels
    /// show their arrows already.
    fn hint(self) -> Option<&'static str> {
        match self {
            Verb::Fill => Some("↑"),
            Verb::Park => Some("↓"),
            Verb::SplitLeft | Verb::SplitRight => None,
            Verb::Stack => Some("S"),
            Verb::Close => Some("C"),
            Verb::Float => Some("F"),
            Verb::ToDesk => Some("D"),
        }
    }

    /// The verb a key picks.
    pub fn for_key(syms: &[Keysym]) -> Option<Verb> {
        syms.iter().find_map(|s| match s.raw() {
            keysyms::KEY_Up => Some(Verb::Fill),
            keysyms::KEY_Down => Some(Verb::Park),
            keysyms::KEY_Left => Some(Verb::SplitLeft),
            keysyms::KEY_Right => Some(Verb::SplitRight),
            keysyms::KEY_s | keysyms::KEY_S => Some(Verb::Stack),
            keysyms::KEY_c | keysyms::KEY_C => Some(Verb::Close),
            keysyms::KEY_f | keysyms::KEY_F => Some(Verb::Float),
            keysyms::KEY_d | keysyms::KEY_D => Some(Verb::ToDesk),
            _ => None,
        })
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
    /// Opened from the keyboard: shows the keys, and the pointer picks
    /// only what it's over (a click elsewhere closes it).
    pub keys: bool,
    pub pos: (f64, f64),
    /// The Desk ring is open.
    pub sub: bool,
    pub hover: Option<Verb>,
    pub desk_hover: Option<DeskId>,
    pub desk_boxes: Vec<(DeskId, Box2)>,
    pub verb_boxes: Vec<(Verb, Box2)>,
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
        keys: false,
        pos: at,
        sub: false,
        hover: None,
        desk_hover: None,
        desk_boxes: Vec::new(),
        verb_boxes: Vec::new(),
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

/// Opens the menu from the keyboard (⌃❖Space) around the window's tab,
/// or its middle when it shows none, moved in from the display's edges.
/// The pointer stays where it is.
pub fn open_from_keys(state: &mut State, win: WinId) {
    let Some(rect) = state.lg.r().wm.get(win).map(|w| w.rect) else { return };
    let r = state.lg.r().decor_tab_rect(&mut state.az.labels, win).unwrap_or(rect);
    let at = (r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0);
    let output = state.az.output_at(Point::from(at));
    let center = match &output {
        Some(o) => on_output(state.az.output_geometry(o).to_f64(), at),
        None => at,
    };
    state.lg.m().overlays.marking = Some(Marking {
        serial: 0,
        win,
        center,
        opened: Instant::now(),
        shown: true,
        sticky: true,
        keys: true,
        pos: center,
        sub: false,
        hover: None,
        desk_hover: None,
        desk_boxes: Vec::new(),
        verb_boxes: Vec::new(),
        output: output.clone(),
        cache: DecorCache::default(),
    });
    if let Some(o) = output {
        state.az.queue_redraw(&o);
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
    if m.keys {
        // The pointer wasn't at the centre: only what it's over counts.
        m.desk_hover = if m.sub { m.desk_boxes.iter().find(|(_, b)| b.contains(x, y)).map(|(d, _)| *d) } else { None };
        m.hover = m.verb_boxes.iter().find(|(_, b)| b.contains(x, y)).map(|(v, _)| *v);
        if let Some(o) = m.output.clone() {
            state.az.queue_redraw(&o);
        }
        return;
    }
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
                if let Some(location) = fit_on_output(data) {
                    // We already hold the pointer lock in this grab callback.
                    handle.motion(data, None, &MotionEvent { location, serial: event.serial, time: event.time });
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

/// Esc (or Space, as ⌃❖Space again) closes the menu; an arrow or a verb's
/// letter picks it, and with the Desk ring open, a digit picks that Desk.
pub fn key(state: &mut State, syms: &[Keysym], pressed: bool) -> Option<KeyDecision> {
    let sub = state.lg.r().overlays.marking.as_ref()?.sub;
    if !pressed {
        return Some(KeyDecision::Swallow);
    }
    if syms.iter().any(|s| matches!(s.raw(), keysyms::KEY_Escape | keysyms::KEY_space)) {
        close(state);
        return Some(KeyDecision::Swallow);
    }
    if sub {
        let n = syms.iter().map(|s| s.raw()).find(|r| (keysyms::KEY_1..=keysyms::KEY_9).contains(r));
        if let Some(d) = n.and_then(|n| state.lg.r().wm.desk_at(n - keysyms::KEY_0)) {
            if let Some(m) = state.lg.m().overlays.marking.as_mut() {
                m.desk_hover = Some(d);
            }
            pick(state);
        }
        return Some(KeyDecision::Swallow);
    }
    let Some(v) = Verb::for_key(syms) else { return Some(KeyDecision::Swallow) };
    let Some(m) = state.lg.m().overlays.marking.as_mut() else { return Some(KeyDecision::Swallow) };
    m.hover = Some(v);
    if v == Verb::ToDesk {
        m.sub = true;
        if let Some(o) = m.output.clone() {
            state.az.queue_redraw(&o);
        }
    } else {
        pick(state);
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
    m.verb_boxes.clear();
    for v in Verb::ALL {
        let a = v.angle();
        let label = v.label();
        let hint = v.hint().filter(|_| m.keys);
        let (tw, _) = az.labels.measure(label, aizome_comp_core::text::Face::Mono, 12.0);
        let hw = hint.map_or(0.0, |h| az.labels.measure(h, aizome_comp_core::text::Face::Mono, 12.0).0.ceil() + 8.0);
        let w = tw.ceil() + hw + 20.0;
        let (vx, vy) = (cx + RADIUS * a.cos(), cy + RADIUS * a.sin());
        let b = Box2::new((vx - w / 2.0).round(), (vy - 12.0).round(), w, 24.0);
        let hot = m.hover == Some(v) && m.desk_hover.is_none();
        let (fill, ink) = if hot { (p.selection, p.on_selection) } else { (p.raised, p.text) };
        pt.panel(b, fill, &p, shadows);
        match hint {
            Some(h) => {
                // The key after the label, quieter.
                let lb = Box2::new(b.x + 10.0, b.y, tw.ceil(), b.h);
                pt.text_center(&mut az.labels, label, mono(12.0, ink), lb);
                let hb = Box2::new(lb.right() + 8.0, b.y, hw - 8.0, b.h);
                pt.text_center(&mut az.labels, h, mono(12.0, if hot { ink } else { p.text3 }), hb);
            }
            None => {
                pt.text_center(&mut az.labels, label, mono(12.0, ink), b);
            }
        }
        m.verb_boxes.push((v, b));
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
    if let Some(pos) = fit_on_output(state)
        && let Some(pointer) = state.az.seat.get_pointer()
    {
        pointer.set_location(pos);
    }
}

/// Adjusts the menu and returns a pointer warp. Callers inside a pointer
/// grab apply the warp through their existing inner handle.
fn fit_on_output(state: &mut State) -> Option<Point<f64, Logical>> {
    let o = state.lg.r().overlays.marking.as_ref()?.output.clone()?;
    let g = state.az.output_geometry(&o).to_f64();
    let m = state.lg.m().overlays.marking.as_mut()?;
    let (cx, cy) = on_output(g, m.center);
    if (cx, cy) == m.center {
        return None;
    }
    let (dx, dy) = (cx - m.center.0, cy - m.center.1);
    m.center = (cx, cy);
    m.pos = (m.pos.0 + dx, m.pos.1 + dy);
    let p = Point::from(m.pos);
    state.az.pointer_pos = p;
    state.az.queue_redraw(&o);
    Some(p)
}

/// A centre moved in from a display's edges so every verb shows.
fn on_output(g: smithay::utils::Rectangle<f64, Logical>, (x, y): (f64, f64)) -> (f64, f64) {
    let reach = RADIUS + 48.0;
    let cx = x.clamp(g.loc.x + reach, (g.loc.x + g.size.w - reach).max(g.loc.x + reach));
    let cy = y.clamp(g.loc.y + reach, (g.loc.y + g.size.h - reach).max(g.loc.y + reach));
    (cx, cy)
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

    #[test]
    fn keys_go_the_way_the_verbs_lie() {
        let k = |raw| Verb::for_key(&[Keysym::new(raw)]);
        assert_eq!(k(keysyms::KEY_Up), Some(Verb::toward(0.0, -50.0)));
        assert_eq!(k(keysyms::KEY_Down), Some(Verb::toward(0.0, 50.0)));
        assert_eq!(k(keysyms::KEY_Left), Some(Verb::toward(-50.0, 0.0)));
        assert_eq!(k(keysyms::KEY_Right), Some(Verb::toward(50.0, 0.0)));
        // Letters for the rest, each the first of its label.
        for v in Verb::ALL {
            if let Some(h) = v.hint().filter(|h| h.chars().all(|c| c.is_ascii_alphabetic())) {
                let first = v.label().trim_start_matches("to ").chars().next().unwrap();
                assert!(h.eq_ignore_ascii_case(&first.to_string()), "{v:?}: {h}");
                assert_eq!(k(keysyms::KEY_a + (first.to_ascii_lowercase() as u32 - 'a' as u32)), Some(v));
            }
        }
        assert_eq!(k(keysyms::KEY_x), None);
    }
}
