//! A menu over what's playing (a game paused, a page, a film): a title,
//! big choices one above the other, the first ready to press.

use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::kurbo::RoundedRect;
use aizome_kit::sidestep_ui::{Canvas, Color, Image, ImageOptions};
use aizome_kit::theme::Theme;

use super::look::*;
use super::{Focus, W};
use crate::pad::{Button, Family};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Menu {
    pub title: String,
    pub subtitle: String,
    pub items: Vec<String>,
    /// A picture key (where a game was left), drawn above the title.
    pub picture: Option<std::path::PathBuf>,
}

const ITEM_H: f64 = 92.0;
const WIDTH: f64 = 760.0;

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

    fn frame(&self) -> Rect {
        let h = 150.0
            + if self.subtitle.is_empty() { 0.0 } else { 48.0 }
            + self.items.len() as f64 * (ITEM_H + 16.0)
            + 40.0;
        let top = (1080.0 - h) / 2.0;
        Rect::new(W / 2.0 - WIDTH / 2.0, top, W / 2.0 + WIDTH / 2.0, top + h)
    }

    fn item_rect(&self, i: usize) -> Rect {
        let f = self.frame();
        let y = f.y0 + 140.0 + if self.subtitle.is_empty() { 0.0 } else { 48.0 } + i as f64 * (ITEM_H + 16.0);
        Rect::new(f.x0 + 48.0, y, f.x1 - 48.0, y + ITEM_H)
    }

    pub fn hit(&self, p: Point) -> Option<usize> {
        (0..self.items.len()).find(|i| self.item_rect(*i).contains(p))
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus, family: Family, ok: Button) {
        let f = self.frame();
        panel(c, f);
        text_centred(c, theme, &self.title, Point::new(f.center().x, f.y0 + 78.0), font(48.0, 800.0), INK);
        if !self.subtitle.is_empty() {
            text_centred(c, theme, &self.subtitle, Point::new(f.center().x, f.y0 + 140.0), font(30.0, 600.0), INK_SOFT);
        }
        for (i, item) in self.items.iter().enumerate() {
            let r = self.item_rect(i);
            let on = focus.row == i;
            c.fill(&RoundedRect::from_rect(r, 24.0), if on { INK } else { Color::hex(0x2c3474) });
            text_centred(c, theme, item, r.center(), font(38.0, 800.0), if on { Color::hex(0x141a3a) } else { INK });
            if on {
                focus_ring(c, r, 24.0);
                button_glyph(c, theme, family, ok, Point::new(r.x1 - 50.0, r.center().y), 24.0);
            }
        }
    }
}

/// A picture inside a rectangle, its shape kept.
pub fn picture(c: &mut Canvas, img: &Image, r: Rect) {
    let (iw, ih) = img.pixel_size();
    let s = (r.width() / f64::from(iw)).min(r.height() / f64::from(ih));
    let dst = Rect::from_center_size(r.center(), aizome_kit::layout::Size::new(f64::from(iw) * s, f64::from(ih) * s));
    c.draw_image_with(img, None, dst, ImageOptions { smooth: false, ..ImageOptions::default() });
}
