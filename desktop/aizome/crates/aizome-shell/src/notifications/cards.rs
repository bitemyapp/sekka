//! Critical notifications stay as cards under the bar until acted on: a
//! `raised` card each, at the top right beside the Deskbar, with the
//! notification's actions as buttons and a ×.

use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions, Margins, button};
use aizome_theme::typeface;

use crate::look::{self, Hue};
use crate::shell::{Msg, Shell, Surface, layer};
use crate::widgets::drawn::{around, drawn};

pub const WIDTH: f64 = 340.0;
pub const CARD: f64 = 92.0;
pub const GAP: f64 = 8.0;

#[derive(Clone, Debug, PartialEq)]
pub struct CardView {
    pub id: u32,
    pub glyph: Icon,
    pub hue: Hue,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub buttons: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CardMsg {
    Open(u32),
    Action(u32, String),
    Dismiss(u32),
}

fn card(c: &CardView) -> Element<CardMsg> {
    let v = c.clone();
    let text = drawn(WIDTH - 24.0, 56.0, (v.summary.clone(), v.body.clone(), v.app.clone()), move |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        v.glyph.draw(cx.canvas, Point::new(0.0, 2.0), 16.0, v.hue.color(t));
        let a = look::mono(10.5, typeface::WEIGHT_MEDIUM, t.text3());
        cx.draw_text_in(&v.app.to_uppercase(), &a, Rect::new(24.0, 0.0, b.x1 - 20.0, 18.0), Align::Start);
        let s = look::ui(13.0 * t.metrics.factor, typeface::WEIGHT_TAB, t.text());
        cx.draw_text_in(&v.summary, &s, Rect::new(0.0, 20.0, b.x1, 38.0), Align::Start);
        let mut bs = look::ui(12.0 * t.metrics.factor, typeface::WEIGHT_REGULAR, t.text2());
        bs.wrap = aizome_kit::sidestep_ui::Wrap::Words;
        bs.max_lines = 1;
        let l = cx.layout_text(&v.body, &bs, Some(b.width()));
        cx.canvas.draw_text(&l, Point::new(0.0, 39.0));
    })
    .on_click(CardMsg::Open(c.id))
    .text(format!("{} {}", c.summary, c.body), SemanticRole::Group);
    let mut buttons: Vec<Element<CardMsg>> = vec![aizome_kit::spacer().into()];
    for (key, label) in c.buttons.iter().take(3) {
        buttons.push(button(label.clone()).compact().on_press(CardMsg::Action(c.id, key.clone())).into());
    }
    buttons.push(button("Dismiss").compact().on_press(CardMsg::Dismiss(c.id)).into());
    let body = aizome_kit::column(vec![text.into(), aizome_kit::row(buttons).spacing(6.0).into()])
        .padding([10.0, 12.0, 8.0, 12.0]);
    around(body, 0.0, Size::new(WIDTH, CARD), ("card", c.id), |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        look::panel(cx.canvas, t, b);
        cx.canvas.fill_rect(Rect::new(1.0, 1.0, 3.0, b.y1 - 1.0), Hue::Red.color(t));
    })
    .key(("card", c.id))
    .into()
}

pub fn view(cards: &[CardView]) -> Element<CardMsg> {
    let items: Vec<Element<CardMsg>> = cards.iter().map(card).collect();
    aizome_kit::container(aizome_kit::column(items).spacing(GAP)).padding([0.0, 3.0, 3.0, 0.0]).into()
}

pub fn height(n: usize) -> f64 {
    n as f64 * CARD + n.saturating_sub(1) as f64 * GAP + 3.0
}

impl Shell {
    pub fn card_views(&self) -> Vec<CardView> {
        self.notes
            .cards()
            .into_iter()
            .take(4)
            .map(|n| {
                let (name, glyph, hue) = self.app_look(&n.filed_under);
                let known = crate::apps::catalog::glyph_for(&n.filed_under).is_some();
                CardView {
                    id: n.id,
                    glyph: if known { glyph } else { Icon::TriangleAlert },
                    hue: if known { hue } else { Hue::Red },
                    app: if n.app_name.is_empty() { name } else { n.app_name.clone() },
                    summary: n.summary.clone(),
                    body: n.first_line().to_owned(),
                    buttons: n.buttons().cloned().collect(),
                }
            })
            .collect()
    }

    /// Open, size or close the cards' surface.
    pub fn sync_cards(&mut self, cx: &mut Cx<Msg>) {
        let n = if self.locked() { 0 } else { self.card_views().len() };
        // Beside the Deskbar when it hangs at the right (one that hides
        // itself overlays them while it shows, as it does windows).
        let beside = self.deskbar.is_some()
            && !self.deskbar_cfg.auto_hide
            && self.deskbar_cfg.side == aizome_config::DeskbarSide::Right;
        let right = if beside { self.deskbar_width() + crate::deskbar::SHADOW + 12.0 } else { 12.0 };
        let margins = Margins::new(crate::menubar::HEIGHT as i32 + 8, right as i32, 0, 0);
        match (n, self.cards) {
            (0, Some((w, _, _))) => {
                cx.close_window(w);
                self.surfaces.remove(&w);
                self.cards = None;
            }
            (0, None) => {}
            (n, None) => {
                let Some(output) = self.primary().map(|s| s.output.id) else { return };
                let opts = LayerOptions::new(Layer::Top, "aizome-notifications")
                    .anchors(Anchors::TOP | Anchors::RIGHT)
                    .exclusive_zone(-1)
                    .margins(margins)
                    .keyboard(KeyboardInteractivity::None)
                    .output(output);
                let w = cx.open_window(layer(opts, WIDTH + 3.0, height(n), "Notifications"));
                self.surfaces.insert(w, Surface::Cards);
                self.cards = Some((w, n, right));
            }
            (n, Some((w, had, at))) => {
                if n != had {
                    cx.set_layer_size(w, Size::new(WIDTH + 3.0, height(n)));
                }
                // The Deskbar moved, widened or folded while they're up.
                if right != at {
                    cx.set_layer_margins(w, margins);
                }
                self.cards = Some((w, n, right));
            }
        }
    }

    pub fn card_msg(&mut self, m: CardMsg, cx: &mut Cx<Msg>) {
        match m {
            CardMsg::Open(id) => {
                self.open_note(id, cx);
            }
            CardMsg::Action(id, key) => {
                self.note_action(id, &key);
                if let Some(n) = self.notes.list.iter_mut().find(|n| n.id == id) {
                    n.read = true;
                }
            }
            CardMsg::Dismiss(id) => self.dismiss_note(id),
        }
    }
}
