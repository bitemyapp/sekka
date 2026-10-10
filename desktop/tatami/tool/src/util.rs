// SPDX-License-Identifier: MIT OR Apache-2.0
//! Small process helpers. Commands are always given as argument vectors; no
//! shell is involved, so selections and window titles are never interpreted.
use std::{
    io::Write,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
};

pub type Result<T> = std::result::Result<T, String>;

/// Run a program and return its stdout, or an error naming the program.
pub fn output(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{program} exited with {}", out.status))
    }
}

/// Run a program to completion, ignoring its output.
pub fn run(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Start a program in its own session so it outlives this helper.
pub fn spawn(program: &str, args: &[&str]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
}

/// Start an application as a transient systemd service in
/// app-graphical.slice, as Omarchy does with uwsm-app. Otherwise it would
/// run inside the compositor's unit, where systemd-oomd under memory
/// pressure would stop the whole session instead of the one application.
///
/// The service manager starts it, so its parent is not this helper or the
/// launcher that ran it (`tatami launch` is Elephant's launch prefix), and
/// restarting the launcher (`tatami reload`) cannot end it. A scope ran the
/// program as the launcher's child, and an application in a NixOS FHS
/// sandbox (`bwrap --die-with-parent`, such as Claude Desktop) is killed
/// when its parent exits. It keeps the caller's environment and working
/// directory, as a scope did: XDG_CONFIG_DIRS with the Tatami
/// configuration, and Hyprland's variables when a key binding started it.
pub fn launch(program: &str, args: &[&str]) {
    if which("systemd-run").is_some() {
        let env: Vec<(String, String)> = std::env::vars_os()
            .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        let full = launch_args(program, args, &env);
        let full: Vec<&str> = full.iter().map(String::as_str).collect();
        spawn("systemd-run", &full);
    } else {
        spawn(program, args);
    }
}

/// Variables systemd sets for the unit a program runs in; an application
/// started as a unit of its own gets its own.
const UNIT_VARIABLES: [&str; 21] = [
    "INVOCATION_ID",
    "JOURNAL_STREAM",
    "MANAGERPID",
    "MAINPID",
    "NOTIFY_SOCKET",
    "LISTEN_PID",
    "LISTEN_FDS",
    "LISTEN_FDNAMES",
    "SYSTEMD_EXEC_PID",
    "WATCHDOG_PID",
    "WATCHDOG_USEC",
    "MEMORY_PRESSURE_WATCH",
    "MEMORY_PRESSURE_WRITE",
    "RUNTIME_DIRECTORY",
    "STATE_DIRECTORY",
    "CACHE_DIRECTORY",
    "LOGS_DIRECTORY",
    "CONFIGURATION_DIRECTORY",
    "CREDENTIALS_DIRECTORY",
    "TRIGGER_UNIT",
    "TRIGGER_PATH",
];

/// systemd-run's arguments for `launch`: the caller's variables copied by
/// name (systemd-run takes a bare name's value from its own environment),
/// leaving out those systemd would refuse, which would stop the application
/// from starting at all. ExitType=cgroup keeps the service, and so the
/// application, running while any of its processes do, as a scope did, for
/// programs that hand over to a child and exit.
pub fn launch_args(program: &str, args: &[&str], env: &[(String, String)]) -> Vec<String> {
    let valid_name = |name: &str| {
        name.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let valid_value = |value: &str| {
        !value
            .chars()
            .any(|c| c.is_control() && c != '\t' && c != '\n')
    };
    let mut full: Vec<String> = [
        "--user",
        "--quiet",
        "--collect",
        "--same-dir",
        "--slice=app-graphical.slice",
        "--property=ExitType=cgroup",
    ]
    .map(str::to_owned)
    .to_vec();
    full.extend(
        env.iter()
            .filter(|(name, value)| {
                valid_name(name) && valid_value(value) && !UNIT_VARIABLES.contains(&name.as_str())
            })
            .map(|(name, _)| format!("--setenv={name}")),
    );
    full.push("--".to_owned());
    full.push(program.to_owned());
    full.extend(args.iter().map(|arg| (*arg).to_owned()));
    full
}

/// This helper, for commands that run it again.
pub fn me() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
}

/// Feed `input` to a program's stdin and return its stdout.
pub fn filter(program: &str, args: &[&str], input: &[u8]) -> Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A picker may exit before reading everything; that is not an error.
        let _ = stdin.write_all(input);
    }
    let out = child
        .wait_with_output()
        .map_err(|error| format!("{program}: {error}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A low-urgency notification. Omarchy draws the glyph in the icon slot;
/// Mako has none, so the glyph leads the summary.
pub fn notify(glyph: &str, summary: &str, body: &str) {
    let summary = if glyph.is_empty() {
        summary.to_owned()
    } else {
        format!("{glyph}    {summary}")
    };
    let mut args = vec!["-u", "low", summary.as_str()];
    if !body.is_empty() {
        args.push(body);
    }
    run("notify-send", &args);
}

/// The first executable named `name` on PATH.
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| std::path::Path::new(dir).join(name))
        .find(|path| {
            std::fs::metadata(path)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

pub fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

/// $XDG_STATE_HOME/tatami; an unset or relative variable means ~/.local/state.
pub fn state_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|dir| dir.starts_with('/'))
        .unwrap_or_else(|| format!("{}/.local/state", home()));
    std::path::Path::new(&base).join("tatami")
}

/// Processes whose command name matches, from /proc.
pub fn pids_named(names: &[&str]) -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .is_ok_and(|comm| names.contains(&comm.trim()))
        })
        // An exited child not yet reaped still has its name: not running.
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !is_zombie(&stat))
        })
        .collect()
}

