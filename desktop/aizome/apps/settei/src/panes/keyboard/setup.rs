//! Setting up a keyboard: the key pressed beside the space bar says which
//! keyboard it is and which modifier arrangement puts ❖ there
//! ([`aizome_ipc::setup`], which Genkan's keyboard step shares); the
//! sample has a keyboard of its own. The keyboard's other keys (Caps Lock,
//! Compose) start as other keyboards' or as another keyboard's.

use std::sync::atomic::{AtomicU64, Ordering};

use aizome_config::{Settings, key_options};
use aizome_ipc::{KeyboardBus, KeyboardInfo, Request, Response};

pub use aizome_ipc::setup::{Beside, beside_space};

/// The arrangement (in [`super::modifiers::CHOICES`]) that puts ❖ beside
/// the space bar, if one does.
pub fn suggested(b: &Beside) -> Option<usize> {
    b.suggest.map(super::modifiers::selected)
}

/// The sample computer's keyboard, which the sample's key comes from.
pub fn sample_keyboard() -> KeyboardInfo {
    KeyboardInfo {
        name: "Apple Inc. Magic Keyboard".into(),
        id: "05ac:029c".into(),
        bus: KeyboardBus::Bluetooth,
        last: true,
    }
}

/// The sample computer's keyboards: its Magic Keyboard, and a mouse's
/// receiver that says it's a keyboard too.
pub fn sample_keyboards() -> Vec<KeyboardInfo> {
    let receiver = KeyboardInfo {
        name: "Logitech USB Receiver".into(),
        id: "046d:c548".into(),
        bus: KeyboardBus::Usb,
        last: false,
    };
    vec![sample_keyboard(), receiver]
}

/// The keyboards attached, as the compositor lists them (none when it
/// doesn't answer).
pub fn attached() -> Vec<KeyboardInfo> {
    match aizome_ipc::Client::connect().and_then(|mut c| c.request(&Request::Keyboards)) {
        Ok(Response::Keyboards { keyboards }) => keyboards,
        _ => Vec::new(),
    }
}

/// Those of `attached` plugged in or paired without an entry of their own,
/// available in the optional setup sheet without a key pressed (a security
/// key or a mouse's receiver has no space bar to press beside).
pub fn not_set_up(s: &Settings, attached: &[KeyboardInfo]) -> Vec<KeyboardInfo> {
    attached.iter().filter(|k| k.bus.plugged() && s.input.keyboard_profile(&k.name, &k.id).is_none()).cloned().collect()
}

/// Where a keyboard's other keys can start from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub label: String,
    /// The keyboard (name and ID), or none for other keyboards'.
    pub who: Option<(String, String)>,
    /// Its keys' options.
    pub options: String,
}

/// Other keyboards' settings, then each keyboard's own.
pub fn sources(s: &Settings) -> Vec<Source> {
    let mut v =
        vec![Source { label: "Other keyboards".into(), who: None, options: key_options(&s.input.keyboard.options) }];
    for k in &s.input.keyboards {
        if let Some(own) = &k.options {
            v.push(Source {
                label: k.name.clone(),
                who: Some((k.name.clone(), k.id.clone())),
                options: key_options(own),
            });
        }
    }
    v
}

/// A keyboard set up, waiting for its key, or its key caught.
#[derive(Clone, Debug, Default)]
pub struct Setup {
    pub waiting: bool,
    /// The key pressed and the keyboard it came from.
    pub keyboard: Option<KeyboardInfo>,
    pub beside: Option<Beside>,
    pub error: Option<String>,
    /// The arrangement chosen (the suggestion at first) and where the other
    /// keys come from (in [`sources`]).
    pub modifiers: usize,
    pub from: usize,
    /// Keyboards not set up ([`not_set_up`]), and the one chosen of them
    /// while no key has come.
    pub unset: Vec<KeyboardInfo>,
    pub chosen: usize,
    /// The keyboard the sheet was opened for (`--keyboard`), chosen once
    /// it's among `unset`.
    pub asked: Option<String>,
}

impl Setup {
    /// The keyboards not set up, `unset`: the one asked about chosen, if
    /// it's one of them, or the first.
    pub fn set_unset(&mut self, unset: Vec<KeyboardInfo>) {
        let asked = self.asked.as_deref();
        self.chosen = asked.and_then(|a| unset.iter().position(|k| asks_for(a, k))).unwrap_or(0);
        self.unset = unset;
    }
}

