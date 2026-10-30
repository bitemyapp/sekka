//! Aizome's desktop shell: one sidestep-ui process, one kit
//! [`Program`](aizome_kit::Program), drawing everything of Aizome that
//! isn't a window's frame or an app (see `docs/shell.md`).

pub mod actions;
pub mod agenda;
pub mod apps;
pub mod auth;
pub mod awake;
pub mod barmenus;
pub mod bell;
pub mod carry;
pub mod clock;
pub mod confirm;
pub mod deskbar;
mod deskbar_glue;
pub mod desktop;
mod desktop_glue;
mod greeter;
pub mod gtk;
pub mod icons;
pub mod ipc;
mod keyboards;
pub mod launch;
pub mod location;
pub mod lock;
pub mod look;
pub mod menubar;
pub mod menus;
pub mod notifications;
pub mod osd;
pub mod palette;
pub mod pam;
pub mod power;
pub mod recording;
mod recording_glue;
pub mod services;
pub mod sharing;
pub mod shelf;
pub mod shell;
pub mod sound;
pub mod sys;
pub mod systray;
pub mod tearoff;
pub mod wallpaper;
pub mod widgets;
pub mod wm;

/// `aizome-shell --help`.
pub const USAGE: &str = "usage: aizome-shell

Aizome's desktop shell: the menu bar, Toolchest, Desks strip, systray,
Deskbar and Shelf, command palette, menus, notifications, OSD, desktop,
lock screen and authentication dialog. The compositor starts it.

Usage:
  aizome-shell             run the shell (the compositor starts it)
  aizome-shell msg REQ     ask the running shell: palette, toolchest, lock,
                           dismiss, state, stop-recording, or a JSON request
                           ('{\"request\":\"record\",\"target\":\"screen\"}')

Environment:
  AIZOME_SOCKET          the compositor's IPC socket
  AIZOME_SHELL_SOCKET    where apps publish their menus to the shell
  AIZOME_SESSION=1       the machine's session (the compositor says): serve
                         polkit, hold logind's sleep and lock for logind
  AIZOME_FONT_DIR        fonts to install for this process
  SIDESTEP_FRAME_STATS=1 print each surface's frame times every 5 s";

/// Run the shell until the session ends: the process's exit code.
pub fn run() -> i32 {
    let opts = shell::Options { live: true, ..shell::Options::default() };
    match aizome_kit::try_run(shell::Shell::new(opts)) {
        Ok(_) => 0,
        Err(e) => {
            log::error!("{e}");
            1
        }
    }
}

/// `aizome-shell msg REQUEST [JSON fields]`: ask the running shell (at
/// `$AIZOME_SHELL_SOCKET`) to do something, as the compositor does:
/// `aizome-shell msg palette`, `aizome-shell msg '{"request":"osd","kind":"volume","value":0.4}'`.
pub fn msg(args: Vec<String>) -> i32 {
    let Some(first) = args.first() else {
        eprintln!("usage: aizome-shell msg REQUEST|JSON");
        return 2;
    };
    let json = if first.trim_start().starts_with('{') { first.clone() } else { format!("{{\"request\":\"{first}\"}}") };
    let request: aizome_shell_proto::ShellRequest = match serde_json::from_str(&json) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("aizome-shell msg: {e}");
            return 2;
        }
    };
    let path = apps::server::socket_path();
    match aizome_shell_proto::request(&path, request) {
        Ok((ok, message)) => {
            if !message.is_empty() {
                println!("{message}");
            }
            if ok { 0 } else { 1 }
        }
        Err(e) => {
            eprintln!("aizome-shell msg: {}: {e}", path.display());
            1
        }
    }
}
