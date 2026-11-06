// SPDX-License-Identifier: GPL-3.0-or-later
//! Completing setup, as a transaction that only goes forward. Its intent
//! is written down before anything changes, and every step after it
//! checks what is already done before doing it, so finishing an
//! interrupted setup (a crash, a power cut, the login screen restarted)
//! does what's missing and nothing twice:
//!
//! 1. **Check**: Genkan's validation, a free uid, the groups there.
//! 2. **Stage**, under `/var/lib/sekka-setup` (root's alone): the password's
//!    hash (`credential`, 0600: never the password), then the choices
//!    (`staged.json`, the intent: from here on setup finishes rather than
//!    starts again), then the configuration it will write.
//! 3. **Account**: made with the staged uid if it isn't there; given the
//!    staged hash only while it has no password of its own; its groups
//!    and home completed. An account of that name with another uid is
//!    someone else's, and setup stops rather than touch it.
//! 4. **Settings**: host name, time zone, locale, and the login screen's
//!    and the person's keyboard, each set only if it differs.
//! 5. **Configuration**: `/etc/nixos/setup.nix`.
//! 6. **Verify** all of that, and evaluate the installed flake offline for
//!    the new host name, proving the configuration the first update builds.
//! 7. **Finish**: the marker (`complete`, no secrets), then the staged
//!    credential and choices are deleted.
//!
//! A retry never gives an account a password it already has, and never
//! changes its uid. Setup refuses to start where people have accounts:
//! an installation set up some other way is never put back into setup.

use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::choices::Plan;
use crate::config::Config;
use crate::protocol::{Done, Failure, State, Step};
use crate::render;
use crate::system::{Account, NewAccount, System, UID_MAX, UID_MIN, unset, write_new};

pub const STAGED_SCHEMA: &str = "sekka.setup.staged/1";
pub const MARKER_SCHEMA: &str = "sekka.setup.complete/1";

/// The choices staged, which setup finishes from: no secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Staged {
    pub schema: String,
    pub username: String,
    pub full_name: String,
    pub uid: u32,
    pub hostname: String,
    pub locale: String,
    pub keyboard: String,
    pub time_zone: String,
    pub desktop: String,
    /// Every group the account gets now.
    pub groups: Vec<String>,
    /// Those setup.nix declares.
    pub groups_declared: Vec<String>,
    /// The installation's root and ESP partition UUIDs, from the handoff.
    pub partitions: Option<(String, String)>,
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
    Password,
    Groups,
    Hostname,
    TimeZone,
    Locale,
    Keyboard,
    SetupNix,
    Verified,
    Marker,
    Cleaned,
}

