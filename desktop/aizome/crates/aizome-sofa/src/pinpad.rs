//! The grown-ups' PIN, pressed on the controller as a console's passcode
//! is: four to eight presses of the buttons in the legend, a dot in a cell
//! for each and nothing lit, so someone watching sees that a button went
//! but not which. A floating panel with a title strip, the cells, the
//! legend, and Delete (and Done, choosing a new PIN; *Forgot it?*, asking
//! for it) under it.
//!
//! The legend's buttons are where they are on every controller: the
//! arrows, the shoulders, and the top and left face buttons. Not the two
//! that say yes and no (Nintendo's swap them), the triggers (some pads
//! have none, and others press them only part way) or a stick. So a PIN
//! made on one controller is the same presses on any other, drawn in the
//! legend as the controller in hand prints them. Each is numbered 1 to 8,
//! the digit a PIN keeps it as. On a keyboard they're those digits, or the
//! arrows, Page Up and Page Down, Tab and Space, as Chanoma's hints say.
//!
//! It checks the PIN as soon as it's as long as the one set; wrong ones
//! say so, and after a few the pad waits.

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect, Vec2};
use aizome_kit::sidestep_ui::kurbo::Circle;
use aizome_kit::sidestep_ui::{Canvas, StrokeStyle};
use aizome_kit::theme::{Role, Theme};

use super::look::*;
use super::{H, W};
use crate::pad::{Button, Family};

/// The buttons a PIN is made of, in the legend's order. A PIN is kept as
/// the digits of its buttons' places here, from 1.
pub const BUTTONS: [Button; 8] =
    [Button::Up, Button::Down, Button::Left, Button::Right, Button::L1, Button::R1, Button::North, Button::West];

/// The digit button `b` is kept as, if it's one of a PIN's.
pub fn digit(b: Button) -> Option<char> {
    let i = BUTTONS.iter().position(|x| *x == b)?;
    char::from_digit(i as u32 + 1, 10)
}

/// What a press (or a click) does on the pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Press(Button),
    Delete,
    /// A new PIN is finished.
    Done,
    /// The PIN is forgotten: sign in with the computer's account instead.
    Forgot,
}

/// What the pad shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pad {
    pub title: String,
    pub prompt: String,
    /// Presses so far (shown as dots).
    pub typed: usize,
    /// How many make a PIN, when known.
    pub length: Option<usize>,
    pub error: Option<String>,
    /// Seconds until it may be tried again.
    pub wait: Option<u64>,
    pub checking: bool,
    /// Whose buttons the legend shows: the controller pressed last, or a
    /// keyboard's keys.
    pub family: Family,
    /// That controller's buttons for Done and Delete.
    pub ok: Button,
    pub back: Button,
    /// Done finishes it (a new PIN, or one whose length wasn't kept).
    pub done: bool,
    /// *Forgot it?* is there (the PIN asked for, not a new one chosen).
    pub forgot: bool,
}

const STRIP_H: f64 = 84.0;
const WIDTH: f64 = 1120.0;
const HEIGHT: f64 = 600.0;
/// The legend's buttons: each on a face this size, a gap between them and
/// a wider one between the arrows, the shoulders and the face buttons.
const CHIP: f64 = 104.0;
const CHIP_GAP: f64 = 14.0;
const GROUP_GAP: f64 = 44.0;
const FOOT_W: f64 = 260.0;
const FOOT_H: f64 = 76.0;

pub fn frame() -> Rect {
    let top = ((H - HEIGHT) / 2.0).round();
    Rect::new((W / 2.0 - WIDTH / 2.0).round(), top, (W / 2.0 + WIDTH / 2.0).round(), top + HEIGHT)
}

/// Legend button `i`'s face.
pub fn chip_rect(i: usize) -> Rect {
    let f = frame();
    let total = 8.0 * CHIP + 5.0 * CHIP_GAP + 2.0 * GROUP_GAP;
    let groups = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0];
    let gaps = i as f64 - groups[i];
    let x = (f.center().x - total / 2.0 + i as f64 * CHIP + gaps * CHIP_GAP + groups[i] * GROUP_GAP).round();
    let y = f.y0 + STRIP_H + 216.0;
    Rect::new(x, y, x + CHIP, y + CHIP)
}

/// What's along the foot, as [`hit`] numbers it: Delete (8), then Done (9)
/// when there's one, then *Forgot it?* (10) when there's that.
fn foot(done: bool, forgot: bool) -> Vec<usize> {
    let mut v = vec![8];
    v.extend(done.then_some(9));
    v.extend(forgot.then_some(10));
    v
}

/// The foot's `k`th of `n` buttons, all of them centred.
fn foot_rect(k: usize, n: usize) -> Rect {
    let f = frame();
    let y = f.y1 - 44.0 - FOOT_H;
    let span = n as f64 * FOOT_W + (n.max(1) - 1) as f64 * 24.0;
    let x = f.center().x - span / 2.0 + k as f64 * (FOOT_W + 24.0);
    Rect::new(x.round(), y, (x + FOOT_W).round(), y + FOOT_H)
}

