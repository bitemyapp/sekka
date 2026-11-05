//! A password field above a plain one, as the lock screen styles its
//! field, for the field's composition and text-input tests.

#![allow(dead_code)]

use std::time::Duration;

use aizome_face::look;
use aizome_face::widgets::field::field;
use aizome_kit::Layoutable;
use aizome_kit::prelude::{AppInfo, Cx, Element, Program, WindowKey, WindowSpec};
use aizome_kit::testing::Harness;
use aizome_theme::typeface;

/// The caret's blink: the field's next event, when nothing else happens.
pub const BLINK: Duration = Duration::from_millis(530);

pub struct Probe {
    pub password: String,
    pub other: String,
    pub secure: bool,
}

#[derive(Clone)]
pub enum M {
    Password(String),
    Other(String),
    /// The program sets the password field's text (the lock screen clears
    /// it after a wrong password).
    Set(String),
    /// The prompt changes whether it echoes.
    Secure(bool),
}

impl Program for Probe {
    type Msg = M;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.FieldProbe", "Field", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<M>) {
        cx.open_window(WindowSpec::new("Password").size(240.0, 80.0));
    }
    fn update(&mut self, m: M, _: &mut Cx<M>) {
        match m {
            M::Password(v) | M::Set(v) => self.password = v,
            M::Other(v) => self.other = v,
            M::Secure(s) => self.secure = s,
        }
    }
    fn view(&self, _: WindowKey) -> Element<M> {
        let spec = |_: &aizome_kit::Theme| {
            look::ui(15.0, typeface::WEIGHT_REGULAR, aizome_kit::theme::color(aizome_theme::NIGHT.text))
        };
        let f = field(&self.password, spec).placeholder("Password").height(32.0).on_change(M::Password);
        let f = if self.secure { Element::from(f.secure()) } else { Element::from(f) };
        let other = field(&self.other, spec).placeholder("Name").height(32.0).on_change(M::Other);
        aizome_kit::column(vec![f.id("password"), Element::from(other).id("other")]).into()
    }
}

/// A harness with the password field (holding `password`, secure or not)
/// focused.
pub fn focused(password: &str, secure: bool) -> (Harness<Probe>, WindowKey) {
    let mut h = Harness::new(Probe { password: password.to_owned(), other: String::new(), secure });
    let w = h.window();
    h.focus_window(w, true);
    click(&mut h, w, "password");
    (h, w)
}

pub fn click(h: &mut Harness<Probe>, w: WindowKey, id: &str) {
    let at = h.find_id(w, id).expect("the field").center();
    h.click(w, at);
}

/// How many pixels of two renders differ.
pub fn differ(a: &(u32, u32, Vec<u8>), b: &(u32, u32, Vec<u8>)) -> usize {
    assert_eq!((a.0, a.1), (b.0, b.1));
    a.2.chunks(4).zip(b.2.chunks(4)).filter(|(x, y)| x != y).count()
}
