// SPDX-License-Identifier: GPL-3.0-or-later
//! Completing setup, as a transaction that only goes forward. Its intent
//! is written down before anything changes, and every part after it is
//! recorded once it's done, so finishing an interrupted setup (a crash, a
//! power cut, the login screen restarted) does what's missing and nothing
//! twice:
//!
//! 1. **Check**: Genkan's validation, a free uid, the groups there.
//! 2. **Stage**, under `/var/lib/sekka-setup` (root's alone): the password's
//!    hash (`credential`, 0600: never the password), then the choices and
//!    the settings they replace (`staged.json`, the intent: from here on
//!    setup finishes rather than starts again), then the configuration it
//!    will write.
//! 3. **Account**: made with the staged uid if it isn't there, its groups
//!    and home completed, and only then given the staged hash, while it
//!    has no password of its own; the credential is deleted then. An
//!    account of that name with another uid is someone else's, and setup
//!    stops rather than touch it.
//! 4. **Settings**: host name, time zone, locale, and the login screen's
//!    and the person's keyboard.
//! 5. **Configuration**: the default desktop in `/etc/nixos/configuration.nix`,
//!    changed as Yukimi changes it, then `/etc/nixos/setup.nix`, naming the
//!    host name the computer has by then.
//! 6. **Verify**: the account, and the installed flake evaluated offline
//!    for that host name, proving the configuration the first update
//!    builds.
//! 7. **Finish**: the marker (`complete`, no secrets), then the staged
//!    choices are deleted.
//!
//! Once the account has its password the person may log in while setup
//! waits to finish (a failure says so), and change what setup set. So each
//! part after the account's is recorded in `staged.json` as it's done, and a
//! retry never does a recorded part again; and a part not yet done changes
//! a setting only from the value it had when setup was staged: one changed
//! since is the person's, and stays. A retry never gives an account a
//! password it already has, and never changes its uid. Setup refuses to
//! start where people have accounts: an installation set up some other way
//! is never put back into setup.

use std::collections::BTreeSet;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use calamares_nixos::Hostname;
use serde::{Deserialize, Serialize};
use yukimi_config::edit;
use zeroize::Zeroizing;

use crate::choices::Plan;
use crate::config::Config;
use crate::protocol::{Done, Failure, State, Step};
use crate::render;
use crate::system::{Account, NewAccount, System, UID_MAX, UID_MIN, unset, write_new};

/// `/2` records the parts done and the settings they replace; `/1`, an
/// earlier helper's, is finished as it would have (`StagedV1`).
pub const STAGED_SCHEMA: &str = "sekka.setup.staged/2";
const STAGED_SCHEMA_1: &str = "sekka.setup.staged/1";
pub const MARKER_SCHEMA: &str = "sekka.setup.complete/1";

/// The choices staged, which setup finishes from, the settings they
/// replace, and the parts done: no secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Staged {
    pub schema: String,
    pub username: String,
    pub full_name: String,
    pub uid: u32,
    /// The account's login shell (`users.defaultUserShell`, by its path).
    pub shell: String,
    /// The host name chosen; once setup.nix is written, the one it names
    /// (the person's own, had they renamed the computer before then).
    pub hostname: String,
    pub locale: String,
    pub keyboard: String,
    pub time_zone: String,
    pub desktop: String,
    /// Every group the account gets now.
    pub groups: Vec<String>,
    /// Those setup.nix declares.
    pub groups_declared: Vec<String>,
    /// What the system had when setup was staged.
    pub before: Before,
    /// The parts done, which a retry leaves alone.
    pub applied: BTreeSet<Part>,
}

/// What the first helper staged (`sekka.setup.staged/1`), on test
/// installations of earlier images, which an update may bring this helper
/// to: no shell, no earlier settings and no parts recorded, and the
/// partitions, which setup.nix no longer names (identity.nix does).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StagedV1 {
    #[serde(rename = "schema")]
    _schema: String,
    username: String,
    full_name: String,
    uid: u32,
    hostname: String,
    locale: String,
    keyboard: String,
    time_zone: String,
    desktop: String,
    groups: Vec<String>,
    groups_declared: Vec<String>,
    #[serde(rename = "partitions")]
    _partitions: Option<(String, String)>,
}

impl StagedV1 {
    /// Finished as the helper that staged it would have: every part not yet
    /// the choice is done, from the settings as they are now, with the shell
    /// that helper made the account with (the configuration's).
    fn upgrade<S: System>(self, config: &Config, sys: &S) -> Result<Staged> {
        Ok(Staged {
            schema: STAGED_SCHEMA.into(),
            username: self.username,
            full_name: self.full_name,
            uid: self.uid,
            shell: config.shell.clone(),
            hostname: self.hostname,
            locale: self.locale,
            keyboard: self.keyboard,
            time_zone: self.time_zone,
            desktop: self.desktop,
            groups: self.groups,
            groups_declared: self.groups_declared,
            before: Before::now(config, sys)?,
            applied: BTreeSet::new(),
        })
    }
}

/// The settings setup replaces, as they were when it was staged: a part
/// changes one only from this, so that a value set since (by the person,
/// logged in while setup waited to finish) stays.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Before {
    pub hostname: String,
    pub time_zone: String,
    pub locale: String,
    pub greeter_keyboard: Option<String>,
    /// `/etc/nixos/setup.nix`, if there was one.
    pub setup_nix: Option<String>,
    /// The default desktop `/etc/nixos/configuration.nix` named, if it named
    /// one as a plain string.
    pub desktop: Option<String>,
}

impl Before {
    /// The settings as the system has them now.
    fn now<S: System>(config: &Config, sys: &S) -> Result<Before> {
        Ok(Before {
            hostname: sys.hostname()?,
            time_zone: sys.time_zone()?,
            locale: sys.locale()?,
            greeter_keyboard: sys.greeter_keyboard()?,
            setup_nix: read_text(&config.setup_nix())?,
            desktop: read_text(&config.configuration_nix())?.as_deref().and_then(configured_desktop),
        })
    }
}

/// The parts of finishing setup that are each done once, and recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Part {
    /// The account's groups and home.
    Groups,
    Password,
    Hostname,
    TimeZone,
    Locale,
    /// The login screen's keyboard and the person's.
    Keyboard,
    /// The default desktop, in configuration.nix.
    Desktop,
    SetupNix,
}

impl Part {
    pub const ALL: [Part; 8] = [
        Part::Groups,
        Part::Password,
        Part::Hostname,
        Part::TimeZone,
        Part::Locale,
        Part::Keyboard,
        Part::Desktop,
        Part::SetupNix,
    ];
}

/// Where configuration.nix names the default desktop, which Yukimi's Default
/// desktop choice rewrites.
const DESKTOP: [&str; 2] = ["calamares", "defaultDesktop"];

/// The default desktop a configuration names as a plain string, as Yukimi
/// reads it; nothing if it names none, names it otherwise, or doesn't parse.
fn configured_desktop(text: &str) -> Option<String> {
    edit::string_value(text, &DESKTOP).ok().flatten()
}

/// The completion marker: who was set up, when, and the system the
/// configuration evaluated to. No secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub schema: String,
    pub username: String,
    pub uid: u32,
    pub desktop: String,
    /// Seconds since 1970.
    pub completed: u64,
    /// The installed flake's system derivation, if it was evaluated.
    pub system: Option<String>,
}

/// The places a transaction is durable at, in order: where the tests stop
/// it, as a crash would.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Point {
    Credential,
    Intent,
    Configuration,
    Account,
    /// A part's change is made (or found made), and not yet recorded.
    Made(Part),
    /// The part is recorded.
    Recorded(Part),
    /// The credential is deleted, the account having its password.
    Forgotten,
    Verified,
    Marker,
    Cleaned,
}

impl Point {
    /// Every point, in the order a transaction reaches them.
    pub fn all() -> Vec<Point> {
        let mut all = vec![Point::Credential, Point::Intent, Point::Configuration, Point::Account];
        for part in Part::ALL {
            all.extend([Point::Made(part), Point::Recorded(part)]);
            if part == Part::Password {
                all.push(Point::Forgotten);
            }
        }
        all.extend([Point::Verified, Point::Marker, Point::Cleaned]);
        all
    }
}

