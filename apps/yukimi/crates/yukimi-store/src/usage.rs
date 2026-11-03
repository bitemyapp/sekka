// SPDX-License-Identifier: MIT OR Apache-2.0
//! What store paths take on disk, and what removing some of them gives back.
//!
//! The store database records each path's NAR size: its contents as Nix
//! serialises them, not the space they take. On disk every file and
//! directory takes whole blocks (4 KiB on ext4), and a symbolic link longer
//! than 60 bytes, as any link into the store is, takes a block of its own:
//! a NixOS generation's `system-path` and `etc`, thousands of links, take
//! several times their NAR size. With `auto-optimise-store` (or
//! `nix-store --optimise`) identical files are one file, hard-linked from
//! each path that has it and from `/nix/store/.links`; its blocks come back
//! only when every path linking it goes, and garbage collection then removes
//! the `.links` entry too.
//!
//! [`Usage`] measures paths as the filesystem reports them (`st_blocks`).
//! A file with more than one link is kept once, by inode, with the number
//! of its links each path holds, so that removing a set of paths counts a
//! shared file only when the set holds all of its links but the store's own.
//!
//! What it can't see: compression and sharing below the filesystem (Btrfs
//! reports blocks before compression, and blocks a snapshot or reflink
//! shares stay), and anything in a path it isn't allowed to read.
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::STORE_DIR;
use crate::db::{Graph, PathId};

/// The disk use of some store paths.
#[derive(Clone, Debug, Default)]
pub struct Usage {
    paths: HashMap<PathId, Measured>,
    files: Vec<Linked>,
    /// The store keeps a link to each optimised file in `/nix/store/.links`,
    /// which garbage collection removes once nothing else links the file.
    pub optimised: bool,
}

/// One path, measured.
#[derive(Clone, Debug, Default)]
struct Measured {
    /// Blocks only this path holds: its directories, symbolic links and
    /// files with a single link.
    own: u64,
    /// Files with more than one link: each file's index in
    /// [`Usage::files`], and how many of its links are in this path.
    shared: Vec<(u32, u32)>,
}

/// A file with more than one link.
#[derive(Clone, Copy, Debug)]
struct Linked {
    bytes: u64,
    links: u64,
}

/// Some paths of a [`Usage`], added up: what removing them all gives back.
#[derive(Clone, Debug, Default)]
pub struct Tally {
    own: u64,
    /// For each shared file, how many of its links these paths hold.
    links: HashMap<u32, u32>,
}

impl Usage {
    /// Measure these paths in the store.
    pub fn measure(graph: &Graph, ids: impl IntoIterator<Item = PathId>) -> Usage {
        let optimised = links_in_use(&Path::new(STORE_DIR).join(".links"));
        Usage::measure_paths(ids.into_iter().map(|id| (id, PathBuf::from(&graph.info(id).path))), optimised)
    }

    /// Measure paths wherever they are: each id, and where it is.
    pub fn measure_paths(paths: impl IntoIterator<Item = (PathId, PathBuf)>, optimised: bool) -> Usage {
        let mut usage = Usage { optimised, ..Usage::default() };
        let mut inodes: HashMap<(u64, u64), u32> = HashMap::new();
        for (id, path) in paths {
            let mut measured = Measured::default();
            let mut here: HashMap<u32, u32> = HashMap::new();
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                usage.visit(&path, &meta, &mut measured, &mut here, &mut inodes);
            }
            measured.shared = here.into_iter().collect();
            measured.shared.sort_unstable();
            usage.paths.insert(id, measured);
        }
        usage
    }

    fn visit(
        &mut self,
        path: &Path,
        meta: &std::fs::Metadata,
        measured: &mut Measured,
        here: &mut HashMap<u32, u32>,
        inodes: &mut HashMap<(u64, u64), u32>,
    ) {
        let bytes = meta.blocks() * 512;
        if meta.is_file() && meta.nlink() > 1 {
            let index = *inodes.entry((meta.dev(), meta.ino())).or_insert_with(|| {
                self.files.push(Linked { bytes, links: meta.nlink() });
                (self.files.len() - 1) as u32
            });
            *here.entry(index).or_default() += 1;
            return;
        }
        measured.own += bytes;
        if meta.is_dir() {
            let Ok(entries) = std::fs::read_dir(path) else { return };
            for entry in entries.flatten() {
                // A directory entry's metadata doesn't follow symbolic links.
                if let Ok(meta) = entry.metadata() {
                    self.visit(&entry.path(), &meta, measured, here, inodes);
                }
            }
        }
    }

    /// Whether `id` was measured.
    pub fn has(&self, id: PathId) -> bool {
        self.paths.contains_key(&id)
    }

    /// Add up some measured paths.
    pub fn tally(&self, ids: impl IntoIterator<Item = PathId>) -> Tally {
        let mut tally = Tally::default();
        for id in ids {
            let Some(measured) = self.paths.get(&id) else { continue };
            tally.own += measured.own;
            for &(file, links) in &measured.shared {
                *tally.links.entry(file).or_default() += links;
            }
        }
        tally
    }

    /// Whether a shared file's blocks come back when `links` of its links go.
    fn gone(&self, file: u32, links: u32) -> bool {
        let total = self.files[file as usize].links;
        let left = total.saturating_sub(u64::from(links));
        left == 0 || (self.optimised && left == 1)
    }

    /// The disk space removing these paths gives back.
    pub fn freed(&self, ids: impl IntoIterator<Item = PathId>) -> u64 {
        self.bytes(&self.tally(ids))
    }

    /// The disk space removing a tally's paths gives back.
    pub fn bytes(&self, tally: &Tally) -> u64 {
        let shared: u64 = tally
            .links
            .iter()
            .filter(|&(&file, &links)| self.gone(file, links))
            .map(|(&file, _)| self.files[file as usize].bytes)
            .sum();
        tally.own + shared
    }

    /// What removing `more` gives back beyond what removing `base`'s paths
    /// does, when both go: a file `more` shares with them counts once all
    /// its links are gone.
    pub fn freed_beyond(&self, base: &Tally, more: impl IntoIterator<Item = PathId>) -> u64 {
        let more = self.tally(more);
        let mut bytes = more.own;
        for (&file, &links) in &more.links {
            let before = base.links.get(&file).copied().unwrap_or(0);
            if !self.gone(file, before) && self.gone(file, before + links) {
                bytes += self.files[file as usize].bytes;
            }
        }
        bytes
    }
}

