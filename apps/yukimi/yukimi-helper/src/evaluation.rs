// SPDX-License-Identifier: MIT OR Apache-2.0
//! One finalized configuration and derivation for the entire transaction.
use super::*;

pub struct Context {
    directory: PathBuf,
    cores: Option<u32>,
}

impl Context {
    pub fn new(directory: &Path, cores: Option<u32>) -> Self {
        Self { directory: directory.to_owned(), cores }
    }

    pub fn nix(&self) -> Command {
        let mut command = nix();
        if let Some(cores) = self.cores {
            command.args(["--option", "eval-cores", &cores.to_string()]);
        }
        command
    }

    fn source_root(&self) -> PathBuf {
        self.directory.join("evaluation-root")
    }

    pub fn result_root(&self) -> PathBuf {
        self.directory.join("system-root")
    }

    pub fn flake(&self, setup: &Setup) -> Result<String> {
        // Configuration edits can introduce inputs, and a standalone flake
        // may have no lock yet. Finalize it before copying the source, just
        // as the old build's implicit locking did, under the same backup.
        let mut lock = nix();
        lock.args(["flake", "lock"]).arg(setup.flake());
        output(lock, "Finalizing the configuration inputs")?;
        // Let Nix select the source, including Git tracking rules and a
        // flake inside a larger repository. Do not use the deliberately
        // lossy generation-backup copier as a build snapshot.
        let mut metadata = nix();
        metadata.args(["flake", "metadata", "--json", "--no-update-lock-file", "--option", "lazy-trees", "false"]);
        metadata.arg(setup.flake());
        let metadata: serde_json::Value = serde_json::from_str(&output(metadata, "Snapshotting the configuration")?)?;
        let source =
            metadata["path"].as_str().ok_or_else(|| Failure("Nix did not return a configuration snapshot".into()))?;
        if !store_path(source) {
            return Err(Failure("Nix returned an invalid configuration snapshot".into()));
        }
        // Register an indirect GC root for the snapshot. After evaluation,
        // the same root protects its derivation until activation finishes.
        let mut keep = command("nix-store");
        keep.arg("--add-root").arg(self.source_root()).args(["--indirect", "--realise", source]);
        output(keep, "Keeping the configuration snapshot")?;
        Ok(reference(&metadata, &setup.flake(), setup.git, source))
    }

    pub fn keep_derivation(&self, derivation: &str) -> Result<()> {
        if !store_path(derivation) || !derivation.ends_with(".drv") {
            return Err(Failure("Nix did not return one system derivation".into()));
        }
        let pending = self.directory.join("evaluation-root.new");
        std::os::unix::fs::symlink(derivation, &pending)?;
        std::fs::rename(pending, self.source_root())?;
        Ok(())
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // A crash leaves roots only for the existing operation-log retention
        // period. Normal success, failure and cancellation release them here,
        // after the profile/activation has taken ownership of the new system.
        for name in ["evaluation-root", "evaluation-root.new", "system-root"] {
            let _ = std::fs::remove_file(self.directory.join(name));
        }
    }
}

fn store_path(path: &str) -> bool {
    let path = Path::new(path);
    path.parent() == Some(Path::new("/nix/store"))
        && path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
            s.len() > 33
                && s.as_bytes()[32] == b'-'
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"+-._?=".contains(&b))
        })
}

fn reference(metadata: &serde_json::Value, original: &str, git: bool, source: &str) -> String {
    let local_inputs = metadata["locks"]["nodes"].as_object().is_some_and(|nodes| {
        nodes.values().any(|node| node["locked"]["type"] == "path" || node.get("parent").is_some())
    });
    if git || local_inputs {
        // Git already has a stable evaluator fingerprint. Keep its self.rev,
        // dirtyRev and subdirectory semantics. Relative/mutable path inputs
        // must also keep their original resolution base.
        original.to_owned()
    } else {
        format!("path:{source}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_keeps_git_metadata_and_local_input_resolution() {
        let source = "/nix/store/00000000000000000000000000000000-source";
        let original = "/home/example/config";
        let locked = serde_json::json!({"locks": {"nodes": {"root": {}, "nixpkgs": {"locked": {"type": "github"}}}}});
        assert_eq!(reference(&locked, original, false, source), format!("path:{source}"));
        assert_eq!(reference(&locked, original, true, source), original);
        for node in [
            serde_json::json!({"locked": {"type": "path", "path": "../local"}}),
            serde_json::json!({"parent": ["root"]}),
        ] {
            let metadata = serde_json::json!({"locks": {"nodes": {"local": node}}});
            assert_eq!(reference(&metadata, original, false, source), original);
        }
    }

    #[test]
    fn accepts_only_one_direct_store_path() {
        assert!(store_path("/nix/store/00000000000000000000000000000000-system.drv"));
        for bad in [
            "",
            "/tmp/system.drv",
            "/nix/store/short.drv",
            "/nix/store/00000000000000000000000000000000-x/bin",
            "/nix/store/00000000000000000000000000000000-x\n/nix/store/other",
        ] {
            assert!(!store_path(bad), "{bad}");
        }
    }
}
