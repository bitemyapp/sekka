// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper's configuration, `/etc/sekka-setup/helper.json`, which the
//! NixOS module writes (`calamares.firstBootSetup`). It is read only from a
//! root-owned chain of directories (Genkan's `read_trusted`): it names the
//! programs the helper runs and the places it writes. Nothing a client
//! sends chooses a path or a program.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use calamares_nixos::Desktop;
use serde::Deserialize;

pub const PATH: &str = "/etc/sekka-setup/helper.json";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The only account the helper answers: the login screen's.
    pub greeter_user: String,
    /// Root-only: the staged setup, its credential and the completion marker.
    pub state_dir: PathBuf,
    /// Where setup.nix goes, beside the installed flake.
    pub nixos_dir: PathBuf,
    /// The Mac installer's handoff on the EFI system partition.
    pub handoff: PathBuf,
    pub zoneinfo: PathBuf,
    /// Where homes are made (`/home`).
    pub home_base: PathBuf,
    /// The new account's login shell, as NixOS writes it for
    /// `users.defaultUserShell`.
    pub shell: String,
    /// Its primary group, as `isNormalUser` gives it (`users`).
    pub primary_group: String,
    /// Its other groups: everything the installed configuration gives the
    /// installer's account (`networkmanager` and `wheel`, and on the M3
    /// Pro's GPU, `render`), which it gets at once.
    pub groups: Vec<String>,
    /// Those setup.nix lists itself, as Genkan's configuration does; the
    /// platform's modules give the rest to `calamares.installUser`.
    pub declared_groups: Vec<String>,
    /// The desktops installed, and the one picked for people by default.
    pub desktops: Vec<Desktop>,
    pub default_desktop: Desktop,
    /// The system's keyboard layout before setup (the image's).
    pub keyboard: String,
    pub locale: String,
    /// What to call the computer in setup's words ("Mac").
    pub device: String,
    /// The login screen's keyboard until the configuration's takes over.
    pub greeter_keyboard: PathBuf,
    /// Evaluate the installed flake before finishing, when there is one.
    pub verify: bool,
    /// The state version Genkan's validation is given (it checks no more).
    pub state_version: String,
    pub tools: Tools,
}

/// The programs the helper runs, by store path.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    pub useradd: PathBuf,
    pub usermod: PathBuf,
    pub chpasswd: PathBuf,
    pub hostnamectl: PathBuf,
    pub timedatectl: PathBuf,
    pub localectl: PathBuf,
    pub nix: PathBuf,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let bytes = calamares_nixos::read_trusted(path).with_context(|| format!("reading {}", path.display()))?;
        let config: Config = serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
        config.check()?;
        Ok(config)
    }

    fn check(&self) -> Result<()> {
        for p in [&self.state_dir, &self.nixos_dir, &self.handoff, &self.zoneinfo, &self.home_base, &self.greeter_keyboard]
        {
            ensure!(p.is_absolute(), "{} isn't an absolute path", p.display());
        }
        let t = &self.tools;
        for p in [&t.useradd, &t.usermod, &t.chpasswd, &t.hostnamectl, &t.timedatectl, &t.localectl, &t.nix] {
            ensure!(p.is_absolute(), "{} isn't an absolute path", p.display());
        }
        ensure!(!self.desktops.is_empty(), "no desktops are installed");
        ensure!(self.desktops.contains(&self.default_desktop), "the default desktop isn't installed");
        ensure!(Path::new(&self.shell).is_absolute(), "the shell isn't an absolute path");
        ensure!(
            self.declared_groups.iter().all(|g| self.groups.contains(g)),
            "the declared groups are among the account's groups"
        );
        Ok(())
    }

    /// Genkan's settings, for its validation: only the zone database is read.
    pub fn genkan(&self) -> calamares_nixos::Settings {
        calamares_nixos::Settings {
            template_dir: "/nonexistent".into(),
            zoneinfo: self.zoneinfo.to_string_lossy().into_owned(),
            state_version: self.state_version.clone(),
            kernel: calamares_nixos::Kernel::Lts,
            test_diagnostics: false,
        }
    }

    pub fn staged(&self) -> PathBuf {
        self.state_dir.join("staged.json")
    }
    pub fn credential(&self) -> PathBuf {
        self.state_dir.join("credential")
    }
    pub fn staged_nix(&self) -> PathBuf {
        self.state_dir.join("setup.nix")
    }
    pub fn marker(&self) -> PathBuf {
        self.state_dir.join("complete")
    }
    pub fn lock(&self) -> PathBuf {
        self.state_dir.join("lock")
    }
    pub fn setup_nix(&self) -> PathBuf {
        self.nixos_dir.join("setup.nix")
    }
}

#[cfg(test)]
pub(crate) fn sample(root: &Path) -> Config {
    let zoneinfo = std::env::var_os("TZDIR").map(PathBuf::from).unwrap_or_else(|| "/etc/zoneinfo".into());
    Config {
        greeter_user: "greeter".into(),
        state_dir: root.join("var/lib/sekka-setup"),
        nixos_dir: root.join("etc/nixos"),
        handoff: root.join("boot/sekka/install.json"),
        zoneinfo,
        home_base: root.join("home"),
        shell: "/run/current-system/sw/bin/fish".into(),
        primary_group: "users".into(),
        groups: vec!["networkmanager".into(), "wheel".into(), "render".into()],
        declared_groups: vec!["networkmanager".into(), "wheel".into()],
        desktops: vec![Desktop::Aizome, Desktop::Tatami],
        default_desktop: Desktop::Aizome,
        keyboard: "us".into(),
        locale: "en_US.UTF-8".into(),
        device: "Mac".into(),
        greeter_keyboard: root.join("etc/aizome/keyboard.toml"),
        verify: true,
        state_version: "26.11".into(),
        tools: Tools {
            useradd: "/bin/false".into(),
            usermod: "/bin/false".into(),
            chpasswd: "/bin/false".into(),
            hostnamectl: "/bin/false".into(),
            timedatectl: "/bin/false".into(),
            localectl: "/bin/false".into(),
            nix: "/bin/false".into(),
        },
    }
}
