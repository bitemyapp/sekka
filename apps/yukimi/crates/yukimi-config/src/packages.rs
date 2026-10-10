// SPDX-License-Identifier: MIT OR Apache-2.0
//! `yukimi.nix`: what Yukimi installs for everyone.
//!
//! A small module of its own next to the configuration file that imports
//! it, so Yukimi never has to rewrite anything a person wrote. It holds two
//! lists: packages, as their attribute names in Nixpkgs (`htop`,
//! `python3Packages.requests`), and NixOS programs, each switched on with
//! `programs.<name>.enable` (Steam, say, needs more than its package). Both
//! are checked before they are written.
use crate::edit::{add_import, let_string_list, nix_string};
use crate::{Error, Result};

/// The file, next to the configuration file that imports it.
pub const FILE: &str = "yukimi.nix";
/// How that file imports it.
pub const IMPORT: &str = "./yukimi.nix";

/// What `yukimi.nix` installs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    pub packages: Vec<String>,
    pub programs: Vec<String>,
}

impl Choices {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.programs.is_empty()
    }
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
    let (packages, programs) = (list(&tidy(&choices.packages)), list(&tidy(&choices.programs)));
    Ok(format!(
        "# Packages and programs for everyone on this computer, added with Yukimi.\n\
         # Yukimi rewrites this file when they are added or removed there.\n\
         {{ lib, pkgs, ... }}:\n\
         let\n  \
           # Attribute names in Nixpkgs, the names `nix search` shows.\n  \
           packages = {packages};\n  \
           # NixOS programs, each switched on with programs.<name>.enable.\n  \
           programs = {programs};\n\
         in\n\
         {{\n  \
           environment.systemPackages = map (name: lib.getAttrFromPath (lib.splitString \".\" name) pkgs) packages;\n  \
           programs = lib.genAttrs programs (_: {{ enable = true; }});\n\
         }}\n"
    ))
}

/// The choices in an existing file. Files from before programs have none.
pub fn read(text: &str) -> Result<Choices> {
    Ok(Choices {
        packages: let_string_list(text, "packages")?.unwrap_or_default(),
        programs: let_string_list(text, "programs")?.unwrap_or_default(),
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
        };
        let text = render(&choices).unwrap();
        assert!(text.contains("    \"htop\"\n    \"python3Packages.rich\"\n    \"ripgrep\"\n"), "{text}");
        let back = read(&text).unwrap();
        assert_eq!(back.packages, vec!["htop", "python3Packages.rich", "ripgrep"]);
        assert_eq!(back.programs, vec!["steam"]);
        assert_eq!(read(&render(&Choices::default()).unwrap()).unwrap(), Choices::default());
        let bad = Choices { packages: vec!["oops; rm".into()], programs: vec![] };
        assert_eq!(render(&bad), Err(Error::BadAttribute("oops; rm".into())));
        let bad = Choices { packages: vec![], programs: vec!["a.b".into()] };
        assert_eq!(render(&bad), Err(Error::BadAttribute("a.b".into())));
    }

    #[test]
    fn reads_files_from_before_programs() {
        let old = "{ lib, pkgs, ... }:\nlet\n  packages = [\n    \"htop\"\n  ];\nin\n{\n  environment.systemPackages = \
                   map (name: lib.getAttrFromPath (lib.splitString \".\" name) pkgs) packages;\n}\n";
        assert_eq!(read(old).unwrap(), Choices { packages: vec!["htop".into()], programs: vec![] });
    }
}
