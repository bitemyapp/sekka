//! Keyboards plugged in or paired without keys of their own: a
//! notification offering Settei's setup, where the key pressed beside the
//! space bar says where ❖ should go on that keyboard. Each is asked about
//! once a session; one set up (or set to type as the others do) isn't
//! asked about again. A laptop's own keyboard and a VM's never are.

use aizome_ipc::KeyboardInfo;
use aizome_kit::prelude::*;

use crate::actions::Action;
use crate::notifications::store::Incoming;
use crate::shell::{Msg, Shell};

impl Shell {
    /// A keyboard the compositor has (at connecting) or just met.
    pub fn keyboard_met(&mut self, k: KeyboardInfo, cx: &mut Cx<Msg>) {
        if !k.bus.plugged() || self.settings.input.keyboard_profile(&k.name, &k.id).is_some() {
            return;
        }
        if !self.keyboards_asked.insert((k.name.clone(), k.id.to_ascii_lowercase())) {
            return;
        }
        let n = Incoming {
            app_name: "Settei".into(),
            summary: format!("Set up {}?", k.name),
            body: "Press the key beside its space bar, and Settei suggests where ❖ goes on it.".into(),
            ..Default::default()
        };
        let setup = Action::Settings { pane: "keyboard.setup".into(), anchor: None };
        self.shell_note(
            n,
            vec![("default".into(), "Set Up…".into(), setup.clone()), ("set-up".into(), "Set Up…".into(), setup)],
            cx,
        );
    }
}
