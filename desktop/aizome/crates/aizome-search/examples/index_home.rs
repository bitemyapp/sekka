//! Indexes a folder (your home by default) and times a few queries:
//! `cargo run --release -p aizome-search --example index_home [dir] [query…]`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use aizome_search::files::{FileIndex, Limits};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()));
    let mut queries: Vec<String> = args.collect();
    if queries.is_empty() {
        queries =
            ["c", "ca", "car", "carg", "cargo", "cargo.toml", "readme", "日本", "arch md"].map(String::from).to_vec();
    }
    let t = Instant::now();
    let index = FileIndex::build(&[root], &Limits::default());
    println!("indexed {} entries in {:?}", index.len(), t.elapsed());
    let none = |_: &Path| 0;
    for q in &queries {
        let t = Instant::now();
        let hits = index.search(q, 8, &none);
        println!(
            "{q:>12}: {:>8.2?}  {}",
            t.elapsed(),
            hits.first().map_or(String::new(), |h| h.path.display().to_string())
        );
    }
}
