//! Chanoma's keyboard over a page, for the services whose text boxes need
//! one ([`crate::services::Service::keyboard`]; a television interface,
//! YouTube's, has its own).
//!
//! - **It comes up for a box**: just after the controller chooses
//!   something on the page (or moves to it with the shoulder buttons),
//!   Chanoma asks the page what has its keyboard, and a text box chosen
//!   brings the keyboard up. A mouse's clicks don't: whoever has a mouse
//!   has a keyboard, as a rule. Select brings it up by hand, for a box in a
//!   frame from another site, which the page can't say.
//! - **It types through the browser** ([`crate::devtools`]), so the box
//!   needn't show: what's in it shows above the keys.
//! - **It goes** with the no button, Select or a click beside it; after
//!   Enter; or when the box lets the keyboard go (a search sent, another
//!   page). Home brings the page's menu as ever.

use std::time::{Duration, Instant};

use aizome_kit::Cx;

use super::{Chanoma, Msg};
use crate::devtools::{Ask, Field, Focused};
use crate::pad::{Button, Dir, Family};
use crate::ui::keyboard::{Key, Keyboard, Typed};

/// How long after a choice the page is asked what has its keyboard: time
/// for the click to land and the page to answer it.
pub const ASK_AFTER: Duration = Duration::from_millis(300);
/// How long after that a box with the keyboard still counts as chosen.
const CHOSEN_FOR: Duration = Duration::from_secs(2);
/// How often the page is asked while the keyboard is up.
const WATCH_EVERY: Duration = Duration::from_millis(700);

/// Why the page was last asked what has its keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Something was chosen with the controller, then.
    Chose(Instant),
    /// The controller moved between the page's controls.
    Moved,
}

/// The keyboard while it's up.
pub struct Typing {
    pub keys: Keyboard,
    /// Brought up with Select, not by a box chosen.
    pub by_hand: bool,
    /// A box has had the page's keyboard since it came up.
    pub had_box: bool,
}

impl Chanoma {
    /// Ask the page what has its keyboard in a moment (after a choice, or a
    /// move between its controls).
    pub fn ask_page_soon(&mut self, why: Why, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.page else { return };
        if p.keyboard && p.typing.is_none() {
            p.asked = Some(why);
            cx.after(ASK_AFTER, Msg::AskPage);
        }
    }

    /// The timer's: ask, while there's a reason to.
    pub fn ask_page(&mut self, cx: &mut Cx<Msg>) {
        match &self.page {
            Some(p) if p.typing.is_some() || p.asked.is_some() => p.browser.ask(Ask::Focused),
            _ => {
                if let Some(t) = self.keyboard_watch.take() {
                    cx.cancel(t);
                }
            }
        }
    }

    /// What has the page's keyboard, as asked.
    pub fn page_focused(&mut self, f: Focused, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.page else { return };
        let why = p.asked.take();
        let Some(t) = &mut p.typing else {
            p.field = match &f {
                Focused::Field(field) => Some(field.clone()),
                _ => None,
            };
            let chosen = matches!(why, Some(Why::Chose(at)) if at.elapsed() < CHOSEN_FOR);
            if let (true, Focused::Field(field)) = (chosen, f) {
                self.open_keyboard(false, Some(field), cx);
            }
            return;
        };
        match f {
            Focused::Field(field) => {
                t.keys.field = Some(field);
                t.keys.frame = false;
                t.had_box = true;
            }
            Focused::Frame => {
                t.keys.field = None;
                t.keys.frame = true;
            }
            // The box let the keyboard go (a search sent, another page):
            // so does Chanoma, unless it was brought up before a box was.
            Focused::Elsewhere if t.had_box || !t.by_hand => self.close_keyboard(cx),
            Focused::Elsewhere => {
                t.keys.field = None;
                t.keys.frame = false;
            }
        }
    }

