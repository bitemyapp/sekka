//! A password text field above a plain one, for the text field's
//! composition and text-input tests.

#![allow(dead_code)]

use std::time::Duration;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;

/// The caret's blink: the field's next event, when nothing else happens.
pub const BLINK: Duration = Duration::from_millis(530);

pub struct Form {
    pub secret: String,
    pub name: String,
    /// The secret field shows its text ("Show password" ticked).
    pub show: bool,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Secret(String),
    Name(String),
    /// The program sets the secret field's text (a sheet clears it after
    /// a wrong password).
    Set(String),
    Show(bool),
}

impl Program for Form {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.PasswordForm", "Form", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Form").size(300.0, 120.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Secret(s) | Msg::Set(s) => self.secret = s,
            Msg::Name(s) => self.name = s,
            Msg::Show(on) => self.show = on,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        // A password field built as Settei's network sheet builds it; this
        // form builds on the field before it was a type of its own too.
        let secret = text_field(&self.secret).on_change(Msg::Secret).id("secret");
        let secret = if self.show { Element::from(secret) } else { Element::from(secret.password()) };
        column![secret, text_field(&self.name).on_change(Msg::Name).id("name")].spacing(8.0).into()
    }
}

/// A harness with the secret field (holding `secret`) focused.
pub fn focused(secret: &str, show: bool) -> (Harness<Form>, WindowKey) {
    let mut h = Harness::new(Form { secret: secret.to_owned(), name: String::new(), show });
    let w = h.window();
    h.focus_window(w, true);
    click(&mut h, w, "secret");
    (h, w)
}

pub fn click(h: &mut Harness<Form>, w: WindowKey, id: &str) {
    let at = h.find_id(w, id).expect("the field").center();
    h.click(w, at);
}

/// How many pixels of two renders differ.
pub fn differ(a: &(u32, u32, Vec<u8>), b: &(u32, u32, Vec<u8>)) -> usize {
    assert_eq!((a.0, a.1), (b.0, b.1));
    a.2.chunks(4).zip(b.2.chunks(4)).filter(|(x, y)| x != y).count()
}
