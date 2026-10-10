// SPDX-License-Identifier: MIT OR Apache-2.0
//! The store database: Nix's record of which store paths exist, how large
//! they are, when they arrived and what each one refers to.
//!
//! [`StoreDb::graph`] reads all of it at once into a [`Graph`]: a few tens of
//! thousands of paths, small enough to keep in memory, which makes closures
//! ("everything Firefox needs") and reverse dependencies ("what keeps this
//! here") instant.
use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::Result;
use crate::path::StorePath;

/// Where Nix keeps the database.
pub const DEFAULT_DB: &str = "/nix/var/nix/db/db.sqlite";

/// An open, read-only store database.
pub struct StoreDb {
    conn: Connection,
}

/// What the database records about one store path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathInfo {
    pub path: String,
    /// Size of the path's contents, in bytes.
    pub nar_size: u64,
    /// When the path arrived in the store, as Unix time.
    pub registered: i64,
    /// The derivation that built it, when known.
    pub deriver: Option<String>,
}

impl StoreDb {
    /// The system's store database.
    pub fn open_default() -> Result<StoreDb> {
        StoreDb::open(DEFAULT_DB)
    }

    /// Open a database read-only. The Nix daemon keeps it in write-ahead-log
    /// mode, which readers normally join through the shared-memory file; when
    /// that file cannot be opened (it belongs to root), read the database as
    /// it stands on disk instead.
    pub fn open(path: impl AsRef<Path>) -> Result<StoreDb> {
        let path = path.as_ref();
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
        let shared = Connection::open_with_flags(path, flags).and_then(|conn| {
            conn.query_row("SELECT count(*) FROM ValidPaths", [], |_| Ok(()))?;
            Ok(conn)
        });
        let conn = match shared {
            Ok(conn) => conn,
            Err(_) => {
                let uri = format!("file:{}?immutable=1", path.display());
                Connection::open_with_flags(uri, flags)?
            }
        };
        Ok(StoreDb { conn })
    }

    /// What the database knows about one path.
    pub fn info(&self, path: &str) -> Result<Option<PathInfo>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT path, narSize, registrationTime, deriver FROM ValidPaths WHERE path = ?1")?;
        let mut rows = statement.query([path])?;
        Ok(match rows.next()? {
            Some(row) => Some(PathInfo {
                path: row.get(0)?,
                nar_size: row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
                registered: row.get(2)?,
                deriver: row.get(3)?,
            }),
            None => None,
        })
    }

    /// Every valid path and every reference between them.
    pub fn graph(&self) -> Result<Graph> {
        let mut index = HashMap::new();
        let mut by_row = HashMap::new();
        let mut nodes = Vec::new();
        let mut statement = self.conn.prepare("SELECT id, path, narSize, registrationTime, deriver FROM ValidPaths")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id = PathId(nodes.len() as u32);
            let path: String = row.get(1)?;
            by_row.insert(row.get::<_, i64>(0)?, id);
            index.insert(path.clone(), id);
            nodes.push(PathInfo {
                path,
                nar_size: row.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
                registered: row.get(3)?,
                deriver: row.get(4)?,
            });
        }
        let mut references = vec![Vec::new(); nodes.len()];
        let mut referrers = vec![Vec::new(); nodes.len()];
        let mut statement = self.conn.prepare("SELECT referrer, reference FROM Refs")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let (Some(&from), Some(&to)) = (by_row.get(&row.get::<_, i64>(0)?), by_row.get(&row.get::<_, i64>(1)?))
            else {
                continue;
            };
            // Most paths refer to themselves; that says nothing about what
            // keeps them alive.
            if from != to {
                references[from.index()].push(to);
                referrers[to.index()].push(from);
            }
        }
        Ok(Graph { nodes, index, references, referrers })
    }
}

/// A store path's position in a [`Graph`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathId(u32);

impl PathId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// All valid store paths and the references between them.
#[derive(Default)]
pub struct Graph {
    nodes: Vec<PathInfo>,
    index: HashMap<String, PathId>,
    references: Vec<Vec<PathId>>,
    referrers: Vec<Vec<PathId>>,
}

impl Graph {
    /// A graph from explicit paths and references, for tests and tools.
    pub fn from_parts(paths: Vec<PathInfo>, edges: &[(usize, usize)]) -> Graph {
        let index = paths.iter().enumerate().map(|(i, info)| (info.path.clone(), PathId(i as u32))).collect();
        let mut references = vec![Vec::new(); paths.len()];
        let mut referrers = vec![Vec::new(); paths.len()];
        for &(from, to) in edges {
            if from != to {
                references[from].push(PathId(to as u32));
                referrers[to].push(PathId(from as u32));
            }
        }
        Graph { nodes: paths, index, references, referrers }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The path for a store path or anything inside it.
    pub fn lookup(&self, path: &str) -> Option<PathId> {
        self.index.get(path).or_else(|| self.index.get(StorePath::parse(path)?.as_str())).copied()
    }

    pub fn info(&self, id: PathId) -> &PathInfo {
        &self.nodes[id.index()]
    }

    pub fn store_path(&self, id: PathId) -> Option<StorePath> {
        StorePath::parse(&self.info(id).path)
    }

    /// What a path refers to directly.
    pub fn references(&self, id: PathId) -> &[PathId] {
        &self.references[id.index()]
    }

    /// What refers to a path directly.
    pub fn referrers(&self, id: PathId) -> &[PathId] {
        &self.referrers[id.index()]
    }

    pub fn ids(&self) -> impl Iterator<Item = PathId> + '_ {
        (0..self.nodes.len() as u32).map(PathId)
    }

