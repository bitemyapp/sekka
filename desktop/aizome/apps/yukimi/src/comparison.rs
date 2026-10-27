//! Read-only source and package drill-down for one exact comparison range.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use yukimi_config::lock::{FlakeLock, InputTree, Locked};
use yukimi_system::diff::{Change, ChangeKind};

use crate::model::{Model, Store};
use crate::update_history::{Link, same_repo};

#[derive(Clone, Debug)]
pub struct Delta {
    pub name: String,
    pub before: Option<Locked>,
    pub after: Option<Locked>,
    pub children: Vec<Delta>,
}

impl Delta {
    pub fn url(&self) -> Option<String> {
        let (before, after) = (self.before.as_ref()?, self.after.as_ref()?);
        same_repo(before, after).then_some(())?;
        before.compare_url(after.rev.as_deref()?)
    }

    pub fn versions(&self) -> String {
        let revision = |s: &Option<Locked>| {
            s.as_ref().map(|s| s.short_rev().unwrap_or("unversioned")).unwrap_or("absent").to_owned()
        };
        format!("{} → {}", revision(&self.before), revision(&self.after))
    }

    pub fn status(&self) -> &'static str {
        match (&self.before, &self.after) {
            (None, Some(_)) => "Added",
            (Some(_), None) => "Removed",
            (Some(a), Some(b)) if !same_repo(a, b) => "Source changed",
            (Some(a), Some(b)) if unchanged(a, b) => "Inputs changed",
            _ => "Updated",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Details {
    pub root: Delta,
    pub source_note: String,
    pub source_error: Option<String>,
    pub packages: Result<Vec<Change>, String>,
}

fn unchanged(a: &Locked, b: &Locked) -> bool {
    same_repo(a, b) && a.rev == b.rev && a.nar_hash == b.nar_hash
}

fn delta(name: &str, before: Option<&InputTree>, after: Option<&InputTree>) -> Option<Delta> {
    let names: BTreeSet<&String> = before
        .into_iter()
        .flat_map(|t| t.children.keys())
        .chain(after.into_iter().flat_map(|t| t.children.keys()))
        .collect();
    let children: Vec<_> = names
        .into_iter()
        .filter_map(|name| {
            delta(name, before.and_then(|t| t.children.get(name)), after.and_then(|t| t.children.get(name)))
        })
        .collect();
    let (old, new) = (before.and_then(|t| t.locked.clone()), after.and_then(|t| t.locked.clone()));
    let same = match (&old, &new) {
        (Some(a), Some(b)) => unchanged(a, b),
        (None, None) => true,
        _ => false,
    };
    (!same || !children.is_empty()).then(|| Delta { name: name.into(), before: old, after: new, children })
}

fn saved_tree(file: &Path, name: &str, expected: &Locked) -> Result<InputTree, String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let tree = FlakeLock::parse(&text).and_then(|lock| lock.input_tree(&[name])).map_err(|e| e.to_string())?;
    if !tree.locked.as_ref().is_some_and(|locked| same_repo(locked, expected) && locked.rev == expected.rev) {
        return Err("The saved source no longer matches this comparison.".into());
    }
    Ok(tree)
}

fn source_tree(
    model: &Model,
    name: &str,
    expected: &Locked,
    generation: Option<u32>,
    after: bool,
    remote: bool,
) -> Result<(InputTree, String), String> {
    if let Some(number) = generation {
        let tree = saved_tree(&yukimi_system::saved_configuration(number).join("flake.lock"), name, expected)?;
        return Ok((tree, format!("generation {number}")));
    }
    if after && let Ok(tree) = saved_tree(&model.setup.dir.join("flake.lock"), name, expected) {
        return Ok((tree, "configured inputs".into()));
    }
    if !model.paths.elsewhere {
        // Only use the relevant selected/running snapshot; a matching revision
        // in an unrelated older generation may have different input overrides.
        if let Some(g) = model
            .generations
            .iter()
            .filter(|g| if after { g.current } else { g.active })
            .max_by_key(|g| (g.current, g.number))
            && let Ok(tree) =
                saved_tree(&yukimi_system::saved_configuration(g.number).join("flake.lock"), name, expected)
        {
            return Ok((tree, format!("generation {}", g.number)));
        }
    }
    if !remote {
        return Err("No saved input details are available for this comparison.".into());
    }
    let text = yukimi_system::updates::revision_lock(expected)?;
    let mut tree = FlakeLock::parse(&text).and_then(|lock| lock.input_tree(&[])).map_err(|e| e.to_string())?;
    tree.locked = Some(expected.clone());
    Ok((tree, "repository lock at this commit".into()))
}

/// Runs on a worker: opening the sheet never builds or evaluates a flake.
pub fn load(model: &Model, name: &str, link: &Link, remote: bool) -> Details {
    let mut root =
        Delta { name: name.into(), before: link.before.clone(), after: Some(link.after.clone()), children: vec![] };
    let sources = (|| {
        let before = link.before.as_ref().ok_or("No previous revision was recorded.")?;
        let (old, old_note) = source_tree(model, name, before, link.generations.map(|g| g.0), false, remote)?;
        let (new, new_note) = source_tree(model, name, &link.after, link.generations.map(|g| g.1), true, remote)?;
        if let Some(changed) = delta(name, Some(&old), Some(&new)) {
            root.children = changed.children;
        }
        Ok::<_, String>(format!("Inputs: {old_note} → {new_note}."))
    })();
    let (source_note, source_error) = match sources {
        Ok(note) => (note, None),
        Err(error) => (String::new(), Some(error)),
    };
    Details { root, source_note, source_error, packages: packages(model, link.generations) }
}

fn packages(model: &Model, generations: Option<(u32, u32)>) -> Result<Vec<Change>, String> {
    let (before, after) = generations.ok_or("Package changes are available once both endpoints have saved system generations. This comparison does not build the update.")?;
    let store = model
        .store
        .as_ref()
        .ok_or("The package store is still loading. Close and reopen this comparison in a moment.")?;
    let generation =
        |number| model.generations.iter().find(|g| g.number == number).ok_or("This generation is no longer available.");
    let (before, after) = (generation(before)?, generation(after)?);
    if store.graph.lookup(before.target.as_str()).is_none() || store.graph.lookup(after.target.as_str()).is_none() {
        return Err("One of these generations is no longer in the package store.".into());
    }
    Ok(package_changes(store, &store.closure_of(&before.target), &store.closure_of(&after.target)))
}

/// Include rebuilds even when the version string is unchanged. A changed
/// output hash is evidence of a different build, not of an upstream commit.
fn package_changes(store: &Store, before: &[yukimi_store::PathId], after: &[yukimi_store::PathId]) -> Vec<Change> {
    let mut changes = yukimi_system::diff::diff(&store.graph, before, after).changes;
    let paths = |ids: &[yukimi_store::PathId]| {
        let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for &id in ids {
            if let Some(path) = store.graph.store_path(id)
                && !path.is_derivation()
                && !path.version().is_empty()
                && !path.name().starts_with("nixos-system-")
            {
                out.entry(path.name().into()).or_default().insert(path.as_str().into());
            }
        }
        out
    };
    let (old, new) = (paths(before), paths(after));
    for (name, old_paths) in old {
        if let Some(new_paths) = new.get(&name)
            && old_paths != *new_paths
            && !changes.iter().any(|c| c.name == name)
        {
            let versions = |paths: &BTreeSet<String>| {
                paths
                    .iter()
                    .filter_map(|p| yukimi_store::StorePath::parse(p))
                    .map(|p| p.version().to_owned())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            };
            changes.push(Change {
                name,
                before: versions(&old_paths),
                after: versions(new_paths),
                kind: ChangeKind::Changed,
                size_delta: 0,
            });
        }
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(rev: &str, node_id: &str) -> FlakeLock {
        FlakeLock::parse(
            &serde_json::json!({
                "root": "root", "nodes": {
                    "root": {"inputs": {"sekka": "sekka", "shared": "library"}},
                    "sekka": {"locked": {"type":"github", "owner":"example", "repo":"sekka", "rev":"parent"},
                        "inputs": {"aizome": node_id, "library": ["shared"]}},
                    node_id: {"locked": {"type":"github", "owner":"example", "repo":"aizome", "rev":rev},
                        "inputs": {"library": ["shared"]}},
                    "library": {"locked": {"type":"github", "owner":"example", "repo":"library", "rev":"same"}}
                }
            })
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn nested_deltas_use_input_names_and_resolve_follows() {
        let before = lock("old", "aizome_2").input_tree(&["sekka"]).unwrap();
        let after = lock("new", "aizome_9").input_tree(&["sekka"]).unwrap();
        assert_eq!(before.children["library"].locked.as_ref().unwrap().rev.as_deref(), Some("same"));
        let delta = delta("sekka", Some(&before), Some(&after)).unwrap();
        assert_eq!(delta.status(), "Inputs changed");
        assert_eq!(delta.children.len(), 1);
        assert_eq!(delta.children[0].name, "aizome");
        assert_eq!(delta.children[0].url().as_deref(), Some("https://github.com/example/aizome/compare/old..new"));
        assert!(delta.children[0].children.is_empty());
    }

    #[test]
    fn additions_removals_and_repository_changes_do_not_make_false_ranges() {
        let before = lock("old", "app").input_tree(&["sekka"]).unwrap();
        let mut after = lock("new", "app").input_tree(&["sekka"]).unwrap();
        after.children.get_mut("aizome").unwrap().locked.as_mut().unwrap().repo = Some("different".into());
        let old_library = after.children.remove("library").unwrap();
        after.children.insert("new-library".into(), old_library);
        let delta = delta("sekka", Some(&before), Some(&after)).unwrap();
        assert_eq!(
            delta.children.iter().map(|d| d.status()).collect::<Vec<_>>(),
            ["Source changed", "Removed", "Added"]
        );
        assert!(delta.children.iter().all(|d| d.url().is_none()));
    }

    #[test]
    fn follows_cycles_and_missing_nodes_are_reported() {
        let lock =
            FlakeLock::parse(r#"{"root":"root","nodes":{"root":{"inputs":{"a":["b"],"b":["a"],"missing":"gone"}}}}"#)
                .unwrap();
        assert!(lock.input_tree(&["a"]).is_err());
        assert!(lock.input_tree(&["missing"]).is_err());
        let self_ref = FlakeLock::parse(r#"{"root":"root","nodes":{"root":{"inputs":{"a":"a"}},"a":{"locked":{"type":"github","rev":"a"},"inputs":{"self":"a"}}}}"#).unwrap();
        assert!(self_ref.input_tree(&["a"]).unwrap().children["self"].children.is_empty());
    }

    #[test]
    fn changed_hash_with_the_same_package_version_is_a_rebuild() {
        let info = |hash: &str, name: &str| yukimi_store::db::PathInfo {
            path: format!("/nix/store/{hash}-{name}"),
            nar_size: 10,
            registered: 0,
            deriver: None,
        };
        let graph = yukimi_store::Graph::from_parts(
            vec![
                info("00000000000000000000000000000000", "aizome-0.1.0"),
                info("11111111111111111111111111111111", "aizome-0.1.0"),
                info("22222222222222222222222222222222", "unchanged-1.0"),
            ],
            &[],
        );
        let ids: Vec<_> = graph.ids().collect();
        let store = Store { graph, ..Default::default() };
        let changes = package_changes(&store, &[ids[0], ids[2]], &[ids[1], ids[2]]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "aizome");
        assert_eq!(changes[0].kind, ChangeKind::Changed);
        assert_eq!(changes[0].before, changes[0].after);
    }

    #[test]
    fn missing_details_keep_the_direct_comparison_and_explain_packages() {
        let model = crate::fixture::model();
        let input = model.inputs.iter().find(|i| i.name == "sekka").unwrap();
        let target = yukimi_config::lock::Movement::Newer(Locked { rev: Some("future".into()), ..Default::default() });
        let link = model.update_history.link(input, Some(&target)).unwrap();
        let details = load(&model, &input.name, &link, false);
        assert!(details.source_error.is_some());
        assert_eq!(details.root.url().as_deref(), Some(link.url.as_str()));
        assert!(details.packages.unwrap_err().contains("once both endpoints"));
    }

    #[test]
    fn local_lock_details_must_match_the_selected_endpoint() {
        let dir = std::env::temp_dir().join(format!("yukimi-comparison-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("flake.lock");
        std::fs::write(&file, r#"{"root":"root","nodes":{"root":{"inputs":{"sekka":"s"}},"s":{"locked":{"type":"github","owner":"example","repo":"sekka","rev":"right"}}}}"#).unwrap();
        let mut expected = Locked {
            kind: "github".into(),
            owner: Some("example".into()),
            repo: Some("sekka".into()),
            rev: Some("wrong".into()),
            ..Default::default()
        };
        assert!(saved_tree(&file, "sekka", &expected).is_err());
        expected.rev = Some("right".into());
        assert!(saved_tree(&file, "sekka", &expected).is_ok());
        std::fs::write(&file, "invalid").unwrap();
        assert!(saved_tree(&file, "sekka", &expected).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
