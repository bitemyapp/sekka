//! The file index behind the palette's Files scope and Hako's Find.
//!
//! A walk of the places a person keeps files (their home folder by
//! default), honouring `.gitignore`, skipping hidden files and the usual
//! build and cache directories, as Omperor's file picker does for a
//! project. Paths are kept relative to their root in one string arena, so a
//! home folder of 200,000 files costs about 15 MB.
//!
//! Matching scores the file's name and its whole path and keeps the better,
//! with a name match worth more; recency and frecency nudge the order. The
//! scan runs on all cores, and a query that extends the previous one only
//! rescans the previous one's matches.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::Query;

/// Directory names never descended into, wherever they are: build outputs,
/// dependency trees and caches that hold thousands of files nobody opens by
/// name.
pub const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    ".git",
    ".hg",
    ".svn",
    ".direnv",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".gradle",
    ".cargo",
    ".rustup",
    ".cache",
    ".nix-defexpr",
    ".nix-profile",
    "dist-newstyle",
    ".stack-work",
    "zig-cache",
    ".zig-cache",
];

/// How a walk is bounded.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Stop after this many entries.
    pub max_entries: usize,
    /// Don't descend deeper than this below a root.
    pub max_depth: usize,
    /// Include folders as results, not just files.
    pub folders: bool,
    /// Folders not walked, nor anything in them (the user's "skip these").
    pub skip: Vec<PathBuf>,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_entries: 300_000, max_depth: 24, folders: true, skip: Vec::new() }
    }
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    /// Which root.
    root: u16,
    /// The path relative to the root, in the arena.
    start: u32,
    len: u32,
    /// Where the file name starts within the relative path.
    name: u32,
    /// Modification time, seconds since the epoch.
    modified: u32,
    folder: bool,
}

/// Room reserved in the arena for each entry a walk may find: the paths in
/// a large home folder average about 115 bytes.
const PATH_BYTES: usize = 128;

/// An entry's path relative to its root.
fn rel_in<'a>(arena: &'a str, e: &Entry) -> &'a str {
    &arena[e.start as usize..(e.start + e.len) as usize]
}

/// How many lines end in `text`. Summed a byte at a time, in chunks no sum
/// can overflow, the count compiles to byte-wide vector adds: a millisecond
/// or two for a 40 MB index, where counting into a `usize` took eight.
fn newlines(text: &str) -> usize {
    text.as_bytes().chunks(255).map(|c| c.iter().map(|&b| u8::from(b == b'\n')).sum::<u8>() as usize).sum()
}

/// An index of files under some roots.
#[derive(Default)]
pub struct FileIndex {
    roots: Vec<PathBuf>,
    arena: String,
    entries: Vec<Entry>,
    /// When the walk finished.
    built: Option<SystemTime>,
    /// The last query and the entries it matched, for narrowing.
    last: Mutex<Option<(String, Vec<u32>)>>,
}

/// A file that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHit {
    pub path: PathBuf,
    pub folder: bool,
    /// Higher is better.
    pub score: i64,
    /// Byte ranges of the file *name* that matched, for highlighting.
    pub name_ranges: Vec<Range<usize>>,
    /// Seconds since the epoch.
    pub modified: u64,
}

