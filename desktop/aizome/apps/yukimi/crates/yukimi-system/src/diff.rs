// SPDX-License-Identifier: MIT OR Apache-2.0
//! What changed between two versions of a system, package by package.
//!
//! Both closures (everything each version needs) are reduced to package
//! names and their versions; a name whose versions differ is a change:
//! added, removed, upgraded, downgraded, or changed when several versions
//! are involved. Paths without a version (configuration files, sources) are
//! left out, as is the system build itself.
use std::cmp::Ordering;
use std::collections::BTreeMap;

use yukimi_store::{Graph, PathId, compare_versions};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Upgraded,
    Downgraded,
    Added,
    Removed,
    Changed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub kind: ChangeKind,
    /// Bytes this package's paths grew (or shrank, when negative).
    pub size_delta: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub changes: Vec<Change>,
    pub size_before: u64,
    pub size_after: u64,
}

impl Diff {
    pub fn count(&self, kind: ChangeKind) -> usize {
        self.changes.iter().filter(|c| c.kind == kind).count()
    }
}

#[derive(Default)]
struct Side {
    versions: Vec<String>,
    size: u64,
}

fn packages(graph: &Graph, closure: &[PathId]) -> BTreeMap<String, Side> {
    let mut map: BTreeMap<String, Side> = BTreeMap::new();
    for &id in closure {
        let Some(path) = graph.store_path(id) else { continue };
        if path.is_derivation() || path.version().is_empty() || path.name().starts_with("nixos-system-") {
            continue;
        }
        let side = map.entry(path.name().to_owned()).or_default();
        side.size += graph.info(id).nar_size;
        let version = path.version().to_owned();
        if !side.versions.contains(&version) {
            side.versions.push(version);
        }
    }
    for side in map.values_mut() {
        side.versions.sort_by(|a, b| compare_versions(a, b));
    }
    map
}

/// The changes from one closure to another.
pub fn diff(graph: &Graph, before: &[PathId], after: &[PathId]) -> Diff {
    let (old, new) = (packages(graph, before), packages(graph, after));
    let mut names: Vec<&String> = old.keys().chain(new.keys()).collect();
    names.sort();
    names.dedup();
    let empty = Side::default();
    let mut changes: Vec<Change> = names
        .into_iter()
        .filter_map(|name| {
            let (a, b) = (old.get(name).unwrap_or(&empty), new.get(name).unwrap_or(&empty));
            if a.versions == b.versions {
                return None;
            }
            let kind = match (a.versions.as_slice(), b.versions.as_slice()) {
                ([], _) => ChangeKind::Added,
                (_, []) => ChangeKind::Removed,
                ([x], [y]) => match compare_versions(x, y) {
                    Ordering::Less => ChangeKind::Upgraded,
                    Ordering::Greater => ChangeKind::Downgraded,
                    Ordering::Equal => ChangeKind::Changed,
                },
                _ => ChangeKind::Changed,
            };
            Some(Change {
                name: name.clone(),
                before: a.versions.clone(),
                after: b.versions.clone(),
                kind,
                size_delta: b.size as i64 - a.size as i64,
            })
        })
        .collect();
    changes.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    Diff { changes, size_before: graph.size(before), size_after: graph.size(after) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yukimi_store::db::PathInfo;

    fn info(name: &str, size: u64) -> PathInfo {
        PathInfo {
            path: format!("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-{name}"),
            nar_size: size,
            registered: 0,
            deriver: None,
        }
    }

    #[test]
    fn package_changes_between_closures() {
        let g = Graph::from_parts(
            vec![
                info("firefox-141.0", 100),           // 0
                info("firefox-142.0", 110),           // 1
                info("htop-3.4.1", 1),                // 2
                info("zlib-1.3.1", 2),                // 3
                info("zlib-1.3.1-dev", 3),            // 4
                info("kernel-6.12.10", 9),            // 5
                info("kernel-6.12.9", 9),             // 6
                info("source", 50),                   // 7
                info("nixos-system-acer-26.11.1", 1), // 8
                info("nixos-system-acer-26.11.2", 1), // 9
                info("old-tool-1.0", 4),              // 10
            ],
            &[],
        );
        let id = |i: usize| g.ids().nth(i).unwrap();
        let before = [0, 3, 4, 5, 7, 8, 10].map(id);
        let after = [1, 2, 3, 4, 6, 7, 9].map(id);
        let d = diff(&g, &before, &after);
        let summary: Vec<(String, ChangeKind)> = d.changes.iter().map(|c| (c.name.clone(), c.kind)).collect();
        assert_eq!(
            summary,
            vec![
                ("firefox".into(), ChangeKind::Upgraded),
                ("kernel".into(), ChangeKind::Downgraded),
                ("htop".into(), ChangeKind::Added),
                ("old-tool".into(), ChangeKind::Removed),
            ]
        );
        let firefox = &d.changes[0];
        assert_eq!((firefox.before.clone(), firefox.after.clone()), (vec!["141.0".into()], vec!["142.0".into()]));
        assert_eq!(firefox.size_delta, 10);
        assert_eq!(d.count(ChangeKind::Added), 1);
    }
}
