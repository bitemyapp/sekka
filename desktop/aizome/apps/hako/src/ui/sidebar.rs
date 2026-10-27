//! A row of the sidebar: a place (an icon in its dye, a name, a quiet
//! detail), selected when it's where the window is, taking drops (from
//! Hako's own drags and other programs'), and an eject button for
//! volumes that can be ejected.

use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::element::{Element, MsgFn};
use aizome_kit::hash::PaintHasher;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::text::TextSpec;
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Align, Constraints, Icon, Layoutable, Length, Point, Rect, Role, Size};

use super::dnd::{Dest, Spot, Spots, Target};

/// Where drops on a row go, the spot it is, the window's spots, and the
/// message for files dropped from another program.
pub type DropInto<Msg> = (Dest, u64, Spots, Rc<dyn Fn(Dest, Vec<String>) -> Msg>);

/// A sidebar row.
pub struct PlaceRow<Msg> {
    pub icon: Icon,
    pub role: Role,
    pub label: Arc<str>,
    pub detail: Option<Arc<str>>,
    pub selected: bool,
    /// A drop target lit here.
    pub lit: bool,
    pub busy: bool,
    pub on_press: Msg,
    pub on_eject: Option<Msg>,
    pub on_menu: Option<Rc<dyn Fn(Point) -> Msg>>,
    /// Where drops on it go, and the spot it is.
    pub drop: Option<DropInto<Msg>>,
}

impl<Msg: Clone + Send + 'static> From<PlaceRow<Msg>> for Element<Msg> {
    fn from(r: PlaceRow<Msg>) -> Element<Msg> {
        let label = r.label.clone();
        Element::new(RowWidget { r }, Vec::new()).width(Length::Fill).label(label)
    }
}

struct RowWidget<Msg> {
    r: PlaceRow<Msg>,
}

impl<Msg: Clone + Send + 'static> RowWidget<Msg> {
    fn eject_rect(&self, size: Size) -> Option<Rect> {
        self.r.on_eject.as_ref().map(|_| Rect::new(size.width - 30.0, 0.0, size.width - 4.0, size.height))
    }
}

impl<Msg: Clone + Send + 'static> Widget for RowWidget<Msg> {
    fn name(&self) -> &'static str {
        "hako-place"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let h = cx.theme().metrics.row + 2.0;
        bc.constrain(Size::new(if bc.max.width.is_finite() { bc.max.width } else { 180.0 }, h))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        if let Some((dest, id, spots, _)) = &self.r.drop {
            let origin = cx.canvas.transform().translation();
            let (dest, id) = (dest.clone(), *id);
            let rect = b + origin;
            spots.set(
                id,
                Spot {
                    rect,
                    priority: 2,
                    resolve: Rc::new(move |_| Some(Target { dest: dest.clone(), spot: id, row: None })),
                },
            );
        }
        let fg = if self.r.selected {
            cx.canvas.fill_rect(b, t.selection());
            t.on_selection()
        } else {
            if cx.is_hovered() {
                cx.canvas.fill_rect(b, t.hover(t.sunk()));
            }
            t.text()
        };
        let ic = if self.r.selected { fg } else { t.role(self.r.role) };
        self.r.icon.draw(cx.canvas, Point::new(10.0, ((b.height() - 16.0) / 2.0).round()), 16.0, ic);
        let mut right = b.x1 - 10.0;
        if let Some(e) = self.eject_rect(b.size()) {
            let c = if self.r.selected { fg } else { t.text3() };
            Icon::Eject.draw(cx.canvas, Point::new(e.x0 + 5.0, ((b.height() - 14.0) / 2.0).round()), 14.0, c);
            right = e.x0 - 2.0;
        }
        if self.r.busy {
            let spec = TextSpec::new(FontSpec::ui(t.metrics.small), if self.r.selected { fg } else { t.text3() });
            let w = cx.draw_text_in("…", &spec, Rect::new(34.0, 0.0, right, b.y1), Align::End);
            right -= w + 6.0;
        } else if let Some(d) = &self.r.detail {
            let spec = TextSpec::new(
                FontSpec { tabular: true, ..FontSpec::mono(t.metrics.small) },
                if self.r.selected { fg } else { t.text3() },
            );
            let w = cx.draw_text_in(d, &spec, Rect::new(34.0, 0.0, right, b.y1), Align::End);
            right -= w + 8.0;
        }
        let spec = TextSpec::new(
            FontSpec { weight: if self.r.selected { 600.0 } else { 400.0 }, ..FontSpec::ui(t.metrics.body) },
            fg,
        );
        cx.draw_text_in(&self.r.label, &spec, Rect::new(34.0, 0.0, right, b.y1), Align::Start);
        if self.r.lit {
            aizome_kit::draw::border(cx.canvas, b, t.focus(), 2.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.r.label).add(&(
            self.r.icon,
            self.r.role,
            self.r.selected,
            self.r.lit,
            self.r.busy,
            self.r.on_eject.is_some(),
        ));
        h.add(&self.r.detail.as_deref());
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Tab
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                match (&self.r.on_eject, self.eject_rect(cx.size())) {
                    (Some(m), Some(r)) if r.contains(*pos) => cx.emit(&MsgFn::new(m.clone())),
                    _ => cx.emit(&MsgFn::new(self.r.on_press.clone())),
                }
                Status::Handled
            }
            Event::PointerDown { pos, button: PointerButton::Right, .. } => match &self.r.on_menu {
                Some(f) => {
                    let at = cx.window_rect().origin() + pos.to_vec2();
                    cx.emit(&MsgFn::new(f(at)));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::DragOver { urls, .. } if self.r.drop.is_some() && !urls.is_empty() => Status::Handled,
            Event::Drop { urls, .. } => match &self.r.drop {
                Some((dest, _, _, f)) if !urls.is_empty() => {
                    cx.emit(&MsgFn::new(f(dest.clone(), urls.clone())));
                    Status::Handled
                }
                _ => Status::Ignored,
            },
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        cx.emit(&MsgFn::new(self.r.on_press.clone()));
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Tab,
            label: Some(self.r.label.to_string()),
            selected: Some(self.r.selected),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.r.label)
    }
}