impl FileIndex {
    /// Walks `roots` (in parallel) and returns their index.
    pub fn build(roots: &[PathBuf], limits: &Limits) -> FileIndex {
        let started = Instant::now();
        // The walkers append to the arena and the entries, both reserved here,
        // on the calling thread. A `String` per path made on the walkers'
        // threads came from a dozen malloc arenas, a few MB each, and freeing
        // it all left each arena that much bigger for good (below glibc's
        // trim threshold), a little more at every rebuild. Memory reserved
        // and not written costs nothing, and past the reservation it's
        // reallocated where it was allocated, never on a walker's arena.
        let room = limits.max_entries.min(Limits::default().max_entries);
        let found = Mutex::new(FileIndex {
            roots: roots.to_vec(),
            arena: String::with_capacity(room * PATH_BYTES),
            entries: Vec::with_capacity(room),
            ..Default::default()
        });
        for (r, root) in roots.iter().enumerate() {
            if limits.skip.iter().any(|s| root.starts_with(s)) {
                continue;
            }
            let skip = limits.skip.clone();
            let mut builder = ignore::WalkBuilder::new(root);
            builder
                .hidden(true)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                // A home folder is rarely a repository; read .gitignore files
                // wherever they are, not only inside one.
                .require_git(false)
                .parents(false)
                .follow_links(false)
                .max_depth(Some(limits.max_depth))
                .filter_entry(move |e| {
                    !(e.file_type().is_some_and(|t| t.is_dir())
                        && (e.file_name().to_str().is_some_and(|n| SKIPPED_DIRS.contains(&n))
                            || skip.iter().any(|s| e.path() == s)))
                });
            builder.build_parallel().run(|| {
                let found = &found;
                Box::new(move |entry| {
                    let Ok(entry) = entry else { return ignore::WalkState::Continue };
                    if entry.depth() == 0 {
                        return ignore::WalkState::Continue;
                    }
                    let kind = entry.file_type();
                    let folder = kind.is_some_and(|k| k.is_dir());
                    if !folder && !kind.is_some_and(|k| k.is_file()) {
                        return ignore::WalkState::Continue;
                    }
                    if folder && !limits.folders {
                        return ignore::WalkState::Continue;
                    }
                    let Some(rel) = entry.path().strip_prefix(root).ok().and_then(|p| p.to_str()) else {
                        return ignore::WalkState::Continue;
                    };
                    let modified = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs().min(u32::MAX as u64) as u32);
                    let name = rel.rfind('/').map_or(0, |i| i + 1) as u32;
                    let mut found = found.lock().unwrap_or_else(PoisonError::into_inner);
                    let start = found.arena.len() as u32;
                    found.arena.push_str(rel);
                    found.entries.push(Entry { root: r as u16, start, len: rel.len() as u32, name, modified, folder });
                    if found.entries.len() >= limits.max_entries {
                        ignore::WalkState::Quit
                    } else {
                        ignore::WalkState::Continue
                    }
                })
            });
        }
        let mut index = found.into_inner().unwrap_or_else(PoisonError::into_inner);
        // Newest first, so an empty query and ties offer what was just
        // worked on. The arena stays in the order the walkers found things.
        let FileIndex { arena, entries, .. } = &mut index;
        entries
            .sort_unstable_by(|a, b| b.modified.cmp(&a.modified).then_with(|| rel_in(arena, a).cmp(rel_in(arena, b))));
        index.arena.shrink_to_fit();
        index.entries.shrink_to_fit();
        index.built = Some(SystemTime::now());
        log::debug!("indexed {} files in {:?}", index.entries.len(), started.elapsed());
        index
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How long ago the walk finished.
    pub fn age(&self) -> Option<Duration> {
        self.built.and_then(|t| t.elapsed().ok())
    }

    fn rel(&self, e: &Entry) -> &str {
        rel_in(&self.arena, e)
    }

    fn full_path(&self, e: &Entry) -> PathBuf {
        self.roots[e.root as usize].join(self.rel(e))
    }

