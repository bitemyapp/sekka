//! `pointer` in the kit engine's shape: `Win.hot`, `.hot_under`, `.active`,
//! `.press_under`, `.pointer` (engine.rs:132-151), and the handlers'
//! `if`s (engine.rs:1932-2265).

use crate::BUGS;
use spec::Sink;
use spec::pointer::{Button, Config, DragFrom, Effect, Event, Hit, Machine, Node, Point, StateName};

pub const IMPLEMENTED: bool = true;

/// The machine's fields, in the current code's shape. Readable through
/// the machine (`Deref`); only `handle` changes them.
pub struct Fields {
    pub config: Config,
    /// Where the pointer is; `None` outside the window.
    pub pointer: Option<Point>,
    pub hot: Option<Node>,
    pub hot_under: Option<Node>,
    /// What took the press (or a capture): it gets the moves and the release.
    pub active: Option<Node>,
    /// Whether `active` takes only drags (the engine asks its widget).
    pub active_drags_only: bool,
    /// A left press taken by a drags-only node: it, where, how many clicks,
    /// and what's under it, should no drag come of it.
    pub press_under: Option<(Node, Point, u8, Option<Node>)>,
    /// SPEC-FIX: the compositor owns the pointer (the engine has no flag;
    /// the platform just stops sending events).
    pub native_drag: bool,
}

impl Fields {
    fn update_hot(&mut self, hit: Hit, out: &mut impl Sink<Effect>) {
        let under = if hit.drags_only { hit.under } else { None };
        if under != self.hot_under {
            if let Some(o) = self.hot_under {
                out.push(Effect::UnderOff(o));
            }
            if let Some(n) = under {
                out.push(Effect::UnderOn(n));
            }
            self.hot_under = under;
        }
        if hit.node != self.hot {
            if let Some(o) = self.hot {
                out.push(Effect::HoverOff(o));
            }
            if let Some(n) = hit.node {
                out.push(Effect::HoverOn(n));
            }
            self.hot = hit.node;
        }
    }

    fn clear_hover(&mut self, out: &mut impl Sink<Effect>) {
        if let Some(u) = self.hot_under.take() {
            out.push(Effect::UnderOff(u));
        }
        if let Some(h) = self.hot.take() {
            out.push(Effect::HoverOff(h));
        }
    }
}

impl Fields {
    fn new(config: Config) -> Self {
        Self {
            config,
            pointer: None,
            hot: None,
            hot_under: None,
            active: None,
            active_drags_only: false,
            press_under: None,
            native_drag: false,
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        match event {
            Event::Move { at, hit } => {
                if self.native_drag {
                    return;
                }
                self.pointer = Some(at);
                let captured = self.active;
                self.update_hot(hit, out);
                match captured {
                    Some(a) => {
                        out.push(Effect::Move { node: a, at });
                        let t = i64::from(self.config.drag_threshold);
                        if let Some((_, p0, ..)) = self.press_under
                            && at.dist2(p0) >= t * t
                        {
                            // The edge took it as its drag.
                            self.press_under = None;
                        }
                    }
                    None => {
                        if let Some(n) = hit.node {
                            out.push(Effect::Move { node: n, at });
                        }
                    }
                }
            }
            Event::Press { at, button, clicks, hit } => {
                if self.native_drag {
                    return;
                }
                self.pointer = Some(at);
                self.update_hot(hit, out);
                self.press_under = None;
                let target = if button == Button::Left { hit.node } else { hit.beneath() };
                let Some(t) = target else { return };
                out.push(Effect::Down { node: t, at, button, clicks });
                let edge = button == Button::Left && hit.drags_only;
                if self.active.is_none() {
                    self.active = Some(t);
                    self.active_drags_only = edge;
                    // SPEC-FIX: kept only for the press that took `active`
                    // (the engine also keeps it for a later one, naming
                    // another node than `active`).
                    if edge {
                        self.press_under = Some((t, at, clicks, hit.under));
                    }
                }
            }
            Event::Release { at, button, hit } => {
                if self.native_drag {
                    return;
                }
                self.pointer = Some(at);
                let active = self.active.take();
                let under = self.press_under.take();
                match active {
                    Some(a) if self.active_drags_only => {
                        out.push(Effect::Up { node: a, at, button, inside: hit.names(a) });
                        if let Some((_, p0, clicks, Some(b))) = under.filter(|u| u.0 == a && button == Button::Left) {
                            out.push(Effect::Down { node: b, at: p0, button: Button::Left, clicks });
                            out.push(Effect::Up { node: b, at, button: Button::Left, inside: hit.names(b) });
                        }
                    }
                    Some(a) => out.push(Effect::Up { node: a, at, button, inside: hit.names(a) }),
                    None => {
                        // BUG(UIW-01): with no press, the release bubbles to
                        // whatever is under the pointer, and a control there fires.
                        if BUGS && let Some(n) = hit.node {
                            out.push(Effect::Up { node: n, at, button, inside: true });
                        }
                    }
                }
                self.active_drags_only = false;
                self.update_hot(hit, out);
            }
            Event::Leave => {
                if self.native_drag || self.pointer.is_none() {
                    return;
                }
                self.pointer = None;
                self.clear_hover(out);
                // SPEC-FIX: every press ends (the engine ends only a drags-only one).
                self.active = None;
                self.press_under = None;
            }
            Event::Capture(n) => {
                // SPEC-FIX: only while over the window.
                if self.pointer.is_some() && !self.native_drag {
                    self.active = Some(n);
                    self.active_drags_only = false;
                }
            }
            Event::Uncapture(n) => {
                // SPEC-FIX: only the owner lets go (the engine clears any).
                if self.active == Some(n) && !self.native_drag {
                    self.active = None;
                }
            }
            Event::StartDrag(from) => {
                if self.native_drag {
                    return;
                }
                // SPEC-FIX: the hover goes with the pointer.
                self.clear_hover(out);
                self.pointer = None;
                self.native_drag = true;
                // BUG(KIT-13): `Core::start_drag` (the program's `Cx::start_drag`)
                // leaves `active` set; only the widget path clears it.
                if from == DragFrom::Widget || !BUGS {
                    self.active = None;
                    self.press_under = None;
                }
            }
            Event::DragEnded => self.native_drag = false,
            Event::Removed(n) => {
                if self.pointer.is_none() || self.native_drag {
                    return;
                }
                if self.active == Some(n) {
                    self.active = None;
                }
                // SPEC-FIX: the hover and the click under forget it too.
                if self.hot == Some(n) {
                    self.hot = None;
                }
                if self.hot_under == Some(n) {
                    self.hot_under = None;
                }
                if let Some((_, _, _, b)) = &mut self.press_under
                    && *b == Some(n)
                {
                    *b = None;
                }
            }
        }
    }

    fn state(&self) -> StateName {
        if self.native_drag {
            return StateName::NativeDrag;
        }
        if self.pointer.is_none() {
            return StateName::Outside;
        }
        match self.active {
            None => StateName::Hovering,
            Some(a) if self.active_drags_only => {
                if self.press_under.is_some_and(|u| u.0 == a) {
                    StateName::DragsOnlyPressed
                } else {
                    StateName::DragsOnlyDragging
                }
            }
            Some(_) => StateName::Pressed,
        }
    }
}

/// The machine: [`Fields`] read through `Deref`, never written from outside.
pub struct Pointer(Fields);

impl std::ops::Deref for Pointer {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        &self.0
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        Pointer(Fields::new(config))
    }
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.0.handle(event, out);
    }
    fn state(&self) -> StateName {
        self.0.state()
    }
}
