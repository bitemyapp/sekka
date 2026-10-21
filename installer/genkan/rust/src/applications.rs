// SPDX-License-Identifier: GPL-3.0-or-later
//! The GUI, privileged parser, and Nix module share one packaged catalog.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeSet, sync::OnceLock};

pub const CATALOG_JSON: &str = include_str!("applications.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub packages: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub unfree: bool,
    #[serde(default)]
    pub terminal: Option<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    /// Nix systems supported by the package; an absent list means every
    /// architecture Sekka supports. The Nix module reads this same metadata.
    #[serde(default)]
    pub systems: Option<Vec<String>>,
}

impl Application {
    fn supports(&self, system: &str) -> bool {
        self.systems
            .as_ref()
            .is_none_or(|systems| systems.iter().any(|supported| supported == system))
    }
}

fn target_system() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64-linux"
    } else {
        panic!("unsupported Sekka installer architecture")
    }
}

fn for_system(system: &str) -> Vec<Application> {
    let catalog: Vec<Application> =
        serde_json::from_str(CATALOG_JSON).expect("packaged application catalog");
    catalog
        .into_iter()
        .filter(|app| app.supports(system))
        .collect()
}

pub fn catalog() -> &'static [Application] {
    static CATALOG: OnceLock<Vec<Application>> = OnceLock::new();
    CATALOG.get_or_init(|| for_system(target_system()))
}

pub fn default_selection() -> Vec<String> {
    vec!["firefox".into()]
}

pub struct ApplicationSelection(Vec<&'static Application>);
impl ApplicationSelection {
    pub fn parse(ids: Vec<String>, allow_unfree: bool) -> Result<Self> {
        ensure!(
            ids.len() <= catalog().len(),
            "Too many application selections"
        );
        let mut selected: BTreeSet<String> = BTreeSet::new();
        for id in ids {
            ensure!(
                catalog().iter().any(|app| app.id == id),
                "Unknown application: {id}"
            );
            ensure!(selected.insert(id.clone()), "Duplicate application: {id}");
        }
        // Normalize dependencies before review and again in the helper.
        loop {
            let before = selected.len();
            for app in catalog() {
                if selected.contains(&app.id) {
                    selected.extend(app.requires.iter().cloned());
                }
            }
            if selected.len() == before {
                break;
            }
        }
        let apps: Vec<_> = catalog()
            .iter()
            .filter(|app| selected.contains(&app.id))
            .collect();
        for app in &apps {
            ensure!(
                allow_unfree || !app.unfree,
                "{} requires allowing proprietary software on the Desktops & Wi-Fi tab",
                app.name
            );
        }
        Ok(Self(apps))
    }
    pub fn selected(&self) -> &[&'static Application] {
        &self.0
    }
    pub fn ids(&self) -> Vec<String> {
        self.0.iter().map(|app| app.id.clone()).collect()
    }
    pub fn names(&self) -> String {
        self.0
            .iter()
            .map(|app| app.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_has_unique_ids_and_valid_dependencies() {
        let all: Vec<Application> = serde_json::from_str(CATALOG_JSON).unwrap();
        let ids: BTreeSet<_> = all.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(ids.len(), all.len());
        for app in &all {
            assert!(!app.name.is_empty() && !app.description.is_empty());
            assert!(
                app.id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            );
            assert!(app.requires.iter().all(|id| ids.contains(id.as_str())));
            assert!(matches!(
                app.source.as_deref(),
                None | Some("ai" | "omp" | "sekka")
            ));
            if let Some(systems) = &app.systems {
                assert!(!systems.is_empty());
                assert!(
                    systems
                        .iter()
                        .all(|system| matches!(system.as_str(), "x86_64-linux" | "aarch64-linux"))
                );
            }
        }
        for system in ["x86_64-linux", "aarch64-linux"] {
            let offered = for_system(system);
            let offered_ids: BTreeSet<_> = offered.iter().map(|app| app.id.as_str()).collect();
            assert!(offered.iter().all(|app| {
                app.requires
                    .iter()
                    .all(|id| offered_ids.contains(id.as_str()))
            }));
        }
        for removed in ["thunderbird", "keepassxc", "gemini-cli", "orbstack"] {
            assert!(!ids.contains(removed));
        }
    }
    #[test]
    fn rustup_implies_build_tools_and_roundtrips_without_duplicates() {
        let apps = ApplicationSelection::parse(vec!["rustup".into()], false).unwrap();
        assert_eq!(apps.ids(), ["build-tools", "rustup"]);
        assert_eq!(
            ApplicationSelection::parse(apps.ids(), false)
                .unwrap()
                .ids(),
            apps.ids()
        );
    }
    #[test]
    fn rejects_unknown_duplicate_and_disallowed_unfree_selections() {
        for ids in [
            vec!["unknown".into()],
            vec!["firefox".into(); 2],
            vec!["google-chrome".into()],
        ] {
            assert!(ApplicationSelection::parse(ids, false).is_err());
        }
        assert!(ApplicationSelection::parse(vec!["google-chrome".into()], true).is_ok());
        assert!(
            ApplicationSelection::parse(vec![], false)
                .unwrap()
                .selected()
                .is_empty()
        );
    }

    #[test]
    fn architecture_limits_apply_to_the_gui_and_privileged_parser() {
        let arm = for_system("aarch64-linux");
        let x86 = for_system("x86_64-linux");
        for id in ["discord", "slack", "spotify", "steam", "superluminal"] {
            assert!(x86.iter().any(|app| app.id == id));
            assert!(!arm.iter().any(|app| app.id == id));
            if target_system() == "aarch64-linux" {
                assert!(ApplicationSelection::parse(vec![id.into()], true).is_err());
            }
        }
        assert!(arm.iter().any(|app| app.id == "firefox"));
        assert_eq!(
            catalog().iter().map(|app| &app.id).collect::<Vec<_>>(),
            for_system(target_system())
                .iter()
                .map(|app| &app.id)
                .collect::<Vec<_>>()
        );
    }
}
