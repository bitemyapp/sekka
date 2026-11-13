//! `pointer` as an enum: the hover pair lives in `Inside`, the press in a
//! nested enum.

use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    pub hot: Option<Node>,
    pub under: Option<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    None,
    Owned { owner: Node },
    Edge { edge: Node, at: Point, clicks: u8, beneath: Option<Node> },
    EdgeDrag { edge: Node },
}

impl Press {
    fn owner(&self) -> Option<Node> {
        match *self {
            Press::None => None,
            Press::Owned { owner } => Some(owner),
            Press::Edge { edge, .. } | Press::EdgeDrag { edge } => Some(edge),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Outside,
    Inside { hover: Hover, press: Press },
    NativeDrag,
}

/// The machine. Its fields are private: the state changes only through
/// `Machine::handle`; [`Pointer::typed`] reads it.
pub struct Pointer {
    config: Config,
    state: State,
}

impl Pointer {
    /// The current state, read-only.
    pub fn typed(&self) -> &State {
        &self.state
    }
}

fn track(h: &mut Hover, hit: Hit, out: &mut impl Sink<Effect>) {
    let under = if hit.drags_only { hit.under } else { None };
    if under != h.under {
        if let Some(o) = h.under {
            out.push(Effect::UnderOff(o));
        }
        if let Some(n) = under {
            out.push(Effect::UnderOn(n));
        }
        h.under = under;
    }
    if hit.node != h.hot {
        if let Some(o) = h.hot {
            out.push(Effect::HoverOff(o));
        }
        if let Some(n) = hit.node {
            out.push(Effect::HoverOn(n));
        }
        h.hot = hit.node;
    }
}

fn clear(h: &Hover, out: &mut impl Sink<Effect>) {
    if let Some(u) = h.under {
        out.push(Effect::UnderOff(u));
    }
    if let Some(n) = h.hot {
        out.push(Effect::HoverOff(n));
    }
}

impl Pointer {
    /// The `Inside` data, entering from `Outside`; `None` during a native drag.
    fn inside(&mut self) -> Option<(&mut Hover, &mut Press)> {
        if self.state == State::Outside {
            self.state = State::Inside { hover: Hover::default(), press: Press::None };
        }
        match &mut self.state {
            State::Inside { hover, press } => Some((hover, press)),
            _ => None,
        }
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        Self { config, state: State::Outside }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let t2 = i64::from(self.config.drag_threshold) * i64::from(self.config.drag_threshold);
        match event {
            Event::Move { at, hit } => {
                let Some((hover, press)) = self.inside() else { return };
                track(hover, hit, out);
                match *press {
                    Press::None => {
                        if let Some(n) = hit.node {
                            out.push(Effect::Move { node: n, at });
                        }
                    }
                    Press::Owned { owner } => out.push(Effect::Move { node: owner, at }),
                    Press::EdgeDrag { edge } => out.push(Effect::Move { node: edge, at }),
                    Press::Edge { edge, at: p0, .. } => {
                        out.push(Effect::Move { node: edge, at });
                        if at.dist2(p0) >= t2 {
                            *press = Press::EdgeDrag { edge };
                        }
                    }
                }
            }
            Event::Press { at, button, clicks, hit } => {
                let Some((hover, press)) = self.inside() else { return };
                track(hover, hit, out);
                let target = if button == Button::Left { hit.node } else { hit.beneath() };
                if let Some(t) = target {
                    out.push(Effect::Down { node: t, at, button, clicks });
                }
                match *press {
                    Press::None => {
                        if let Some(t) = target {
                            *press = if button == Button::Left && hit.drags_only {
                                Press::Edge { edge: t, at, clicks, beneath: hit.under }
                            } else {
                                Press::Owned { owner: t }
                            };
                        }
                    }
                    Press::Edge { edge, .. } => *press = Press::EdgeDrag { edge },
                    Press::Owned { .. } | Press::EdgeDrag { .. } => {}
                }
            }
            Event::Release { at, button, hit } => {
                let Some((hover, press)) = self.inside() else { return };
                match std::mem::replace(press, Press::None) {
                    Press::None => {}
                    Press::Owned { owner: n } | Press::EdgeDrag { edge: n } => {
                        out.push(Effect::Up { node: n, at, button, inside: hit.names(n) });
                    }
                    Press::Edge { edge, at: p0, clicks, beneath } => {
                        out.push(Effect::Up { node: edge, at, button, inside: hit.names(edge) });
                        if button == Button::Left
                            && let Some(b) = beneath
                        {
                            out.push(Effect::Down { node: b, at: p0, button: Button::Left, clicks });
                            out.push(Effect::Up { node: b, at, button: Button::Left, inside: hit.names(b) });
                        }
                    }
                }
                track(hover, hit, out);
            }
            Event::Leave => {
                if let State::Inside { hover, .. } = &self.state {
                    clear(hover, out);
                    self.state = State::Outside;
                }
            }
            Event::Capture(n) => {
                if let State::Inside { press, .. } = &mut self.state {
                    *press = Press::Owned { owner: n };
                }
            }
            Event::Uncapture(n) => {
                if let State::Inside { press, .. } = &mut self.state
                    && press.owner() == Some(n)
                {
                    *press = Press::None;
                }
            }
            Event::StartDrag(_) => match &self.state {
                State::Inside { hover, .. } => {
                    clear(hover, out);
                    self.state = State::NativeDrag;
                }
                State::Outside => self.state = State::NativeDrag,
                State::NativeDrag => {}
            },
            Event::DragEnded => {
                if self.state == State::NativeDrag {
                    self.state = State::Outside;
                }
            }
            Event::Removed(n) => {
                if let State::Inside { hover, press } = &mut self.state {
                    if hover.hot == Some(n) {
                        hover.hot = None;
                    }
                    if hover.under == Some(n) {
                        hover.under = None;
                    }
                    if press.owner() == Some(n) {
                        *press = Press::None;
                    } else if let Press::Edge { beneath, .. } = press
                        && *beneath == Some(n)
                    {
                        *beneath = None;
                    }
                }
            }
        }
    }

    fn state(&self) -> StateName {
        match self.state {
            State::Outside => StateName::Outside,
            State::NativeDrag => StateName::NativeDrag,
            State::Inside { press, .. } => match press {
                Press::None => StateName::Hovering,
                Press::Owned { .. } => StateName::Pressed,
                Press::Edge { .. } => StateName::DragsOnlyPressed,
                Press::EdgeDrag { .. } => StateName::DragsOnlyDragging,
            },
        }
    }
}
