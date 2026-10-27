//! The display: a sunken well with the expression (or the answer) large
//! in IBM Plex Mono with tabular digits, and above it the answer's
//! expression, a preview of the value, or what went wrong. Indicators
//! (DEG, 2ND, M, the word size) sit at its top left.

use aizome_kit::draw;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Role};
use aizome_kit::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};
use aizome_kit::{Element, Layoutable, Length};

use crate::entry::Shown;
use crate::grouped::{self, Cut, Grouped};

/// The display's height.
pub const HEIGHT: f64 = 76.0;
const PAD: f64 = 10.0;
const BIG: f64 = 32.0;
const SMALL_MAIN: f64 = 15.0;

/// An indicator: its text and whether it's lit.
pub type Indicator = (String, bool);

pub fn display<Msg: 'static>(shown: Shown, indicators: Vec<Indicator>) -> Element<Msg> {
    Element::new(DisplayWidget { shown, indicators }, Vec::new()).key("display").width(Length::Fill).height(HEIGHT)
}

struct DisplayWidget {
    shown: Shown,
    indicators: Vec<Indicator>,
}

fn mono(size: f64, weight: f32) -> FontSpec {
    let mut f = FontSpec::mono(size);
    f.weight = weight;
    f.tabular = true;
    f
}

impl Widget for DisplayWidget {
    fn name(&self) -> &'static str {
        "soroban-display"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width.min(10_000.0), HEIGHT))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        draw::sunken(cx.canvas, t, r, t.sunk(), t.line2());

        // Indicators, top left.
        let mut x = r.x0 + PAD;
        let top = Rect::new(r.x0, r.y0 + 5.0, r.x1, r.y0 + 22.0);
        for (text, lit) in &self.indicators {
            let mut f = mono(10.5, if *lit { 600.0 } else { 500.0 });
            f.tabular = false;
            let mut spec = TextSpec::new(f, if *lit { t.role(Role::Accent) } else { t.text3() });
            spec.letter_spacing = 0.8;
            let w = cx.draw_text_in(text, &spec, Rect::new(x, top.y0, r.x1 - PAD, top.y1), aizome_kit::Align::Start);
            x += w + 10.0;
        }

        // The small line, right aligned after the indicators.
        let s = &self.shown;
        if !s.sub.is_empty() {
            let color = if s.error { t.role(Role::Danger) } else { t.text3() };
            let spec = TextSpec::new(mono(12.5, if s.error { 500.0 } else { 400.0 }), color);
            let room = (r.x1 - PAD) - (x + 4.0);
            let text = grouped::fit(cx, &s.sub, &spec, room.max(20.0), Cut::Start);
            grouped::draw_in(cx, &text, &spec, Rect::new(x + 4.0, top.y0, r.x1 - PAD, top.y1), aizome_kit::Align::End);
        }

        // The big line: as large as fits, then cut from the left.
        let room = r.width() - 2.0 * PAD;
        let ghost: String = ")".repeat(s.open.min(8));
        let weight = if s.answer { 600.0 } else { 500.0 };
        let probe = TextSpec::new(mono(BIG, weight), t.text());
        let full = format!("{}{}", s.main, ghost);
        let w_big = Grouped::new(cx, &full, &probe).size().width.max(1.0);
        let size = if w_big <= room { BIG } else { ((BIG * room / w_big) * 2.0).floor() / 2.0 }.max(SMALL_MAIN);
        let spec = TextSpec::new(mono(size, weight), t.text());
        let ghost_spec = TextSpec::new(mono(size, 400.0), t.faint());
        let gw = if ghost.is_empty() { 0.0 } else { cx.layout_text(&ghost, &ghost_spec, None).size().width };
        let main = grouped::fit(cx, &s.main, &spec, room - gw, Cut::Start);
        let l = Grouped::new(cx, &main, &spec);
        let ls = l.size();
        let baseline_box = Rect::new(r.x0 + PAD, r.y0 + 24.0, r.x1 - PAD, r.y1 - 6.0);
        let y = (baseline_box.y0 + (baseline_box.height() - ls.height) / 2.0).round();
        let x = (baseline_box.x1 - gw - ls.width).round();
        l.draw(cx.canvas, Point::new(x, y));
        if !ghost.is_empty() {
            let g = cx.layout_text(&ghost, &ghost_spec, None);
            cx.canvas.draw_text(&g, Point::new(x + ls.width, y));
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        let s = &self.shown;
        h.add(&(&s.main, &s.sub, s.error, s.open, s.answer, &self.indicators));
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Label,
            label: Some("Display".into()),
            value: Some(if self.shown.sub.is_empty() {
                self.shown.main.clone()
            } else {
                format!("{}, {}", self.shown.main, self.shown.sub)
            }),
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.shown.main)
    }
}
