//! The command line, which needs no display:
//!
//! ```text
//! aizome-kura scan [PATH] [--json] [--output DIR] [--cross-filesystems]
//!                  [--depth N] [--no-duplicates] [--no-nix] [--threads N] [--quiet]
//! aizome-kura report [PATH] [--output DIR]
//! aizome-kura hints [PATH] [--json] [--output DIR]
//! aizome-kura trash [--json] PATH…
//! ```

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::format::{self, size};
use crate::index::{self, Index, TreeLimits};
use crate::scan::{self, Progress, ScanOptions};

pub const USAGE: &str = "\
Kura (Disk Usage): what fills a disk, for people and for agents.

Usage:
  aizome-kura [PATH]            Open the window (scanning PATH if given)
  aizome-kura scan [PATH]       Scan PATH (default: your home folder) and write
                                index.json and index.md for agents
  aizome-kura report [PATH]     Print the last scan's report (index.md);
                                scans first if there's none, or if PATH is given
  aizome-kura hints [PATH]      Print what could be freed, and how
  aizome-kura trash PATH...     Move PATHs to the Trash (freedesktop spec);
                                refuses the Nix store and other filesystems

Options:
  --json                 Print JSON (scan: the whole index; hints: the hints)
  --output DIR           Where index.json and index.md live
                         (default: $XDG_CACHE_HOME/aizome/kura)
  --cross-filesystems    Enter other filesystems mounted inside PATH
  --depth N              Levels of the tree the index keeps (default 4)
  --no-duplicates        Don't look for duplicate files (they're read to compare)
  --no-nix               Don't measure the Nix store
  --threads N            Scanner threads (default: one per CPU, 2 to 16)
  --quiet                No progress on stderr
  -h, --help             This help
  -V, --version          Kura's version

The index's format is documented in Aizome's docs/kura-index.md.
";

/// What the command line asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    pub command: String,
    pub paths: Vec<PathBuf>,
    pub json: bool,
    pub output: Option<PathBuf>,
    pub cross: bool,
    pub depth: Option<u32>,
    pub duplicates: bool,
    pub nix: bool,
    pub threads: usize,
    pub quiet: bool,
}

/// The subcommands; anything else opens the window.
pub const COMMANDS: [&str; 4] = ["scan", "report", "hints", "trash"];

/// Whether these arguments are for the command line (else the window).
pub fn is_cli(args: &[String]) -> bool {
    args.first()
        .is_some_and(|a| COMMANDS.contains(&a.as_str()) || matches!(a.as_str(), "-h" | "--help" | "-V" | "--version"))
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args { duplicates: true, nix: true, ..Args::default() };
    let mut it = args.iter();
    a.command = it.next().cloned().unwrap_or_default();
    let mut only_paths = false;
    while let Some(arg) = it.next() {
        if only_paths || !arg.starts_with('-') || arg == "-" {
            a.paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => only_paths = true,
            "--json" => a.json = true,
            "--output" | "-o" => a.output = Some(PathBuf::from(it.next().ok_or("--output needs a folder")?)),
            "--cross-filesystems" | "-x" => a.cross = true,
            "--depth" => {
                a.depth = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--depth needs a number")?);
            }
            "--no-duplicates" => a.duplicates = false,
            "--no-nix" => a.nix = false,
            "--threads" => a.threads = it.next().and_then(|v| v.parse().ok()).ok_or("--threads needs a number")?,
            "--quiet" | "-q" => a.quiet = true,
            other => {
                if let Some(v) = other.strip_prefix("--output=") {
                    a.output = Some(PathBuf::from(v));
                } else if let Some(v) = other.strip_prefix("--depth=") {
                    a.depth = Some(v.parse().map_err(|_| "--depth needs a number")?);
                } else {
                    return Err(format!("unknown option {other}"));
                }
            }
        }
    }
    Ok(a)
}

/// Run the command line: the process's exit code.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{USAGE}");
            return 0;
        }
        Some("-V" | "--version") => {
            println!("aizome-kura {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        _ => {}
    }
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("aizome-kura: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let result = match a.command.as_str() {
        "scan" => cmd_scan(&a),
        "report" => cmd_report(&a),
        "hints" => cmd_hints(&a),
        "trash" => cmd_trash(&a),
        _ => Err(format!("unknown command {}", a.command)),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("aizome-kura: {e}");
            1
        }
    }
}

