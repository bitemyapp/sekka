//! The scanner, hints, index and report together, on a made-up home folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use kura::hints::{self, Risk};
use kura::scan::{self, Progress, ScanOptions};

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kura-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
}

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
    fs::write(path, data).unwrap();
}

fn age(path: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 86_400);
    fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
}

/// A home with one of everything the hints look for.
fn home(name: &str) -> PathBuf {
    let h = fixture(name);
    write(&h.join(".cache/someapp/blob"), 2_000_000);
    write(&h.join(".cache/pip/http/x"), 1_500_000);
    write(&h.join(".cache/mozilla/firefox/abc.default/cache2/entries/1"), 1_200_000);
    write(&h.join("src/proj/Cargo.toml"), 10);
    write(&h.join("src/proj/target/debug/big.rlib"), 3_000_000);
    write(&h.join("src/web/package.json"), 10);
    write(&h.join("src/web/node_modules/left-pad/index.js"), 1_100_000);
    write(&h.join("src/web/node_modules/x/node_modules/y/z.js"), 1_100_000);
    write(&h.join("Downloads/old-installer.iso"), 2_500_000);
    age(&h.join("Downloads/old-installer.iso"), 400);
    write(&h.join("Downloads/new.pdf"), 1_000_000);
    write(&h.join("Pictures/a.jpg"), 1_300_000);
    write(&h.join("Documents/copy-of-a.jpg"), 1_300_000);
    fs::copy(h.join("Pictures/a.jpg"), h.join("Documents/copy-of-a.jpg")).unwrap();
    write(&h.join("repo/.git/HEAD"), 10);
    write(&h.join("repo/asset.bin"), 1_300_000);
    fs::copy(h.join("Pictures/a.jpg"), h.join("repo/asset.bin")).unwrap();
    write(&h.join(".local/share/Trash/files/gone.bin"), 1_400_000);
    write(&h.join(".local/share/Trash/info/gone.bin.trashinfo"), 60);
    write(&h.join(".local/state/app/logs/app.log"), 11_000_000);
    write(&h.join(".config/chrome-ish/Default/IndexedDB/x.leveldb/000123.log"), 2_000_000);
    write(&h.join(".config/chrome-ish/Default/IndexedDB/x.leveldb/CURRENT"), 16);
    h
}

fn context(h: &Path) -> hints::Context {
    let mut cx = hints::Context::from_env();
    cx.home = Some(h.to_path_buf());
    cx.cache_home = Some(h.join(".cache"));
    cx.data_home = Some(h.join(".local/share"));
    cx.config_home = Some(h.join(".config"));
    cx.downloads = Some(h.join("Downloads"));
    cx.trash = Some(h.join(".local/share/Trash"));
    cx.nix_profiles = Vec::new();
    cx.nix_store = None;
    cx
}

#[test]
fn hints_cover_each_kind_once() {
    let h = home("hints");
    let s = scan::scan(ScanOptions::new(&h), Progress::new()).unwrap();
    let cx = context(&h);
    let hs = hints::detect(&s, &cx);
    let by_id = |id: &str| {
        hs.iter()
            .find(|x| x.id == id)
            .unwrap_or_else(|| panic!("a {id} hint in {:#?}", hs.iter().map(|x| &x.id).collect::<Vec<_>>()))
    };
    let has = |id: &str, p: &Path| by_id(id).paths.iter().any(|x| Path::new(&x.path) == p);

    assert!(has("caches", &h.join(".cache/someapp")));
    assert!(has("package-caches", &h.join(".cache/pip")));
    assert!(has("browser-caches", &h.join(".cache/mozilla/firefox/abc.default/cache2")));
    let build = by_id("build-outputs");
    assert_eq!(build.risk, Risk::Low);
    assert!(has("build-outputs", &h.join("src/proj/target")));
    let target = build.paths.iter().find(|p| p.path.ends_with("proj/target")).unwrap();
    assert!(target.command.as_deref().unwrap().starts_with("cargo clean --manifest-path"));
    // The outer node_modules only: the inner one is part of it.
    assert!(has("build-outputs", &h.join("src/web/node_modules")));
    assert_eq!(build.paths.iter().filter(|p| p.path.contains("node_modules")).count(), 1);
    assert!(has("old-downloads", &h.join("Downloads/old-installer.iso")));
    assert!(!has("old-downloads", &h.join("Downloads/new.pdf")));
    assert!(has("trash", &h.join(".local/share/Trash/files")));
    assert!(has("logs", &h.join(".local/state/app/logs")));
    assert!(!hs.iter().any(|x| x.paths.iter().any(|p| p.path.ends_with("000123.log"))), "LevelDB data isn't a log");

    let d = by_id("duplicates");
    assert_eq!(d.groups.len(), 1);
    assert_eq!(d.groups[0].verified, "full");
    assert_eq!(d.groups[0].paths.len(), 2, "the copy in a Git working tree isn't offered: {:?}", d.groups[0].paths);
    assert_eq!(d.count_unit.as_deref(), Some("sets"));

    // No byte counted twice: the hints' sum is within what was scanned.
    let total: u64 = hs.iter().map(|x| x.reclaimable_bytes).sum();
    assert!(total <= s.tree.node(s.tree.root()).alloc);
    for x in &hs {
        let listed: u64 = x.paths.iter().map(|p| p.allocated_bytes).sum();
        if x.more_paths == 0 && x.id != "duplicates" {
            assert_eq!(listed, x.reclaimable_bytes, "{}", x.id);
        }
    }
}

