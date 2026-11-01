//! The grown-ups' PIN pad, as Aizome's lock screen asks for a password: a
//! floating panel with a title strip, a sunken cell for each digit (a dot
//! in each one typed), and the digits in a telephone's three by four,
//! moved through with the arrows (or typed on a keyboard). It checks the
//! PIN as soon as it's as long as the one set; wrong ones say so, and
//! after a few the pad waits.

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::{Role, Theme};

use super::look::*;
use super::{Focus, H, W};

/// The keys, in reading order.
pub const KEYS: [Key; 12] = [
    Key::Digit(1),
    Key::Digit(2),
    Key::Digit(3),
    Key::Digit(4),
    Key::Digit(5),
    Key::Digit(6),
    Key::Digit(7),
    Key::Digit(8),
    Key::Digit(9),
    Key::Erase,
    Key::Digit(0),
    Key::Done,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Digit(u8),
    Erase,
    Done,
}

/// What the pad shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pad {
    pub title: String,
    pub prompt: String,
    /// Digits typed (shown as dots).
    pub typed: usize,
    /// How many make a PIN, when known.
    pub length: Option<usize>,
    pub error: Option<String>,
    /// Seconds until it may be tried again.
    pub wait: Option<u64>,
    pub checking: bool,
}

const KEY_W: f64 = 136.0;
const KEY_H: f64 = 96.0;
const KEY_GAP: f64 = 16.0;
const STRIP_H: f64 = 84.0;
const HEIGHT: f64 = 770.0;

pub fn frame() -> Rect {
    let w = 3.0 * KEY_W + 2.0 * KEY_GAP + 2.0 * 72.0;
    let top = ((H - HEIGHT) / 2.0).round();
    Rect::new((W / 2.0 - w / 2.0).round(), top, (W / 2.0 + w / 2.0).round(), top + HEIGHT)
}

/// Key `i`'s rectangle.
pub fn key_rect(i: usize) -> Rect {
    let f = frame();
    let x = f.center().x - (1.5 * KEY_W + KEY_GAP) + (i % 3) as f64 * (KEY_W + KEY_GAP);
    let y = f.y0 + STRIP_H + 210.0 + (i / 3) as f64 * (KEY_H + KEY_GAP);
    Rect::new(x.round(), y, (x + KEY_W).round(), y + KEY_H)
}

pub fn hit(p: Point) -> Option<usize> {
    (0..KEYS.len()).find(|i| key_rect(*i).contains(p))
}

/// The key the focus moves to from `i` in `dir` (wrapping within a line).
pub fn step(i: usize, dir: crate::pad::Dir) -> usize {
    use crate::pad::Dir;
    let (row, col) = (i / 3, i % 3);
    match dir {
        Dir::Up => row.saturating_sub(1) * 3 + col,
        Dir::Down => (row + 1).min(3) * 3 + col,
        Dir::Left => row * 3 + (col + 2) % 3,
        Dir::Right => row * 3 + (col + 1) % 3,
    }
}

impl Pad {
    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus) {
        let f = frame();
        floating(c, theme, f);
        let strip = Rect::new(f.x0 + EDGE, f.y0 + EDGE, f.x1 - EDGE, f.y0 + STRIP_H);
        c.fill_rect(strip, theme.menu_title());
        icon(c, Icon::Lock, Point::new(strip.x0 + 36.0 + 15.0, strip.center().y), 30.0, theme.focus());
        let spec = font(TITLE, SEMIBOLD);
        text_mid(c, theme, &self.title, strip.x0 + 36.0 + 30.0 + 18.0, strip.center().y, spec, theme.on_menu_title());
        let (line, color) = match (&self.wait, &self.error) {
            (Some(s), _) => (format!("Too many tries. Wait {}:{:02}", s / 60, s % 60), theme.role(Role::Danger)),
            (None, Some(e)) => (e.clone(), theme.role(Role::Danger)),
            (None, None) if self.checking => ("Checking…".into(), theme.text2()),
            (None, None) => (self.prompt.clone(), theme.text2()),
        };
        let line = fit_text(theme, &line, f.width() - 72.0, font(BODY - 2.0, REGULAR));
        text_centred(
            c,
            theme,
            &line,
            Point::new(f.center().x, f.y0 + STRIP_H + 52.0),
            font(BODY - 2.0, REGULAR),
            color,
        );
        // A cell for each digit, as many as the PIN is long, a dot in
        // those typed.
        let n = self.length.unwrap_or(self.typed.max(4)).max(self.typed);
        let (cw, gap) = (52.0, 14.0);
        let total = n as f64 * cw + (n as f64 - 1.0) * gap;
        let x0 = (f.center().x - total / 2.0).round();
        let y0 = f.y0 + STRIP_H + 104.0;
        for i in 0..n {
            let cell = Rect::new(x0 + i as f64 * (cw + gap), y0, x0 + i as f64 * (cw + gap) + cw, y0 + 64.0);
            well(c, theme, cell);
            if i < self.typed {
                c.fill_rect(Rect::from_center_size(cell.center(), (16.0, 16.0)), theme.text());
            }
        }
        let locked = self.wait.is_some() || self.checking;
        for (i, k) in KEYS.iter().enumerate() {
            let r = key_rect(i);
            let on = focus.row == i && !locked;
            let ink = if on {
                lit(c, theme, r);
                theme.on_selection()
            } else {
                face(c, theme, r, theme.raised(), theme.line2());
                if locked { theme.faint() } else { theme.text() }
            };
            match k {
                Key::Digit(d) => text_centred(c, theme, &d.to_string(), r.center(), mono(HEADLINE - 4.0, MEDIUM), ink),
                Key::Erase => icon(c, Icon::Delete, r.center(), 40.0, ink),
                Key::Done => text_centred(c, theme, "OK", r.center(), font(TITLE, SEMIBOLD), ink),
            }
            if on {
                ring(c, theme, r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::Dir;

    #[test]
    fn the_focus_walks_the_pad_like_a_telephones() {
        assert_eq!(step(0, Dir::Down), 3);
        assert_eq!(step(9, Dir::Down), 9);
        assert_eq!(step(10, Dir::Up), 7);
        assert_eq!(step(0, Dir::Left), 2);
        assert_eq!(step(2, Dir::Right), 0);
        assert_eq!(KEYS[10], Key::Digit(0));
        assert_eq!(hit(key_rect(4).center()), Some(4));
    }
}
