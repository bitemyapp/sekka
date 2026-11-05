//! SYC-07: a password field must not show what an input method is
//! composing in it.
//!
//! The lock screen (`aizome-shell/src/lock.rs:236-244`), the polkit dialog
//! and the greeter use `field(..).secure()`. In secure mode the value is
//! drawn as dots, but `paint` appends the IME preedit unmasked
//! (`field.rs:223`, `all = shown(value) + preedit`). The test renders, in
//! the kit's headless harness, the lock screen's field holding "pa" while
//! an input method composes "WW", and the same field style showing the
//! plain text "••WW". Correct: the two differ (the composed characters are
//! masked or hidden). At 8e0acbe5 they are pixel for pixel the same; a
//! password field now keeps only a count of what is composed ([`Masked`]),
//! drawn as dots.
//!
//! [`Masked`]: aizome_face::widgets::field::Masked

use aizome_face::look;
use aizome_face::widgets::field::field;
use aizome_kit::prelude::{AppInfo, Cx, Element, Point, Program, WindowKey, WindowSpec};
use aizome_kit::testing::Harness;
use aizome_theme::typeface;

struct Probe {
    value: String,
    secure: bool,
}

#[derive(Clone)]
enum M {
    Changed(String),
}

impl Program for Probe {
    type Msg = M;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.ReproSyc07", "Field", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<M>) {
        cx.open_window(WindowSpec::new("Password").size(240.0, 40.0));
    }
    fn update(&mut self, m: M, _: &mut Cx<M>) {
        let M::Changed(v) = m;
        self.value = v;
    }
    fn view(&self, _: WindowKey) -> Element<M> {
        // The lock screen's field (lock.rs:236-244), less its box.
        let f = field(&self.value, |_| {
            look::ui(15.0, typeface::WEIGHT_REGULAR, aizome_kit::theme::color(aizome_theme::NIGHT.text))
        })
        .placeholder("Password")
        .height(32.0)
        .on_change(M::Changed);
        // A password field is a type of its own now; this form builds on
        // the field before that too, so the test runs against both.
        if self.secure { Element::from(f.secure()) } else { Element::from(f) }
    }
}

/// A focused field showing `value` (secure or not), an input method
/// composing `preedit` in it; its pixels at 2×.
fn shown(value: &str, secure: bool, preedit: &str) -> (u32, u32, Vec<u8>) {
    let mut h = Harness::new(Probe { value: value.to_owned(), secure });
    let w = h.window();
    h.focus_window(w, true);
    h.click(w, Point::new(120.0, 16.0));
    h.ime(w, None, preedit);
    h.render_rgba(w, 2.0)
}

#[test]
fn a_password_field_hides_the_text_being_composed() {
    let composing = shown("pa", true, "WW");
    let in_clear = shown("••WW", false, "");
    let dots_only = shown("pa", true, "");
    assert_eq!((composing.0, composing.1), (in_clear.0, in_clear.1));
    let differ = |a: &[u8], b: &[u8]| a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
    let vs_clear = differ(&composing.2, &in_clear.2);
    let vs_dots = differ(&composing.2, &dots_only.2);
    eprintln!("pixels differing from the clear-text field: {vs_clear}; from the field with no preedit: {vs_dots}");
    assert!(vs_dots > 0, "the preedit shows somehow");
    assert!(
        vs_clear > 0,
        "the secure field composing \"WW\" draws exactly what a plain field showing \"••WW\" draws: the composed text is in clear"
    );
}