    /// Everything the roots need, the roots included.
    pub fn closure(&self, roots: impl IntoIterator<Item = PathId>) -> Vec<PathId> {
        self.kept(roots, false)
    }

    /// Everything the roots keep from garbage collection: their closure and,
    /// with Nix's `keep-derivations` setting (on by default), the derivation
    /// that built each kept path, with everything that derivation needs.
    pub fn kept(&self, roots: impl IntoIterator<Item = PathId>, keep_derivations: bool) -> Vec<PathId> {
        let mut seen = vec![false; self.nodes.len()];
        let mut stack: Vec<PathId> = Vec::new();
        let mut out = Vec::new();
        for root in roots {
            if !std::mem::replace(&mut seen[root.index()], true) {
                stack.push(root);
            }
        }
        while let Some(id) = stack.pop() {
            out.push(id);
            for &next in self.references(id) {
                if !std::mem::replace(&mut seen[next.index()], true) {
                    stack.push(next);
                }
            }
            if keep_derivations
                && let Some(deriver) = self.info(id).deriver.as_deref().and_then(|d| self.index.get(d))
                && !std::mem::replace(&mut seen[deriver.index()], true)
            {
                stack.push(*deriver);
            }
        }
        out
    }

    /// Total size of some paths.
    pub fn size(&self, ids: &[PathId]) -> u64 {
        ids.iter().map(|&id| self.info(id).nar_size).sum()
    }

    /// Total size of the whole store.
    pub fn total_size(&self) -> u64 {
        self.nodes.iter().map(|info| info.nar_size).sum()
    }

    /// One chain of references from a root to `target`, root first: the
    /// answer to "why is this here?". Breadth-first, so it is a shortest one.
    pub fn why(&self, roots: &[PathId], target: PathId) -> Option<Vec<PathId>> {
        let mut parent: Vec<Option<PathId>> = vec![None; self.nodes.len()];
        let mut seen = vec![false; self.nodes.len()];
        let mut queue = std::collections::VecDeque::new();
        for &root in roots {
            if !std::mem::replace(&mut seen[root.index()], true) {
                queue.push_back(root);
            }
        }
        while let Some(id) = queue.pop_front() {
            if id == target {
                let mut chain = vec![id];
                let mut current = id;
                while let Some(up) = parent[current.index()] {
                    chain.push(up);
                    current = up;
                }
                chain.reverse();
                return Some(chain);
            }
            for &next in self.references(id) {
                if !std::mem::replace(&mut seen[next.index()], true) {
                    parent[next.index()] = Some(id);
                    queue.push_back(next);
                }
            }
        }
        None
    }

    /// Paths no root keeps: what garbage collection would remove (apart
    /// from paths that running programs hold open, which only root can see).
    pub fn dead(&self, roots: impl IntoIterator<Item = PathId>, keep_derivations: bool) -> Vec<PathId> {
        let mut live = vec![false; self.nodes.len()];
        for id in self.kept(roots, keep_derivations) {
            live[id.index()] = true;
        }
        self.ids().filter(|id| !live[id.index()]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, size: u64) -> PathInfo {
        PathInfo {
            path: format!("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-{name}"),
            nar_size: size,
            registered: 0,
            deriver: None,
        }
    }

    fn sample() -> Graph {
        // 0 system -> 1 firefox -> 2 glibc; 0 -> 3 bash -> 2; 4 old-firefox -> 2; 5 lonely
        Graph::from_parts(
            vec![
                info("nixos-system", 1),
                info("firefox-142.0", 100),
                info("glibc-2.40", 30),
                info("bash-5.3", 2),
                info("firefox-141.0", 99),
                info("lonely", 7),
            ],
            &[(0, 1), (1, 2), (0, 3), (3, 2), (4, 2), (1, 1)],
        )
    }

    #[test]
    fn closures_reverse_edges_and_dead_paths() {
        let g = sample();
        let mut closure = g.closure([PathId(0)]);
        closure.sort();
        assert_eq!(closure, vec![PathId(0), PathId(1), PathId(2), PathId(3)]);
        assert_eq!(g.size(&closure), 133);
        assert_eq!(g.referrers(PathId(2)).len(), 3);
        assert!(g.references(PathId(1)).iter().all(|&id| id != PathId(1)));
        let dead = g.dead([PathId(0)], false);
        assert_eq!(dead, vec![PathId(4), PathId(5)]);
        assert_eq!(g.total_size(), 239);
    }

    #[test]
    fn derivations_kept_with_their_outputs() {
        // 0 system -> 1 hello; 1 was built by 2 hello.drv, which needs 3 the source.
        let mut nodes =
            vec![info("nixos-system", 1), info("hello-2.12", 10), info("hello-2.12.drv", 1), info("src", 5)];
        nodes[1].deriver = Some(nodes[2].path.clone());
        let g = Graph::from_parts(nodes, &[(0, 1), (2, 3)]);
        assert_eq!(g.dead([PathId(0)], false), vec![PathId(2), PathId(3)]);
        assert!(g.dead([PathId(0)], true).is_empty());
        assert_eq!(g.closure([PathId(0)]).len(), 2);
    }

    #[test]
    fn why_finds_a_shortest_chain() {
        let g = sample();
        let chain = g.why(&[PathId(0)], PathId(2)).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0], PathId(0));
        assert_eq!(chain[2], PathId(2));
        assert!(g.why(&[PathId(0)], PathId(5)).is_none());
        let inside = format!("{}/bin/firefox", g.info(PathId(1)).path);
        assert_eq!(g.lookup(&inside), Some(PathId(1)));
    }
}
