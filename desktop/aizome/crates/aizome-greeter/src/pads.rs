//! The login screen with a game controller, for a computer by a
//! television (and the TV account, `aizome_sys::tv`).
//!
//! - **Nothing until it's pressed.** Controllers are read from the start
//!   (`aizome_gamepad`, listening only: nothing else is reading them
//!   here), but one switched on or plugged in changes nothing. The first
//!   press of one of its buttons, or a push of its arrows or stick, shows
//!   what it does: a ring round what it's on and its buttons along the
//!   foot. That first press does nothing else, so a controller knocked on
//!   the sofa logs nobody in.
//! - **The keyboard and the mouse win back.** A key typed or the pointer
//!   moved (but over a panel the controller brought up) puts the
//!   controller's rings and keyboard away; the login screen is as it was.
//! - **Choosing, typing, logging in.** The arrows or the stick move
//!   between the people, the field, the session and the buttons at the
//!   top; yes chooses and no goes back, each controller's own (Nintendo's
//!   the other way round). A password is typed on the keyboard Chanoma
//!   has ([`aizome_sofa::keyboard`]), which comes up by itself; the TV
//!   account's PIN on the PIN pad ([`aizome_sofa::pinpad`]); Start logs
//!   in. Home (or Select and Start held) opens the panel that says all
//!   this, as the controller button at the top right does for a mouse.

use aizome_gamepad::{Button, Confirm, Dir, Family, PadEvent, PadInfo};
use aizome_kit::prelude::*;
use aizome_sofa::keyboard::{Action, Edit, Field, Keyboard, Typed};
use aizome_sofa::pinpad::{Chorded, Chords};
use aizome_sys::tv::Auth;

use crate::app::{Greeter, Msg, PowerVerb, Stage};
use crate::login::Step;

/// What the controller is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Spot {
    /// The buttons at the top right: the controllers' panel, and power.
    Controllers,
    Power,
    #[default]
    People,
    /// The password (or user name), or the TV account's Start.
    Field,
    Session,
    /// "Log in as someone else".
    Others,
}

/// The controllers, and what they've brought up.
#[derive(Debug, Default)]
pub struct Couch {
    pub pads: Vec<PadInfo>,
    /// A controller was what was used last: its rings and hints show.
    pub active: bool,
    /// The controller pressed last.
    pub in_hand: Option<u32>,
    pub spot: Spot,
    /// The keyboard on the screen, while it's up.
    pub keys: Option<Keyboard>,
    /// The PIN pad, for the TV account's PIN, while it's up.
    pub pin: bool,
    pub chords: Chords,
    /// The panel about controllers.
    pub help: bool,
    /// The power menu, and its lit row.
    pub power: Option<usize>,
    /// The press that woke the controller was an arrow, whose direction
    /// comes next: that's the first touch too.
    swallow_nav: bool,
}

/// What's in the power menu.
pub const POWER: [PowerVerb; 3] = [PowerVerb::Sleep, PowerVerb::Restart, PowerVerb::ShutDown];

impl Couch {
    fn pad(&self) -> Option<&PadInfo> {
        self.in_hand.and_then(|id| self.pads.iter().find(|p| p.id == id)).or(self.pads.first())
    }

    /// Whose buttons to draw: the controller in hand's (Xbox's letters
    /// before there's one, as most controllers print them).
    pub fn family(&self) -> Family {
        self.pad().map_or(Family::Xbox, |p| p.family)
    }

    /// The buttons the controller in hand has.
    pub fn have(&self) -> u32 {
        self.pad().map_or(Button::all_bits(), |p| p.buttons)
    }

    pub fn ok(&self) -> Button {
        Confirm::Auto.ok(self.family())
    }

    pub fn back(&self) -> Button {
        Confirm::Auto.back(self.family())
    }

    /// Something the controller brought up is showing.
    pub fn showing(&self) -> bool {
        self.active || self.help
    }
}

impl Greeter {
    /// The TV account's way in, if it's the one logging in.
    pub fn tv_auth(&self) -> Option<Auth> {
        match self.stage {
            Stage::Name => None,
            _ => self.user().and_then(|u| u.tv.as_ref()).map(|t| t.auth),
        }
    }

