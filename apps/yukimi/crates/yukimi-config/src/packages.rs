// SPDX-License-Identifier: MIT OR Apache-2.0
//! `yukimi.nix`: what Yukimi installs for everyone.
//!
//! A small module of its own next to the configuration file that imports
//! it, so Yukimi never has to rewrite anything a person wrote. It holds two
//! lists: packages, as their attribute names in Nixpkgs (`htop`,
//! `python3Packages.requests`), and NixOS programs, each switched on with
//! `programs.<name>.enable` (Steam, say, needs more than its package). Both
//! are checked before they are written. It can also keep packages at the
//! version the system had (`programs.yukimi.held`, from Yukimi's module).
use std::collections::BTreeMap;

use crate::edit::{add_import, let_string_list, let_string_sets, nix_string};
use crate::{Error, Result};

/// The file, next to the configuration file that imports it.
pub const FILE: &str = "yukimi.nix";
/// How that file imports it.
pub const IMPORT: &str = "./yukimi.nix";

/// Where a held package comes from: a saved configuration of this system,
/// pinned by its hash, and the system's name in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub flake: String,
    pub host: String,
}

/// Where the helper keeps the configurations held packages come from.
pub const HELD_DIR: &str = "/var/lib/yukimi/held";

/// What `yukimi.nix` installs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    pub packages: Vec<String>,
    pub programs: Vec<String>,
    /// Packages kept at the version the system had, by name without the
    /// version (`firefox`).
    pub held: BTreeMap<String, Hold>,
}

impl Choices {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.programs.is_empty() && self.held.is_empty()
    }
}

/// Whether a name is a package name without its version, as NixOS's
/// `pname` or `builtins.parseDrvName` gives it (`firefox`, `gst-plugins-good`,
/// `python3.13-requests`).
pub fn valid_held_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

/// Whether a hold names a configuration the helper saved, pinned by hash.
fn valid_hold(hold: &Hold) -> bool {
    let pinned = hold.flake.strip_prefix("path:").and_then(|f| f.strip_prefix(HELD_DIR)).is_some_and(|rest| {
        rest.starts_with('/')
            && rest.contains("?narHash=")
            && !rest.contains("..")
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '?' | '=' | '%'))
    });
    let host =
        !hold.host.is_empty() && hold.host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    pinned && host
}

/// Whether a name is an attribute path Yukimi will write: Nix identifiers
/// separated by dots.
pub fn valid_attribute(name: &str) -> bool {
    !name.is_empty() && name.len() <= 200 && name.split('.').all(valid_identifier)
}

/// Whether a name is one Nix identifier, as a program under `programs` is.
pub fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\''))
}

/// Sorted, without duplicates.
fn tidy(names: &[String]) -> Vec<String> {
    let mut names = names.to_vec();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        return "[ ]".to_owned();
    }
    let lines: String = names.iter().map(|name| format!("    {}\n", nix_string(name))).collect();
    format!("[\n{lines}  ]")
}

/// The file's contents for these choices.
pub fn render(choices: &Choices) -> Result<String> {
    if let Some(bad) = choices.packages.iter().find(|name| !valid_attribute(name)) {
        return Err(Error::BadAttribute(bad.clone()));
    }
    if let Some(bad) = choices.programs.iter().find(|name| !valid_identifier(name)) {
        return Err(Error::BadAttribute(bad.clone()));
    }
    if let Some((bad, _)) = choices.held.iter().find(|(name, hold)| !valid_held_name(name) || !valid_hold(hold)) {
        return Err(Error::BadAttribute(bad.clone()));
    }
    let (packages, programs) = (list(&tidy(&choices.packages)), list(&tidy(&choices.programs)));
    // Only a file that holds something uses the setting, so a system whose
    // Yukimi module predates it still builds.
    let (held, use_held) = if choices.held.is_empty() {
        (String::new(), String::new())
    } else {
        let entries: String = choices
            .held
            .iter()
            .map(|(name, hold)| {
                format!(
                    "    {} = {{\n      flake = {};\n      host = {};\n    }};\n",
                    nix_string(name),
                    nix_string(&hold.flake),
                    nix_string(&hold.host)
                )
            })
            .collect();
        (
            format!(
                "  # Packages kept at the version this computer had, each from the\n  \
                 # saved configuration it was installed with.\n  \
                 held = {{\n{entries}  }};\n"
            ),
            "  imports = [ { programs.yukimi.held = held; } ];\n".to_owned(),
        )
    };
    Ok(format!(
        "# Packages and programs for everyone on this computer, added with Yukimi.\n\
         # Yukimi rewrites this file when they are added or removed there.\n\
         {{ lib, pkgs, ... }}:\n\
         let\n  \
           # Attribute names in Nixpkgs, the names `nix search` shows.\n  \
           packages = {packages};\n  \
           # NixOS programs, each switched on with programs.<name>.enable.\n  \
           programs = {programs};\n\
         {held}\
         in\n\
         {{\n  \
           environment.systemPackages = map (name: lib.getAttrFromPath (lib.splitString \".\" name) pkgs) packages;\n  \
           programs = lib.genAttrs programs (_: {{ enable = true; }});\n\
         {use_held}\
         }}\n"
    ))
}

