//! The keyboard on the screen, for typing with a controller where nothing
//! else types: a service's text boxes in Chanoma (a sign-in, a search), a
//! password at the login screen. A floating panel along the foot of the
//! screen, what's in the box above its keys (dots for a password), as the
//! box may be under the panel, and the keys in rows as a phone's are,
//! letters and a page of symbols. The arrows or the stick move between
//! keys and the confirm button presses one; the other buttons do what's
//! typed most ([`Keyboard::action`]), as the hints along the panel's foot
//! say.
//!
//! What's typed goes where the program says: [`Keyboard::press`] gives
//! [`Typed`] text and edits, and the program puts them in its box and
//! tells the keyboard what the box holds now ([`Field`]).

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::Theme;

use crate::look::*;
use crate::{H, W};
use aizome_gamepad::{Button, Dir, Family};

/// The box typed in, as the keyboard shows it above its keys.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Field {
    /// What it's for: its label, its placeholder or its name.
    pub label: String,
    /// What's in it; nothing for a password, whose length alone is told.
    pub text: String,
    pub length: usize,
    /// Where typing goes in it, in characters from the start.
    pub caret: usize,
    pub secret: bool,
}

/// Keys that edit rather than type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edit {
    Backspace,
    Enter,
    /// The caret back or on.
    Left,
    Right,
}

/// What a controller's button does while the keyboard is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// A key pressed: the one the controller is on, or one with a button
    /// of its own.
    Key(Key),
    Edit(Edit),
    /// Put the keyboard away.
    Close,
}

/// A key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    /// Several characters at once.
    Text(&'static str),
    Shift,
    /// Letters, or symbols.
    Page,
    Space,
    Backspace,
    Enter,
}

/// Capitals: for the next letter, or until Shift again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Shift {
    #[default]
    Off,
    Once,
    Lock,
}

/// What a key typed, for the box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    Text(String),
    Edit(Edit),
}

/// Five rows of keys, each ten wide (a key's width in tenths of a row).
type Rows = [Vec<(Key, u8)>; 5];

fn chars(s: &str) -> Vec<(Key, u8)> {
    s.chars().map(|c| (Key::Char(c), 1)).collect()
}

fn letters() -> Rows {
    let mut third = vec![(Key::Shift, 1)];
    third.extend(chars("zxcvbnm."));
    third.push((Key::Backspace, 1));
    [
        chars("1234567890"),
        chars("qwertyuiop"),
        chars("asdfghjkl@"),
        third,
        vec![(Key::Page, 2), (Key::Char('-'), 1), (Key::Space, 4), (Key::Char('_'), 1), (Key::Enter, 2)],
    ]
}

fn symbols() -> Rows {
    let mut third = chars("+=[]{}<>|");
    third.push((Key::Backspace, 1));
    [
        chars("1234567890"),
        chars("!@#$%^&*()"),
        chars("'\":;/\\?,~`"),
        third,
        vec![(Key::Page, 2), (Key::Text(".com"), 2), (Key::Space, 3), (Key::Char('-'), 1), (Key::Enter, 2)],
    ]
}

/// The keyboard as it shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Keyboard {
    pub symbols: bool,
    pub shift: Shift,
    /// The key the controller is on: its row, and its place in the row.
    pub at: (usize, usize),
    /// The box typed in, when the page says.
    pub field: Option<Field>,
    /// Typing goes into a frame from another site, which isn't asked.
    pub frame: bool,
    /// Whose buttons the hints show, and that controller's yes and no.
    pub family: Family,
    pub ok: Button,
    pub back: Button,
}

/// A key's height, the gap between keys, and a tenth of a row.
const KEY_H: f64 = 64.0;
const GAP: f64 = 8.0;
const UNIT: f64 = 116.0;
const PAD: f64 = 40.0;
const ROW_W: f64 = 10.0 * UNIT + 9.0 * GAP;
const HEIGHT: f64 = 24.0 + 86.0 + 20.0 + 5.0 * KEY_H + 4.0 * GAP + 18.0 + 44.0 + 22.0;
/// The keys' first row, below what's in the box.
const KEYS_TOP: f64 = 24.0 + 86.0 + 20.0;

impl Keyboard {
    pub fn new(field: Option<Field>) -> Keyboard {
        // On the first letter.
        Keyboard { field, at: (1, 0), ..Keyboard::default() }
    }

    pub fn rows(&self) -> Rows {
        if self.symbols { symbols() } else { letters() }
    }

    /// The key the controller is on.
    pub fn key(&self) -> Key {
        let rows = self.rows();
        rows[self.at.0.min(4)].get(self.at.1).or(rows[self.at.0.min(4)].last()).map_or(Key::Space, |k| k.0)
    }

