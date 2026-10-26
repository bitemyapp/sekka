//! The core's ABI fingerprint: a hash of everything that decides the
//! layout of the core's types and the names of its symbols, so a logic
//! library is loaded only into the core it was built against.
//!
//! It covers
//!
//! - the sources of every path package the core depends on (this crate,
//!   aizome-config, aizome-ipc, aizome-theme, Sidestep's crates): their
//!   `src/`, `build.rs` and `Cargo.toml`;
//! - the entries of `Cargo.lock` for every other package in the core's
//!   dependency closure (name, version, source, checksum);
//! - `rustc -vV`;
//! - the target, the profile (opt level, debug info, panic strategy, debug
//!   assertions) and RUSTFLAGS (less linker arguments, which change no
//!   code);
//! - the core's own features.
//!
//! The parts are hashed separately too (`AIZOME_CORE_ABI_PARTS`), so a
//! refusal can say what differs. The logic embeds the fingerprint of the
//! core it was compiled against; the running core compares it with its own
//! before it loads the library, and the dynamic loader then checks every
//! symbol (docs/reload.md).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn env(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    std::env::var(name).ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir.parent().and_then(Path::parent).expect("the workspace").to_path_buf();

    // Without -C prefer-dynamic the core takes std in statically, and
    // nothing can link it ("cannot satisfy dependencies so `std` only shows
    // up once"): say so before that happens.
    let wrapped = env("RUSTC_WRAPPER").is_some_and(|w| w.contains("rustc-wrapper"))
        || env("CARGO_ENCODED_RUSTFLAGS").is_some_and(|f| f.contains("prefer-dynamic"));
    if !wrapped {
        println!(
            "cargo:warning=aizome-comp-core needs `-C prefer-dynamic`: build in `nix develop`, or set \
             RUSTC_WRAPPER={}/scripts/rustc-wrapper",
            workspace.display()
        );
    }

    let mut parts: BTreeMap<&str, String> = BTreeMap::new();

    // The toolchain.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = std::process::Command::new(&rustc)
        .arg("-vV")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    parts.insert("toolchain", digest(version.as_bytes()));

    // Target, profile and flags.
    let mut build = String::new();
    for name in ["TARGET", "PROFILE", "OPT_LEVEL", "DEBUG", "CARGO_CFG_PANIC", "CARGO_CFG_TARGET_FEATURE"] {
        build.push_str(&format!("{name}={}\n", env(name).unwrap_or_default()));
    }
    build.push_str(&format!("debug_assertions={}\n", std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some()));
    // Cargo's `--target` (even naming the host) puts the build under
    // target/<triple>/ and gives every crate other symbols.
    let target = std::env::var("TARGET").unwrap_or_default();
    let explicit_target =
        std::env::var("OUT_DIR").is_ok_and(|d| Path::new(&d).components().any(|c| c.as_os_str() == target.as_str()));
    build.push_str(&format!("explicit_target={explicit_target}\n"));
    let flags = env("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    build.push_str(&format!("rustflags={}\n", codegen_flags(&flags).join(" ")));
    let mut features: Vec<String> =
        std::env::vars().filter_map(|(k, _)| k.strip_prefix("CARGO_FEATURE_").map(String::from)).collect();
    features.sort();
    build.push_str(&format!("features={}\n", features.join(",")));
    parts.insert("build", digest(build.as_bytes()));

    // The dependency closure, from Cargo.lock.
    let lock_path = workspace.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    println!("cargo:rerun-if-changed={}", workspace.join("Cargo.toml").display());
    let lock = std::fs::read_to_string(&lock_path).expect("Cargo.lock");
    let lock: toml::Table = lock.parse().expect("Cargo.lock parses");
    let packages: Vec<toml::Table> = lock
        .get("package")
        .and_then(|p| p.as_array())
        .map(|a| a.iter().filter_map(|p| p.as_table().cloned()).collect())
        .unwrap_or_default();
    let field = |p: &toml::Table, k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let closure = closure(&packages, "aizome-comp-core");
    let mut registry = String::new();
    let mut path_packages = Vec::new();
    for i in &closure {
        let p = &packages[*i];
        if p.get("source").is_none() {
            path_packages.push(field(p, "name"));
        } else {
            registry.push_str(&format!(
                "{} {} {} {}\n",
                field(p, "name"),
                field(p, "version"),
                field(p, "source"),
                field(p, "checksum")
            ));
        }
    }
    parts.insert("lock", digest(registry.as_bytes()));

    // The path packages' sources.
    let mut sources = Sha256::new();
    // Each package's too, so a refused logic can say which differ.
    let mut each_package = Vec::new();
    path_packages.sort();
    for name in &path_packages {
        let Some(dir) = package_dir(&workspace, name) else {
            panic!("aizome-comp-core: can't find the path package {name} (looked in crates/ and sidestep/crates/)");
        };
        sources.update(name.as_bytes());
        let mut files = Vec::new();
        for f in ["Cargo.toml", "build.rs"] {
            let p = dir.join(f);
            if p.is_file() {
                files.push(p);
            }
        }
        collect(&dir.join("src"), &mut files);
        files.sort();
        println!("cargo:rerun-if-changed={}", dir.join("src").display());
        println!("cargo:rerun-if-changed={}", dir.join("Cargo.toml").display());
        let mut this = Sha256::new();
        for f in files {
            let rel = f.strip_prefix(&dir).unwrap_or(&f);
            let bytes = std::fs::read(&f).unwrap_or_default();
            sources.update(rel.to_string_lossy().as_bytes());
            sources.update(&bytes);
            this.update(rel.to_string_lossy().as_bytes());
            this.update(&bytes);
        }
        each_package.push(format!("{name}={}", &hex(&this.finalize())[..12]));
    }
    parts.insert("sources", hex(&sources.finalize()));

    let mut all = Sha256::new();
    for (k, v) in &parts {
        all.update(k.as_bytes());
        all.update(v.as_bytes());
    }
    let abi = hex(&all.finalize());
    let summary: Vec<String> = parts.iter().map(|(k, v)| format!("{k}={}", &v[..12])).collect();
    println!("cargo:rustc-env=AIZOME_CORE_ABI={abi}");
    println!("cargo:rustc-env=AIZOME_CORE_ABI_PARTS={}", summary.join(" "));
    println!("cargo:rustc-env=AIZOME_CORE_RUSTC={}", version.lines().next().unwrap_or("").trim());
    println!("cargo:rustc-env=AIZOME_CORE_TARGET={}", std::env::var("TARGET").unwrap_or_default());
    println!("cargo:rustc-env=AIZOME_CORE_PROFILE={}", std::env::var("PROFILE").unwrap_or_default());
    println!("cargo:rustc-env=AIZOME_CORE_OPT_LEVEL={}", std::env::var("OPT_LEVEL").unwrap_or_default());
    println!("cargo:rustc-env=AIZOME_CORE_RUSTFLAGS={}", codegen_flags(&flags).join(" "));
    println!("cargo:rustc-env=AIZOME_CORE_EXPLICIT_TARGET={explicit_target}");
    println!("cargo:rustc-env=AIZOME_CORE_SOURCES={}", each_package.join(" "));
}

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// RUSTFLAGS (as Cargo encodes them, separated by 0x1f) without the ones
/// that only steer the linker or rewrite paths: they change no code.
fn codegen_flags(encoded: &str) -> Vec<String> {
    let args: Vec<&str> = encoded.split('\u{1f}').filter(|a| !a.is_empty()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        let (flag, value, consumed) = if (a == "-C" || a == "--remap-path-prefix") && i + 1 < args.len() {
            (a, args[i + 1], 2)
        } else if let Some(v) = a.strip_prefix("-C") {
            ("-C", v, 1)
        } else {
            (a, "", 1)
        };
        let drop = (flag == "-C"
            && (value.starts_with("link-arg") || value.starts_with("linker") || value.starts_with("link-args")))
            || flag == "--remap-path-prefix"
            || a.starts_with("--remap-path-prefix=");
        if !drop {
            out.push(if consumed == 2 { format!("{flag} {value}") } else { a.to_string() });
        }
        i += consumed;
    }
    out
}

