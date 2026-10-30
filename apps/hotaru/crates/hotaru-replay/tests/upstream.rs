//! Snapshots are produced by the pinned Zig libghostty-vt oracle, never by the
//! Rust implementation. Running these requires no Zig library or toolchain.
use serde_json::Value;
use std::{path::PathBuf, process::Command};

fn strip_metadata(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|key, _| !key.starts_with('_'));
            for value in fields.values_mut() {
                strip_metadata(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_metadata(value);
            }
        }
        _ => {}
    }
}

#[test]
fn matches_pinned_ghostty_snapshots_at_read_boundaries() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fixtures");
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "vt"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "upstream fixtures missing");
    for file in files {
        let fixture: Value =
            serde_json::from_slice(&std::fs::read(file.with_extension("json")).unwrap()).unwrap();
        assert_eq!(
            fixture["upstream_revision"],
            "6ba144158577d033ccb6ed4b574e4013957c9161"
        );
        let mut expected = fixture["snapshot"].clone();
        strip_metadata(&mut expected);
        for chunk in [1, 7, 65536] {
            let output = Command::new(env!("CARGO_BIN_EXE_hotaru-replay"))
                .arg("--data")
                .arg(&file)
                .args([
                    "--rows",
                    &expected["rows"].to_string(),
                    "--cols",
                    &expected["cols"].to_string(),
                    "--chunk",
                    &chunk.to_string(),
                    "--format",
                    "json",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}: {}",
                file.display(),
                String::from_utf8_lossy(&output.stderr)
            );
            let mut actual: Value = serde_json::from_slice(&output.stdout).unwrap();
            strip_metadata(&mut actual);
            assert_eq!(actual, expected, "{} chunk={chunk}", file.display());
        }
    }
}