    pub fn frame() -> Rect {
        let w = ROW_W + 2.0 * PAD;
        let x0 = (W / 2.0 - w / 2.0).round();
        let y1 = H - 32.0;
        Rect::new(x0, (y1 - HEIGHT).round(), x0 + w, y1)
    }

    /// Key `i` of row `row`'s face.
    pub fn rect(&self, row: usize, i: usize) -> Rect {
        let f = Keyboard::frame();
        let rows = self.rows();
        let before: u8 = rows[row].iter().take(i).map(|k| k.1).sum();
        let w = f64::from(rows[row].get(i).map_or(1, |k| k.1));
        let x = f.x0 + PAD + f64::from(before) * (UNIT + GAP);
        let y = f.y0 + KEYS_TOP + row as f64 * (KEY_H + GAP);
        Rect::new(x.round(), y, (x + w * UNIT + (w - 1.0) * GAP).round(), y + KEY_H)
    }

    /// The key at design point `p`, as one number (its row's place times
    /// sixteen and its own).
    pub fn hit(&self, p: Point) -> Option<usize> {
        let rows = self.rows();
        (0..5).find_map(|r| (0..rows[r].len()).find(|i| self.rect(r, *i).contains(p)).map(|i| r * 16 + i))
    }

    /// Put the controller on key `n` (from [`Keyboard::hit`]).
    pub fn go_to(&mut self, n: usize) {
        let rows = self.rows();
        let row = (n / 16).min(4);
        self.at = (row, (n % 16).min(rows[row].len() - 1));
    }

    /// Move between keys: along a row round to its other end, and up or
    /// down to the key above or below the middle of this one.
    pub fn step(&mut self, dir: Dir) {
        let rows = self.rows();
        let (row, i) = self.at;
        let n = rows[row].len();
        match dir {
            Dir::Left => self.at.1 = (i + n - 1) % n,
            Dir::Right => self.at.1 = (i + 1) % n,
            Dir::Up | Dir::Down => {
                let to = if dir == Dir::Up { row.saturating_sub(1) } else { (row + 1).min(4) };
                if to == row {
                    return;
                }
                let x = self.rect(row, i).center().x;
                let near = (0..rows[to].len())
                    .min_by(|a, b| {
                        let da = (self.rect(to, *a).center().x - x).abs();
                        let db = (self.rect(to, *b).center().x - x).abs();
                        da.total_cmp(&db)
                    })
                    .unwrap_or(0);
                self.at = (to, near);
            }
        }
    }

    /// What button `b`, pressed, does here (its controller's yes and no
    /// are [`Keyboard::ok`] and [`Keyboard::back`]): yes presses the key
    /// it's on, no closes, and the rest are what's typed most, as the
    /// hints say. Start (Enter) and Select (closing) are a program's to
    /// take when they're let go alone, since together they're Home.
    pub fn action(&self, b: Button) -> Option<Action> {
        Some(match b {
            _ if b == self.ok => Action::Key(self.key()),
            _ if b == self.back => Action::Close,
            Button::West => Action::Key(Key::Backspace),
            Button::North => Action::Key(Key::Space),
            Button::L2 => Action::Key(Key::Shift),
            Button::R2 => Action::Key(Key::Page),
            Button::L1 => Action::Edit(Edit::Left),
            Button::R1 => Action::Edit(Edit::Right),
            Button::Start => Action::Key(Key::Enter),
            Button::Select => Action::Close,
            _ => return None,
        })
    }

