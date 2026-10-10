// SPDX-License-Identifier: MIT OR Apache-2.0
//! Applications to offer before anything is searched for: a few dozen
//! well-known ones with friendly names, descriptions and categories.
//!
//! Yukimi has its own catalog, whose applications it installs as it
//! installs any package (or, for those that need more, by switching on
//! their NixOS program). A system can bring catalogs of its own through
//! Yukimi's NixOS module, as an installer does for the applications it
//! offered: those are installed through a setting of the system's, a list
//! of application ids, and take the place of Yukimi's own entries for the
//! same applications.
use serde::Deserialize;

use crate::setup::CatalogSource;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    /// The packages it installs, as attributes in Nixpkgs.
    #[serde(default)]
    pub packages: Vec<String>,
    /// The NixOS program it switches on, for applications that need more
    /// than their package: `steam`.
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub unfree: bool,
    /// Other applications this one selects too.
    #[serde(default)]
    pub requires: Vec<String>,
    /// For applications from a system's catalog, the setting that installs
    /// them, such as `calamares.applications`.
    #[serde(skip)]
    pub setting: Option<String>,
}

impl Application {
    /// The names its packages and program go by in a store path: what to
    /// look for in the running system to tell whether it is installed.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.packages.iter().map(|p| p.rsplit('.').next().unwrap_or(p)).collect();
        names.extend(self.program.as_deref());
        names
    }
}

/// Yukimi's own catalog.
pub fn own() -> Vec<Application> {
    parse(include_str!("../data/applications.json")).unwrap_or_default()
}

pub fn parse(text: &str) -> Option<Vec<Application>> {
    serde_json::from_str(text).ok()
}

/// Yukimi's catalog and the system's, with the system's entries in place of
/// Yukimi's for the same application (the same name, or the same first
/// package).
pub fn load(sources: &[CatalogSource]) -> Vec<Application> {
    let mut theirs: Vec<Application> = Vec::new();
    for source in sources {
        let setting = source.setting.clone();
        if setting.as_deref().is_some_and(|s| !valid_setting(s)) {
            continue;
        }
        let Some(apps) = std::fs::read_to_string(&source.file).ok().and_then(|text| parse(&text)) else {
            continue;
        };
        theirs.extend(
            apps.into_iter().filter(|a| valid_id(&a.id)).map(|a| Application { setting: setting.clone(), ..a }),
        );
    }
    let same = |a: &Application, b: &Application| {
        a.name.eq_ignore_ascii_case(&b.name) || (!a.packages.is_empty() && a.packages.first() == b.packages.first())
    };
    let mut all: Vec<Application> = own().into_iter().filter(|mine| !theirs.iter().any(|t| same(mine, t))).collect();
    // The system's applications come first: someone chose to offer them.
    theirs.append(&mut all);
    theirs
}

/// Whether an id is one Yukimi will write into the configuration.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether a setting is a plain dotted name Yukimi will look for.
pub fn valid_setting(setting: &str) -> bool {
    setting.split('.').count() >= 2 && setting.split('.').all(yukimi_config::packages::valid_identifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_catalog_is_sound() {
        let apps = own();
        assert!(apps.len() >= 40, "{}", apps.len());
        for app in &apps {
            assert!(valid_id(&app.id), "{}", app.id);
            assert!(!app.packages.is_empty() || app.program.is_some(), "{} installs nothing", app.id);
            for package in &app.packages {
                assert!(yukimi_config::packages::valid_attribute(package), "{package}");
            }
            if let Some(program) = &app.program {
                assert!(yukimi_config::packages::valid_identifier(program), "{program}");
            }
            assert!(app.setting.is_none());
        }
        let mut ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), apps.len(), "ids repeat");
    }

    #[test]
    fn reads_an_installers_catalog_in_place_of_its_own() {
        let dir = std::env::temp_dir().join(format!("yukimi-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("applications.json");
        std::fs::write(
            &file,
            r#"[{"id":"firefox","name":"Firefox","description":"Browse.","category":"Browsers","packages":["firefox"]},
                {"id":"rustup","name":"Rustup","description":"Toolchains.","category":"Development tools","packages":["rustup"],"requires":["build-tools"]},
                {"id":"Bad Id","name":"Bad","description":"","category":"x"}]"#,
        )
        .unwrap();
        let sources = [CatalogSource {
            file: file.to_string_lossy().into_owned(),
            setting: Some("calamares.applications".into()),
            title: None,
        }];
        let apps = load(&sources);
        let _ = std::fs::remove_dir_all(&dir);
        let firefoxes: Vec<&Application> = apps.iter().filter(|a| a.name == "Firefox").collect();
        assert_eq!(firefoxes.len(), 1);
        assert_eq!(firefoxes[0].setting.as_deref(), Some("calamares.applications"));
        assert_eq!(apps[1].requires, vec!["build-tools"]);
        assert!(!apps.iter().any(|a| a.id == "Bad Id"));
        assert!(apps.iter().any(|a| a.id == "steam" && a.program.as_deref() == Some("steam")));
        assert!(valid_setting("calamares.applications") && !valid_setting("x") && !valid_setting("a.${b}"));
    }
}
