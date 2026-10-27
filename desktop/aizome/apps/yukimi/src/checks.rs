// SPDX-License-Identifier: MIT OR Apache-2.0
//! Checking for updates: asking every source of the system for its newest
//! version without changing anything, keeping the answer, and working out
//! from it how updating would move each source and what to update.
//!
//! The GTK Yukimi's logic (Sekka's `ui/mod.rs` and `ui/updates.rs`), apart
//! from its window.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use yukimi_config::lock::Movement;
use yukimi_system::setup::Kind;
use yukimi_system::updates::{Release, channel_movement, movement};

use crate::model::Model;
use crate::ops::Operation;

/// How old the last check may be before the Updates page checks again by
/// itself: a commit pushed a moment ago should show.
pub const STALE_AFTER: i64 = 10 * 60;

/// What checking for updates found: the newest version of each source (or
/// why it couldn't be asked), and when it looked.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub when: i64,
    /// The configuration it was for: a flake's directory, or `channels`.
    pub of: String,
    pub newest: BTreeMap<String, Release>,
    pub failures: BTreeMap<String, String>,
    /// Where each source came from when it was checked, so that an answer
    /// about a source since pointed elsewhere isn't taken for the new one.
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
}

impl UpdateCheck {
    /// Whether it's old enough to check again by itself.
    pub fn stale(&self, now: i64) -> bool {
        now - self.when > STALE_AFTER
    }
}

/// Where each of the system's sources comes from: what a flake's input asks
/// for, or a channel's name.
pub fn sources(model: &Model) -> BTreeMap<String, String> {
    match model.setup.kind {
        Kind::Flake => model
            .inputs
            .iter()
            .filter(|i| i.follows.is_none())
            .filter_map(|i| Some((i.name.clone(), i.original.as_ref().or(i.locked.as_ref())?.describe())))
            .collect(),
        Kind::Channels => model.channels.iter().map(|c| (c.name.clone(), c.release.clone())).collect(),
    }
}

/// What the last check is about: the flake's directory, or `channels`.
pub fn subject(model: &Model) -> String {
    match model.setup.kind {
        Kind::Flake => model.setup.flake(),
        Kind::Channels => "channels".to_owned(),
    }
}

/// Where the last check for updates is kept.
fn check_file(cache: &Path) -> PathBuf {
    cache.join("updates.json")
}

/// The last check for this configuration: Yukimi's own, or else the GTK
/// Yukimi's (the same answer, from the same sources).
pub fn load(of: &str) -> Option<UpdateCheck> {
    [crate::paths::cache_dir(), crate::paths::shared_cache_dir()]
        .into_iter()
        .flatten()
        .filter_map(|dir| load_from(&dir, of))
        .max_by_key(|check| check.when)
}

/// The check kept in `cache`, if it's about `of`.
pub fn load_from(cache: &Path, of: &str) -> Option<UpdateCheck> {
    let check: UpdateCheck = serde_json::from_str(&std::fs::read_to_string(check_file(cache)).ok()?).ok()?;
    (check.of == of).then_some(check)
}

/// Keep `check` in `cache`.
pub fn save(cache: &Path, check: &UpdateCheck) {
    let _ = std::fs::create_dir_all(cache);
    let _ = std::fs::write(check_file(cache), serde_json::to_string_pretty(check).unwrap_or_default());
}

/// Whether the flake is one Sekka's single update knows (its `flake.nix`
/// takes Sekka, or can be moved to).
pub fn is_sekka(model: &Model) -> bool {
    model.setup.kind == Kind::Flake
        && std::fs::read_to_string(model.setup.dir.join("flake.nix"))
            .is_ok_and(|text| yukimi_config::sekka::supported(&text))
}

/// Ask every source for its newest version, keeping the answers in
/// `cache`. Takes a second or two; nothing is downloaded, except for
/// sources only Nix can ask.
pub fn run(model: &Model) -> Result<UpdateCheck, String> {
    let cache = crate::paths::cache_dir().ok_or("There is no cache directory")?;
    let answers = match model.setup.kind {
        Kind::Flake => {
            let scratch = cache.join("checks");
            let _ = std::fs::remove_dir_all(&scratch);
            std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
            let mut answers = yukimi_system::updates::check_flake(&model.setup.flake(), &model.inputs, &scratch);
            let _ = std::fs::remove_dir_all(&scratch);
            if let Ok(text) = std::fs::read_to_string(model.setup.dir.join("flake.nix"))
                && yukimi_config::sekka::supported(&text)
            {
                yukimi_system::updates::sekka_targets(&text, &model.inputs, &mut answers);
            }
            answers
        }
        Kind::Channels => yukimi_system::updates::check_channels(&model.channels),
    };
    let check = gather(model, yukimi_system::now(), answers)?;
    save(&cache, &check);
    Ok(check)
}

