//! Kitty clipboard protocol (OSC 5522) answers recorded from the pinned Zig
//! libghostty-vt by `tools/kitty-clipboard-oracle.py`, never by the Rust
//! implementation. Running these requires no Zig library or toolchain.
use serde_json::Value;
use std::{path::PathBuf, process::Command};

#[test]
fn matches_pinned_ghostty_kitty_clipboard_answers_at_read_boundaries() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/kitty-clipboard-fixtures.json");
    let fixtures: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let cases = fixtures["cases"].as_array().unwrap();
    assert!(cases.len() > 150, "kitty clipboard fixtures missing");
    for chunk in [1, 3, 65536] {
        let output = Command::new(env!("CARGO_BIN_EXE_kitty-clipboard-replay"))
            .arg("--scenarios")
            .arg(&path)
            .args(["--chunk", &chunk.to_string()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual.as_array().unwrap().len(), cases.len());
        for (case, result) in cases.iter().zip(actual.as_array().unwrap()) {
            assert_eq!(
                result["steps"], case["expected"],
                "{} chunk={chunk}",
                case["name"]
            );
        }
    }
}
