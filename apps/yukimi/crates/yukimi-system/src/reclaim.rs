// SPDX-License-Identifier: MIT OR Apache-2.0
//! What deleting system generations gives back.
//!
//! A generation's closure, everything that version of the system needs, is
//! nearly all shared: with the generations around it, the running and
//! started systems, users' profiles, projects' build results and dev shells,
//! and what programs running now still use. Deleting a generation lets go
//! only what nothing else keeps, and only when garbage is collected.
//!
//! [`Reclaim`] works that out from every root the garbage collector has
//! (as Nix lists them, so programs running as other users count too),
//! following Nix's `keep-derivations` and `keep-outputs` as the collector
//! does. [`Usage`] says what those paths take on disk, hard-linked files
//! counted once. [`estimate`] does all of it for this system.
use std::collections::HashMap;

use yukimi_store::{Graph, Keep, PathId, Root, RootKind, StoreDb, Usage, roots};

/// Which paths deleting system generations lets go.
#[derive(Clone, Debug, Default)]
pub struct Reclaim {
    /// Each system generation's link among the roots: its number, and the
    /// build it points at.
    generations: Vec<(u32, PathId)>,
    /// Kept by a root that isn't a system generation.
    held: Vec<bool>,
    /// For each path no other root holds, how many generation links keep it.
    keepers: Vec<u16>,
    keep: Keep,
}

impl Reclaim {
    /// From the store's graph and all of its roots.
    pub fn new(graph: &Graph, roots: &[Root], keep: Keep) -> Reclaim {
        let mut generations = Vec::new();
        let mut others = Vec::new();
        for root in roots {
            let Some(id) = graph.lookup(root.target.as_str()) else { continue };
            match root.kind {
                RootKind::SystemGeneration(number) => generations.push((number, id)),
                _ => others.push(id),
            }
        }
        generations.sort_unstable();
        generations.dedup();
        let mut held = vec![false; graph.len()];
        for id in graph.kept(others, keep) {
            held[id.index()] = true;
        }
        let mut keepers = vec![0u16; graph.len()];
        for &(_, target) in &generations {
            for id in graph.kept([target], keep) {
                if !held[id.index()] {
                    keepers[id.index()] = keepers[id.index()].saturating_add(1);
                }
            }
        }
        Reclaim { generations, held, keepers, keep }
    }

    /// The system generations among the roots.
    pub fn generations(&self) -> impl Iterator<Item = u32> + '_ {
        self.generations.iter().map(|&(number, _)| number)
    }

    pub fn has(&self, number: u32) -> bool {
        self.generations.iter().any(|&(n, _)| n == number)
    }

    /// What nothing keeps: what collecting garbage removes now.
    pub fn garbage(&self) -> Vec<PathId> {
        self.ids(|i| !self.held[i] && self.keepers[i] == 0)
    }

    /// Everything that deleting generations and collecting garbage could
    /// remove: the garbage, and what only system generations keep.
    pub fn candidates(&self) -> Vec<PathId> {
        self.ids(|i| !self.held[i])
    }

    fn ids(&self, keep: impl Fn(usize) -> bool) -> Vec<PathId> {
        (0..self.held.len()).filter(|&i| keep(i)).map(PathId::from_index).collect()
    }

    /// What only the generations `numbers` keep: what deleting them lets go
    /// (beyond what is garbage already). A generation that two links name
    /// stays while either is left.
    pub fn only_kept_by(&self, graph: &Graph, numbers: &[u32]) -> Vec<PathId> {
        let mut count: HashMap<PathId, u16> = HashMap::new();
        for &(_, target) in self.generations.iter().filter(|(n, _)| numbers.contains(n)) {
            for id in graph.kept([target], self.keep) {
                if !self.held[id.index()] {
                    *count.entry(id).or_default() += 1;
                }
            }
        }
        let mut only: Vec<PathId> =
            count.into_iter().filter(|&(id, n)| n == self.keepers[id.index()]).map(|(id, _)| id).collect();
        only.sort_unstable();
        only
    }
}

