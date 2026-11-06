// SPDX-License-Identifier: GPL-3.0-or-later
//! What setup changes outside its own files: the account, through shadow's
//! tools (NixOS keeps people's accounts imperative with `mutableUsers`);
//! the host name, time zone and locale, through systemd's services, which
//! the module lets change them before the configuration names them; the
//! login screen's and the person's keyboard, in the files Aizome reads; and
//! the installed flake's evaluation. A trait, so the transaction's tests
//! run against a stand-in and interrupt it anywhere.

use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use zeroize::Zeroizing;

use crate::config::Config;

/// An account in `/etc/passwd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub gecos: String,
    pub home: PathBuf,
    pub shell: String,
}

/// The account setup makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAccount {
    pub name: String,
    pub uid: u32,
    pub full_name: String,
    pub primary_group: String,
    pub groups: Vec<String>,
    pub home: PathBuf,
    pub shell: String,
}

/// People's accounts are those NixOS gives `isNormalUser` (1000 up) below
/// the system's own high ones (`nobody`): login.defs' range.
pub const UID_MIN: u32 = 1000;
pub const UID_MAX: u32 = 60000;

pub trait System {
    fn account(&self, name: &str) -> Result<Option<Account>>;
    /// Everyone's accounts.
    fn accounts(&self) -> Result<Vec<Account>>;
    /// Ids NixOS has handed out before (`/var/lib/nixos/uid-map`), which a
    /// new account mustn't take.
    fn reserved_uids(&self) -> Result<Vec<u32>>;
    fn group_exists(&self, group: &str) -> Result<bool>;
    /// The groups `name` is a member of (besides its primary one).
    fn groups_of(&self, name: &str) -> Result<Vec<String>>;
    /// The password field of `name`'s shadow entry.
    fn password_hash(&self, name: &str) -> Result<Option<Zeroizing<String>>>;
    fn create_account(&mut self, account: &NewAccount) -> Result<()>;
    fn set_password_hash(&mut self, name: &str, hash: &str) -> Result<()>;
    fn add_groups(&mut self, name: &str, groups: &[String]) -> Result<()>;
    /// Make `account`'s home if it's missing (0700, theirs); never change
    /// one that is there.
    fn ensure_home(&mut self, account: &Account) -> Result<()>;
    fn hostname(&self) -> Result<String>;
    fn set_hostname(&mut self, name: &str) -> Result<()>;
    fn time_zone(&self) -> Result<String>;
    fn set_time_zone(&mut self, zone: &str) -> Result<()>;
    fn locale(&self) -> Result<String>;
    fn set_locale(&mut self, locale: &str) -> Result<()>;
    /// The keyboard of `account`'s Aizome session, unless they have
    /// settings already.
    fn set_user_keyboard(&mut self, account: &Account, layout: &str) -> Result<()>;
    /// The login screen's keyboard.
    fn set_greeter_keyboard(&mut self, layout: &str) -> Result<()>;
    fn greeter_keyboard(&self) -> Result<Option<String>>;
    /// Evaluate the installed flake's configuration for `host`, offline:
    /// its system's derivation.
    fn evaluate(&mut self, flake: &Path, host: &str) -> Result<String>;
}

/// Whether a shadow password field lets nobody in yet: locked or unset, as
/// `useradd` leaves it.
pub fn unset(hash: &str) -> bool {
    hash.is_empty() || hash.starts_with('!') || hash.starts_with('*')
}

pub fn parse_passwd(text: &str) -> Vec<Account> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() != 7 {
                return None;
            }
            Some(Account {
                name: f[0].to_owned(),
                uid: f[2].parse().ok()?,
                gid: f[3].parse().ok()?,
                gecos: f[4].to_owned(),
                home: f[5].into(),
                shell: f[6].to_owned(),
            })
        })
        .collect()
}

/// The groups whose member lists name `name`, from `/etc/group`'s text.
pub fn parse_groups_of(text: &str, name: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            (f.len() == 4 && f[3].split(',').any(|m| m == name)).then(|| f[0].to_owned())
        })
        .collect()
}

/// The running system, through its tools.
pub struct Host {
    config: Config,
    root: PathBuf,
}

impl Host {
    pub fn new(config: &Config) -> Host {
        Host { config: config.clone(), root: "/".into() }
    }

    fn etc(&self, name: &str) -> PathBuf {
        self.root.join("etc").join(name)
    }

    /// Run `program` with `args` and nothing of the helper's environment,
    /// feeding `input`, within `limit`; its output in the error.
    fn run(&self, program: &Path, args: &[&OsStr], input: Option<&[u8]>, limit: Duration) -> Result<String> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .env("PATH", "/run/current-system/sw/bin")
            .env("LC_ALL", "C.UTF-8")
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().with_context(|| format!("starting {}", program.display()))?;
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin.write_all(input)?;
        }
        let started = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if started.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                bail!("{} took longer than {} s", program.display(), limit.as_secs());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let out = child.wait_with_output()?;
        let text = |b: &[u8]| String::from_utf8_lossy(b).trim().chars().take(4000).collect::<String>();
        let name = program.file_name().unwrap_or(program.as_os_str()).to_string_lossy().into_owned();
        ensure!(out.status.success(), "{name} failed ({}): {}", out.status, text(&out.stderr));
        Ok(text(&out.stdout))
    }

    fn quick(&self, program: &Path, args: &[&str]) -> Result<String> {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        self.run(program, &args, None, Duration::from_secs(60))
    }
}

