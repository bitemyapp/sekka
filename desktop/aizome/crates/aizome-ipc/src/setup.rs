//! Setting up a keyboard: the key pressed beside the space bar says which
//! keyboard it is and which modifier arrangement puts ❖ there. The
//! compositor catches the key ([`Request::CatchKey`]), with the keyboard it
//! came from, since no app hears a modifier key alone. Settei's keyboard
//! setup and Genkan's keyboard step ask this way.

use std::time::Duration;

use crate::{Client, Event, KeyboardInfo, Request, Response};

/// Swap the left Alt and Super keys, preserving right Alt (AltGr).
pub const SWAP_LEFT_ALT_SUPER: &str = "altwin:swap_lalt_lwin";
/// The arrangement that swaps Control and Super, on both sides.
pub const SWAP_CONTROL_SUPER: &str = "ctrl:swap_lwin_lctl,ctrl:swap_rwin_rctl";

/// What the key beside the space bar is, and what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beside {
    /// For “Beside the space bar: …”.
    pub what: &'static str,
    /// The XKB options of the modifier arrangement that puts ❖ there (empty
    /// when it's there already), if one does.
    pub suggest: Option<&'static str>,
    pub why: &'static str,
}

/// The key beside the space bar, by its evdev code; an error says which key
/// to press instead.
pub fn beside_space(code: u32) -> Result<Beside, &'static str> {
    Ok(match code {
        125 => Beside {
            what: "❖ Super (⌘ Command, or the Windows key)",
            suggest: Some(""),
            why: "❖ is there already, as on a Mac: the modifier keys can stay as they are.",
        },
        56 => Beside {
            what: "⌥ Alt (Option)",
            suggest: Some(SWAP_LEFT_ALT_SUPER),
            why: "Swap left Alt and Super to put ❖ there, under your thumb, where ⌘ is on a Mac. Right Alt (AltGr) stays as it is.",
        },
        29 => Beside {
            what: "⌃ Control",
            suggest: Some(SWAP_CONTROL_SUPER),
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

/// Escape, which gives up on the key.
pub const ESCAPE: u32 = 1;

/// Wait for the next key, tagged `tag`: listening first, so the key can't
/// come between, then asking the compositor to catch it, on the same
/// connection, held while waiting (the compositor tells no other
/// connection of the key, and waits no longer than this one is open, so not
/// after the app is gone). Its evdev code and keyboard, or `None` once
/// `given_up` (another wait began, or the app stopped asking). Blocks: call
/// it on a thread of its own.
pub fn catch_key(tag: &str, given_up: impl Fn() -> bool) -> Result<Option<(u32, Option<KeyboardInfo>)>, String> {
    let mut events = Client::connect().and_then(|c| c.events()).map_err(|e| e.to_string())?;
    // The latest to ask is the one that waits: not this, after a newer one.
    if given_up() {
        return Ok(None);
    }
    match events.request(&Request::CatchKey { tag: Some(tag.into()) }) {
        Ok(Response::Error { message }) => return Err(message),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    events.stream().set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
    loop {
        if given_up() {
            // This wait's alone: a newer one, on its own connection, goes on.
            let _ = events.request(&Request::CatchKey { tag: None });
            return Ok(None);
        }
        match events.next() {
            Some(Ok(Event::KeyCaught { tag: t, code, keyboard })) if t == tag => return Ok(Some((code, keyboard))),
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
        assert_eq!(beside_space(125).unwrap().suggest, Some(""), "a Mac's Command: nothing to swap");
        assert_eq!(beside_space(56).unwrap().suggest, Some(SWAP_LEFT_ALT_SUPER), "a PC's Alt: swap the left keys");
        assert_eq!(beside_space(29).unwrap().suggest, Some(SWAP_CONTROL_SUPER));
        assert_eq!(beside_space(94).unwrap().suggest, None, "無変換 can't be Super");
        assert!(beside_space(57).is_err());
        assert!(beside_space(100).is_err());
    }
}
