//! Unprivileged inspection of the live Aizome session. The helper's
//! `read_system` never connects to a user's compositor or follows their PATH.
use super::{Action, Completion};
use aizome_ipc::{Client, ReloadReport, Request, Response};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

type Environment = BTreeMap<String, String>;
const ENVIRONMENT: &str = "etc/xdg/aizome/environment";
const ENVIRONMENT_KEYS: &str = "AIZOME_ENVIRONMENT_KEYS";
const ENVIRONMENT_FILE: &str = "AIZOME_ENVIRONMENT_FILE";

struct Runtime {
    core: PathBuf,
    standard_library: PathBuf,
}

struct Installed {
    compositor: PathBuf,
    shell: PathBuf,
    abi: String,
}

struct Running {
    compositor: PathBuf,
    abi: String,
    environment: Environment,
    cwd: PathBuf,
    logic_healthy: bool,
    renderer_after_restart: bool,
    reload: ReloadReport,
    runtime: Result<Runtime, String>,
}

pub(super) fn read(active: &Path) -> Completion {
    inspect(active).unwrap_or_else(|reason| Completion::new(Action::Unknown, reason))
}

fn inspect(active: &Path) -> Result<Completion, String> {
    // Always compare with the activated system. `aizome` on the user's PATH
    // can be a development build or an older profile entry.
    let compositor = std::fs::canonicalize(active.join("sw/bin/aizome"))
        .map_err(|_| "The active system's Aizome compositor could not be verified.")?;
    let shell = std::fs::canonicalize(active.join("sw/bin/aizome-shell"))
        .map_err(|_| "The active system's Aizome shell could not be verified.")?;
    let package = compositor.parent().and_then(Path::parent).ok_or("Aizome's package could not be located.")?;
    if !package.join("lib/aizome/libaizome_comp_logic.so").is_file() {
        return Err("The active system's Aizome logic library could not be found.".into());
    }
    let build = super::read_json::<serde_json::Value>(&package.join("share/aizome/build-env.json"))
        .map_err(|_| "The installed Aizome core's build information could not be read.")?;
    let abi = abi(&build["core"])?;
    let installed = Installed { compositor, shell, abi };
    let socket = aizome_ipc::socket_path().ok_or("The running Aizome session could not be located.")?;
    let running = snapshot(&socket, Path::new("/proc"), Duration::from_secs(2))?;
    let current = environment_file(&active.join(ENVIRONMENT), true)?;
    let environment = session_environment_changes(&current, &running.environment);
    let result = assess_session(&installed, &running, environment);
    // Include the runtime file reads above: a concurrent switch must not
    // turn evidence about the previous generation into an all-clear.
    if std::fs::canonicalize("/run/current-system").ok().as_deref() != Some(active) {
        return Err("The active system changed during this check. Refresh Yukimi to check again.".into());
    }
    Ok(result)
}

fn assess_session(installed: &Installed, running: &Running, environment: Result<Vec<String>, String>) -> Completion {
    let mut result = assess(installed, running, environment.as_deref().unwrap_or_default());
    if let Err(reason) = environment {
        // A known core change still needs a login, which also records the
        // environment. Missing evidence must never certify None or Reload.
        if matches!(result.action, Action::None | Action::Reload) {
            result.action = Action::Unknown;
        }
        result.reasons.push(reason);
    }
    result
}

