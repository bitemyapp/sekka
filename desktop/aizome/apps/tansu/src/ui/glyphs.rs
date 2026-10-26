//! Tansu's own glyphs for kinds of drive (an M.2 stick, a 2.5" SSD, a
//! hard disk's platter, a USB stick, an SD card, a disc, an image), drawn
//! as Lucide draws (a 24-unit grid, 1.5pt strokes at 16pt, round ends),
//! and the sidebar's drive rows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Align, Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::kurbo::{Affine, BezPath};
use aizome_kit::sidestep_ui::{Canvas, Color, LineCap, LineJoin, PointerButton, StrokeStyle};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Theme};
use aizome_kit::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Element, Layoutable};
use aizome_sys::disks::Health;

use crate::drives::{Entry, Kind};

fn svg(kind: Kind) -> &'static [&'static str] {
    match kind {
        // An M.2 stick: the board, two chips, the screw notch.
        Kind::Nvme => &[
            "M3 8.5h18a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1z",
            "M5.5 10.5h3.5v3h-3.5z",
            "M11 10.5h3.5v3H11z",
            "M19.5 11v2",
        ],
        // A 2.5" SSD: the case, a chip, the label's rule.
        Kind::Ssd => &[
            "M6 3h12a1.5 1.5 0 0 1 1.5 1.5v15A1.5 1.5 0 0 1 18 21H6a1.5 1.5 0 0 1-1.5-1.5v-15A1.5 1.5 0 0 1 6 3z",
            "M9 7h6v5H9z",
            "M8 16h8",
        ],
        // A hard disk: the case, the platter, the arm.
        Kind::Hdd => &[
            "M5 2.5h14a1.5 1.5 0 0 1 1.5 1.5v16a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 20V4A1.5 1.5 0 0 1 5 2.5z",
            "M12 5a5.5 5.5 0 1 1 0 11a5.5 5.5 0 1 1 0-11z",
            "M12 9.75a.75.75 0 1 1 0 1.5a.75.75 0 1 1 0-1.5z",
            "M17 18.5l-3.5-5",
        ],
        // A USB stick: the body and its plug.
        Kind::Usb => &[
            "M8.5 10h7a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1-1.5 1.5h-7A1.5 1.5 0 0 1 7 20.5v-9A1.5 1.5 0 0 1 8.5 10z",
            "M9 10V3h6v7",
            "M11 5.5v1M13 5.5v1",
        ],
        // An SD card: the cut corner and the contacts.
        Kind::Sd => &["M7 3h7.5L18 6.5V20a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z", "M9 6v3M11.5 6v3M14 6v3"],
        Kind::Optical => &[
            "M12 3a9 9 0 1 1 0 18a9 9 0 1 1 0-18z",
            "M12 9.5a2.5 2.5 0 1 1 0 5a2.5 2.5 0 1 1 0-5z",
            "M12 6a6 6 0 0 1 5.2 3",
        ],
        // An image: a file with a disc on it.
        Kind::Image => &[
            "M6 2.5h8l4.5 4.5v14.5H6z",
            "M14 2.5V7h4.5",
            "M12 10.5a3.75 3.75 0 1 1 0 7.5a3.75 3.75 0 1 1 0-7.5z",
            "M12 13.5a.75.75 0 1 1 0 1.5a.75.75 0 1 1 0-1.5z",
        ],
    }
}

thread_local! {
    static PATHS: RefCell<HashMap<Kind, Arc<BezPath>>> = RefCell::new(HashMap::new());
}

fn path(kind: Kind) -> Arc<BezPath> {
    PATHS.with(|p| {
        p.borrow_mut()
            .entry(kind)
            .or_insert_with(|| {
                let mut out = BezPath::new();
                for d in svg(kind) {
                    if let Ok(part) = BezPath::from_svg(d) {
                        out.extend(part);
                    }
                }
                Arc::new(out)
            })
            .clone()
    })
}

/// Draw `kind`'s glyph, `size` points square, its top left at `at`.
pub fn draw(canvas: &mut Canvas, kind: Kind, at: Point, size: f64, color: Color) {
    let mut shape = (*path(kind)).clone();
    shape.apply_affine(Affine::translate(at.to_vec2()) * Affine::scale(size / 24.0));
    let width = (1.5 * size / 16.0).max(1.0);
    canvas.stroke(&shape, &StrokeStyle::new(width).with_cap(LineCap::Round).with_join(LineJoin::Round), color);
}

/// The colour of a SMART health dot (`None`: nothing known, no dot).
pub fn health_color(health: Health, theme: &Theme) -> Option<Color> {
    let p = &theme.palette;
    let c = match health {
        Health::Unknown => return None,
        Health::Good => p.green,
        Health::Warning => p.amber,
        Health::Failing => p.red,
    };
    Some(aizome_kit::theme::color(c))
}

