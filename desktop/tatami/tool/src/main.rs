// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tatami`: session launcher and desktop commands for the Tatami session
//! (keyboard-driven Hyprland inspired by Omarchy), replacing the Bash `omarchy-*` scripts of Omarchy 4
//! (https://github.com/basecamp/omarchy, MIT) that it is modelled on.
mod background;
mod browser;
mod capture;
mod clip;
mod displays;
mod hypr;
mod info;
mod media;
mod menu;
mod network;
mod power;
mod terminal;
mod toggle;
mod util;
mod weather;
mod window;

use std::{path::Path, process::ExitCode};

/// Tatami configuration shipped by the NixOS module.
pub const CONFIG: &str = "/etc/xdg/tatami";

const USAGE: &str = "usage: tatami <command>
  session                         start Hyprland with the Tatami configuration
  restore                         apply remembered toggles (session start)
  reload                          apply a rebuilt Tatami to the running session
  menu [learn|trigger|capture|screenrecord|toggle|hardware|setup|network|dns|system]
  apps | emoji | keybindings | about | power-profile | edit-config
  network | bluetooth | audio             the bar's panels
  display                         the Displays window
  displays apply [--dry-run]      keep and apply display settings (JSON on stdin)
  nightlight [on|off|status|KELVIN] Night Shift for the Displays window
  dns <dhcp|cloudflare|google|custom>     DNS for the connection in use
  background [set <image>|ensure|path]    desktop background picker and choice
  weather [refresh|place]                 bar weather, refresh, choose the place
  terminal [command...]           terminal in the focused terminal's directory
  tui <command...>                terminal application (focuses an open one)
  editor [file...] | files [--cwd] | launch <program> [args...]
  browser [--private] [url...]
  volume <raise|lower|mute-toggle|+N|-N> | mic-mute
  brightness <+5%|5%-|N%|+1%|1%-> | keyboard-brightness <up|down|cycle>
  media <next|previous|play-pause>
  clipboard <copy|paste|cut|history>
  screenshot [smart|region|windows|fullscreen] | color-picker
  screenrecord [--desktop-audio] [--microphone] [--stop|--menu] | capture <text|qr>
  window <pop|tiled-fullscreen|transparency|close-all>
  scale <up|down> | laptop-display | touchpad [on|off|toggle] | zoom <in|reset>
  toggle <idle|nightlight|notifications|bar|gaps|aspect|layout>
  indicator <idle|notifications|nightlight|screenrecording|tray>
  notify <time|battery> | bluetooth-toggle
  lock [--lock-only] | wake [seconds] | logout | reboot | shutdown | suspend | hibernate";

/// XDG_CONFIG_DIRS with the Tatami directory first, without duplicates.
pub fn config_dirs(existing: Option<&str>) -> String {
    let mut dirs = vec![CONFIG];
    dirs.extend(
        existing
            .unwrap_or("/etc/xdg")
            .split(':')
            .filter(|dir| !dir.is_empty() && *dir != CONFIG),
    );
    dirs.join(":")
}

