// SPDX-License-Identifier: GPL-3.0-or-later
//! Keeps one desktop session's environment out of the next.
//!
//! Desktop sessions export their environment into the user's systemd manager
//! (`dbus-update-activation-environment`, `UpdateActivationEnvironment`), and
//! nothing removes it at logout. The manager outlives the session, so the
//! next desktop's services start with it: after an X11 desktop, Plasma ran
//! plasmashell with `QT_QPA_PLATFORM=xcb` and LXQt's Qt theme, without a panel.
//!
//! `save PATH` runs as the manager starts, before any session, and records
//! its environment. `reset` runs from the login screen's PAM session stack
//! before each session starts. As the user, it puts the recorded environment
//! back, unless another graphical session of the user is still open.
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

const BUSCTL: &str = "/run/current-system/systemd/bin/busctl";
const LOGINCTL: &str = "/run/current-system/systemd/bin/loginctl";
/// The record's name in the user's runtime directory, where `save` puts it.
const RECORD: &str = "calamares-session-env";

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["save", path] => save(Path::new(path)),
        ["reset"] => reset(),
        _ => Err("usage: calamares-session-env save PATH | reset".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("calamares-session-env: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Record the manager's environment. Runs as the user, from a user unit.
fn save(path: &Path) -> Result<()> {
    let environment = manager_environment(&[])?;
    let text = serde_json::to_string(&environment).map_err(|error| error.to_string())?;
    let partial = path.with_extension("partial");
    fs::write(&partial, text).map_err(|error| format!("{}: {error}", partial.display()))?;
    fs::rename(&partial, path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Put the recorded environment back before a session starts. Runs as root
/// from `pam_exec`, so it becomes the user before reading anything.
fn reset() -> Result<()> {
    if std::env::var("PAM_TYPE").as_deref() != Ok("open_session") {
        return Ok(());
    }
    let user = std::env::var("PAM_USER").map_err(|_| "PAM_USER is not set")?;
    // The session being opened; without it, other sessions cannot be told apart.
    let Ok(session) = std::env::var("XDG_SESSION_ID") else {
        return Ok(());
    };
    let uid = become_user(&user)?;
    let runtime = PathBuf::from(format!("/run/user/{uid}"));
    let Ok(text) = fs::read_to_string(runtime.join(RECORD)) else {
        // The manager has not recorded an environment: nothing to restore.
        return Ok(());
    };
    let recorded: Vec<String> = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if other_graphical_session(uid, &session)? {
        return Ok(());
    }
    let bus = [(
        "DBUS_SESSION_BUS_ADDRESS",
        format!("unix:path={}/bus", runtime.display()),
    )];
    let current = manager_environment(&bus)?;
    let (unset, set) = changes(&recorded, &current);
    if unset.is_empty() && set.is_empty() {
        return Ok(());
    }
    let mut args = vec![
        "--user".to_owned(),
        "--timeout=5".to_owned(),
        "call".to_owned(),
        "org.freedesktop.systemd1".to_owned(),
        "/org/freedesktop/systemd1".to_owned(),
        "org.freedesktop.systemd1.Manager".to_owned(),
        "UnsetAndSetEnvironment".to_owned(),
        "asas".to_owned(),
        unset.len().to_string(),
    ];
    args.extend(unset);
    args.push(set.len().to_string());
    args.extend(set);
    output(BUSCTL, &args, &bus).map(drop)
}

/// The manager's environment as `NAME=value` entries.
fn manager_environment(env: &[(&str, String)]) -> Result<Vec<String>> {
    let args = [
        "--user",
        "--timeout=5",
        "--json=short",
        "get-property",
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "Environment",
    ];
    parse_environment(&output(BUSCTL, &args, env)?)
}

fn parse_environment(json: &str) -> Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    value["data"]
        .as_array()
        .ok_or("busctl returned no environment")?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "non-string environment entry".to_owned())
        })
        .collect()
}

/// Names to unset and `NAME=value` entries to set so that `current` becomes
/// `recorded`.
fn changes(recorded: &[String], current: &[String]) -> (Vec<String>, Vec<String>) {
    let recorded = entries(recorded);
    let current = entries(current);
    let unset = current
        .keys()
        .filter(|name| !recorded.contains_key(*name))
        .map(|name| (*name).to_owned())
        .collect();
    let set = recorded
        .iter()
        .filter(|(name, value)| current.get(*name) != Some(value))
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    (unset, set)
}

