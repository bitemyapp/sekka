//! The grown-ups' PIN pad: a dot for each digit typed, and digits in a
//! telephone's three by four, moved through with the arrows (or typed on
//! a keyboard). It checks the PIN as soon as it's as long as the one set;
//! wrong ones shake out with a message, and after a few the pad waits.

use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::kurbo::{Circle, RoundedRect};
use aizome_kit::sidestep_ui::{Canvas, Color, StrokeStyle};
use aizome_kit::theme::Theme;

use super::look::*;
use super::{Focus, W};

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

const KEY: f64 = 116.0;
const KEY_GAP: f64 = 22.0;
const TOP: f64 = 110.0;

fn frame() -> Rect {
    let w = 3.0 * KEY + 2.0 * KEY_GAP + 200.0;
    Rect::new(W / 2.0 - w / 2.0, TOP, W / 2.0 + w / 2.0, TOP + 880.0)
}

/// Key `i`'s rectangle.
pub fn key_rect(i: usize) -> Rect {
    let f = frame();
    let x = f.center().x - (1.5 * KEY + KEY_GAP) + (i % 3) as f64 * (KEY + KEY_GAP);
    let y = f.y0 + 290.0 + (i / 3) as f64 * (KEY + KEY_GAP);
    Rect::new(x, y, x + KEY, y + KEY)
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
        panel(c, f);
        padlock(c, Point::new(f.center().x, f.y0 + 62.0), 54.0, FOCUS);
        text_centred(c, theme, &self.title, Point::new(f.center().x, f.y0 + 132.0), font(42.0, 800.0), INK);
        let (line, color) = match (&self.wait, &self.error) {
            (Some(s), _) => (format!("Too many tries. Wait {}:{:02}", s / 60, s % 60), DANGER),
            (None, Some(e)) => (e.clone(), DANGER),
            (None, None) if self.checking => ("Checking…".into(), INK_SOFT),
            (None, None) => (self.prompt.clone(), INK_SOFT),
        };
        text_centred(c, theme, &line, Point::new(f.center().x, f.y0 + 184.0), font(28.0, 600.0), color);
        // The dots: as many as the PIN is long, filled as typed.
        let n = self.length.unwrap_or(self.typed.max(4)).max(self.typed);
        let spacing = 44.0;
        let x0 = f.center().x - spacing * (n as f64 - 1.0) / 2.0;
        for i in 0..n {
            let at = Point::new(x0 + i as f64 * spacing, f.y0 + 238.0);
            if i < self.typed {
                c.fill(&Circle::new(at, 13.0), INK);
            } else {
                c.stroke(&Circle::new(at, 12.0), &StrokeStyle::new(3.0), INK_FAINT);
            }
        }
        let locked = self.wait.is_some() || self.checking;
        for (i, k) in KEYS.iter().enumerate() {
            let r = key_rect(i);
            let on = focus.row == i && !locked;
            let fill = if on { INK } else { Color::hex(0x2c3474) };
            c.fill(&RoundedRect::from_rect(r, 30.0), fill);
            let ink = if on {
                Color::hex(0x141a3a)
            } else if locked {
                INK_FAINT
            } else {
                INK
            };
            match k {
                Key::Digit(d) => text_centred(c, theme, &d.to_string(), r.center(), font(52.0, 800.0), ink),
                Key::Erase => text_centred(c, theme, "⌫", r.center(), font(44.0, 700.0), ink),
                Key::Done => text_centred(c, theme, "OK", r.center(), font(36.0, 800.0), ink),
            }
            if on {
                focus_ring(c, r, 30.0);
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
