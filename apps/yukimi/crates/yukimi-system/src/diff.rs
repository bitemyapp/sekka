// SPDX-License-Identifier: MIT OR Apache-2.0
//! What changed between two versions of a system, package by package.
//!
//! Both closures (everything each version needs) are reduced to package
//! names, their versions, and the store paths of each version's outputs. A
//! name whose versions differ is a change: added, removed, upgraded,
//! downgraded, or changed when several versions are involved. At the same
//! versions, a package one of whose outputs is now a different path was
//! rebuilt (from changed sources, dependencies or build options); one that
//! only gained outputs (its programs, its manual) was added, and one that
//! only lost them removed. Paths without a version (configuration files,
//! sources) are left out, as are the system build itself and the
//! configuration NixOS writes under names that read as versioned.
use std::cmp::Ordering;
use std::collections::BTreeMap;

use yukimi_store::{Graph, PathId, StorePath, compare_versions};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Upgraded,
    Downgraded,
    Added,
    Removed,
    Changed,
    /// Built again at the same versions. Ordered after every change in what
    /// is installed.
    Rebuilt,
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

    /// The changes in what is installed, then the packages rebuilt at the
    /// same versions, as [`diff`] orders them.
    pub fn split(&self) -> (&[Change], &[Change]) {
        self.changes.split_at(self.changes.partition_point(|c| c.kind != ChangeKind::Rebuilt))
    }
}

#[derive(Default)]
struct Side {
    versions: Vec<String>,
    /// The paths of each version's outputs, by full name (`zlib-1.3.1`,
    /// `zlib-1.3.1-dev`).
    builds: BTreeMap<String, Vec<PathId>>,
    size: u64,
}

/// Whether a path belongs to a package: it has a version, and is neither a
/// derivation, the system build, nor configuration NixOS writes under a
/// name Nix reads as versioned (`dbus-1`, `etc-sysctl.d-60-nixos.conf`,
/// `fc-53-no-bitmaps.conf`, `unit-40-wlan0.network`, `unit-getty-.service`),
/// whose paths change with settings rather than packages.
fn is_package(path: &StorePath) -> bool {
    let name = path.full_name();
    path.version().starts_with(|c: char| c.is_ascii_digit())
        && !path.is_derivation()
        && !path.name().starts_with("nixos-system-")
        && name != "dbus-1"
        && !["etc-", "fc-", "unit-"].iter().any(|prefix| name.starts_with(prefix))
}

fn packages(graph: &Graph, closure: &[PathId]) -> BTreeMap<String, Side> {
    let mut map: BTreeMap<String, Side> = BTreeMap::new();
    for &id in closure {
        let Some(path) = graph.store_path(id) else { continue };
        if !is_package(&path) {
            continue;
        }
        let side = map.entry(path.name().to_owned()).or_default();
        side.size += graph.info(id).nar_size;
        side.builds.entry(path.full_name().to_owned()).or_default().push(id);
        let version = path.version().to_owned();
        if !side.versions.contains(&version) {
            side.versions.push(version);
        }
    }
    for side in map.values_mut() {
        side.versions.sort_by(|a, b| compare_versions(a, b));
        side.builds.values_mut().for_each(|ids| ids.sort());
    }
    map
}