fn abi(build: &serde_json::Value) -> Result<String, String> {
    build["abi"]
        .as_str()
        .filter(|abi| abi.len() == 64 && abi.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or_else(|| "The Aizome core's build fingerprint could not be verified.".into())
}

fn snapshot(socket: &Path, proc: &Path, timeout: Duration) -> Result<Running, String> {
    let failed = || {
        "The running Aizome desktop could not be checked. Refresh Yukimi to try again; desktop update completion is unverified.".to_owned()
    };
    let mut client = Client::connect_to_timeout(socket, timeout).map_err(|_| failed())?;
    client.stream().set_read_timeout(Some(timeout)).map_err(|_| failed())?;
    client.stream().set_write_timeout(Some(timeout)).map_err(|_| failed())?;
    let Response::Version { pid, protocol, exe: Some(exe), build: Some(build), .. } =
        client.request(&Request::Version).map_err(|_| failed())?
    else {
        return Err(failed());
    };
    if pid == 0 || protocol != aizome_ipc::PROTOCOL_VERSION {
        return Err(failed());
    }
    let process = proc.join(pid.to_string());
    let compositor = std::fs::read_link(process.join("exe")).map_err(|_| failed())?;
    if compositor != Path::new(&exe) {
        return Err(failed());
    }
    let abi = abi(&build)?;
    // This is the compositor's environment, not Yukimi's (which may have
    // been launched from a terminal with its own PATH and shell overrides).
    let raw = std::fs::read(process.join("environ")).map_err(|_| failed())?;
    let environment: Environment = String::from_utf8(raw)
        .map_err(|_| failed())?
        .split('\0')
        .filter_map(|entry| entry.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    let cwd = std::fs::read_link(process.join("cwd")).map_err(|_| failed())?;
    let Response::Stats { stats } = client.request(&Request::Stats { reset: false }).map_err(|_| failed())? else {
        return Err(failed());
    };
    if stats.logic.core_abi != abi {
        return Err(failed());
    }
    let Response::Reload { report } = client
        .request(&Request::Reload { logic: false, shell: false, from: None, shell_from: None, check: true })
        .map_err(|_| failed())?
    else {
        return Err(failed());
    };
    let runtime = if ["LD_LIBRARY_PATH", "LD_PRELOAD", "LD_AUDIT"]
        .iter()
        .any(|key| environment.get(*key).is_some_and(|value: &String| !value.is_empty()))
    {
        Err("Aizome's runtime library selection is overridden. Check LD_LIBRARY_PATH, LD_PRELOAD and LD_AUDIT before relying on the packaged runtime check.".into())
    } else {
        std::fs::read_to_string(process.join("maps"))
            .map_err(|_| "The running Aizome core's libraries could not be inspected.".into())
            .and_then(|maps| mapped_runtime(&maps))
    };
    Ok(Running {
        compositor,
        abi,
        environment,
        cwd,
        logic_healthy: stats.logic.healthy,
        renderer_after_restart: stats.renderer_after_restart,
        reload: report,
        runtime,
    })
}

fn assess(installed: &Installed, running: &Running, environment_changed: &[String]) -> Completion {
    let mut result = Completion::default();
    if installed.abi != running.abi {
        result.action = Action::Logout;
        result.reasons.push("The installed Aizome compositor core differs from the one running. Sign out and back in to use it; restarting only the shell does not replace the compositor.".into());
    } else {
        match running
            .runtime
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|runtime| runtime_matches(&installed.compositor, &running.compositor, runtime))
        {
            Ok(true) => (),
            Ok(false) => {
                result.action = Action::Logout;
                result.reasons.push("Aizome's compositor or its libraries changed. Sign out and back in to use the update; reloading the logic or shell keeps the old compositor running.".into());
            }
            Err(reason) => {
                result.action = Action::Unknown;
                result.reasons.push(reason);
            }
        }
    }
    if !environment_changed.is_empty() {
        if result.action != Action::Unknown {
            result.action = Action::Logout;
        }
        result.reasons.push(format!("Aizome's startup environment changed ({}). A new login is needed to apply those settings; a shell reload keeps the session's old environment.", environment_changed.join(", ")));
    }
    if running.renderer_after_restart {
        if result.action != Action::Unknown {
            result.action = Action::Logout;
        }
        result.reasons.push("Aizome's configured renderer takes effect at the next login.".into());
    }
    let Some(path) = running.environment.get("PATH") else {
        result.action = Action::Unknown;
        result
            .reasons
            .insert(0, "The session's PATH is unavailable, so its Aizome launch paths could not be verified.".into());
        return result;
    };
    for (name, expected, selected) in [
        ("aizome", &installed.compositor, program("aizome", path, &running.cwd)),
        (
            "aizome-shell",
            &installed.shell,
            program(
                running
                    .environment
                    .get("AIZOME_SHELL_BIN")
                    .filter(|value| !value.is_empty())
                    .map(String::as_str)
                    .unwrap_or("aizome-shell"),
                path,
                &running.cwd,
            ),
        ),
    ] {
        let configuration = if name == "aizome" { "profile and PATH" } else { "profile, PATH and AIZOME_SHELL_BIN" };
        match selected {
            Some(selected) if selected == *expected => (),
            Some(selected) => {
                result.action = Action::Unknown;
                result.reasons.insert(0, format!("The session selects {name} from {}, instead of the active system's {}. Check your {configuration} override before relying on a reload or new login to use the update.", selected.display(), expected.display()));
            }
            None => {
                result.action = Action::Unknown;
                result.reasons.insert(0, format!("The session's {name} launch path could not be verified. Check your {configuration} before finishing the desktop update."));
            }
        }
    }
    if result.action != Action::None {
        return result;
    }
    // A development compositor reloads the library beside itself, not the
    // system package. Its check cannot certify a system update as applied.
    if running.compositor != installed.compositor && !running.compositor.starts_with("/nix/store") {
        return Completion::new(
            Action::Unknown,
            "Aizome is running from a development build. Its reload does not necessarily select the active system's desktop. Check the session's launch configuration before signing out and back in.",
        );
    }
    if !running.logic_healthy || running.reload.core.action != "same" {
        return Completion::new(
            Action::Unknown,
            "The running Aizome core or logic could not be verified as ready. Check Aizome's recovery status, then refresh Yukimi.",
        );
    }
    for (name, part) in [("compositor logic", &running.reload.logic), ("shell", &running.reload.shell)] {
        match part.action.as_str() {
            "unchanged" => (),
            "would-reload" => {
                if result.action == Action::None {
                    result.action = Action::Reload;
                }
                result.reasons.push(format!("The running Aizome {name} has not picked up the installed version."));
            }
            _ => {
                result.action = Action::Unknown;
                result.reasons.push(
                    format!("The Aizome {name} update could not be verified ({}). {}", part.action, part.detail)
                        .trim_end()
                        .to_owned(),
                );
            }
        }
    }
    // Settings are live. `reload --check` always says `would-reload` for
    // them; it is not evidence of unapplied changes.
    result
}

fn program(name: &str, path: &str, cwd: &Path) -> Option<PathBuf> {
    let candidates = if name.contains('/') {
        vec![cwd.join(name)]
    } else {
        std::env::split_paths(path).map(|dir| cwd.join(dir).join(name)).collect()
    };
    candidates
        .into_iter()
        .find(|candidate| {
            std::fs::metadata(candidate).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
}

// The ABI answers whether logic can load, not whether the core's runtime
// changed. Compare the actual mapped core/std and executable byte for byte.
// Their ELF search paths contain immutable Nix native-library identities,
// so a native rebuild or packaging change is covered without forcing a
// login just because unrelated apps/logic changed the package's store path.
fn runtime_matches(installed: &Path, executable: &Path, running: &Runtime) -> Result<bool, String> {
    let check = || -> std::io::Result<bool> {
        let dir = installed.parent().and_then(Path::parent).unwrap_or(installed).join("lib/aizome");
        let libraries: Vec<_> = std::fs::read_dir(&dir)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("libstd-") && name.ends_with(".so")
            })
            .collect();
        let [standard_library] = libraries.as_slice() else {
            return Err(std::io::Error::other("expected one packaged Rust standard library"));
        };
        Ok(same_file_contents(installed, executable)?
            && same_file_contents(&dir.join("libaizome_comp_core.so"), &running.core)?
            && same_file_contents(&standard_library.path(), &running.standard_library)?)
    };
    check().map_err(|_| "The running Aizome core's packaged runtime could not be verified.".into())
}

fn same_file_contents(a: &Path, b: &Path) -> std::io::Result<bool> {
    let mut a = std::fs::File::open(a)?;
    let mut b = std::fs::File::open(b)?;
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let mut left = [0; 65536];
    let mut right = [0; 65536];
    loop {
        let n = a.read(&mut left)?;
        if n == 0 {
            return Ok(true);
        }
        b.read_exact(&mut right[..n])?;
        if left[..n] != right[..n] {
            return Ok(false);
        }
    }
}

fn mapped_runtime(maps: &str) -> Result<Runtime, String> {
    let mut cores = BTreeSet::new();
    let mut standards = BTreeSet::new();
    for line in maps.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(path) = fields.get(5).map(Path::new) else { continue };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let collection = if name == "libaizome_comp_core.so" {
            &mut cores
        } else if name.starts_with("libstd-") && name.ends_with(".so") {
            &mut standards
        } else {
            continue;
        };
        // Only immutable files can attest to bytes mapped earlier. Deleted
        // files and mutable development libraries need a fresh session.
        if fields.len() != 6 || !path.starts_with("/nix/store") {
            return Err("Aizome's loaded core/runtime is not an intact packaged library.".into());
        }
        collection.insert(path.to_owned());
    }
    if cores.len() != 1 || standards.len() != 1 {
        return Err("Aizome's loaded core and Rust runtime could not be identified uniquely.".into());
    }
    Ok(Runtime { core: cores.pop_first().unwrap(), standard_library: standards.pop_first().unwrap() })
}

