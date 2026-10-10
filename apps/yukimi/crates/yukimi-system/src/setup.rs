// SPDX-License-Identifier: MIT OR Apache-2.0
//! How this NixOS system is configured: where its configuration is, whether
//! it is a flake or follows channels, which file Yukimi adds its own file
//! to, and what Yukimi's NixOS module recorded about the running system.
//!
//! The same rules as `nixos-rebuild`: a flake when `/etc/nixos/flake.nix`
//! exists (following it if it links elsewhere), otherwise
//! `/etc/nixos/configuration.nix` built with the channels. The module can
//! name another directory, for a flake kept in a home directory.
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::CONFIG_DIR;

/// What Yukimi's NixOS module records about the running system.
pub const FACTS: &str = "/etc/yukimi/system.json";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    /// The Nixpkgs the running system was built from.
    pub nixpkgs: Option<String>,
    /// The system accepts unfree packages.
    pub allow_unfree: Option<bool>,
    /// The directory the configuration is in, when it isn't `/etc/nixos`.
    pub configuration: Option<String>,
    /// Applications to offer besides Yukimi's own.
    #[serde(default)]
    pub catalogs: Vec<CatalogSource>,
}

/// A catalog of applications that comes with the system, such as an
/// installer's.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CatalogSource {
    /// A JSON list of applications.
    pub file: String,
    /// The setting, a list of application ids, that installs them; without
    /// one, their packages are installed as Yukimi installs any package.
    pub setting: Option<String>,
    /// What to call the applications installed through the setting, such
    /// as "Apps chosen when installing".
    pub title: Option<String>,
}

impl Facts {
    pub fn read() -> Facts {
        let mut facts: Facts =
            std::fs::read_to_string(FACTS).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        // A channel's Nixpkgs is recorded where it was found, a link into the
        // store; and a path that is gone (collected) is no use.
        facts.nixpkgs = facts
            .nixpkgs
            .and_then(|path| std::fs::canonicalize(path).ok())
            .map(|path| path.to_string_lossy().into_owned());
        facts
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Built from a flake's `nixosConfigurations`, with its inputs pinned in
    /// `flake.lock`.
    Flake,
    /// Built from `configuration.nix` with the Nixpkgs of root's channels.
    Channels,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub kind: Kind,
    /// The directory the configuration is in.
    pub dir: PathBuf,
    /// The configuration file Yukimi's own file is imported from, and whose
    /// package lists it shows: `configuration.nix`, or for a flake without
    /// one, the file that imports the hardware configuration.
    pub main: Option<PathBuf>,
    /// The `nixosConfigurations` entry for this computer: its host name.
    pub host: String,
    /// The directory is in a Git work tree, where a flake sees only files
    /// Git knows about.
    pub git: bool,
    pub facts: Facts,
}

impl Setup {
    /// This system's setup.
    pub fn detect() -> Setup {
        Setup::detect_in(Path::new(CONFIG_DIR), Facts::read(), &crate::info::hostname())
    }

    /// The setup of a system configured in `etc_nixos`.
    pub fn detect_in(etc_nixos: &Path, facts: Facts, host: &str) -> Setup {
        let dir = facts
            .configuration
            .as_deref()
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .or_else(|| {
                let flake = etc_nixos.join("flake.nix");
                flake.exists().then(|| std::fs::canonicalize(&flake).ok()?.parent().map(Path::to_path_buf)).flatten()
            })
            .unwrap_or_else(|| etc_nixos.to_path_buf());
        let kind = if dir.join("flake.nix").exists() { Kind::Flake } else { Kind::Channels };
        let main = Some(dir.join("configuration.nix"))
            .filter(|file| file.is_file())
            .or_else(|| (kind == Kind::Flake).then(|| find_main(&dir, host)).flatten());
        let git = dir.ancestors().any(|d| d.join(".git").exists());
        Setup { kind, dir, main, host: host.to_owned(), git, facts }
    }