/// What became of a package at the same versions in both closures, if
/// anything: rebuilt when an output both have is a different path, added
/// or removed when it only gained or only lost outputs.
fn same_versions(a: &Side, b: &Side) -> Option<ChangeKind> {
    if a.builds == b.builds {
        return None;
    }
    if a.builds.iter().any(|(name, ids)| b.builds.get(name).is_some_and(|other| other != ids)) {
        return Some(ChangeKind::Rebuilt);
    }
    let gained = b.builds.keys().any(|name| !a.builds.contains_key(name));
    let lost = a.builds.keys().any(|name| !b.builds.contains_key(name));
    Some(match (gained, lost) {
        (true, false) => ChangeKind::Added,
        (false, true) => ChangeKind::Removed,
        _ => ChangeKind::Changed,
    })
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
            let kind = match (a.versions.as_slice(), b.versions.as_slice()) {
                (x, y) if x == y => same_versions(a, b)?,
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

    /// The same name built again, under another hash.
    fn rebuilt(name: &str, size: u64) -> PathInfo {
        PathInfo { path: format!("/nix/store/{}-{name}", "a".repeat(32)), ..info(name, size) }
    }

    #[test]
    fn rebuilds_at_the_same_version() {
        let g = Graph::from_parts(
            vec![
                info("kakehashi-0.1.0", 1000),            // 0
                rebuilt("kakehashi-0.1.0", 1132),         // 1
                info("dbus-1", 5),                        // 2
                rebuilt("dbus-1", 5),                     // 3
                info("etc-sysctl.d-60-nixos.conf", 1),    // 4
                rebuilt("etc-sysctl.d-60-nixos.conf", 2), // 5
                info("unit-getty-.service", 1),           // 6
                rebuilt("unit-getty-.service", 1),        // 7
                info("system-path", 10),                  // 8
                rebuilt("system-path", 11),               // 9
                info("zlib-1.3.1", 2),                    // 10
                info("firefox-141.0", 100),               // 11
                info("firefox-142.0", 110),               // 12
            ],
            &[],
        );
        let id = |i: usize| g.ids().nth(i).unwrap();
        // Kakehashi built again from newer source at 0.1.0, with the
        // configuration that changes alongside: one rebuild, nothing more.
        let d = diff(&g, &[0, 2, 4, 6, 8, 10].map(id), &[1, 3, 5, 7, 9, 10].map(id));
        assert_eq!(
            d.changes,
            [Change {
                name: "kakehashi".into(),
                before: vec!["0.1.0".into()],
                after: vec!["0.1.0".into()],
                kind: ChangeKind::Rebuilt,
                size_delta: 132,
            }]
        );
        assert_eq!(d.split(), (&[][..], &d.changes[..]));
        // Rebuilds come after every change in versions.
        let d = diff(&g, &[0, 10, 11].map(id), &[1, 10, 12].map(id));
        let summary: Vec<(&str, ChangeKind)> = d.changes.iter().map(|c| (c.name.as_str(), c.kind)).collect();
        assert_eq!(summary, [("firefox", ChangeKind::Upgraded), ("kakehashi", ChangeKind::Rebuilt)]);
        assert_eq!(d.split(), (&d.changes[..1], &d.changes[1..]));
    }

    #[test]
    fn outputs_gained_or_lost_at_the_same_version() {
        let g = Graph::from_parts(
            vec![
                info("ffmpeg-7.1-lib", 20), // 0
                info("ffmpeg-7.1-bin", 3),  // 1
                info("curl-8.11.1", 4),     // 2
                info("curl-8.11.1-man", 1), // 3
            ],
            &[],
        );
        let id = |i: usize| g.ids().nth(i).unwrap();
        // Installing what was only a dependency adds its other outputs, and
        // removing it again takes them away; neither is a rebuild.
        let d = diff(&g, &[0, 2, 3].map(id), &[0, 1, 2].map(id));
        let summary: Vec<(&str, ChangeKind, i64)> =
            d.changes.iter().map(|c| (c.name.as_str(), c.kind, c.size_delta)).collect();
        assert_eq!(summary, [("ffmpeg", ChangeKind::Added, 3), ("curl", ChangeKind::Removed, -1)]);
        assert_eq!(d.changes[0].before, ["7.1"]);
    }

    #[test]
    fn identical_closures_have_no_changes() {
        let g = Graph::from_parts(
            vec![
                info("kakehashi-0.1.0", 1000), // 0
                info("zlib-1.3.1", 2),         // 1
                info("zlib-1.3.1-dev", 3),     // 2
                info("source", 50),            // 3
                info("dbus-1", 5),             // 4
                rebuilt("dbus-1", 5),          // 5
                info("etc", 7),                // 6
                rebuilt("etc", 8),             // 7
            ],
            &[],
        );
        let id = |i: usize| g.ids().nth(i).unwrap();
        let d = diff(&g, &[0, 1, 2, 3, 4, 6].map(id), &[6, 4, 3, 2, 1, 0].map(id));
        assert_eq!(d, Diff { changes: Vec::new(), size_before: 1067, size_after: 1067 });
        // Only configuration differs: still the same packages.
        let d = diff(&g, &[0, 1, 2, 4, 6].map(id), &[0, 1, 2, 5, 7].map(id));
        assert!(d.changes.is_empty());
        assert_eq!(d.split(), (&[][..], &[][..]));
    }
}