fn output_dir(a: &Args) -> Result<PathBuf, String> {
    a.output
        .clone()
        .or_else(index::default_dir)
        .ok_or_else(|| "no cache folder: set HOME or XDG_CACHE_HOME, or pass --output".into())
}

fn scan_root(a: &Args) -> Result<PathBuf, String> {
    match a.paths.first() {
        Some(p) => Ok(p.clone()),
        None => crate::home_dir().ok_or_else(|| "no PATH given and no HOME".into()),
    }
}

/// Scan, analyse and write the index: the index.
fn scan_and_write(a: &Args, root: &Path) -> Result<(Index, PathBuf, PathBuf), String> {
    let dir = output_dir(a)?;
    let progress = Progress::new();
    let show = !a.quiet && std::io::stderr().is_terminal();
    let ticker = show.then(|| spawn_ticker(progress.clone()));
    let options =
        ScanOptions { root: root.to_path_buf(), cross_filesystems: a.cross, threads: a.threads, skip: Vec::new() };
    let result = scan::scan(options, progress.clone());
    if let Some((stop, handle)) = ticker {
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = handle.join();
        eprint!("\r\x1b[2K");
    }
    let scan = result.map_err(|e| format!("{}: {e}", root.display()))?;
    if show {
        eprint!("Looking for what could be freed…");
    }
    let mut cx = crate::context(a.nix, Some(&dir));
    cx.duplicates = a.duplicates;
    cx.dupe_limits.threads = scan.threads.min(8);
    let limits =
        a.depth.map(|d| TreeLimits { max_depth: d, ..TreeLimits::for_total(scan.tree.node(scan.tree.root()).alloc) });
    let ix = crate::analyse(&scan, &cx, limits);
    if show {
        eprint!("\r\x1b[2K");
    }
    let (json, md) = index::write(&ix, &dir).map_err(|e| format!("writing the index to {}: {e}", dir.display()))?;
    Ok((ix, json, md))
}