    /// The flake reference of the configuration, as `nixos-rebuild` would
    /// use it: the directory, which Nix reads through Git when it is in a
    /// work tree.
    pub fn flake(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// Where Yukimi keeps what it installs for everyone: `yukimi.nix` next
    /// to the main configuration file.
    pub fn packages_file(&self) -> Option<PathBuf> {
        Some(self.main.as_ref()?.parent()?.join(yukimi_config::packages::FILE))
    }

    /// The main configuration file, relative to the configuration's
    /// directory, for saying where something is.
    pub fn main_name(&self) -> Option<String> {
        let main = self.main.as_ref()?;
        Some(main.strip_prefix(&self.dir).unwrap_or(main).to_string_lossy().into_owned())
    }
}

/// A flake's main configuration file: the `.nix` file that imports
/// `hardware-configuration.nix`, preferring one in a directory named after
/// this computer when there are several.
fn find_main(dir: &Path, host: &str) -> Option<PathBuf> {
    let mut found = Vec::new();
    walk(dir, 0, &mut found);
    let candidates: Vec<PathBuf> = found
        .into_iter()
        .filter(|file| {
            std::fs::read_to_string(file).is_ok_and(|text| {
                text.lines()
                    .any(|line| !line.trim_start().starts_with('#') && line.contains("hardware-configuration.nix"))
            })
        })
        .collect();
    let for_host: Vec<&PathBuf> = candidates
        .iter()
        .filter(|file| !host.is_empty() && file.strip_prefix(dir).is_ok_and(|p| p.iter().any(|part| part == host)))
        .collect();
    match (for_host.as_slice(), candidates.as_slice()) {
        ([one], _) => Some((*one).clone()),
        (_, [one]) => Some(one.clone()),
        _ => None,
    }
}

/// `.nix` files a few directories deep, without hidden directories, build
/// results or the files that can't be the main one.
fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    // A configuration has a few dozen files; a directory with far more
    // isn't searched through to the end.
    for entry in entries.flatten().take(500) {
        if found.len() >= 500 {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with("result") {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && depth < 3 {
            walk(&entry.path(), depth + 1, found);
        } else if kind.is_file()
            && name.ends_with(".nix")
            && !matches!(name.as_str(), "flake.nix" | "hardware-configuration.nix" | "yukimi.nix")
        {
            found.push(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yukimi-setup-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn channels_with_configuration_nix() {
        let dir = tree("channels", &[("configuration.nix", "{ imports = [ ./hardware-configuration.nix ]; }")]);
        let setup = Setup::detect_in(&dir, Facts::default(), "acer");
        assert_eq!(setup.kind, Kind::Channels);
        assert_eq!(setup.main, Some(dir.join("configuration.nix")));
        assert_eq!(setup.packages_file(), Some(dir.join("yukimi.nix")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flakes_with_hosts() {
        let dir = tree(
            "hosts",
            &[
                ("flake.nix", "{ }"),
                ("hosts/acer/default.nix", "{ imports = [ ./hardware-configuration.nix ../common.nix ]; }"),
                ("hosts/pi/default.nix", "{ imports = [ ./hardware-configuration.nix ]; }"),
                ("hosts/common.nix", "{ }"),
                ("modules/old.nix", "# ./hardware-configuration.nix\n{ }"),
            ],
        );
        let setup = Setup::detect_in(&dir, Facts::default(), "acer");
        assert_eq!(setup.kind, Kind::Flake);
        assert_eq!(setup.main, Some(dir.join("hosts/acer/default.nix")));
        assert_eq!(setup.main_name().as_deref(), Some("hosts/acer/default.nix"));
        assert_eq!(setup.packages_file(), Some(dir.join("hosts/acer/yukimi.nix")));
        // Two hosts, neither of them this one: Yukimi can't tell.
        assert_eq!(Setup::detect_in(&dir, Facts::default(), "laptop").main, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_module_can_name_the_directory() {
        let elsewhere = tree("elsewhere", &[("flake.nix", "{ }"), ("configuration.nix", "{ }")]);
        let etc = std::env::temp_dir().join(format!("yukimi-setup-etc-{}", std::process::id()));
        let facts = Facts { configuration: Some(elsewhere.to_string_lossy().into_owned()), ..Facts::default() };
        let setup = Setup::detect_in(&etc, facts, "acer");
        assert_eq!((setup.kind, setup.dir.clone()), (Kind::Flake, elsewhere.clone()));
        assert_eq!(setup.flake(), elsewhere.to_string_lossy());
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    #[test]
    fn facts_from_the_module() {
        let facts: Facts = serde_json::from_str(
            r#"{"nixpkgs":"/nix/store/0123456789abcdfghijklmnpqrsvwxyz-source","allowUnfree":true,
               "configuration":null,"catalogs":[{"file":"/nix/store/x-applications.json","setting":"calamares.applications","title":null}]}"#,
        )
        .unwrap();
        assert_eq!(facts.allow_unfree, Some(true));
        assert_eq!(facts.catalogs[0].setting.as_deref(), Some("calamares.applications"));
        assert_eq!(serde_json::from_str::<Facts>("{}").unwrap(), Facts::default());
    }
}
