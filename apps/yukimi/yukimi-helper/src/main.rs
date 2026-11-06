// SPDX-License-Identifier: MIT OR Apache-2.0
//! `yukimi-helper`: the part of Yukimi that changes the whole system. It
//! runs as root through `pkexec`, so it does only a few fixed things, with
//! arguments it checks itself, on the configuration it finds itself (see
//! `yukimi_system::setup`):
//!
//! - `change [options] --mode auto|switch|boot` changes what the system installs
//!   and where it comes from, builds the new system and switches to it. If
//!   anything before the switch fails, every file it touched, and root's
//!   channels if it updated them, is put back as it was
//!   ([`yukimi_helper::change`]). The options:
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
//! - `clean [--older-than-days n]` deletes old system generations, updates
//!   the boot menu, and collects garbage ([`yukimi_helper::clean`]).
//! - `delete <generation>... [--collect-garbage]` deletes particular system
//!   generations: never the one selected for the next start, the running
//!   one or the one the computer started with. Their kept configurations go
//!   with them, and the boot menu is made again without them, in the same
//!   step as `clean`'s. What only they kept goes when garbage is collected:
//!   then, with `--collect-garbage`, or at the next clean-up.
//! - `estimate <generation>...`, which needs no administrator, says what
//!   deleting generations would give back (see `yukimi_system::reclaim`).
//!
//! Each generation's configuration is kept: a copy of the configuration's
//! directory in [`SAVED`]`/generation-<n>`, which records the system it was
//! kept for ([`yukimi_helper::snapshots`]). So going back to a generation
//! brings back the choices that made it, and the next change builds on
//! those.
//!
//! Progress goes to standard error in Nix's machine-readable log format,
//! which Yukimi shows; the outcome goes to standard output as JSON. When
//! standard input is a pipe and Yukimi closes it, the helper stops what it
//! is doing and puts the configuration back, unless the new system is
//! already being started, which is never interrupted.
#![forbid(clippy::disallowed_methods)]
#![forbid(unsafe_code)]
use std::io::Write;

mod transaction;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use yukimi_config::packages;
use yukimi_config::sekka;
use yukimi_config::{branch, edit, lock};
use yukimi_helper::change::{Committed, RestorePoint};
use yukimi_helper::clean::{self as cleaning, Generations, generation_words};
use yukimi_helper::process::{
    Failure, Nix, Operation, PATH, Program, Result, command, nix, output, report, stage, stage_done, stop,
    watch_for_stop,
};
use yukimi_helper::{process, snapshots};
use yukimi_system::catalog;
use yukimi_system::completion::{self, Receipt};
use yukimi_system::saved::{GenerationId, Snapshot};
use yukimi_system::setup::{Choice, Kind, Setup};

const PROFILE: &str = yukimi_system::generations::SYSTEM_PROFILE;
/// Where each generation's configuration is kept, as `generation-<n>`.
const SAVED: &str = yukimi_system::SAVED_CONFIGURATIONS;
/// Where Nix keeps the store and builds; the one with least room counts.
const BUILD_PLACES: [&str; 2] = ["/nix", "/tmp"];
/// A new system isn't built with less room than this.
const SPACE_NEEDED: u64 = 10_000_000_000;
/// Building stops when room falls below this, rather than fill the disk.
const SPACE_FLOOR: u64 = 4_000_000_000;

/// The system generation the profile points at now.
fn current_generation() -> Option<GenerationId> {
    GenerationId::current(Path::new(PROFILE))
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
    let mut build = nix(Nix::Build);
    build.args(["--log-format", "internal-json", "-v", "--no-link", "--print-out-paths"]);
    if let Some((substituters, keys)) = caches(&system) {
        build.args(["--option", "extra-substituters", &substituters.join(" ")]);
        build.args(["--option", "extra-trusted-public-keys", &keys.join(" ")]);
    }
    build.arg(format!("{system}.config.system.build.toplevel"));
    Ok(with_room(|| output(build, "Building the new system"))?.trim().to_owned())
}