fn session_environment_changes(current: &Environment, running: &Environment) -> Result<Vec<String>, String> {
    let missing = || {
        "Aizome's applied startup environment was not recorded by this session's launcher. Sign out and back in with the updated launcher, then refresh Yukimi; environment completion is unverified.".to_owned()
    };
    let keys = running.get(ENVIRONMENT_KEYS).and_then(|value| value.strip_prefix("1 ")).ok_or_else(missing)?;
    let previous: BTreeSet<String> = keys.split(' ').map(str::to_owned).collect();
    if previous.iter().any(|key| {
        key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || key.as_bytes()[0].is_ascii_digit()
            || !running.contains_key(key)
    }) {
        return Err(missing());
    }
    let user_file = running.get(ENVIRONMENT_FILE).filter(|path| Path::new(path).is_absolute()).ok_or_else(missing)?;
    let user = environment_file(Path::new(user_file), false)?;
    let mut expected = current.clone();
    expected.extend(user);
    // The launcher assigns these after reading both files.
    expected.insert("XDG_SESSION_DESKTOP".into(), "aizome".into());
    expected.insert("XDG_SESSION_TYPE".into(), "wayland".into());
    if expected.get("XDG_CURRENT_DESKTOP").is_none_or(|value| value.is_empty()) {
        expected.insert("XDG_CURRENT_DESKTOP".into(), "Aizome".into());
    }
    Ok(environment_changes(&previous, &expected, &Environment::new(), running))
}

