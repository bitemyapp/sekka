// SPDX-License-Identifier: MIT OR Apache-2.0
//! Update completion is a property of the running machine, not a source revision.
//! No commands or mutations are performed while reading this state.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const RECEIPT: &str = "/var/lib/yukimi/update.json";
pub const SESSION_MANIFEST: &str = "etc/xdg/tatami/update.json";
pub const SESSION_RECEIPT: &str = "tatami/update-session.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    None,
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
            Action::Logout => "Sign out to finish the desktop update",
            Action::Reboot => "Restart to finish updating",
            Action::Unknown => "Update needs attention",
        }
    }
    pub fn description(&self) -> String {
        match self.action {
            Action::None => "System changes are active. No computer restart is needed. Open applications can keep running until you close them.".into(),
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
        Ok(reasons) if reasons.is_empty() => Completion::default(),
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
    let mut result = system_state(&booted, &active, &selected, receipt.as_ref().ok(), &boot_id());
    // Reboot and activation failure take precedence over all session advice.
    if result.action != Action::None || !include_session || !in_tatami() {
        return result;
    }
    let session = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).map(|p| p.join(SESSION_RECEIPT));
    match (
        session.and_then(|p| read_json::<SessionManifest>(&p).ok()),
        read_json::<SessionManifest>(&active.join(SESSION_MANIFEST)),
    ) {
        (Some(mut old), Ok(new)) => {
            if old.session_id != std::env::var("XDG_SESSION_ID").unwrap_or_default() {
                return Completion::new(
                    Action::Unknown,
                    "The recorded desktop belongs to another session. This session's applied version could not be verified.",
                );
            }
            let (awake, current) = match awake_state(&new.keep_awake, &old.keep_awake) {
                Ok(state) => state,
                Err(error) => return Completion::new(Action::Unknown, error),
            };
            if current {
                old.keep_awake = new.keep_awake.clone();
            }
            result = session_state(&old, &new, &boot_id(), awake);
        }
        _ => {
            result = Completion::new(
                Action::Unknown,
                "The desktop's applied version could not be verified. A new Tatami session records it automatically.",
            )
        }
    }
    result
}

/// Called after a successful live switch, by the unprivileged frontend only.
/// It cannot reboot, sign out, or replace the compositor.
pub fn apply_desktop() {
    if in_tatami() {
        let _ = std::process::Command::new("/run/current-system/sw/bin/tatami").args(["update-session"]).status();
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
