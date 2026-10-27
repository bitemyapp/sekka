//! The command key for every app: ❖-shortcuts become the Ctrl ones apps
//! that don't use Aizome's kit expect (Ctrl+Shift in terminals).

/// Aizome's own apps name themselves `org.aizome.…` and take ❖ as is.
pub fn is_kit_app(app_id: &str) -> bool {
    app_id.starts_with("org.aizome.")
}

/// A terminal's fallback menu owns Close/Quit. Other clients keep their
/// application commands (and non-kit clients keep normal Ctrl translation).
pub fn terminal_commands(app_id: &str, terminals: &[String], enabled: bool, client_has_focus: bool) -> bool {
    enabled && client_has_focus && !is_kit_app(app_id) && terminals.iter().any(|t| t == app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_window_commands_do_not_take_native_or_other_client_keys() {
        let terminals = vec!["foot".into(), "kitty".into(), "org.aizome.Sumi".into()];
        assert!(terminal_commands("foot", &terminals, true, true));
        assert!(terminal_commands("kitty", &terminals, true, true));
        for app in ["org.aizome.Sumi", "org.gnome.TextEditor", "unknown", ""] {
            assert!(!terminal_commands(app, &terminals, true, true), "{app}");
        }
        assert!(!terminal_commands("foot", &terminals, false, true), "command translation disabled");
        assert!(!terminal_commands("foot", &terminals, true, false), "layer/popup owns keyboard");
        assert!(!terminal_commands("foot", &[], true, true), "terminal list is authoritative");
    }
}
