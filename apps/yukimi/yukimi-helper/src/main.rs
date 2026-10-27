// SPDX-License-Identifier: MIT OR Apache-2.0
//! `yukimi-helper`: the part of Yukimi that changes the whole system. It
//! runs as root through `pkexec`, so it does only a few fixed things, with
//! arguments it checks itself, on the configuration it finds itself (see
//! `yukimi_system::setup`):
//!
//! - `change [options] --mode auto|switch|boot` changes what the system installs
//!   and where it comes from, builds the new system and switches to it. If
//!   anything before the switch fails, every file it touched is put back as
//!   it was. The options:
//!   - `--packages a,b` and `--programs x,y`: what `yukimi.nix` installs
//!     (written next to the main configuration file, and imported from it
//!     once);
//!   - `--setting name --applications x,y`: a list of application ids that
//!     a catalog the system brings is installed through;
//!   - `--unlist-system a,b` and `--unlist-user name:a,b`: entries to take
//!     out of `environment.systemPackages` or a user's `packages` in the
//!     main configuration file;
//!   - `--update i,j`: flake inputs to update, with `--lock file` a lock
//!     file Yukimi prepared without administrator rights, which is checked
//!     to change only those inputs;
//!   - `--follow input=branch`: make a flake input follow another branch of
//!     its repository (its address in `flake.nix` changes) and update it;
//!   - `--channels`: update root's channels.
//!   - `--sekka`: migrate a predecessor installation if needed, then update
//!     the Sekka input, preserving all independent dependency pins.
//! - `rollback <generation>` switches to an earlier system generation, and
//!   puts back the configuration it was built from.
//! - `clean [--older-than-days n]` deletes old system generations and
//!   collects garbage.
//!
//! Each generation's configuration is kept: a copy of the configuration's
//! directory in [`SAVED`]`/generation-<n>`. So going back to a generation
//! brings back the choices that made it, and the next change builds on
//! those.
//!
//! Progress goes to standard error in Nix's machine-readable log format,
//! which Yukimi shows; the outcome goes to standard output as JSON. When
//! standard input is a pipe and Yukimi closes it, the helper stops what it
//! is doing and puts the configuration back, unless the new system is
//! already being started, which is never interrupted.
use std::io::{Read, Write};

mod transaction;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use yukimi_config::packages;
use yukimi_config::sekka;
use yukimi_config::{branch, edit, lock};
use yukimi_system::catalog;
use yukimi_system::completion::{self, Receipt};
use yukimi_system::setup::{Choice, Kind, Setup};

const PROFILE: &str = "/nix/var/nix/profiles/system";
/// pkexec starts programs with an almost empty environment.
const PATH: &str = "/run/current-system/sw/bin:/run/wrappers/bin";
/// Where each generation's configuration is kept, as `generation-<n>`.
const SAVED: &str = yukimi_system::SAVED_CONFIGURATIONS;
/// Files bigger than this aren't configuration, and aren't kept with it.
const LARGEST_KEPT: u64 = 1 << 20;
/// Where Nix keeps the store and builds; the one with least room counts.
const BUILD_PLACES: [&str; 2] = ["/nix", "/tmp"];
/// A new system isn't built with less room than this.
const SPACE_NEEDED: u64 = 10_000_000_000;
/// Building stops when room falls below this, rather than fill the disk.
const SPACE_FLOOR: u64 = 4_000_000_000;

/// Where the work is, which decides whether it can still be stopped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Working,
    /// Yukimi asked to stop.
    Stopping,
    /// The new system is being started: too late to stop.
    Committed,
}

/// The phase, and the program running now (to stop when asked).
static STATE: Mutex<(Phase, Option<u32>)> = Mutex::new((Phase::Working, None));
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