/// The state in /proc/PID/stat, after the parenthesized name (which may
/// itself contain spaces and parentheses).
fn is_zombie(stat: &str) -> bool {
    stat.rsplit_once(") ")
        .is_some_and(|(_, rest)| rest.starts_with('Z'))
}

pub fn terminate(pids: &[i32]) {
    for pid in pids {
        unsafe {
            libc::kill(*pid, libc::SIGTERM);
        }
    }
}

/// Ask Waybar to refresh a custom module bound to `signal` (SIGRTMIN+n).
pub fn signal_waybar(offset: i32) {
    for pid in pids_named(&["waybar", ".waybar-wrapped"]) {
        unsafe {
            libc::kill(pid, libc::SIGRTMIN() + offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn applications_start_as_services_with_the_callers_environment() {
        let env = [
            ("XDG_CONFIG_DIRS", "/etc/xdg/tatami:/etc/xdg"),
            ("ELECTRON_OZONE_PLATFORM_HINT", "wayland"),
            ("INVOCATION_ID", "0123"),
            ("BASH_FUNC_x%%", "() { :; }"),
            ("BAD", "a\u{1b}b"),
            ("MULTI", "a\nb"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned()));
        let args = launch_args("chatgpt", &["--new-window"], &env);
        assert!(!args.contains(&"--scope".to_owned()));
        assert!(args.contains(&"--property=ExitType=cgroup".to_owned()));
        assert!(args.contains(&"--same-dir".to_owned()));
        let set: Vec<&str> = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--setenv="))
            .collect();
        assert_eq!(
            set,
            ["XDG_CONFIG_DIRS", "ELECTRON_OZONE_PLATFORM_HINT", "MULTI"]
        );
        assert_eq!(args[args.len() - 3..], ["--", "chatgpt", "--new-window"]);
    }
    #[test]
    fn zombies_are_not_running() {
        assert!(is_zombie("1479 (gpu-screen-reco) Z 1468 1468"));
        assert!(!is_zombie("1468 (slurp) S 1 1468"));
        assert!(!is_zombie("7 (a) Z) R 1 7"));
    }
}