/// Whether the store has optimised files: `.links` has anything in it.
pub fn links_in_use(links: &Path) -> bool {
    std::fs::read_dir(links).is_ok_and(|mut entries| entries.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: usize) -> PathId {
        PathId::from_index(n)
    }

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn blocks(path: &Path) -> u64 {
        std::fs::symlink_metadata(path).unwrap().blocks() * 512
    }

    #[test]
    fn blocks_and_hard_links() {
        let dir = Dir(std::env::temp_dir().join(format!("yukimi-usage-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&dir.0);
        let (a, b, c, links) = (dir.0.join("a"), dir.0.join("b"), dir.0.join("c"), dir.0.join(".links"));
        for d in [&a, &b, &c, &links] {
            std::fs::create_dir_all(d).unwrap();
        }
        // a: a small file (a block for 10 bytes), a long link, and a file
        // optimised against b's copy and .links.
        std::fs::write(a.join("small"), "0123456789").unwrap();
        std::os::unix::fs::symlink(format!("{STORE_DIR}/{}-something-with-a-long-name", "x".repeat(32)), a.join("ln"))
            .unwrap();
        std::fs::write(links.join("shared"), vec![7u8; 100_000]).unwrap();
        std::fs::hard_link(links.join("shared"), a.join("shared")).unwrap();
        std::fs::hard_link(links.join("shared"), b.join("shared")).unwrap();
        // c: a file linked twice inside it, and once from .links.
        std::fs::write(links.join("twice"), vec![1u8; 50_000]).unwrap();
        std::fs::hard_link(links.join("twice"), c.join("one")).unwrap();
        std::fs::hard_link(links.join("twice"), c.join("two")).unwrap();
        let ids = [(id(0), a.clone()), (id(1), b.clone()), (id(2), c.clone())];
        let usage = Usage::measure_paths(ids.clone(), true);
        let own_a = blocks(&a) + blocks(&a.join("small")) + blocks(&a.join("ln"));
        let shared = blocks(&links.join("shared"));
        assert!(blocks(&a.join("small")) >= 10, "a block, not ten bytes");
        // Removing a alone leaves b's link: only what's a's own.
        assert_eq!(usage.freed([id(0)]), own_a);
        // Removing a and b frees the shared file: .links goes with them.
        assert_eq!(usage.freed([id(0), id(1)]), own_a + blocks(&b) + shared);
        // c holds both of its file's links but the store's own.
        assert_eq!(usage.freed([id(2)]), blocks(&c) + blocks(&links.join("twice")));
        // b on top of a, which goes anyway, frees the shared file too.
        let base = usage.tally([id(0)]);
        assert_eq!(usage.freed_beyond(&base, [id(1)]), blocks(&b) + shared);
        assert_eq!(usage.freed_beyond(&usage.tally([]), [id(1)]), blocks(&b));
        // Without .links (a store never optimised), a file's every link
        // must go: the shared file stays even with a and b gone.
        let plain = Usage::measure_paths(ids, false);
        assert_eq!(plain.freed([id(0), id(1)]), own_a + blocks(&b));
        assert!(links_in_use(&links) && !links_in_use(&a.join("missing")));
        assert!(usage.has(id(2)) && !usage.has(id(3)));
    }
}
