// SPDX-License-Identifier: MIT OR Apache-2.0
//! Launch the default web browser, searching the desktop-entry locations that
//! NixOS actually uses (profiles under /etc and /run), not only /usr.
use crate::util;
use std::{env, fs, path::PathBuf};

fn data_dirs() -> Vec<PathBuf> {
    let home = util::home();
    let mut dirs = vec![PathBuf::from(
        env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share")),
    )];
    dirs.extend(
        env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/run/current-system/sw/share".into())
            .split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(PathBuf::from),
    );
    dirs
}

/// Whether a desktop entry's [Desktop Entry] lists the WebBrowser category.
/// Other programs claim web links too: the ChatGPT app does.
pub fn is_browser(entry: &str) -> bool {
    let mut in_main = false;
    entry.lines().map(str::trim).any(|line| {
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            return false;
        }
        in_main
            && line.strip_prefix("Categories=").is_some_and(|categories| {
                categories
                    .split(';')
                    .any(|category| category == "WebBrowser")
            })
    })
}

/// The program from a desktop entry's first `Exec=` line in [Desktop Entry].
pub fn exec_program(entry: &str) -> Option<String> {
    let mut in_main = false;
    for line in entry.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
        } else if in_main && let Some(exec) = line.strip_prefix("Exec=") {
            return exec
                .split_whitespace()
                .next()
                .map(|program| program.trim_matches('"').to_owned());
        }
    }
    None
}

/// Private-browsing flag spelling by browser family.
pub fn private_flag(program: &str) -> &'static str {
    let name = program.rsplit('/').next().unwrap_or(program);
    if name.contains("firefox") || name.contains("librewolf") || name.contains("zen") {
        "--private-window"
    } else if name.contains("edge") {
        "--inprivate"
    } else {
        "--incognito"
    }
}

fn default_program() -> Option<String> {
    let id = util::output("xdg-settings", &["get", "default-web-browser"])
        .ok()
        .map(|id| id.trim().to_owned())
        .filter(|id| id.ends_with(".desktop") && !id.contains('/'))?;
    data_dirs()
        .into_iter()
        .find_map(|dir| fs::read_to_string(dir.join("applications").join(&id)).ok())
        .filter(|entry| is_browser(entry))
        .and_then(|entry| exec_program(&entry))
}

pub fn launch(args: &[String]) {
    let program = default_program()
        .or_else(|| {
            ["firefox", "chromium", "google-chrome-stable"]
                .into_iter()
                .find(|name| util::which(name).is_some())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "xdg-open".into());
    let flag = private_flag(&program);
    let args: Vec<&str> = args
        .iter()
        .map(|arg| {
            if arg == "--private" {
                flag
            } else {
                arg.as_str()
            }
        })
        .collect();
    util::launch(&program, &args);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exec_line_comes_from_the_main_section() {
        let entry = "[Desktop Entry]\nName=Firefox\nExec=firefox --name firefox %U\n[Desktop Action new-private-window]\nExec=firefox --private-window %U\n";
        assert_eq!(exec_program(entry).as_deref(), Some("firefox"));
        let action_first = "[Desktop Action x]\nExec=wrong\n[Desktop Entry]\nExec=\"/nix/store/a-chromium/bin/chromium\" %U\n";
        assert_eq!(
            exec_program(action_first).as_deref(),
            Some("/nix/store/a-chromium/bin/chromium")
        );
        assert_eq!(exec_program("[Desktop Entry]\nName=x\n"), None);
    }
    #[test]
    fn only_web_browsers_count_as_the_default_browser() {
        assert!(is_browser(
            "[Desktop Entry]\nName=Firefox\nCategories=Network;WebBrowser;\n"
        ));
        assert!(!is_browser(
            "[Desktop Entry]\nName=ChatGPT\nCategories=Office;Utility;\nMimeType=x-scheme-handler/https;\n"
        ));
        assert!(!is_browser(
            "[Desktop Entry]\nName=App\n[Desktop Action web]\nCategories=WebBrowser;\n"
        ));
    }
    #[test]
    fn private_flags_by_family() {
        assert_eq!(
            private_flag("/run/current-system/sw/bin/firefox"),
            "--private-window"
        );
        assert_eq!(private_flag("chromium"), "--incognito");
        assert_eq!(private_flag("microsoft-edge"), "--inprivate");
    }
}