/// The free space, in bytes, of the filesystem holding `path`.
fn free_space(path: &str) -> Option<u64> {
    yukimi_system::sys::free_space(Path::new(path))
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
    let mut eval = nix(Nix::Eval);
    eval.args(["--json", &format!("{system}.config.nix.settings"), "--apply"]);
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
        let mut build = nix(Nix::Build);
        build.args(["--impure", "--log-format", "internal-json", "-v", "--no-link", "--print-out-paths"]);
        build.args(["--file", "<nixpkgs/nixos>", "system"]);
        build.env("NIXOS_CONFIG", setup.dir.join("configuration.nix"));
        // Nix's own setting says where `<nixpkgs>` is; NixOS before 24.05
        // said so only in the environment, which pkexec clears.
        let configured = output(
            {
                let mut c = nix(Nix::Config);
                c.args(["show", "nix-path"]);
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
    let mut names = nix(Nix::Eval);
    names.args(["--json", &format!("{}#nixosConfigurations", setup.flake()), "--apply", "builtins.attrNames"]);
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
fn activate(committed: &Committed, setup: &Setup, system: &str, mode: &str) -> Result<()> {
    committed.set_system_profile(Path::new(system))?;
    // The configuration that built it, for going back to it later.
    if let Some(generation) = current_generation() {
        let _ = snapshots::save(setup, Path::new(SAVED), &generation);
    }
    committed.switch_to_configuration(Path::new(system), mode)
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
    let mut list = command(Program::LOGINCTL);
    list.args(["list-users", "--no-legend"]);
    let Ok(users) = output(list, "Listing signed-in users") else {
        return;
    };
    // UID USER LINGER STATE
    for user in users.lines().filter_map(|line| line.split_whitespace().nth(1)).filter(|u| valid_user(u)) {
        let mut restart = command(Program::SYSTEMCTL);
        restart.args(["--user", &format!("--machine={user}@.host"), "try-restart", "tatami-elephant.service"]);
        if output(restart, "Refreshing the application launcher").is_err() {
            report(1, &format!("{user}'s application launcher shows the change after their next sign-in"));
        }
    }
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

fn change(operation: &mut Operation, args: &[String]) -> Result<String> {
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
        let saved = snapshots::keep(&setup, Path::new(SAVED), &generation);
        if change.sekka {
            saved?;
        }
    }
    let lock_file = setup.dir.join("flake.lock");
    let flake_file = setup.dir.join("flake.nix");
    let mut touched = vec![lock_file.clone(), flake_file.clone()];
    touched.extend(main.clone());
    touched.extend(packages_file.clone());
    // Everything up to the plan is put back if it fails or Yukimi stops it.
    let (committed, (system, reasons, mode)) = RestorePoint::take(&touched)?.attempt(operation, |changing| {
        if let Some(text) = &sekka_flake {
            stage(1, "Preparing your Sekka configuration");
            changing.write_file(&setup, &flake_file, text)?;
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
                changing.write_file(&setup, &file, &packages::render(&choices)?)?;
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
            changing.write_file(&setup, main, &configuration)?;
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
            changing.write_file(&setup, &flake_file, &edit::set_string_value(&text, &path, &followed)?)?;
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
                    changing.write_file(&setup, &lock_file, &proposed)?;
                }
                None => {
                    // Inputs that follow Sekka's move with it, to what its
                    // nightly build checked; on their own they can't move.
                    let following = sekka_flake.as_deref().map(sekka::following).unwrap_or_default();
                    let inputs: Vec<&str> =
                        change.update.iter().map(String::as_str).filter(|i| !following.contains(i)).collect();
                    changing.update_lock(&setup, &inputs)?;
                }
            }
            stage_done(2);
        }
        if change.channels {
            stage(2, "Fetching the newest channels");
            changing.cover_channels()?.update_channels()?;
            stage_done(2);
        }
        stage(3, "Evaluating your configuration");
        let system = build(&setup)?;
        stage_done(3);
        let active = std::fs::canonicalize("/run/current-system")?;
        let booted =
            std::fs::canonicalize("/run/booted-system").unwrap_or_else(|_| PathBuf::from("/run/booted-system"));
        let reasons = completion::plan(&booted, &active, Path::new(&system));
        let mode = activation_mode(&change.mode, &reasons)?;
        Ok((system, reasons, mode))
    })?;
    let activating = Receipt::activating(PathBuf::from(&system), reasons);
    save_update(activating.receipt())?;
    stage(4, if mode == "switch" { "Switching to the new system" } else { "Setting it up for the next start" });
    // Only for refreshing the launcher afterwards, which never fails an update.
    let before = std::fs::canonicalize("/run/current-system").ok();
    activate(&committed, &setup, &system, mode)?;
    if mode == "switch" && std::fs::canonicalize("/run/current-system").ok().as_deref() != Some(Path::new(&system)) {
        return Err(Failure("Activation returned without making the built system current".into()));
    }
    save_update(&if mode == "boot" { activating.prepared() } else { activating.activated() })?;
    if let (Some(before), "switch") = (&before, mode) {
        refresh_launchers(before, Path::new(&system));
    }
    stage_done(4);
    Ok(serde_json::json!({"ok": true, "system": system, "mode": mode, "completion": completion::read_system()})
        .to_string())
}

fn save_update(receipt: &Receipt) -> Result<()> {
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

fn rollback(operation: &mut Operation, args: &[String]) -> Result<String> {
    let generation: u32 = args
        .first()
        .and_then(|g| g.parse().ok())
        .ok_or_else(|| Failure("rollback needs a generation number".into()))?;
    let Some(chosen) = GenerationId::read(Path::new(PROFILE), generation) else {
        return Err(Failure(format!("There is no generation {generation}")));
    };
    let setup = Setup::detect();
    let target = std::fs::canonicalize(format!("{PROFILE}-{generation}-link"))?;
    let active = std::fs::canonicalize("/run/current-system")?;
    let booted = std::fs::canonicalize("/run/booted-system")?;
    let reasons = completion::plan(&booted, &active, &target);
    let mode = activation_mode("auto", &reasons)?;
    let running = current_generation();
    if let Some(current) = &running {
        let _ = snapshots::keep(&setup, Path::new(SAVED), current);
    }
    stage(1, &format!("Returning to generation {generation}"));
    // Nothing is changed before the commit, so there is nothing to put back.
    let committed = operation.commit_without_changes()?;
    committed.switch_generation(generation)?;
    let system = std::fs::canonicalize(PROFILE)?;
    let activating = Receipt::activating(system.clone(), reasons);
    save_update(activating.receipt())?;
    // The configuration follows the system, so the next change builds on
    // what this generation was made from: the copy kept for this very
    // generation, not for an earlier one that had its number.
    let kept = Snapshot::of(Path::new(SAVED), &chosen);
    let mut left = Vec::new();
    if let Some(kept) = &kept {
        let running = running.and_then(|r| Snapshot::of(Path::new(SAVED), &r));
        left = snapshots::restore_choices(&setup, kept, running.as_ref());
        for file in &left {
            report(1, &format!("{} was changed by hand since, so it was left as it is", file.display()));
        }
    }
    committed.switch_to_configuration(Path::new(PROFILE), mode)?;
    if mode == "switch" && std::fs::canonicalize("/run/current-system").ok().as_ref() != Some(&system) {
        return Err(Failure("Rollback did not make the selected generation current".into()));
    }
    save_update(&if mode == "boot" { activating.prepared() } else { activating.activated() })?;
    if mode == "switch" {
        refresh_launchers(&active, &system);
    }
    stage_done(1);
    let left: Vec<String> = left.iter().map(|f| f.display().to_string()).collect();
    Ok(serde_json::json!({"ok": true, "generation": generation, "configuration": kept.is_some(), "left": left,
        "system": system, "mode": mode, "completion": completion::read_system()})
    .to_string())
}

fn clean(operation: &mut Operation, args: &[String]) -> Result<String> {
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
    // Deleting generations and updating the boot menu is one step a stop
    // waits for; only then can garbage be collected.
    let generations = Generations::hold(operation)?;
    let menu = match days {
        Some(days) => generations.delete_older_than(days).update_boot_menu()?,
        None => generations.update_boot_menu()?,
    };
    let freed = menu.collect_garbage().map_err(|Failure(message)| match days {
        Some(_) => Failure(format!(
            "{}. The old generations were deleted, and the boot menu lists only the ones left",
            if message == "Stopped" { "Collecting garbage was stopped" } else { &message }
        )),
        None => Failure(message),
    })?;
    Ok(serde_json::json!({"ok": true, "freed": freed}).to_string())
}

/// The free blocks of the filesystem holding `path`, in bytes, counting
/// those kept for root: what deleting files changes.
fn blocks_free(path: &str) -> Option<u64> {
    yukimi_system::sys::blocks_free(Path::new(path))
}

/// A generation number as given: digits, without sign or leading zero.
fn parse_generation(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 10 || text.starts_with('0') || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// What `delete` was asked to do.
#[derive(Debug, PartialEq, Eq)]
struct Deletion {
    generations: Vec<u32>,
    /// Collect garbage afterwards, so the space comes back now.
    collect: bool,
}

impl Deletion {
    fn parse(args: &[String]) -> Result<Deletion> {
        let mut deletion = Deletion { generations: Vec::new(), collect: false };
        for arg in args {
            if arg == "--collect-garbage" && !deletion.collect {
                deletion.collect = true;
                continue;
            }
            let number = parse_generation(arg).ok_or_else(|| Failure(format!("Not a generation number: {arg:?}")))?;
            if deletion.generations.contains(&number) {
                return Err(Failure(format!("Generation {number} was named twice")));
            }
            deletion.generations.push(number);
        }
        if deletion.generations.is_empty() {
            return Err(Failure("delete needs the numbers of the generations to delete".into()));
        }
        deletion.generations.sort_unstable();
        Ok(deletion)
    }
}

/// The build a generation's link points at: a path directly in the store.
fn generation_target(link: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(link).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let target = std::fs::read_link(link).ok()?;
    (target.parent() == Some(Path::new("/nix/store"))).then_some(target)
}

/// Why generation `number` (whose link leads to `target`, if it exists)
/// mustn't be deleted, if it mustn't: the profile's selected one, the
/// running system or the one the computer started with.
fn refusal(
    number: u32,
    target: Option<&Path>,
    selected: Option<u32>,
    running: Option<&Path>,
    booted: Option<&Path>,
) -> Option<String> {
    let Some(target) = target else {
        return Some(format!("There is no generation {number}"));
    };
    if selected == Some(number) {
        return Some(format!("Generation {number} is selected for the next start, so it can't be deleted"));
    }
    if running == Some(target) {
        return Some(format!("Generation {number} is the running system, so it can't be deleted"));
    }
    if booted == Some(target) {
        return Some(format!(
            "Generation {number} is the one this computer started with, so it can't be deleted until the next start"
        ));
    }
    None
}

fn delete(operation: &mut Operation, args: &[String]) -> Result<String> {
    let deletion = Deletion::parse(args)?;
    let selected = current_generation().map(|g| g.number());
    let running = std::fs::canonicalize("/run/current-system").ok();
    let booted = std::fs::canonicalize("/run/booted-system").ok();
    for &number in &deletion.generations {
        let target = generation_target(Path::new(&format!("{PROFILE}-{number}-link")));
        if let Some(why) = refusal(number, target.as_deref(), selected, running.as_deref(), booted.as_deref()) {
            return Err(Failure(why));
        }
    }
    let what = generation_words(&deletion.generations);
    // What deleting gives back, worked out as root, who sees every root.
    let estimate = yukimi_system::reclaim::estimate(&deletion.generations);
    match &estimate {
        Ok(e) => report(
            1,
            &format!(
                "Deleting {what} lets {} go once garbage is collected; {} is garbage already",
                yukimi_system::human_size(e.disk),
                yukimi_system::human_size(e.garbage)
            ),
        ),
        Err(e) => report(1, &format!("What deleting {what} gives back couldn't be worked out: {e}")),
    }
    // Deleting the generations and updating the boot menu is one step a
    // stop waits for; only then can garbage be collected.
    let menu = Generations::hold(operation)?.delete(&deletion.generations).update_boot_menu()?;
    let mut report_json = serde_json::json!({
        "ok": true,
        "deleted": deletion.generations,
        "collected": false,
        "estimate": estimate.ok(),
    });
    if deletion.collect {
        let before = blocks_free("/nix/store");
        match menu.collect_garbage() {
            Ok(summary) => {
                report_json["collected"] = true.into();
                report_json["summary"] = summary.into();
            }
            Err(Failure(message)) => {
                // The generations are gone either way; the space comes
                // back at the next clean-up.
                report(1, &format!("Collecting garbage didn't finish: {message}"));
                report_json["collect_error"] = message.into();
                stage_done(3);
            }
        }
        if let (Some(before), Some(after)) = (before, blocks_free("/nix/store")) {
            report_json["freed"] = after.saturating_sub(before).into();
        }
    }
    Ok(report_json.to_string())
}

/// What deleting generations would give back, for anyone to ask.
fn estimate(args: &[String]) -> Result<String> {
    let mut numbers = Vec::new();
    for arg in args {
        let number = parse_generation(arg).ok_or_else(|| Failure(format!("Not a generation number: {arg:?}")))?;
        if !numbers.contains(&number) {
            numbers.push(number);
        }
    }
    if numbers.is_empty() {
        return Err(Failure("estimate needs the numbers of generations".into()));
    }
    let estimate = yukimi_system::reclaim::estimate(&numbers).map_err(Failure)?;
    let mut json = serde_json::to_value(estimate)?;
    json["ok"] = true.into();
    Ok(json.to_string())
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
    // Working out what deleting generations would free only reads.
    if args.first().is_some_and(|s| s == "estimate") {
        return match estimate(&args[1..]) {
            Ok(json) => {
                let _ = writeln!(std::io::stdout(), "{json}");
                ExitCode::SUCCESS
            }
            Err(Failure(message)) => {
                let _ = writeln!(std::io::stderr(), "{message}");
                let _ = writeln!(std::io::stdout(), "{}", serde_json::json!({"ok": false, "error": message}));
                ExitCode::FAILURE
            }
        };
    }
    if !yukimi_system::sys::is_root() {
        report(0, "yukimi-helper must be started through pkexec");
        return ExitCode::from(2);
    }
    // The operation's unit runs this after its worker ends, however it
    // ended (ExecStopPost=, SIGKILL included): it finishes a clean-up that
    // was killed before it rewrote the boot menu. While another operation
    // holds the lock, that operation's own start does it instead.
    if args.first().is_some_and(|s| s == "__recover") {
        let mut operation = match Operation::begin() {
            Ok(operation) => operation,
            Err(Failure(message)) => {
                report(1, &format!("Not recovering now: {message}"));
                return ExitCode::SUCCESS;
            }
        };
        return match cleaning::recover(&mut operation) {
            Ok(_) => ExitCode::SUCCESS,
            Err(Failure(message)) => {
                report(0, &format!("Finishing an interrupted clean-up failed: {message}"));
                ExitCode::FAILURE
            }
        };
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
                let message = if process::committed() {
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
    let result = if args.first().is_some_and(|s| matches!(s.as_str(), "change" | "rollback" | "clean" | "delete")) {
        transaction::client(&args)
    } else {
        Err(Failure("Usage: yukimi-helper change|rollback|clean|delete … (or estimate …)".into()))
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
    // concurrently: the operation holds the lock until it ends.
    let mut operation = Operation::begin()?;
    watch_for_stop();
    // A clean-up killed, or cut off by a power failure, before it rewrote
    // the boot menu: finish that first.
    if let Err(Failure(message)) = cleaning::recover(&mut operation) {
        report(0, &format!("Finishing an interrupted clean-up failed: {message}"));
    }
    match command.as_str() {
        "change" => change(&mut operation, rest),
        "rollback" => rollback(&mut operation, rest),
        "clean" => clean(&mut operation, rest),
        "delete" => delete(&mut operation, rest),
        _ => Err(Failure("Usage: yukimi-helper change|rollback|clean|delete … (or estimate …)".into())),
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
    fn deletions_are_checked() {
        assert_eq!(
            Deletion::parse(&args("27 3 --collect-garbage")).unwrap(),
            Deletion { generations: vec![3, 27], collect: true }
        );
        assert_eq!(Deletion::parse(&args("4")).unwrap(), Deletion { generations: vec![4], collect: false });
        for bad in [
            "",
            "--collect-garbage",
            "0",
            "03",
            "+3",
            "-3",
            "3.0",
            "3a",
            "4294967296",
            "99999999999",
            "3 3",
            "3 --collect-garbage --collect-garbage",
            "--older-than-days 3",
            "system-3-link",
            "../3",
        ] {
            assert!(Deletion::parse(&args(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(parse_generation("4294967295"), Some(u32::MAX));
        assert_eq!(generation_words(&[3]), "generation 3");
        assert_eq!(generation_words(&[2, 3, 4]), "generations 2, 3 and 4");
    }

    #[test]
    fn the_selected_running_and_started_generations_stay() {
        let (old, running, booted) = (
            Path::new("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-a-1"),
            Path::new("/nix/store/1123456789abcdfghijklmnpqrsvwxyz-nixos-system-a-2"),
            Path::new("/nix/store/2123456789abcdfghijklmnpqrsvwxyz-nixos-system-a-3"),
        );
        let why = |n, target| refusal(n, target, Some(9), Some(running), Some(booted));
        assert_eq!(why(4, Some(old)), None);
        assert!(why(4, None).unwrap().contains("no generation 4"));
        assert!(why(9, Some(old)).unwrap().contains("selected for the next start"));
        assert!(why(5, Some(running)).unwrap().contains("running"));
        assert!(why(6, Some(booted)).unwrap().contains("started with"));
        // Without /run/booted-system (a container, say), only the others.
        assert_eq!(refusal(6, Some(booted), Some(9), Some(running), None), None);
        // A link that isn't one to a store path isn't a generation.
        let dir = std::env::temp_dir().join(format!("yukimi-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(old, dir.join("good")).unwrap();
        std::os::unix::fs::symlink("/etc/shadow", dir.join("bad")).unwrap();
        std::os::unix::fs::symlink(old.join("bin"), dir.join("inside")).unwrap();
        std::fs::write(dir.join("file"), "").unwrap();
        assert_eq!(generation_target(&dir.join("good")).as_deref(), Some(old));
        for name in ["bad", "inside", "file", "missing"] {
            assert_eq!(generation_target(&dir.join(name)), None, "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
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
}
