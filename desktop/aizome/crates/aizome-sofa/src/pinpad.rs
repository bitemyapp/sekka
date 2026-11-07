//! A PIN pressed on a controller as a console's passcode is: four to eight
//! presses of the buttons in the legend, a dot in a cell for each and
//! nothing lit, so someone watching sees that a button went but not
//! which. A floating panel with a title strip, the cells, the legend, and
//! Delete (and Done, choosing a new PIN; *Forgot it?*, asking for it)
//! under it. Chanoma's grown-ups' PIN, and the TV account's at the login
//! screen.
//!
//! **Ten digits.** Each digit has a button that's in the same place on
//! every controller: the arrows (1 to 4), the shoulders (5, 6), the top
//! and left face buttons (7, 8) and the triggers (9, 0), so the legend
//! reads 1 to 9 and then 0, as a keyboard's row of numbers does. Not the
//! two face buttons that say yes and no (Nintendo's swap them), nor a
//! stick. A PIN made on one controller is the same presses on any other,
//! drawn in the legend as the controller in hand prints them. On a
//! keyboard they're the digits themselves (or the arrows, Page Up and
//! Page Down, Tab and Space, as Chanoma's hints say).
//!
//! **Chords, for a controller short of buttons.** A pad shaped as an old
//! console's may have no triggers (a Super pad's), or no shoulders or top
//! buttons either (a NES pad's). Its missing digits are pressed with
//! Select held, Select being shift ([`presses`]): Select with a shoulder
//! is the trigger behind it, Select with an arrow is the digit four on,
//! and failing those the face buttons stand in. Only a controller that
//! lacks a digit's own button gets a chord for it, and the legend draws
//! the chord in that digit's place. [`Chords`] turns a controller's
//! presses into digits.
//!
//! It checks the PIN as soon as it's as long as the one set; wrong ones
//! say so, and after a few the pad waits.

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect, Vec2};
use aizome_kit::sidestep_ui::kurbo::Circle;
use aizome_kit::sidestep_ui::{Canvas, StrokeStyle};
use aizome_kit::theme::{Role, Theme};

use crate::look::*;
use crate::{H, W};
use aizome_gamepad::{Button, Family};

/// The buttons a PIN is made of, in the legend's order: digit 1's button
/// first, then 2's, and 0's last. A PIN is kept as its digits.
pub const BUTTONS: [Button; 10] = [
    Button::Up,
    Button::Down,
    Button::Left,
    Button::Right,
    Button::L1,
    Button::R1,
    Button::North,
    Button::West,
    Button::L2,
    Button::R2,
];

/// The digit button `b` is kept as, if it's one of a PIN's.
pub fn digit(b: Button) -> Option<char> {
    let i = BUTTONS.iter().position(|x| *x == b)?;
    char::from_digit((i as u32 + 1) % 10, 10)
}

/// The button digit `d` (0 to 9) is pressed with.
pub fn button_of(d: u8) -> Option<Button> {
    match d {
        0 => Some(BUTTONS[9]),
        1..=9 => Some(BUTTONS[usize::from(d) - 1]),
        _ => None,
    }
}

/// How a controller presses one of a PIN's digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Press {
    /// Its own button.
    Button(Button),
    /// The controller hasn't that: shift (Select, or Start on a pad with
    /// no Select) held, and this.
    Chord(Button, Button),
}

/// Which buttons stand in for each digit's, in order of preference, on a
/// controller without it: the shoulders' triggers come from the shoulders,
/// the shoulders and the top buttons from the arrows, and the face buttons
/// stand in where nothing else is left.
const STAND_INS: [&[Button]; 10] = [
    &[],
    &[],
    &[],
    &[],
    &[Button::Up],
    &[Button::Down],
    &[Button::Left],
    &[Button::Right],
    &[Button::L1, Button::West, Button::South],
    &[Button::R1, Button::North, Button::East],
];

/// The button that's shift for chords on a controller with buttons
/// `have`, if it needs chords at all.
pub fn shift(have: u32) -> Option<Button> {
    let has = |b: Button| have & b.bit() != 0;
    if BUTTONS.iter().all(|b| has(*b)) {
        None
    } else if has(Button::Select) {
        Some(Button::Select)
    } else if has(Button::Start) {
        Some(Button::Start)
    } else {
        None
    }
}

