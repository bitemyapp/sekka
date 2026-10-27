//! Running Settei headlessly over the sample system, with its settings
//! in a directory of the test's own.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use aizome_kit::testing::Harness;
use settei::{Settei, Start};

static N: AtomicU32 = AtomicU32::new(0);

/// A fresh directory for a test's files.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "settei-test-{}-{name}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    dir
}

/// Settei over the sample system, opened at `start` (a pane id, an
/// `id.entry`, or empty for the first pane).
pub fn open(name: &str, start: &str) -> (Harness<Settei>, PathBuf) {
    let dir = temp_dir(name);
    let path = dir.join("settings.toml");
    let start = if start.is_empty() { Start::default() } else { Start::parse(start) };
    let h = Harness::new(Settei::sample(path.clone(), start));
    (h, path)
}

/// Where pictures for people to look at go.
pub fn shots_dir() -> PathBuf {
    let dir = std::env::var_os("SETTEI_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("settei-shots"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}