/// Whether `asked` (`--keyboard`) is keyboard `k`: “Name (vendor:product)”,
/// or its vendor:product or name alone.
pub fn asks_for(asked: &str, k: &KeyboardInfo) -> bool {
    let both = asked.strip_suffix(')').and_then(|a| a.rsplit_once(" ("));
    both.is_some_and(|(name, id)| k.name == name && k.id.eq_ignore_ascii_case(id))
        || k.id.eq_ignore_ascii_case(asked)
        || k.name == asked
}

/// The tag of wait `n`.
fn tag(n: u64) -> String {
    format!("settei-{}-{n}", std::process::id())
}

/// Wait `n` for the next key, on a thread ([`aizome_ipc::setup::catch_key`]).
/// `None` when given up on: `waits` moved past `n` (another wait began, or
/// the sheet closed).
pub fn catch_key(n: u64, waits: &AtomicU64) -> Result<Option<(u32, Option<KeyboardInfo>)>, String> {
    aizome_ipc::setup::catch_key(&tag(n), || waits.load(Ordering::SeqCst) != n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_beside_the_space_bar_says_where_super_goes() {
        let suggest = |code| suggested(&beside_space(code).unwrap());
        assert_eq!(suggest(125), Some(0), "a Mac's Command: nothing to swap");
        assert_eq!(suggest(56), Some(2), "a PC's Alt: swap the left keys, preserving AltGr");
        assert_eq!(suggest(29), Some(4));
        assert_eq!(suggest(94), None, "無変換 can't be Super");
        assert!(beside_space(57).is_err());
        assert!(beside_space(100).is_err());
        assert_eq!(super::super::modifiers::CHOICES[2].0, "Swap left Alt and Super");
        assert_eq!(super::super::modifiers::CHOICES[4].0, "Swap Control and Super");
    }

    #[test]
    fn other_keys_start_from_other_keyboards_or_a_keyboard_of_its_own() {
        let s = Settings::from_toml(
            "[input.keyboard]\noptions = \"grp:alt_shift_toggle,ctrl:nocaps\"\n\
             [[input.keyboards]]\nname = \"PC\"\nid = \"046d:c31c\"\noptions = \"altwin:swap_alt_win,compose:ralt\"\n\
             [[input.keyboards]]\nname = \"Same as the others\"\nid = \"1234:5678\"\n",
        )
        .unwrap();
        let v = sources(&s);
        let labels: Vec<_> = v.iter().map(|s| (s.label.as_str(), s.options.as_str())).collect();
        assert_eq!(labels, [("Other keyboards", "ctrl:nocaps"), ("PC", "altwin:swap_alt_win,compose:ralt")]);

        // Those not set up: plugged in or paired, without an entry.
        let mut attached = sample_keyboards();
        attached.push(KeyboardInfo { name: "PC".into(), id: "046D:C31C".into(), bus: KeyboardBus::Usb, last: false });
        attached.push(KeyboardInfo {
            name: "PC Keyboard".into(),
            id: "046d:c31c".into(),
            bus: KeyboardBus::Usb,
            last: false,
        });
        let builtin =
            KeyboardInfo { name: "Built in".into(), id: "0001:0001".into(), bus: KeyboardBus::BuiltIn, last: false };
        attached.push(builtin);
        let names: Vec<_> = not_set_up(&s, &attached).into_iter().map(|k| k.name).collect();
        assert_eq!(names, ["Apple Inc. Magic Keyboard", "Logitech USB Receiver"]);
    }

    #[test]
    fn the_keyboard_asked_about_is_chosen() {
        let mut st = Setup::default();
        st.set_unset(sample_keyboards());
        assert_eq!(st.chosen, 0, "none asked about: the first");
        for asked in ["Logitech USB Receiver (046D:C548)", "046d:c548", "Logitech USB Receiver"] {
            st.asked = Some(asked.into());
            st.set_unset(sample_keyboards());
            assert_eq!(st.unset[st.chosen].name, "Logitech USB Receiver", "{asked}");
        }
        // Another of that name, or one set up since: the first.
        st.asked = Some("Logitech USB Receiver (046d:c52b)".into());
        st.set_unset(sample_keyboards());
        assert_eq!(st.chosen, 0);
    }
}