fn environment_file(path: &Path, required: bool) -> Result<Environment, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if !required && e.kind() == std::io::ErrorKind::NotFound => return Ok(Environment::new()),
        Err(_) => return Err("Aizome's startup environment configuration could not be read.".into()),
    };
    // Match aizome-session: literal KEY=VALUE lines, without shell expansion.
    Ok(text
        .split_terminator('\n')
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| !matches!(*key, ENVIRONMENT_KEYS | ENVIRONMENT_FILE))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect())
}

fn environment_changes(
    previous: &BTreeSet<String>,
    current: &Environment,
    user: &Environment,
    running: &Environment,
) -> Vec<String> {
    let mut expected = current.clone();
    expected.extend(user.clone());
    let keys: BTreeSet<_> = previous.iter().chain(expected.keys()).collect();
    keys.into_iter()
        .filter(|key| match expected.get(*key) {
            Some(value) => running.get(*key) != Some(value),
            // These are the names applied at this login, including user-only
            // variables. A removed name remains pending regardless of value.
            None => running.contains_key(*key),
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_ipc::ReloadPart;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::{fs::symlink, net::UnixListener};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "yukimi-aizome-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("bin")).unwrap();
            for name in ["aizome", "aizome-shell"] {
                let path = root.join("bin").join(name);
                std::fs::write(&path, "this fixture must never be executed").unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            std::fs::create_dir_all(root.join("lib/aizome")).unwrap();
            std::fs::write(root.join("lib/aizome/libaizome_comp_core.so"), "core with native library paths").unwrap();
            std::fs::write(root.join("lib/aizome/libstd-test.so"), "Rust runtime").unwrap();
            Self { root }
        }
        fn installed(&self) -> Installed {
            Installed {
                compositor: self.root.join("bin/aizome"),
                shell: self.root.join("bin/aizome-shell"),
                abi: "a".repeat(64),
            }
        }
        fn running(&self) -> Running {
            Running {
                compositor: self.installed().compositor,
                abi: self.installed().abi,
                environment: [("PATH".into(), self.root.join("bin").display().to_string())].into(),
                cwd: self.root.clone(),
                logic_healthy: true,
                renderer_after_restart: false,
                reload: ReloadReport {
                    core: ReloadPart::new("same", ""),
                    logic: ReloadPart::new("unchanged", ""),
                    shell: ReloadPart::new("unchanged", ""),
                    settings: ReloadPart::new("would-reload", ""),
                },
                runtime: Ok(Runtime {
                    core: self.root.join("lib/aizome/libaizome_comp_core.so"),
                    standard_library: self.root.join("lib/aizome/libstd-test.so"),
                }),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn changed_core_needs_a_login_even_after_the_shell_was_restarted() {
        let f = Fixture::new();
        let mut running = f.running();
        running.abi = "b".repeat(64);
        running.reload.core = ReloadPart::new("same", "an older PATH target can report the same core");
        running.reload.logic = ReloadPart::new("refused", "different core");
        let c = assess(&f.installed(), &running, &[]);
        assert_eq!(c.action, Action::Logout);
        assert!(c.description().contains("restarting only the shell does not replace the compositor"));
        assert!(c.description().contains("No computer restart is needed"));
    }

    #[test]
    fn compatible_logic_or_shell_changes_need_a_reload_not_logout() {
        let f = Fixture::new();
        for logic in [false, true] {
            let mut running = f.running();
            if logic {
                running.reload.logic.action = "would-reload".into();
            } else {
                running.reload.shell.action = "would-reload".into();
            }
            let c = assess(&f.installed(), &running, &[]);
            assert_eq!(c.action, Action::Reload);
            assert!(c.description().contains("Reload Aizome"));
            assert!(!c.description().contains("Sign out"));
        }
    }

    #[test]
    fn current_components_are_applied_even_though_settings_would_reload() {
        let f = Fixture::new();
        assert_eq!(assess(&f.installed(), &f.running(), &[]), Completion::default());
        // Another package generation can contain the same core build.
        let old = Fixture::new();
        assert!(
            runtime_matches(&f.installed().compositor, &old.installed().compositor, &old.running().runtime.unwrap())
                .unwrap()
        );
    }

    #[test]
    fn identical_abi_does_not_hide_changed_core_or_native_runtime() {
        for file in ["libaizome_comp_core.so", "libstd-test.so"] {
            let f = Fixture::new();
            let old = Fixture::new();
            let mut running = f.running();
            running.runtime = old.running().runtime;
            // The mapped library is from before a native dependency/RPATH
            // change. Rust's ABI and the reload report are still identical.
            std::fs::write(f.root.join("lib/aizome").join(file), "new native library identity").unwrap();
            let result = assess(&f.installed(), &running, &[]);
            assert_eq!(result.action, Action::Logout, "{file}");
            assert!(result.description().contains("libraries changed"));
        }
        let f = Fixture::new();
        let old = Fixture::new();
        std::fs::write(old.root.join("bin/aizome"), "different executable/runtime search paths").unwrap();
        assert!(
            !runtime_matches(&f.installed().compositor, &old.installed().compositor, &old.running().runtime.unwrap())
                .unwrap()
        );
    }

    #[test]
    fn unknown_runtime_never_certifies_a_compatible_core() {
        let f = Fixture::new();
        let mut running = f.running();
        running.runtime = Err("missing mapped library evidence".into());
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown);
        running.runtime = f.running().runtime;
        std::fs::remove_file(f.root.join("lib/aizome/libstd-test.so")).unwrap();
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown);
    }

    #[test]
    fn runtime_maps_require_unique_immutable_core_and_std_files() {
        let core = "1000-2000 r-xp 00000000 00:00 1 /nix/store/old-aizome/lib/aizome/libaizome_comp_core.so\n";
        let std = "2000-3000 r-xp 00000000 00:00 2 /nix/store/old-aizome/lib/aizome/libstd-test.so\n";
        let maps = format!("{core}{std}{core}");
        let runtime = mapped_runtime(&maps).unwrap();
        assert_eq!(runtime.core, Path::new("/nix/store/old-aizome/lib/aizome/libaizome_comp_core.so"));
        assert!(mapped_runtime(core).is_err());
        assert!(mapped_runtime(&maps.replace("/nix/store/old-aizome", "/tmp/build")).is_err());
        assert!(mapped_runtime(&maps.replace("libstd-test.so\n", "libstd-test.so (deleted)\n")).is_err());
        assert!(mapped_runtime(&format!("{maps}{}", core.replace("old-aizome", "another-core"))).is_err());
    }

    #[test]
    fn runtime_comparison_checks_bytes_beyond_the_first_buffer() {
        let f = Fixture::new();
        let a = f.root.join("a");
        let b = f.root.join("b");
        let mut bytes = vec![0u8; 65537];
        std::fs::write(&a, &bytes).unwrap();
        std::fs::write(&b, &bytes).unwrap();
        assert!(same_file_contents(&a, &b).unwrap());
        bytes[65536] = 1;
        std::fs::write(&b, bytes).unwrap();
        assert!(!same_file_contents(&a, &b).unwrap());
    }

    #[test]
    fn profile_override_prevents_an_all_clear_or_misleading_sign_out_button() {
        let f = Fixture::new();
        let old = Fixture::new();
        let mut running = f.running();
        running.abi = "b".repeat(64);
        running
            .environment
            .insert("PATH".into(), format!("{}:{}", old.root.join("bin").display(), f.root.join("bin").display()));
        let c = assess(&f.installed(), &running, &[]);
        assert_eq!(c.action, Action::Unknown);
        assert!(c.description().contains("instead of the active system"));
        assert!(c.description().contains("Sign out and back in"));
        assert!(c.description().contains("Check your profile"));
        assert!(c.reasons[0].contains("instead of the active system"));
    }

    #[test]
    fn shell_override_is_detected_even_if_the_core_and_logic_are_current() {
        let f = Fixture::new();
        let old = Fixture::new();
        let mut running = f.running();
        running.environment.insert("AIZOME_SHELL_BIN".into(), old.root.join("bin/aizome-shell").display().to_string());
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown);
        running.environment.insert("AIZOME_SHELL_BIN".into(), f.installed().shell.display().to_string());
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::None);
    }

    #[test]
    fn launch_paths_follow_symlinks_and_the_compositors_working_directory() {
        let f = Fixture::new();
        std::fs::create_dir(f.root.join("profile")).unwrap();
        symlink(f.root.join("bin/aizome"), f.root.join("profile/aizome")).unwrap();
        assert_eq!(program("aizome", "profile:bin", &f.root), Some(f.installed().compositor));
        assert_eq!(program("./bin/aizome-shell", "", &f.root), Some(f.installed().shell));
        assert_eq!(program("absent", "bin", &f.root), None);
        let mut running = f.running();
        running.environment.remove("PATH");
        running.cwd = f.root.join("bin");
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown);
    }

    #[test]
    fn development_compositor_cannot_certify_the_system_update() {
        let f = Fixture::new();
        let mut running = f.running();
        running.compositor = f.root.join("target/debug/aizome");
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown);
    }

    #[test]
    fn a_pending_renderer_change_also_needs_a_new_login() {
        let f = Fixture::new();
        let mut running = f.running();
        running.renderer_after_restart = true;
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Logout);
    }

    #[test]
    fn missing_refused_or_future_reload_results_are_not_success() {
        let f = Fixture::new();
        for action in ["", "skipped", "refused", "failed", "future-action"] {
            for logic in [false, true] {
                let mut running = f.running();
                if logic {
                    running.reload.logic.action = action.into();
                } else {
                    running.reload.shell.action = action.into();
                }
                assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown, "{action}");
            }
        }
        let mut running = f.running();
        running.logic_healthy = false;
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown);
        running.logic_healthy = true;
        running.reload.core.action = "skipped".into();
        assert_eq!(assess(&f.installed(), &running, &[]).action, Action::Unknown);
    }

    #[test]
    fn environment_changes_include_removed_variables_and_respect_user_settings() {
        let old: Environment =
            [("SDL_VIDEODRIVER".into(), "wayland,x11".into()), ("QT_QPA_PLATFORM".into(), "wayland".into())].into();
        let current: Environment = [("QT_QPA_PLATFORM".into(), "wayland;xcb".into())].into();
        let previous = old.keys().cloned().collect();
        assert_eq!(
            environment_changes(&previous, &current, &Environment::new(), &old),
            vec!["QT_QPA_PLATFORM", "SDL_VIDEODRIVER"]
        );
        assert!(environment_changes(&previous, &current, &old, &old).is_empty());
        assert!(environment_changes(&previous, &current, &Environment::new(), &current).is_empty());
        let f = Fixture::new();
        let c = assess(&f.installed(), &f.running(), &["SDL_VIDEODRIVER".into()]);
        assert_eq!(c.action, Action::Logout);
        assert!(c.description().contains("old environment"));
    }

    #[test]
    fn removals_follow_the_login_environment_including_user_only_settings() {
        // These values may have been absent at boot, changed since boot,
        // or supplied only by the user's now-removed environment file.
        for value in ["wayland,x11", "x11", ""] {
            let running: Environment = [("SDL_VIDEODRIVER".into(), value.into())].into();
            let applied = running.keys().cloned().collect();
            assert_eq!(
                environment_changes(&applied, &Environment::new(), &Environment::new(), &running),
                vec!["SDL_VIDEODRIVER"]
            );
            // Unrelated inherited application variables are not managed.
            assert!(
                environment_changes(&BTreeSet::new(), &Environment::new(), &Environment::new(), &running).is_empty()
            );
        }
    }

    #[test]
    fn missing_or_invalid_startup_records_are_unverified() {
        let f = Fixture::new();
        let mut running: Environment = [(ENVIRONMENT_FILE.into(), f.root.join("missing").display().to_string())].into();
        assert!(session_environment_changes(&Environment::new(), &running).is_err());
        for record in ["", "2 HOME", "1 ", "1 ABSENT", "1 bad-name", "1 9INVALID"] {
            running.insert(ENVIRONMENT_KEYS.into(), record.into());
            assert!(session_environment_changes(&Environment::new(), &running).is_err(), "{record}");
        }
    }

    #[test]
    fn unrecorded_environment_prevents_an_all_clear_or_reload_only_advice() {
        let f = Fixture::new();
        for action in ["unchanged", "would-reload"] {
            let mut running = f.running();
            running.reload.logic.action = action.into();
            let environment = session_environment_changes(&Environment::new(), &running.environment);
            assert_eq!(assess_session(&f.installed(), &running, environment).action, Action::Unknown);
        }
        let mut running = f.running();
        running.abi = "b".repeat(64);
        let result = assess_session(&f.installed(), &running, Err("missing startup record".into()));
        assert_eq!(result.action, Action::Logout);
        assert!(result.description().contains("missing startup record"));
    }

    #[test]
    fn launcher_records_the_applied_environment_for_both_launch_paths() {
        for manager in ["running", "offline"] {
            let f = Fixture::new();
            let config = f.root.join("config");
            let environment = config.join("aizome/environment");
            std::fs::create_dir_all(environment.parent().unwrap()).unwrap();
            std::fs::write(&environment, format!(
                "USER_ONLY=first\nUSER_ONLY=$literal=last\nSDL_VIDEODRIVER=x11\nXDG_CONFIG_HOME={}\nXDG_SESSION_DESKTOP=ignored\nXDG_SESSION_TYPE=ignored\nAIZOME_ENVIRONMENT_KEYS=forged\nAIZOME_ENVIRONMENT_FILE=/forged\nPATH={}:{}\n",
                f.root.join("elsewhere").display(), f.root.join("bin").display(), std::env::var("PATH").unwrap()
            )).unwrap();
            for (name, script) in [
                ("aizome", "#!/bin/sh\nexec env -0 > \"$TEST_RECORD\"\n"),
                ("dbus-run-session", "#!/bin/sh\nexec \"$@\"\n"),
                ("dbus-update-activation-environment", "#!/bin/sh\nexit 0\n"),
                (
                    "systemctl",
                    r#"#!/bin/sh
case "$*" in
  '--user is-system-running') echo "$TEST_MANAGER" ;;
  '--user -q is-active aizome.service') exit 1 ;;
  '--user import-environment '*) printf '%s\n' "$@" > "$TEST_IMPORT" ;;
  '--user --wait start aizome.service') exec aizome ;;
