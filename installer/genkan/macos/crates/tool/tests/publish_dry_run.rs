// SPDX-License-Identifier: GPL-3.0-or-later
//! packaging/publish-dry-run.py with this crate's sekka-mac-tool. The dry
//! run asks the tool whether a ledger is complete, so what it refuses is
//! what `ledger check` refuses.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sekka_mac_contract::{Digest, GIB, samples};
use serde_json::{Value, json};

const TOOL: &str = env!("CARGO_BIN_EXE_sekka-mac-tool");

fn tool(args: &[&str]) -> Vec<u8> {
    let out = Command::new(TOOL).args(args).output().unwrap();
    assert!(out.status.success(), "sekka-mac-tool {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

/// Whether the dry run accepted the candidate, and what it said.
fn dry_run(dir: &Path) -> (bool, String) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/publish-dry-run.py");
    let out = Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(dir)
        .arg("--tool")
        .arg(TOOL)
        .output()
        .expect("python3 runs the packaging scripts");
    let said = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), said)
}

/// A development candidate: the five artifacts a release names and the
/// DMG, a catalog for them signed through `catalog` and `sign`, and a
/// ledger started by `ledger init` with every case passed.
struct Candidate {
    dir: tempfile::TempDir,
}

impl Candidate {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str| dir.path().join(name);
        let files: [(&str, &str, Vec<u8>); 6] = [
            ("payload", "payload.zip", vec![7; 8192]),
            ("installer_data", "installer_data.json", b"{}".to_vec()),
            ("manifest", "manifest.json", b"{\"m\":1}".to_vec()),
            ("engine", "engine.tar.gz", b"engine".to_vec()),
            ("recovery_app", "recovery.zip", b"recovery".to_vec()),
            ("", "Sekka-Installer-0.1.0.dmg", b"dmg".to_vec()),
        ];
        let mut release = serde_json::to_value(samples::release()).unwrap();
        release["image"]["root_image"]["size"] = json!(4096);
        release["allocation"] = json!({"minimum": 40 * GIB, "recommended": 120 * GIB});
        let mut recorded = Vec::new();
        for (role, name, data) in &files {
            fs::write(at(name), data).unwrap();
            let (size, digest) = (data.len(), Digest::of(data).to_string());
            if !role.is_empty() {
                let url = format!("https://downloads.sekka.computer/mac/v1/releases/{}/{name}", release["id"].as_str().unwrap());
                release["artifacts"][role] = json!({"url": url, "size": size, "digest": digest});
            }
            recorded.push(json!({"name": name, "size": size, "digest": digest}));
        }
        let sample = samples::catalog();
        let spec = json!({
            "channel": "development", "serial": 7, "days": 30, "minimum_app_version": "0.1.0",
            "protocol": 1, "profiles": sample.profiles, "releases": [release.clone()],
        });
        let path = |name: &str| at(name).to_string_lossy().into_owned();
        fs::write(at("spec.json"), spec.to_string()).unwrap();
        fs::write(at("catalog.json"), tool(&["catalog", &path("spec.json")])).unwrap();
        let signed = tool(&["sign", "--development", sekka_mac_trust::DEVELOPMENT_KEY_ID, &path("catalog.json")]);
        fs::write(at("catalog.signed.json"), signed).unwrap();
        let candidate = json!({
            "source_revision": release["source"]["revision"], "lock": release["source"]["lock"],
            "artifacts": recorded, "created": "2026-11-06T21:00:00Z", "toolchain": "cargo test",
        });
        fs::write(at("candidate.json"), candidate.to_string()).unwrap();
        let mut ledger: Value = serde_json::from_slice(&tool(&["ledger", "init", &path("candidate.json")])).unwrap();
        for row in ledger["rows"].as_array_mut().unwrap() {
            if row["gate"] == "M7" {
                row["status"] = json!("not-applicable");
                row["reason"] = json!("Encryption is deferred: docs/apple-silicon/encryption.md");
            } else {
                row["status"] = json!("pass");
                row["observed"] = json!("as the case says");
                row["evidence"] = json!("runs/1");
            }
        }
        let c = Self { dir };
        c.write_ledger(&ledger);
        for scratch in ["spec.json", "catalog.json", "candidate.json"] {
            fs::remove_file(c.path().join(scratch)).unwrap();
        }
        c
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path().join(name)
    }

    fn ledger(&self) -> Value {
        serde_json::from_slice(&fs::read(self.file("ledger.json")).unwrap()).unwrap()
    }

    fn write_ledger(&self, ledger: &Value) {
        fs::write(self.file("ledger.json"), serde_json::to_vec_pretty(ledger).unwrap()).unwrap();
    }
}

fn refused(c: &Candidate, why: &str) {
    let (ok, said) = dry_run(c.path());
    assert!(!ok, "accepted:\n{said}");
    assert!(said.contains(why), "refused, but not for {why:?}:\n{said}");
    assert!(!said.contains("would upload"), "{said}");
}

#[test]
fn a_qualified_candidate_is_planned_catalog_last() {
    let c = Candidate::new();
    let (ok, said) = dry_run(c.path());
    assert!(ok, "{said}");
    let uploads: Vec<&str> = said.lines().filter(|l| l.starts_with("would upload")).collect();
    assert_eq!(uploads.len(), 6, "{said}");
    assert!(uploads[5].ends_with("catalog-development.signed.json"), "{said}");
}

#[test]
fn an_empty_partial_or_unfinished_ledger_is_refused() {
    let c = Candidate::new();
    let full = c.ledger();

    let mut empty = full.clone();
    empty["rows"] = json!([]);
    c.write_ledger(&empty);
    refused(&c, "refused: the ledger isn't complete: M0-01 is missing");

    c.write_ledger(&json!({"rows": []}));
    refused(&c, "refused: the ledger isn't complete: missing field");

    let mut partial = full.clone();
    partial["rows"].as_array_mut().unwrap().retain(|r| r["gate"] != "M4");
    c.write_ledger(&partial);
    refused(&c, "refused: the ledger isn't complete: M4-17 is missing");

    let mut unfinished = full.clone();
    unfinished["rows"][3]["status"] = json!("not-run");
    c.write_ledger(&unfinished);
    refused(&c, "some cases haven't passed");

    let mut repeated = full.clone();
    let first = repeated["rows"][0].clone();
    repeated["rows"].as_array_mut().unwrap().push(first);
    c.write_ledger(&repeated);
    refused(&c, "M0-01 appears twice");

    c.write_ledger(&full);
    assert!(dry_run(c.path()).0);
}

#[test]
fn another_candidates_ledger_is_refused() {
    let c = Candidate::new();
    let other = Candidate::new();
    let mut foreign = other.ledger();
    // Another candidate of the same revision, with other payload bytes.
    for a in foreign["candidate"]["artifacts"].as_array_mut().unwrap() {
        if a["name"] == "payload.zip" {
            a["digest"] = json!(Digest::of(b"rc1").to_string());
        }
    }
    c.write_ledger(&foreign);
    refused(&c, "the ledger's candidate doesn't record payload.zip with the catalog's size and digest");
}
