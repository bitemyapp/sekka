// SPDX-License-Identifier: GPL-3.0-or-later
//! `sekka-mac-tool`: release tooling for Sekka's Mac installer.
//!
//! ```text
//! sekka-mac-tool keygen KEYFILE              a new signing key (prints its public half)
//! sekka-mac-tool catalog SPEC.json > CATALOG an unsigned catalog from a specification
//! sekka-mac-tool sign KEYFILE KEY-ID CATALOG > SIGNED
//! sekka-mac-tool verify SIGNED               with this build's trust roots
//! sekka-mac-tool artifact FILE URL           size and digest, as a catalog artifact
//! sekka-mac-tool ledger init CANDIDATE.json > LEDGER
//! sekka-mac-tool ledger check LEDGER
//! ```
//!
//! `catalog` and `sign` write the document's exact canonical bytes, with
//! no newline after them: the app verifies a catalog byte for byte, so
//! `> SIGNED` is the file to publish. `verify` checks a file's bytes as they
//! are, as the app does.
//!
//! Signing with the development key (`--development` in place of a key
//! file) is for tests and local catalogs. A production key is generated
//! offline and never stored in the repository; nothing here publishes
//! anything.
use std::fs;
use std::io::{Read, Write};
use std::process::ExitCode;

use base64::Engine as _;
use sekka_mac_contract::{
    Artifact, Catalog, CatalogSchema, Digest, Hasher, PRODUCT, Timestamp, canonical, limits,
};

mod ledger;

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("sekka-mac-tool: {message}");
    ExitCode::FAILURE
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    if path == "-" {
        let mut v = Vec::new();
        std::io::stdin().read_to_end(&mut v).map_err(|e| e.to_string())?;
        return Ok(v);
    }
    fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn digest_file(path: &str) -> Result<(u64, Digest), String> {
    let mut f = fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok((size, hasher.finish()));
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
}

/// What `catalog` reads: everything a catalog has but its schema and
/// product, with dates as an issue time and a lifetime.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    channel: String,
    serial: u64,
    /// Issue time; now when absent.
    issued_at: Option<Timestamp>,
    days: i64,
    minimum_app_version: String,
    protocol: u32,
    profiles: Vec<serde_json::Value>,
    releases: Vec<serde_json::Value>,
    notice: Option<String>,
}

fn seed(path: &str) -> Result<[u8; 32], String> {
    if path == "--development" {
        return Ok(sekka_mac_trust::signing::development_seed());
    }
    let text = String::from_utf8(read(path)?).map_err(|e| e.to_string())?;
    let line = text
        .lines()
        .find(|l| !l.starts_with('#') && !l.trim().is_empty())
        .ok_or("the key file has no key")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(line.trim())
        .map_err(|e| e.to_string())?;
    bytes.try_into().map_err(|_| "a key is 32 bytes".to_string())
}

/// Write a document's exact bytes, nothing added.
fn emit(out: &mut dyn Write, bytes: &[u8]) -> Result<(), String> {
    out.write_all(bytes).and_then(|()| out.flush()).map_err(|e| e.to_string())
}

/// Write a line meant for a person.
fn say(out: &mut dyn Write, line: impl std::fmt::Display) -> Result<(), String> {
    writeln!(out, "{line}").map_err(|e| e.to_string())
}

