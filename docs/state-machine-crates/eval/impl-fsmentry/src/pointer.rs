//! `pointer` with fsmentry 0.4.0. fsmentry has no hierarchy, so `Inside` is
//! nested the way its README suggests: the `Window` graph (`Outside`,
//! `Inside`, `NativeDrag`) carries in `Inside` the hover pair and a second
//! graph, `Press` (`Hovering`, `Pressed`, `DragsOnlyPressed`,
//! `DragsOnlyDragging`). There are no events or actions either: `handle`
//! matches `(entry(), event)` and pushes the effects itself. A transition
//! hands back the data of the state it leaves, which is where the exit
//! action (clearing the hover) and the release's `Up` get their nodes.

use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

pub const IMPLEMENTED: bool = true;

/// `Inside`'s hover pair, shared by every press state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    hot: Option<Node>,
    under: Option<Node>,
}

/// A drags-only node's left press: no drag yet, so a release clicks `beneath`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgePress {
    pub edge: Node,
    pub at: Point,
    pub clicks: u8,
    pub beneath: Option<Node>,
}

/// `Inside`'s data: the superstate's hover and the press sub-machine.
#[derive(Debug, PartialEq, Eq)]
pub struct Over {
    hover: Hover,
    press: Press,
}

/// The press graph (one module per graph, as fsmentry recommends: a handle's
/// `&mut` field is private to the module that expands the macro).
pub mod press {
    use super::*;
    fsmentry::fsmentry! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Press {
            // A press (or, from `Hovering`, a capture).
            Hovering -press-> Pressed(Node),
            Hovering -edge_press-> DragsOnlyPressed(EdgePress) -drag-> DragsOnlyDragging(Node),
            // The release (or an uncapture, or the owner removed).
            Pressed & DragsOnlyPressed & DragsOnlyDragging -release-> Hovering,
            Pressed & DragsOnlyPressed & DragsOnlyDragging -capture-> Pressed,
        }
    }
}

pub mod window {
    use super::*;
    fsmentry::fsmentry! {
        #[derive(Debug, PartialEq, Eq)]
        pub enum Window {
            Outside -enter-> Inside(Over) -leave-> Outside,
            Outside & Inside -start_drag-> NativeDrag -drag_ended-> Outside,
        }
    }
}

pub use press::{Press, PressEntry};
pub use window::{Window, WindowEntry};