pub fn health_words(health: Health) -> &'static str {
    match health {
        Health::Unknown => "No SMART data",
        Health::Good => "Healthy",
        Health::Warning => "Needs watching",
        Health::Failing => "Failing: back it up now",
    }
}

/// A drive glyph as an element (for the disk's header).
pub fn glyph<Msg: 'static>(kind: Kind, size: f64) -> Element<Msg> {
    Element::new(Glyph { kind, size }, Vec::new())
}

struct Glyph {
    kind: Kind,
    size: f64,
}

impl Widget for Glyph {
    fn name(&self) -> &'static str {
        "tansu-glyph"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(self.size, self.size))
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.kind).add(&self.size.to_bits());
    }

    fn paint(&self, cx: &mut PaintCx) {
        let c = cx.theme().text2();
        draw(cx.canvas, self.kind, Point::ZERO, self.size, c);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, label: Some(self.kind.words().into()), ..Default::default() })
    }
}

/// A SMART health dot as an element.
pub fn dot<Msg: 'static>(health: Health) -> Element<Msg> {
    Element::new(Dot { health }, Vec::new())
}

struct Dot {
    health: Health,
}

impl Widget for Dot {
    fn name(&self) -> &'static str {
        "tansu-dot"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(10.0, 10.0))
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.health as u8));
    }

    fn paint(&self, cx: &mut PaintCx) {
        paint_dot(cx.canvas, cx.theme(), self.health, Point::new(5.0, 5.0), 4.0);
    }
}

fn paint_dot(canvas: &mut Canvas, theme: &Theme, health: Health, center: Point, r: f64) {
    use aizome_kit::sidestep_ui::kurbo::Circle;
    match health_color(health, theme) {
        Some(c) => canvas.fill(&Circle::new(center, r), c),
        None => canvas.stroke(&Circle::new(center, r - 0.5), &StrokeStyle::new(1.0), theme.faint()),
    }
}

/// A drive in the sidebar: its glyph, its name, its size and kind, and its
/// SMART health dot.
pub fn drive_row<Msg: Clone + Send + 'static>(entry: &Entry, selected: bool, on_press: Msg) -> Element<Msg> {
    let w = DriveRow {
        kind: entry.kind,
        name: entry.name.clone().into(),
        subtitle: entry.subtitle().into(),
        health: entry.health,
        selected,
        on_press: MsgFn::new(on_press),
    };
    Element::new(w, Vec::new()).key(entry.key.clone()).width(aizome_kit::Length::Fill)
}

struct DriveRow {
    kind: Kind,
    name: Arc<str>,
    subtitle: Arc<str>,
    health: Health,
    selected: bool,
    on_press: MsgFn,
}

impl Widget for DriveRow {
    fn name(&self) -> &'static str {
        "tansu-drive"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(if bc.max.width.is_finite() { bc.max.width } else { 200.0 }, 44.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme().clone();
        let r = cx.bounds();
        let (fg, fg2) = if self.selected {
            cx.canvas.fill_rect(r, t.selection());
            (t.on_selection(), t.on_selection().with_alpha(0.8))
        } else {
            if cx.is_hovered() {
                cx.canvas.fill_rect(r, t.hover(t.sunk()));
            }
            (t.text(), t.text3())
        };
        draw(
            cx.canvas,
            self.kind,
            Point::new(10.0, ((r.height() - 22.0) / 2.0).round()),
            22.0,
            if self.selected { fg } else { t.text2() },
        );
        let right = r.x1 - 26.0;
        let name = TextSpec::new(FontSpec { weight: 600.0, ..FontSpec::ui(t.metrics.body) }, fg);
        cx.draw_text_in(&self.name, &name, Rect::new(42.0, 4.0, right, 23.0), Align::Start);
        let sub = TextSpec::new(FontSpec { tabular: true, ..FontSpec::ui(t.metrics.small) }, fg2);
        cx.draw_text_in(&self.subtitle, &sub, Rect::new(42.0, 22.0, right, 39.0), Align::Start);
        if self.selected && self.health != Health::Unknown {
            // A ring keeps the dot's colour readable on the selection.
            use aizome_kit::sidestep_ui::kurbo::Circle;
            cx.canvas.fill(&Circle::new(Point::new(r.x1 - 14.0, r.height() / 2.0), 6.0), t.on_selection());
        }
        paint_dot(cx.canvas, &t, self.health, Point::new(r.x1 - 14.0, r.height() / 2.0), 4.5);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.name).str(&self.subtitle).add(&(self.kind, self.health as u8, self.selected));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Tab
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => {
                cx.emit(&self.on_press);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        cx.emit(&self.on_press);
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Tab,
            label: Some(format!("{}, {}, {}", self.name, self.subtitle, health_words(self.health))),
            selected: Some(self.selected),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.name)
    }
}