impl System for Host {
    fn account(&self, name: &str) -> Result<Option<Account>> {
        Ok(self.accounts()?.into_iter().find(|a| a.name == name))
    }

    fn accounts(&self) -> Result<Vec<Account>> {
        Ok(parse_passwd(&std::fs::read_to_string(self.etc("passwd"))?))
    }

    fn reserved_uids(&self) -> Result<Vec<u32>> {
        // {"name": uid, ...}; absent on a system that never declared one.
        let map = self.root.join("var/lib/nixos/uid-map");
        let text = match std::fs::read_to_string(&map) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", map.display())),
        };
        let uids: std::collections::BTreeMap<String, u32> = serde_json::from_str(&text)?;
        Ok(uids.into_values().collect())
    }

    fn group_exists(&self, group: &str) -> Result<bool> {
        let text = std::fs::read_to_string(self.etc("group"))?;
        Ok(text.lines().any(|l| l.split(':').next() == Some(group)))
    }

    fn groups_of(&self, name: &str) -> Result<Vec<String>> {
        Ok(parse_groups_of(&std::fs::read_to_string(self.etc("group"))?, name))
    }

    fn password_hash(&self, name: &str) -> Result<Option<Zeroizing<String>>> {
        let text = Zeroizing::new(std::fs::read_to_string(self.etc("shadow"))?);
        Ok(text.lines().find_map(|l| {
            let mut f = l.split(':');
            (f.next() == Some(name)).then(|| Zeroizing::new(f.next().unwrap_or("").to_owned()))
        }))
    }

    fn create_account(&mut self, a: &NewAccount) -> Result<()> {
        let uid = a.uid.to_string();
        let groups = a.groups.join(",");
        let home = a.home.to_string_lossy();
        let mut args = vec![
            "--uid",
            &uid,
            "--gid",
            &a.primary_group,
            "--comment",
            &a.full_name,
            "--home-dir",
            &home,
            "--create-home",
            "--shell",
            &a.shell,
        ];
        if !groups.is_empty() {
            args.extend(["--groups", &groups]);
        }
        args.push(&a.name);
        self.quick(&self.config.tools.useradd.clone(), &args)?;
        Ok(())
    }

    fn set_password_hash(&mut self, name: &str, hash: &str) -> Result<()> {
        // On chpasswd's input, never its command line, which anyone can read.
        let mut line = Zeroizing::new(format!("{name}:{hash}\n").into_bytes());
        let tool = self.config.tools.chpasswd.clone();
        let result = self.run(&tool, &[OsStr::new("--encrypted")], Some(&line), Duration::from_secs(60));
        zeroize::Zeroize::zeroize(&mut *line);
        result.map(|_| ())
    }

    fn add_groups(&mut self, name: &str, groups: &[String]) -> Result<()> {
        let groups = groups.join(",");
        self.quick(&self.config.tools.usermod.clone(), &["--append", "--groups", &groups, name])?;
        Ok(())
    }

    fn ensure_home(&mut self, a: &Account) -> Result<()> {
        match std::fs::symlink_metadata(&a.home) {
            Ok(m) => {
                ensure!(m.is_dir() && !m.file_type().is_symlink(), "{} isn't a directory", a.home.display());
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&a.home)?;
                std::os::unix::fs::chown(&a.home, Some(a.uid), Some(a.gid))?;
                std::fs::set_permissions(&a.home, std::fs::Permissions::from_mode(0o700))?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn hostname(&self) -> Result<String> {
        Ok(std::fs::read_to_string(self.etc("hostname")).unwrap_or_default().trim().to_owned())
    }

    fn set_hostname(&mut self, name: &str) -> Result<()> {
        // Static (/etc/hostname, which the module points hostnamed at while
        // networking.hostName is empty) and the kernel's.
        self.quick(&self.config.tools.hostnamectl.clone(), &["--static", "--transient", "hostname", name])?;
        Ok(())
    }

    fn time_zone(&self) -> Result<String> {
        let link = std::fs::read_link(self.etc("localtime")).unwrap_or_default();
        let link = link.to_string_lossy();
        Ok(link.split_once("zoneinfo/").map(|(_, z)| z.to_owned()).unwrap_or_default())
    }

    fn set_time_zone(&mut self, zone: &str) -> Result<()> {
        self.quick(&self.config.tools.timedatectl.clone(), &["set-timezone", zone])?;
        Ok(())
    }

    fn locale(&self) -> Result<String> {
        let text = std::fs::read_to_string(self.etc("locale.conf")).unwrap_or_default();
        Ok(text.lines().find_map(|l| l.strip_prefix("LANG=")).unwrap_or("").trim_matches('"').to_owned())
    }

    fn set_locale(&mut self, locale: &str) -> Result<()> {
        // /etc/locale.conf, which the module makes localed's
        // (i18n.imperativeLocale) until the configuration names it.
        self.quick(&self.config.tools.localectl.clone(), &["set-locale", &format!("LANG={locale}")])?;
        Ok(())
    }

    fn set_user_keyboard(&mut self, a: &Account, layout: &str) -> Result<()> {
        // ~/.config/aizome/settings.toml, as Settings would write it; made
        // only in a home of theirs, and only where nothing is yet.
        let meta = std::fs::symlink_metadata(&a.home)?;
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink() && std::os::unix::fs::MetadataExt::uid(&meta) == a.uid,
            "{} isn't {}'s home",
            a.home.display(),
            a.name
        );
        let mut dir = a.home.clone();
        for part in [".config", "aizome"] {
            dir.push(part);
            match std::fs::symlink_metadata(&dir) {
                Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
                Ok(_) => bail!("{} isn't a directory", dir.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&dir)?;
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
                    std::os::unix::fs::chown(&dir, Some(a.uid), Some(a.gid))?;
                }
                Err(e) => return Err(e.into()),
            }
        }
        let file = dir.join("settings.toml");
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&file)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        std::os::unix::fs::fchown(&f, Some(a.uid), Some(a.gid))?;
        f.write_all(crate::render::user_keyboard(layout).as_bytes())?;
        f.sync_all()?;
        Ok(())
    }

    fn set_greeter_keyboard(&mut self, layout: &str) -> Result<()> {
        crate::transaction::write_atomic(&self.config.greeter_keyboard, crate::render::greeter_keyboard(layout).as_bytes(), 0o644)
    }

    fn greeter_keyboard(&self) -> Result<Option<String>> {
        Ok(std::fs::read_to_string(&self.config.greeter_keyboard).ok().and_then(|t| crate::render::read_greeter_keyboard(&t)))
    }

    fn evaluate(&mut self, flake: &Path, host: &str) -> Result<String> {
        let installable = format!(
            "{}#nixosConfigurations.{}.config.system.build.toplevel.drvPath",
            flake.display(),
            calamares_nixos::config::nix_string(host)
        );
        let cache = self.config.state_dir.join("cache");
        std::fs::create_dir_all(&cache)?;
        let mut cmd = Command::new(&self.config.tools.nix);
        cmd.args([
            "--extra-experimental-features",
            "nix-command flakes",
            "eval",
            "--raw",
            "--offline",
            "--no-write-lock-file",
            &installable,
        ])
        .env_clear()
        .env("PATH", "/run/current-system/sw/bin")
        .env("HOME", &cache)
        .env("XDG_CACHE_HOME", &cache)
        .env("NIX_REMOTE", "daemon")
        .stdin(Stdio::null());
        let out = cmd.output().context("starting nix")?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            // The end of Nix's trace says what went wrong.
            let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
            let tail = tail[tail.len().saturating_sub(6)..].join("\n");
            bail!("the configuration in {} didn't evaluate:\n{tail}", flake.display());
        }
        let drv = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        ensure!(drv.starts_with("/nix/store/") && drv.ends_with(".drv"), "nix gave {drv:?} for the system");
        Ok(drv)
    }
}

