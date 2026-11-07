//! What Chanoma finds in games folders, and what the games' databases say
//! of them, timed: a first look, then a second.
//!
//! ```text
//! cargo run --release -p aizome-chanoma --example library -- FOLDER… [--metadata DIR] [--db FILE]
//! ```
//!
//! Cores are looked for as Chanoma does (`$CHANOMA_CORES`, then the usual
//! places); games of systems without one are counted, not listed.
//! `--db` keeps the files index somewhere other than the shared one.

use std::path::PathBuf;
use std::time::Instant;

use aizome_chanoma::library::{Cores, scan};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut folders = Vec::new();
    let (mut metadata, mut db) = (aizome_chanoma::metadata::dir(), None);
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--metadata") => metadata = args.next().map(PathBuf::from),
            Some("--db") => db = args.next().map(PathBuf::from),
            _ => folders.push(PathBuf::from(a)),
        }
    }
    let mut index = match db {
        Some(p) => aizome_index::Index::open_at(&p),
        None => aizome_index::Index::open(),
    }
    .expect("the files index");
    let cores = Cores::search_path();
    let mut catalogs = aizome_chanoma::metadata::Catalogs::default();
    // A first start (the index read through), then a start as every later
    // one is: what the index knows, then a look for what's changed.
    for (pass, refresh) in [("first look", true), ("from the index", false), ("looked again", true)] {
        if !refresh {
            // A later start reads the databases afresh.
            catalogs = aizome_chanoma::metadata::Catalogs::default();
        }
        let began = Instant::now();
        let lib = scan(&mut index, &folders, &cores, metadata.as_deref(), &mut catalogs, refresh, &|_| {});
        let took = began.elapsed();
        let known = lib.games.iter().filter(|g| g.info.is_some()).count();
        println!(
            "{pass}: {} games ({} need a core not here), {known} known to the databases, in {took:.2?}",
            lib.games.len(),
            lib.unplayable
        );
        let mut by: Vec<(&str, usize, usize)> = Vec::new();
        for g in &lib.games {
            match by.iter_mut().find(|(s, ..)| *s == g.system.name) {
                Some(e) => {
                    e.1 += 1;
                    e.2 += usize::from(g.info.is_some());
                }
                None => by.push((g.system.name, 1, usize::from(g.info.is_some()))),
            }
        }
        for (system, n, known) in by {
            println!("  {system}: {n} games, {known} known");
        }
    }
}
