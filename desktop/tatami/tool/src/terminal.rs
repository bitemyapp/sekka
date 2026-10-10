// SPDX-License-Identifier: MIT OR Apache-2.0
//! Terminals and terminal applications in foot, Omarchy 4's terminal. foot
//! finds the Tatami configuration through XDG_CONFIG_DIRS, which the
//! session prepends with /etc/xdg/tatami; a user's own ~/.config/foot
//! still takes precedence.
use crate::{CONFIG, hypr, util};
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
        .and_then(|pid| children(pid).into_iter().last())
        .and_then(|shell| {
            let exe = fs::read_link(format!("/proc/{shell}/exe")).ok()?;
            let cwd = fs::read_link(format!("/proc/{shell}/cwd")).ok()?;
            (is_shell(&exe, &etc_shells) && cwd.is_dir()).then(|| cwd.display().to_string())
        })
        .unwrap_or_else(util::home)
}

/// Open a terminal in the focused terminal's directory.
pub fn terminal(command: &[String]) {
    let cwd = format!("--working-directory={}", active_cwd());
    let mut args = vec![cwd.as_str()];
    if !command.is_empty() {
        args.push("--");
        args.extend(command.iter().map(String::as_str));
    }
    util::launch("foot", &args);
}

/// How a terminal application opens.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tui {
    /// A new window every time.
    New,
    /// Focus a window already showing it (panels such as audio controls).
    Focus,
    /// Keep the window after the program exits (About).
    Hold,
    /// A bar panel (audio, Bluetooth): opens, or comes forward when open,
    /// and closes when it already has focus, like Omarchy 4's panels.
    Panel,
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
    if mode == Tui::Panel
        && let Ok(window) = hypr::json("activewindow")
        && window["class"].as_str() == Some(class.as_str())
        && let Some(address) = hypr::address(&window)
    {
        hypr::dispatch(&format!(
            "hl.dsp.window.close({{ window = \"address:{address}\" }})"
        ));
        return;
    }
    if matches!(mode, Tui::Focus | Tui::Panel)
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
    let app_id = format!("--app-id={class}");
    let mut args = vec![app_id.as_str()];
    if mode == Tui::Hold {
        args.push("--hold");
    }
    args.push("--");
    args.extend(command.iter().map(String::as_str));
    // btop reads only ~/.config/btop; until the user has their own
    // configuration, use the Tatami one and its Tokyo Night theme.
    let btop = format!("{CONFIG}/btop/btop.conf");
    let themes = format!("{CONFIG}/btop/themes");
    let own = format!("{}/.config/btop/btop.conf", util::home());
    if name == "btop" && command.len() == 1 && !Path::new(&own).exists() {
        args.extend(["--config", btop.as_str(), "--themes-dir", themes.as_str()]);
    }
    util::launch("foot", &args);
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
