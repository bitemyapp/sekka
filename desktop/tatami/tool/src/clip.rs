// SPDX-License-Identifier: MIT OR Apache-2.0
//! Universal copy, paste and cut (Super+C, V and X), after Omarchy 4's
//! clipboard bindings: terminals get Ctrl+Insert and Shift+Insert, other
//! windows Ctrl+C and Ctrl+V. The chord goes to the focused window with
//! explicit modifiers, so the Super key still held does not join it. (Separate
//! key-down and key-up events sent from here, as Omarchy sends them from its
//! configuration, left bracketed-paste markers in the shell during testing.)
use crate::hypr;

/// Modifiers and key for an action, by whether the window is a terminal.
pub fn chord(action: &str, terminal: bool) -> Option<(&'static str, &'static str)> {
    Some(match (action, terminal) {
        ("copy", true) => ("CTRL", "Insert"),
        ("copy", false) => ("CTRL", "C"),
        ("paste", true) => ("SHIFT", "Insert"),
        ("paste", false) => ("CTRL", "V"),
        ("cut", _) => ("CTRL", "X"),
        _ => return None,
    })
}

/// The window rules tag terminals "terminal"; dynamic tags end in "*".
pub fn is_terminal(window: &serde_json::Value) -> bool {
    window["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tag| tag.as_str())
        .any(|tag| tag.trim_end_matches('*') == "terminal")
}

pub fn send(action: &str) {
    let terminal = hypr::json("activewindow").is_ok_and(|window| is_terminal(&window));
    let Some((mods, key)) = chord(action, terminal) else {
        return;
    };
    hypr::dispatch(&format!(
        "hl.dsp.send_shortcut({{ mods = \"{mods}\", key = \"{key}\" }})"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn terminals_use_the_insert_chords() {
        assert_eq!(chord("copy", true), Some(("CTRL", "Insert")));
        assert_eq!(chord("paste", false), Some(("CTRL", "V")));
        assert_eq!(chord("cut", true), Some(("CTRL", "X")));
        assert_eq!(chord("select", false), None);
        assert!(is_terminal(
            &json!({"tags": ["default-opacity*", "terminal*"]})
        ));
        assert!(!is_terminal(&json!({"tags": ["default-opacity*"]})));
        assert!(!is_terminal(&json!({})));
    }
}