fn stop() {
    let mut state = state();
    if state.0 == Phase::Committed {
        return;
    }
    state.0 = Phase::Stopping;
    if let Some(pid) = state.1 {
        // SAFETY: signalling a process has no memory-safety preconditions.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    }
}

fn state() -> std::sync::MutexGuard<'static, (Phase, Option<u32>)> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Stop when standard input closes, if it is a pipe: that is how Yukimi
/// asks. Standard input that is a terminal or /dev/null means nothing.
fn watch_for_stop() {
    // Terminal cancellation must follow the same rollback path as closing
    // the GUI's pipe, not abruptly exit with a half-edited configuration.
    // SAFETY: the signal handler only stores to a lock-free atomic.
    unsafe {
        libc::signal(libc::SIGINT, interrupt as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, interrupt as *const () as libc::sighandler_t);
    }
    std::thread::spawn(|| {
        loop {
            if INTERRUPTED.swap(false, Ordering::Relaxed) {
                stop();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });
    // SAFETY: fstat writes only into the struct it is given.
    let is_pipe = unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        libc::fstat(0, &mut stat) == 0 && (stat.st_mode & libc::S_IFMT) == libc::S_IFIFO
    };
    if !is_pipe {
        return;
    }
    std::thread::spawn(|| {
        let mut buffer = [0u8; 64];
        while matches!(std::io::stdin().read(&mut buffer), Ok(n) if n > 0) {}
        stop();
    });
}

fn check_stopped() -> Result<()> {
    if state().0 == Phase::Stopping { Err(Failure("Stopped".into())) } else { Ok(()) }
}

/// From here on the work finishes even if Yukimi goes away, so that the
/// system is never left half switched.
fn commit() -> Result<()> {
    let mut state = state();
    if state.0 == Phase::Stopping {
        return Err(Failure("Stopped".into()));
    }
    state.0 = Phase::Committed;
    Ok(())
}

/// Start a command, remembering it so it can be stopped, and wait for it.
fn wait(c: &mut Command) -> Result<std::process::Output> {
    let child = {
        let mut state = state();
        if state.0 == Phase::Stopping {
            return Err(Failure("Stopped".into()));
        }
        let child = c.spawn()?;
        state.1 = Some(child.id());
        child
    };
    let out = child.wait_with_output();
    state().1 = None;
    check_stopped()?;
    Ok(out?)
}

/// A failure, in words for the person who asked.
#[derive(Debug)]
struct Failure(String);

impl<E: std::fmt::Display> From<E> for Failure {
    fn from(e: E) -> Failure {
        Failure(e.to_string())
    }
}

type Result<T> = std::result::Result<T, Failure>;

/// Say what is happening, as an activity Yukimi shows.
fn stage(id: u64, text: &str) {
    eprintln!(
        "@nix {}",
        serde_json::json!({"action": "start", "id": id, "level": 0, "type": 0, "text": text, "fields": []})
    );
}

fn stage_done(id: u64) {
    eprintln!("@nix {}", serde_json::json!({"action": "stop", "id": id}));
}

fn report(level: u8, message: &str) {
    eprintln!("@nix {}", serde_json::json!({"action": "msg", "level": level, "msg": message}));
}

fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    // Only the helper handles terminal cancellation. In particular, an
    // activation already committed must finish even when Ctrl-C is pressed.
    c.process_group(0);
    c.env_clear().env("PATH", PATH).env("HOME", "/root").env("LANG", "C.UTF-8").stdin(Stdio::null());
    if let Ok(archive) = std::fs::read_link("/run/current-system/sw/lib/locale/locale-archive") {
        c.env("LOCALE_ARCHIVE", archive);
    }
    c
}

fn nix() -> Command {
    let mut c = command("nix");
    c.args(["--extra-experimental-features", "nix-command flakes"]);
    c
}

/// Run a command whose error output goes straight to Yukimi.
fn run(mut c: Command, what: &str) -> Result<()> {
    let out = wait(c.stdout(Stdio::inherit()).stderr(Stdio::inherit()))?;
    if out.status.success() { Ok(()) } else { Err(Failure(format!("{what} failed"))) }
}

/// Run a command and keep its standard output.
fn output(mut c: Command, what: &str) -> Result<String> {
    let out = wait(c.stdout(Stdio::piped()).stderr(Stdio::inherit()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Failure(format!("{what} failed")))
    }
}

/// Files as they were, to put back if a change fails.
struct Backup(Vec<(PathBuf, Option<Vec<u8>>)>);

impl Backup {
    fn take(files: &[PathBuf]) -> Backup {
        Backup(files.iter().map(|path| (path.clone(), std::fs::read(path).ok())).collect())
    }

    fn restore(&self) {
        for (path, contents) in &self.0 {
            match contents {
                Some(bytes) => {
                    let _ = std::fs::write(path, bytes);
                }
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

/// Write a file of the configuration. A new file belongs to whoever owns
/// its directory, so a configuration kept in someone's home stays theirs,
/// and in a Git work tree, Git is told about it (as intended to be added),
/// or a flake there wouldn't see it.
fn write_file(setup: &Setup, path: &Path, contents: &str) -> Result<()> {
    let new = !path.exists();
    std::fs::write(path, contents)?;
    if !new {
        return Ok(());
    }
    if let Some(meta) = path.parent().and_then(|dir| std::fs::metadata(dir).ok()) {
        use std::os::unix::fs::MetadataExt;
        let _ = std::os::unix::fs::chown(path, Some(meta.uid()), Some(meta.gid()));
    }
    if setup.git {
        let mut git = command("git");
        // Root works in someone else's work tree here, which Git refuses
        // unless told it is safe.
        git.args(["-c", "safe.directory=*", "-C"]).arg(path.parent().unwrap_or(&setup.dir));
        git.args(["add", "--intent-to-add", "--"]).arg(path);
        if !wait(git.stdout(Stdio::null()).stderr(Stdio::null()))?.status.success() {
            return Err(Failure(format!(
                "{} is in a Git repository, and Git couldn't be told about {}",
                setup.dir.display(),
                path.display()
            )));
        }
    }
    Ok(())
}

/// The number of the system generation the profile points at.
fn current_generation() -> Option<u32> {
    let link = std::fs::read_link(PROFILE).ok()?;
    generation_number(link.file_name()?.to_str()?)
}

/// `system-12-link` is generation 12.
fn generation_number(link: &str) -> Option<u32> {
    link.strip_prefix("system-")?.strip_suffix("-link")?.parse().ok()
}

fn saved(generation: u32) -> PathBuf {
    yukimi_system::saved_configuration(generation)
}

/// Copy a configuration directory: its files and folders, not hidden ones
/// (such as a Git directory), build results or large files. Each file keeps
/// its permissions, so a file only its owner may read (in a configuration
/// kept in a home directory, say) stays that way.
fn copy_configuration(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.starts_with('.') || text.starts_with("result") {
            continue;
        }
        let (from, to) = (entry.path(), to.join(&name));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_configuration(&from, &to)?;
        } else if kind.is_file() && entry.metadata()?.len() <= LARGEST_KEPT {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Keep the configuration as generation `generation`'s, unless one is kept
/// already (or `replace`).
fn save_configuration(setup: &Setup, generation: u32, replace: bool) -> Result<()> {
    let dir = saved(generation);
    if dir.is_dir() && !replace {
        return Ok(());
    }
    let partial = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&partial);
    copy_configuration(&setup.dir, &partial)?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&partial, &dir)?;
    Ok(())
}

/// Put back the files Yukimi changes (the lock file, its own file, the main
/// configuration file) as a kept configuration has them. A file someone
/// changed since the running generation was built is left as it is, so
/// that nothing written by hand is lost. Returns the files left alone.
fn restore_choices(setup: &Setup, kept: &Path, running: Option<&Path>) -> Vec<PathBuf> {
    let mut files = vec![setup.dir.join("flake.nix"), setup.dir.join("flake.lock")];
    files.extend(setup.main.clone());
    files.extend(setup.packages_file());
    let mut left = Vec::new();
    for file in files {
        let Ok(relative) = file.strip_prefix(&setup.dir) else { continue };
        let now = std::fs::read(&file).ok();
        let then = std::fs::read(kept.join(relative)).ok();
        if now == then {
            continue;
        }
        // Unchanged since the running generation was made from it?
        let untouched = running.is_some_and(|r| std::fs::read(r.join(relative)).ok() == now);
        if !untouched {
            left.push(file);
            continue;
        }
        let _ = match then {
            Some(bytes) => std::fs::write(&file, bytes),
            None => std::fs::remove_file(&file),
        };
    }
    left
}

/// Forget the configurations of generations that no longer exist.
fn prune_saved() {
    let Ok(entries) = std::fs::read_dir(SAVED) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let generation = name.strip_prefix("generation-").and_then(|n| n.parse::<u32>().ok());
        let exists =
            generation.map(|g| Path::new(&format!("{PROFILE}-{g}-link")).symlink_metadata().is_ok()).unwrap_or(false);
        if !exists {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn split_list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

fn valid_host(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn valid_user(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Build a flake's system for `host`, with the binary caches it configures
/// as well as the running system's: a cache the new system adds is used for
/// the build that adds it. The daemon takes them from root.
fn build_flake(setup: &Setup, host: &str) -> std::result::Result<String, Failure> {
    let system = format!("{}#nixosConfigurations.\"{host}\"", setup.flake());
    let mut build = nix();
    build.args(["build", "--log-format", "internal-json", "-v", "--no-link", "--print-out-paths"]);
    if let Some((substituters, keys)) = caches(&system) {
        build.args(["--option", "extra-substituters", &substituters.join(" ")]);
        build.args(["--option", "extra-trusted-public-keys", &keys.join(" ")]);
    }
    build.arg(format!("{system}.config.system.build.toplevel"));
    Ok(with_room(|| output(build, "Building the new system"))?.trim().to_owned())
}

/// The free space, in bytes, of the filesystem holding `path`.
fn free_space(path: &str) -> Option<u64> {
    let path = std::ffi::CString::new(path).ok()?;
    // SAFETY: statvfs writes only into the struct it is given.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(path.as_ptr(), &mut stat) == 0).then(|| stat.f_bavail as u64 * stat.f_frsize as u64)
    }
}

fn least_room() -> Option<u64> {
    BUILD_PLACES.iter().filter_map(|p| free_space(p)).min()
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

/// Run `work` (building, or downloading for it) only with room to spare,
/// and stop it before the disk fills up: whatever it would download or
/// compile, an update never leaves the computer without space. The files it
/// changed are then put back, as for any failure.
fn with_room<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    if let Some(free) = least_room()
        && free < SPACE_NEEDED
    {
        return Err(Failure(format!(
            "Only {} is free, and updating needs {}. Free some space in Yukimi's Storage page, then try again.",
            gigabytes(free),
            gigabytes(SPACE_NEEDED)
        )));
    }
    let (done, low) = (std::sync::Arc::new(AtomicBool::new(false)), std::sync::Arc::new(AtomicBool::new(false)));
    let watcher = {
        let (done, low) = (done.clone(), low.clone());
        std::thread::spawn(move || {
            // Every two seconds, waking often enough to end with the work.
            for tick in 0u64.. {
                if done.load(Ordering::Relaxed) {
                    return;
                }
                if tick % 20 == 0 && least_room().is_some_and(|free| free < SPACE_FLOOR) {
                    low.store(true, Ordering::Relaxed);
                    stop();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
    };
    let result = work();
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    if low.load(Ordering::Relaxed) {
        return Err(Failure(format!(
            "Stopped before the disk filled up: less than {} was left. Nothing was changed.",
            gigabytes(SPACE_FLOOR)
        )));
    }
    result
}

/// The binary caches and their keys a system's configuration names, if it
/// names any. A configuration that doesn't evaluate is reported by the build.
fn caches(system: &str) -> Option<(Vec<String>, Vec<String>)> {
    let mut eval = nix();
    eval.args(["eval", "--json", &format!("{system}.config.nix.settings"), "--apply"]);
    eval.arg(
        "s: { substituters = (s.substituters or [ ]) ++ (s.extra-substituters or [ ]); \
         keys = (s.trusted-public-keys or [ ]) ++ (s.extra-trusted-public-keys or [ ]); }",
    );
    let value: serde_json::Value = serde_json::from_str(&output(eval, "Reading the binary caches").ok()?).ok()?;
    let words = |key: &str| -> Vec<String> {
        value[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .filter(|w| !w.is_empty() && !w.chars().any(char::is_whitespace))
            .map(str::to_owned)
            .collect()
    };
    let (substituters, keys) = (words("substituters"), words("keys"));
    (!substituters.is_empty() && !keys.is_empty()).then_some((substituters, keys))
}

/// Build the new system: a flake's configuration for this computer (or its
/// only one), or `configuration.nix` with the channels' Nixpkgs.
fn build(setup: &Setup) -> Result<String> {
    if setup.kind == Kind::Channels {
        let mut build = nix();
        build.args(["build", "--impure", "--log-format", "internal-json", "-v", "--no-link", "--print-out-paths"]);
        build.args(["--file", "<nixpkgs/nixos>", "system"]);
        build.env("NIXOS_CONFIG", setup.dir.join("configuration.nix"));
        // Nix's own setting says where `<nixpkgs>` is; NixOS before 24.05
        // said so only in the environment, which pkexec clears.
        let configured = output(
            {
                let mut c = nix();
                c.args(["config", "show", "nix-path"]);
                c
            },
            "Reading Nix's settings",
        )
        .unwrap_or_default();
        if configured.trim().is_empty() {
            build.env(
                "NIX_PATH",
                format!(
                    "nixpkgs={0}/nixos:nixos-config={1}/configuration.nix:{0}",
                    yukimi_system::channels::ROOT_CHANNELS,
                    setup.dir.display()
                ),
            );
        }
        return Ok(with_room(|| output(build, "Building the new system"))?.trim().to_owned());
    }
    if !valid_host(&setup.host) {
        return Err(Failure(format!("Not a configuration name: {:?}", setup.host)));
    }
    // Usually the configuration is named after the computer; only when it
    // isn't, ask the flake what it has.
    let mut names = nix();
    names.args(["eval", "--json", &format!("{}#nixosConfigurations", setup.flake()), "--apply", "builtins.attrNames"]);
    let listed: Vec<String> = serde_json::from_str(&output(names, "Reading the configurations")?)?;
    let host = if listed.contains(&setup.host) {
        setup.host.clone()
    } else {
        match listed.as_slice() {
            [only] if valid_host(only) => only.clone(),
            _ => {
                return Err(Failure(format!(
                    "The flake in {} has no configuration named {:?} (it has {})",
                    setup.dir.display(),
                    setup.host,
                    listed.join(", ")
                )));
            }
        }
    };
    build_flake(setup, &host)
}

/// Make a built system the current generation and start it (`switch`) or
/// make it the one the next start uses (`boot`), as nixos-rebuild does:
/// inside the independent system operation, with persistent output files.
fn activate(setup: &Setup, system: &str, mode: &str) -> Result<()> {
    let mut set = command("nix-env");
    set.args(["-p", PROFILE, "--set", system]);
    run(set, "Setting the system profile")?;
    // The configuration that built it, for going back to it later.
    if let Some(generation) = current_generation() {
        let _ = save_configuration(setup, generation, true);
    }
    switch(&format!("{system}/bin/switch-to-configuration"), mode)
}

type Entries = std::collections::BTreeMap<String, Option<PathBuf>>;

/// What a system gives its users to launch: the entries in the application
/// folder of everyone's packages ("") and of each user's own, each with the
/// store file it is.
fn launchable(system: &Path) -> std::collections::BTreeMap<String, Entries> {
    let entries = |folder: PathBuf| -> Entries {
        std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            // Generated files such as mimeinfo.cache differ in every new
            // folder; the launcher reads only the entries.
            .filter(|e| e.file_name().to_string_lossy().ends_with(".desktop"))
            .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::canonicalize(e.path()).ok()))
            .collect()
    };
    let mut folders = std::collections::BTreeMap::new();
    folders.insert(String::new(), entries(system.join("sw/share/applications")));
    if let Ok(users) = std::fs::read_dir(system.join("etc/profiles/per-user")) {
        for user in users.flatten() {
            let name = user.file_name().to_string_lossy().into_owned();
            folders.insert(name, entries(user.path().join("share/applications")));
        }
    }
    folders
}

/// After a live switch that added, removed or changed an application entry,
/// for everyone or for one user, restart Tatami's
/// launcher backend, Elephant, in each signed-in user's session. Elephant
/// watches the application folders it found when the session started. On
/// NixOS it reaches them through /run/current-system and each user's profile,
/// which a switch points at new store paths, so the folders it watches never
/// change and it would not see what was installed or removed until the next
/// sign-in. Walker reconnects to it by itself. A session without Tatami has
/// no such unit, and `try-restart` leaves a stopped one stopped.
fn refresh_launchers(before: &Path, after: &Path) {
    if launchable(before) == launchable(after) {
        return;
    }
    let mut list = command("loginctl");
    list.args(["list-users", "--no-legend"]);
    let Ok(users) = output(list, "Listing signed-in users") else {
        return;
    };
    // UID USER LINGER STATE
    for user in users.lines().filter_map(|line| line.split_whitespace().nth(1)).filter(|u| valid_user(u)) {
        let mut restart = command("systemctl");
        restart.args(["--user", &format!("--machine={user}@.host"), "try-restart", "tatami-elephant.service"]);
        if output(restart, "Refreshing the application launcher").is_err() {
            report(1, &format!("{user}'s application launcher shows the change after their next sign-in"));
        }
    }
}

fn switch(program: &str, mode: &str) -> Result<()> {
    // The entire helper already runs in a system service with persistent logs.
    // Never reconnect this process to the GUI's file descriptors.
    let mut c = command(program);
    c.arg(mode);
    run(c, "Switching to the new system")
}

fn activation_mode(requested: &str, reasons: &[String]) -> Result<&'static str> {
    match requested {
        "boot" => Ok("boot"),
        "switch" if !reasons.is_empty() => Err(Failure(format!(
            "Live activation was refused: {} Prepare the update with --boot or use automatic mode.",
            reasons.join(" ")
        ))),
        "auto" if !reasons.is_empty() => Ok("boot"),
        _ => Ok("switch"),
    }
}

/// What `change` was asked to do.
#[derive(Debug, Default, PartialEq, Eq)]
struct Change {
    packages: Option<Vec<String>>,
    programs: Option<Vec<String>>,
    setting: Option<String>,
    applications: Option<Vec<String>>,
    unlist_system: Vec<String>,
    unlist_user: Vec<(String, Vec<String>)>,
    update: Vec<String>,
    lock: Option<PathBuf>,
    /// An input to follow another branch, and the branch.
    follow: Option<(String, String)>,
    /// The system's own choices to write (`--choose setting=a,b`), each a
    /// setting the distribution declares, to values it declares.
    choices: Vec<(String, Vec<String>)>,
    channels: bool,
    sekka: bool,
    mode: String,
}

impl Change {
    fn parse(args: &[String]) -> Result<Change> {
        let mut change = Change { mode: "auto".into(), ..Change::default() };
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            if flag == "--sekka" {
                change.sekka = true;
                continue;
            }
            if flag == "--channels" {
                change.channels = true;
                continue;
            }
            let value = it.next().ok_or_else(|| Failure(format!("{flag} needs a value")))?;
            match flag.as_str() {
                "--packages" => change.packages = Some(split_list(value)),
                "--programs" => change.programs = Some(split_list(value)),
                "--setting" => change.setting = Some(value.clone()),
                "--applications" => change.applications = Some(split_list(value)),
                "--unlist-system" => change.unlist_system = split_list(value),
                "--unlist-user" => {
                    let (user, attrs) =
                        value.split_once(':').ok_or_else(|| Failure("--unlist-user needs name:a,b".into()))?;
                    change.unlist_user.push((user.to_owned(), split_list(attrs)));
                }
                "--update" => change.update = split_list(value),
                "--lock" => change.lock = Some(PathBuf::from(value)),
                "--follow" => {
                    let (input, branch) =
                        value.split_once('=').ok_or_else(|| Failure("--follow needs input=branch".into()))?;
                    if !packages::valid_identifier(input) || !branch::valid(branch) {
                        return Err(Failure(format!("Not an input and a branch: {value:?}")));
                    }
                    change.follow = Some((input.to_owned(), branch.to_owned()));
                }
                "--choose" => {
                    let (setting, values) =
                        value.split_once('=').ok_or_else(|| Failure("--choose needs setting=value[,value]".into()))?;
                    if !valid_setting(setting) || change.choices.iter().any(|(s, _)| s == setting) {
                        return Err(Failure(format!("Not a setting to choose: {setting:?}")));
                    }
                    let values = split_list(values);
                    if let Some(bad) = values.iter().find(|v| !catalog::valid_id(v)) {
                        return Err(Failure(format!("Not a choice: {bad:?}")));
                    }
                    change.choices.push((setting.to_owned(), values));
                }
                "--mode" => change.mode = value.clone(),
                other => return Err(Failure(format!("Unknown option {other}"))),
            }
        }
        if !matches!(change.mode.as_str(), "auto" | "switch" | "boot") {
            return Err(Failure(format!("Unknown mode {}", change.mode)));
        }
        let attrs = change.unlist_system.iter().chain(change.unlist_user.iter().flat_map(|(_, a)| a));
        if let Some(bad) = change.update.iter().chain(attrs).find(|i| !packages::valid_attribute(i)) {
            return Err(Failure(format!("Not an attribute name: {bad:?}")));
        }
        if let Some((bad, _)) = change.unlist_user.iter().find(|(user, _)| !valid_user(user)) {
            return Err(Failure(format!("Not a user name: {bad:?}")));
        }
        if let Some(bad) = change.applications.iter().flatten().find(|a| !catalog::valid_id(a)) {
            return Err(Failure(format!("Not an application id: {bad:?}")));
        }
        if change.applications.is_some() != change.setting.is_some() {
            return Err(Failure("--applications goes with --setting".into()));
        }
        if change.lock.is_some() && change.update.is_empty() {
            return Err(Failure("--lock goes with --update".into()));
        }
        if change.sekka {
            // The system's choices and Sekka's own branch may change with it.
            let other_follow = change.follow.as_ref().is_some_and(|(input, _)| input != "sekka");
            let other_edits = change.packages.is_some()
                || change.programs.is_some()
                || change.applications.is_some()
                || !change.unlist_system.is_empty()
                || !change.unlist_user.is_empty();
            if change.channels || other_follow || change.lock.is_some() || other_edits {
                return Err(Failure("--sekka only goes with --mode, --update, --choose and --follow sekka=…".into()));
            }
            if !change.update.iter().any(|i| i == "sekka") {
                change.update.push("sekka".into());
            }
        }
        // An input that follows another branch is updated too, by the helper:
        // a lock file prepared beforehand would be for the old branch.
        if let Some((input, _)) = &change.follow {
            if change.lock.is_some() {
                return Err(Failure("--follow can't go with --lock".into()));
            }
            if !change.update.contains(input) {
                change.update.push(input.clone());
            }
        }
        Ok(change)
    }

    /// Whether it changes the main configuration file.
    fn edits_main(&self) -> bool {
        self.packages.is_some()
            || self.programs.is_some()
            || self.applications.is_some()
            || !self.unlist_system.is_empty()
            || !self.unlist_user.is_empty()
            || !self.choices.is_empty()
    }
}

/// A dotted setting name of plain identifiers: `calamares.loginScreen`.
fn valid_setting(name: &str) -> bool {
    name.len() <= 100
        && name.split('.').all(|part| {
            part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// The main configuration file with one of the system's choices written as
/// it is declared: a list of strings, a boolean, or a string.
fn write_choice(text: &str, choice: &Choice, values: &[String]) -> Result<String> {
    let path: Vec<&str> = choice.setting.split('.').collect();
    Ok(if choice.many {
        edit::set_string_list(text, &path, values)?
    } else if choice.boolean {
        edit::set_bool(text, &path, values[0] == "true")?
    } else {
        edit::set_string(text, &path, &values[0])?
    })
}

/// The main configuration file with an entry taken out of a package list.
fn unlist(text: &str, path: &[&str], attr: &str) -> Result<String> {
    edit::remove_package(text, path, attr)?
        .ok_or_else(|| Failure(format!("{attr} isn't in {} in the configuration", path.join("."))))
}

fn change(args: &[String]) -> Result<String> {
    let change = Change::parse(args)?;
    let setup = Setup::detect();
    // Validate migration before touching any files. The GUI's detection is
    // only a convenience; the privileged helper always checks again.
    let sekka_flake = if change.sekka {
        if setup.kind != Kind::Flake {
            return Err(Failure("This installation has no Sekka flake to update".into()));
        }
        Some(sekka::prepare(&std::fs::read_to_string(setup.dir.join("flake.nix"))?)?)
    } else {
        None
    };
    if setup.kind == Kind::Flake && change.channels {
        return Err(Failure("This system is a flake; it has no channels to update".into()));
    }
    if setup.kind == Kind::Channels && !change.update.is_empty() {
        return Err(Failure("This system isn't a flake; it has no inputs to update".into()));
    }
    if let Some(setting) = &change.setting
        && !setup.facts.catalogs.iter().any(|c| c.setting.as_ref() == Some(setting))
    {
        return Err(Failure(format!("{setting} isn't a setting a catalog of this system installs through")));
    }
    // Only settings the distribution declares, to values it declares.
    for (setting, values) in &change.choices {
        let declared = setup
            .facts
            .choices
            .iter()
            .find(|c| &c.setting == setting)
            .ok_or_else(|| Failure(format!("{setting} isn't a choice this system offers")))?;
        declared.allows(values).map_err(Failure)?;
    }
    let main = setup.main.clone();
    let packages_file = setup.packages_file();
    if change.edits_main() && main.is_none() {
        return Err(Failure(format!(
            "Yukimi couldn't tell which file in {} is this computer's configuration",
            setup.dir.display()
        )));
    }

    // The configuration as it is belongs to the running generation: keep it,
    // so going back to that generation brings it back.
    if let Some(generation) = current_generation() {
        let saved = save_configuration(&setup, generation, false);
        if change.sekka {
            saved?;
        }
    }
    let lock_file = setup.dir.join("flake.lock");
    let flake_file = setup.dir.join("flake.nix");
    let mut touched = vec![lock_file.clone(), flake_file.clone()];
    touched.extend(main.clone());
    touched.extend(packages_file.clone());
    let backup = Backup::take(&touched);
    let attempt = || -> Result<String> {
        if let Some(text) = &sekka_flake {
            stage(1, "Preparing your Sekka configuration");
            write_file(&setup, &flake_file, text)?;
            stage_done(1);
        }
        if let (Some(main), true) = (&main, change.edits_main()) {
            stage(1, "Writing your configuration");
            let mut configuration = std::fs::read_to_string(main)?;
            if change.packages.is_some() || change.programs.is_some() {
                let file = packages_file.clone().ok_or_else(|| Failure("No place for yukimi.nix".into()))?;
                let mut choices = std::fs::read_to_string(&file)
                    .ok()
                    .map(|text| packages::read(&text))
                    .transpose()?
                    .unwrap_or_default();
                if let Some(list) = &change.packages {
                    choices.packages = list.clone();
                }
                if let Some(list) = &change.programs {
                    choices.programs = list.clone();
                }
                write_file(&setup, &file, &packages::render(&choices)?)?;
                configuration = packages::import_into(&configuration)?;
            }
            if let (Some(setting), Some(list)) = (&change.setting, &change.applications) {
                let path: Vec<&str> = setting.split('.').collect();
                configuration = edit::set_string_list(&configuration, &path, list)?;
            }
            for (setting, values) in &change.choices {
                // Checked above: each is declared and allows its values.
                let declared = setup.facts.choices.iter().find(|c| &c.setting == setting);
                if let Some(choice) = declared {
                    configuration = write_choice(&configuration, choice, values)?;
                }
            }
            for attr in &change.unlist_system {
                configuration = unlist(&configuration, &["environment", "systemPackages"], attr)?;
            }
            for (user, attrs) in &change.unlist_user {
                for attr in attrs {
                    configuration = unlist(&configuration, &["users", "users", user, "packages"], attr)?;
                }
            }
            write_file(&setup, main, &configuration)?;
            stage_done(1);
        }
        if let Some((input, branch)) = &change.follow {
            stage(5, &format!("Following {branch} for {input}"));
            let text = std::fs::read_to_string(&flake_file)?;
            let path = ["inputs", input.as_str(), "url"];
            let url = edit::string_value(&text, &path)?.ok_or_else(|| {
                Failure(format!(
                    "flake.nix doesn't give {input}'s address as inputs.{input}.url, so Yukimi can't change it"
                ))
            })?;
            // Sekka's main, chosen: early updates, which a Sekka update keeps.
            let followed = if input == "sekka" && branch == "main" {
                sekka::EARLY.to_owned()
            } else {
                branch::with(&url, branch)
                    .ok_or_else(|| Failure(format!("{input} comes from {url}, which has no branch to choose")))?
            };
            write_file(&setup, &flake_file, &edit::set_string_value(&text, &path, &followed)?)?;
            stage_done(5);
        }
        if !change.update.is_empty() {
            stage(2, "Fetching the newest versions");
            match &change.lock {
                // Prepared by Yukimi, with every new source already in the store.
                Some(prepared) => {
                    let proposed = std::fs::read_to_string(prepared)?;
                    let current = std::fs::read_to_string(&lock_file)?;
                    lock::check_update(&current, &proposed, &change.update)
                        .map_err(|e| Failure(format!("The prepared update was refused: {e}")))?;
                    std::fs::write(&lock_file, proposed)?;
                }
                None => {
                    // Inputs that follow Sekka's move with it, to what its
                    // nightly build checked; on their own they can't move.
                    let following = sekka_flake.as_deref().map(sekka::following).unwrap_or_default();
                    let mut c = nix();
                    let flake = setup.flake();
                    c.args(["flake", "update", "--refresh", "--log-format", "internal-json", "-v", "--flake", &flake]);
                    c.args(change.update.iter().filter(|i| !following.contains(&i.as_str())));
                    run(c, "Updating the flake inputs")?;
                }
            }
            stage_done(2);
        }
        if change.channels {
            stage(2, "Fetching the newest channels");
            let mut c = command("nix-channel");
            c.arg("--update");
            run(c, "Updating the channels")?;
            stage_done(2);
        }
        stage(3, "Evaluating your configuration");
        let system = build(&setup)?;
        stage_done(3);
        Ok(system)
    };
    let (system, reasons, mode) = match attempt().and_then(|system| {
        let active = std::fs::canonicalize("/run/current-system")?;
        let booted =
            std::fs::canonicalize("/run/booted-system").unwrap_or_else(|_| PathBuf::from("/run/booted-system"));
        let reasons = completion::plan(&booted, &active, Path::new(&system));
        let mode = activation_mode(&change.mode, &reasons)?;
        commit()?;
        Ok((system, reasons, mode))
    }) {
        Ok(prepared) => prepared,
        Err(e) => {
            backup.restore();
            return Err(e);
        }
    };
    let mut receipt =
        Receipt { system: PathBuf::from(&system), boot_id: completion::boot_id(), state: "activating".into(), reasons };
    save_update(&receipt)?;
    stage(4, if mode == "switch" { "Switching to the new system" } else { "Setting it up for the next start" });
    // Only for refreshing the launcher afterwards, which never fails an update.
    let before = std::fs::canonicalize("/run/current-system").ok();
    activate(&setup, &system, mode)?;
    if mode == "switch" && std::fs::canonicalize("/run/current-system").ok().as_deref() != Some(Path::new(&system)) {
        return Err(Failure("Activation returned without making the built system current".into()));
    }
    receipt.state = if mode == "boot" { "prepared" } else { "activated" }.into();
    save_update(&receipt)?;
    if let (Some(before), "switch") = (&before, mode) {
        refresh_launchers(before, Path::new(&system));
    }
    stage_done(4);
    Ok(serde_json::json!({"ok": true, "system": system, "mode": mode, "completion": completion::read_system()})
        .to_string())
}

fn save_update(receipt: &Receipt) -> Result<()> {
    use std::io::Write;
    let path = Path::new(completion::RECEIPT);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension("new");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(&serde_json::to_vec(receipt)?)?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    std::fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}

fn rollback(args: &[String]) -> Result<String> {
    let generation: u32 = args
        .first()
        .and_then(|g| g.parse().ok())
        .ok_or_else(|| Failure("rollback needs a generation number".into()))?;
    if Path::new(&format!("{PROFILE}-{generation}-link")).symlink_metadata().is_err() {
        return Err(Failure(format!("There is no generation {generation}")));
    }
    let setup = Setup::detect();
    let target = std::fs::canonicalize(format!("{PROFILE}-{generation}-link"))?;
    let active = std::fs::canonicalize("/run/current-system")?;
    let booted = std::fs::canonicalize("/run/booted-system")?;
    let reasons = completion::plan(&booted, &active, &target);
    let mode = activation_mode("auto", &reasons)?;
    let running = current_generation();
    if let Some(current) = running {
        let _ = save_configuration(&setup, current, false);
    }
    stage(1, &format!("Returning to generation {generation}"));
    commit()?;
    let mut set = command("nix-env");
    set.args(["-p", PROFILE, "--switch-generation", &generation.to_string()]);
    run(set, "Choosing the generation")?;
    let mut receipt = Receipt {
        system: std::fs::canonicalize(PROFILE)?,
        boot_id: completion::boot_id(),
        state: "activating".into(),
        reasons,
    };
    save_update(&receipt)?;
    // The configuration follows the system, so the next change builds on
    // what this generation was made from.
    let kept = saved(generation).is_dir();
    let mut left = Vec::new();
    if kept {
        let running = running.map(saved).filter(|dir| dir.is_dir());
        left = restore_choices(&setup, &saved(generation), running.as_deref());
        for file in &left {
            report(1, &format!("{} was changed by hand since, so it was left as it is", file.display()));
        }
    }
    switch(&format!("{PROFILE}/bin/switch-to-configuration"), mode)?;
    if mode == "switch" && std::fs::canonicalize("/run/current-system").ok().as_ref() != Some(&receipt.system) {
        return Err(Failure("Rollback did not make the selected generation current".into()));
    }
    receipt.state = if mode == "boot" { "prepared" } else { "activated" }.into();
    save_update(&receipt)?;
    if mode == "switch" {
        refresh_launchers(&active, &receipt.system);
    }
    stage_done(1);
    let left: Vec<String> = left.iter().map(|f| f.display().to_string()).collect();
    Ok(serde_json::json!({"ok": true, "generation": generation, "configuration": kept, "left": left,
        "system": receipt.system, "mode": mode, "completion": completion::read_system()})
    .to_string())
}

fn clean(args: &[String]) -> Result<String> {
    let mut days = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--older-than-days" => {
                days = Some(
                    it.next()
                        .and_then(|d| d.parse::<u32>().ok())
                        .filter(|d| *d > 0)
                        .ok_or_else(|| Failure("--older-than-days needs a number of days".into()))?,
                )
            }
            other => return Err(Failure(format!("Unknown option {other}"))),
        }
    }
    if let Some(days) = days {
        stage(1, "Deleting old generations");
        let mut delete = command("nix-env");
        delete.args(["-p", PROFILE, "--delete-generations", &format!("{days}d")]);
        run(delete, "Deleting old generations")?;
        prune_saved();
        stage_done(1);
    }
    stage(2, "Collecting garbage");
    let mut gc = command("nix-store");
    gc.arg("--gc");
    let out = wait(gc.stdout(Stdio::piped()).stderr(Stdio::piped()))?;
    let text = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        eprintln!("{line}");
    }
    if !out.status.success() {
        return Err(Failure("Collecting garbage failed".into()));
    }
    stage_done(2);
    // The boot menu lists only generations that still exist.
    stage(3, "Updating the boot menu");
    commit()?;
    switch(&format!("{PROFILE}/bin/switch-to-configuration"), "boot")?;
    stage_done(3);
    let freed = text.lines().rev().find(|l| l.contains("freed")).unwrap_or("").trim().to_owned();
    Ok(serde_json::json!({"ok": true, "freed": freed}).to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // A target-side pre-switch check is read-only and also protects legacy
    // clients which still explicitly request a live switch.
    if args.first().is_some_and(|s| s == "check-activation") {
        return match check_activation(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(Failure(message)) => {
                let _ = writeln!(std::io::stderr(), "{message}");
                ExitCode::FAILURE
            }
        };
    }
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        report(0, "yukimi-helper must be started through pkexec");
        return ExitCode::from(2);
    }
    if args.first().is_some_and(|s| s == "__worker") {
        let Some((directory, args)) = args.get(1..).and_then(|args| args.split_first()) else {
            return ExitCode::FAILURE;
        };
        let Ok(directory) = transaction::worker_directory(directory) else {
            return ExitCode::FAILURE;
        };
        let result = worker(args);
        let (json, success) = match result {
            Ok(json) => (json, true),
            Err(Failure(message)) => {
                let message = if state().0 == Phase::Committed {
                    format!(
                        "System changes had already begun; the configuration was not restored. {message}. Review the log before restarting or retrying."
                    )
                } else {
                    message
                };
                report(0, &message);
                (serde_json::json!({"ok": false, "error": message}).to_string(), false)
            }
        };
        if transaction::save_result(&directory, &json).is_err() {
            return ExitCode::FAILURE;
        }
        return if success { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    let result = if args.first().is_some_and(|s| matches!(s.as_str(), "change" | "rollback" | "clean")) {
        transaction::client(&args)
    } else {
        Err(Failure("Usage: yukimi-helper change|rollback|clean …".into()))
    };
    let json = match result {
        Ok(json) => json,
        Err(Failure(message)) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            serde_json::json!({"ok": false, "error": message}).to_string()
        }
    };
    let success = serde_json::from_str::<serde_json::Value>(&json).is_ok_and(|v| v["ok"] == true);
    let _ = writeln!(std::io::stdout(), "{json}");
    if success { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn check_activation(args: &[String]) -> Result<()> {
    let [target, action] = args else {
        return Err(Failure("Invalid activation check".into()));
    };
    if !matches!(action.as_str(), "switch" | "test") {
        return Ok(());
    }
    let target = std::fs::canonicalize(target)?;
    let active = std::fs::canonicalize("/run/current-system")?;
    let booted = std::fs::canonicalize("/run/booted-system")?;
    activation_mode("switch", &completion::plan(&booted, &active, &target))?;
    Ok(())
}

fn worker(args: &[String]) -> Result<String> {
    let (command, rest) = args.split_first().ok_or_else(|| Failure("Missing operation".into()))?;
    // CLI and GUI must not edit the configuration or activate generations
    // concurrently. The kernel releases this lock even after a crash.
    let _guard = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open("/run/lock/yukimi.lock")
        .and_then(|file| {
            file.try_lock()?;
            Ok(file)
        })
        .map_err(|e| Failure(format!("Another Yukimi operation is running, or its lock is unavailable: {e}")))?;
    watch_for_stop();
    match command.as_str() {
        "change" => change(rest),
        "rollback" => rollback(rest),
        "clean" => clean(rest),
        _ => Err(Failure("Usage: yukimi-helper change|rollback|clean …".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn lists_and_names() {
        assert_eq!(split_list("a, b,,c"), vec!["a", "b", "c"]);
        assert!(split_list("").is_empty());
        assert!(valid_host("acer-predator") && !valid_host("a b") && !valid_host("x\"y"));
        assert!(valid_user("alice") && !valid_user("a:b") && !valid_user(""));
        assert_eq!(generation_number("system-12-link"), Some(12));
        assert_eq!(generation_number("system"), None);
    }

    #[test]
    fn changes_are_checked() {
        assert_eq!(
            Change::parse(&args("--sekka --update nixpkgs,sekka --mode auto")).unwrap().update,
            vec!["nixpkgs", "sekka"]
        );
        assert_eq!(Change::parse(&args("--sekka --mode boot")).unwrap().update, vec!["sekka"]);
        for bad in [
            "--sekka --channels",
            "--sekka --follow tatami=main",
            "--sekka --packages htop",
            "--sekka --update sekka --lock /tmp/x",
        ] {
            assert!(Change::parse(&args(bad)).is_err(), "{bad}");
        }
        let change = Change::parse(&args(
            "--packages htop,git --programs steam --unlist-system vim --unlist-user alice:thunderbird,kdePackages.kate \
             --update nixpkgs --lock /tmp/x.lock --mode boot",
        ))
        .unwrap();
        assert_eq!(change.programs, Some(vec!["steam".to_owned()]));
        assert_eq!(
            change.unlist_user,
            vec![("alice".to_owned(), vec!["thunderbird".into(), "kdePackages.kate".into()])]
        );
        assert_eq!(change.lock.as_deref(), Some(Path::new("/tmp/x.lock")));
        assert!(change.edits_main());
        assert!(Change::parse(&args("--channels")).unwrap().channels);
        // A Sekka update may also choose early updates and the system's choices.
        let early = Change::parse(&args("--sekka --follow sekka=main --choose calamares.loginScreen=aizome")).unwrap();
        assert_eq!(early.follow, Some(("sekka".to_owned(), "main".to_owned())));
        assert_eq!(early.choices, vec![("calamares.loginScreen".to_owned(), vec!["aizome".to_owned()])]);
        assert!(early.edits_main());
        let desktops = Change::parse(&args("--choose calamares.desktops=aizome,tatami")).unwrap();
        assert_eq!(desktops.choices[0].1, vec!["aizome", "tatami"]);
        for bad in [
            "--choose calamares.loginScreen",
            "--choose calamares..x=a",
            "--choose calamares.loginScreen=a;b",
            "--choose calamares.loginScreen=Aizome",
            "--choose calamares.loginScreen=a --choose calamares.loginScreen=b",
            "--choose 1x.y=a",
        ] {
            assert!(Change::parse(&args(bad)).is_err(), "{bad}");
        }
        let follow = Change::parse(&args("--follow tatami=main")).unwrap();
        assert_eq!(follow.follow, Some(("tatami".to_owned(), "main".to_owned())));
        assert_eq!(follow.update, vec!["tatami".to_owned()]);
        for bad in [
            "--mode reboot",
            "--unlist-system a;b",
            "--unlist-user root;x:a",
            "--applications firefox",
            "--setting calamares.applications --applications Bad",
            "--lock /tmp/x.lock",
            "--follow tatami",
            "--follow tatami=a..b",
            "--follow x;y=main",
            "--update tatami --lock /tmp/x.lock --follow tatami=main",
            "--packages",
        ] {
            assert!(Change::parse(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn choices_are_written_as_declared() {
        let choice = |json: &str| serde_json::from_str::<Choice>(json).unwrap();
        let splash = choice(
            r#"{"setting":"calamares.bootSplash.enable","title":"Boot splash","boolean":true,"value":"false",
                "options":[{"id":"true","title":"On"},{"id":"false","title":"Off"}]}"#,
        );
        let login = choice(
            r#"{"setting":"calamares.loginScreen","title":"Login screen","value":"auto",
                "options":[{"id":"auto","title":"Automatic"},{"id":"aizome","title":"Aizome"}]}"#,
        );
        let desktops = choice(
            r#"{"setting":"calamares.desktops","title":"Desktops","many":true,"value":["aizome"],
                "options":[{"id":"aizome","title":"Aizome"},{"id":"gnome","title":"GNOME"}]}"#,
        );
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let installed = "{ ... }:\n{\n  calamares.desktops = [ \"aizome\" ];\n}\n";
        let text = write_choice(installed, &splash, &v(&["true"])).unwrap();
        let text = write_choice(&text, &login, &v(&["aizome"])).unwrap();
        let text = write_choice(&text, &desktops, &v(&["aizome", "gnome"])).unwrap();
        assert_eq!(
            text,
            "{ ... }:\n{\n  calamares.desktops = [ \"aizome\" \"gnome\" ];\n  calamares.bootSplash.enable = true;\n  \
             calamares.loginScreen = \"aizome\";\n}\n"
        );
        let off = write_choice(&text, &splash, &v(&["false"])).unwrap();
        assert_eq!(off, text.replace("enable = true", "enable = false"));
    }

    #[test]
    fn configurations_copy_without_hidden_or_large_files() {
        let dir = std::env::temp_dir().join(format!("yukimi-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (from, to) = (dir.join("nixos"), dir.join("saved"));
        std::fs::create_dir_all(from.join("modules")).unwrap();
        std::fs::create_dir_all(from.join(".git")).unwrap();
        std::fs::write(from.join("configuration.nix"), "{ }").unwrap();
        std::fs::write(from.join("modules/desk.nix"), "{ }").unwrap();
        std::fs::write(from.join(".git/HEAD"), "ref").unwrap();
        std::fs::write(from.join("wallpaper.png"), vec![0u8; (LARGEST_KEPT + 1) as usize]).unwrap();
        copy_configuration(&from, &to).unwrap();
        assert_eq!(std::fs::read_to_string(to.join("configuration.nix")).unwrap(), "{ }");
        assert!(to.join("modules/desk.nix").is_file());
        assert!(!to.join(".git").exists() && !to.join("wallpaper.png").exists());
        // A private file stays private.
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(from.join("secrets.nix"), "{ }").unwrap();
        std::fs::set_permissions(from.join("secrets.nix"), std::fs::Permissions::from_mode(0o600)).unwrap();
        copy_configuration(&from, &to).unwrap();
        assert_eq!(std::fs::metadata(to.join("secrets.nix")).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backups_restore_contents_and_absence() {
        let dir = std::env::temp_dir().join(format!("yukimi-backup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (kept, created) = (dir.join("kept"), dir.join("created"));
        std::fs::write(&kept, "before").unwrap();
        let backup = Backup::take(&[kept.clone(), created.clone()]);
        std::fs::write(&kept, "after").unwrap();
        std::fs::write(&created, "new").unwrap();
        backup.restore();
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "before");
        assert!(!created.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn going_back_keeps_what_was_written_by_hand() {
        let dir = std::env::temp_dir().join(format!("yukimi-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (config, older, running) = (dir.join("nixos"), dir.join("generation-1"), dir.join("generation-2"));
        for d in [&config, &older, &running] {
            std::fs::create_dir_all(d).unwrap();
        }
        // Migration changes flake.nix too; it must go back with its lock.
        std::fs::write(older.join("flake.nix"), "old repositories").unwrap();
        std::fs::write(running.join("flake.nix"), "sekka").unwrap();
        std::fs::write(config.join("flake.nix"), "sekka").unwrap();
        // flake.lock: unchanged since generation 2, so it goes back to 1's.
        std::fs::write(older.join("flake.lock"), "lock 1").unwrap();
        std::fs::write(running.join("flake.lock"), "lock 2").unwrap();
        std::fs::write(config.join("flake.lock"), "lock 2").unwrap();
        // configuration.nix: edited by hand since generation 2, so it stays.
        std::fs::write(older.join("configuration.nix"), "{ a = 1; }").unwrap();
        std::fs::write(running.join("configuration.nix"), "{ a = 2; }").unwrap();
        std::fs::write(config.join("configuration.nix"), "{ a = 3; }").unwrap();
        // yukimi.nix: generation 1 had none.
        std::fs::write(running.join("yukimi.nix"), "{ }").unwrap();
        std::fs::write(config.join("yukimi.nix"), "{ }").unwrap();
        let setup = Setup::detect_in(&config, Default::default(), "acer");
        let left = restore_choices(&setup, &older, Some(&running));
        assert_eq!(std::fs::read_to_string(config.join("flake.nix")).unwrap(), "old repositories");
        assert_eq!(std::fs::read_to_string(config.join("flake.lock")).unwrap(), "lock 1");
        assert_eq!(std::fs::read_to_string(config.join("configuration.nix")).unwrap(), "{ a = 3; }");
        assert!(!config.join("yukimi.nix").exists());
        assert_eq!(left, vec![config.join("configuration.nix")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