    /// The `limit` best matches for `query`. `boost` adds to an entry's
    /// score by its absolute path (frecency, being in the Desk's folder).
    pub fn search(&self, query: &str, limit: usize, boost: &(dyn Fn(&Path) -> i64 + Sync)) -> Vec<FileHit> {
        let query = query.trim();
        if query.is_empty() {
            return self
                .entries
                .iter()
                .take(limit)
                .map(|e| FileHit {
                    path: self.full_path(e),
                    folder: e.folder,
                    score: 0,
                    name_ranges: Vec::new(),
                    modified: e.modified as u64,
                })
                .collect();
        }
        // Narrow from the last query's matches when this one extends it:
        // anything this one matches, that one matched too.
        let candidates: Option<Vec<u32>> = {
            let last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
            last.as_ref().filter(|(q, _)| query.starts_with(q.as_str()) && !q.is_empty()).map(|(_, m)| m.clone())
        };
        let ids: Vec<u32> = candidates.unwrap_or_else(|| (0..self.entries.len() as u32).collect());
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
        let chunk = ids.len().div_ceil(threads).max(2048);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let scored: Vec<(u32, i64)> = std::thread::scope(|s| {
            let handles: Vec<_> = ids
                .chunks(chunk)
                .map(|ids| {
                    s.spawn(move || {
                        let mut name_q = Query::new(query);
                        let mut path_q = Query::for_paths(query);
                        let mut out = Vec::new();
                        for &id in ids {
                            let e = &self.entries[id as usize];
                            let rel = self.rel(e);
                            let name = &rel[e.name as usize..];
                            let by_name = name_q.score(name).map(|s| s as i64 * 3 / 2);
                            let by_path = path_q.score(rel).map(|s| s as i64);
                            let Some(score) = by_name.max(by_path) else { continue };
                            // Shallow paths and recent files edge ahead.
                            let depth = rel.matches('/').count() as i64;
                            let age_days = now.saturating_sub(e.modified as u64) / 86400;
                            let recency = match age_days {
                                0 => 12,
                                1..=7 => 8,
                                8..=30 => 4,
                                _ => 0,
                            };
                            out.push((id, score - depth * 2 + recency));
                        }
                        out
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((query.to_owned(), scored.iter().map(|(id, _)| *id).collect()));
        let mut ranked: Vec<(u32, i64)> = scored;
        // Boost only plausible results; frecency lookups cost a hash each.
        ranked.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.truncate(limit.saturating_mul(4).max(limit));
        let mut hits: Vec<FileHit> = ranked
            .into_iter()
            .map(|(id, score)| {
                let e = &self.entries[id as usize];
                let path = self.full_path(e);
                let score = score + boost(&path);
                FileHit { path, folder: e.folder, score, name_ranges: Vec::new(), modified: e.modified as u64 }
            })
            .collect();
        hits.sort_by_key(|h| std::cmp::Reverse(h.score));
        hits.truncate(limit);
        let mut name_q = Query::new(query);
        for hit in &mut hits {
            if let Some(name) = hit.path.file_name().and_then(|n| n.to_str())
                && let Some(m) = name_q.matches(name)
            {
                hit.name_ranges = m.ranges;
            }
        }
        hits
    }

    /// Saves the index to `path` so the next session starts with it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        w.write_all(b"AZFI1\n")?;
        for root in &self.roots {
            writeln!(w, "{}", root.display())?;
        }
        w.write_all(b"\n")?;
        for e in &self.entries {
            writeln!(w, "{}\t{}\t{}\t{}", e.root, e.modified, e.folder as u8, self.rel(e))?;
        }
        w.flush()?;
        drop(w);
        std::fs::rename(tmp, path)
    }

    /// Loads an index saved with [`FileIndex::save`].
    pub fn load(path: &Path) -> Option<FileIndex> {
        let text = std::fs::read_to_string(path).ok()?;
        let built = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mut lines = text.lines();
        if lines.next()? != "AZFI1" {
            return None;
        }
        let mut index = FileIndex { built, ..Default::default() };
        // One allocation for each, made before anything is read into it: the
        // paths are most of the file, and it has a line for each entry (and
        // a few more: its header and roots). Grown a push at a time, the
        // entries were reallocated at every power of two on the thread that
        // reads, and what the smaller ones left stayed in that thread's
        // malloc arena after the index was dropped: a few MB for each of the
        // shell's pool threads that had ever read one. (No line `save`
        // writes for an entry is under eight bytes, so a file that isn't an
        // index can't ask for much more than its own size.)
        index.arena.reserve(text.len());
        index.entries.reserve(newlines(&text).min(text.len() / 8));
        for line in lines.by_ref() {
            if line.is_empty() {
                break;
            }
            index.roots.push(PathBuf::from(line));
        }
        for line in lines {
            let mut parts = line.splitn(4, '\t');
            let root: u16 = parts.next()?.parse().ok()?;
            let modified: u32 = parts.next()?.parse().ok()?;
            let folder = parts.next()? == "1";
            let rel = parts.next()?;
            if root as usize >= index.roots.len() {
                return None;
            }
            let name = rel.rfind('/').map_or(0, |i| i + 1) as u32;
            let start = index.arena.len() as u32;
            index.arena.push_str(rel);
            index.entries.push(Entry { root, start, len: rel.len() as u32, name, modified, folder });
        }
        index.arena.shrink_to_fit();
        index.entries.shrink_to_fit();
        Some(index)
    }

    /// How long ago the index saved at `path` was walked, without reading
    /// it: the age [`FileIndex::load`] would give it.
    pub fn saved_age(path: &Path) -> Option<Duration> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()?.elapsed().ok()
    }

    /// The default cache path: `$XDG_CACHE_HOME/aizome/files.idx`.
    pub fn default_cache_path() -> PathBuf {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("aizome/files.idx")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("aizome-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let write = |rel: &str| {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "").unwrap();
        };
        for rel in [
            "Documents/replay notes.md",
            "work/sidestep/crates/sidestep-engine/src/trace/replay.rs",
            "work/sidestep/target/debug/replay.rs",
            "work/proj/node_modules/x/replay.js",
            "work/proj/build/out.txt",
            ".hidden/replay.txt",
            "Pictures/日本語.png",
            "naïve/résumé.md",
        ] {
            write(rel);
        }
        fs::write(root.join("work/proj/.gitignore"), "build/\n").unwrap();
        root
    }

    #[test]
    fn walks_honouring_ignores_and_skips() {
        let root = fixture("walk");
        let index = FileIndex::build(std::slice::from_ref(&root), &Limits { folders: false, ..Default::default() });
        let mut rels: Vec<&str> = index.entries.iter().map(|e| index.rel(e)).collect();
        rels.sort_unstable();
        assert_eq!(
            rels,
            [
                "Documents/replay notes.md",
                "Pictures/日本語.png",
                "naïve/résumé.md",
                "work/sidestep/crates/sidestep-engine/src/trace/replay.rs",
            ]
        );
        // Folders the user skips aren't walked, nor what's in them.
        let skip = vec![root.join("work"), root.join("Pictures/")];
        let index =
            FileIndex::build(std::slice::from_ref(&root), &Limits { folders: false, skip, ..Default::default() });
        let mut rels: Vec<&str> = index.entries.iter().map(|e| index.rel(e)).collect();
        rels.sort_unstable();
        assert_eq!(rels, ["Documents/replay notes.md", "naïve/résumé.md"]);
        let none =
            FileIndex::build(std::slice::from_ref(&root), &Limits { skip: vec![root.clone()], ..Default::default() });
        assert!(none.entries.is_empty(), "a skipped root isn't walked");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn newest_first_then_by_path() {
        let root = fixture("order");
        let modified = |rel: &str, secs: u64| {
            let f = fs::File::options().write(true).open(root.join(rel)).unwrap();
            f.set_modified(UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
        };
        modified("work/sidestep/crates/sidestep-engine/src/trace/replay.rs", 1_000_000);
        modified("naïve/résumé.md", 2_000_000);
        modified("Pictures/日本語.png", 2_000_000);
        modified("Documents/replay notes.md", 3_000_000);
        let index = FileIndex::build(std::slice::from_ref(&root), &Limits { folders: false, ..Default::default() });
        let rels: Vec<&str> = index.entries.iter().map(|e| index.rel(e)).collect();
        assert_eq!(
            rels,
            [
                "Documents/replay notes.md",
                "Pictures/日本語.png",
                "naïve/résumé.md",
                "work/sidestep/crates/sidestep-engine/src/trace/replay.rs",
            ]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn names_outrank_paths_and_narrowing_agrees() {
        let root = fixture("rank");
        let index = FileIndex::build(std::slice::from_ref(&root), &Limits::default());
        let none = |_: &Path| 0;
        let hits = index.search("repla", 10, &none);
        assert!(hits.len() >= 2, "{hits:?}");
        assert!(hits[0].path.ends_with("replay.rs") || hits[0].path.ends_with("replay notes.md"));
        let narrowed = index.search("replay", 10, &none);
        let fresh = FileIndex::build(std::slice::from_ref(&root), &Limits::default()).search("replay", 10, &none);
        assert_eq!(narrowed, fresh);
        assert!(index.search("한글", 10, &none).is_empty());
        assert_eq!(index.search("resume", 10, &none).len(), 1);
        let hit = &index.search("日本", 10, &none)[0];
        assert_eq!(hit.name_ranges, vec![0..6]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_loads() {
        let root = fixture("save");
        let index = FileIndex::build(std::slice::from_ref(&root), &Limits::default());
        let path = root.join("idx");
        index.save(&path).unwrap();
        let loaded = FileIndex::load(&path).unwrap();
        assert_eq!(loaded.len(), index.len());
        let none = |_: &Path| 0;
        assert_eq!(loaded.search("trace", 5, &none), index.search("trace", 5, &none));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn counts_lines_across_its_chunks() {
        let text: String = (0..2000).map(|i| if i % 7 == 0 || i % 255 == 254 { '\n' } else { 'a' }).collect();
        assert_eq!(newlines(&text), text.matches('\n').count());
        // A chunk of nothing else is the most one sum holds.
        assert_eq!(newlines(&"\n".repeat(1000)), 1000);
        assert_eq!(newlines("no line ends here"), 0);
        assert_eq!(newlines(""), 0);
    }

    #[test]
    fn a_loaded_index_is_the_walked_one_in_blocks_its_own_size() {
        let root = fixture("sized");
        let index = FileIndex::build(std::slice::from_ref(&root), &Limits::default());
        let path = root.join("idx");
        index.save(&path).unwrap();
        let loaded = FileIndex::load(&path).unwrap();
        // The same entries in the same order, wherever each keeps its paths
        // (in the order they were walked, or the order they were saved).
        let entries = |i: &FileIndex| -> Vec<(u16, String, u32, u32, bool)> {
            i.entries.iter().map(|e| (e.root, i.rel(e).to_owned(), e.name, e.modified, e.folder)).collect()
        };
        assert_eq!(entries(&loaded), entries(&index));
        assert_eq!(loaded.roots, index.roots);
        // Each block was sized once and holds no more than was read: none
        // was grown into, which would leave it at a power of two (so the
        // fixture mustn't have that many entries).
        assert!(!loaded.entries.len().is_power_of_two(), "{}", loaded.entries.len());
        assert_eq!(loaded.entries.capacity(), loaded.entries.len());
        assert_eq!(loaded.arena.capacity(), loaded.arena.len());
        // Saved again, it's the same file.
        let again = root.join("idx2");
        loaded.save(&again).unwrap();
        assert_eq!(fs::read(&again).unwrap(), fs::read(&path).unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
