// SPDX-License-Identifier: MIT OR Apache-2.0
//! Packages a user installed for themselves with `nix profile`.
//!
//! The profile's current generation has a `manifest.json` listing each
//! package: the flake it came from, the attribute in it, and its store
//! paths. Versions 2 (a list) and 3 (named entries) are both read.
use std::path::{Path, PathBuf};

use serde::Deserialize;
use yukimi_store::StorePath;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// The name `nix profile remove` takes.
    pub name: String,
    /// The attribute it was installed from, such as
    /// `legacyPackages.x86_64-linux.htop`.
    pub attr_path: Option<String>,
    /// The flake it was installed from, as written: `nixpkgs`, `github:…`.
    pub original_url: Option<String>,
    pub store_paths: Vec<StorePath>,
    pub active: bool,
}

impl Element {
    /// The package's attribute in Nixpkgs, without the
    /// `legacyPackages.<system>.` in front.
    pub fn package_attr(&self) -> Option<&str> {
        let attr = self.attr_path.as_deref()?;
        Some(
            attr.strip_prefix("legacyPackages.")
                .or_else(|| attr.strip_prefix("packages."))
                .and_then(|rest| rest.split_once('.').map(|(_, a)| a))
                .unwrap_or(attr),
        )
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    elements: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawElement {
    #[serde(default = "yes")]
    active: bool,
    attr_path: Option<String>,
    original_url: Option<String>,
    #[serde(default)]
    store_paths: Vec<String>,
}

fn yes() -> bool {
    true
}

/// Parse a `manifest.json`.
pub fn parse(text: &str) -> Option<Vec<Element>> {
    let manifest: Manifest = serde_json::from_str(text).ok()?;
    let named: Vec<(String, RawElement)> = match manifest.version {
        3 => serde_json::from_value::<std::collections::BTreeMap<String, RawElement>>(manifest.elements)
            .ok()?
            .into_iter()
            .collect(),
        2 => serde_json::from_value::<Vec<RawElement>>(manifest.elements)
            .ok()?
            .into_iter()
            .enumerate()
            .map(|(i, e)| (i.to_string(), e))
            .collect(),
        _ => return None,
    };
    Some(
        named
            .into_iter()
            .map(|(name, raw)| {
                let store_paths: Vec<StorePath> = raw.store_paths.iter().filter_map(|p| StorePath::parse(p)).collect();
                // Version 2 has no names: use the package's.
                let name = if manifest.version == 2 {
                    store_paths.first().map(|p| p.name().to_owned()).unwrap_or(name)
                } else {
                    name
                };
                Element {
                    name,
                    attr_path: raw.attr_path,
                    original_url: raw.original_url,
                    store_paths,
                    active: raw.active,
                }
            })
            .collect(),
    )
}

/// The current user's profile link: `~/.local/state/nix/profiles/profile`,
/// or the older `~/.nix-profile`.
pub fn user_profile() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let state =
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| Path::new(&home).join(".local/state"));
    [state.join("nix/profiles/profile"), Path::new(&home).join(".nix-profile")]
        .into_iter()
        .find(|p| std::fs::symlink_metadata(p).is_ok())
}

/// The packages in the current user's profile.
pub fn user_elements() -> Vec<Element> {
    user_profile()
        .and_then(|p| std::fs::read_to_string(p.join("manifest.json")).ok())
        .and_then(|text| parse(&text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "0123456789abcdfghijklmnpqrsvwxyz";

    #[test]
    fn version_three() {
        let text = format!(
            r#"{{"version":3,"elements":{{"htop":{{"active":true,"attrPath":"legacyPackages.x86_64-linux.htop",
            "originalUrl":"flake:nixpkgs","outputs":null,"priority":5,"storePaths":["/nix/store/{H}-htop-3.4.1"],
            "url":"github:NixOS/nixpkgs/abc"}}}}}}"#
        );
        let elements = parse(&text).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].name, "htop");
        assert_eq!(elements[0].package_attr(), Some("htop"));
        assert_eq!(elements[0].store_paths[0].version(), "3.4.1");
    }

    #[test]
    fn version_two() {
        let text = format!(
            r#"{{"version":2,"elements":[{{"active":true,"attrPath":"legacyPackages.x86_64-linux.ripgrep",
            "originalUrl":"flake:nixpkgs","storePaths":["/nix/store/{H}-ripgrep-14.1.1"]}}]}}"#
        );
        let elements = parse(&text).unwrap();
        assert_eq!(elements[0].name, "ripgrep");
        assert!(parse(r#"{"version":9,"elements":[]}"#).is_none());
    }
}
