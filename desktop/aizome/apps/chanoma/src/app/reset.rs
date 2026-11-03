//! A forgotten PIN: *Forgot it?* on the PIN pad leads here, to sign in
//! with the password of the computer's account Chanoma runs as, checked
//! through PAM (`aizome_sys::pam`, the lock screen's service); then a new
//! PIN is chosen, in grown-up mode. Kids know the PIN's buttons, perhaps,
//! but not the account's password.
//!
//! It's the account Chanoma runs as because PAM, run without privileges,
//! checks only that one's password. Every Sekka account is a full one
//! today; once there are kids' accounts, one of those mustn't be able to
//! reset the PIN this way, and this is where to say so.
//!
//! The password is typed on Chanoma's keyboard (its box shows dots) or a
//! real one. Wrong ones count with wrong PINs: three free, then waits.

use aizome_kit::prelude::*;

use super::{AfterPin, Chanoma, Input, Modal, Msg, PinState, Screen};
use crate::devtools::Field;
use crate::pad::Family;
use crate::ui::keyboard::{Key, Keyboard, Typed};
use crate::ui::signin::SignIn;

/// Signing in for a new PIN.
#[derive(Clone, Debug, PartialEq)]
pub struct Reset {
    pub password: String,
    pub keys: Keyboard,
    /// The account: its login name and its name.
    pub login: String,
    pub name: String,
    pub checking: bool,
    pub error: Option<String>,
}

impl Chanoma {
    /// *Forgot it?* on the PIN pad.
    pub fn forgot_pin(&mut self, cx: &mut Cx<Msg>) {
        let (login, name) = self.config.account.clone();
        log::info!("chanoma: the PIN forgotten; signing in as {login}");
        self.modal = None;
        self.reset = Some(Reset {
            password: String::new(),
            keys: Keyboard::new(None),
            login,
            name,
            checking: false,
            error: None,
        });
        self.go(Screen::Reset, cx);
        self.sync_hud(cx);
    }

    pub fn reset_view(&self) -> SignIn {
        let Some(r) = &self.reset else {
            return SignIn { title: String::new(), lines: Vec::new(), error: None, checking: false };
        };
        let who = if r.name == r.login { r.login.clone() } else { format!("{} ({})", r.name, r.login) };
        let wait = self.lockout.wait(self.now_secs());
        SignIn {
            title: "Forgot the PIN?".into(),
            lines: vec![
                format!("Sign in with the password of this computer's account, {who},"),
                "then choose a new PIN.".into(),
            ],
            error: match wait {
                Some(s) => Some(format!("Too many tries. Wait {}:{:02}", s / 60, s % 60)),
                None => r.error.clone(),
            },
            checking: r.checking,
        }
    }

    /// The keyboard under it, its box dots.
    pub fn reset_keyboard(&self) -> Option<Keyboard> {
        let r = self.reset.as_ref()?;
        let family = match self.family {
            Family::Keyboard => self.controllers.first().map_or(Family::Generic, |p| p.family),
            f => f,
        };
        let n = r.password.chars().count();
        Some(Keyboard {
            family,
            ok: self.ok_button(family),
            back: self.back_button(family),
            field: Some(Field {
                label: format!("{}'s password", r.login),
                text: String::new(),
                length: n,
                caret: n,
                secret: true,
            }),
            ..r.keys.clone()
        })
    }

    pub fn reset_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let keyboard = self.family == Family::Keyboard;
        let Some(r) = &mut self.reset else { return };
        if r.checking {
            return;
        }
        match i {
            Input::Dir(d) => r.keys.step(d),
            // On a keyboard, Enter signs in.
            Input::Ok if keyboard => self.sign_in(cx),
            Input::Ok => {
                let key = r.keys.key();
                self.reset_key(key, cx);
            }
            Input::Typed(c) => self.reset_type(&c.to_string()),
            Input::Digit(d) => self.reset_type(&d.to_string()),
            Input::Space | Input::Alt => self.reset_type(" "),
            Input::Erase => {
                r.password.pop();
                r.error = None;
            }
            Input::Menu => self.sign_in(cx),
            Input::Back => self.leave_reset(cx),
            _ => {}
        }
    }

    /// Key `n` of the keyboard clicked ([`Keyboard::hit`]).
    pub fn reset_click_key(&mut self, n: usize, cx: &mut Cx<Msg>) {
        let Some(r) = &mut self.reset else { return };
        r.keys.go_to(n);
        let key = r.keys.key();
        self.reset_key(key, cx);
    }

    fn reset_key(&mut self, key: Key, cx: &mut Cx<Msg>) {
        let Some(r) = &mut self.reset else { return };
        match r.keys.press(key) {
            Some(Typed::Text(t)) => self.reset_type(&t),
            Some(Typed::Key(crate::devtools::Key::Backspace)) => {
                r.password.pop();
            }
            Some(Typed::Key(crate::devtools::Key::Enter)) => self.sign_in(cx),
            _ => {}
        }
    }

    fn reset_type(&mut self, t: &str) {
        if let Some(r) = &mut self.reset {
            r.password.push_str(t);
            r.error = None;
        }
    }

    /// Check the password, off this thread (PAM can take seconds, more
    /// after a wrong one).
    fn sign_in(&mut self, cx: &mut Cx<Msg>) {
        let wait = self.lockout.wait(self.now_secs());
        let Some(r) = &mut self.reset else { return };
        if r.password.is_empty() || wait.is_some() {
            return;
        }
        r.checking = true;
        r.error = None;
        let (login, password) = (r.login.clone(), std::mem::take(&mut r.password));
        let auth = self.config.authenticator.clone();
        cx.spawn(move || Msg::SignedIn(auth.authenticate(&login, &password).is_ok()));
    }

    /// The password checked: a new PIN to choose, or another try.
    pub fn signed_in(&mut self, ok: bool, cx: &mut Cx<Msg>) {
        let now = self.now_secs();
        let Some(r) = &mut self.reset else { return };
        r.checking = false;
        if !ok {
            r.error = Some("That's not the password".into());
            self.lockout.failed(now);
            self.save_lockout();
            log::info!("chanoma: signing in for a new PIN: not the password");
            return;
        }
        log::info!("chanoma: signed in for a new PIN");
        self.lockout.succeeded();
        self.save_lockout();
        self.reset = None;
        self.unlock(cx);
        self.go(Screen::Home, cx);
        self.modal = Some(Modal::Pin { pin: PinState { new: true, ..PinState::default() }, after: AfterPin::Unlock });
        self.modal_focus = 0;
        self.sync_hud(cx);
    }

    /// Back to where the PIN was asked for: Home.
    pub fn leave_reset(&mut self, cx: &mut Cx<Msg>) {
        self.reset = None;
        self.go(Screen::Home, cx);
    }
}