#[test]
fn index_round_trips_and_reports() {
    let h = home("index");
    let s = scan::scan(ScanOptions::new(&h), Progress::new()).unwrap();
    let cx = context(&h);
    let ix = kura::analyse(&s, &cx, None);
    assert_eq!(ix.schema, "org.aizome.kura.index");
    assert_eq!(ix.version, 1);
    assert_eq!(ix.root, h.display().to_string());
    assert_eq!(ix.totals.allocated_bytes, s.tree.node(s.tree.root()).alloc);
    assert!(ix.largest_files[0].allocated_bytes >= ix.largest_files[1].allocated_bytes);
    assert!(ix.largest_files.iter().any(|f| f.path.ends_with("old-installer.iso") && f.kind == "disk-image"));
    assert!(ix.kinds.iter().any(|k| k.kind == kura::kinds::FileKind::Log));
    assert_eq!(ix.tree.node_type, "directory");
    assert!(!ix.tree.children.is_empty());

    let out = fixture("out");
    let (json, md) = kura::index::write(&ix, &out).unwrap();
    let back = kura::index::read(&json).unwrap();
    assert_eq!(back.hints.len(), ix.hints.len());
    assert_eq!(back.totals.files, ix.totals.files);
    let report = fs::read_to_string(md).unwrap();
    assert!(report.starts_with("# Disk usage of "));
    assert!(report.contains("## What could be freed"));
    assert!(report.contains("Build outputs and dependencies"));
    assert!(report.contains("aizome-kura trash"));

    // A newer version than this Kura knows is refused, not misread.
    let text = fs::read_to_string(&json).unwrap().replacen("\"version\": 1", "\"version\": 99", 1);
    fs::write(&json, text).unwrap();
    assert!(kura::index::read(&json).is_err());
}

#[test]
fn command_line_scans_into_output() {
    let h = home("cli");
    let out = fixture("cli-out");
    let bin = env!("CARGO_BIN_EXE_aizome-kura");
    let run = std::process::Command::new(bin)
        .args(["scan", h.to_str().unwrap(), "--output", out.to_str().unwrap(), "--no-nix", "--quiet", "--json"])
        .output()
        .unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let v: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(v["schema"], "org.aizome.kura.index");
    assert!(out.join("index.json").exists() && out.join("index.md").exists());

    let hints =
        std::process::Command::new(bin).args(["hints", "--json", "--output", out.to_str().unwrap()]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&hints.stdout).unwrap();
    assert!(v["hints"].as_array().unwrap().iter().any(|x| x["id"] == "build-outputs"));

    let report = std::process::Command::new(bin).args(["report", "--output", out.to_str().unwrap()]).output().unwrap();
    assert!(String::from_utf8_lossy(&report.stdout).contains("## Largest files"));

    let bad = std::process::Command::new(bin).args(["scan", "--frobnicate"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
}