    /// The TV account with nothing to sign in with, not yet started:
    /// Start, rather than a field.
    pub fn tv_start(&self) -> bool {
        self.stage == Stage::Password && self.tv_auth() == Some(Auth::None) && self.login.step == Step::Nobody
    }

    /// What the controllers did.
    pub fn pad(&mut self, e: PadEvent, cx: &mut Cx<Msg>) {
        match e {
            PadEvent::Connected(info) => {
                self.couch.pads.retain(|p| p.id != info.id);
                self.couch.pads.push(info);
                self.couch.pads.sort_by_key(|p| p.slot);
            }
            PadEvent::Disconnected { id, .. } => {
                self.couch.pads.retain(|p| p.id != id);
                self.couch.chords.forget(id);
                if self.couch.in_hand == Some(id) {
                    self.couch.in_hand = None;
                }
            }
            PadEvent::Battery { id, battery } => {
                if let Some(p) = self.couch.pads.iter_mut().find(|p| p.id == id) {
                    p.battery = battery;
                }
            }
            PadEvent::Raw { .. } => {}
            PadEvent::Nav { id, dir, .. } => {
                self.couch.in_hand = Some(id);
                if std::mem::take(&mut self.couch.swallow_nav) || self.wake(cx) {
                    return;
                }
                // The PIN's arrows are the direction pad's presses, once
                // each (its buttons say so), not the stick's.
                if self.couch.pin {
                    return;
                }
                self.nav(dir, cx);
            }
            PadEvent::Button { id, button, pressed, .. } => {
                self.couch.in_hand = Some(id);
                if pressed {
                    self.couch.swallow_nav = false;
                }
                let arrow = matches!(button, Button::Up | Button::Down | Button::Left | Button::Right);
                if self.couch.pin {
                    let have = self.couch.have();
                    let chorded = self.couch.chords.button(id, have, button, pressed);
                    if pressed && self.wake(cx) {
                        self.couch.swallow_nav = arrow;
                        return;
                    }
                    return match chorded {
                        Chorded::Digit(b) => self.pin_press(b, cx),
                        Chorded::Other(b) => self.button(b, cx),
                        Chorded::Nothing => {}
                    };
                }
                if !pressed {
                    return;
                }
                if self.wake(cx) {
                    self.couch.swallow_nav = arrow;
                    return;
                }
                self.button(button, cx);
            }
        }
    }

    /// A controller's first touch shows what it does, and does nothing
    /// else: true if this was it.
    fn wake(&mut self, cx: &mut Cx<Msg>) -> bool {
        if self.couch.active {
            return false;
        }
        log::info!("greeter: a controller's first press");
        cx.hide_cursor_until_moved();
        self.couch.active = true;
        self.couch.spot = self.main_spot();
        self.bring_up_typing(cx);
        true
    }

    /// The keyboard and the mouse are back: the controller's things go.
    pub fn leave_couch(&mut self) {
        if self.couch.active {
            log::info!("greeter: the keyboard or the mouse again");
        }
        self.couch.active = false;
        self.couch.keys = None;
        self.couch.pin = false;
        self.couch.power = None;
    }

    /// Where the controller starts on this stage.
    pub fn main_spot(&self) -> Spot {
        match self.stage {
            Stage::People => Spot::People,
            Stage::Password | Stage::Name => Spot::Field,
        }
    }

    /// On a stage with a field, the keyboard (or the PIN pad) comes up by
    /// itself.
    pub fn bring_up_typing(&mut self, _cx: &mut Cx<Msg>) {
        if !self.couch.active || self.couch.spot != Spot::Field {
            return;
        }
        match (self.stage, self.tv_auth()) {
            (Stage::People, _) => {}
            (Stage::Password, Some(Auth::None)) => {}
            (Stage::Password, Some(Auth::Pin)) => self.couch.pin = true,
            _ => {
                if self.couch.keys.is_none() {
                    self.couch.keys = Some(Keyboard { fixed_caret: true, ..Keyboard::new(None) });
                }
            }
        }
    }