fn spawn_ticker(progress: Arc<Progress>) -> (Arc<std::sync::atomic::AtomicBool>, std::thread::JoinHandle<()>) {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s = stop.clone();
    let handle = std::thread::spawn(move || {
        while !s.load(std::sync::atomic::Ordering::Relaxed) {
            let p = progress.snapshot();
            let mut err = std::io::stderr();
            let _ = write!(
                err,
                "\r\x1b[2KScanning… {} files, {} folders, {}",
                format::count(p.files),
                format::count(p.dirs),
                size(p.bytes)
            );
            let _ = err.flush();
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    (stop, handle)
}

fn cmd_scan(a: &Args) -> Result<i32, String> {
    let root = scan_root(a)?;
    let (ix, json, md) = scan_and_write(a, &root)?;
    if a.json {
        println!("{}", serde_json::to_string_pretty(&ix).map_err(|e| e.to_string())?);
        return Ok(0);
    }
    let t = &ix.totals;
    println!(
        "{}: {} on disk ({} apparent) in {} and {}, scanned in {:.1} s.",
        format::path(Path::new(&ix.root)),
        size(t.allocated_bytes),
        size(t.apparent_bytes),
        format::plural(t.files, "file", "files"),
        format::plural(t.directories, "folder", "folders"),
        ix.duration_ms as f64 / 1000.0
    );
    if t.errors > 0 {
        println!("{} couldn't be read.", format::plural(t.errors, "path", "paths"));
    }
    if ix.reclaimable.total_bytes > 0 {
        println!(
            "About {} could be freed ({} of it safe): `aizome-kura hints` says how.",
            size(ix.reclaimable.total_bytes),
            size(ix.reclaimable.by_risk.get("safe").copied().unwrap_or(0))
        );
    }
    println!("Index: {}\nReport: {}", json.display(), md.display());
    Ok(0)
}

/// The index to read: the last one, or a new scan if PATH is given or
/// there's none (or it can't be read).
fn load_or_scan(a: &Args) -> Result<Index, String> {
    let dir = output_dir(a)?;
    if a.paths.is_empty()
        && let Ok(ix) = index::read(&dir.join("index.json"))
    {
        return Ok(ix);
    }
    let root = scan_root(a)?;
    Ok(scan_and_write(a, &root)?.0)
}

fn cmd_report(a: &Args) -> Result<i32, String> {
    let ix = load_or_scan(a)?;
    print!("{}", crate::report::markdown(&ix));
    Ok(0)
}

/// What `hints --json` prints.
#[derive(serde::Serialize)]
struct HintsOut<'a> {
    schema: &'a str,
    version: u32,
    root: &'a str,
    scanned_at: &'a str,
    reclaimable: &'a index::Reclaimable,
    hints: &'a [crate::hints::Hint],
}

fn cmd_hints(a: &Args) -> Result<i32, String> {
    let ix = load_or_scan(a)?;
    if a.json {
        let out = HintsOut {
            schema: &ix.schema,
            version: ix.version,
            root: &ix.root,
            scanned_at: &ix.scanned_at,
            reclaimable: &ix.reclaimable,
            hints: &ix.hints,
        };
        println!("{}", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?);
        return Ok(0);
    }
    println!("{} (scanned {}):", format::path(Path::new(&ix.root)), ix.scanned_at);
    if ix.hints.is_empty() {
        println!("  Nothing stands out.");
    }
    for h in &ix.hints {
        let frees = h.frees();
        println!("\n{} ({}, {} risk)\n  {}", h.title, frees, h.risk.id(), h.summary);
        if let Some(c) = &h.action.command {
            println!("  {}\n  $ {}", h.action.description, c);
        } else {
            println!("  {}", h.action.description);
        }
        for p in h.paths.iter().take(5) {
            let size_s =
                if p.allocated_bytes > 0 { format!("{:>9}  ", size(p.allocated_bytes)) } else { String::new() };
            println!("    {size_s}{}", format::path(Path::new(&p.path)));
        }
        let more = h.paths.len().saturating_sub(5) as u64 + h.more_paths;
        if more > 0 {
            println!("    …and {more} more");
        }
    }
    Ok(0)
}

fn cmd_trash(a: &Args) -> Result<i32, String> {
    if a.paths.is_empty() {
        return Err("trash needs at least one PATH".into());
    }
    let trash = crate::trash::home_trash().ok_or("no home folder for the Trash")?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let paths: Vec<PathBuf> = a.paths.iter().map(|p| if p.is_absolute() { p.clone() } else { cwd.join(p) }).collect();
    let out = crate::trash::trash(&paths, &trash, &|_| None);
    if a.json {
        let v = serde_json::json!({
            "trashed": out.trashed.iter().map(|(from, to)| serde_json::json!({"path": from, "in_trash": to})).collect::<Vec<_>>(),
            "refused": out.refused.iter().map(|(p, why)| serde_json::json!({"path": p, "reason": why})).collect::<Vec<_>>(),
        });
        println!("{v:#}");
    } else {
        for (from, _) in &out.trashed {
            println!("Moved to the Trash: {}", from.display());
        }
        for (p, why) in &out.refused {
            eprintln!("Not moved: {}: {why}", p.display());
        }
    }
    Ok(if out.refused.is_empty() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_arguments() {
        let a = parse(&s(&["scan", "/data", "--json", "--output", "/tmp/x", "--depth=3", "--no-duplicates"])).unwrap();
        assert_eq!(a.command, "scan");
        assert_eq!(a.paths, [PathBuf::from("/data")]);
        assert!(a.json && !a.duplicates && a.nix);
        assert_eq!(a.output, Some(PathBuf::from("/tmp/x")));
        assert_eq!(a.depth, Some(3));
        assert!(parse(&s(&["scan", "--bogus"])).is_err());
        let t = parse(&s(&["trash", "--", "--weird-name"])).unwrap();
        assert_eq!(t.paths, [PathBuf::from("--weird-name")]);
        assert!(is_cli(&s(&["hints", "--json"])));
        assert!(!is_cli(&s(&["/home"])));
        assert!(!is_cli(&[]));
    }
}