    /// Bring the keyboard up over the page, the sticks Chanoma's while it's
    /// there.
    pub fn open_keyboard(&mut self, by_hand: bool, field: Option<Field>, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.page else { return };
        if !p.keyboard || p.typing.is_some() {
            return;
        }
        p.browser.bridge.release_all();
        p.browser.bridge.hold(true);
        let had_box = field.is_some();
        p.typing = Some(Typing { keys: Keyboard::new(field), by_hand, had_box });
        if by_hand {
            p.browser.ask(Ask::Focused);
        }
        if let Some(t) = self.keyboard_watch.replace(cx.every(WATCH_EVERY, Msg::AskPage)) {
            cx.cancel(t);
        }
        log::info!("chanoma: the keyboard over the page");
        self.bar_until = None;
        self.sync_hud(cx);
    }

    pub fn close_keyboard(&mut self, cx: &mut Cx<Msg>) {
        if let Some(t) = self.keyboard_watch.take() {
            cx.cancel(t);
        }
        let Some(p) = &mut self.page else { return };
        if p.typing.take().is_none() {
            return;
        }
        p.browser.bridge.hold(false);
        log::info!("chanoma: the keyboard put away");
        self.sync_hud(cx);
    }

    pub fn is_typing(&self) -> bool {
        self.page.as_ref().is_some_and(|p| p.typing.is_some())
    }

    /// A button while the keyboard is up. Select and Start count alone,
    /// when let go (together they're Home).
    pub fn keyboard_button(&mut self, b: Button, family: Family, pressed: bool, alone: bool, cx: &mut Cx<Msg>) {
        let key = match b {
            Button::Select if alone => return self.close_keyboard(cx),
            Button::Start if alone => Key::Enter,
            _ if !pressed => return,
            _ if b == self.ok_button(family) => match &self.page {
                Some(p) => p.typing.as_ref().map_or(Key::Space, |t| t.keys.key()),
                None => return,
            },
            _ if b == self.back_button(family) => return self.close_keyboard(cx),
            Button::West => Key::Backspace,
            Button::North => Key::Space,
            Button::L2 => Key::Shift,
            Button::R2 => Key::Page,
            Button::L1 => return self.keyboard_send(Typed::Key(crate::devtools::Key::Left), cx),
            Button::R1 => return self.keyboard_send(Typed::Key(crate::devtools::Key::Right), cx),
            _ => return,
        };
        self.keyboard_key(key, cx);
    }

    /// The arrows or the stick, between the keys.
    pub fn keyboard_nav(&mut self, dir: Dir) {
        if let Some(t) = self.page.as_mut().and_then(|p| p.typing.as_mut()) {
            t.keys.step(dir);
        }
    }

    /// Key `n` clicked ([`Keyboard::hit`]).
    pub fn keyboard_click(&mut self, n: usize, cx: &mut Cx<Msg>) {
        let Some(t) = self.page.as_mut().and_then(|p| p.typing.as_mut()) else { return };
        t.keys.go_to(n);
        let key = t.keys.key();
        self.keyboard_key(key, cx);
    }

    /// Press `key` on the keyboard.
    pub fn keyboard_key(&mut self, key: Key, cx: &mut Cx<Msg>) {
        let Some(t) = self.page.as_mut().and_then(|p| p.typing.as_mut()) else { return };
        if let Some(typed) = t.keys.press(key) {
            self.keyboard_send(typed, cx);
        }
    }

    /// Type into the page, then ask it what's in the box now. Enter sends
    /// what's typed, and the keyboard goes.
    fn keyboard_send(&mut self, typed: Typed, cx: &mut Cx<Msg>) {
        let Some(p) = &self.page else { return };
        let enter = typed == Typed::Key(crate::devtools::Key::Enter);
        p.browser.ask(match typed {
            Typed::Text(s) => Ask::Type(s),
            Typed::Key(k) => Ask::Key(k),
        });
        if enter {
            self.close_keyboard(cx);
        } else {
            p.browser.ask(Ask::Focused);
        }
    }
}