    /// The keyboard as it shows: its box what's typed (dots for a
    /// password), its hints in the controller in hand's buttons.
    pub fn keyboard_view(&self) -> Option<Keyboard> {
        let k = self.couch.keys.as_ref()?;
        // A user name, a password (dots), or another question's answer.
        let (label, text, secret) = match (&self.stage, &self.login.step) {
            (Stage::Name, _) if self.login.user.is_none() || self.login.step == Step::Nobody => {
                ("User name".to_owned(), self.name.clone(), false)
            }
            (_, Step::Ask { secret: true, prompt }) => (self.field_label(prompt), String::new(), true),
            (_, Step::Ask { secret: false, prompt }) => (self.field_label(prompt), self.password.clone(), false),
            _ => (self.field_label("Password"), String::new(), true),
        };
        let typed = if secret { self.password.chars().count() } else { text.chars().count() };
        let family = self.couch.family();
        Some(Keyboard {
            field: Some(Field { label, text, length: typed, caret: typed, secret }),
            family,
            ok: self.couch.ok(),
            back: self.couch.back(),
            fixed_caret: true,
            ..k.clone()
        })
    }

    /// "Alice's password", as the keyboard labels its box.
    fn field_label(&self, prompt: &str) -> String {
        match self.user() {
            Some(u) => format!("{}: {}", u.real_name, prompt.to_lowercase()),
            None => prompt.to_owned(),
        }
    }

    fn nav(&mut self, dir: Dir, _cx: &mut Cx<Msg>) {
        if let Some(i) = self.couch.power.as_mut() {
            match dir {
                Dir::Up => *i = i.saturating_sub(1),
                Dir::Down => *i = (*i + 1).min(POWER.len() - 1),
                _ => {}
            }
            return;
        }
        if self.couch.help {
            return;
        }
        if let Some(k) = self.couch.keys.as_mut() {
            return k.step(dir);
        }
        let several = self.sessions.len() > 1 && self.tv_auth().is_none();
        let others = self.cfg.user_list && self.users.len() > 1 && self.stage == Stage::Password;
        let power = self.cfg.power;
        let main = self.main_spot();
        self.couch.spot = match (self.couch.spot, dir) {
            (Spot::People, Dir::Left) => {
                self.previous_person();
                Spot::People
            }
            (Spot::People, Dir::Right) => {
                self.next_person();
                Spot::People
            }
            (Spot::Controllers, Dir::Right) if power => Spot::Power,
            (Spot::Power, Dir::Left) => Spot::Controllers,
            (Spot::Controllers | Spot::Power, Dir::Down) => main,
            (Spot::People | Spot::Field, Dir::Up) => Spot::Controllers,
            (Spot::Field, Dir::Down) if several => Spot::Session,
            (Spot::Field, Dir::Down) if others => Spot::Others,
            (Spot::Session, Dir::Left) => {
                self.step_session(false);
                Spot::Session
            }
            (Spot::Session, Dir::Right) => {
                self.step_session(true);
                Spot::Session
            }
            (Spot::Session, Dir::Up) => Spot::Field,
            (Spot::Session, Dir::Down) if others => Spot::Others,
            (Spot::Others, Dir::Up) => {
                if several {
                    Spot::Session
                } else {
                    Spot::Field
                }
            }
            (spot, _) => spot,
        };
    }

