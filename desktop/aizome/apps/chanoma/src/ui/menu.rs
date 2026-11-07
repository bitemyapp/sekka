//! A menu over what's playing (a game paused, a page, a film), drawn as
//! Aizome's titled menus are: a dark title strip, engraved rows one above
//! the other, and the row the controller is on lit in the selection's
//! indigo with the focus colour's rail beside it and the button that
//! chooses it at its end. The first is ready to press.

use aizome_kit::Icon;
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
/// Items showing at once; a longer menu scrolls.
pub const SHOWN: usize = 8;

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
        let h = STRIP_H + self.items.len().min(SHOWN) as f64 * ITEM_H + 2.0 * EDGE;
        let top = ((H - h) / 2.0).round();
        Rect::new(W / 2.0 - WIDTH / 2.0, top, W / 2.0 + WIDTH / 2.0, top + h)
    }

    /// The first item showing with the focus on `focus`: a menu longer
    /// than fits (a film's many subtitles) scrolls to keep it in the
    /// middle, as near as the ends allow.
    pub fn first_for(&self, focus: usize) -> usize {
        let n = self.items.len();
        if n <= SHOWN { 0 } else { focus.saturating_sub(SHOWN / 2).min(n - SHOWN) }
    }

    /// Item `i`'s rectangle, scrolled to `first`.
    pub fn item_rect(&self, i: usize, first: usize) -> Rect {
        let f = self.frame();
        let y = f.y0 + STRIP_H + (i as f64 - first as f64) * ITEM_H;
        Rect::new(f.x0 + EDGE, y, f.x1 - EDGE, y + ITEM_H)
    }

    pub fn hit(&self, p: Point, focus: usize) -> Option<usize> {
        let first = self.first_for(focus);
        (first..self.items.len().min(first + SHOWN)).find(|i| self.item_rect(*i, first).contains(p))
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus, family: Family, ok: Button) {
        let f = self.frame();
        floating(c, theme, f);
        let strip = Rect::new(f.x0 + EDGE, f.y0 + EDGE, f.x1 - EDGE, f.y0 + STRIP_H);
        c.fill_rect(strip, theme.menu_title());
        // The subtitle at the strip's end, if it leaves the title room; the
        // title fitted to what's left.
        let sub =
            (!self.subtitle.is_empty()).then(|| caption_width(theme, &self.subtitle, CAPTION)).filter(|w| *w <= 300.0);
        let room = strip.width() - 72.0 - sub.map_or(0.0, |w| w + 24.0);
        let title = fit_text(theme, &self.title, room, font(TITLE, SEMIBOLD));
        text_mid(c, theme, &title, strip.x0 + 36.0, strip.center().y, font(TITLE, SEMIBOLD), theme.on_menu_title());
        if let Some(w) = sub {
            caption(c, theme, &self.subtitle, strip.x1 - 36.0 - w, strip.center().y, theme.text3());
        }
        let first = self.first_for(focus.row);
        let last = self.items.len().min(first + SHOWN);
        // More above or below: arrows over the dimmed screen.
        if first > 0 {
            icon(c, Icon::ChevronUp, Point::new(f.center().x, f.y0 - 28.0), 32.0, theme.text2());
        }
        if last < self.items.len() {
            icon(c, Icon::ChevronDown, Point::new(f.center().x, f.y1 + 28.0), 32.0, theme.text2());
        }
        for (i, item) in self.items.iter().enumerate().take(last).skip(first) {
            let r = self.item_rect(i, first);
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
                if i + 1 < last && focus.row != i + 1 {
                    c.fill_rect(Rect::new(r.x0, r.y1 - EDGE, r.x1, r.y1), theme.line());
                }
                theme.text()
            };
            // Short of the button that chooses it.
            let spec = font(BODY + 2.0, MEDIUM);
            let item = fit_text(theme, item, r.width() - 36.0 - 96.0, spec);
            text_mid(c, theme, &item, r.x0 + 36.0, r.center().y, spec, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_menu_scrolls_and_stays_on_the_screen() {
        let items: Vec<String> =
            (1..=24).map(|i| format!("Track {i}: English (SDH), commentary with the director")).collect();
        let m = Menu::new("Subtitles", &items.iter().map(String::as_str).collect::<Vec<_>>());
        let f = m.frame();
        assert!(f.y0 >= 60.0 && f.y1 <= H - 60.0, "{f:?}");
        for focus in 0..items.len() {
            let first = m.first_for(focus);
            let r = m.item_rect(focus, first);
            assert!(r.y0 >= f.y0 && r.y1 <= f.y1, "item {focus} at {r:?} in {f:?}");
            assert_eq!(m.hit(r.center(), focus), Some(focus));
        }
        // A short one doesn't scroll.
        let short = Menu::new("Netflix", &["Keep watching", "Go home"]);
        assert_eq!(short.first_for(1), 0);
        assert_eq!(short.frame().height(), STRIP_H + 2.0 * ITEM_H + 2.0 * EDGE);
    }
}
