//! Refresh the index for a folder and say what it took:
//! `cargo run -p aizome-index --example index -- PATH [--stat] [--db FILE]`
//! (the shared index unless `--db`).

use std::path::PathBuf;

use aizome_index::{Index, Kind, Refresh};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = PathBuf::from(args.first().ok_or("PATH [--stat] [--db FILE]")?);
    let stat = args.iter().any(|a| a == "--stat");
    let db = args.iter().position(|a| a == "--db").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let mut index = match &db {
        Some(f) => Index::open_at(f)?,
        None => Index::open()?,
    };
    let report = index.refresh(&path, &Refresh { stat, ..Refresh::default() })?;
    let entries = index.entries(&path)?;
    let files = entries.iter().filter(|e| e.kind == Kind::File).count();
    println!(
        "{} folders looked at, {} listed; {} added, {} removed, {} changed; {} files indexed; {:.2} s",
        report.folders,
        report.listed,
        report.added,
        report.removed,
        report.changed,
        files,
        report.took.as_secs_f64()
    );
    Ok(())
}
