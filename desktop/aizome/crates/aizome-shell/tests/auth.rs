//! The authentication dialog's look, headless (polkit can't be asked in a
//! test: its view is drawn alone).

mod common;

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_shell::auth::{AuthMsg, AuthView, view};
use common::*;

struct Preview {
    v: AuthView,
    said: Vec<AuthMsg>,
}

impl Program for Preview {
    type Msg = AuthMsg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Test", "Test", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<AuthMsg>) {
        let s = aizome_shell::auth::size();
        cx.open_window(WindowSpec::new("auth").size(s.width, s.height));
    }
    fn update(&mut self, m: AuthMsg, _cx: &mut Cx<AuthMsg>) {
        if let AuthMsg::Password(p) = &m {
            self.v.password = p.clone();
        }
        self.said.push(m);
    }
    fn view(&self, _w: WindowKey) -> Element<AuthMsg> {
        view(&self.v)
    }
}

fn dialog(identities: usize) -> AuthView {
    AuthView {
        message: "Authentication is required to mount sekka-nightly.iso.".into(),
        identities: ["Chris Allen (callen)", "Administrator (root)"]
            .iter()
            .take(identities)
            .map(|s| s.to_string())
            .collect(),
        identity: 0,
        prompt: "Password:".into(),
        echo: false,
        password: String::new(),
        note: Some(("Sorry, try again.".into(), true)),
        busy: false,
    }
}

#[test]
fn the_dialog_takes_a_password_and_answers() {
    let mut h = Harness::new(Preview { v: dialog(1), said: vec![] });
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t.contains("mount sekka-nightly.iso")));
    h.click_text(w, "Cancel");
    assert_eq!(h.program().said, [AuthMsg::Cancel]);
}

#[test]
fn pictures_of_the_dialog() {
    for (day, n) in [(false, 2), (true, 1)] {
        let mut h = Harness::with(Preview { v: dialog(n), said: vec![] }, appearance(day), false);
        let w = h.window();
        let name = if day { "auth-day.png" } else { "auth-night.png" };
        h.save_png(w, shots().join(name), 2.0);
        aizome_kit::testing::assert_golden(&mut h, w, golden(name), 2.0);
    }
}