    fn button(&mut self, b: Button, cx: &mut Cx<Msg>) {
        let (ok, back) = (self.couch.ok(), self.couch.back());
        // Home, or Select and Start held: the panel about controllers.
        if b == Button::Guide {
            self.couch.power = None;
            self.couch.help = !self.couch.help;
            return;
        }
        if let Some(i) = self.couch.power {
            if b == ok {
                self.couch.power = None;
                self.power(POWER[i], cx);
            } else if b == back {
                self.couch.power = None;
            }
            return;
        }
        if self.couch.help {
            if b == ok || b == back || b == Button::Start {
                self.couch.help = false;
            }
            return;
        }
        if self.couch.pin {
            if b == ok {
                self.submit(cx);
            } else if b == back {
                if self.password.is_empty() {
                    self.couch.pin = false;
                    self.go_back(cx);
                } else {
                    self.erase();
                }
            }
            return;
        }
        if let Some(k) = self.couch.keys.as_mut() {
            k.ok = ok;
            k.back = back;
            match k.action(b) {
                Some(Action::Key(key)) => {
                    if let Some(typed) = k.press(key) {
                        self.typed(typed, cx);
                    }
                }
                Some(Action::Edit(e)) => self.typed(Typed::Edit(e), cx),
                Some(Action::Close) => self.couch.keys = None,
                None => {}
            }
            return;
        }
        match self.couch.spot {
            Spot::Controllers if b == ok => self.couch.help = true,
            Spot::Power if b == ok => self.couch.power = Some(0),
            Spot::People if b == ok || b == Button::Start => self.pick(self.selected, cx),
            Spot::Field if self.tv_start() && (b == ok || b == Button::Start) => self.start_tv(cx),
            Spot::Field if b == ok => self.bring_up_typing(cx),
            Spot::Field if b == Button::Start => self.submit_typed(cx),
            Spot::Session if b == ok => self.step_session(true),
            Spot::Others if b == ok => self.back_to_people(cx),
            _ if b == back => self.go_back(cx),
            _ => {}
        }
    }

    /// No, with nothing up: from the top's buttons back down, from a
    /// password back to the people.
    fn go_back(&mut self, cx: &mut Cx<Msg>) {
        match self.couch.spot {
            Spot::Controllers | Spot::Power => self.couch.spot = self.main_spot(),
            _ if self.stage == Stage::Password && self.cfg.user_list && self.users.len() > 1 => {
                self.back_to_people(cx);
            }
            _ => {}
        }
    }

    /// What the keyboard typed, into the field.
    fn typed(&mut self, typed: Typed, cx: &mut Cx<Msg>) {
        let name = self.stage == Stage::Name && (self.login.user.is_none() || self.login.step == Step::Nobody);
        match typed {
            Typed::Text(s) if name => self.name.push_str(&s),
            Typed::Text(s) => self.password.push_str(&s),
            Typed::Edit(Edit::Backspace) if name => {
                self.name.pop();
            }
            Typed::Edit(Edit::Backspace) => self.erase(),
            Typed::Edit(Edit::Enter) => self.submit_typed(cx),
            // A password's caret stays at its end.
            Typed::Edit(Edit::Left | Edit::Right) => {}
        }
    }

    fn erase(&mut self) {
        self.password.pop();
    }

    /// Enter: the user name, or the password.
    fn submit_typed(&mut self, cx: &mut Cx<Msg>) {
        if self.stage == Stage::Name && (self.login.user.is_none() || self.login.step == Step::Nobody) {
            self.submit_name(cx);
        } else {
            self.submit(cx);
        }
    }

    /// One of the PIN's digits pressed (by its legend button): the PIN
    /// goes as soon as it's as long as the one set.
    fn pin_press(&mut self, b: Button, cx: &mut Cx<Msg>) {
        let Some(d) = aizome_sofa::pinpad::digit(b) else { return };
        if self.password.len() >= 8 {
            return;
        }
        self.password.push(d);
        let length = self.user().and_then(|u| u.tv.as_ref()).and_then(|t| t.pin_length).map(usize::from);
        if length == Some(self.password.len()) {
            self.submit(cx);
        }
    }

    /// The next (or previous) session for whoever's logging in.
    fn step_session(&mut self, forward: bool) {
        let n = self.sessions.len();
        let (Some(user), Some(cur)) = (self.user().map(|u| u.name.clone()), self.session().map(|s| s.id.clone()))
        else {
            return;
        };
        if n < 2 {
            return;
        }
        let i = self.sessions.iter().position(|s| s.id == cur).unwrap_or(0);
        let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
        self.session_of.insert(user, self.sessions[j].id.clone());
    }