esac
"#,
                ),
            ] {
                let path = f.root.join("bin").join(name);
                std::fs::write(&path, script).unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let record = f.root.join("record");
            let imports = f.root.join("imports");
            let launcher =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../desktop/aizome/session/aizome-session");
            let result = std::process::Command::new("sh")
                .arg(launcher)
                .arg("--no-login-shell")
                .env_clear()
                .env("HOME", &f.root)
                .env("XDG_CONFIG_HOME", &config)
                .env("PATH", format!("{}:{}", f.root.join("bin").display(), std::env::var("PATH").unwrap()))
                .env("TEST_MANAGER", manager)
                .env("TEST_RECORD", &record)
                .env("TEST_IMPORT", &imports)
                .output()
                .unwrap();
            assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
            let raw = std::fs::read_to_string(record).unwrap();
            let running: Environment =
                raw.split('\0').filter_map(|v| v.split_once('=')).map(|(k, v)| (k.into(), v.into())).collect();
            assert_eq!(running["USER_ONLY"], "$literal=last");
            assert_eq!(running[ENVIRONMENT_FILE], environment.display().to_string());
            assert!(running[ENVIRONMENT_KEYS].split(' ').any(|key| key == "USER_ONLY"));
            if manager == "running" {
                let names = std::fs::read_to_string(imports).unwrap();
                assert!(names.lines().any(|name| name == ENVIRONMENT_KEYS));
                assert!(names.lines().any(|name| name == ENVIRONMENT_FILE));
                assert_eq!(names.lines().filter(|name| *name == ENVIRONMENT_KEYS).count(), 1);
            }
            // Read-only access to the same system file the launcher read;
            // every command that could touch the session is a private stub.
            let current = environment_file(Path::new("/etc/xdg/aizome/environment"), false).unwrap();
            assert_eq!(session_environment_changes(&current, &running).unwrap(), Vec::<String>::new());
            std::fs::remove_file(environment).unwrap();
            let changes = session_environment_changes(&current, &running).unwrap();
            assert!(changes.contains(&"USER_ONLY".into()));
        }
    }

    #[test]
    fn environment_files_use_the_session_launchers_literal_syntax() {
        let f = Fixture::new();
        let path = f.root.join("environment");
        std::fs::write(&path, "#ignored=yes\nKEY=first\nKEY=$literal=last\nEMPTY=\n").unwrap();
        assert_eq!(
            environment_file(&path, true).unwrap(),
            [("KEY".into(), "$literal=last".into()), ("EMPTY".into(), "".into())].into()
        );
        assert!(environment_file(&f.root.join("missing"), true).is_err());
        assert!(environment_file(&f.root.join("missing"), false).unwrap().is_empty());
    }

    #[test]
    fn build_fingerprints_must_be_present_and_complete() {
        for value in [serde_json::json!({}), serde_json::json!({"abi":""}), serde_json::json!({"abi":"unknown"})] {
            assert!(abi(&value).is_err());
        }
        assert_eq!(abi(&serde_json::json!({"abi":"a".repeat(64)})).unwrap(), "a".repeat(64));
    }

    #[test]
    fn ipc_inspection_only_checks_and_reads_the_compositors_environment() {
        let f = Fixture::new();
        let process = f.root.join("proc/42");
        std::fs::create_dir_all(&process).unwrap();
        symlink(f.installed().compositor, process.join("exe")).unwrap();
        symlink(&f.root, process.join("cwd")).unwrap();
        std::fs::write(process.join("environ"), format!("PATH={}\0AIZOME_SHELL_BIN=\0", f.root.join("bin").display()))
            .unwrap();
        let socket = f.root.join("ipc");
        let listener = UnixListener::bind(&socket).unwrap();
        let version = Response::Version {
            version: "same-version-label".into(),
            protocol: 1,
            pid: 42,
            exe: Some(f.installed().compositor.display().to_string()),
            build: Some(serde_json::json!({"abi":f.installed().abi})),
        };
        let reload = Response::Reload { report: f.running().reload };
        let stats = Response::Stats {
            stats: aizome_ipc::Stats {
                logic: aizome_ipc::LogicStats { core_abi: f.installed().abi, healthy: true, ..Default::default() },
                ..Default::default()
            },
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            for (expected, response) in [
                (Request::Version, version),
                (Request::Stats { reset: false }, stats),
                (Request::Reload { logic: false, shell: false, from: None, shell_from: None, check: true }, reload),
            ] {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), expected);
                writeln!(stream, "{}", serde_json::to_string(&response).unwrap()).unwrap();
            }
        });
        let running = snapshot(&socket, &f.root.join("proc"), Duration::from_secs(1)).unwrap();
        assert_eq!(running.environment.get("PATH"), Some(&f.root.join("bin").display().to_string()));
        assert_eq!(running.abi, f.installed().abi);
        assert!(running.runtime.is_err()); // Fixture has no /proc maps evidence.
        server.join().unwrap();
    }

    #[test]
    fn absent_or_unresponsive_ipc_is_unverified_and_does_not_wait_forever() {
        let f = Fixture::new();
        let socket = f.root.join("ipc");
        assert!(snapshot(&socket, &f.root, Duration::from_millis(20)).is_err());
        let listener = UnixListener::bind(&socket).unwrap();
        let (done, wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let start = std::time::Instant::now();
        assert!(snapshot(&socket, &f.root, Duration::from_millis(20)).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        done.send(()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn a_full_ipc_accept_queue_cannot_block_completion() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        let f = Fixture::new();
        let socket = f.root.join("ipc");
        let listener = UnixListener::bind(&socket).unwrap();
        // Safety: this is the live descriptor of our private fixture listener.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let _queued = UnixStream::connect(&socket).unwrap();
        let (done, wait) = std::sync::mpsc::channel();
        let proc = f.root.clone();
        let worker = std::thread::spawn(move || {
            done.send(snapshot(&socket, &proc, Duration::from_millis(20)).is_err()).unwrap();
        });
        let within_budget = wait.recv_timeout(Duration::from_millis(300));
        // Also release/reap a regressed blocking implementation before the
        // assertion, so a failed test never leaves a hung worker behind.
        let _drained = listener.accept().unwrap();
        if within_budget.is_err() {
            assert!(wait.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        worker.join().unwrap();
        assert_eq!(within_budget, Ok(true), "connect exceeded the inspection timeout");
    }

    #[test]
    fn old_or_invalid_ipc_responses_cannot_be_an_all_clear() {
        for response in [
            "not json",
            r#"{"response":"error","message":"unsupported"}"#,
            r#"{"response":"version","version":"0.1.0","protocol":1}"#,
        ] {
            let f = Fixture::new();
            let socket = f.root.join("ipc");
            let listener = UnixListener::bind(&socket).unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut request).unwrap();
                assert_eq!(serde_json::from_str::<Request>(&request).unwrap(), Request::Version);
                writeln!(stream, "{response}").unwrap();
            });
            assert!(snapshot(&socket, &f.root, Duration::from_secs(1)).is_err());
            server.join().unwrap();
        }
    }
}