/// A sidebar section's caption, as the kit sets one.
pub fn caption<Msg: 'static>(title: &str) -> Element<Msg> {
    aizome_kit::caption(title.to_owned())
}

/// A folder of the path bar: its name (and an icon for the first), a
/// press to go there, and drops into it.
pub struct Crumb<Msg> {
    pub icon: Option<Icon>,
    pub label: Arc<str>,
    pub current: bool,
    pub lit: bool,
    pub on_press: Option<Msg>,
    pub drop: Option<DropInto<Msg>>,
}

impl<Msg: Clone + Send + 'static> From<Crumb<Msg>> for Element<Msg> {
    fn from(c: Crumb<Msg>) -> Element<Msg> {
        let label = c.label.clone();
        Element::new(CrumbWidget { c, width: 0.0 }, Vec::new()).label(label)
    }
}

struct CrumbWidget<Msg> {
    c: Crumb<Msg>,
    width: f64,
}

impl<Msg: Clone + Send + 'static> CrumbWidget<Msg> {
    fn spec(&self, t: &aizome_kit::Theme) -> TextSpec {
        let color = if self.c.current { t.text() } else { t.text2() };
        TextSpec::new(
            FontSpec { weight: if self.c.current { 600.0 } else { 400.0 }, ..FontSpec::ui(t.metrics.small + 1.0) },
            color,
        )
    }
}

impl<Msg: Clone + Send + 'static> Widget for CrumbWidget<Msg> {
    fn name(&self) -> &'static str {
        "hako-crumb"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let t = cx.theme().clone();
        let w = cx.measure(&self.c.label, &self.spec(&t)).width.ceil()
            + 12.0
            + if self.c.icon.is_some() { 18.0 } else { 0.0 };
        self.width = w;
        bc.constrain(Size::new(w.min(if bc.max.width.is_finite() { bc.max.width } else { w }), t.metrics.control))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        if let Some((dest, id, spots, _)) = &self.c.drop {
            let origin = cx.canvas.transform().translation();
            let (dest, id) = (dest.clone(), *id);
            spots.set(
                id,
                Spot {
                    rect: b + origin,
                    priority: 2,
                    resolve: Rc::new(move |_| Some(Target { dest: dest.clone(), spot: id, row: None })),
                },
            );
        }
        if cx.is_hovered() && self.c.on_press.is_some() {
            cx.canvas.fill_rect(b, t.hover(t.surface()));
        }
        let mut x = 6.0;
        if let Some(i) = self.c.icon {
            i.draw(
                cx.canvas,
                Point::new(x, ((b.height() - 14.0) / 2.0).round()),
                14.0,
                if self.c.current { t.text() } else { t.text2() },
            );
            x += 18.0;
        }
        let spec = self.spec(t);
        cx.draw_text_in(&self.c.label, &spec, Rect::new(x, 0.0, b.x1 - 6.0, b.y1), Align::Start);
        if self.c.lit {
            aizome_kit::draw::border(cx.canvas, b, t.focus(), 2.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.c.label).add(&(self.c.icon, self.c.current, self.c.lit, self.c.on_press.is_some()));
    }

    fn hit(&self) -> bool {
        self.c.on_press.is_some() || self.c.drop.is_some()
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => match &self.c.on_press {
                Some(m) => {
                    cx.emit(&MsgFn::new(m.clone()));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::DragOver { urls, .. } if self.c.drop.is_some() && !urls.is_empty() => Status::Handled,
            Event::Drop { urls, .. } => match &self.c.drop {
                Some((dest, _, _, f)) if !urls.is_empty() => {
                    cx.emit(&MsgFn::new(f(dest.clone(), urls.clone())));
                    Status::Handled
                }
                _ => Status::Ignored,
            },
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Button, label: Some(self.c.label.to_string()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.c.label)
    }
}