    /// Press `key`: what it types, if anything (Shift and the page of
    /// symbols change the keys instead).
    pub fn press(&mut self, key: Key) -> Option<Typed> {
        let typed = match key {
            Key::Char(c) if c.is_alphabetic() && self.shift != Shift::Off => {
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                }
                Typed::Text(c.to_uppercase().collect())
            }
            Key::Char(c) => Typed::Text(c.to_string()),
            Key::Text(s) => Typed::Text(s.to_owned()),
            Key::Space => Typed::Text(" ".into()),
            Key::Backspace => Typed::Edit(Edit::Backspace),
            Key::Enter => Typed::Edit(Edit::Enter),
            Key::Shift => {
                self.shift = match self.shift {
                    Shift::Off => Shift::Once,
                    Shift::Once => Shift::Lock,
                    Shift::Lock => Shift::Off,
                };
                return None;
            }
            Key::Page => {
                // The same row, the place kept where the new row has it.
                self.symbols = !self.symbols;
                let n = self.rows()[self.at.0].len();
                self.at.1 = self.at.1.min(n - 1);
                return None;
            }
        };
        Some(typed)
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme) {
        let f = Keyboard::frame();
        floating(c, theme, f);
        self.paint_box(c, theme, f);
        let rows = self.rows();
        for (r, row) in rows.iter().enumerate() {
            for (i, (key, _)) in row.iter().enumerate() {
                let rect = self.rect(r, i);
                let on = self.at == (r, i);
                let lit_shift = *key == Key::Shift && self.shift != Shift::Off;
                let ink = if on {
                    lit(c, theme, rect);
                    theme.on_selection()
                } else if lit_shift {
                    face(c, theme, rect, theme.focus(), theme.line2());
                    theme.on_focus()
                } else {
                    face(c, theme, rect, theme.raised(), theme.line2());
                    theme.text()
                };
                self.paint_key(c, theme, *key, rect, ink);
                if on {
                    ring(c, theme, rect);
                }
            }
        }
        // What the buttons do, along the foot.
        let y = f.y1 - 22.0 - 22.0;
        let mut x = f.x0 + PAD;
        x = hint(c, theme, self.family, self.ok, "Type", x, y);
        x = hint(c, theme, self.family, Button::West, "Delete", x, y);
        x = hint(c, theme, self.family, Button::North, "Space", x, y);
        let w = glyph_width(theme, self.family, Button::L1, 22.0);
        button_glyph(c, theme, self.family, Button::L1, Point::new(x + w / 2.0, y), 22.0);
        x += w + 10.0;
        x = hint(c, theme, self.family, Button::R1, "Move", x, y);
        x = hint(c, theme, self.family, Button::L2, "Shift", x, y);
        x = hint(c, theme, self.family, Button::Start, "Enter", x, y);
        hint(c, theme, self.family, self.back, "Close", x, y);
    }

    /// What's in the box, the caret where typing goes; dots for a password.
    fn paint_box(&self, c: &mut Canvas, theme: &Theme, f: Rect) {
        let well_r = Rect::new(f.x0 + PAD, f.y0 + 24.0 + 30.0, f.x1 - PAD, f.y0 + 24.0 + 86.0);
        let label = match &self.field {
            Some(field) if !field.label.is_empty() => field.label.clone(),
            Some(_) | None if self.frame => "Typing into the page".into(),
            Some(_) => "Typing".into(),
            None => "Nothing on the page takes typing here: choose a box first".into(),
        };
        let label = fit_text(theme, &label, well_r.width(), font(SMALL, REGULAR));
        text_mid(c, theme, &label, well_r.x0, f.y0 + 24.0 + 12.0, font(SMALL, REGULAR), theme.text2());
        well(c, theme, well_r);
        let Some(field) = &self.field else { return };
        let spec = font(BODY, REGULAR);
        let room = well_r.width() - 40.0;
        let (before, after) = if field.secret {
            ("•".repeat(field.length.min(48)), String::new())
        } else {
            let at = field.text.char_indices().nth(field.caret).map_or(field.text.len(), |(i, _)| i);
            (field.text[..at].to_owned(), field.text[at..].to_owned())
        };
        // What's just before the caret shows, cut from the left if it's
        // long.
        let before: Vec<char> = before.chars().collect();
        let mut from = before.len().saturating_sub(120);
        let mut shown: String = before[from..].iter().collect();
        while from < before.len() && measure(theme, &shown, spec).width > room {
            from += 1;
            shown = format!("…{}", before[from..].iter().collect::<String>());
        }
        let x = well_r.x0 + 20.0;
        let y = well_r.center().y;
        let w = text_mid(c, theme, &shown, x, y, spec, theme.text());
        let caret = Rect::new((x + w + 1.0).round(), y - 18.0, (x + w + 1.0).round() + 3.0, y + 18.0);
        c.fill_rect(caret, theme.focus());
        if !after.is_empty() {
            let rest = fit_text(theme, &after, (room - w - 8.0).max(0.0), spec);
            text_mid(c, theme, &rest, caret.x1 + 4.0, y, spec, theme.text());
        }
    }

    fn paint_key(&self, c: &mut Canvas, theme: &Theme, key: Key, r: Rect, ink: aizome_kit::sidestep_ui::Color) {
        let at = r.center();
        match key {
            Key::Char(ch) => {
                let s: String = if ch.is_alphabetic() && self.shift != Shift::Off {
                    ch.to_uppercase().collect()
                } else {
                    ch.to_string()
                };
                text_centred(c, theme, &s, at, mono(TITLE, MEDIUM), ink);
            }
            Key::Text(s) => text_centred(c, theme, s, at, mono(BODY, MEDIUM), ink),
            Key::Shift => {
                icon(c, Icon::ArrowUp, at, 30.0, ink);
                if self.shift == Shift::Lock {
                    c.fill_rect(Rect::new(at.x - 12.0, at.y + 19.0, at.x + 12.0, at.y + 22.0), ink);
                }
            }
            Key::Page => text_centred(c, theme, if self.symbols { "abc" } else { "#+=" }, at, mono(BODY, MEDIUM), ink),
            Key::Space => text_centred(c, theme, "space", at, mono(BODY, MEDIUM), ink),
            Key::Backspace => icon(c, Icon::Delete, at, 30.0, ink),
            Key::Enter => icon(c, Icon::CornerDownLeft, at, 30.0, ink),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_fill_their_rows_and_the_focus_walks_them() {
        for symbols in [false, true] {
            let k = Keyboard { symbols, ..Keyboard::new(None) };
            for row in k.rows() {
                assert_eq!(row.iter().map(|k| u32::from(k.1)).sum::<u32>(), 10, "{row:?}");
            }
            // The keys sit in the panel, below what's in the box.
            let f = Keyboard::frame();
            assert!(f.y0 > H / 3.0 && f.y1 < H);
            assert!(k.rect(4, 4).x1 <= f.x1 - PAD + 1.0 && k.rect(0, 0).x0 >= f.x0 + PAD);
            assert!(k.rect(4, 0).y1 < f.y1 - 44.0);
        }
        let mut k = Keyboard::new(None);
        assert_eq!(k.key(), Key::Char('q'));
        k.step(Dir::Left);
        assert_eq!(k.key(), Key::Char('p'), "round to the row's other end");
        // Down from the bottom row's middle to the space bar, and up again
        // to the key above the middle of it.
        k.at = (3, 5);
        k.step(Dir::Down);
        assert_eq!(k.key(), Key::Space);
        k.step(Dir::Up);
        assert_eq!(k.at.0, 3);
        k.step(Dir::Up);
        k.step(Dir::Up);
        k.step(Dir::Up);
        k.step(Dir::Up);
        assert_eq!(k.at.0, 0, "no further than the top");
        assert_eq!(k.hit(k.rect(2, 3).center()), Some(2 * 16 + 3));
        k.go_to(2 * 16 + 3);
        assert_eq!(k.key(), Key::Char('f'));
    }

    #[test]
    fn shift_is_for_one_letter_or_until_pressed_again() {
        let mut k = Keyboard::new(None);
        assert_eq!(k.press(Key::Shift), None);
        assert_eq!(k.press(Key::Char('a')), Some(Typed::Text("A".into())));
        assert_eq!(k.press(Key::Char('a')), Some(Typed::Text("a".into())));
        k.press(Key::Shift);
        k.press(Key::Shift);
        assert_eq!(k.shift, Shift::Lock);
        assert_eq!(k.press(Key::Char('b')), Some(Typed::Text("B".into())));
        assert_eq!(k.press(Key::Char('1')), Some(Typed::Text("1".into())));
        assert_eq!(k.press(Key::Char('c')), Some(Typed::Text("C".into())));
        // Symbols, and back, the place in the row kept.
        k.at = (4, 4);
        assert_eq!(k.press(Key::Page), None);
        assert!(k.symbols);
        assert_eq!(k.press(Key::Text(".com")), Some(Typed::Text(".com".into())));
        assert_eq!(k.press(Key::Enter), Some(Typed::Edit(Edit::Enter)));
    }

    #[test]
    fn the_buttons_do_what_the_hints_say_with_each_makers_yes_and_no() {
        use aizome_gamepad::Confirm;
        let mut k = Keyboard::new(None);
        k.ok = Confirm::Auto.ok(Family::Xbox);
        k.back = Confirm::Auto.back(Family::Xbox);
        assert_eq!(k.action(Button::South), Some(Action::Key(Key::Char('q'))));
        assert_eq!(k.action(Button::East), Some(Action::Close));
        assert_eq!(k.action(Button::West), Some(Action::Key(Key::Backspace)));
        assert_eq!(k.action(Button::R1), Some(Action::Edit(Edit::Right)));
        assert_eq!(k.action(Button::Start), Some(Action::Key(Key::Enter)));
        assert_eq!(k.action(Button::Guide), None);
        // Nintendo's yes is the right-hand button.
        k.ok = Confirm::Auto.ok(Family::Nintendo);
        k.back = Confirm::Auto.back(Family::Nintendo);
        assert_eq!(k.action(Button::East), Some(Action::Key(Key::Char('q'))));
        assert_eq!(k.action(Button::South), Some(Action::Close));
    }
}
