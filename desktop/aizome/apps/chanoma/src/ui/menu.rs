//! A menu over what's playing (a game paused, a page, a film), drawn as
//! Aizome's titled menus are: a dark title strip, engraved rows one above
//! the other, and the row the controller is on lit in the selection's
//! indigo with the focus colour's rail beside it and the button that
//! chooses it at its end. The first is ready to press.

use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::Theme;

use super::look::*;
use super::{Focus, H, W};
use crate::pad::{Button, Family};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Menu {
    pub title: String,
    pub subtitle: String,
    pub items: Vec<String>,
    /// A picture key (where a game was left), drawn above the title.
    pub picture: Option<std::path::PathBuf>,
}

const STRIP_H: f64 = 84.0;
const ITEM_H: f64 = 88.0;
const WIDTH: f64 = 720.0;

impl Menu {
    pub fn new(title: impl Into<String>, items: &[&str]) -> Menu {
        Menu {
            title: title.into(),
            subtitle: String::new(),
            items: items.iter().map(|s| (*s).to_owned()).collect(),
            picture: None,
        }
    }

    pub fn with_subtitle(mut self, s: impl Into<String>) -> Menu {
        self.subtitle = s.into();
        self
    }

    pub fn frame(&self) -> Rect {
        let h = STRIP_H + self.items.len() as f64 * ITEM_H + 2.0 * EDGE;
        let top = ((H - h) / 2.0).round();
        Rect::new(W / 2.0 - WIDTH / 2.0, top, W / 2.0 + WIDTH / 2.0, top + h)
    }

    fn item_rect(&self, i: usize) -> Rect {
        let f = self.frame();
        let y = f.y0 + STRIP_H + i as f64 * ITEM_H;
        Rect::new(f.x0 + EDGE, y, f.x1 - EDGE, y + ITEM_H)
    }

    pub fn hit(&self, p: Point) -> Option<usize> {
        (0..self.items.len()).find(|i| self.item_rect(*i).contains(p))
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus, family: Family, ok: Button) {
        let f = self.frame();
        floating(c, theme, f);
        let strip = Rect::new(f.x0 + EDGE, f.y0 + EDGE, f.x1 - EDGE, f.y0 + STRIP_H);
        c.fill_rect(strip, theme.menu_title());
        let title = fit_text(theme, &self.title, strip.width() - 300.0, font(TITLE, SEMIBOLD));
        text_mid(c, theme, &title, strip.x0 + 36.0, strip.center().y, font(TITLE, SEMIBOLD), theme.on_menu_title());
        if !self.subtitle.is_empty() {
            let w = caption_width(theme, &self.subtitle, CAPTION);
            caption(c, theme, &self.subtitle, strip.x1 - 36.0 - w, strip.center().y, theme.text3());
        }
        for (i, item) in self.items.iter().enumerate() {
            let r = self.item_rect(i);
            let on = focus.row == i;
            let ink = if on {
                c.fill_rect(r, theme.selection());
                c.fill_rect(Rect::new(r.x0, r.y0, r.x0 + RAIL, r.y1), theme.focus());
                button_glyph(c, theme, family, ok, Point::new(r.x1 - 36.0 - 22.0, r.center().y), 22.0);
                theme.on_selection()
            } else {
                // Engraved: the highlight along the top, a rule under it
                // where another row follows.
                c.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + EDGE), theme.bevel_hi());
                if i + 1 < self.items.len() && focus.row != i + 1 {
                    c.fill_rect(Rect::new(r.x0, r.y1 - EDGE, r.x1, r.y1), theme.line());
                }
                theme.text()
            };
            text_mid(c, theme, item, r.x0 + 36.0, r.center().y, font(BODY + 2.0, MEDIUM), ink);
        }
    }
}
