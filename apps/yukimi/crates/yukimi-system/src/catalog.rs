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
//!
//! An application that needs its NixOS program needs it however it is
//! found: a search for Steam's package installs the catalog's Steam, never
//! the package alone, which stops before its window opens without the
//! 32-bit graphics drivers its program switches on.
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
    /// What its program sets up that the package alone can't, to say why it
    /// can only be installed for everyone: `32-bit graphics drivers and
    /// device rules for its controllers`.
    #[serde(default)]
    pub needs: Option<String>,
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

    /// Whether a search names this application: what was typed begins its
    /// name, its id, one of its packages or its program, or a word in any
    /// of them. A single letter names nothing.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.chars().count() < 2 {
            return false;
        }
        let begins = |text: &str| {
            let text = text.to_lowercase();
            text.starts_with(&query)
                || text.match_indices(&query).any(|(i, _)| text[..i].ends_with([' ', '-', '_', '.']))
        };
        [&self.name, &self.id].into_iter().chain(&self.packages).chain(&self.program).any(|t| begins(t))
    }
}

/// The application to install in place of a Nixpkgs package that can't be
/// installed on its own: one that switches on a NixOS program, whose
/// program or one of whose packages it is.
pub fn needing_system<'a>(catalog: &'a [Application], attr: &str) -> Option<&'a Application> {
    catalog
        .iter()
        .find(|a| a.program.as_deref().is_some_and(|program| program == attr || a.packages.iter().any(|p| p == attr)))
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
/// package). An entry taking the place of one that switches on a program
/// keeps that program, and what it needs: the system's setting installs
/// Steam, and Steam still can't be installed as its package alone.
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
    let own = own();
    for app in theirs.iter_mut().filter(|a| a.program.is_none()) {
        if let Some(mine) = own.iter().find(|mine| mine.program.is_some() && same(mine, app)) {
            app.program = mine.program.clone();
            app.needs = app.needs.take().or_else(|| mine.needs.clone());
        }
    }
    let mut all: Vec<Application> = own.into_iter().filter(|mine| !theirs.iter().any(|t| same(mine, t))).collect();
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
                assert!(app.needs.is_some(), "{} doesn't say what its program sets up", app.id);
            }
            assert!(app.setting.is_none());
        }
        let mut ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), apps.len(), "ids repeat");
    }

    /// Yukimi's catalog with a system's, installed through
    /// `calamares.applications`, read from a file of its own.
    fn with_system_catalog(test: &str, json: &str) -> Vec<Application> {
        let dir = std::env::temp_dir().join(format!("yukimi-catalog-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("applications.json");
        std::fs::write(&file, json).unwrap();
        let sources = [CatalogSource {
            file: file.to_string_lossy().into_owned(),
            setting: Some("calamares.applications".into()),
            title: None,
        }];
        let apps = load(&sources);
        let _ = std::fs::remove_dir_all(&dir);
        apps
    }

    #[test]
    fn reads_an_installers_catalog_in_place_of_its_own() {
        let apps = with_system_catalog(
            "installer",
            r#"[{"id":"firefox","name":"Firefox","description":"Browse.","category":"Browsers","packages":["firefox"]},
                {"id":"rustup","name":"Rustup","description":"Toolchains.","category":"Development tools","packages":["rustup"],"requires":["build-tools"]},
                {"id":"uta","name":"Uta for YouTube Music","description":"Downloads when selected.","category":"Media & creativity","source":"sekka","packages":["uta"],"preload":false},
                {"id":"Bad Id","name":"Bad","description":"","category":"x"}]"#,
        );
        let firefoxes: Vec<&Application> = apps.iter().filter(|a| a.name == "Firefox").collect();
        assert_eq!(firefoxes.len(), 1);
        assert_eq!(firefoxes[0].setting.as_deref(), Some("calamares.applications"));
        assert_eq!(apps[1].requires, vec!["build-tools"]);
        let uta = apps.iter().find(|app| app.id == "uta").unwrap();
        assert_eq!(uta.setting.as_deref(), Some("calamares.applications"));
        assert!(uta.matches("uta") && uta.matches("youtube"));
        assert!(!uta.unfree);
        assert!(!apps.iter().any(|a| a.id == "Bad Id"));
        assert!(apps.iter().any(|a| a.id == "steam" && a.program.as_deref() == Some("steam")));
        assert!(valid_setting("calamares.applications") && !valid_setting("x") && !valid_setting("a.${b}"));
    }

    #[test]
    fn a_programs_package_belongs_to_its_application() {
        let apps = own();
        let steam = needing_system(&apps, "steam").expect("Steam's package needs its program");
        assert_eq!(steam.id, "steam");
        assert_eq!(steam.program.as_deref(), Some("steam"));
        assert!(steam.setting.is_none());
        assert!(steam.needs.as_deref().is_some_and(|n| n.contains("32-bit graphics")));
        assert_eq!(needing_system(&apps, "localsend").map(|a| a.id.as_str()), Some("localsend"));
    }

    #[test]
    fn ordinary_packages_install_on_their_own() {
        let apps = own();
        // Catalog applications without a program, Steam's relatives, and
        // packages the catalog doesn't know.
        for attr in
            ["firefox", "lutris", "kdePackages.kdenlive", "steam-run", "steam-unwrapped", "steamcmd", "htop", ""]
        {
            assert!(needing_system(&apps, attr).is_none(), "{attr}");
        }
    }

    #[test]
    fn an_installers_steam_keeps_its_program() {
        // Genkan offers Steam by its package and switches on programs.steam
        // through its own setting.
        let apps = with_system_catalog(
            "steam",
            r#"[{"id":"firefox","name":"Firefox","description":"Browse.","category":"Browsers","packages":["firefox"]},
                {"id":"steam","name":"Steam","description":"Play.","category":"Gaming","packages":["steam"],"unfree":true}]"#,
        );
        assert_eq!(apps.iter().filter(|a| a.name == "Steam").count(), 1);
        let steam = needing_system(&apps, "steam").expect("the installer's Steam");
        assert_eq!(steam.setting.as_deref(), Some("calamares.applications"));
        assert_eq!(steam.category, "Gaming");
        assert_eq!(steam.program.as_deref(), Some("steam"));
        assert_eq!(steam.needs, own().into_iter().find(|a| a.id == "steam").unwrap().needs);
        assert!(needing_system(&apps, "firefox").is_none());
        assert!(apps.iter().find(|a| a.id == "firefox").is_some_and(|a| a.program.is_none()));
    }

    #[test]
    fn searches_name_applications() {
        let apps = own();
        let found = |query: &str| apps.iter().filter(|a| a.matches(query)).map(|a| a.id.as_str()).collect::<Vec<_>>();
        assert_eq!(found("steam"), ["steam"]);
        assert_eq!(found(" STEAM "), ["steam"]);
        assert_eq!(found("fire"), ["firefox"]);
        assert_eq!(found("studio"), ["obs-studio", "vscode"]);
        assert_eq!(found("editor"), ["zed"]);
        assert!(found("eam").is_empty(), "not at the start of a word");
        assert!(found("s").is_empty() && found("  ").is_empty());
    }
}