/// One run of the transaction: the system it changes, who hears its
/// progress, and (in tests) where it stops.
pub struct Run<'a, S: System> {
    pub config: &'a Config,
    pub sys: &'a mut S,
    pub progress: &'a mut dyn FnMut(Step),
    pub stop: &'a mut dyn FnMut(Point) -> Result<()>,
}

/// An exclusive hold on the state directory, released when dropped.
pub struct Lock(#[allow(dead_code)] std::fs::File);

pub fn lock(config: &Config) -> Result<Lock> {
    std::fs::create_dir_all(&config.state_dir)?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(config.lock())
        .with_context(|| format!("opening {}", config.lock().display()))?;
    // Safety: flock on a descriptor this function owns.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error()).context("locking setup's state");
    }
    Ok(Lock(f))
}

/// Write `contents` to `path` whole or not at all: a new file beside it,
/// synced and renamed over it, and its directory synced.
pub fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let dir = path.parent().context("a file without a directory")?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("new");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    // The mode asked for, whatever the umask (the service runs with 0077).
    f.set_permissions(std::fs::Permissions::from_mode(mode))?;
    f.write_all(contents)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    crate::system::sync_dir(path)
}

fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => crate::system::sync_dir(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

/// A file's text, or nothing if there's no file.
fn read_text(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn read_staged<S: System>(config: &Config, sys: &S) -> Result<Option<Staged>> {
    let bytes = match std::fs::read(config.staged()) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let unreadable = "setup's staged choices are unreadable";
    let value: serde_json::Value = serde_json::from_slice(&bytes).context(unreadable)?;
    match value.get("schema").and_then(|s| s.as_str()) {
        Some(STAGED_SCHEMA) => Ok(Some(serde_json::from_value(value).context(unreadable)?)),
        Some(STAGED_SCHEMA_1) => {
            let v1: StagedV1 = serde_json::from_value(value).context(unreadable)?;
            Ok(Some(v1.upgrade(config, sys)?))
        }
        other => anyhow::bail!("setup's staged choices are of an unknown kind ({})", other.unwrap_or("none")),
    }
}

pub fn read_marker(config: &Config) -> Result<Option<Marker>> {
    match std::fs::read(config.marker()) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).context("setup's completion record is unreadable")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn read_credential(config: &Config) -> Result<Option<Zeroizing<String>>> {
    match std::fs::read_to_string(config.credential()) {
        Ok(text) => {
            let text = Zeroizing::new(text);
            Ok(Some(Zeroizing::new(text.trim().to_owned())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// People's accounts on the system: uids `UID_MIN` up to `UID_MAX` with a
/// shell to log in with, as the login screen lists them. Nix's build users
/// (`nixbld1`, 30001 up) are in that range, with no shell.
pub fn people<S: System>(sys: &S) -> Result<Vec<Account>> {
    Ok(sys.accounts()?.into_iter().filter(|a| (UID_MIN..UID_MAX).contains(&a.uid) && a.can_log_in()).collect())
}

/// Where setup stands, and what it staged.
pub fn state<S: System>(config: &Config, sys: &S) -> Result<(State, Option<Staged>)> {
    if read_marker(config)?.is_some() {
        return Ok((State::Complete, None));
    }
    if let Some(s) = read_staged(config, sys)? {
        return Ok((State::Interrupted, Some(s)));
    }
    if people(sys)?.is_empty() { Ok((State::Needed, None)) } else { Ok((State::NotNeeded, None)) }
}

/// Whether `s`'s account exists, as setup made it.
pub fn account_made<S: System>(sys: &S, s: &Staged) -> Result<bool> {
    Ok(sys.account(&s.username)?.is_some_and(|a| a.uid == s.uid))
}

/// Whether `s`'s account exists and has a password to log in with.
pub fn can_log_in<S: System>(sys: &S, s: &Staged) -> Result<bool> {
    Ok(account_made(sys, s)? && !unset(&sys.password_hash(&s.username)?.unwrap_or_default()))
}

/// Remove what an unfinished stage left without an intent, and what a
/// finished one left behind. An intent is kept even if this helper can't
/// read it.
fn tidy(config: &Config) -> Result<()> {
    let done = read_marker(config)?.is_some();
    if done || !config.staged().exists() {
        remove(&config.credential())?;
        remove(&config.staged_nix())?;
        if done {
            remove(&config.staged())?;
            let _ = std::fs::remove_dir_all(config.state_dir.join("cache"));
        }
    }
    Ok(())
}

/// Tidy what a stopped run left, as the helper starts: a run stopped
/// after its marker is never resumed (the login screen sees setup
/// complete), so its leftovers go here.
pub fn settle(config: &Config) -> Result<()> {
    let _lock = lock(config)?;
    tidy(config)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The lowest uid from `UID_MIN` no account has and NixOS hasn't given out.
fn free_uid<S: System>(sys: &S) -> Result<u32> {
    let used: std::collections::BTreeSet<u32> =
        sys.accounts()?.iter().map(|a| a.uid).chain(sys.reserved_uids()?).collect();
    (UID_MIN..UID_MAX).find(|u| !used.contains(u)).context("no user id is free")
}

/// Genkan's password scheme: SHA-512 crypt, 100 000 rounds (install.rs).
pub fn hash(password: &str) -> Result<Zeroizing<String>> {
    let params = sha_crypt::Sha512Params::new(100_000).map_err(|e| anyhow::anyhow!("password hashing: {e:?}"))?;
    Ok(Zeroizing::new(sha_crypt::sha512_simple(password, &params).map_err(|_| anyhow::anyhow!("password hashing failed"))?))
}

/// What a part does with a setting that is `now`, was `before` when setup
/// was staged, and was chosen as `chosen`.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    /// It is the choice already.
    Done,
    /// It is as setup found it: change it.
    Make,
    /// It was changed since setup was staged, by someone else: it stays.
    Keep,
}

fn change<T: PartialEq>(now: &T, before: &T, chosen: &T) -> Change {
    if now == chosen {
        Change::Done
    } else if now == before {
        Change::Make
    } else {
        Change::Keep
    }
}

/// What a failure in a step tells the person.
fn failed<S: System>(sys: &S, staged: Option<&Staged>, what: &str, e: anyhow::Error) -> Failure {
    eprintln!("setup: {what}: {e:#}");
    let account_exists = staged.is_some_and(|s| account_made(sys, s).unwrap_or(false));
    let can_log_in = staged.is_some_and(|s| can_log_in(sys, s).unwrap_or(false));
    let mut message = format!("Setup couldn't {what}: {e:#}.");
    message = message.replace(".:", ":").replace("..", ".");
    message.push_str(if can_log_in {
        " Try again. Your account is ready, so you can also log in now and setup finishes the next time it can."
    } else if account_exists {
        " Try again: setup finishes the account it began."
    } else {
        " Try again; if it fails again, restart the computer and set it up once more."
    });
    Failure { message, account_exists, can_log_in, can_retry: true }
}

impl<S: System> Run<'_, S> {
    /// Begin setup with `plan`, then finish it.
    pub fn begin(&mut self, plan: Plan) -> Result<Done, Failure> {
        let _lock = lock(self.config).map_err(|e| failed(self.sys, None, "start", e))?;
        (self.progress)(Step::Check);
        tidy(self.config).map_err(|e| failed(self.sys, None, "start", e))?;
        let (state, staged) = state(self.config, self.sys).map_err(|e| failed(self.sys, None, "start", e))?;
        match (state, &staged) {
            (State::Needed, _) => {}
            // Interrupted before its account was made: these choices
            // replace the ones staged.
            (State::Interrupted, Some(s)) if !account_made(self.sys, s).unwrap_or(true) => {
                if !people(self.sys).map(|p| p.is_empty()).unwrap_or(false) {
                    return Err(Failure::choices("This computer already has an account, so it doesn't need setting up."));
                }
                remove(&self.config.staged()).map_err(|e| failed(self.sys, None, "start", e))?;
                remove(&self.config.credential()).map_err(|e| failed(self.sys, None, "start", e))?;
            }
            (State::Interrupted, s) => {
                return Err(Failure {
                    message: "Setup was already under way. Finish it first.".into(),
                    account_exists: true,
                    can_log_in: s.as_ref().is_some_and(|s| can_log_in(self.sys, s).unwrap_or(false)),
                    can_retry: true,
                });
            }
            (State::Complete, _) => return Err(Failure::choices("This computer is set up already.")),
            (State::NotNeeded, _) => {
                return Err(Failure::choices("This computer already has an account, so it doesn't need setting up."));
            }
        }
        let staged = self.stage(&plan).map_err(|e| failed(self.sys, None, "save your choices", e))?;
        drop(plan);
        self.finish(staged)
    }

    /// Finish what was staged.
    pub fn resume(&mut self) -> Result<Done, Failure> {
        let _lock = lock(self.config).map_err(|e| failed(self.sys, None, "start", e))?;
        (self.progress)(Step::Check);
        if let Some(m) = read_marker(self.config).map_err(|e| failed(self.sys, None, "start", e))? {
            tidy(self.config).map_err(|e| failed(self.sys, None, "start", e))?;
            let full_name = self.sys.account(&m.username).ok().flatten().map(|a| a.gecos).unwrap_or_default();
            return Ok(Done { username: m.username, full_name, uid: m.uid, desktop: m.desktop });
        }
        match read_staged(self.config, self.sys) {
            Ok(Some(s)) => self.finish(s),
            Ok(None) => Err(Failure::choices("There's no setup to finish.")),
            Err(e) => Err(failed(self.sys, None, "read what was staged", e)),
        }
    }

    fn stage(&mut self, plan: &Plan) -> Result<Staged> {
        let c = self.config;
        for g in &c.groups {
            ensure!(self.sys.group_exists(g)?, "this system has no {g} group for the account");
        }
        ensure!(self.sys.group_exists(&c.primary_group)?, "this system has no {} group", c.primary_group);
        let staged = Staged {
            schema: STAGED_SCHEMA.into(),
            username: plan.username.as_str().into(),
            full_name: plan.full_name.clone(),
            uid: free_uid(self.sys)?,
            shell: c.shell.clone(),
            hostname: plan.hostname.as_str().into(),
            locale: plan.locale.clone(),
            keyboard: plan.keyboard.clone(),
            time_zone: plan.time_zone.as_str().into(),
            desktop: plan.desktop.id().into(),
            groups: c.groups.clone(),
            groups_declared: c.declared_groups.clone(),
            before: Before::now(c, self.sys)?,
            applied: BTreeSet::new(),
        };
        let hash = hash(&plan.password)?;
        let mut line = Zeroizing::new(format!("{}\n", hash.as_str()));
        remove(&c.credential())?;
        write_new(&c.credential(), line.as_bytes(), 0o600)?;
        zeroize::Zeroize::zeroize(&mut *line);
        (self.stop)(Point::Credential)?;
        write_atomic(&c.staged(), &serde_json::to_vec_pretty(&staged)?, 0o600)?;
        (self.stop)(Point::Intent)?;
        write_atomic(&c.staged_nix(), render::setup_nix(&staged).as_bytes(), 0o600)?;
        (self.stop)(Point::Configuration)?;
        Ok(staged)
    }

    fn finish(&mut self, mut s: Staged) -> Result<Done, Failure> {
        (self.progress)(Step::Account);
        let account = self.account(&mut s).map_err(|e| self.fail(&s, "make your account", e))?;
        (self.progress)(Step::Settings);
        self.settings(&mut s, &account).map_err(|e| self.fail(&s, "apply your settings", e))?;
        (self.progress)(Step::Configuration);
        self.configuration(&mut s).map_err(|e| self.fail(&s, "save the configuration", e))?;
        (self.progress)(Step::Verify);
        let system = self.verify(&s).map_err(|e| self.fail(&s, "check the result", e))?;
        (self.progress)(Step::Finish);
        self.complete(&s, system).map_err(|e| self.fail(&s, "finish", e))?;
        Ok(Done { username: s.username, full_name: s.full_name, uid: s.uid, desktop: s.desktop })
    }

    fn fail(&self, s: &Staged, what: &str, e: anyhow::Error) -> Failure {
        failed(self.sys, Some(s), what, e)
    }

    /// `part` is done: what it changed made to last (the tools it ran don't
    /// all sync), then recorded, so a retry leaves it alone.
    fn record(&mut self, s: &mut Staged, part: Part) -> Result<()> {
        (self.stop)(Point::Made(part))?;
        self.sys.sync()?;
        s.applied.insert(part);
        write_atomic(&self.config.staged(), &serde_json::to_vec_pretty(s)?, 0o600)?;
        (self.stop)(Point::Recorded(part))
    }

    fn account(&mut self, s: &mut Staged) -> Result<Account> {
        let account = match self.sys.account(&s.username)? {
            None => {
                // Given its password, it was the person's to keep or remove.
                ensure!(
                    !s.applied.contains(&Part::Password),
                    "the account {} was removed after setup made it, and setup won't make it again",
                    s.username
                );
                ensure!(
                    !self.sys.accounts()?.iter().any(|a| a.uid == s.uid),
                    "user id {} was taken meanwhile",
                    s.uid
                );
                self.sys.create_account(&NewAccount {
                    name: s.username.clone(),
                    uid: s.uid,
                    full_name: s.full_name.clone(),
                    primary_group: self.config.primary_group.clone(),
                    groups: s.groups.clone(),
                    home: self.config.home_base.join(&s.username),
                    shell: s.shell.clone(),
                })?;
                (self.stop)(Point::Account)?;
                self.sys.account(&s.username)?.context("the account wasn't there after it was made")?
            }
            Some(a) if a.uid == s.uid => a,
            Some(a) => anyhow::bail!(
                "an account named {} already exists (user id {}), and setup won't change it; choose another name",
                a.name,
                a.uid
            ),
        };
        // The account whole before anyone can log in to it.
        if !s.applied.contains(&Part::Groups) {
            let has = self.sys.groups_of(&s.username)?;
            let missing: Vec<String> = s.groups.iter().filter(|g| !has.contains(g)).cloned().collect();
            if !missing.is_empty() {
                self.sys.add_groups(&s.username, &missing)?;
            }
            self.sys.ensure_home(&account)?;
            let has = self.sys.groups_of(&s.username)?;
            let missing: Vec<&String> = s.groups.iter().filter(|g| !has.contains(g)).collect();
            ensure!(missing.is_empty(), "the account isn't in {missing:?}");
            self.record(s, Part::Groups)?;
        }
        if !s.applied.contains(&Part::Password) {
            // Only while it has none: one it has (set before an
            // interruption, or by the person since) stays.
            if unset(&self.sys.password_hash(&s.username)?.unwrap_or_default()) {
                let credential = read_credential(self.config)?;
                let hash = credential.as_ref().context("the password given at setup is gone; set up the account again")?;
                self.sys.set_password_hash(&s.username, hash)?;
            }
            ensure!(!unset(&self.sys.password_hash(&s.username)?.unwrap_or_default()), "the account has no password");
            self.record(s, Part::Password)?;
        }
        // The account has its password: the staged hash isn't needed again.
        remove(&self.config.credential())?;
        (self.stop)(Point::Forgotten)?;
        Ok(account)
    }

    fn settings(&mut self, s: &mut Staged, account: &Account) -> Result<()> {
        if !s.applied.contains(&Part::Hostname) {
            let now = self.sys.hostname()?;
            match change(&now, &s.before.hostname, &s.hostname) {
                Change::Make => {
                    self.sys.set_hostname(&s.hostname)?;
                    ensure!(self.sys.hostname()? == s.hostname, "the host name didn't change");
                }
                Change::Keep => eprintln!("setup: the host name was changed to {now} meanwhile; it stays"),
                Change::Done => {}
            }
            self.record(s, Part::Hostname)?;
        }
        if !s.applied.contains(&Part::TimeZone) {
            let now = self.sys.time_zone()?;
            match change(&now, &s.before.time_zone, &s.time_zone) {
                Change::Make => {
                    self.sys.set_time_zone(&s.time_zone)?;
                    ensure!(self.sys.time_zone()? == s.time_zone, "the time zone didn't change");
                }
                Change::Keep => eprintln!("setup: the time zone was changed to {now} meanwhile; it stays"),
                Change::Done => {}
            }
            self.record(s, Part::TimeZone)?;
        }
        if !s.applied.contains(&Part::Locale) {
            let now = self.sys.locale()?;
            match change(&now, &s.before.locale, &s.locale) {
                Change::Make => {
                    self.sys.set_locale(&s.locale)?;
                    ensure!(self.sys.locale()? == s.locale, "the language didn't change");
                }
                Change::Keep => eprintln!("setup: the language was changed to {now} meanwhile; it stays"),
                Change::Done => {}
            }
            self.record(s, Part::Locale)?;
        }
        if !s.applied.contains(&Part::Keyboard) {
            let now = self.sys.greeter_keyboard()?;
            let chosen = Some(s.keyboard.clone());
            if change(&now, &s.before.greeter_keyboard, &chosen) == Change::Make {
                self.sys.set_greeter_keyboard(&s.keyboard)?;
            }
            // The session starts from the system's keyboard; only another
            // one needs saying, and never over settings the person has.
            if s.keyboard != self.config.keyboard {
                self.sys.set_user_keyboard(account, &s.keyboard)?;
            }
            self.record(s, Part::Keyboard)?;
        }
        Ok(())
    }

    fn configuration(&mut self, s: &mut Staged) -> Result<()> {
        self.desktop(s)?;
        if !s.applied.contains(&Part::SetupNix) {
            self.name_the_computer(s)?;
            let path = self.config.setup_nix();
            let text = render::setup_nix(s);
            let now = read_text(&path)?;
            match change(&now.as_deref(), &s.before.setup_nix.as_deref(), &Some(text.as_str())) {
                Change::Make => write_atomic(&path, text.as_bytes(), 0o644)?,
                Change::Keep => eprintln!("setup: {} was written meanwhile; it stays", path.display()),
                Change::Done => {}
            }
            self.record(s, Part::SetupNix)?;
        }
        Ok(())
    }

    /// The host name setup.nix names, and the check evaluates: the one the
    /// computer has as setup.nix is written. That is the one chosen, unless
    /// the person renamed the computer since (logged in while setup waited
    /// to finish); then theirs, so that the configuration and the running
    /// system agree: nixos-rebuild looks up the system by the running name,
    /// and the first update keeps it. A name the configuration can't hold
    /// (not one DNS label) gives way to the chosen one, which that update
    /// sets.
    fn name_the_computer(&mut self, s: &mut Staged) -> Result<()> {
        let now = self.sys.hostname()?;
        if now != s.hostname {
            if Hostname::parse(&now).is_ok() {
                eprintln!("setup: the computer was renamed {now} meanwhile; setup.nix names it so");
                s.hostname = now;
            } else {
                eprintln!("setup: the configuration can't name the computer {now:?}; setup.nix names it {}", s.hostname);
            }
        }
        Ok(())
    }

    /// The default desktop, in configuration.nix rather than setup.nix: the
    /// image names it there as a plain string, which Yukimi's Default
    /// desktop choice rewrites, and a second definition of it in setup.nix
    /// would either conflict with that one or override it. So setup makes
    /// Yukimi's own edit (only that string changes, or it is added beside
    /// the other `calamares` settings where there is none).
    fn desktop(&mut self, s: &mut Staged) -> Result<()> {
        if s.applied.contains(&Part::Desktop) {
            return Ok(());
        }
        let path = self.config.configuration_nix();
        match read_text(&path)? {
            None => eprintln!("setup: there's no {}, so no default desktop to set", path.display()),
            Some(text) => {
                let now = configured_desktop(&text);
                match change(&now, &s.before.desktop, &Some(s.desktop.clone())) {
                    Change::Make => match edit::set_string(&text, &DESKTOP, &s.desktop) {
                        Ok(new) => {
                            let mode = std::fs::metadata(&path)?.permissions().mode() & 0o7777;
                            write_atomic(&path, new.as_bytes(), mode)?;
                            let set = read_text(&path)?.as_deref().and_then(configured_desktop);
                            ensure!(set.as_ref() == Some(&s.desktop), "the default desktop didn't change");
                        }
                        // Set some other way, by hand: Yukimi leaves it too,
                        // and the login screen still starts the person's.
                        Err(e @ yukimi_config::Error::NotAString(_)) => {
                            eprintln!("setup: {}: {e}", path.display())
                        }
                        Err(e) => return Err(e).with_context(|| format!("changing {}", path.display())),
                    },
                    Change::Keep => eprintln!(
                        "setup: the default desktop was changed to {} meanwhile; it stays",
                        now.as_deref().unwrap_or("something else")
                    ),
                    Change::Done => {}
                }
            }
        }
        self.record(s, Part::Desktop)
    }

    fn verify(&mut self, s: &Staged) -> Result<Option<String>> {
        // Each part checked its own result as it was done; what the person
        // has changed since is theirs.
        let a = self.sys.account(&s.username)?.context("the account is missing")?;
        ensure!(a.uid == s.uid, "the account has user id {}, not {}", a.uid, s.uid);
        let missing: Vec<&Part> = Part::ALL.iter().filter(|p| !s.applied.contains(p)).collect();
        ensure!(missing.is_empty(), "these weren't done: {missing:?}");
        let flake = &self.config.nixos_dir;
        let system = if self.config.verify && flake.join("flake.nix").exists() {
            Some(self.sys.evaluate(flake, &s.hostname)?)
        } else {
            None
        };
        (self.stop)(Point::Verified)?;
        Ok(system)
    }

    fn complete(&mut self, s: &Staged, system: Option<String>) -> Result<()> {
        let marker = Marker {
            schema: MARKER_SCHEMA.into(),
            username: s.username.clone(),
            uid: s.uid,
            desktop: s.desktop.clone(),
            completed: now(),
            system,
        };
        write_atomic(&self.config.marker(), &serde_json::to_vec_pretty(&marker)?, 0o644)?;
        (self.stop)(Point::Marker)?;
        remove(&self.config.credential())?;
        remove(&self.config.staged())?;
        remove(&self.config.staged_nix())?;
        let _ = std::fs::remove_dir_all(self.config.state_dir.join("cache"));
        (self.stop)(Point::Cleaned)?;
        eprintln!("setup: {} (uid {}) is set up", s.username, s.uid);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use super::*;
    use crate::choices;
    use crate::config::sample;
    use crate::protocol::Draft;

    /// A system in memory, which can be told to fail an operation.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Fake {
        pub accounts: Vec<Account>,
        pub shadow: BTreeMap<String, String>,
        pub groups: BTreeMap<String, BTreeSet<String>>,
        pub homes: BTreeSet<PathBuf>,
        pub hostname: String,
        pub time_zone: String,
        pub locale: String,
        pub user_keyboards: BTreeMap<String, String>,
        pub greeter_keyboard: Option<String>,
        pub evaluated: Vec<String>,
        pub reserved: Vec<u32>,
        pub fail: Option<&'static str>,
        pub calls: Vec<&'static str>,
    }

    impl Fake {
        pub fn image() -> Fake {
            let account = |name: &str, uid| Account {
                name: name.into(),
                uid,
                gid: 100,
                gecos: String::new(),
                home: "/var/empty".into(),
                shell: "/run/current-system/sw/bin/nologin".into(),
            };
            Fake {
                accounts: vec![
                    account("root", 0),
                    account("greeter", 999),
                    account("nixbld1", 30001),
                    account("nobody", 65534),
                ],
                shadow: [("root".into(), "!".into()), ("greeter".into(), "!".into())].into(),
                groups: ["users", "wheel", "networkmanager", "render"].iter().map(|g| (g.to_string(), BTreeSet::new())).collect(),
                hostname: String::new(),
                time_zone: "UTC".into(),
                locale: "en_US.UTF-8".into(),
                reserved: vec![999],
                ..Fake::default()
            }
        }

        fn op(&mut self, name: &'static str) -> Result<()> {
            self.calls.push(name);
            if self.fail == Some(name) {
                anyhow::bail!("{name} failed (a test)");
            }
            Ok(())
        }
    }

    impl System for Fake {
        fn account(&self, name: &str) -> Result<Option<Account>> {
            Ok(self.accounts.iter().find(|a| a.name == name).cloned())
        }
        fn accounts(&self) -> Result<Vec<Account>> {
            Ok(self.accounts.clone())
        }
        fn reserved_uids(&self) -> Result<Vec<u32>> {
            Ok(self.reserved.clone())
        }
        fn group_exists(&self, group: &str) -> Result<bool> {
            Ok(self.groups.contains_key(group))
        }
        fn groups_of(&self, name: &str) -> Result<Vec<String>> {
            Ok(self.groups.iter().filter(|(_, m)| m.contains(name)).map(|(g, _)| g.clone()).collect())
        }
        fn password_hash(&self, name: &str) -> Result<Option<Zeroizing<String>>> {
            Ok(self.shadow.get(name).map(|h| Zeroizing::new(h.clone())))
        }
        fn create_account(&mut self, a: &NewAccount) -> Result<()> {
            self.op("create_account")?;
            ensure!(self.account(&a.name)?.is_none(), "useradd: user '{}' already exists", a.name);
            self.accounts.push(Account {
                name: a.name.clone(),
                uid: a.uid,
                gid: 100,
                gecos: a.full_name.clone(),
                home: a.home.clone(),
                shell: a.shell.clone(),
            });
            self.shadow.insert(a.name.clone(), "!".into());
            for g in &a.groups {
                self.groups.get_mut(g).context("no group")?.insert(a.name.clone());
            }
            self.homes.insert(a.home.clone());
            Ok(())
        }
        fn set_password_hash(&mut self, name: &str, hash: &str) -> Result<()> {
            self.op("set_password_hash")?;
            self.shadow.insert(name.into(), hash.into());
            Ok(())
        }
        fn add_groups(&mut self, name: &str, groups: &[String]) -> Result<()> {
            self.op("add_groups")?;
            for g in groups {
                self.groups.get_mut(g).context("no group")?.insert(name.into());
            }
            Ok(())
        }
        fn ensure_home(&mut self, a: &Account) -> Result<()> {
            self.op("ensure_home")?;
            self.homes.insert(a.home.clone());
            Ok(())
        }
        fn hostname(&self) -> Result<String> {
            Ok(self.hostname.clone())
        }
        fn set_hostname(&mut self, name: &str) -> Result<()> {
            self.op("set_hostname")?;
            self.hostname = name.into();
            Ok(())
        }
        fn time_zone(&self) -> Result<String> {
            Ok(self.time_zone.clone())
        }
        fn set_time_zone(&mut self, zone: &str) -> Result<()> {
            self.op("set_time_zone")?;
            self.time_zone = zone.into();
            Ok(())
        }
        fn locale(&self) -> Result<String> {
            Ok(self.locale.clone())
        }
        fn set_locale(&mut self, locale: &str) -> Result<()> {
            self.op("set_locale")?;
            self.locale = locale.into();
            Ok(())
        }
        fn set_user_keyboard(&mut self, a: &Account, layout: &str) -> Result<()> {
            self.op("set_user_keyboard")?;
            self.user_keyboards.entry(a.name.clone()).or_insert_with(|| layout.into());
            Ok(())
        }
        fn set_greeter_keyboard(&mut self, layout: &str) -> Result<()> {
            self.op("set_greeter_keyboard")?;
            self.greeter_keyboard = Some(layout.into());
            Ok(())
        }
        fn greeter_keyboard(&self) -> Result<Option<String>> {
            Ok(self.greeter_keyboard.clone())
        }
        fn evaluate(&mut self, flake: &Path, host: &str) -> Result<String> {
            self.op("evaluate")?;
            ensure!(flake.join("setup.nix").exists(), "setup.nix isn't there to import");
            self.evaluated.push(host.into());
            Ok(format!("/nix/store/00000000000000000000000000000000-nixos-system-{host}.drv"))
        }
        fn sync(&self) -> Result<()> {
            Ok(())
        }
    }

    pub fn draft() -> Draft {
        Draft {
            full_name: "Ada Lovelace".into(),
            username: "ada".into(),
            hostname: "analytical".into(),
            locale: "de_DE.UTF-8".into(),
            keyboard: "de".into(),
            time_zone: "Europe/Berlin".into(),
            desktop: "tatami".into(),
        }
    }

    pub const PASSWORD: &str = "correct horse battery";

    struct World {
        _dir: tempfile::TempDir,
        config: Config,
        sys: Fake,
        /// What the person chooses.
        draft: Draft,
        steps: Vec<Step>,
    }

    /// The image's configuration.nix, as far as setup changes it.
    const CONFIGURATION: &str = include_str!("../test-data/configuration.nix");
    /// That as setup leaves it for Tatami.
    const CONFIGURATION_AFTER: &str = include_str!("../test-data/configuration.after.nix");

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        std::fs::create_dir_all(&config.nixos_dir).unwrap();
        std::fs::write(config.configuration_nix(), CONFIGURATION).unwrap();
        World { _dir: dir, config, sys: Fake::image(), draft: draft(), steps: Vec::new() }
    }

    impl World {
        fn begin(&mut self, stop_at: Option<Point>) -> Result<Done, Failure> {
            let plan = choices::plan(&self.config, &self.draft, &Zeroizing::new(PASSWORD.into()), &|_| false)
                .expect("a valid plan");
            let steps = &mut self.steps;
            let mut progress = |s: Step| steps.push(s);
            let mut stop = |p: Point| if Some(p) == stop_at { anyhow::bail!("stopped at {p:?}") } else { Ok(()) };
            Run { config: &self.config, sys: &mut self.sys, progress: &mut progress, stop: &mut stop }.begin(plan)
        }

        fn resume(&mut self, stop_at: Option<Point>) -> Result<Done, Failure> {
            let steps = &mut self.steps;
            let mut progress = |s: Step| steps.push(s);
            let mut stop = |p: Point| if Some(p) == stop_at { anyhow::bail!("stopped at {p:?}") } else { Ok(()) };
            Run { config: &self.config, sys: &mut self.sys, progress: &mut progress, stop: &mut stop }.resume()
        }

        fn state(&self) -> State {
            state(&self.config, &self.sys).unwrap().0
        }

        fn staged(&self) -> Staged {
            read_staged(&self.config, &self.sys).unwrap().expect("staged choices")
        }

        /// Everything that isn't a secret or a time, for comparing runs.
        fn outcome(&self) -> (Fake, String, String, Marker) {
            let mut sys = self.sys.clone();
            sys.calls.clear();
            sys.shadow.insert("ada".into(), "<hash>".into());
            // Each world has its own directory.
            let local = |p: &Path| p.strip_prefix(self._dir.path()).map(Path::to_path_buf).unwrap_or(p.to_path_buf());
            for a in &mut sys.accounts {
                a.home = local(&a.home);
            }
            sys.homes = sys.homes.iter().map(|h| local(h)).collect();
            let mut marker = read_marker(&self.config).unwrap().expect("the marker");
            marker.completed = 0;
            (sys, std::fs::read_to_string(self.config.setup_nix()).unwrap(), self.configuration(), marker)
        }

        fn configuration(&self) -> String {
            std::fs::read_to_string(self.config.configuration_nix()).unwrap()
        }

        fn files(&self) -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(&self.config.state_dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        }

        /// Setup interrupted after the account can log in: the evaluation
        /// fails, as it would with the installed flake's inputs missing.
        fn handed_over(&mut self) -> Failure {
            std::fs::write(self.config.nixos_dir.join("flake.nix"), "{ }").unwrap();
            self.sys.fail = Some("evaluate");
            let f = self.begin(None).unwrap_err();
            assert!(f.can_log_in && f.account_exists && f.can_retry, "{f:?}");
            assert!(f.message.contains("you can also log in now"), "{}", f.message);
            self.sys.fail = None;
            f
        }
    }

    #[test]
    fn setup_completes_and_leaves_no_secret_behind() {
        let mut w = world();
        assert_eq!(w.state(), State::Needed);
        let done = w.begin(None).unwrap();
        assert_eq!(done, Done { username: "ada".into(), full_name: "Ada Lovelace".into(), uid: 1000, desktop: "tatami".into() });
        assert_eq!(w.steps, Step::ALL.to_vec());
        assert_eq!(w.state(), State::Complete);
        let ada = w.sys.account("ada").unwrap().unwrap();
        assert_eq!((ada.uid, ada.gecos.as_str(), ada.shell.as_str()), (1000, "Ada Lovelace", "/run/current-system/sw/bin/fish"));
        assert_eq!(w.sys.groups_of("ada").unwrap(), ["networkmanager", "render", "wheel"]);
        let hash = w.sys.shadow["ada"].clone();
        assert!(hash.starts_with("$6$rounds=100000$"), "Genkan's scheme: {hash}");
        assert!(sha_crypt::sha512_check(PASSWORD, &hash).is_ok());
        assert_eq!((w.sys.hostname.as_str(), w.sys.time_zone.as_str(), w.sys.locale.as_str()), ("analytical", "Europe/Berlin", "de_DE.UTF-8"));
        assert_eq!(w.sys.greeter_keyboard.as_deref(), Some("de"));
        assert_eq!(w.sys.user_keyboards["ada"], "de");
        // The default desktop, where Yukimi changes it, and the file's mode kept.
        assert_eq!(w.configuration(), CONFIGURATION_AFTER);
        assert_eq!(std::fs::metadata(w.config.configuration_nix()).unwrap().permissions().mode() & 0o777, 0o644);
        // Only the lock and the marker remain: no credential, no staged choices.
        assert_eq!(w.files(), ["complete", "lock"]);
        let marker = std::fs::read_to_string(w.config.marker()).unwrap();
        let setup = std::fs::read_to_string(w.config.setup_nix()).unwrap();
        assert!(setup.contains(r#"shell = "/run/current-system/sw/bin/fish";"#), "the account's own: {setup}");
        for text in [&marker, &setup, &w.configuration()] {
            assert!(!text.contains(PASSWORD) && !text.contains(&hash) && !text.contains("$6$"), "{text}");
        }
        // No flake: nothing evaluated, and the marker says so.
        assert!(w.sys.evaluated.is_empty());
        assert_eq!(read_marker(&w.config).unwrap().unwrap().system, None);
        // Done means done: setup refuses to run again.
        assert!(w.begin(None).is_err());
        assert_eq!(w.sys.shadow["ada"], hash);
    }

    /// The service's umask is 0077; what others read (setup.nix, the login
    /// screen's keyboard) still gets its mode, and the secret stays root's.
    #[test]
    fn files_get_their_modes_whatever_the_umask() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        // Safety: umask is process-wide; these files are this test's alone.
        let old = unsafe { libc::umask(0o077) };
        write_atomic(&dir.path().join("public"), b"x", 0o644).unwrap();
        write_new(&dir.path().join("secret"), b"x", 0o600).unwrap();
        unsafe { libc::umask(old) };
        let mode = |n: &str| std::fs::metadata(dir.path().join(n)).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode("public"), mode("secret")), (0o644, 0o600));
    }

    #[test]
    fn the_installed_flake_is_evaluated_for_the_new_host_name() {
        let mut w = world();
        std::fs::write(w.config.nixos_dir.join("flake.nix"), "{ }").unwrap();
        w.begin(None).unwrap();
        assert_eq!(w.sys.evaluated, ["analytical"]);
        let marker = read_marker(&w.config).unwrap().unwrap();
        assert_eq!(marker.system.as_deref(), Some("/nix/store/00000000000000000000000000000000-nixos-system-analytical.drv"));
        // An evaluation that fails stops setup short of done; fixed, a
        // retry finishes.
        let mut w = world();
        let f = w.handed_over();
        assert!(f.message.starts_with("Setup couldn't check the result: evaluate failed"), "{}", f.message);
        assert_eq!(w.state(), State::Interrupted);
        w.resume(None).unwrap();
        assert_eq!(w.state(), State::Complete);
    }

    /// Stopped at each durable point, then finished as the login screen
    /// would (a retry, or nothing once complete, the helper tidying as it
    /// starts): the same account, password, settings and files as setup
    /// that was never interrupted, and nothing done twice.
    #[test]
    fn an_interruption_anywhere_is_finished_by_a_retry() {
        let mut reference = world();
        reference.begin(None).unwrap();
        let expected = reference.outcome();
        let all = Point::all();
        let forgotten = all.iter().position(|p| *p == Point::Forgotten).unwrap();
        for (at, point) in all.iter().copied().enumerate() {
            let mut w = world();
            assert!(w.begin(Some(point)).is_err(), "{point:?}");
            let state = w.state();
            let first_hash = w.sys.shadow.get("ada").cloned();
            // The credential lasts until the account has its password.
            assert_eq!(w.config.credential().exists(), at < forgotten, "{point:?}");
            match point {
                // No intent yet: it starts again (and tidies the stray credential).
                Point::Credential => {
                    assert_eq!(state, State::Needed, "{point:?}");
                    w.begin(None).unwrap();
                }
                // Done: the login screen asks no more; the helper tidies as
                // it next starts.
                Point::Marker | Point::Cleaned => {
                    assert_eq!(state, State::Complete, "{point:?}");
                    settle(&w.config).unwrap();
                }
                _ => {
                    assert_eq!(state, State::Interrupted, "{point:?}");
                    w.resume(None).unwrap();
                }
            }
            assert_eq!(w.state(), State::Complete, "{point:?}");
            assert_eq!(w.outcome(), expected, "after stopping at {point:?}");
            assert!(sha_crypt::sha512_check(PASSWORD, &w.sys.shadow["ada"]).is_ok(), "{point:?}");
            // A password the account had kept what it was.
            if let Some(h) = first_hash.filter(|h| !unset(h)) {
                assert_eq!(w.sys.shadow["ada"], h, "{point:?}");
            }
            assert_eq!(w.files(), ["complete", "lock"], "{point:?}");
            // Nothing done twice: one account made, one password set, each
            // setting changed once.
            for op in ["create_account", "set_password_hash", "set_hostname", "set_time_zone", "set_locale", "set_greeter_keyboard"] {
                let n = w.sys.calls.iter().filter(|c| **c == op).count();
                assert!(n <= 1, "{point:?}: {op} {n} times: {:?}", w.sys.calls);
            }
        }
    }

    /// A failing operation leaves setup interrupted; a retry finishes it.
    /// Log In is offered only once the account has its password.
    #[test]
    fn a_failed_step_is_retried() {
        for (op, made, ready) in [
            ("create_account", false, false),
            ("ensure_home", true, false),
            ("set_password_hash", true, false),
            ("set_hostname", true, true),
            ("set_time_zone", true, true),
            ("set_locale", true, true),
            ("set_greeter_keyboard", true, true),
            ("set_user_keyboard", true, true),
        ] {
            let mut w = world();
            w.sys.fail = Some(op);
            let f = w.begin(None).unwrap_err();
            assert!(f.can_retry, "{op}");
            assert_eq!((f.account_exists, f.can_log_in), (made, ready), "{op}: {f:?}");
            assert_eq!(f.message.contains("log in now"), ready, "{op}: {}", f.message);
            assert_eq!(w.state(), State::Interrupted, "{op}");
            w.sys.fail = None;
            w.resume(None).unwrap();
            assert_eq!(w.state(), State::Complete, "{op}");
            assert!(sha_crypt::sha512_check(PASSWORD, &w.sys.shadow["ada"]).is_ok());
        }
    }

    #[test]
    fn a_retry_never_resets_a_password_or_moves_a_uid() {
        // Interrupted after the password was set; then the person changed it.
        let mut w = world();
        assert!(w.begin(Some(Point::Made(Part::Password))).is_err());
        let theirs = hash("their own new password").unwrap().to_string();
        w.sys.shadow.insert("ada".into(), theirs.clone());
        w.resume(None).unwrap();
        assert_eq!(w.sys.shadow["ada"], theirs);
        // Locked by the person after setup gave it one: it stays locked.
        let mut w = world();
        w.handed_over();
        w.sys.shadow.insert("ada".into(), format!("!{theirs}"));
        w.resume(None).unwrap();
        assert_eq!(w.sys.shadow["ada"], format!("!{theirs}"));
        // An account of that name with another uid is someone else's.
        let mut w = world();
        assert!(w.begin(Some(Point::Intent)).is_err());
        w.sys.accounts.push(Account {
            name: "ada".into(),
            uid: 4242,
            gid: 100,
            gecos: String::new(),
            home: "/home/ada".into(),
            shell: "/bin/sh".into(),
        });
        w.sys.shadow.insert("ada".into(), "$6$theirs".into());
        let f = w.resume(None).unwrap_err();
        assert!(f.message.contains("already exists (user id 4242)"), "{}", f.message);
        assert!(!f.account_exists && !f.can_log_in, "{f:?}");
        assert_eq!(w.sys.shadow["ada"], "$6$theirs");
        assert_eq!(w.sys.account("ada").unwrap().unwrap().uid, 4242);
    }

    /// Told it could log in, the person did, and changed what setup set:
    /// the retry the login screen sends when they log out keeps all of it.
    #[test]
    fn a_retry_keeps_what_the_person_changed_after_logging_in() {
        let mut w = world();
        w.handed_over();
        assert_eq!(w.staged().applied, Part::ALL.into_iter().collect(), "everything but the check done");
        w.sys.hostname = "babbage".into();
        // Back to what it was before setup: still theirs.
        w.sys.time_zone = "UTC".into();
        w.sys.locale = "ja_JP.UTF-8".into();
        w.sys.groups.get_mut("render").unwrap().remove("ada");
        let mine = "# Mine now.\n{ ... }: { }\n";
        std::fs::write(w.config.setup_nix(), mine).unwrap();
        // Yukimi's Default desktop choice, back to Aizome.
        let theirs = edit::set_string(&w.configuration(), &DESKTOP, "aizome").unwrap();
        std::fs::write(w.config.configuration_nix(), &theirs).unwrap();
        let calls = w.sys.calls.len();
        w.resume(None).unwrap();
        assert_eq!(w.state(), State::Complete);
        assert_eq!((w.sys.hostname.as_str(), w.sys.time_zone.as_str(), w.sys.locale.as_str()), ("babbage", "UTC", "ja_JP.UTF-8"));
        assert_eq!(w.sys.groups_of("ada").unwrap(), ["networkmanager", "wheel"]);
        assert_eq!(std::fs::read_to_string(w.config.setup_nix()).unwrap(), mine);
        assert_eq!(w.configuration(), theirs);
        assert_eq!(w.sys.calls[calls..], ["evaluate"], "only the check again");
        assert_eq!(w.files(), ["complete", "lock"]);
    }

    /// Renamed by the person before setup.nix was written (logged in after
    /// a failure): setup.nix names the computer as it's called, and the
    /// check evaluates that system, so the configuration, the running
    /// system and the system nixos-rebuild looks up agree.
    #[test]
    fn setup_nix_names_the_computer_as_the_person_renamed_it() {
        // Before its host name was set, and after.
        for (op, kept) in [("set_hostname", "analytical"), ("set_time_zone", "analytical")] {
            let mut w = world();
            std::fs::write(w.config.nixos_dir.join("flake.nix"), "{ }").unwrap();
            w.sys.fail = Some(op);
            assert!(w.begin(None).unwrap_err().can_log_in, "{op}");
            assert_eq!(w.sys.hostname, if op == "set_hostname" { "" } else { kept }, "{op}");
            w.sys.hostname = "babbage".into();
            w.sys.fail = None;
            w.resume(None).unwrap();
            assert_eq!(w.sys.hostname, "babbage", "{op}: the person's name stays");
            let setup = std::fs::read_to_string(w.config.setup_nix()).unwrap();
            assert!(setup.contains(r#"networking.hostName = "babbage";"#), "{op}: {setup}");
            assert_eq!(w.sys.evaluated, ["babbage"], "{op}");
            let system = read_marker(&w.config).unwrap().unwrap().system;
            assert_eq!(system.as_deref(), Some("/nix/store/00000000000000000000000000000000-nixos-system-babbage.drv"));
        }
        // A name the configuration can't hold: setup.nix names the chosen
        // one, which the first update sets.
        let mut w = world();
        w.sys.fail = Some("set_time_zone");
        assert!(w.begin(None).unwrap_err().can_log_in);
        w.sys.hostname = "babbage.local".into();
        w.sys.fail = None;
        w.resume(None).unwrap();
        let setup = std::fs::read_to_string(w.config.setup_nix()).unwrap();
        assert!(setup.contains(r#"networking.hostName = "analytical";"#), "{setup}");
        assert_eq!(w.sys.hostname, "babbage.local");
    }

    /// What wasn't done yet when the person logged in is done, but never
    /// over a setting they changed meanwhile.
    #[test]
    fn a_part_not_yet_done_never_replaces_the_persons_change() {
        let mut w = world();
        w.sys.fail = Some("set_time_zone");
        let f = w.begin(None).unwrap_err();
        assert!(f.can_log_in, "the account had its password: {f:?}");
        assert_eq!(w.sys.hostname, "analytical");
        // Logged in, they set their own time zone (and the language stays the image's).
        w.sys.time_zone = "Asia/Tokyo".into();
        w.sys.fail = None;
        w.resume(None).unwrap();
        assert_eq!(w.sys.time_zone, "Asia/Tokyo");
        assert_eq!(w.sys.locale, "de_DE.UTF-8", "not done yet, and still as setup found it: done now");
        assert_eq!(w.sys.greeter_keyboard.as_deref(), Some("de"));
        assert!(w.config.setup_nix().exists());
        // A setup.nix written meanwhile is theirs too.
        let mut w = world();
        w.sys.fail = Some("set_locale");
        assert!(w.begin(None).unwrap_err().can_log_in);
        std::fs::write(w.config.setup_nix(), "{ }\n").unwrap();
        w.sys.fail = None;
        w.resume(None).unwrap();
        assert_eq!(std::fs::read_to_string(w.config.setup_nix()).unwrap(), "{ }\n");
        // One that was there before setup began is replaced.
        let mut w = world();
        std::fs::write(w.config.setup_nix(), "# stale\n").unwrap();
        w.begin(None).unwrap();
        assert!(std::fs::read_to_string(w.config.setup_nix()).unwrap().starts_with("# Written by Sekka's first-boot setup"));
        // The default desktop they chose in Yukimi meanwhile stays; one they
        // left alone is set, and whatever else they changed in
        // configuration.nix stays.
        for (theirs, expected) in [(Some("gnome"), "gnome"), (None, "tatami")] {
            let mut w = world();
            w.sys.fail = Some("set_locale");
            assert!(w.begin(None).unwrap_err().can_log_in);
            let mut text = w.configuration().replace("  calamares = {", "  services.printing.enable = true;\n  calamares = {");
            if let Some(d) = theirs {
                text = edit::set_string(&text, &DESKTOP, d).unwrap();
            }
            std::fs::write(w.config.configuration_nix(), &text).unwrap();
            w.sys.fail = None;
            w.resume(None).unwrap();
            let text = w.configuration();
            assert_eq!(configured_desktop(&text).as_deref(), Some(expected), "{text}");
            assert!(text.contains("services.printing.enable = true;"), "{text}");
        }
    }

    /// The default desktop goes where the image names it and Yukimi changes
    /// it: configuration.nix, by Yukimi's own edit.
    #[test]
    fn the_default_desktop_is_set_where_yukimi_changes_it() {
        // `test-data/configuration.after.nix` is what setup makes of the
        // image's for Tatami (the configuration check evaluates both).
        // `SEKKA_SETUP_GOLDEN=write` writes it anew.
        let after = edit::set_string(CONFIGURATION, &DESKTOP, "tatami").unwrap();
        if std::env::var("SEKKA_SETUP_GOLDEN").as_deref() == Ok("write") {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/configuration.after.nix");
            std::fs::write(path, &after).unwrap();
        }
        assert_eq!(after, CONFIGURATION_AFTER);
        assert_eq!(CONFIGURATION.replace("\"aizome\";", "\"tatami\";"), after, "only the default desktop changes");
        // Yukimi changes it again later, the same way.
        let later = edit::set_string(&after, &DESKTOP, "aizome").unwrap();
        assert_eq!(later.matches("defaultDesktop").count(), 1, "{later}");
        // Chosen as the image has it: configuration.nix stays byte for byte.
        let mut w = world();
        w.draft.desktop = "aizome".into();
        w.begin(None).unwrap();
        assert_eq!(w.configuration(), CONFIGURATION);
        // None named: added beside the other calamares settings.
        let mut w = world();
        let without = "{ ... }:\n{\n  calamares = {\n    desktops = [ \"aizome\" \"tatami\" ];\n  };\n}\n";
        std::fs::write(w.config.configuration_nix(), without).unwrap();
        w.begin(None).unwrap();
        assert_eq!(configured_desktop(&w.configuration()).as_deref(), Some("tatami"), "{}", w.configuration());
        // Named some other way, by hand: left alone, as Yukimi leaves it,
        // and setup still finishes.
        let mut w = world();
        let by_hand = "{ lib, ... }:\n{\n  calamares.defaultDesktop = lib.mkDefault \"aizome\";\n}\n";
        std::fs::write(w.config.configuration_nix(), by_hand).unwrap();
        w.begin(None).unwrap();
        assert_eq!(w.configuration(), by_hand);
        assert_eq!(w.state(), State::Complete);
        // No configuration.nix: nothing to change.
        let mut w = world();
        std::fs::remove_file(w.config.configuration_nix()).unwrap();
        w.begin(None).unwrap();
        assert!(!w.config.configuration_nix().exists());
        assert_eq!(w.state(), State::Complete);
    }

    /// An earlier helper's staged choices (`sekka.setup.staged/1`), left by
    /// a setup interrupted after it made the account and set its password,
    /// on an installation since updated to this helper: finished as that
    /// helper would have, setup.nix naming no partitions.
    #[test]
    fn an_earlier_helpers_staged_setup_is_finished() {
        let mut w = world();
        std::fs::create_dir_all(&w.config.state_dir).unwrap();
        let v1 = serde_json::json!({
            "schema": "sekka.setup.staged/1",
            "username": "ada",
            "full_name": "Ada Lovelace",
            "uid": 1000,
            "hostname": "analytical",
            "locale": "de_DE.UTF-8",
            "keyboard": "de",
            "time_zone": "Europe/Berlin",
            "desktop": "tatami",
            "groups": ["networkmanager", "wheel", "render"],
            "groups_declared": ["networkmanager", "wheel"],
            "partitions": ["63d89be1-72f7-4aff-9679-09cafa7e0c2d", "910de5ca-7390-4716-9a54-8028916e4d78"],
        });
        std::fs::write(w.config.staged(), serde_json::to_vec_pretty(&v1).unwrap()).unwrap();
        w.sys.create_account(&NewAccount {
            name: "ada".into(),
            uid: 1000,
            full_name: "Ada Lovelace".into(),
            primary_group: "users".into(),
            groups: vec!["networkmanager".into(), "wheel".into(), "render".into()],
            home: w.config.home_base.join("ada"),
            shell: w.config.shell.clone(),
        })
        .unwrap();
        let theirs = hash(PASSWORD).unwrap().to_string();
        w.sys.shadow.insert("ada".into(), theirs.clone());
        w.sys.hostname = "analytical".into();
        // Settling as the helper starts keeps it, and the login screen
        // offers to finish it.
        settle(&w.config).unwrap();
        assert_eq!(w.state(), State::Interrupted);
        let staged = w.staged();
        assert_eq!((staged.schema.as_str(), staged.shell.as_str()), (STAGED_SCHEMA, w.config.shell.as_str()));
        w.resume(None).unwrap();
        assert_eq!(w.state(), State::Complete);
        assert_eq!(w.sys.shadow["ada"], theirs);
        assert_eq!((w.sys.hostname.as_str(), w.sys.time_zone.as_str(), w.sys.locale.as_str()), ("analytical", "Europe/Berlin", "de_DE.UTF-8"));
        assert_eq!(w.configuration(), CONFIGURATION_AFTER);
        let setup = std::fs::read_to_string(w.config.setup_nix()).unwrap();
        assert!(!setup.contains("63d89be1") && !setup.contains("fileSystems"), "{setup}");
        assert_eq!(w.files(), ["complete", "lock"]);
        // A kind of staged file this helper doesn't know: it says so, and
        // settling leaves it and its credential alone.
        let mut w = world();
        std::fs::create_dir_all(&w.config.state_dir).unwrap();
        std::fs::write(w.config.staged(), r#"{"schema":"sekka.setup.staged/9"}"#).unwrap();
        std::fs::write(w.config.credential(), "$6$x\n").unwrap();
        settle(&w.config).unwrap();
        assert!(w.config.credential().exists());
        let e = state(&w.config, &w.sys).unwrap_err();
        assert!(format!("{e:#}").contains("unknown kind (sekka.setup.staged/9)"), "{e:#}");
        let f = w.resume(None).unwrap_err();
        assert!(f.message.contains("unknown kind"), "{}", f.message);
    }

    #[test]
    fn an_installation_with_people_is_never_set_up() {
        // The original developer image: its sekka account, its password.
        let mut w = world();
        w.sys.accounts.push(Account {
            name: "sekka".into(),
            uid: 1000,
            gid: 100,
            gecos: "Sekka".into(),
            home: "/home/sekka".into(),
            shell: "/bin/sh".into(),
        });
        w.sys.shadow.insert("sekka".into(), "$6$developer".into());
        assert_eq!(w.state(), State::NotNeeded);
        let f = w.begin(None).unwrap_err();
        assert!(!f.can_retry && f.message.contains("already has an account"), "{f:?}");
        assert!(w.resume(None).is_err());
        assert_eq!(w.sys.shadow["sekka"], "$6$developer");
        assert!(w.sys.calls.is_empty(), "{:?}", w.sys.calls);
        assert!(!w.config.setup_nix().exists() && !w.config.staged().exists() && !w.config.credential().exists());
    }

    #[test]
    fn new_choices_replace_staged_ones_until_the_account_is_made() {
        let mut w = world();
        assert!(w.begin(Some(Point::Configuration)).is_err());
        assert_eq!(w.state(), State::Interrupted);
        // Not made yet: starting over with the same or other choices is fine.
        w.begin(None).unwrap();
        // Made: it can only be finished.
        let mut w = world();
        assert!(w.begin(Some(Point::Account)).is_err());
        let f = w.begin(None).unwrap_err();
        assert!(f.account_exists && !f.can_log_in && f.can_retry, "{f:?}");
        w.resume(None).unwrap();
    }

    #[test]
    fn the_uid_avoids_ids_nixos_handed_out() {
        let mut w = world();
        w.sys.reserved = vec![999, 1000, 1001];
        assert_eq!(w.begin(None).unwrap().uid, 1002);
    }

    #[test]
    fn a_missing_group_stops_setup_before_anything_changes() {
        let mut w = world();
        w.sys.groups.remove("render");
        let f = w.begin(None).unwrap_err();
        assert!(f.message.contains("no render group"), "{}", f.message);
        assert_eq!(w.state(), State::Needed);
        assert!(w.sys.calls.is_empty());
    }
}
