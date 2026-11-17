//! The command key for every app: ❖-shortcuts become the Ctrl ones apps
//! that don't use Aizome's kit expect (Ctrl+Shift in terminals).

use aizome_comp_core::ipc::IpcServer;

/// Kit apps announce their command keys on a live compositor connection,
/// independently of their desktop identity. The old IDs keep already-built
/// Aizome apps and Genkan working before they can announce themselves.
pub fn is_kit_app(app_id: &str, pid: Option<i32>, ipc: &IpcServer) -> bool {
    ipc.uses_native_command_keys(pid) || app_id.starts_with("org.aizome.") || app_id == "org.calamares.NixOSRust"
}

/// A terminal's fallback menu owns Close/Quit. Other clients keep their
/// application commands (and non-kit clients keep normal Ctrl translation).
pub fn terminal_commands(
    app_id: &str,
    terminals: &[String],
    enabled: bool,
    client_has_focus: bool,
    kit_app: bool,
) -> bool {
    enabled && client_has_focus && !kit_app && terminals.iter().any(|t| t == app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genkans_preserved_identity_is_native_without_exempting_other_apps() {
        let ipc = IpcServer::default();
        assert!(is_kit_app("org.calamares.NixOSRust", None, &ipc));
        assert!(is_kit_app("org.aizome.Sumi", None, &ipc));
        for app in ["org.calamares.Other", "org.calamares.NixOSRust.Other", "org.calamares.NixOSRust ", ""] {
            assert!(!is_kit_app(app, None, &ipc), "{app}");
        }
    }

    #[test]
    fn terminal_window_commands_do_not_take_native_or_other_client_keys() {
        let ipc = IpcServer::default();
        let terminals = vec![
            "foot".into(),
            "kitty".into(),
            "org.aizome.Sumi".into(),
            "org.calamares.NixOSRust".into(),
            "org.example.Terminal".into(),
        ];
        assert!(terminal_commands("foot", &terminals, true, true, false));
        assert!(terminal_commands("kitty", &terminals, true, true, false));
        for app in ["org.aizome.Sumi", "org.calamares.NixOSRust", "org.gnome.TextEditor", "unknown", ""] {
            assert!(!terminal_commands(app, &terminals, true, true, is_kit_app(app, None, &ipc)), "{app}");
        }
        assert!(!terminal_commands("foot", &terminals, false, true, false), "command translation disabled");
        assert!(!terminal_commands("foot", &terminals, true, false, false), "layer/popup owns keyboard");
        assert!(!terminal_commands("foot", &[], true, true, false), "terminal list is authoritative");
        assert!(!terminal_commands("org.example.Terminal", &terminals, true, true, true), "native terminal");
    }
}