/// How a controller with buttons `have` (`Button::bit`s) presses each
/// digit, in the legend's order: `None` for a digit it can't press at all
/// (no button, and no shift to make a chord with).
pub fn presses(have: u32) -> [Option<Press>; 10] {
    let has = |b: Button| have & b.bit() != 0;
    let shift = shift(have);
    let mut out = [None; 10];
    let mut used: Vec<Button> = Vec::new();
    for (i, b) in BUTTONS.iter().enumerate() {
        if has(*b) {
            out[i] = Some(Press::Button(*b));
        }
    }
    let Some(shift) = shift else { return out };
    for (i, stand_ins) in STAND_INS.iter().enumerate() {
        if out[i].is_some() {
            continue;
        }
        if let Some(s) = stand_ins.iter().find(|s| has(**s) && **s != shift && !used.contains(s)) {
            used.push(*s);
            out[i] = Some(Press::Chord(shift, *s));
        }
    }
    out
}

/// What a controller's press means on the pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Chorded {
    /// One of the PIN's digits, by its legend button (`L2` for 9, whether
    /// pressed or chorded).
    Digit(Button),
    /// A button that isn't a digit here, pressed: the program's (yes, no,
    /// Start).
    Other(Button),
    /// Nothing: shift going down or up, a button let go.
    Nothing,
}

/// Each controller's shift, held or not, turning its presses into digits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chords {
    /// The controllers holding their shift, by their connection's number.
    held: Vec<u32>,
}

impl Chords {
    /// Button `b` of controller `id`, which has buttons `have`, went down
    /// (`pressed`) or up.
    pub fn button(&mut self, id: u32, have: u32, b: Button, pressed: bool) -> Chorded {
        let shift = shift(have);
        if Some(b) == shift {
            self.held.retain(|x| *x != id);
            if pressed {
                self.held.push(id);
            }
            return Chorded::Nothing;
        }
        if !pressed {
            return Chorded::Nothing;
        }
        let table = presses(have);
        if let Some(s) = shift
            && self.held.contains(&id)
            && let Some(i) = table.iter().position(|p| *p == Some(Press::Chord(s, b)))
        {
            return Chorded::Digit(BUTTONS[i]);
        }
        // A slip of shift, or a controller with every button: the button
        // as it is.
        if table.contains(&Some(Press::Button(b))) { Chorded::Digit(b) } else { Chorded::Other(b) }
    }

    /// Controller `id` went: its shift is let go.
    pub fn forget(&mut self, id: u32) {
        self.held.retain(|x| *x != id);
    }
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    /// The buttons that controller has (`Button::bit`s), for the chords
    /// its missing ones need.
    pub have: u32,
    /// That controller's buttons for Done and Delete.
    pub ok: Button,
    pub back: Button,
    /// Done finishes it (a new PIN, or one whose length wasn't kept).
    pub done: bool,
    /// *Forgot it?* is there (the PIN asked for, not a new one chosen).
    pub forgot: bool,
}

impl Default for Pad {
    fn default() -> Pad {
        Pad {
            title: String::new(),
            prompt: String::new(),
            typed: 0,
            length: None,
            error: None,
            wait: None,
            checking: false,
            family: Family::default(),
            have: Button::all_bits(),
            ok: Button::South,
            back: Button::East,
            done: false,
            forgot: false,
        }
    }
}

const STRIP_H: f64 = 84.0;
const WIDTH: f64 = 1360.0;
const HEIGHT: f64 = 600.0;
/// The legend's buttons: each on a face this size, a gap between them and
/// a wider one between the arrows, the shoulders, the face buttons and the
/// triggers.
const CHIP: f64 = 104.0;
const CHIP_GAP: f64 = 14.0;
const GROUP_GAP: f64 = 44.0;
const FOOT_W: f64 = 260.0;
const FOOT_H: f64 = 76.0;