/// What's at design point `p`: a legend button (0 to 7), Delete (8), Done
/// (9) or *Forgot it?* (10).
pub fn hit(p: Point, done: bool, forgot: bool) -> Option<usize> {
    let foot = foot(done, forgot);
    (0..BUTTONS.len())
        .find(|i| chip_rect(*i).contains(p))
        .or_else(|| foot.iter().enumerate().find(|(k, _)| foot_rect(*k, foot.len()).contains(p)).map(|(_, id)| *id))
}

/// What a click on target `i` (from [`hit`]) does.
pub fn key_at(i: usize) -> Key {
    match i {
        8 => Key::Delete,
        9 => Key::Done,
        10 => Key::Forgot,
        _ => Key::Press(BUTTONS[i.min(BUTTONS.len() - 1)]),
    }
}

/// Where face button `b` is among the four, small: the one meant filled.
fn place(c: &mut Canvas, theme: &Theme, b: Button, at: Point) {
    let k = 9.0;
    for (bb, dx, dy) in
        [(Button::North, 0.0, -1.0), (Button::East, 1.0, 0.0), (Button::South, 0.0, 1.0), (Button::West, -1.0, 0.0)]
    {
        let p = at + Vec2::new(dx * k, dy * k);
        if bb == b {
            c.fill(&Circle::new(p, 4.5), theme.focus());
        } else {
            c.stroke(&Circle::new(p, 3.2), &StrokeStyle::new(1.8), theme.text3());
        }
    }
}

impl Pad {
    pub fn paint(&self, c: &mut Canvas, theme: &Theme) {
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
        // A cell for each press, as many as the PIN is long, a dot in
        // those pressed: the same dot whichever button it was.
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
        // The legend: never lit, whatever's pressed.
        let locked = self.wait.is_some() || self.checking;
        for (i, b) in BUTTONS.iter().enumerate() {
            let r = chip_rect(i);
            face(c, theme, r, theme.raised(), theme.line2());
            // A keyboard's wider keys made to fit.
            let w = glyph_width(theme, self.family, *b, 30.0);
            let size = if w > CHIP - 24.0 { 30.0 * (CHIP - 24.0) / w } else { 30.0 };
            // A face button's place under it: what's Y on one controller is
            // X on another, and the PIN is the place.
            let placed = matches!(b, Button::North | Button::West) && self.family != Family::Keyboard;
            let at = if placed { r.center() - Vec2::new(0.0, 12.0) } else { r.center() };
            button_glyph(c, theme, self.family, *b, at, size);
            if placed {
                place(c, theme, *b, Point::new(r.center().x, r.y1 - 20.0));
            }
            // Its number in the corner, the digit a PIN keeps it as.
            let n = (i + 1).to_string();
            text_mid(c, theme, &n, r.x0 + 11.0, r.y0 + 19.0, mono(CAPTION, MEDIUM), theme.text3());
            if locked {
                c.fill_rect(r, theme.raised().with_alpha(0.6));
            }
        }
        let ids = foot(self.done, self.forgot);
        let foot = ids.iter().enumerate().map(|(k, id)| {
            let (b, what) = match id {
                8 => (self.back, "Delete"),
                9 => (self.ok, "Done"),
                _ => (Button::Start, "Forgot it?"),
            };
            (foot_rect(k, ids.len()), b, what)
        });
        for (r, b, what) in foot {
            face(c, theme, r, theme.raised(), theme.line2());
            let w = hint_width(theme, self.family, b, what);
            hint(c, theme, self.family, b, what, r.center().x - w / 2.0, r.center().y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_is_the_legends_places_and_its_buttons_are_where_they_are_on_any_controller() {
        assert_eq!(digit(Button::Up), Some('1'));
        assert_eq!(digit(Button::West), Some('8'));
        // Yes and no swap between makers; triggers and sticks aren't on
        // every controller: none of them is a PIN's.
        for b in [Button::South, Button::East, Button::L2, Button::R2, Button::Start, Button::Select, Button::Guide] {
            assert_eq!(digit(b), None, "{b:?}");
        }
        assert_eq!(hit(chip_rect(4).center(), false, false), Some(4));
        assert_eq!(key_at(4), Key::Press(Button::L1));
        assert_eq!(hit(foot_rect(0, 1).center(), false, false), Some(8));
        assert_eq!(hit(foot_rect(1, 2).center(), true, false), Some(9));
        assert_eq!(key_at(9), Key::Done);
        // Asked for the PIN: Delete, then *Forgot it?*.
        assert_eq!(hit(foot_rect(1, 2).center(), false, true), Some(10));
        assert_eq!(key_at(10), Key::Forgot);
        // The legend fits the pad, three along its foot too.
        assert!(chip_rect(0).x0 > frame().x0 && chip_rect(7).x1 < frame().x1);
        assert!(chip_rect(0).y1 < foot_rect(0, 3).y0);
        assert!(foot_rect(0, 3).x0 > frame().x0 && foot_rect(2, 3).x1 < frame().x1);
    }
}