/// A check from the answers of each source. Nothing answering at all
/// (offline, say) is an error, so the last check stays.
pub fn gather(
    model: &Model,
    when: i64,
    answers: BTreeMap<String, Result<Release, String>>,
) -> Result<UpdateCheck, String> {
    let mut check = UpdateCheck { when, of: subject(model), sources: sources(model), ..UpdateCheck::default() };
    for (name, answer) in answers {
        match answer {
            Ok(release) => {
                check.newest.insert(name, release);
            }
            Err(e) => {
                check.failures.insert(name, e);
            }
        }
    }
    if check.newest.is_empty() && !check.failures.is_empty() {
        return Err(check.failures.into_values().next().unwrap_or_default());
    }
    Ok(check)
}

/// How updating would move each source, as the last check found. Sources
/// pointed elsewhere since are left out.
pub fn movements(model: &Model, check: &UpdateCheck) -> BTreeMap<String, Movement> {
    let now = sources(model);
    let unchanged = |name: &str| check.sources.is_empty() || check.sources.get(name) == now.get(name);
    let mut found: BTreeMap<String, Movement> = match model.setup.kind {
        Kind::Flake => model
            .inputs
            .iter()
            .filter_map(|input| {
                let locked = input.locked.as_ref().filter(|_| input.follows.is_none())?;
                Some((input.name.clone(), movement(locked, check.newest.get(&input.name)?)))
            })
            .collect(),
        Kind::Channels => model
            .channels
            .iter()
            .filter_map(|channel| {
                Some((channel.name.clone(), channel_movement(channel, check.newest.get(&channel.name)?)))
            })
            .collect(),
    };
    found.retain(|name, _| unchanged(name));
    found
}

/// How many sources have a newer version.
pub fn newer_count(movements: &BTreeMap<String, Movement>) -> usize {
    movements.values().filter(|m| matches!(m, Movement::Newer(_))).count()
}

/// The sources the Update button updates: every one with a newer version,
/// and on a Sekka system Sekka too (its single update also retries getting
/// ready, and moves an older installation over), unless Sekka would go
/// back.
pub fn to_update(model: &Model, movements: Option<&BTreeMap<String, Movement>>, sekka: bool) -> Vec<String> {
    let mut newer: Vec<String> = movements
        .map(|m| m.iter().filter(|(_, m)| matches!(m, Movement::Newer(_))).map(|(name, _)| name.clone()).collect())
        .unwrap_or_default();
    if model.setup.kind != Kind::Channels
        && sekka
        && !newer.iter().any(|input| input == "sekka")
        && !movements.is_some_and(|m| matches!(m.get("sekka"), Some(Movement::Older(_))))
    {
        newer.push("sekka".into());
    }
    newer
}

/// What updating `inputs` is: the channels; Sekka's single update (which
/// also moves an installation from Sekka's predecessors over); or the
/// flake's chosen inputs.
pub fn update_operation(model: &Model, inputs: Vec<String>, sekka: bool) -> Operation {
    let migrating = !model.inputs.iter().any(|i| i.name == "sekka");
    if model.setup.kind == Kind::Channels {
        Operation::UpdateChannels
    } else if sekka && (inputs.iter().any(|i| i == "sekka") || migrating) {
        Operation::UpdateSekka {
            inputs: inputs
                .into_iter()
                .filter(|i| !migrating || !["calamares", "tatami", "yukimi"].contains(&i.as_str()))
                .collect(),
        }
    } else {
        Operation::UpdateFlake { inputs }
    }
}