impl Point {
    pub const ALL: [Point; 14] = [
        Point::Credential,
        Point::Intent,
        Point::Configuration,
        Point::Account,
        Point::Password,
        Point::Groups,
        Point::Hostname,
        Point::TimeZone,
        Point::Locale,
        Point::Keyboard,
        Point::SetupNix,
        Point::Verified,
        Point::Marker,
        Point::Cleaned,
    ];
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

fn read_staged(config: &Config) -> Result<Option<Staged>> {
    match std::fs::read(config.staged()) {
        Ok(bytes) => {
            let s: Staged = serde_json::from_slice(&bytes).context("setup's staged choices are unreadable")?;
            ensure!(s.schema == STAGED_SCHEMA, "setup's staged choices are of an unknown kind ({})", s.schema);
            Ok(Some(s))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
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

/// People's accounts on the system (uids `UID_MIN` up to `UID_MAX`).
pub fn people<S: System>(sys: &S) -> Result<Vec<Account>> {
    Ok(sys.accounts()?.into_iter().filter(|a| (UID_MIN..UID_MAX).contains(&a.uid)).collect())
}

/// Where setup stands, and what it staged.
pub fn state<S: System>(config: &Config, sys: &S) -> Result<(State, Option<Staged>)> {
    if read_marker(config)?.is_some() {
        return Ok((State::Complete, None));
    }
    if let Some(s) = read_staged(config)? {
        return Ok((State::Interrupted, Some(s)));
    }
    if people(sys)?.is_empty() { Ok((State::Needed, None)) } else { Ok((State::NotNeeded, None)) }
}

/// Whether `s`'s account exists, as setup made it.
pub fn account_made<S: System>(sys: &S, s: &Staged) -> Result<bool> {
    Ok(sys.account(&s.username)?.is_some_and(|a| a.uid == s.uid))
}

/// Remove what an unfinished stage left without an intent, and what a
/// finished one left behind.
fn tidy(config: &Config) -> Result<()> {
    let done = read_marker(config)?.is_some();
    if done || read_staged(config)?.is_none() {
        remove(&config.credential())?;
        remove(&config.staged_nix())?;
        if done {
            remove(&config.staged())?;
        }
    }
    Ok(())
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

/// What a failure in a step tells the person.
fn failed<S: System>(sys: &S, staged: Option<&Staged>, what: &str, e: anyhow::Error) -> Failure {
    eprintln!("setup: {what}: {e:#}");
    let account_exists = staged.is_some_and(|s| account_made(sys, s).unwrap_or(false));
    let mut message = format!("Setup couldn't {what}: {e:#}.");
    message = message.replace(".:", ":").replace("..", ".");
    message.push_str(if account_exists {
        " Try again. Your account is ready, so you can also log in now and setup finishes the next time it can."
    } else {
        " Try again; if it fails again, restart the computer and set it up once more."
    });
    Failure { message, account_exists, can_retry: true }
}

impl<S: System> Run<'_, S> {
    /// Begin setup with `plan`, then finish it.
    pub fn begin(&mut self, plan: Plan, partitions: Option<(String, String)>) -> Result<Done, Failure> {
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
            (State::Interrupted, _) => {
                return Err(Failure {
                    message: "Setup was already under way. Finish it first.".into(),
                    account_exists: true,
                    can_retry: true,
                });
            }
            (State::Complete, _) => return Err(Failure::choices("This computer is set up already.")),
            (State::NotNeeded, _) => {
                return Err(Failure::choices("This computer already has an account, so it doesn't need setting up."));
            }
        }
        let staged = self.stage(&plan, partitions).map_err(|e| failed(self.sys, None, "save your choices", e))?;
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
        match read_staged(self.config) {
            Ok(Some(s)) => self.finish(s),
            Ok(None) => Err(Failure::choices("There's no setup to finish.")),
            Err(e) => Err(failed(self.sys, None, "read what was staged", e)),
        }
    }

    fn stage(&mut self, plan: &Plan, partitions: Option<(String, String)>) -> Result<Staged> {
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
            hostname: plan.hostname.as_str().into(),
            locale: plan.locale.clone(),
            keyboard: plan.keyboard.clone(),
            time_zone: plan.time_zone.as_str().into(),
            desktop: plan.desktop.id().into(),
            groups: c.groups.clone(),
            groups_declared: c.declared_groups.clone(),
            partitions,
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

    fn finish(&mut self, s: Staged) -> Result<Done, Failure> {
        (self.progress)(Step::Account);
        let credential = read_credential(self.config).map_err(|e| failed(self.sys, Some(&s), "read the password", e))?;
        let account = self.account(&s, credential.as_ref()).map_err(|e| self.fail(&s, "make your account", e))?;
        (self.progress)(Step::Settings);
        self.settings(&s, &account).map_err(|e| self.fail(&s, "apply your settings", e))?;
        (self.progress)(Step::Configuration);
        self.configuration(&s).map_err(|e| self.fail(&s, "save the configuration", e))?;
        (self.progress)(Step::Verify);
        let system = self.verify(&s, credential.as_ref()).map_err(|e| self.fail(&s, "check the result", e))?;
        (self.progress)(Step::Finish);
        self.complete(&s, system).map_err(|e| self.fail(&s, "finish", e))?;
        Ok(Done { username: s.username, full_name: s.full_name, uid: s.uid, desktop: s.desktop })
    }

    fn fail(&self, s: &Staged, what: &str, e: anyhow::Error) -> Failure {
        failed(self.sys, Some(s), what, e)
    }

    fn account(&mut self, s: &Staged, credential: Option<&Zeroizing<String>>) -> Result<Account> {
        let account = match self.sys.account(&s.username)? {
            None => {
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
                    shell: self.config.shell.clone(),
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
        let current = self.sys.password_hash(&s.username)?.unwrap_or_default();
        if unset(&current) {
            let hash = credential.context("the password given at setup is gone; set up the account again")?;
            self.sys.set_password_hash(&s.username, hash)?;
            (self.stop)(Point::Password)?;
        }
        let has = self.sys.groups_of(&s.username)?;
        let missing: Vec<String> = s.groups.iter().filter(|g| !has.contains(g)).cloned().collect();
        if !missing.is_empty() {
            self.sys.add_groups(&s.username, &missing)?;
        }
        self.sys.ensure_home(&account)?;
        (self.stop)(Point::Groups)?;
        Ok(account)
    }

    fn settings(&mut self, s: &Staged, account: &Account) -> Result<()> {
        if self.sys.hostname()? != s.hostname {
            self.sys.set_hostname(&s.hostname)?;
        }
        (self.stop)(Point::Hostname)?;
        if self.sys.time_zone()? != s.time_zone {
            self.sys.set_time_zone(&s.time_zone)?;
        }
        (self.stop)(Point::TimeZone)?;
        if self.sys.locale()? != s.locale {
            self.sys.set_locale(&s.locale)?;
        }
        (self.stop)(Point::Locale)?;
        if self.sys.greeter_keyboard()?.as_deref() != Some(s.keyboard.as_str()) {
            self.sys.set_greeter_keyboard(&s.keyboard)?;
        }
        // The session starts from the system's keyboard; only another one
        // needs saying.
        if s.keyboard != self.config.keyboard {
            self.sys.set_user_keyboard(account, &s.keyboard)?;
        }
        (self.stop)(Point::Keyboard)?;
        Ok(())
    }

    fn configuration(&mut self, s: &Staged) -> Result<()> {
        let text = render::setup_nix(s);
        let path = self.config.setup_nix();
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            write_atomic(&path, text.as_bytes(), 0o644)?;
        }
        (self.stop)(Point::SetupNix)?;
        Ok(())
    }

    fn verify(&mut self, s: &Staged, credential: Option<&Zeroizing<String>>) -> Result<Option<String>> {
        let a = self.sys.account(&s.username)?.context("the account is missing")?;
        ensure!(a.uid == s.uid, "the account has user id {}, not {}", a.uid, s.uid);
        let has = self.sys.groups_of(&s.username)?;
        let missing: Vec<&String> = s.groups.iter().filter(|g| !has.contains(g)).collect();
        ensure!(missing.is_empty(), "the account isn't in {missing:?}");
        let hash = self.sys.password_hash(&s.username)?.unwrap_or_default();
        ensure!(!unset(&hash), "the account has no password");
        // Set from the staged hash; unless the person has changed it since,
        // which a retry keeps.
        if let Some(c) = credential
            && hash.as_str() != c.as_str()
        {
            eprintln!("setup: {}'s password was changed after it was set; it stays as it is", s.username);
        }
        ensure!(self.sys.hostname()? == s.hostname, "the host name didn't change");
        ensure!(self.sys.time_zone()? == s.time_zone, "the time zone didn't change");
        ensure!(self.sys.locale()? == s.locale, "the language didn't change");
        let written = std::fs::read_to_string(self.config.setup_nix())?;
        ensure!(written == render::setup_nix(s), "{} isn't what setup wrote", self.config.setup_nix().display());
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
                accounts: vec![account("root", 0), account("greeter", 999), account("nobody", 65534)],
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

    fn plan(config: &Config) -> Plan {
        choices::plan(config, &draft(), &Zeroizing::new(PASSWORD.into()), &|_| false).expect("a valid plan")
    }

    struct World {
        _dir: tempfile::TempDir,
        config: Config,
        sys: Fake,
        steps: Vec<Step>,
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        std::fs::create_dir_all(&config.nixos_dir).unwrap();
        World { _dir: dir, config, sys: Fake::image(), steps: Vec::new() }
    }

    impl World {
        fn begin(&mut self, stop_at: Option<Point>) -> Result<Done, Failure> {
            let plan = plan(&self.config);
            let steps = &mut self.steps;
            let mut progress = |s: Step| steps.push(s);
            let mut stop = |p: Point| if Some(p) == stop_at { anyhow::bail!("stopped at {p:?}") } else { Ok(()) };
            Run { config: &self.config, sys: &mut self.sys, progress: &mut progress, stop: &mut stop }.begin(plan, None)
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

        /// Everything that isn't a secret or a time, for comparing runs.
        fn outcome(&self) -> (Fake, String, Marker) {
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
            (sys, std::fs::read_to_string(self.config.setup_nix()).unwrap(), marker)
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
        // Only the lock and the marker remain: no credential, no staged choices.
        assert_eq!(w.files(), ["complete", "lock"]);
        let marker = std::fs::read_to_string(w.config.marker()).unwrap();
        let setup = std::fs::read_to_string(w.config.setup_nix()).unwrap();
        for text in [&marker, &setup] {
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
        std::fs::write(w.config.nixos_dir.join("flake.nix"), "{ }").unwrap();
        w.sys.fail = Some("evaluate");
        let f = w.begin(None).unwrap_err();
        assert!(f.can_retry && f.account_exists, "{f:?}");
        assert!(f.message.starts_with("Setup couldn't check the result: evaluate failed"), "{}", f.message);
        assert_eq!(w.state(), State::Interrupted);
        w.sys.fail = None;
        w.resume(None).unwrap();
        assert_eq!(w.state(), State::Complete);
    }

    /// Stopped at each durable point, then finished: the same account,
    /// password, settings and files as setup that was never interrupted.
    #[test]
    fn an_interruption_anywhere_is_finished_by_a_retry() {
        let mut reference = world();
        reference.begin(None).unwrap();
        let expected = reference.outcome();
        for point in Point::ALL {
            let mut w = world();
            let stopped = w.begin(Some(point));
            if point == Point::Cleaned {
                // Stopped after its last step: done, a few files left over.
                assert!(stopped.is_err());
            } else {
                assert!(stopped.is_err(), "{point:?}");
            }
            let state = w.state();
            let first_hash = w.sys.shadow.get("ada").cloned();
            match point {
                // No intent yet: it starts again (and tidies the stray credential).
                Point::Credential => {
                    assert_eq!(state, State::Needed, "{point:?}");
                    w.begin(None).unwrap();
                }
                Point::Marker | Point::Cleaned => {
                    assert_eq!(state, State::Complete, "{point:?}");
                    w.resume(None).unwrap();
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
            // Nothing done twice: one account made, one password set.
            let made = w.sys.calls.iter().filter(|c| **c == "create_account").count();
            assert!(made <= 1, "{point:?}: {:?}", w.sys.calls);
        }
    }

    /// A failing operation leaves setup interrupted; a retry finishes it.
    #[test]
    fn a_failed_step_is_retried() {
        for op in [
            "create_account",
            "set_password_hash",
            "ensure_home",
            "set_hostname",
            "set_time_zone",
            "set_locale",
            "set_greeter_keyboard",
            "set_user_keyboard",
        ] {
            let mut w = world();
            w.sys.fail = Some(op);
            let f = w.begin(None).unwrap_err();
            assert!(f.can_retry, "{op}");
            assert_eq!(f.account_exists, op != "create_account", "{op}: {f:?}");
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
        assert!(w.begin(Some(Point::Password)).is_err());
        let theirs = hash("their own new password").unwrap().to_string();
        w.sys.shadow.insert("ada".into(), theirs.clone());
        w.resume(None).unwrap();
        assert_eq!(w.sys.shadow["ada"], theirs);
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
        assert_eq!(w.sys.shadow["ada"], "$6$theirs");
        assert_eq!(w.sys.account("ada").unwrap().unwrap().uid, 4242);
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
        assert!(f.account_exists && f.can_retry, "{f:?}");
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