/// What deleting some generations gives back, worked out on this system.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Estimate {
    pub generations: Vec<u32>,
    /// Store paths only these generations keep.
    pub paths: usize,
    /// Their size as the store database records it (NAR size).
    pub nar: u64,
    /// The disk space they take that garbage collection gives back once the
    /// generations are deleted: blocks, with hard-linked files counted once
    /// and only when all their links go.
    pub disk: u64,
    /// What collecting garbage gives back anyway, on disk: what nothing
    /// keeps now.
    pub garbage: u64,
    /// Everything the generations need, shared or not (NAR size), which is
    /// what Yukimi showed as a generation's size before.
    pub closure: u64,
    /// The store has hard-linked (optimised) files.
    pub optimised: bool,
    /// Where the roots came from: `nix`, the collector's own list, or
    /// `links`, only the links Yukimi could read.
    pub roots: String,
}

/// The garbage collector's roots: Nix's own list, or else the links Yukimi
/// can read (without what running programs use), and which it is.
pub fn system_roots() -> (Vec<Root>, &'static str) {
    match crate::nix::gc_roots() {
        Some(roots) if !roots.is_empty() => (roots, "nix"),
        _ => (roots::scan(), "links"),
    }
}

/// Work out what deleting generations `numbers` would give back on this
/// system, once garbage is collected.
pub fn estimate(numbers: &[u32]) -> Result<Estimate, String> {
    let graph = StoreDb::open_default().and_then(|db| db.graph()).map_err(|e| e.to_string())?;
    let (roots, source) = system_roots();
    estimate_in(&graph, &roots, crate::nix::keep(), numbers, source, |ids| Usage::measure(&graph, ids))
}

