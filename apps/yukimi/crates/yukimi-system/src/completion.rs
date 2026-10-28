// SPDX-License-Identifier: MIT OR Apache-2.0
//! Update completion is a property of the running machine, not a source revision.
//! Reading this state only inspects files, services and read-only desktop IPC.
mod aizome;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const RECEIPT: &str = "/var/lib/yukimi/update.json";
pub const SESSION_MANIFEST: &str = "etc/xdg/tatami/update.json";
pub const SESSION_RECEIPT: &str = "tatami/update-session.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    None,
    Reload,
    Logout,
    Reboot,
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub action: Action,
    pub reasons: Vec<String>,
}

impl Completion {
    pub fn new(action: Action, reason: impl Into<String>) -> Self {
        Self { action, reasons: vec![reason.into()] }
    }
    pub fn title(&self) -> &'static str {
        match self.action {
            Action::None => "Update applied",
            Action::Reload => "Reload Aizome to finish the desktop update",
            Action::Logout => "Sign out to finish the desktop update",
            Action::Reboot => "Restart to finish updating",
            Action::Unknown => "Update needs attention",
        }
    }
    pub fn description(&self) -> String {
        match self.action {
            Action::None => format!("System changes are active. No computer restart is needed. Open applications can keep running until you close them. {}", self.reasons.join(" ")).trim_end().into(),
            Action::Reload => format!("{} Choose Reload Aizome in the command palette (Ctrl+Super+R by default), then refresh Yukimi to check again. No sign-out or computer restart is needed.", self.reasons.join(" ")),
            Action::Reboot => format!("{} Save your work, then restart when you are ready. Yukimi can be closed.", self.reasons.join(" ")),
            Action::Logout => format!("{} Save your work before signing out. No computer restart is needed.", self.reasons.join(" ")),
            _ => self.reasons.join(" "),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub system: PathBuf,
    pub boot_id: String,
    /// Written before activation; a failed/interrupted activation never looks ready.
    pub state: String,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionManifest {
    pub version: u32,
    #[serde(default)]
    pub boot_id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub session_invocation: String,
    pub session: BTreeMap<String, String>,
    pub reload: BTreeMap<String, String>,
    pub keep_awake: String,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn boot_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap_or_default().trim().to_owned()
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Compare actual boot artifacts, not release labels or whole generations.
pub fn boot_changes(booted: &Path, target: &Path) -> Result<Vec<String>, String> {
    let mut reasons = Vec::new();
    for (part, reason) in [
        ("kernel", "The kernel changed."),
        ("initrd", "The boot environment changed."),
        ("kernel-modules", "Kernel drivers changed."),
    ] {
        let a =
            std::fs::canonicalize(booted.join(part)).map_err(|_| format!("Could not verify the running {part}."))?;
        let b =
            std::fs::canonicalize(target.join(part)).map_err(|_| format!("Could not verify the installed {part}."))?;
        if a != b {
            reasons.push(reason.into());
        }
    }
    let params = |system: &Path| {
        std::fs::read_to_string(system.join("kernel-params"))
            .map(|s| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
            .map_err(|_| "Could not verify boot settings.".to_owned())
    };
    if params(booted)? != params(target)? {
        reasons.push("Kernel boot settings changed.".into());
    }
    Ok(reasons)
}

/// Reasons to prepare one complete system for boot instead of disrupting the
/// current session. Unknown activation compatibility is never claimed safe.
pub fn plan(booted: &Path, active: &Path, target: &Path) -> Vec<String> {
    let mut reasons = match boot_changes(booted, target) {
        Ok(reasons) => reasons,
        Err(reason) => vec![reason],
    };
    if active == target {
        return reasons;
    }
    for part in ["init-interface-version", "etc/yukimi/activation.json"] {
        match (std::fs::read(active.join(part)), std::fs::read(target.join(part))) {
            (Ok(a), Ok(b)) if a == b => (),
            _ => reasons.push(if part == "init-interface-version" {
                "System startup compatibility changed.".into()
            } else {
                "This system transition needs a fresh start to apply safely.".into()
            }),
        }
    }
    // A display manager can be supplied by a package under an alias rather
    // than config.systemd.services.display-manager. Inspect the effective
    // units and their drop-ins as well as the declarative manifest.
    let mut sessions_only = false;
    for (scope, units) in [
        (
            "system",
            &[
                "display-manager.service",
                "plasmalogin.service",
                "greetd.service",
                "systemd-logind.service",
                "dbus.service",
                "dbus-broker.service",
            ][..],
        ),
        (
            "user",
            &[
                "wayland-wm@.service",
                "wayland-wm@tatami.service",
                "wayland-session@.target",
                "wayland-session-envelope@.target",
                "graphical-session.target",
            ][..],
        ),
    ] {
        for unit in units {
            let relative = format!("etc/systemd/{scope}/{unit}");
            match (unit_definition(&active.join(&relative)), unit_definition(&target.join(&relative))) {
                (Ok(old), Ok(new)) if old == new || dbus_reload_only(unit, &old, &new) => {}
                (Ok(old), Ok(new)) if display_manager_sessions_only(unit, &old, &new) => sessions_only = true,
                _ => reasons.push(format!("A session-critical service changed ({unit}).")),
            }
        }
    }
    // The running display manager may have read either system's entries.
    if sessions_only && let Some(reason) = outdated_sessions(&[booted, active], target) {
        reasons.push(reason);
    }
    let restrictions = |system: &Path| -> Result<BTreeMap<String, String>, String> {
        let path = system.join("switch-inhibitors");
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        read_json(&path)
    };
    match (restrictions(active), restrictions(target)) {
        (Ok(old), Ok(new)) => {
            for (key, value) in &old {
                if new.get(key).is_some_and(|new| new != value) {
                    reasons.push(format!("A core system component changed ({key})."));
                }
            }
        }
        _ => reasons.push("Live activation restrictions could not be verified.".into()),
    }
    reasons
}

fn unit_definition(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, std::io::Error> {
    let mut result = BTreeMap::new();
    match std::fs::read(path) {
        Ok(contents) => {
            result.insert("unit".into(), contents);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && std::fs::symlink_metadata(path).is_err() => (),
        Err(e) => return Err(e),
    }
    let dropins = path.with_file_name(format!("{}.d", path.file_name().unwrap().to_string_lossy()));
    match std::fs::read_dir(dropins) {
        Ok(files) => {
            for file in files {
                let file = file?;
                if file.path().extension().is_some_and(|ext| ext == "conf") {
                    result.insert(file.file_name().to_string_lossy().into_owned(), std::fs::read(file.path())?);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    Ok(result)
}

/// NixOS reloads D-Bus when its policy/service catalog changes. Those generated
/// restart-trigger paths vary even for an ordinary application installation.
/// Only ignore that trigger: executable, environment, dependencies, and reload
/// policy changes must still require a fresh start.
fn dbus_reload_only(unit: &str, old: &BTreeMap<String, Vec<u8>>, new: &BTreeMap<String, Vec<u8>>) -> bool {
    if !matches!(unit, "dbus.service" | "dbus-broker.service") {
        return false;
    }
    let without_trigger = |files: &BTreeMap<String, Vec<u8>>| -> Option<BTreeMap<String, String>> {
        let mut reload = false;
        let mut result = BTreeMap::new();
        for (name, bytes) in in_order(files)? {
            let mut section = "";
            let mut contents = String::new();
            for line in std::str::from_utf8(bytes).ok()?.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    section = trimmed;
                }
                if let Some((key, value)) = trimmed.split_once('=') {
                    if section == "[Service]" && key.trim() == "X-ReloadIfChanged" {
                        reload = value.trim() == "true";
                    }
                    if section == "[Unit]" && key.trim() == "X-Restart-Triggers" {
                        continue;
                    }
                }
                contents.push_str(line);
                contents.push('\n');
            }
            result.insert(name.clone(), contents);
        }
        reload.then_some(result)
    };
    matches!((without_trigger(old), without_trigger(new)), (Some(a), Some(b)) if a == b)
}

const DISPLAY_MANAGERS: [&str; 3] = ["display-manager.service", "plasmalogin.service", "greetd.service"];

/// NixOS never restarts a display manager whose unit says
/// X-RestartIfChanged=false; it uses the new definition from its next start.
/// GDM, LightDM and Plasma Login Manager find login sessions through their
/// XDG_DATA_DIRS, so their unit changes whenever a session entry does. Only
/// ignore directories that hold nothing but session entries: the executable,
/// the rest of the environment, dependencies, drop-ins and the restart policy
/// must still match. The entries themselves are compared by plan, through
/// outdated_sessions.
fn display_manager_sessions_only(unit: &str, old: &BTreeMap<String, Vec<u8>>, new: &BTreeMap<String, Vec<u8>>) -> bool {
    if !DISPLAY_MANAGERS.contains(&unit) {
        return false;
    }
    let without_sessions = |files: &BTreeMap<String, Vec<u8>>| -> Option<BTreeMap<String, String>> {
        let mut restart = true;
        let mut result = BTreeMap::new();
        for (name, bytes) in in_order(files)? {
            let mut section = "";
            let mut contents = String::new();
            for line in std::str::from_utf8(bytes).ok()?.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    section = trimmed;
                }
                if let Some((key, value)) = trimmed.split_once('=')
                    && section == "[Service]"
                {
                    if key.trim() == "X-RestartIfChanged" {
                        restart = !matches!(value.trim(), "false" | "no" | "off" | "0");
                    }
                    if key.trim() == "Environment"
                        && let Some(dirs) = data_dirs(value)
                    {
                        let dirs: Vec<_> = dirs
                            .iter()
                            .map(|dir| if session_entries(dir) { "(sessions)" } else { dir.as_str() })
                            .collect();
                        contents.push_str(&format!("Environment=XDG_DATA_DIRS={}\n", dirs.join(":")));
                        continue;
                    }
                }
                contents.push_str(line);
                contents.push('\n');
            }
            result.insert(name.clone(), contents);
        }
        (!restart).then_some(result)
    };
    matches!((without_sessions(old), without_sessions(new)), (Some(a), Some(b)) if a == b)
}

/// systemd reads the base unit first, then drop-ins in lexical order.
fn in_order(files: &BTreeMap<String, Vec<u8>>) -> Option<impl Iterator<Item = (&String, &Vec<u8>)>> {
    let base = files.get_key_value("unit")?;
    Some(std::iter::once(base).chain(files.iter().filter(|(name, _)| *name != "unit")))
}

/// The directories of an `Environment=` assignment of XDG_DATA_DIRS alone,
/// as NixOS writes it. Anything else is compared as written.
fn data_dirs(value: &str) -> Option<Vec<String>> {
    let value = value.trim();
    let value = value.strip_prefix('"').and_then(|value| value.strip_suffix('"')).unwrap_or(value);
    let dirs = value.strip_prefix("XDG_DATA_DIRS=")?;
    (!dirs.contains(['"', '\'', '\\', '%']) && !dirs.contains(char::is_whitespace))
        .then(|| dirs.split(':').map(str::to_owned).collect())
}

/// A data directory with nothing but login session entries, such as NixOS's
/// collected `desktops` or Genkan's list of chosen sessions.
fn session_entries(dir: &str) -> bool {
    if !Path::new(dir).is_absolute() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut found = false;
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        if !matches!(entry.file_name().to_str(), Some("wayland-sessions" | "xsessions")) || !entry.path().is_dir() {
            return false;
        }
        found = true;
    }
    found
}

/// The login sessions a display manager offers, by entry and contents. It
/// reads them when it starts, so across live switches the running one keeps
/// offering the booted system's, or the active one's if it has restarted.
fn login_sessions(system: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut sessions = BTreeMap::new();
    for unit in DISPLAY_MANAGERS {
        let Ok(files) = unit_definition(&system.join("etc/systemd/system").join(unit)) else {
            continue;
        };
        // A later assignment replaces an earlier one.
        let mut dirs = Vec::new();
        for (_, bytes) in in_order(&files).into_iter().flatten() {
            for line in String::from_utf8_lossy(bytes).lines() {
                if let Some((key, value)) = line.trim().split_once('=')
                    && key.trim() == "Environment"
                    && let Some(value) = data_dirs(value)
                {
                    dirs = value;
                }
            }
        }
        for dir in dirs.iter().filter(|dir| session_entries(dir)) {
            for kind in ["wayland-sessions", "xsessions"] {
                for entry in std::fs::read_dir(Path::new(dir).join(kind)).into_iter().flatten().flatten() {
                    if let Ok(contents) = std::fs::read(entry.path()) {
                        sessions.insert(format!("{kind}/{}", entry.file_name().to_string_lossy()), contents);
                    }
                }
            }
        }
    }
    sessions
}

/// Signing in runs what the login screen's entry names, often by store path
/// (startplasma-wayland, Hyprland, an X session script). An entry the target
/// changes, among those a running display manager read from `started`, would
/// start the previous version of its desktop until the computer restarts.
/// Entries only added or removed change the list, not what a login starts.
fn outdated_sessions(started: &[&Path], target: &Path) -> Option<String> {
    let offered = login_sessions(target);
    let mut names = BTreeSet::new();
    for system in started {
        for (entry, contents) in login_sessions(system) {
            if offered.get(&entry).is_some_and(|new| *new != contents) {
                names.insert(session_name(&entry, &contents));
            }
        }
    }
    (!names.is_empty()).then(|| {
        format!(
            "Until the computer restarts, the login screen would start the previous version of {}.",
            names.into_iter().collect::<Vec<_>>().join(", ")
        )
    })
}

/// An entry's Name, or its file name without the extension.
fn session_name(entry: &str, contents: &[u8]) -> String {
    String::from_utf8_lossy(contents)
        .lines()
        .find_map(|line| {
            line.trim().strip_prefix("Name=").map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned)
        })
        .unwrap_or_else(|| entry.rsplit('/').next().unwrap_or(entry).trim_end_matches(".desktop").to_owned())
}

/// UWSM intentionally does not put XDG_SESSION_ID in the shared application
/// activation environment. Verify its running compositor and login context
/// instead of interpreting an absent variable as a different session.
pub fn tatami_session_id(record: &SessionManifest) -> Result<String, String> {
    if record.boot_id != boot_id() || record.session_id.is_empty() {
        return Err("The desktop record does not identify a session in this boot.".into());
    }
    let output = std::process::Command::new("systemctl")
        .args([
            "--user",
            "show",
            "wayland-wm@tatami.service",
            "--property=ActiveState",
            "--property=InvocationID",
            "--property=EnvironmentFiles",
        ])
        .output()
        .map_err(|_| "Could not verify the running Tatami session.")?;
    if !output.status.success() {
        return Err("Could not verify the running Tatami session.".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let values: BTreeMap<_, _> = text.lines().filter_map(|line| line.split_once('=')).collect();
    if values.get("ActiveState") != Some(&"active") {
        return Err("Tatami's compositor session is not active.".into());
    }
    if !record.session_invocation.is_empty()
        && values.get("InvocationID").copied() != Some(record.session_invocation.as_str())
    {
        return Err("The recorded desktop belongs to an earlier compositor session.".into());
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("The session runtime directory is unavailable.")?;
    let context = PathBuf::from(runtime).join("uwsm/env_session.conf");
    // Only read the known file if the active unit actually consumes it.
    if !values
        .get("EnvironmentFiles")
        .is_some_and(|files| files.split_whitespace().any(|part| part == context.to_string_lossy()))
    {
        return Err("Tatami's login context could not be verified.".into());
    }
    let context = std::fs::read_to_string(context).map_err(|_| "Tatami's login context could not be read.")?;
    let id = context
        .lines()
        .find_map(|line| line.strip_prefix("XDG_SESSION_ID="))
        .map(|id| id.trim_matches(['\'', '"']))
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        .ok_or("Tatami's login context has no valid session ID.")?;
    let supplied = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    if id != record.session_id || (!supplied.is_empty() && supplied != id) {
        return Err(
            "The recorded desktop belongs to another session. This session's applied version could not be verified."
                .into(),
        );
    }
    Ok(id.into())
}

/// Live activation leaves the display manager running (see plan), with the
/// list of sessions it started with.
const LOGIN_SCREEN_LIST: &str = "The login screen updates its list of desktops at the next restart.";

pub fn system_state(
    booted: &Path,
    active: &Path,
    selected: &Path,
    receipt: Option<&Receipt>,
    this_boot: &str,
) -> Completion {
    // A successful boot into the prepared system supersedes every old request,
    // including errors recorded before that boot. No second reboot for it.
    if let Some(r) = receipt.filter(|r| r.state == "activating")
        && !(r.system == selected
            && booted == selected
            && active == selected
            && !this_boot.is_empty()
            && r.boot_id != this_boot)
    {
        return Completion::new(
            Action::Unknown,
            "Activation did not finish successfully. Review the update log before restarting.",
        );
    }
    if booted == selected && active == selected {
        return Completion::default();
    }
    if active != selected {
        if let Some(r) = receipt.filter(|r| r.system == selected && r.state == "prepared") {
            return Completion {
                action: Action::Reboot,
                reasons: if r.reasons.is_empty() {
                    vec!["The complete update is prepared. Save your work and restart whenever you are ready; Yukimi does not need to stay open.".into()]
                } else {
                    r.reasons.clone()
                },
            };
        }
        return Completion::new(
            Action::Unknown,
            "The selected system differs from the running system. Its boot preparation has not been verified.",
        );
    }
    match boot_changes(booted, selected) {
        // Yukimi's planner stages a changed entry; a switch made otherwise can still bring one.
        Ok(reasons) if reasons.is_empty() => match outdated_sessions(&[booted], active) {
            Some(reason) => Completion::new(Action::Reboot, reason),
            None if login_sessions(booted) != login_sessions(active) => {
                Completion::new(Action::None, LOGIN_SCREEN_LIST)
            }
            None => Completion::default(),
        },
        Ok(reasons) => Completion { action: Action::Reboot, reasons },
        Err(reason) => Completion::new(Action::Unknown, reason),
    }
}

pub fn session_state(old: &SessionManifest, new: &SessionManifest, this_boot: &str, awake: bool) -> Completion {
    if old.version != 1 || new.version != 1 || old.boot_id != this_boot || this_boot.is_empty() {
        return Completion::new(Action::Unknown, "The desktop's applied version could not be verified.");
    }
    if let Some(error) = &old.error {
        return Completion::new(Action::Unknown, format!("Desktop changes could not be applied: {error}"));
    }
    let mut reasons = Vec::new();
    if old.session != new.session {
        reasons.push("Desktop startup components or session settings changed.".into());
    }
    if old.reload != new.reload {
        reasons.push("Some desktop configuration has not been reloaded.".into());
    }
    if awake && old.keep_awake != new.keep_awake {
        reasons.push("Keep Awake remains protected by its running version until you sign out.".into());
    }
    if reasons.is_empty() { Completion::default() } else { Completion { action: Action::Logout, reasons } }
}

pub fn in_tatami() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|s| s.split(':').any(|s| s.eq_ignore_ascii_case("tatami")))
}

fn in_aizome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|s| s.split(':').any(|s| s.eq_ignore_ascii_case("aizome")))
}

fn awake_command_matches(raw: &[u8], expected: &str) -> bool {
    let Some((tool, systemd)) = expected.split_once(':') else {
        return false;
    };
    let args: Vec<_> = raw.split(|b| *b == 0).collect();
    args.first() == Some(&format!("{systemd}/bin/systemd-inhibit").as_bytes())
        && args.contains(&format!("{tool}/bin/tatami").as_bytes())
}

/// Inspect the running invocation, not systemd's newly loaded ExecStart.
/// An automatic recovery or an off/on toggle may already have replaced an
/// old inhibitor, so its login-time fingerprint alone would be a false positive.
fn awake_state(expected: &str, previous: &str) -> Result<(bool, bool), String> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show", "tatami-keep-awake.service", "--property=ActiveState", "--property=MainPID"])
        .output()
        .map_err(|_| "Could not verify the running Keep Awake service.".to_owned())?;
    if !out.status.success() {
        return Err("Could not verify the running Keep Awake service.".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let values: BTreeMap<_, _> = text.lines().filter_map(|line| line.split_once('=')).collect();
    match values.get("ActiveState").copied() {
        Some("inactive" | "failed") => Ok((false, false)),
        Some("active") => {
            let pid = values
                .get("MainPID")
                .and_then(|pid| pid.parse::<u32>().ok())
                .filter(|pid| *pid > 0)
                .ok_or("Keep Awake is changing; check again shortly.")?;
            let raw = std::fs::read(format!("/proc/{pid}/cmdline"))
                .map_err(|_| "Could not verify the running Keep Awake version.")?;
            let current = awake_command_matches(&raw, expected);
            if !current && !awake_command_matches(&raw, previous) {
                return Err("The running Keep Awake command does not match its recorded version.".into());
            }
            Ok((true, current))
        }
        _ => Err("Keep Awake is changing; check again shortly.".into()),
    }
}

pub fn read() -> Completion {
    read_with_session(true)
}

pub fn read_system() -> Completion {
    read_with_session(false)
}

fn read_with_session(include_session: bool) -> Completion {
    let resolve = |path| {
        std::fs::canonicalize(path)
            .map_err(|_| Completion::new(Action::Unknown, "The installed and running system could not be verified."))
    };
    let (booted, active, selected) = match (
        resolve("/run/booted-system"),
        resolve("/run/current-system"),
        resolve("/nix/var/nix/profiles/system"),
    ) {
        (Ok(b), Ok(a), Ok(s)) => (b, a, s),
        _ => return Completion::new(Action::Unknown, "The installed and running system could not be verified."),
    };
    let receipt = read_json::<Receipt>(Path::new(RECEIPT));
    if Path::new(RECEIPT).exists() && receipt.is_err() {
        return Completion::new(
            Action::Unknown,
            "The update record could not be read. Completion has not been verified.",
        );
    }
    let system = system_state(&booted, &active, &selected, receipt.as_ref().ok(), &boot_id());
    with_session(system, include_session, || {
        if in_aizome() {
            aizome::read(&active)
        } else if in_tatami() {
            read_tatami(&active)
        } else {
            Completion::default()
        }
    })
}

fn with_session(system: Completion, include_session: bool, read: impl FnOnce() -> Completion) -> Completion {
    // Reboot and activation failure take precedence. In particular the
    // privileged helper's system-only check never enters session inspection.
    if system.action != Action::None || !include_session {
        return system;
    }
    let result = read();
    // A current session keeps the system's note about the login screen.
    if result == Completion::default() { system } else { result }
}

fn read_tatami(active: &Path) -> Completion {
    let session = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).map(|p| p.join(SESSION_RECEIPT));
    match (
        session.and_then(|p| read_json::<SessionManifest>(&p).ok()),
        read_json::<SessionManifest>(&active.join(SESSION_MANIFEST)),
    ) {
        (Some(mut old), Ok(new)) => {
            if let Err(reason) = tatami_session_id(&old) {
                return Completion::new(Action::Unknown, reason);
            }
            let (awake, current) = match awake_state(&new.keep_awake, &old.keep_awake) {
                Ok(state) => state,
                Err(error) => return Completion::new(Action::Unknown, error),
            };
            if current {
                old.keep_awake = new.keep_awake.clone();
            }
            session_state(&old, &new, &boot_id(), awake)
        }
        _ => Completion::new(
            Action::Unknown,
            "The desktop's applied version could not be verified. A new Tatami session records it automatically.",
        ),
    }
}

/// Called after a successful live switch, by the unprivileged frontend only.
/// It cannot reboot, sign out, or replace the compositor.
pub fn apply_desktop() {
    if in_tatami() {
        let record = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .and_then(|dir| read_json::<SessionManifest>(&dir.join(SESSION_RECEIPT)).ok());
        if let Some(id) = record.and_then(|record| tatami_session_id(&record).ok()) {
            let _ = std::process::Command::new("/run/current-system/sw/bin/tatami")
                .env("XDG_SESSION_ID", id)
                .args(["update-session"])
                .status();
        }
    }
}

pub fn finish_update(report: &mut serde_json::Value) {
    if report["ok"] != true || report.get("mode").is_none() {
        return;
    }
    if report["mode"] == "switch"
        && report["completion"]["action"] == "none"
        && read_system().action == Action::None
        && report["system"].as_str().is_some_and(|target| {
            std::fs::canonicalize("/run/current-system").ok().as_deref() == Some(Path::new(target))
        })
    {
        apply_desktop();
    }
    report["completion"] = serde_json::to_value(read()).unwrap_or_default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn system_only_and_incomplete_system_checks_never_inspect_the_session() {
        for action in [Action::None, Action::Reboot, Action::Unknown] {
            let system = Completion::new(action.clone(), "system status");
            assert_eq!(with_session(system.clone(), false, || panic!("privileged helper read session")), system);
            if action != Action::None {
                assert_eq!(with_session(system.clone(), true, || panic!("system advice was overridden")), system);
            }
        }
    }

    #[test]
    fn desktop_advice_replaces_the_all_clear_but_keeps_system_notes_when_current() {
        let system = Completion::new(Action::None, LOGIN_SCREEN_LIST);
        assert_eq!(with_session(system.clone(), true, Completion::default), system);
        for action in [Action::Reload, Action::Logout, Action::Unknown] {
            let desktop = Completion::new(action, "desktop status");
            assert_eq!(with_session(system.clone(), true, || desktop.clone()), desktop);
        }
    }
    struct Fixture {
        root: PathBuf,
        old: PathBuf,
        new: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "yukimi-completion-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let old = root.join("old");
            let new = root.join("new");
            std::fs::create_dir_all(root.join("artifacts")).unwrap();
            for system in [&old, &new] {
                std::fs::create_dir_all(system.join("etc/yukimi")).unwrap();
                for part in ["kernel", "initrd", "kernel-modules"] {
                    std::fs::write(root.join("artifacts").join(part), part).unwrap();
                    std::os::unix::fs::symlink(root.join("artifacts").join(part), system.join(part)).unwrap();
                }
                std::fs::write(system.join("kernel-params"), "quiet loglevel=4").unwrap();
                std::fs::write(system.join("init-interface-version"), "2").unwrap();
                std::fs::write(system.join("etc/yukimi/activation.json"), "{\"version\":1}").unwrap();
                std::fs::write(system.join("switch-inhibitors"), "{\"dbus\":\"broker\"}").unwrap();
            }
            Self { root, old, new }
        }
        fn receipt(&self, state: &str) -> Receipt {
            Receipt { system: self.new.clone(), state: state.into(), boot_id: "before".into(), reasons: vec![] }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn session() -> SessionManifest {
        SessionManifest {
            version: 1,
            boot_id: "boot".into(),
            session_id: String::new(),
            session_invocation: String::new(),
            session: BTreeMap::from([("compositor".into(), "same".into())]),
            reload: BTreeMap::from([("hyprland".into(), "same".into())]),
            keep_awake: "same".into(),
            error: None,
        }
    }
    #[test]
    fn generation_or_source_change_is_not_a_reboot_requirement() {
        let f = Fixture::new();
        assert!(plan(&f.old, &f.old, &f.new).is_empty());
        assert_eq!(system_state(&f.old, &f.new, &f.new, Some(&f.receipt("activated")), "before").action, Action::None);
        assert_eq!(session_state(&session(), &session(), "boot", true).action, Action::None);
    }
    #[test]
    fn staged_update_needs_one_reboot_even_with_identical_kernel() {
        let f = Fixture::new();
        let r = f.receipt("prepared");
        assert_eq!(system_state(&f.old, &f.old, &f.new, Some(&r), "before").action, Action::Reboot);
        assert_eq!(system_state(&f.new, &f.new, &f.new, Some(&r), "after").action, Action::None);
        // Reopening Yukimi again does not resurrect the request.
        assert_eq!(system_state(&f.new, &f.new, &f.new, Some(&r), "after").action, Action::None);
    }
    #[test]
    fn kernel_and_boot_settings_are_compared_to_what_booted() {
        let f = Fixture::new();
        std::fs::write(f.new.join("kernel-params"), "quiet loglevel=4 new-setting=1").unwrap();
        assert!(!plan(&f.old, &f.old, &f.new).is_empty());
        assert_eq!(system_state(&f.old, &f.new, &f.new, None, "boot").action, Action::Reboot);
    }
    #[test]
    fn critical_transitions_are_prepared_before_live_activation() {
        let f = Fixture::new();
        std::fs::write(f.new.join("switch-inhibitors"), "{\"dbus\":\"daemon\"}").unwrap();
        assert!(plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("dbus")));
    }
    #[test]
    fn display_manager_alias_and_dropins_are_checked_without_rebooting_for_unrelated_services() {
        let f = Fixture::new();
        for system in [&f.old, &f.new] {
            let units = system.join("etc/systemd/system");
            std::fs::create_dir_all(&units).unwrap();
            std::fs::write(units.join("plasmalogin.service"), "[Service]\nExecStart=/same/login\n").unwrap();
            std::os::unix::fs::symlink("plasmalogin.service", units.join("display-manager.service")).unwrap();
        }
        std::fs::write(f.new.join("etc/systemd/system/unrelated.service"), "[Service]\nExecStart=/new/app\n").unwrap();
        assert!(plan(&f.old, &f.old, &f.new).is_empty());
        let dropins = f.new.join("etc/systemd/system/display-manager.service.d");
        std::fs::create_dir_all(&dropins).unwrap();
        std::fs::write(dropins.join("override.conf"), "[Service]\nEnvironment=CHANGED=1\n").unwrap();
        assert!(plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("display-manager.service")));
        std::fs::remove_dir_all(dropins).unwrap();
        std::fs::write(f.new.join("etc/systemd/system/plasmalogin.service"), "[Service]\nExecStart=/new/login\n")
            .unwrap();
        assert!(plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("display-manager.service")));
    }
    #[test]
    fn dbus_catalog_reload_does_not_require_restart_but_binary_and_policy_changes_do() {
        let f = Fixture::new();
        for (system, trigger) in [(&f.old, "old-catalog"), (&f.new, "new-catalog")] {
            let dir = system.join("etc/systemd/system");
            std::fs::create_dir_all(dir.join("dbus-broker.service.d")).unwrap();
            std::fs::write(
                dir.join("dbus-broker.service"),
                "[Service]\nExecStart=/same/dbus-broker\nType=notify-reload\n",
            )
            .unwrap();
            std::fs::write(
                dir.join("dbus-broker.service.d/overrides.conf"),
                format!("[Unit]\nX-Restart-Triggers={trigger}\n[Service]\nX-ReloadIfChanged=true\n"),
            )
            .unwrap();
        }
        assert!(plan(&f.old, &f.old, &f.new).is_empty());
        let unit = f.new.join("etc/systemd/system/dbus-broker.service");
        let original = std::fs::read_to_string(&unit).unwrap();
        std::fs::write(&unit, original.replace("/same/", "/new/")).unwrap();
        assert!(plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("dbus-broker")));
        std::fs::write(unit, original).unwrap();
        std::fs::write(
            f.new.join("etc/systemd/system/dbus-broker.service.d/zzz-policy.conf"),
            "[Service]\nX-ReloadIfChanged=false\n",
        )
        .unwrap();
        assert!(plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("dbus-broker")));
    }
    /// A directory of login session entries, as NixOS collects them.
    fn sessions(root: &Path, name: &str, entries: &[(&str, &str)]) -> String {
        let dir = root.join(name).join("share");
        std::fs::create_dir_all(dir.join("xsessions")).unwrap();
        for (entry, contents) in entries {
            std::fs::create_dir_all(dir.join(entry).parent().unwrap()).unwrap();
            std::fs::write(dir.join(entry), contents).unwrap();
        }
        dir.to_string_lossy().into_owned()
    }
    const SERVICE: &str =
        "Environment=\"PATH=/same/bin\"\nX-RestartIfChanged=false\nExecStart=/same/gdm\nRestart=always\n";
    /// GDM's unit as NixOS writes it: sessions are one of its data directories.
    fn display_manager(system: &Path, data_dirs: &str, service: &str) {
        let units = system.join("etc/systemd/system");
        std::fs::create_dir_all(&units).unwrap();
        std::fs::write(
            units.join("display-manager.service"),
            format!("[Unit]\nAfter=systemd-user-sessions.service\n\n[Service]\nEnvironment=\"XDG_DATA_DIRS={data_dirs}\"\n{service}"),
        )
        .unwrap();
    }
    #[test]
    fn display_manager_session_list_change_stays_live_but_its_program_and_settings_do_not() {
        let f = Fixture::new();
        let gdm = f.root.join("gdm/share");
        std::fs::create_dir_all(gdm.join("gnome-session/sessions")).unwrap();
        let gdm = gdm.to_string_lossy();
        let tatami = ("wayland-sessions/tatami.desktop", "Exec=/run/current-system/sw/bin/tatami session\n");
        // The same entry, and another desktop beside it.
        let old = sessions(&f.root, "old-desktops", &[tatami]);
        let new = sessions(&f.root, "new-desktops", &[tatami, ("xsessions/xfce.desktop", "Exec=/new/xsession\n")]);
        display_manager(&f.old, &format!("{gdm}:{old}"), SERVICE);
        display_manager(&f.new, &format!("{gdm}:{new}"), SERVICE);
        assert!(plan(&f.old, &f.old, &f.new).is_empty());
        let restarts = |data_dirs: &str, service: &str| {
            display_manager(&f.new, data_dirs, service);
            plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("display-manager.service"))
        };
        let sessions_changed = format!("{gdm}:{new}");
        assert!(restarts(&sessions_changed, &SERVICE.replace("/same/gdm", "/new/gdm")));
        assert!(restarts(&sessions_changed, &SERVICE.replace("/same/bin", "/new/bin")));
        assert!(restarts(&sessions_changed, &SERVICE.replace("X-RestartIfChanged=false\n", "")));
        assert!(restarts(&sessions_changed, &format!("{SERVICE}Environment=\"GDM_X_SESSION_WRAPPER=/new/wrapper\"\n")));
        // Data directories with more than sessions in them, or another one.
        assert!(restarts(&format!("{gdm}:{gdm}"), SERVICE));
        assert!(restarts(&format!("{gdm}:{new}:{new}"), SERVICE));
        std::fs::create_dir_all(Path::new(&new).join("applications")).unwrap();
        assert!(restarts(&sessions_changed, SERVICE));
        std::fs::remove_dir(Path::new(&new).join("applications")).unwrap();
        assert!(!restarts(&sessions_changed, SERVICE));
        let dropins = f.new.join("etc/systemd/system/display-manager.service.d");
        std::fs::create_dir_all(&dropins).unwrap();
        std::fs::write(dropins.join("override.conf"), "[Service]\nEnvironment=CHANGED=1\n").unwrap();
        assert!(restarts(&sessions_changed, SERVICE));
    }
    #[test]
    fn a_changed_login_entry_waits_for_a_restart_and_names_its_desktop() {
        let f = Fixture::new();
        let plasma = |exec: &str| format!("[Desktop Entry]\nName=Plasma (Wayland)\nExec={exec}\n");
        let (old_plasma, new_plasma) = (plasma("/old/startplasma-wayland"), plasma("/new/startplasma-wayland"));
        let old = sessions(
            &f.root,
            "old-desktops",
            &[
                ("wayland-sessions/plasma.desktop", &old_plasma),
                ("wayland-sessions/hyprland.desktop", "Exec=/old/start-hyprland\n"),
            ],
        );
        let new = sessions(
            &f.root,
            "new-desktops",
            &[
                ("wayland-sessions/plasma.desktop", &new_plasma),
                ("wayland-sessions/hyprland.desktop", "Exec=/new/start-hyprland\n"),
            ],
        );
        display_manager(&f.old, &old, SERVICE);
        display_manager(&f.new, &new, SERVICE);
        let reason = "Until the computer restarts, the login screen would start the previous version of Plasma (Wayland), hyprland.";
        assert_eq!(plan(&f.old, &f.old, &f.new), vec![reason]);
        // A display manager that restarted after a live switch read the active system's.
        assert_eq!(plan(&f.new, &f.old, &f.new), vec![reason]);
    }
    #[test]
    fn plasma_login_session_list_in_its_dropin_stays_live_unless_it_would_restart() {
        let f = Fixture::new();
        for (system, list) in [(&f.old, "old-sessions"), (&f.new, "new-sessions")] {
            let dir = sessions(
                &f.root,
                list,
                &[("wayland-sessions/tatami.desktop", "Exec=/run/current-system/sw/bin/tatami session\n")],
            );
            let units = system.join("etc/systemd/system");
            std::fs::create_dir_all(units.join("plasmalogin.service.d")).unwrap();
            std::fs::write(units.join("plasmalogin.service"), "[Service]\nExecStart=/same/plasmalogin\n").unwrap();
            std::os::unix::fs::symlink("plasmalogin.service", units.join("display-manager.service")).unwrap();
            std::fs::write(
                units.join("plasmalogin.service.d/overrides.conf"),
                format!("[Unit]\n\n[Service]\nEnvironment=\"XDG_DATA_DIRS={dir}\"\nX-RestartIfChanged=false\n"),
            )
            .unwrap();
        }
        assert!(plan(&f.old, &f.old, &f.new).is_empty());
        // A later drop-in decides, for both systems alike.
        for system in [&f.old, &f.new] {
            std::fs::write(
                system.join("etc/systemd/system/plasmalogin.service.d/zz-restart.conf"),
                "[Service]\nX-RestartIfChanged=true\n",
            )
            .unwrap();
        }
        assert!(plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("plasmalogin.service")));
    }
    #[test]
    fn greetd_keeps_its_sessions_in_its_configuration_which_still_stages() {
        let f = Fixture::new();
        for (system, config) in [(&f.old, "/old/greetd.toml"), (&f.new, "/new/greetd.toml")] {
            let units = system.join("etc/systemd/system");
            std::fs::create_dir_all(&units).unwrap();
            std::fs::write(
                units.join("greetd.service"),
                format!("[Service]\nExecStart=/same/greetd --config {config}\nX-RestartIfChanged=false\n"),
            )
            .unwrap();
            std::os::unix::fs::symlink("greetd.service", units.join("display-manager.service")).unwrap();
        }
        assert!(plan(&f.old, &f.old, &f.new).iter().any(|r| r.contains("greetd.service")));
    }
    #[test]
    fn live_session_list_change_is_explained_until_restart() {
        let f = Fixture::new();
        let gnome = ("wayland-sessions/gnome.desktop", "Name=GNOME\nExec=/same/gnome-session\n");
        display_manager(&f.old, &sessions(&f.root, "old-desktops", &[gnome]), SERVICE);
        display_manager(&f.new, &sessions(&f.root, "same-desktops", &[gnome]), SERVICE);
        // Other store paths, the same entries.
        assert_eq!(
            system_state(&f.old, &f.new, &f.new, Some(&f.receipt("activated")), "before"),
            Completion::default()
        );
        let xfce = ("xsessions/xfce.desktop", "Name=Xfce Session\nExec=/new/xsession\n");
        display_manager(&f.new, &sessions(&f.root, "more-desktops", &[gnome, xfce]), SERVICE);
        let state = system_state(&f.old, &f.new, &f.new, Some(&f.receipt("activated")), "before");
        assert_eq!(state.action, Action::None);
        assert!(state.description().starts_with("System changes are active."));
        assert!(state.description().ends_with(LOGIN_SCREEN_LIST));
        // Starting the updated system takes it up.
        assert_eq!(system_state(&f.new, &f.new, &f.new, Some(&f.receipt("activated")), "after"), Completion::default());
        // A switch made outside Yukimi's planner can change an entry the login screen still starts.
        let gnome = ("wayland-sessions/gnome.desktop", "Name=GNOME\nExec=/new/gnome-session\n");
        display_manager(&f.new, &sessions(&f.root, "new-desktops", &[gnome]), SERVICE);
        let state = system_state(&f.old, &f.new, &f.new, Some(&f.receipt("activated")), "before");
        assert_eq!(state.action, Action::Reboot);
        assert!(
            state.description().starts_with(
                "Until the computer restarts, the login screen would start the previous version of GNOME."
            )
        );
    }
    #[test]
    fn missing_evidence_and_failed_activation_never_claim_success() {
        let f = Fixture::new();
        assert_eq!(
            system_state(&f.old, &f.old, &f.old, Some(&f.receipt("activating")), "before").action,
            Action::Unknown
        );
        assert_eq!(system_state(&f.old, &f.old, &f.new, None, "before").action, Action::Unknown);
        assert_eq!(
            system_state(&f.old, &f.new, &f.new, Some(&f.receipt("activating")), "before").action,
            Action::Unknown
        );
        std::fs::remove_file(f.old.join("kernel")).unwrap();
        assert_eq!(system_state(&f.old, &f.new, &f.new, None, "before").action, Action::Unknown);
        assert!(!plan(&f.old, &f.old, &f.new).is_empty());
    }
    #[test]
    fn booting_successfully_finishes_an_interrupted_receipt() {
        let f = Fixture::new();
        assert_eq!(system_state(&f.new, &f.new, &f.new, Some(&f.receipt("activating")), "after").action, Action::None);
        assert_eq!(
            system_state(&f.new, &f.new, &f.new, Some(&f.receipt("activating")), "before").action,
            Action::Unknown
        );
    }
    #[test]
    fn rollback_does_not_resurrect_an_unrelated_update_request() {
        let f = Fixture::new();
        assert_eq!(system_state(&f.old, &f.old, &f.old, Some(&f.receipt("prepared")), "boot").action, Action::None);
    }
    #[test]
    fn desktop_pending_changes_clear_at_login_without_reboot() {
        let old = session();
        let mut new = old.clone();
        new.session.insert("compositor".into(), "new".into());
        assert_eq!(session_state(&old, &new, "boot", false).action, Action::Logout);
        assert_eq!(session_state(&new, &new, "boot", false).action, Action::None);
    }
    #[test]
    fn inactive_inhibitor_and_already_applied_configuration_do_not_request_logout() {
        let old = session();
        let mut new = old.clone();
        new.keep_awake = "new".into();
        assert_eq!(session_state(&old, &new, "boot", false).action, Action::None);
        assert_eq!(session_state(&old, &new, "boot", true).action, Action::Logout);
    }
    #[test]
    fn inhibitor_version_comes_from_the_actual_process() {
        let raw =
            b"/nix/store/systemd/bin/systemd-inhibit\0--what=sleep:idle\0/nix/store/new/bin/tatami\0awake\0hold\0";
        assert!(awake_command_matches(raw, "/nix/store/new:/nix/store/systemd"));
        assert!(!awake_command_matches(raw, "/nix/store/old:/nix/store/systemd"));
        assert!(!awake_command_matches(b"", "/nix/store/new:/nix/store/systemd"));
    }
    #[test]
    fn unknown_or_failed_desktop_reload_is_not_reported_as_applied() {
        let mut old = session();
        assert_eq!(session_state(&old, &old, "different-boot", false).action, Action::Unknown);
        old.error = Some("reload failed".into());
        assert_eq!(session_state(&old, &session(), "boot", false).action, Action::Unknown);
    }
}