/// The sources that would go back, by name.
pub fn older(movements: &BTreeMap<String, Movement>) -> Vec<&str> {
    movements.iter().filter(|(_, m)| matches!(m, Movement::Older(_))).map(|(name, _)| name.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yukimi_config::lock::{Input, Locked};

    fn input(name: &str, rev: &str, modified: i64) -> Input {
        let locked = Locked {
            kind: "github".into(),
            owner: Some("NixOS".into()),
            repo: Some(name.into()),
            rev: Some(rev.into()),
            last_modified: Some(modified),
            ..Locked::default()
        };
        Input {
            name: name.into(),
            original: Some(locked.clone()),
            locked: Some(locked),
            follows: None,
            follows_inputs: Vec::new(),
        }
    }

    fn release(rev: &str, modified: i64) -> Release {
        Release { rev: Some(rev.into()), last_modified: Some(modified), ..Release::default() }
    }

    fn flake_model() -> Model {
        let mut model = Model::default();
        model.setup.kind = Kind::Flake;
        model.inputs = vec![input("nixpkgs", "aaa", 100), input("home-manager", "bbb", 100), input("sops", "ccc", 100)];
        model
    }

    #[test]
    fn checks_say_how_each_source_moves() {
        let model = flake_model();
        let answers = BTreeMap::from([
            ("nixpkgs".to_owned(), Ok(release("ddd", 200))),
            ("home-manager".to_owned(), Ok(release("bbb", 100))),
            ("sops".to_owned(), Ok(release("eee", 50))),
        ]);
        let check = gather(&model, 1000, answers).unwrap();
        let moves = movements(&model, &check);
        assert!(matches!(moves["nixpkgs"], Movement::Newer(_)));
        assert_eq!(moves["home-manager"], Movement::Current);
        assert!(matches!(moves["sops"], Movement::Older(_)));
        assert_eq!(newer_count(&moves), 1);
        assert_eq!(older(&moves), ["sops"]);
        assert_eq!(to_update(&model, Some(&moves), false), ["nixpkgs"]);
        assert_eq!(
            update_operation(&model, vec!["nixpkgs".into()], false),
            Operation::UpdateFlake { inputs: vec!["nixpkgs".into()] }
        );
        assert!(!check.stale(1000 + STALE_AFTER) && check.stale(1001 + STALE_AFTER));
    }

    #[test]
    fn a_source_pointed_elsewhere_since_has_no_answer() {
        let mut model = flake_model();
        let answers = BTreeMap::from([("nixpkgs".to_owned(), Ok(release("ddd", 200)))]);
        let check = gather(&model, 1000, answers).unwrap();
        model.inputs[0].original.as_mut().unwrap().reference = Some("nixos-unstable-small".into());
        assert!(!movements(&model, &check).contains_key("nixpkgs"));
    }

    #[test]
    fn nothing_answering_keeps_the_last_check() {
        let model = flake_model();
        let answers = BTreeMap::from([("nixpkgs".to_owned(), Err("api.github.com couldn't be reached".to_owned()))]);
        assert_eq!(gather(&model, 1000, answers).unwrap_err(), "api.github.com couldn't be reached");
        let some = BTreeMap::from([
            ("nixpkgs".to_owned(), Err("offline".to_owned())),
            ("sops".to_owned(), Ok(release("ccc", 100))),
        ]);
        assert_eq!(gather(&model, 1000, some).unwrap().failures["nixpkgs"], "offline");
    }

    #[test]
    fn sekka_updates_in_one_step_unless_it_would_go_back() {
        let mut model = flake_model();
        model.inputs.push(input("sekka", "fff", 100));
        assert_eq!(to_update(&model, None, true), ["sekka"]);
        assert_eq!(
            update_operation(&model, vec!["sekka".into(), "nixpkgs".into()], true),
            Operation::UpdateSekka { inputs: vec!["sekka".into(), "nixpkgs".into()] }
        );
        let older = BTreeMap::from([("sekka".to_owned(), Movement::Older(Locked::default()))]);
        assert!(to_update(&model, Some(&older), true).is_empty());
        // An installation from before Sekka moves over, leaving behind the
        // inputs Sekka takes the place of.
        let mut old = flake_model();
        old.inputs.push(input("tatami", "ggg", 100));
        assert_eq!(
            update_operation(&old, vec!["tatami".into(), "nixpkgs".into()], true),
            Operation::UpdateSekka { inputs: vec!["nixpkgs".into()] }
        );
    }

    #[test]
    fn checks_are_kept_for_their_configuration() {
        let dir = std::env::temp_dir().join(format!("aizome-yukimi-checks-{}", std::process::id()));
        let check = UpdateCheck { when: 5, of: "/etc/nixos".into(), ..UpdateCheck::default() };
        save(&dir, &check);
        assert_eq!(load_from(&dir, "/etc/nixos"), Some(check));
        assert_eq!(load_from(&dir, "channels"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