fn entries(list: &[String]) -> BTreeMap<&str, &str> {
    list.iter()
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

/// Whether the user has a graphical session open besides `own`.
fn other_graphical_session(uid: u32, own: &str) -> Result<bool> {
    let uid = uid.to_string();
    let sessions = output(
        LOGINCTL,
        &["show-user", &uid, "-p", "Sessions", "--value"],
        &[],
    )?;
    for id in sessions.split_whitespace().filter(|id| *id != own) {
        let properties = [
            "show-session",
            id,
            "-p",
            "Type",
            "-p",
            "State",
            "-p",
            "Class",
        ];
        let shown = output(LOGINCTL, &properties, &[])?;
        if is_open_graphical(&shown) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `loginctl show-session` output describes an open graphical user session.
fn is_open_graphical(shown: &str) -> bool {
    let properties: BTreeMap<&str, &str> = shown
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    properties.get("Class") == Some(&"user")
        && matches!(properties.get("Type"), Some(&("x11" | "wayland" | "mir")))
        && matches!(properties.get("State"), Some(&("active" | "online")))
}

/// Drop root for `user`, irreversibly, so files and buses are reached as them.
fn become_user(user: &str) -> Result<u32> {
    let name = CString::new(user).map_err(|_| format!("invalid user name {user:?}"))?;
    // SAFETY: getpwnam returns null or a pointer to static storage, which is
    // read at once; this program has a single thread.
    let (uid, gid) = unsafe {
        let entry = libc::getpwnam(name.as_ptr());
        if entry.is_null() {
            return Err(format!("unknown user {user}"));
        }
        ((*entry).pw_uid, (*entry).pw_gid)
    };
    // SAFETY: plain system calls without pointers to live data; groups and
    // the group ID are dropped before the user ID, which ends privilege.
    unsafe {
        if libc::geteuid() == uid {
            return Ok(uid);
        }
        if libc::setgroups(0, std::ptr::null()) != 0
            || libc::setgid(gid) != 0
            || libc::setuid(uid) != 0
        {
            return Err(format!(
                "cannot become {user}: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(uid)
}

/// Run a program without a shell and return its stdout.
fn output<S: AsRef<std::ffi::OsStr>>(
    program: &str,
    args: &[S],
    env: &[(&str, String)],
) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .envs(env.iter().map(|(name, value)| (*name, value)))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{program} exited with {}", out.status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    #[test]
    fn exported_variables_are_unset_and_changed_ones_restored() {
        let recorded = list(&["HOME=/home/a", "PATH=/run/wrappers/bin", "LANG=en_US.UTF-8"]);
        let current = list(&[
            "HOME=/home/a",
            "PATH=/nix/store/x/bin",
            "LANG=en_US.UTF-8",
            "QT_QPA_PLATFORM=xcb",
            "QT_QPA_PLATFORMTHEME=lxqt",
        ]);
        let (unset, set) = changes(&recorded, &current);
        assert_eq!(unset, ["QT_QPA_PLATFORM", "QT_QPA_PLATFORMTHEME"]);
        assert_eq!(set, ["PATH=/run/wrappers/bin"]);
    }

    #[test]
    fn removed_variables_come_back_and_equal_ones_are_left_alone() {
        let recorded = list(&["HOME=/home/a", "XDG_RUNTIME_DIR=/run/user/1000"]);
        let current = list(&["HOME=/home/a"]);
        assert_eq!(
            changes(&recorded, &current),
            (vec![], list(&["XDG_RUNTIME_DIR=/run/user/1000"]))
        );
        assert_eq!(changes(&recorded, &recorded), (vec![], vec![]));
    }

    #[test]
    fn values_may_contain_equals_signs_and_malformed_entries_are_ignored() {
        let recorded = list(&["OPTIONS=a=b"]);
        let current = list(&["OPTIONS=a=c", "noequals", "=empty"]);
        assert_eq!(
            changes(&recorded, &current),
            (vec![], list(&["OPTIONS=a=b"]))
        );
    }

    #[test]
    fn busctl_json_is_parsed() {
        let json = r#"{"type":"as","data":["HOME=/home/a","EMPTY=","NL=a\nb"]}"#;
        assert_eq!(
            parse_environment(json).unwrap(),
            ["HOME=/home/a", "EMPTY=", "NL=a\nb"]
        );
        assert!(parse_environment(r#"{"type":"s","data":"x"}"#).is_err());
    }

    #[test]
    fn only_open_graphical_user_sessions_count() {
        assert!(is_open_graphical(
            "Type=wayland\nState=active\nClass=user\n"
        ));
        assert!(is_open_graphical("Type=x11\nState=online\nClass=user\n"));
        assert!(!is_open_graphical(
            "Type=wayland\nState=closing\nClass=user\n"
        ));
        assert!(!is_open_graphical("Type=tty\nState=active\nClass=user\n"));
        assert!(!is_open_graphical(
            "Type=unspecified\nState=active\nClass=manager\n"
        ));
    }
}
