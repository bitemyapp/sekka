use anyhow::{Result, bail, ensure};
use serde_json::Value;
use std::{collections::BTreeSet, fs};

fn settings(text: &str, kernel: &str) -> Result<()> {
    let value: Value = serde_json::from_str(text)?;
    ensure!(value["kernel"] == kernel, "Incorrect kernel choice");
    ensure!(
        value["test_diagnostics"] == false,
        "Test diagnostics shipped enabled"
    );
    ensure!(
        value["template_dir"] == "/etc/calamares-nixos",
        "Wrong template path"
    );
    ensure!(
        value["zoneinfo"]
            .as_str()
            .is_some_and(|s| s.starts_with("/nix/store/") && s.ends_with("/share/zoneinfo")),
        "Unpinned timezone database"
    );
    ensure!(
        matches!(value["state_version"].as_str(), Some("26.05" | "26.11")),
        "Unsupported state version"
    );
    Ok(())
}

/// Inputs of the installed flake, which extends the external dependency lock with Sekka.
const TARGET_INPUTS: [&str; 7] = [
    "nixpkgs",
    "determinate",
    "fh",
    "applications",
    "ai-apps",
    "omp",
    "sekka",
];

fn template(source: &str, lock: &str) -> Result<()> {
    ensure!(
        source.matches("@HOSTNAME@").count() == 1,
        "Expected one hostname placeholder"
    );
    let value: Value = serde_json::from_str(lock)?;
    ensure!(value["version"] == 7, "Unsupported lock version");
    let root = value["root"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing lock root"))?;
    let inputs = value["nodes"][root]["inputs"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Missing inputs"))?;
    ensure!(
        inputs.keys().map(String::as_str).collect::<BTreeSet<_>>() == BTreeSet::from(TARGET_INPUTS),
        "Unexpected target inputs"
    );
    for name in TARGET_INPUTS {
        let node = inputs[name]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Expected direct locked input"))?;
        ensure!(
            value["nodes"][node]["locked"]["narHash"]
                .as_str()
                .is_some_and(|s| s.starts_with("sha256-")),
            "Unlocked input: {name}"
        );
    }
    Ok(())
}
pub fn main(args: Vec<String>) -> Result<()> {
    ensure!(
        args.len() == 3,
        "Usage: check_config.rs template|settings FILE FILE"
    );
    match args[0].as_str() {
        "template" => template(
            &fs::read_to_string(&args[1])?,
            &fs::read_to_string(&args[2])?,
        )?,
        "settings" => {
            settings(&fs::read_to_string(&args[1])?, "lts")?;
            settings(&fs::read_to_string(&args[2])?, "latest")?;
        }
        _ => bail!("Unknown check"),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_test_diagnostics_and_unpinned_zones() {
        let good = serde_json::json!({"kernel":"lts","test_diagnostics":false,"template_dir":"/etc/calamares-nixos","zoneinfo":"/nix/store/test-tzdata/share/zoneinfo","state_version":"26.11"});
        assert!(settings(&good.to_string(), "lts").is_ok());
        assert!(settings(&good.to_string(), "latest").is_err());
        for (key, replacement) in [
            ("test_diagnostics", Value::Bool(true)),
            ("zoneinfo", Value::String("/tmp/zones".into())),
        ] {
            let mut bad = good.clone();
            bad[key] = replacement;
            assert!(settings(&bad.to_string(), "lts").is_err());
        }
    }
    #[test]
    fn requires_locked_template() {
        assert!(template("missing", "{}").is_err());
        assert!(template("@HOSTNAME@ @HOSTNAME@", "{}").is_err());
        assert!(
            template(
                "@HOSTNAME@",
                r#"{"version":7,"root":"root","nodes":{"root":{"inputs":{}}}}"#
            )
            .is_err()
        );
    }
}
