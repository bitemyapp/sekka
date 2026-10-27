//! The logic's build identity: a hash of its sources, so the core can say
//! which logic is running and whether a candidate differs from it.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut files = Vec::new();
    collect(&dir.join("src"), &mut files);
    for f in ["Cargo.toml", "build.rs"] {
        files.push(dir.join(f));
    }
    files.sort();
    let mut h = Sha256::new();
    for f in &files {
        h.update(f.strip_prefix(&dir).unwrap_or(f).to_string_lossy().as_bytes());
        h.update(std::fs::read(f).unwrap_or_default());
    }
    // A feature or variant built from the same sources is another build.
    for (k, v) in std::env::vars() {
        if k.starts_with("CARGO_FEATURE_") || k == "AIZOME_LOGIC_VARIANT" {
            h.update(k.as_bytes());
            h.update(v.as_bytes());
        }
    }
    println!("cargo:rerun-if-env-changed=AIZOME_LOGIC_VARIANT");
    println!("cargo:rerun-if-changed=src");
    let hash: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=AIZOME_LOGIC_BUILD={hash}");
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else {
            out.push(p);
        }
    }
}