/// Carry out one command, writing what it produces to `out`.
fn run(args: &[String], out: &mut dyn Write) -> Result<(), String> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["keygen", file] => {
            let mut s = [0u8; 32];
            fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut s))
                .map_err(|e| e.to_string())?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(s);
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            options
                .open(file)
                .and_then(|mut f| f.write_all(format!("# Sekka Mac catalog signing key. Keep it offline.\n{b64}\n").as_bytes()))
                .map_err(|e| format!("{file}: {e}"))?;
            let key = sekka_mac_trust::signing::key(&s);
            emit(out, sekka_mac_trust::signing::public_key_literal(&key).as_bytes())
        }
        ["artifact", file, url] => {
            let (size, digest) = digest_file(file)?;
            let artifact = Artifact {
                url: url.parse().map_err(|e: sekka_mac_contract::Error| e.to_string())?,
                size,
                digest,
            };
            say(out, serde_json::to_string_pretty(&artifact).map_err(|e| e.to_string())?)
        }
        ["catalog", spec] => {
            let spec: Spec = serde_json::from_slice(&read(spec)?).map_err(|e| e.to_string())?;
            let issued = spec.issued_at.unwrap_or_else(Timestamp::now);
            let value = serde_json::json!({
                "schema": CatalogSchema::VALUE,
                "product": PRODUCT,
                "channel": spec.channel,
                "serial": spec.serial,
                "issued_at": issued,
                "expires_at": issued.plus(spec.days * 86_400),
                "minimum_app_version": spec.minimum_app_version,
                "protocol": spec.protocol,
                "profiles": spec.profiles,
                "releases": spec.releases,
                "notice": spec.notice,
            });
            let catalog: Catalog = serde_json::from_value(value).map_err(|e| e.to_string())?;
            catalog.check().map_err(|e| e.to_string())?;
            emit(out, &canonical::to_vec(&catalog).map_err(|e| e.to_string())?)
        }
        ["sign", key, key_id, catalog] => {
            let catalog: Catalog = canonical::from_slice_relaxed(&read(catalog)?, limits::CATALOG, CatalogSchema::VALUE)
                .map_err(|e| e.to_string())?;
            catalog.check().map_err(|e| e.to_string())?;
            let signing = sekka_mac_trust::signing::key(&seed(key)?);
            let signed = sekka_mac_trust::signing::sign(catalog, key_id, &signing).map_err(|e| e.to_string())?;
            emit(out, &canonical::to_vec(&signed).map_err(|e| e.to_string())?)
        }
        ["verify", signed] => {
            // The file's bytes exactly as the app receives them: a newline
            // or any other change makes the app refuse it, so it fails here.
            let verified = sekka_mac_trust::verify(
                &read(signed)?,
                &sekka_mac_trust::TrustConfig::for_this_build(),
                Timestamp::now(),
            )
            .map_err(|e| e.to_string())?;
            let c = verified.catalog();
            say(
                out,
                format_args!(
                    "{} catalog {} for {}: {} release(s), signed by {}, valid until {}",
                    c.channel,
                    c.serial,
                    c.product,
                    c.releases.len(),
                    verified.signed().key_id,
                    c.expires_at
                ),
            )
        }
        ["ledger", "init", candidate] => {
            let candidate: ledger::Candidate = serde_json::from_slice(&read(candidate)?).map_err(|e| e.to_string())?;
            let l = ledger::Ledger {
                schema: ledger::LedgerSchema,
                candidate,
                rows: ledger::rows(),
            };
            say(out, serde_json::to_string_pretty(&l).map_err(|e| e.to_string())?)
        }
        ["ledger", "check", path] => {
            let l: ledger::Ledger = serde_json::from_slice(&read(path)?).map_err(|e| e.to_string())?;
            let (problems, complete) = ledger::check(&l);
            for gate in ledger::GATES.iter().map(|g| g.0) {
                let rows: Vec<&ledger::Row> = l.rows.iter().filter(|r| r.gate == gate).collect();
                let pass = rows.iter().filter(|r| r.status == ledger::Status::Pass).count();
                let na = rows.iter().filter(|r| r.status == ledger::Status::NotApplicable).count();
                say(out, format_args!("{gate}: {pass} pass, {na} not applicable, {} other, of {}", rows.len() - pass - na, rows.len()))?;
            }
            for p in &problems {
                say(out, format_args!("problem: {p}"))?;
            }
            if !problems.is_empty() {
                return Err(format!("{} problem(s) in the ledger", problems.len()));
            }
            if !complete {
                return Err("the candidate isn't qualified: some cases haven't passed".into());
            }
            say(out, "Every required case passed or doesn't apply.")
        }
        _ => Err("usage: sekka-mac-tool keygen|catalog|sign|verify|artifact|ledger … (see the source's header)".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args, &mut std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a command writes to standard output.
    fn tool(args: &[&str]) -> Result<Vec<u8>, String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut out = Vec::new();
        run(&args, &mut out).map(|()| out)
    }

    /// A specification becomes a catalog through `catalog` and is signed
    /// with the development key through `sign`. What `sign` writes is the
    /// file the app reads: it verifies unchanged, and with a newline added
    /// it doesn't.
    #[test]
    fn catalog_sign_verify() {
        let dir = tempfile::tempdir().unwrap();
        let sample = sekka_mac_contract::samples::catalog();
        let spec = serde_json::json!({
            "channel": "development",
            "serial": 3,
            "days": 30,
            "minimum_app_version": "0.1.0",
            "protocol": 1,
            "profiles": sample.profiles,
            "releases": sample.releases,
            "notice": "Development builds only.",
        });
        let at = |name: &str| dir.path().join(name).to_string_lossy().into_owned();
        fs::write(at("spec.json"), serde_json::to_vec(&spec).unwrap()).unwrap();

        let catalog = tool(&["catalog", &at("spec.json")]).unwrap();
        let parsed: Catalog = canonical::from_slice(&catalog, limits::CATALOG, CatalogSchema::VALUE)
            .expect("catalog writes exactly the canonical bytes");
        assert_eq!(parsed.serial, 3);
        fs::write(at("catalog.json"), &catalog).unwrap();

        let signed = tool(&["sign", "--development", sekka_mac_trust::DEVELOPMENT_KEY_ID, &at("catalog.json")]).unwrap();
        assert_ne!(signed.last(), Some(&b'\n'));
        fs::write(at("signed.json"), &signed).unwrap();
        let config = sekka_mac_trust::TrustConfig::for_this_build();
        let verified = sekka_mac_trust::verify(&fs::read(at("signed.json")).unwrap(), &config, Timestamp::now())
            .expect("what sign writes verifies as the app verifies it");
        assert_eq!(verified.catalog(), &parsed);
        let said = String::from_utf8(tool(&["verify", &at("signed.json")]).unwrap()).unwrap();
        assert!(said.contains("development catalog 3"), "{said}");

        // One byte more and the app refuses it, so verify does too.
        let mut newline = signed.clone();
        newline.push(b'\n');
        fs::write(at("newline.json"), &newline).unwrap();
        assert!(sekka_mac_trust::verify(&newline, &config, Timestamp::now()).is_err());
        assert!(tool(&["verify", &at("newline.json")]).is_err());

        // A key from keygen signs too, and isn't trusted.
        tool(&["keygen", &at("other.key")]).unwrap();
        assert!(tool(&["keygen", &at("other.key")]).is_err(), "never overwrites a key");
        let forged = tool(&["sign", &at("other.key"), sekka_mac_trust::DEVELOPMENT_KEY_ID, &at("catalog.json")]).unwrap();
        fs::write(at("forged.json"), &forged).unwrap();
        assert!(tool(&["verify", &at("forged.json")]).is_err());
    }
}
