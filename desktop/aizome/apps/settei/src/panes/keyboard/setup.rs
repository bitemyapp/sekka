//! Setting up a keyboard: the key pressed beside the space bar says which
//! keyboard it is and which modifier arrangement puts ❖ there. The
//! compositor catches the key, with the keyboard it came from, since no app
//! hears a modifier key alone; the sample has a keyboard of its own. The
//! keyboard's other keys (Caps Lock, Compose) start as other keyboards'
//! or as another keyboard's.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aizome_config::{Settings, key_options};
use aizome_ipc::{Event, KeyboardBus, KeyboardInfo, Request, Response};

/// What the key beside the space bar is, and what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beside {
    /// For “Beside the space bar: …”.
    pub what: &'static str,
    /// The arrangement (in [`super::modifiers::CHOICES`]) that puts ❖
    /// there, if one does.
    pub suggest: Option<usize>,
    pub why: &'static str,
}

/// The key beside the space bar, by its evdev code; an error says which key
/// to press instead.
pub fn beside_space(code: u32) -> Result<Beside, &'static str> {
    Ok(match code {
        125 => Beside {
            what: "❖ Super (⌘ Command, or the Windows key)",
            suggest: Some(0),
            why: "❖ is there already, as on a Mac: the modifier keys can stay as they are.",
        },
        56 => Beside {
            what: "⌥ Alt (Option)",
            suggest: Some(1),
            why: "Swap Alt and Super to put ❖ there, under your thumb, where ⌘ is on a Mac.",
        },
        29 => Beside {
            what: "⌃ Control",
            suggest: Some(4),
            why: "Swap Control and Super to put ❖ there, under your thumb.",
        },
        57 => return Err("That's the space bar: press the key just to its left."),
        97 | 100 | 126 => return Err("That's right of the space bar: press the key just to its left."),
        _ => Beside {
            what: "a key that can't be ❖",
            suggest: None,
            why: "Choose the modifier keys yourself, or press another key.",
        },
    })
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
}

/// The tag of wait `n`.
fn tag(n: u64) -> String {
    format!("settei-{}-{n}", std::process::id())
}

/// Wait `n` for the next key, on a thread: listening first, so the key
/// can't come between, then asking the compositor to catch it, on a
/// connection held while waiting (the compositor waits no longer than
/// it's open, so not after Settei is gone). `None` when given up on:
/// `waits` moved past `n` (another wait began, or the sheet closed).
pub fn catch_key(n: u64, waits: &AtomicU64) -> Result<Option<(u32, Option<KeyboardInfo>)>, String> {
    let given_up = || waits.load(Ordering::SeqCst) != n;
    let mut events = aizome_ipc::Client::connect().and_then(|c| c.events()).map_err(|e| e.to_string())?;
    events.stream().set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
    let mut asker = aizome_ipc::Client::connect().map_err(|e| e.to_string())?;
    match asker.request(&Request::CatchKey { tag: Some(tag(n)) }) {
        Ok(Response::Error { message }) => return Err(message),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    loop {
        if given_up() {
            let _ = asker.request(&Request::CatchKey { tag: None });
            return Ok(None);
        }
        match events.next() {
            Some(Ok(Event::KeyCaught { tag: t, code, keyboard })) if t == tag(n) => return Ok(Some((code, keyboard))),
            Some(Ok(_)) => {}
            Some(Err(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            // A line cut short by the timeout.
            Some(Err(e)) if e.kind() == std::io::ErrorKind::InvalidData => {}
            Some(Err(e)) => return Err(e.to_string()),
            None => return Err("Aizome stopped answering".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_beside_the_space_bar_says_where_super_goes() {
        assert_eq!(beside_space(125).unwrap().suggest, Some(0), "a Mac's Command: nothing to swap");
        assert_eq!(beside_space(56).unwrap().suggest, Some(1), "a PC's Alt: swap Alt and Super");
        assert_eq!(beside_space(29).unwrap().suggest, Some(4));
        assert_eq!(beside_space(94).unwrap().suggest, None, "無変換 can't be Super");
        assert!(beside_space(57).is_err());
        assert!(beside_space(100).is_err());
        for i in [0, 1, 4] {
            assert!(super::super::modifiers::CHOICES.get(i).is_some());
        }
        assert_eq!(super::super::modifiers::CHOICES[1].0, "Swap Alt and Super");
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
    }
}