/// The indices of `root` and every package it depends on, through
/// Cargo.lock's dependency lists.
fn closure(packages: &[toml::Table], root: &str) -> Vec<usize> {
    let name_of = |p: &toml::Table| p.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let version_of = |p: &toml::Table| p.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut seen = BTreeSet::new();
    let mut todo: Vec<usize> = packages.iter().position(|p| name_of(p) == root).into_iter().collect();
    while let Some(i) = todo.pop() {
        if !seen.insert(i) {
            continue;
        }
        let deps = packages[i].get("dependencies").and_then(|d| d.as_array()).cloned().unwrap_or_default();
        for d in deps {
            let Some(d) = d.as_str() else { continue };
            // "name", "name version" or "name version (source)".
            let mut words = d.split_whitespace();
            let name = words.next().unwrap_or("");
            let version = words.next();
            let found = packages.iter().position(|p| name_of(p) == name && version.is_none_or(|v| version_of(p) == v));
            if let Some(j) = found {
                todo.push(j);
            }
        }
    }
    seen.into_iter().collect()
}

/// Where a path package lives: the workspace's crates or Sidestep's.
fn package_dir(workspace: &Path, name: &str) -> Option<PathBuf> {
    for base in [workspace.join("crates"), workspace.join("sidestep/crates"), workspace.join("apps")] {
        let dir = base.join(name);
        if declares(&dir, name) {
            return Some(dir);
        }
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for e in entries.flatten() {
            if declares(&e.path(), name) {
                return Some(e.path());
            }
        }
    }
    None
}

fn declares(dir: &Path, name: &str) -> bool {
    std::fs::read_to_string(dir.join("Cargo.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("package")?.get("name")?.as_str().map(|n| n == name))
        .unwrap_or(false)
}

/// Every file under a package's `src/`, but its programs' (`src/main.rs`,
/// `src/bin/`): they aren't part of the library the core links.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let top = dir.file_name().is_some_and(|n| n == "src");
    for e in entries.flatten() {
        let p = e.path();
        if top && (p.file_name().is_some_and(|n| n == "bin" || n == "main.rs")) {
            continue;
        }
        if p.is_dir() {
            collect(&p, out);
        } else {
            out.push(p);
        }
    }
}
