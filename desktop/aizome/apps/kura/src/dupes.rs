//! Duplicate large files: the same bytes under different inodes.
//!
//! Candidates share an apparent size; among those, a hash of the first and
//! last 64 KiB splits them further, and a hash of the whole file (xxh3,
//! 128-bit) confirms them while a byte budget allows (reading every copy
//! of a large video set would take minutes). Sets confirmed only by the
//! partial hash say so.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use xxhash_rust::xxh3::Xxh3;

/// How the duplicate search is bounded.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Files smaller than this aren't considered.
    pub min_bytes: u64,
    /// Bytes read for whole-file hashes, at most.
    pub full_hash_budget: u64,
    pub threads: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { min_bytes: 1_000_000, full_hash_budget: 4 << 30, threads: 4 }
    }
}

/// A file to compare.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub size: u64,
    pub alloc: u64,
    pub mtime: i64,
}

/// Files with the same contents.
#[derive(Clone, Debug)]
pub struct Set {
    pub size: u64,
    /// xxh3-128, in hex (of the whole file, or of its ends if not verified).
    pub hash: String,
    /// Whether every byte was compared (else only the size and both ends).
    pub verified: bool,
    pub files: Vec<Candidate>,
}

impl Set {
    /// What removing all but one copy frees: every copy's allocation but
    /// the largest.
    pub fn reclaimable(&self) -> u64 {
        let total: u64 = self.files.iter().map(|f| f.alloc).sum();
        total - self.files.iter().map(|f| f.alloc).max().unwrap_or(0)
    }
}

const EDGE: u64 = 64 * 1024;

fn edge_hash(c: &Candidate) -> Option<u128> {
    let mut f = File::open(&c.path).ok()?;
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; EDGE as usize];
    let n = read_full(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    if c.size > EDGE * 2 {
        f.seek(SeekFrom::Start(c.size - EDGE)).ok()?;
        let n = read_full(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    }
    h.update(&c.size.to_le_bytes());
    Some(h.digest128())
}

fn full_hash(c: &Candidate) -> Option<u128> {
    let mut f = File::open(&c.path).ok()?;
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => h.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(h.digest128())
}

fn read_full(f: &mut File, buf: &mut [u8]) -> Option<usize> {
    let mut got = 0;
    while got < buf.len() {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(got)
}

/// Run `f` over `items` on `threads` threads, keeping the results in order.
fn par_map<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicU64::new(0);
    let out: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed) as usize;
                    if i >= items.len() {
                        return;
                    }
                    let r = f(&items[i]);
                    out.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(r);
                }
            });
        }
    });
    out.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().map(|r| r.expect("every item mapped")).collect()
}

/// The sets of duplicates among `files`, largest waste first.
pub fn find(files: Vec<Candidate>, limits: Limits) -> Vec<Set> {
    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for f in files.into_iter().filter(|f| f.size >= limits.min_bytes) {
        by_size.entry(f.size).or_default().push(f);
    }
    let groups: Vec<Vec<Candidate>> = by_size.into_values().filter(|g| g.len() > 1).collect();
    // Ends first.
    let flat: Vec<Candidate> = groups.into_iter().flatten().collect();
    let edges = par_map(&flat, limits.threads, edge_hash);
    let mut by_edge: HashMap<(u64, u128), Vec<Candidate>> = HashMap::new();
    for (c, h) in flat.into_iter().zip(edges) {
        if let Some(h) = h {
            by_edge.entry((c.size, h)).or_default().push(c);
        }
    }
    let mut partial: Vec<(u128, Vec<Candidate>)> =
        by_edge.into_iter().filter(|(_, g)| g.len() > 1).map(|((_, h), g)| (h, g)).collect();
    // Confirm the most wasteful sets first, while the budget lasts.
    partial.sort_by_key(|(_, g)| std::cmp::Reverse(g[0].size * (g.len() as u64 - 1)));
    let mut sets = Vec::new();
    let mut budget = limits.full_hash_budget;
    for (edge, group) in partial {
        let cost = group[0].size * group.len() as u64;
        // Small files' ends are the whole file: already verified.
        if group[0].size <= EDGE * 2 {
            sets.push(Set { size: group[0].size, hash: format!("{edge:032x}"), verified: true, files: group });
            continue;
        }
        if cost > budget {
            sets.push(Set { size: group[0].size, hash: format!("{edge:032x}"), verified: false, files: group });
            continue;
        }
        budget -= cost;
        let hashes = par_map(&group, limits.threads, full_hash);
        let mut by_full: HashMap<u128, Vec<Candidate>> = HashMap::new();
        for (c, h) in group.into_iter().zip(hashes) {
            if let Some(h) = h {
                by_full.entry(h).or_default().push(c);
            }
        }
        for (h, g) in by_full {
            if g.len() > 1 {
                sets.push(Set { size: g[0].size, hash: format!("{h:032x}"), verified: true, files: g });
            }
        }
    }
    for s in &mut sets {
        s.files.sort_by(|a, b| a.mtime.cmp(&b.mtime).then_with(|| a.path.cmp(&b.path)));
    }
    sets.sort_by(|a, b| b.reclaimable().cmp(&a.reclaimable()).then_with(|| a.files[0].path.cmp(&b.files[0].path)));
    sets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(path: PathBuf) -> Candidate {
        let m = std::fs::metadata(&path).unwrap();
        Candidate { size: m.len(), alloc: m.len(), mtime: 0, path }
    }

    #[test]
    fn finds_same_bytes_not_same_sizes() {
        let dir = crate::testdir("dupes");
        let big: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut other = big.clone();
        other[150_000] ^= 1; // same size, same ends, different middle
        std::fs::write(dir.join("a"), &big).unwrap();
        std::fs::write(dir.join("b"), &big).unwrap();
        std::fs::write(dir.join("c"), &other).unwrap();
        std::fs::write(dir.join("small1"), b"tiny").unwrap();
        std::fs::write(dir.join("small2"), b"tiny").unwrap();
        let files = ["a", "b", "c", "small1", "small2"].iter().map(|n| cand(dir.join(n))).collect();
        let sets = find(files, Limits { min_bytes: 100_000, full_hash_budget: 10 << 20, threads: 2 });
        assert_eq!(sets.len(), 1);
        assert!(sets[0].verified);
        let names: Vec<_> = sets[0].files.iter().map(|f| f.path.file_name().unwrap().to_owned()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(sets[0].reclaimable(), 300_000);

        // Without the budget, the ends alone match all three.
        let files = ["a", "b", "c"].iter().map(|n| cand(dir.join(n))).collect();
        let sets = find(files, Limits { min_bytes: 100_000, full_hash_budget: 0, threads: 2 });
        assert_eq!(sets.len(), 1);
        assert!(!sets[0].verified);
        assert_eq!(sets[0].files.len(), 3);
    }
}