/// The choices in an existing file. Files from before programs have none.
pub fn read(text: &str) -> Result<Choices> {
    let mut held = BTreeMap::new();
    for (name, mut fields) in let_string_sets(text, "held")?.unwrap_or_default() {
        let (Some(flake), Some(host)) = (fields.remove("flake"), fields.remove("host")) else {
            return Err(Error::NotAString(format!("held.{name}")));
        };
        held.insert(name, Hold { flake, host });
    }
    Ok(Choices {
        packages: let_string_list(text, "packages")?.unwrap_or_default(),
        programs: let_string_list(text, "programs")?.unwrap_or_default(),
        held,
    })
}

/// A configuration file with the file imported.
pub fn import_into(configuration: &str) -> Result<String> {
    add_import(configuration, IMPORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_names() {
        for good in ["htop", "python3Packages.requests", "gnome-tweaks", "_7zz", "kdePackages.kate"] {
            assert!(valid_attribute(good), "{good}");
        }
        for bad in ["", "a b", "a;b", "a..b", ".a", "${x}", "a\"b", "7zip", "../x"] {
            assert!(!valid_attribute(bad), "{bad}");
        }
        assert!(valid_identifier("steam") && !valid_identifier("a.b"));
    }

    #[test]
    fn renders_and_reads_back() {
        let choices = Choices {
            packages: vec!["ripgrep".into(), "htop".into(), "htop".into(), "python3Packages.rich".into()],
            programs: vec!["steam".into()],
            ..Choices::default()
        };
        let text = render(&choices).unwrap();
        assert!(text.contains("    \"htop\"\n    \"python3Packages.rich\"\n    \"ripgrep\"\n"), "{text}");
        let back = read(&text).unwrap();
        assert_eq!(back.packages, vec!["htop", "python3Packages.rich", "ripgrep"]);
        assert_eq!(back.programs, vec!["steam"]);
        assert_eq!(read(&render(&Choices::default()).unwrap()).unwrap(), Choices::default());
        let bad = Choices { packages: vec!["oops; rm".into()], ..Choices::default() };
        assert_eq!(render(&bad), Err(Error::BadAttribute("oops; rm".into())));
        let bad = Choices { programs: vec!["a.b".into()], ..Choices::default() };
        assert_eq!(render(&bad), Err(Error::BadAttribute("a.b".into())));
        assert!(!render(&Choices::default()).unwrap().contains("held"));
    }

    #[test]
    fn held_packages_render_and_read_back() {
        let hold = Hold {
            flake: format!(
                "{}/generation-23?narHash=sha256-Wfvum7uFHXjpM4wMi8MevrUBgwYxtKrd5rAwj4hw1qw%3D",
                "path:/var/lib/yukimi/held"
            ),
            host: "ganymede".into(),
        };
        let choices = Choices {
            packages: vec!["htop".into()],
            held: BTreeMap::from([("codex".into(), hold.clone()), ("python3.13-requests".into(), hold.clone())]),
            ..Choices::default()
        };
        let text = render(&choices).unwrap();
        assert!(text.contains("  imports = [ { programs.yukimi.held = held; } ];\n"), "{text}");
        assert!(text.contains("    \"codex\" = {\n      flake = \"path:/var/lib/yukimi/held/generation-23?"), "{text}");
        assert!(rnix::Root::parse(&text).errors().is_empty(), "{text}");
        assert_eq!(read(&text).unwrap(), Choices { packages: vec!["htop".into()], ..choices.clone() });
        for (name, bad) in [
            ("codex; rm", hold.clone()),
            ("codex", Hold { flake: "path:/etc/nixos?narHash=sha256-x".into(), ..hold.clone() }),
            ("codex", Hold { flake: "path:/var/lib/yukimi/held/../../etc?narHash=sha256-x".into(), ..hold.clone() }),
            ("codex", Hold { flake: "path:/var/lib/yukimi/held/generation-1".into(), ..hold.clone() }),
            ("codex", Hold { host: "a\"b".into(), ..hold.clone() }),
        ] {
            let choices = Choices { held: BTreeMap::from([(name.to_owned(), bad)]), ..Choices::default() };
            assert!(render(&choices).is_err(), "{name}");
        }
    }

    #[test]
    fn reads_files_from_before_programs() {
        let old = "{ lib, pkgs, ... }:\nlet\n  packages = [\n    \"htop\"\n  ];\nin\n{\n  environment.systemPackages = \
                   map (name: lib.getAttrFromPath (lib.splitString \".\" name) pkgs) packages;\n}\n";
        assert_eq!(read(old).unwrap(), Choices { packages: vec!["htop".into()], ..Choices::default() });
    }
}