/// The configuration Hyprland starts with when the user has none of their
/// own: a file outside the Nix store that loads the shipped one. Hyprland
/// resolves the path it is given once and re-reads that file on every
/// reload; given the /etc symlink, it would keep reading the store file the
/// session began with, whatever the system has installed since.
pub fn session_config() -> String {
    format!(
        "-- Written by `tatami session` for this session: Tatami's configuration,
-- as installed when Hyprland reads it.
dofile(\"{CONFIG}/hypr/hyprland.lua\")
"
    )
}

/// Start the compositor with the user's ~/.config/tatami/hyprland.lua if
/// they have one (Setup › Hyprland makes it), otherwise session_config.
fn session() -> ExitCode {
    // The runtime directory outlives a session that ends while another is open.
    window::forget_monitors();
    let user = format!("{}/.config/tatami/hyprland.lua", util::home());
    let session = window::session_dir().map(|dir| dir.join("hyprland.lua"));
    let config = if Path::new(&user).is_file() {
        user
    } else if let Some(path) = session.filter(|path| {
        path.parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::write(path, session_config()).is_ok()
    }) {
        path.to_string_lossy().into_owned()
    } else {
        // Pinned, so `tatami reload` leaves Hyprland's configuration alone.
        format!("{CONFIG}/hypr/hyprland.lua")
    };
    let launcher = util::which("start-hyprland")
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_else(|| "/run/current-system/sw/bin/start-hyprland".into());
    let mut command = std::process::Command::new(&launcher);
    command
        .env(
            "XDG_CONFIG_DIRS",
            config_dirs(std::env::var("XDG_CONFIG_DIRS").ok().as_deref()),
        )
        .args(["--", "--config", &config]);
    let error = std::os::unix::process::CommandExt::exec(&mut command);
    eprintln!("tatami: cannot start {launcher}: {error}");
    ExitCode::FAILURE
}

/// Apply a rebuilt Tatami (after `nixos-rebuild switch`) to the running
/// session, without logging out: systemd reads the session units' new
/// definitions, Hyprland its configuration with every monitor left on or
/// off as it is (window::reload_hyprland), and the running units restart
/// from the new /etc/xdg/tatami. The session's environment
/// (/etc/xdg/uwsm/env-tatami) still takes a new login: uwsm reads it, and
/// clears it at logout, only for the variables it set itself.
fn reload() -> ExitCode {
    let mut failed = vec![];
    if !util::run("systemctl", &["--user", "daemon-reload"]) {
        failed.push("systemd user units");
    }
    let pinned = match window::reload_hyprland() {
        window::Reload::Done => false,
        window::Reload::Failed => {
            failed.push("Hyprland");
            false
        }
        window::Reload::Pinned => true,
    };
    // Only the units that are running: the on-demand nightlight stays off.
    if !util::run("systemctl", &["--user", "try-restart", "tatami-*.service"]) {
        failed.push("session units");
    }
    // Mako is among the restarted units: a notification sent before it is
    // back on the bus has D-Bus start another notification daemon.
    for _ in 0..40 {
        if notifications_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let later = if pinned {
        "Hyprland's configuration applies from the next login"
    } else {
        ""
    };
    if failed.is_empty() {
        println!("Tatami reloaded");
        if pinned {
            println!("{later}");
        }
        util::notify("\u{f0450}", "Tatami reloaded", later);
        ExitCode::SUCCESS
    } else {
        let failed = failed.join(", ");
        eprintln!("tatami: could not reload {failed}");
        util::notify("\u{f0450}", "Tatami reload failed", &failed);
        ExitCode::FAILURE
    }
}

/// Whether a notification daemon has its bus name, asked without starting one.
fn notifications_ready() -> bool {
    util::output(
        "busctl",
        &[
            "--user",
            "call",
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            "s",
            "org.freedesktop.Notifications",
        ],
    )
    .is_ok_and(|reply| reply.trim() == "b true")
}

/// The user's configuration as Setup › Hyprland first writes it: Tatami's
/// defaults, then their own settings, so updates to the defaults still
/// reach them (Omarchy's user hyprland.lua works the same way).
pub fn user_config() -> String {
    format!(
        "-- Your Tatami Hyprland configuration, used from the next login.
-- Tatami's defaults load first, so their updates keep reaching you;
-- settings below override them. See https://wiki.hypr.land/Configuring/Start/
dofile(\"{CONFIG}/hypr/hyprland.lua\")

-- For example, wider gaps:
-- hl.config({{ general = {{ gaps_out = 20 }} }})
"
    )
}

/// Setup › Hyprland: edit the user's own configuration, made the first
/// time. The session uses it from the next login on.
fn edit_config() {
    use std::os::unix::fs::PermissionsExt;
    let dir = format!("{}/.config/tatami", util::home());
    let user = format!("{dir}/hyprland.lua");
    if !Path::new(&user).exists() {
        let written = std::fs::create_dir_all(&dir).is_ok()
            && std::fs::write(&user, user_config()).is_ok()
            && std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o644)).is_ok();
        if !written {
            util::notify(
                "\u{f359}",
                "Cannot create ~/.config/tatami/hyprland.lua",
                "",
            );
            return;
        }
        util::notify(
            "\u{f359}",
            "Your Hyprland configuration",
            "~/.config/tatami/hyprland.lua adds to Tatami's defaults from the next login",
        );
    }
    terminal::editor(&[user]);
}

/// Super+Shift+F (and Super+Alt+Shift+F in the terminal's directory).
fn files(cwd: bool) {
    let dir = cwd.then(terminal::active_cwd);
    let mut args = vec!["--new-window"];
    if let Some(dir) = &dir {
        args.push(dir);
    }
    util::launch("nautilus", &args);
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let first = |default: &'static str| rest.first().map_or(default, String::as_str);
    let ok = match args.first().map(String::as_str) {
        Some("session") => return session(),
        Some("restore") => {
            window::restore();
            true
        }
        Some("reload") => return reload(),
        Some("menu") => {
            menu::show(first(""));
            menu::walker_args(first("")).is_some()
        }
        Some("dns") => network::dns_command(first("")),
        Some("background") => match first("") {
            "" => {
                background::pick();
                true
            }
            "set" if rest.len() == 2 => background::set(&rest[1]),
            "ensure" => {
                background::ensure();
                true
            }
            "path" => {
                println!("{}", background::path().display());
                true
            }
            _ => false,
        },
        Some("weather") => match first("") {
            "" => {
                println!("{}", weather::bar());
                true
            }
            "refresh" => {
                weather::refresh();
                true
            }
            "place" => {
                weather::choose_place();
                true
            }
            _ => false,
        },
        // The bar's display icon, Super+Ctrl+D and Setup › Monitors. A second
        // launch brings the open window forward (GtkApplication).
        Some("display") => {
            menu::after_menu();
            util::launch("tatami-displays", &[]);
            true
        }
        Some("nightlight") => toggle::nightlight_command(first("status")),
        Some("displays") if first("") == "apply" => {
            match displays::apply_command(rest.iter().any(|arg| arg == "--dry-run")) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("tatami: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        Some("audio") => {
            menu::after_menu();
            let wiremix = ["wiremix", "--tab", "output"].map(str::to_owned);
            terminal::tui(&wiremix, terminal::Tui::Panel);
            true
        }
        Some("apps") => {
            menu::apps();
            true
        }
        Some("emoji") => {
            menu::emoji();
            true
        }
        Some("edit-config") => {
            edit_config();
            true
        }
        Some("keybindings") => {
            menu::keybindings();
            true
        }
        Some("power-profile") => {
            menu::power_profile();
            true
        }
        Some("network") => {
            network::show();
            true
        }
        Some("bluetooth") => {
            power::bluetooth();
            true
        }
        Some("about") => {
            terminal::tui(
                &[
                    "fastfetch".into(),
                    "--config".into(),
                    format!("{CONFIG}/fastfetch/config.jsonc"),
                ],
                terminal::Tui::Hold,
            );
            true
        }
        Some("terminal") => {
            terminal::terminal(rest);
            true
        }
        Some("tui") if !rest.is_empty() => {
            terminal::tui(rest, terminal::Tui::Focus);
            true
        }
        Some("editor") => {
            terminal::editor(rest);
            true
        }
        Some("files") => {
            files(rest.iter().any(|arg| arg == "--cwd"));
            true
        }
        Some("launch") if !rest.is_empty() => {
            let args: Vec<&str> = rest[1..].iter().map(String::as_str).collect();
            util::launch(&rest[0], &args);
            true
        }
        Some("browser") => {
            browser::launch(rest);
            true
        }
        Some("volume") => {
            media::volume(first("raise"));
            true
        }
        Some("mic-mute") => {
            media::mic_mute();
            true
        }
        Some("brightness") => {
            media::brightness(first("+5%"));
            true
        }
        Some("keyboard-brightness") => {
            media::keyboard_brightness(first("cycle"));
            true
        }
        Some("media") => {
            media::media(first("play-pause"));
            true
        }
        Some("clipboard") if first("") == "history" => {
            menu::clipboard();
            true
        }
        Some("clipboard") => {
            clip::send(first(""));
            clip::chord(first(""), false).is_some()
        }
        Some("screenshot") => {
            capture::screenshot(first("smart"));
            true
        }
        Some("screenshot-notify") if rest.len() == 1 => {
            capture::notify_edit(&rest[0]);
            true
        }
        Some("screenrecord") => {
            capture::screenrecord(rest);
            true
        }
        Some("capture") => match first("") {
            "text" => {
                capture::text();
                true
            }
            "qr" => {
                capture::qr();
                true
            }
            _ => false,
        },
        Some("color-picker") => {
            capture::color_picker();
            true
        }
        Some("window") => match first("") {
            "pop" => {
                window::pop();
                true
            }
            "tiled-fullscreen" => {
                window::tiled_fullscreen();
                true
            }
            "transparency" => {
                window::transparency();
                true
            }
            "close-all" => {
                window::close_all();
                true
            }
            _ => false,
        },
        Some("scale") => {
            window::scale(first("up") == "up");
            true
        }
        Some("laptop-display") => {
            window::laptop_display();
            true
        }
        Some("touchpad") => {
            window::touchpad(first("toggle"));
            true
        }
        Some("zoom") => {
            window::zoom(first("in"));
            true
        }
        Some("toggle") => match first("") {
            "idle" => {
                toggle::idle();
                true
            }
            "nightlight" => {
                toggle::nightlight();
                true
            }
            "notifications" => {
                toggle::notifications();
                true
            }
            "bar" => {
                toggle::bar();
                true
            }
            "gaps" => {
                window::gaps();
                true
            }
            "aspect" => {
                window::aspect();
                true
            }
            "layout" => {
                window::workspace_layout();
                true
            }
            _ => false,
        },
        Some("indicator") => {
            println!("{}", toggle::indicator(first("")));
            true
        }
        Some("notify") => match first("") {
            "time" => {
                info::time();
                true
            }
            "battery" => {
                info::battery();
                true
            }
            _ => false,
        },
        Some("bluetooth-toggle") => {
            power::bluetooth_toggle();
            true
        }
        Some("lock") => {
            power::lock(rest.iter().any(|arg| arg == "--lock-only"));
            true
        }
        Some("lock-wait") => {
            power::lock_wait();
            true
        }
        Some("wake") => {
            power::wake(first("0").parse().unwrap_or(0));
            true
        }
        Some("logout") => {
            power::leave(power::Leave::Logout);
            true
        }
        Some("reboot") => {
            power::leave(power::Leave::Reboot);
            true
        }
        Some("shutdown") => {
            power::leave(power::Leave::Shutdown);
            true
        }
        Some("suspend") => {
            util::run("systemctl", &["suspend"]);
            true
        }
        Some("hibernate") => {
            power::hibernate();
            true
        }
        _ => false,
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        eprintln!("{USAGE}");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tatami_configuration_is_searched_first_once() {
        assert_eq!(config_dirs(None), "/etc/xdg/tatami:/etc/xdg");
        assert_eq!(
            config_dirs(Some(
                "/etc/xdg:/etc/xdg/tatami::/run/current-system/sw/etc/xdg"
            )),
            "/etc/xdg/tatami:/etc/xdg:/run/current-system/sw/etc/xdg"
        );
    }
    #[test]
    fn session_configuration_loads_the_installed_one() {
        let config = session_config();
        let code: Vec<&str> = config
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with("--"))
            .collect();
        assert_eq!(code, ["dofile(\"/etc/xdg/tatami/hypr/hyprland.lua\")"]);
    }
    #[test]
    fn user_configuration_loads_the_defaults_first() {
        let config = user_config();
        let code: Vec<&str> = config
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with("--"))
            .collect();
        assert_eq!(code, ["dofile(\"/etc/xdg/tatami/hypr/hyprland.lua\")"]);
    }
}