/// [`estimate`] with the graph, roots and measuring given.
pub fn estimate_in(
    graph: &Graph,
    roots: &[Root],
    keep: Keep,
    numbers: &[u32],
    source: &str,
    measure: impl FnOnce(Vec<PathId>) -> Usage,
) -> Result<Estimate, String> {
    let reclaim = Reclaim::new(graph, roots, keep);
    if let Some(missing) = numbers.iter().find(|&&n| !reclaim.has(n)) {
        return Err(format!("There is no generation {missing}"));
    }
    let targets: Vec<PathId> =
        reclaim.generations.iter().filter(|(n, _)| numbers.contains(n)).map(|&(_, id)| id).collect();
    let only = reclaim.only_kept_by(graph, numbers);
    let garbage = reclaim.garbage();
    let usage = measure(garbage.iter().chain(&only).copied().collect());
    let base = usage.tally(garbage.iter().copied());
    let mut generations = numbers.to_vec();
    generations.sort_unstable();
    generations.dedup();
    Ok(Estimate {
        generations,
        paths: only.len(),
        nar: graph.size(&only),
        disk: usage.freed_beyond(&base, only.iter().copied()),
        garbage: usage.bytes(&base),
        closure: graph.size(&graph.closure(targets)),
        optimised: usage.optimised,
        roots: source.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use yukimi_store::StorePath;
    use yukimi_store::db::PathInfo;

    const H: &str = "0123456789abcdfghijklmnpqrsvwxyz";

    fn info(name: &str, size: u64) -> PathInfo {
        PathInfo { path: format!("/nix/store/{H}-{name}"), nar_size: size, registered: 0, deriver: None }
    }

    fn root(link: &str, graph: &Graph, id: usize) -> Root {
        let link = PathBuf::from(link);
        let target = StorePath::parse(&graph.info(PathId::from_index(id)).path).unwrap();
        Root { kind: roots::classify(&link), link, target }
    }

    /// Five generations, 1 to 5, over a shared base, 5 running:
    /// 0 base; 1..=5 systems; 6 only 2 has; 7 2 and 3 share; 8 3 and a
    /// user's profile share; 9 only 3 has, built by 10 (a derivation),
    /// which needs 11; 12 garbage; 13 what a running program of 4's uses.
    fn sample() -> (Graph, Vec<Root>) {
        let mut nodes: Vec<PathInfo> = vec![info("base", 1000)];
        for n in 1..=5 {
            nodes.push(info(&format!("system-{n}"), 1));
        }
        for (name, size) in [("only-2", 20), ("both", 30), ("profile", 40), ("only-3", 50), ("only-3.drv", 2)] {
            nodes.push(info(name, size));
        }
        nodes.push(info("src", 3));
        nodes.push(info("garbage", 70));
        nodes.push(info("busy", 80));
        nodes[9].deriver = Some(nodes[10].path.clone());
        let mut edges: Vec<(usize, usize)> = (1..=5).map(|s| (s, 0)).collect();
        edges.extend([(2, 6), (2, 7), (3, 7), (3, 8), (3, 9), (10, 11), (4, 13)]);
        let graph = Graph::from_parts(nodes, &edges);
        // Each system's own path is unique, so tell them apart.
        let mut roots: Vec<Root> =
            (1..=5).map(|n| root(&format!("/nix/var/nix/profiles/system-{n}-link"), &graph, n)).collect();
        roots.push(root("/run/current-system", &graph, 5));
        roots.push(root("/home/ref/.local/state/nix/profiles/profile-1-link", &graph, 8));
        roots.push(Root { link: "{censored}".into(), kind: RootKind::Running, ..root("/x", &graph, 13) });
        (graph, roots)
    }

    #[test]
    fn what_each_deletion_lets_go() {
        let (graph, roots) = sample();
        // Each system's build is its own path, so the same hash with
        // another name; lookups go by full path.
        let reclaim = Reclaim::new(&graph, &roots, Keep::default());
        assert_eq!(reclaim.generations().collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
        assert_eq!(reclaim.garbage(), [PathId::from_index(12)]);
        let names =
            |ids: Vec<PathId>| ids.into_iter().map(|id| graph.info(id).path[44..].to_owned()).collect::<Vec<_>>();
        // 1 has nothing of its own but its build.
        assert_eq!(names(reclaim.only_kept_by(&graph, &[1])), ["system-1"]);
        // 2 lets go of what only it has, not what it shares with 3.
        assert_eq!(names(reclaim.only_kept_by(&graph, &[2])), ["system-2", "only-2"]);
        // 3: not the profile's path, but its own and its derivation's.
        assert_eq!(names(reclaim.only_kept_by(&graph, &[3])), ["system-3", "only-3", "only-3.drv", "src"]);
        // 2 and 3 together let go of what they share.
        assert!(names(reclaim.only_kept_by(&graph, &[2, 3])).contains(&"both".to_owned()));
        // 4's running program keeps what it uses; 5 is running.
        assert_eq!(names(reclaim.only_kept_by(&graph, &[4])), ["system-4"]);
        assert!(reclaim.only_kept_by(&graph, &[5]).is_empty());
        // Without keep-derivations the derivation is garbage already.
        let plain = Reclaim::new(&graph, &roots, Keep::CLOSURE);
        assert_eq!(names(plain.only_kept_by(&graph, &[3])), ["system-3", "only-3"]);
        assert!(plain.garbage().contains(&PathId::from_index(10)));
    }

    #[test]
    fn two_links_to_one_build_keep_it_until_both_go() {
        let (graph, mut roots) = sample();
        roots.push(root("/nix/var/nix/profiles/system-6-link", &graph, 2));
        let reclaim = Reclaim::new(&graph, &roots, Keep::default());
        assert!(reclaim.only_kept_by(&graph, &[2]).is_empty());
        assert!(reclaim.only_kept_by(&graph, &[6]).is_empty());
        assert_eq!(reclaim.only_kept_by(&graph, &[2, 6]).len(), 2);
    }

    #[test]
    fn estimates_say_what_comes_back_and_what_was_shown_before() {
        let (graph, roots) = sample();
        // Stand-in measurement: each path a file as big as its NAR.
        let measure = |ids: Vec<PathId>| {
            let dir = std::env::temp_dir().join(format!("yukimi-reclaim-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let paths: Vec<(PathId, PathBuf)> = ids
                .iter()
                .map(|&id| {
                    let path = dir.join(id.index().to_string());
                    std::fs::write(&path, vec![1u8; 100 * graph.info(id).nar_size as usize]).unwrap();
                    (id, path)
                })
                .collect();
            let usage = Usage::measure_paths(paths, false);
            let _ = std::fs::remove_dir_all(&dir);
            usage
        };
        let estimate = estimate_in(&graph, &roots, Keep::default(), &[3], "nix", measure).unwrap();
        assert_eq!(estimate.generations, [3]);
        assert_eq!(estimate.paths, 4);
        assert_eq!(estimate.nar, 1 + 50 + 2 + 3);
        // Before, Yukimi showed the whole closure: base and all.
        assert_eq!(estimate.closure, 1 + 1000 + 30 + 40 + 50);
        assert!(estimate.disk > 0 && estimate.garbage > 0, "{estimate:?}");
        assert!(estimate_in(&graph, &roots, Keep::default(), &[9], "nix", measure).is_err());
    }
}
