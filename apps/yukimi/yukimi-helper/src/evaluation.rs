// SPDX-License-Identifier: MIT OR Apache-2.0
//! One finalized configuration and derivation for the entire transaction.
use super::*;

/// The binary caches and their keys a configuration names, if any.
pub type Caches = Option<(Vec<String>, Vec<String>)>;

/// The last snapshot's system and the caches its configuration names.
const KNOWN_CACHES: &str = "/var/lib/yukimi/configured-caches.json";

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
        // flake inside a larger repository (reached through a symlink, say),
        // and decide from what it selected. With lazy trees this copies
        // nothing, so a Git work tree is never copied only to be set aside.
        if !snapshot(&metadata(setup, &[])?) {
            return Ok(setup.flake());
        }
        // Copy the plain directory. Do not use the deliberately lossy
        // generation-backup copier as a build snapshot.
        let copied = metadata(setup, &["--option", "lazy-trees", "false"])?;
        let source =
            copied["path"].as_str().ok_or_else(|| Failure("Nix did not return a configuration snapshot".into()))?;
        if !snapshot(&copied) || !store_path(source) {
            return Err(Failure("Nix returned an invalid configuration snapshot".into()));
        }
        // Keep the snapshot available for each command that reads nixConfig,
        // even when the system derivation does not reference the source tree.
        let mut keep = command("nix-store");
        keep.arg("--add-root").arg(self.source_root()).args(["--indirect", "--realise", source]);
        output(keep, "Keeping the configuration snapshot")?;
        Ok(format!("path:{source}"))
    }

    /// The binary caches a snapshot's configuration names, as found by an
    /// earlier operation. `nix eval` never uses Nix's evaluation cache, so
    /// reading them costs most of an evaluation; an immutable snapshot,
    /// evaluated purely, always names the same ones.
    pub fn known_caches(&self, system: &str) -> Option<Caches> {
        if !system.starts_with("path:/nix/store/") {
            return None;
        }
        let text = std::fs::read_to_string(KNOWN_CACHES).ok()?;
        let (known, caches): (String, Caches) = serde_json::from_str(&text).ok()?;
        (known == system).then_some(caches)
    }

    /// Remember what a snapshot's configuration names, for the next retry.
    /// Losing this only costs that evaluation again.
    pub fn remember_caches(&self, system: &str, caches: &Caches) {
        if !system.starts_with("path:/nix/store/") {
            return;
        }
        let pending = Path::new(KNOWN_CACHES).with_extension("new");
        if let Ok(json) = serde_json::to_vec(&(system, caches))
            && std::fs::write(&pending, json).is_ok()
        {
            let _ = std::fs::rename(&pending, KNOWN_CACHES);
        }
    }

    pub fn keep_derivation(&self, derivation: &str) -> Result<()> {
        if !store_path(derivation) || !derivation.ends_with(".drv") {
            return Err(Failure("Nix did not return one system derivation".into()));
        }
        // Without ^out, `nix build` roots the opaque .drv path itself; it
        // does not build the system. Keep the source root separately.
        let mut keep = nix();
        keep.args(["build", "--out-link"]).arg(self.directory.join("derivation-root")).arg(derivation);
        output(keep, "Keeping the system derivation").map(|_| ())
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // A crash leaves roots only for the existing operation-log retention
        // period. Normal success, failure and cancellation release them here,
        // after the profile/activation has taken ownership of the new system.
        for name in ["evaluation-root", "derivation-root", "system-root"] {
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

fn metadata(setup: &Setup, options: &[&str]) -> Result<serde_json::Value> {
    let mut metadata = nix();
    metadata.args(["flake", "metadata", "--json", "--no-update-lock-file"]).args(options).arg(setup.flake());
    Ok(serde_json::from_str(&output(metadata, "Snapshotting the configuration")?)?)
}

/// Whether the flake Nix selected is a plain directory without local
/// inputs, whose store copy can stand in for it.
fn snapshot(metadata: &serde_json::Value) -> bool {
    let local_inputs = metadata["locks"]["nodes"].as_object().is_some_and(|nodes| {
        nodes.values().any(|node| node["locked"]["type"] == "path" || node.get("parent").is_some())
    });
    // Git already has a stable evaluator fingerprint. Keep its self.rev,
    // dirtyRev and subdirectory semantics: its source is the whole work
    // tree, not the flake's directory. Relative/mutable path inputs must
    // also keep their original resolution base.
    metadata["locked"]["type"] == "path" && metadata["locked"].get("dir").is_none() && !local_inputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_keeps_git_metadata_and_local_input_resolution() {
        let directory = serde_json::json!({"type": "path", "path": "/home/example/config"});
        let nodes = serde_json::json!({"root": {}, "nixpkgs": {"locked": {"type": "github"}}});
        assert!(snapshot(&serde_json::json!({"locked": directory, "locks": {"nodes": nodes}})));
        // Nix decides, not a search for .git: a symlink into a repository
        // makes a Git flake in a subdirectory, whose source is the whole tree.
        for locked in [
            serde_json::json!({"type": "git", "url": "file:///home/example/dotfiles", "dir": "nixos"}),
            serde_json::json!({"type": "git", "url": "file:///home/example/config"}),
            serde_json::json!({"type": "path", "path": "/home/example", "dir": "config"}),
        ] {
            assert!(!snapshot(&serde_json::json!({"locked": locked, "locks": {"nodes": nodes}})));
        }
        for node in [
            serde_json::json!({"locked": {"type": "path", "path": "../local"}}),
            serde_json::json!({"parent": ["root"]}),
        ] {
            let metadata = serde_json::json!({"locked": directory, "locks": {"nodes": {"local": node}}});
            assert!(!snapshot(&metadata));
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
