//! The compile-fail suite: probes of each target's illegal operations
//! (numbered in `spec/src/<target>/SPEC.md`) against each implementation's
//! own API, classified by where they live:
//!
//! ```text
//! cases/<impl>/<target>/reject/<ID>-<slug>.rs   rejected at compile time (with its .stderr)
//! cases/<impl>/<target>/runtime/<ID>-<slug>.rs  compiles; the implementation refuses at run time
//! cases/<impl>/<target>/accept/<ID>-<slug>.rs   compiles and the illegal operation happens
//! cases/<impl>/<target>/forge/F-<means>.rs       compiles; forging the state (not a transition)
//!                                                commits one of the target's illegal operations
//! ```
//!
//! See "Transitions and forging" in IMPLEMENTING.md for which class a case
//! belongs to.
//!
//! A `reject` case must fail to compile (trybuild compares its error
//! output with the `.stderr` beside it: `TRYBUILD=overwrite` writes it). A
//! `runtime` or `accept` case must compile and run to success: its `main`
//! asserts the refusal, or that the illegal operation went through.

use std::path::{Path, PathBuf};

pub const TARGETS: [&str; 6] = ["pointer", "wrap", "polkit", "transfer", "lock", "install"];

fn cases(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> =
        entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
    v.sort();
    v
}

/// Runs every case of `implementation` (the directory name under `cases/`).
pub fn run(implementation: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases").join(implementation);
    let mut found = Vec::new();
    for target in TARGETS {
        for kind in ["reject", "runtime", "accept", "forge"] {
            for case in cases(&root.join(target).join(kind)) {
                found.push((kind, case));
            }
        }
    }
    if found.is_empty() {
        eprintln!("compile-fail: no cases for {implementation} yet");
        return;
    }
    let t = trybuild::TestCases::new();
    for (kind, case) in &found {
        match *kind {
            "reject" => t.compile_fail(case),
            _ => t.pass(case),
        }
    }
    // The cases run when `t` is dropped.
}
