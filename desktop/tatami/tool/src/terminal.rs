// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hotaru terminals and terminal applications, with independent windows
//! for the desktop's shortcuts and application-specific window rules.
use crate::{hypr, util};
use std::{fs, path::Path};

const SHELLS: &[&str] = &[
    "bash", "zsh", "fish", "sh", "dash", "nu", "ksh", "mksh", "tcsh", "xonsh", "elvish",
];

/// Whether an executable is a login shell. NixOS lists store paths and
/// /run/current-system/sw/bin entries in /etc/shells, so compare names, not
/// resolved paths (Omarchy's exact path check always failed on NixOS).
pub fn is_shell(exe: &Path, etc_shells: &str) -> bool {
    let Some(name) = exe.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let name = name.trim_start_matches('.').trim_end_matches("-wrapped");
    SHELLS.contains(&name)
        || etc_shells
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter_map(|line| Path::new(line.trim()).file_name()?.to_str())
            .any(|shell| shell == name)
}

fn is_hotaru(exe: &Path) -> bool {
    exe.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.trim_start_matches('.').trim_end_matches("-wrapped"),
                "hotaru" | "aizome-hotaru"
            )
        })
}

/// Children of a process, newest (highest PID) last.
fn children(pid: i32) -> Vec<i32> {
    let mut found: Vec<i32> = fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|child| {
            fs::read_to_string(format!("/proc/{child}/stat"))
                .ok()
                .and_then(|stat| parent_from_stat(&stat))
                == Some(pid)
        })
        .collect();
    found.sort_unstable();
    found
}

/// The PPID field of /proc/<pid>/stat. The command name is parenthesized and
/// may itself contain spaces or parentheses, so parse after the last ')'.
pub fn parent_from_stat(stat: &str) -> Option<i32> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Working directory of the shell in the focused terminal, else $HOME.
pub fn active_cwd() -> String {
    let etc_shells = fs::read_to_string("/etc/shells").unwrap_or_default();
    hypr::json("activewindow")
        .ok()
        .and_then(|window| hypr::active_window_pid(&window))
        .and_then(|pid| {
            // Hotaru knows which of its tabs/splits has focus. Child PID order
            // cannot tell us that, and OSC 7 can be absent or out of date.
            if fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| is_hotaru(&exe)) {
                return util::output(
                    "aizome-hotaru",
                    &["--print-working-directory", &pid.to_string()],
                )
                .ok()
                .filter(|cwd| Path::new(cwd).is_absolute() && Path::new(cwd).is_dir());
            }
            // Other terminals: only an unambiguous shell is safe to inherit.
            // In particular, a failed Hotaru query must not pick a random tab.
            let mut shells = children(pid).into_iter().filter_map(|shell| {
                let exe = fs::read_link(format!("/proc/{shell}/exe")).ok()?;
                let cwd = fs::read_link(format!("/proc/{shell}/cwd")).ok()?;
                (is_shell(&exe, &etc_shells) && cwd.is_dir()).then(|| cwd.display().to_string())
            });
            let cwd = shells.next()?;
            shells.next().is_none().then_some(cwd)
        })
        .unwrap_or_else(util::home)
}

/// Open a terminal in the focused terminal's directory.
pub fn terminal(command: &[String]) {
    let cwd = active_cwd();
    let mut args = vec!["--new-window", "--working-directory", cwd.as_str()];
    if !command.is_empty() {
        args.push("--");
        args.extend(command.iter().map(String::as_str));
    }
    util::launch("aizome-hotaru", &args);
}

/// How a terminal application opens.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tui {
    /// A new window every time.
    New,
    /// Focus a window already showing it.
    Focus,
    /// Keep the window after the program exits (About).
    Hold,
}

/// A terminal application in a window with app-id org.tatami.<name>, as
/// omarchy-launch-tui does; the window rules float the panel-like ones.
pub fn tui(command: &[String], mode: Tui) {
    let Some(program) = command.first() else {
        return;
    };
    let name = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tui");
    let class = format!("org.tatami.{name}");
    if mode == Tui::Focus
        && let Ok(clients) = hypr::json("clients")
        && let Some(address) = clients
            .as_array()
            .into_iter()
            .flatten()
            .find(|client| client["class"].as_str() == Some(class.as_str()))
            .and_then(hypr::address)
    {
        hypr::dispatch(&format!(
            "hl.dsp.focus({{ window = \"address:{address}\" }})"
        ));
        return;
    }
    let mut args = vec!["--app-id", class.as_str()];
    if mode == Tui::Hold {
        args.push("--hold");
    }
    args.push("--");
    args.extend(command.iter().map(String::as_str));
    util::launch("aizome-hotaru", &args);
}

/// Terminal editors open in a terminal, others in their own window.
pub fn is_terminal_editor(editor: &str) -> bool {
    let name = Path::new(editor)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(editor);
    matches!(
        name,
        "nvim" | "vim" | "vi" | "nano" | "micro" | "hx" | "helix" | "kak" | "emacs" | "mg"
    )
}

/// Super+Shift+N: the user's $EDITOR (NixOS defaults to nano).
pub fn editor(files: &[String]) {
    let configured = std::env::var("EDITOR").unwrap_or_default();
    let editor = configured
        .split_whitespace()
        .next()
        .filter(|program| util::which(program).is_some() || program.starts_with('/'))
        .unwrap_or("nano")
        .to_owned();
    let mut command = vec![editor.clone()];
    command.extend(files.iter().cloned());
    if is_terminal_editor(&editor) {
        tui(&command, Tui::New);
    } else {
        let args: Vec<&str> = files.iter().map(String::as_str).collect();
        util::launch(&editor, &args);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shells_are_recognized_by_name_on_nixos() {
        let etc = "/run/current-system/sw/bin/bash\n/nix/store/abc-zsh-5.9/bin/zsh\n# comment\n";
        assert!(is_shell(Path::new("/nix/store/xyz-bash-5.3/bin/bash"), etc));
        assert!(is_shell(Path::new("/nix/store/xyz-fish-4/bin/fish"), ""));
        assert!(is_shell(Path::new("/nix/store/xyz/bin/.zsh-wrapped"), etc));
        assert!(!is_shell(Path::new("/nix/store/xyz-nvim/bin/nvim"), etc));
        assert!(!is_shell(Path::new("/"), etc));
    }
    #[test]
    fn hotaru_queries_include_both_package_wrappers_and_tui_app_ids() {
        // Use the executable, not the window class: a TUI has its own app ID.
        for name in [
            "hotaru",
            "aizome-hotaru",
            ".hotaru-wrapped",
            ".aizome-hotaru-wrapped",
        ] {
            assert!(is_hotaru(&Path::new("/nix/store/package/bin").join(name)));
        }
        assert!(!is_hotaru(Path::new("/run/current-system/sw/bin/foot")));
        assert!(!is_hotaru(Path::new(
            "/run/current-system/sw/bin/tatami-activity"
        )));
    }
    #[test]
    fn stat_parent_survives_odd_command_names() {
        assert_eq!(parent_from_stat("123 (bash) S 77 123 123 0"), Some(77));
        assert_eq!(parent_from_stat("9 (a) b (c)) R 4 9 9"), Some(4));
        assert_eq!(parent_from_stat("garbage"), None);
    }
    #[test]
    fn terminal_editors_open_in_a_terminal() {
        assert!(is_terminal_editor("nano"));
        assert!(is_terminal_editor("/run/current-system/sw/bin/nvim"));
        assert!(!is_terminal_editor("code"));
        assert!(!is_terminal_editor("gnome-text-editor"));
    }
}
