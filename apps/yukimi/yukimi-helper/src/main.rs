// SPDX-License-Identifier: MIT OR Apache-2.0
//! `yukimi-helper`: the part of Yukimi that changes the whole system. It
//! runs as root through `pkexec`, so it does only a few fixed things, with
//! arguments it checks itself, on the configuration it finds itself (see
//! `yukimi_system::setup`):
//!
//! - `change [options] --mode switch|boot` changes what the system installs
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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;

use yukimi_config::packages;
use yukimi_config::{branch, edit, lock};
use yukimi_system::catalog;
use yukimi_system::setup::{Kind, Setup};

const PROFILE: &str = "/nix/var/nix/profiles/system";
/// pkexec starts programs with an almost empty environment.
const PATH: &str = "/run/current-system/sw/bin:/run/wrappers/bin";
/// Where each generation's configuration is kept, as `generation-<n>`.
const SAVED: &str = yukimi_system::SAVED_CONFIGURATIONS;
/// Files bigger than this aren't configuration, and aren't kept with it.
const LARGEST_KEPT: u64 = 1 << 20;

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

fn state() -> std::sync::MutexGuard<'static, (Phase, Option<u32>)> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Stop when standard input closes, if it is a pipe: that is how Yukimi
/// asks. Standard input that is a terminal or /dev/null means nothing.
fn watch_for_stop() {
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
        let mut state = state();
        if state.0 == Phase::Committed {
            return;
        }
        state.0 = Phase::Stopping;
        if let Some(pid) = state.1 {
            // SAFETY: signalling a process has no memory-safety preconditions.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
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
    let mut files = vec![setup.dir.join("flake.lock")];
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

/// Build a flake's system for `host`.
fn build_flake(setup: &Setup, host: &str) -> std::result::Result<String, Failure> {
    let mut build = nix();
    build.args(["build", "--log-format", "internal-json", "-v", "--no-link", "--print-out-paths"]);
    build.arg(format!("{}#nixosConfigurations.\"{host}\".config.system.build.toplevel", setup.flake()));
    Ok(output(build, "Building the new system")?.trim().to_owned())
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
        return Ok(output(build, "Building the new system")?.trim().to_owned());
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
/// through systemd-run, so restarting services cannot stop it halfway.
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

fn switch(program: &str, mode: &str) -> Result<()> {
    let mut c = command("systemd-run");
    c.args([
        "-E",
        "LOCALE_ARCHIVE",
        "-E",
        "NIXOS_INSTALL_BOOTLOADER",
        "--collect",
        "--no-ask-password",
        "--pipe",
        "--quiet",
        "--service-type=exec",
        "--unit=yukimi-switch-to-configuration",
        "--wait",
        program,
        mode,
    ]);
    run(c, "Switching to the new system")
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
    channels: bool,
    mode: String,
}

impl Change {
    fn parse(args: &[String]) -> Result<Change> {
        let mut change = Change { mode: "switch".into(), ..Change::default() };
        let mut it = args.iter();
        while let Some(flag) = it.next() {
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
                "--mode" => change.mode = value.clone(),
                other => return Err(Failure(format!("Unknown option {other}"))),
            }
        }
        if !matches!(change.mode.as_str(), "switch" | "boot") {
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
    }
}

/// The main configuration file with an entry taken out of a package list.
fn unlist(text: &str, path: &[&str], attr: &str) -> Result<String> {
    edit::remove_package(text, path, attr)?
        .ok_or_else(|| Failure(format!("{attr} isn't in {} in the configuration", path.join("."))))
}

fn change(args: &[String]) -> Result<String> {
    let change = Change::parse(args)?;
    let setup = Setup::detect();
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
        let _ = save_configuration(&setup, generation, false);
    }
    let lock_file = setup.dir.join("flake.lock");
    let flake_file = setup.dir.join("flake.nix");
    let mut touched = vec![lock_file.clone(), flake_file.clone()];
    touched.extend(main.clone());
    touched.extend(packages_file.clone());
    let backup = Backup::take(&touched);
    let attempt = || -> Result<String> {
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
            let followed = branch::with(&url, branch)
                .ok_or_else(|| Failure(format!("{input} comes from {url}, which has no branch to choose")))?;
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
                    let mut c = nix();
                    let flake = setup.flake();
                    c.args(["flake", "update", "--refresh", "--log-format", "internal-json", "-v", "--flake", &flake]);
                    c.args(&change.update);
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
    let system = match attempt().and_then(|system| commit().map(|_| system)) {
        Ok(system) => system,
        Err(e) => {
            backup.restore();
            return Err(e);
        }
    };
    let mode = change.mode.as_str();
    stage(4, if mode == "switch" { "Switching to the new system" } else { "Setting it up for the next start" });
    activate(&setup, &system, mode)?;
    stage_done(4);
    Ok(serde_json::json!({"ok": true, "system": system}).to_string())
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
    let running = current_generation();
    if let Some(current) = running {
        let _ = save_configuration(&setup, current, false);
    }
    stage(1, &format!("Returning to generation {generation}"));
    commit()?;
    let mut set = command("nix-env");
    set.args(["-p", PROFILE, "--switch-generation", &generation.to_string()]);
    run(set, "Choosing the generation")?;
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
    switch(&format!("{PROFILE}/bin/switch-to-configuration"), "switch")?;
    stage_done(1);
    let left: Vec<String> = left.iter().map(|f| f.display().to_string()).collect();
    Ok(serde_json::json!({"ok": true, "generation": generation, "configuration": kept, "left": left}).to_string())
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
    let (command, rest) = match args.split_first() {
        Some((c, rest)) => (c.as_str(), rest),
        None => ("", &[][..]),
    };
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        report(0, "yukimi-helper must be started through pkexec");
        return ExitCode::from(2);
    }
    watch_for_stop();
    let result = match command {
        "change" => change(rest),
        "rollback" => rollback(rest),
        "clean" => clean(rest),
        _ => Err(Failure("Usage: yukimi-helper change|rollback|clean …".into())),
    };
    match result {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(Failure(message)) => {
            report(0, &message);
            println!("{}", serde_json::json!({"ok": false, "error": message}));
            ExitCode::FAILURE
        }
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
        assert_eq!(std::fs::read_to_string(config.join("flake.lock")).unwrap(), "lock 1");
        assert_eq!(std::fs::read_to_string(config.join("configuration.nix")).unwrap(), "{ a = 3; }");
        assert!(!config.join("yukimi.nix").exists());
        assert_eq!(left, vec![config.join("configuration.nix")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