    /// A click on what the controller brought up: a key, the PIN pad's,
    /// a hint, the panel's Close or a power row; beside them all, it goes.
    pub fn sofa_click(&mut self, t: crate::sofa::Target, cx: &mut Cx<Msg>) {
        use crate::sofa::Target;
        use aizome_sofa::pinpad::Key;
        match t {
            Target::Key(n) => {
                let Some(k) = self.couch.keys.as_mut() else { return };
                k.go_to(n);
                let key = k.key();
                if let Some(typed) = k.press(key) {
                    self.typed(typed, cx);
                }
            }
            Target::Pin(i) => match aizome_sofa::pinpad::key_at(i) {
                Key::Press(b) => self.pin_press(b, cx),
                Key::Delete => self.erase(),
                Key::Done => self.submit(cx),
                Key::Forgot => {}
            },
            Target::Hint(b) => self.button(b, cx),
            Target::Close => self.couch.help = false,
            Target::Power(i) => {
                self.couch.power = None;
                self.power(POWER[i], cx);
            }
            Target::Panel => {}
            Target::Outside if self.couch.help => self.couch.help = false,
            Target::Outside if self.couch.power.is_some() => self.couch.power = None,
            Target::Outside => self.leave_couch(),
        }
    }

    /// What's drawn over the login screen for the controller (on the
    /// display with the login).
    pub fn sofa(&self) -> Option<crate::sofa::Scene> {
        if !self.couch.showing() {
            return None;
        }
        let family = self.couch.family();
        let length = self.user().and_then(|u| u.tv.as_ref()).and_then(|t| t.pin_length).map(usize::from);
        let pin = (self.couch.active && self.couch.pin).then(|| aizome_sofa::pinpad::Pad {
            title: self.user().map(|u| u.real_name.clone()).unwrap_or_default(),
            prompt: match length {
                Some(_) => "Press the PIN's buttons".into(),
                None => "Press the PIN's buttons, then Done".into(),
            },
            typed: self.password.chars().count(),
            length,
            error: self.login.message.as_ref().map(|(m, _)| m.replace("password", "PIN")),
            wait: None,
            checking: self.login.step == Step::Waiting,
            family,
            have: self.couch.have(),
            ok: self.couch.ok(),
            back: self.couch.back(),
            done: length.is_none(),
            forgot: false,
        });
        let help = self.couch.help.then(|| crate::sofa::Help {
            family,
            ok: self.couch.ok(),
            back: self.couch.back(),
            guide: self.couch.have() & Button::Guide.bit() != 0,
            pads: self.couch.pads.iter().map(|p| (p.name.clone(), p.battery)).collect(),
            tv: self.users.iter().any(|u| u.tv.is_some()),
        });
        let mut hints = if self.couch.active { self.hints() } else { Vec::new() };
        // The panel has its own Close; the power menu says what's what.
        if self.couch.help {
            hints = Vec::new();
        } else if self.couch.power.is_some() {
            hints = vec![(self.couch.ok(), "Choose".into()), (self.couch.back(), "Close".into())];
        }
        Some(crate::sofa::Scene {
            hints,
            family,
            keys: if self.couch.active { self.keyboard_view() } else { None },
            pin,
            help,
            power: self.couch.power,
        })
    }

    /// The hints along the foot for where the controller is.
    pub fn hints(&self) -> Vec<(Button, String)> {
        let (ok, back) = (self.couch.ok(), self.couch.back());
        let guide = self.couch.have() & Button::Guide.bit() != 0;
        let others = self.cfg.user_list && self.users.len() > 1;
        let mut h: Vec<(Button, String)> = match self.couch.spot {
            _ if self.couch.keys.is_some() || self.couch.pin || self.couch.help || self.couch.power.is_some() => {
                Vec::new()
            }
            Spot::People => vec![(Button::Right, "Choose".into()), (ok, "Log in".into())],
            Spot::Field if self.tv_start() => vec![(ok, "Start".into())],
            Spot::Field => vec![(ok, "Keyboard".into()), (Button::Start, "Log in".into())],
            Spot::Session => vec![(Button::Right, "Session".into())],
            Spot::Others => vec![(ok, "Someone else".into())],
            Spot::Controllers => vec![(ok, "About controllers".into())],
            Spot::Power => vec![(ok, "Power".into())],
        };
        if !h.is_empty() {
            if self.stage == Stage::Password && others {
                h.push((back, "Back".into()));
            }
            if guide {
                h.push((Button::Guide, "Help".into()));
            }
        }
        h
    }
}