impl Hover {
    fn track(&mut self, hit: Hit, out: &mut impl Sink<Effect>) {
        let under = if hit.drags_only { hit.under } else { None };
        if under != self.under {
            if let Some(o) = self.under {
                out.push(Effect::UnderOff(o));
            }
            if let Some(n) = under {
                out.push(Effect::UnderOn(n));
            }
            self.under = under;
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

    /// Exit `Inside`.
    fn clear(self, out: &mut impl Sink<Effect>) {
        if let Some(u) = self.under {
            out.push(Effect::UnderOff(u));
        }
        if let Some(n) = self.hot {
            out.push(Effect::HoverOff(n));
        }
    }

    fn forget(&mut self, n: Node) {
        if self.hot == Some(n) {
            self.hot = None;
        }
        if self.under == Some(n) {
            self.under = None;
        }
    }
}

fn up(node: Node, at: Point, button: Button, hit: Hit) -> Effect {
    Effect::Up { node, at, button, inside: hit.names(node) }
}

impl Over {
    /// Everything `Inside` does without leaving it.
    fn handle(&mut self, event: Event, threshold: i32, out: &mut impl Sink<Effect>) {
        let hover = &mut self.hover;
        if let Event::Removed(n) = event {
            hover.forget(n);
        }
        match (self.press.entry(), event) {
            (press, Event::Move { at, hit }) => {
                hover.track(hit, out);
                let node = match press {
                    PressEntry::Hovering(_) => hit.node,
                    PressEntry::Pressed(p) => Some(*p.as_ref()),
                    PressEntry::DragsOnlyDragging(d) => Some(*d.as_ref()),
                    PressEntry::DragsOnlyPressed(p) => {
                        let EdgePress { edge, at: p0, .. } = *p.as_ref();
                        if at.dist2(p0) >= i64::from(threshold) * i64::from(threshold) {
                            p.drag(edge);
                        }
                        Some(edge)
                    }
                };
                if let Some(node) = node {
                    out.push(Effect::Move { node, at });
                }
            }
            (press, Event::Press { at, button, clicks, hit }) => {
                hover.track(hit, out);
                let target = if button == Button::Left { hit.node } else { hit.beneath() };
                if let Some(node) = target {
                    out.push(Effect::Down { node, at, button, clicks });
                }
                match (press, target) {
                    (PressEntry::Hovering(h), Some(edge)) if button == Button::Left && hit.drags_only => {
                        h.edge_press(EdgePress { edge, at, clicks, beneath: hit.under })
                    }
                    (PressEntry::Hovering(h), Some(owner)) => h.press(owner),
                    // Another button: no click under any more.
                    (PressEntry::DragsOnlyPressed(p), _) => {
                        let edge = p.as_ref().edge;
                        p.drag(edge);
                    }
                    _ => {}
                }
            }
            (press, Event::Release { at, button, hit }) => {
                match press {
                    PressEntry::Hovering(_) => {}
                    PressEntry::Pressed(p) => out.push(up(p.release(), at, button, hit)),
                    PressEntry::DragsOnlyDragging(d) => out.push(up(d.release(), at, button, hit)),
                    PressEntry::DragsOnlyPressed(p) => {
                        let EdgePress { edge, at: p0, clicks, beneath } = p.release();
                        out.push(up(edge, at, button, hit));
                        if button == Button::Left
                            && let Some(b) = beneath
                        {
                            out.push(Effect::Down { node: b, at: p0, button: Button::Left, clicks });
                            out.push(up(b, at, Button::Left, hit));
                        }
                    }
                }
                hover.track(hit, out);
            }
            (PressEntry::Hovering(h), Event::Capture(n)) => h.press(n),
            (PressEntry::Pressed(p), Event::Capture(n)) => _ = p.capture(n),
            (PressEntry::DragsOnlyPressed(p), Event::Capture(n)) => _ = p.capture(n),
            (PressEntry::DragsOnlyDragging(d), Event::Capture(n)) => _ = d.capture(n),
            (PressEntry::Pressed(p), Event::Uncapture(n) | Event::Removed(n)) if *p.as_ref() == n => _ = p.release(),
            (PressEntry::DragsOnlyDragging(d), Event::Uncapture(n) | Event::Removed(n)) if *d.as_ref() == n => {
                _ = d.release()
            }
            (PressEntry::DragsOnlyPressed(p), Event::Uncapture(n) | Event::Removed(n)) if p.as_ref().edge == n => {
                _ = p.release()
            }
            (PressEntry::DragsOnlyPressed(mut p), Event::Removed(n)) => {
                let beneath = &mut p.as_mut().beneath;
                if *beneath == Some(n) {
                    *beneath = None;
                }
            }
            _ => {}
        }
    }
}

pub struct Pointer {
    drag_threshold: i32,
    window: Window,
}

impl Pointer {
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// fsmentry's transition API on the live machine: only the current
    /// state's legal edges (and none of `handle`'s effects).
    pub fn entry(&mut self) -> WindowEntry<'_> {
        self.window.entry()
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        Self { drag_threshold: config.drag_threshold, window: Window::Outside }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        // From `Outside`, a move, press or release enters `Inside` first.
        if let (WindowEntry::Outside(o), Event::Move { .. } | Event::Press { .. } | Event::Release { .. }) =
            (self.window.entry(), event)
        {
            o.enter(Over { hover: Hover::default(), press: Press::Hovering });
        }
        match (self.window.entry(), event) {
            (WindowEntry::Outside(o), Event::StartDrag(_)) => o.start_drag(),
            (WindowEntry::NativeDrag(d), Event::DragEnded) => d.drag_ended(),
            (WindowEntry::Inside(i), Event::Leave) => i.leave().hover.clear(out),
            (WindowEntry::Inside(i), Event::StartDrag(_)) => i.start_drag().hover.clear(out),
            (WindowEntry::Inside(mut i), _) => i.as_mut().handle(event, self.drag_threshold, out),
            _ => {}
        }
    }

    fn state(&self) -> StateName {
        match &self.window {
            Window::Outside => StateName::Outside,
            Window::NativeDrag => StateName::NativeDrag,
            Window::Inside(Over { press, .. }) => match press {
                Press::Hovering => StateName::Hovering,
                Press::Pressed(_) => StateName::Pressed,
                Press::DragsOnlyPressed(_) => StateName::DragsOnlyPressed,
                Press::DragsOnlyDragging(_) => StateName::DragsOnlyDragging,
            },
        }
    }
}