/// What's along the foot, as [`hit`] numbers it after the legend's ten.
pub const DELETE: usize = 10;
pub const DONE: usize = 11;
pub const FORGOT: usize = 12;

pub fn frame() -> Rect {
    let top = ((H - HEIGHT) / 2.0).round();
    Rect::new((W / 2.0 - WIDTH / 2.0).round(), top, (W / 2.0 + WIDTH / 2.0).round(), top + HEIGHT)
}

/// Legend button `i`'s face.
pub fn chip_rect(i: usize) -> Rect {
    let f = frame();
    let groups = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0];
    let n = BUTTONS.len() as f64;
    let total = n * CHIP + (n - 4.0) * CHIP_GAP + 3.0 * GROUP_GAP;
    let gaps = i as f64 - groups[i];
    let x = (f.center().x - total / 2.0 + i as f64 * CHIP + gaps * CHIP_GAP + groups[i] * GROUP_GAP).round();
    let y = f.y0 + STRIP_H + 216.0;
    Rect::new(x, y, x + CHIP, y + CHIP)
}

/// What's along the foot: Delete, then Done when there's one, then
/// *Forgot it?* when there's that.
fn foot(done: bool, forgot: bool) -> Vec<usize> {
    let mut v = vec![DELETE];
    v.extend(done.then_some(DONE));
    v.extend(forgot.then_some(FORGOT));
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

/// What's at design point `p`: a legend button (0 to 9), [`DELETE`],
/// [`DONE`] or [`FORGOT`].
pub fn hit(p: Point, done: bool, forgot: bool) -> Option<usize> {
    let foot = foot(done, forgot);
    (0..BUTTONS.len())
        .find(|i| chip_rect(*i).contains(p))
        .or_else(|| foot.iter().enumerate().find(|(k, _)| foot_rect(*k, foot.len()).contains(p)).map(|(_, id)| *id))
}

/// What a click on target `i` (from [`hit`]) does.
pub fn key_at(i: usize) -> Key {
    match i {
        DELETE => Key::Delete,
        DONE => Key::Done,
        FORGOT => Key::Forgot,
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

/// Is a face button, whose place is drawn under it: what's Y on one
/// controller is X on another, and the PIN is the place.
fn placed(b: Button) -> bool {
    matches!(b, Button::North | Button::West | Button::South | Button::East)
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
        let keyboard = self.family == Family::Keyboard;
        let presses = if keyboard { presses(Button::all_bits()) } else { presses(self.have) };
        for (i, b) in BUTTONS.iter().enumerate() {
            let r = chip_rect(i);
            face(c, theme, r, theme.raised(), theme.line2());
            let d = char::from_digit((i as u32 + 1) % 10, 10).unwrap_or('0');
            match presses[i] {
                // The triggers on a keyboard are their digits.
                _ if keyboard && matches!(b, Button::L2 | Button::R2) => {
                    keycap(c, theme, &d.to_string(), r.center(), 30.0);
                }
                Some(Press::Button(b)) => self.paint_button(c, theme, b, r, 30.0),
                Some(Press::Chord(s, b)) => {
                    // "Select +" small along the top, the button under it.
                    let y = r.y0 + 28.0;
                    button_glyph(c, theme, self.family, s, Point::new(r.center().x - 10.0, y), 16.0);
                    text_centred(c, theme, "+", Point::new(r.center().x + 22.0, y), mono(SMALL, MEDIUM), theme.text2());
                    let at = Rect::new(r.x0, r.y0 + 40.0, r.x1, r.y1);
                    self.paint_button(c, theme, b, at, 24.0);
                }
                None => text_centred(c, theme, "—", r.center(), font(TITLE, REGULAR), theme.text3()),
            }
            // Its number in the corner, the digit a PIN keeps it as.
            text_mid(c, theme, &d.to_string(), r.x0 + 11.0, r.y0 + 19.0, mono(CAPTION, MEDIUM), theme.text3());
            if locked {
                c.fill_rect(r, theme.raised().with_alpha(0.6));
            }
        }
        let ids = foot(self.done, self.forgot);
        let foot = ids.iter().enumerate().map(|(k, id)| {
            let (b, what) = match *id {
                DELETE => (self.back, "Delete"),
                DONE => (self.ok, "Done"),
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

    /// Button `b` in the legend face `r`, `size` its radius: a keyboard's
    /// wider keys made to fit, and a face button's place under it.
    fn paint_button(&self, c: &mut Canvas, theme: &Theme, b: Button, r: Rect, size: f64) {
        let w = glyph_width(theme, self.family, b, size);
        let size = if w > CHIP - 24.0 { size * (CHIP - 24.0) / w } else { size };
        let placed = placed(b) && self.family != Family::Keyboard;
        let at = if placed { r.center() - Vec2::new(0.0, size * 0.4) } else { r.center() };
        button_glyph(c, theme, self.family, b, at, size);
        if placed {
            place(c, theme, b, Point::new(r.center().x, r.y1 - 20.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(bs: &[Button]) -> u32 {
        bs.iter().fold(0, |b, x| b | x.bit())
    }

    #[test]
    fn a_pin_is_the_legends_places_and_its_buttons_are_where_they_are_on_any_controller() {
        assert_eq!(digit(Button::Up), Some('1'));
        assert_eq!(digit(Button::West), Some('8'));
        assert_eq!(digit(Button::L2), Some('9'));
        assert_eq!(digit(Button::R2), Some('0'));
        assert_eq!(button_of(9), Some(Button::L2));
        assert_eq!(button_of(0), Some(Button::R2));
        assert_eq!(button_of(1), Some(Button::Up));
        // Yes and no swap between makers; sticks aren't on every
        // controller: none of them is a PIN's.
        for b in [Button::South, Button::East, Button::Start, Button::Select, Button::Guide, Button::L3] {
            assert_eq!(digit(b), None, "{b:?}");
        }
        assert_eq!(hit(chip_rect(4).center(), false, false), Some(4));
        assert_eq!(key_at(4), Key::Press(Button::L1));
        assert_eq!(key_at(9), Key::Press(Button::R2));
        assert_eq!(hit(foot_rect(0, 1).center(), false, false), Some(DELETE));
        assert_eq!(hit(foot_rect(1, 2).center(), true, false), Some(DONE));
        assert_eq!(key_at(DONE), Key::Done);
        // Asked for the PIN: Delete, then *Forgot it?*.
        assert_eq!(hit(foot_rect(1, 2).center(), false, true), Some(FORGOT));
        assert_eq!(key_at(FORGOT), Key::Forgot);
        // The legend fits the pad, in order, three along its foot too.
        assert!(chip_rect(0).x0 > frame().x0 + 40.0 && chip_rect(9).x1 < frame().x1 - 40.0);
        assert!((1..10).all(|i| chip_rect(i).x0 > chip_rect(i - 1).x1));
        assert!(chip_rect(0).y1 < foot_rect(0, 3).y0);
        assert!(foot_rect(0, 3).x0 > frame().x0 && foot_rect(2, 3).x1 < frame().x1);
        assert!(frame().x0 > 0.0 && frame().x1 < W);
    }

    #[test]
    fn a_controller_with_every_button_needs_no_chords() {
        let all = Button::all_bits();
        assert_eq!(shift(all), None);
        assert!(presses(all).iter().zip(BUTTONS).all(|(p, b)| *p == Some(Press::Button(b))));
        let mut c = Chords::default();
        assert_eq!(c.button(1, all, Button::Select, true), Chorded::Other(Button::Select));
        assert_eq!(c.button(1, all, Button::L1, true), Chorded::Digit(Button::L1));
        assert_eq!(c.button(1, all, Button::R2, true), Chorded::Digit(Button::R2));
        assert_eq!(c.button(1, all, Button::R2, false), Chorded::Nothing);
        assert_eq!(c.button(1, all, Button::South, true), Chorded::Other(Button::South));
    }

    #[test]
    fn a_pad_without_triggers_presses_nine_and_zero_with_select_and_a_shoulder() {
        // Nintendo's Super pad: arrows, four face buttons, two shoulders.
        let snes = bits(&[
            Button::Up,
            Button::Down,
            Button::Left,
            Button::Right,
            Button::South,
            Button::East,
            Button::West,
            Button::North,
            Button::L1,
            Button::R1,
            Button::Select,
            Button::Start,
        ]);
        let p = presses(snes);
        assert_eq!(p[8], Some(Press::Chord(Button::Select, Button::L1)));
        assert_eq!(p[9], Some(Press::Chord(Button::Select, Button::R1)));
        assert_eq!(p[4], Some(Press::Button(Button::L1)), "a chord only where the button's missing");
        let mut c = Chords::default();
        assert_eq!(c.button(1, snes, Button::L1, true), Chorded::Digit(Button::L1));
        assert_eq!(c.button(1, snes, Button::Select, true), Chorded::Nothing);
        assert_eq!(c.button(1, snes, Button::L1, true), Chorded::Digit(Button::L2), "9");
        assert_eq!(c.button(1, snes, Button::L1, false), Chorded::Nothing);
        assert_eq!(c.button(1, snes, Button::R1, true), Chorded::Digit(Button::R2), "0");
        // Another controller's Select is its own.
        assert_eq!(c.button(2, snes, Button::R1, true), Chorded::Digit(Button::R1));
        // A slip: Select with an arrow is the arrow.
        assert_eq!(c.button(1, snes, Button::Up, true), Chorded::Digit(Button::Up));
        assert_eq!(c.button(1, snes, Button::Select, false), Chorded::Nothing);
        assert_eq!(c.button(1, snes, Button::R1, true), Chorded::Digit(Button::R1));
        // Gone while holding Select, and back.
        c.button(1, snes, Button::Select, true);
        c.forget(1);
        assert_eq!(c.button(1, snes, Button::L1, true), Chorded::Digit(Button::L1));
    }

    #[test]
    fn a_nes_pad_presses_every_digit_and_chords_are_never_shared() {
        let nes = bits(&[
            Button::Up,
            Button::Down,
            Button::Left,
            Button::Right,
            Button::South,
            Button::East,
            Button::Select,
            Button::Start,
        ]);
        let p = presses(nes);
        assert!(p.iter().all(Option::is_some), "{p:?}");
        assert_eq!(p[4], Some(Press::Chord(Button::Select, Button::Up)));
        assert_eq!(p[8], Some(Press::Chord(Button::Select, Button::South)));
        assert_eq!(p[9], Some(Press::Chord(Button::Select, Button::East)));
        // Every controller with the arrows, yes and no and Select, whatever
        // else it lacks: each digit pressed one way, and no two alike.
        let optional = [Button::L1, Button::R1, Button::North, Button::West, Button::L2, Button::R2];
        for mask in 0u32..(1 << optional.len()) {
            let mut have = nes;
            for (k, b) in optional.iter().enumerate() {
                if mask & (1 << k) != 0 {
                    have |= b.bit();
                }
            }
            let p = presses(have);
            assert!(p.iter().all(Option::is_some), "{mask:b}: {p:?}");
            for i in 0..10 {
                for j in 0..i {
                    assert_ne!(p[i], p[j], "{mask:b}: digits {} and {}", i + 1, j + 1);
                }
            }
            // And each chord comes back as its digit.
            let mut c = Chords::default();
            for (i, press) in p.iter().enumerate() {
                let got = match press.unwrap() {
                    Press::Button(b) => c.button(7, have, b, true),
                    Press::Chord(s, b) => {
                        c.button(7, have, s, true);
                        let got = c.button(7, have, b, true);
                        c.button(7, have, s, false);
                        got
                    }
                };
                assert_eq!(got, Chorded::Digit(BUTTONS[i]), "{mask:b}: digit {}", i + 1);
            }
        }
        // Without Select, Start is shift (a GameCube pad's).
        let cube = bits(&[Button::Up, Button::Down, Button::Left, Button::Right, Button::South, Button::East])
            | bits(&[Button::North, Button::West, Button::R1, Button::L2, Button::R2, Button::Start]);
        assert_eq!(shift(cube), Some(Button::Start));
        assert_eq!(presses(cube)[4], Some(Press::Chord(Button::Start, Button::Up)));
    }
}