/// Write `contents` to `path` only if there's no file there: a private
/// file made with exactly `mode`, synced, with its directory.
pub fn write_new(path: &Path, contents: &[u8], mode: u32) -> Result<bool> {
    let mut f = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
    };
    // The mode asked for, whatever the umask (the service runs with 0077).
    f.set_permissions(std::fs::Permissions::from_mode(mode))?;
    f.write_all(contents)?;
    f.sync_all()?;
    sync_dir(path)?;
    Ok(true)
}

/// Sync the directory holding `path`, so a new name in it lasts.
pub fn sync_dir(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwd_and_group() {
        let passwd = "root:x:0:0:System administrator:/root:/run/current-system/sw/bin/bash\n\
                      greeter:x:999:998::/var/empty:/run/current-system/sw/bin/nologin\n\
                      ada:x:1000:100:Ada Lovelace:/home/ada:/run/current-system/sw/bin/fish\n\
                      broken:x:1001\n";
        let accounts = parse_passwd(passwd);
        assert_eq!(accounts.len(), 3);
        assert_eq!(accounts[2].gecos, "Ada Lovelace");
        assert_eq!(accounts[2].home, Path::new("/home/ada"));
        let group = "wheel:x:1:ada\nusers:x:100:\nnetworkmanager:x:57:greeter,ada\nrender:x:303:greeter\n";
        assert_eq!(parse_groups_of(group, "ada"), ["wheel", "networkmanager"]);
        assert_eq!(parse_groups_of(group, "ad"), Vec::<String>::new());
        assert!(unset("!") && unset("!*") && unset("*") && unset("") && unset("!$6$x"));
        assert!(!unset("$6$rounds=100000$salt$hash"));
    }
}
