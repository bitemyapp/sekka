//! Keyboards plugged in or paired without keys of their own: a
//! notification offering Settei's setup, where the key pressed beside the
//! space bar says where ❖ should go on that keyboard, or to have it type as
//! the others do (a security key or a mouse's receiver that says it's a
//! keyboard has no space bar to press beside). Each is asked about once a
//! session; one set up (or set to type as the others do) isn't asked about
//! again. A laptop's own keyboard and a VM's never are.

use aizome_config::{KeyboardProfile, Settings};
use aizome_ipc::{KeyboardInfo, Request};
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
        // Settei's sheet opens with this keyboard chosen, should no key
        // come from it.
        let setup = Action::SetUpKeyboard { name: k.name.clone(), id: k.id.clone() };
        let same = Action::KeyboardAsOthers { name: k.name.clone(), id: k.id.clone() };
        self.shell_note(
            n,
            vec![
                ("default".into(), "Set Up…".into(), setup.clone()),
                ("set-up".into(), "Set Up…".into(), setup),
                // Settei's “Same as Other Keyboards”, short enough to fit
                // beside Set Up… in the Deskbar.
                ("same".into(), "Same as Others".into(), same),
            ],
            cx,
        );
    }

    /// A keyboard types as other keyboards do: an entry with no keys of
    /// its own, as Settei's “Same as Other Keyboards” writes, so it isn't
    /// asked about again.
    pub fn keyboard_as_others(&mut self, name: &str, id: &str) {
        // Read again, so nobody else's edit is lost; a file that doesn't
        // read is left as it is.
        let mut s = if self.opts.live {
            match Settings::load() {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("{name} can't be saved as typing as other keyboards do: {e}");
                    return;
                }
            }
        } else {
            self.settings.clone()
        };
        if s.input.keyboard_profile(name, id).is_none() {
            s.input.keyboards.push(KeyboardProfile { name: name.into(), id: id.into(), ..Default::default() });
        }
        if self.opts.live {
            if let Err(e) = s.save() {
                log::warn!("saving {name} as typing as other keyboards do: {e}");
                return;
            }
            self.ipc.send(Request::ReloadConfig);
        }
        self.settings = s;
    }
}
