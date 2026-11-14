// SPDX-License-Identifier: MIT OR Apache-2.0
//! Evaluate an update and ask Nix what it would build, without changing the
//! installed configuration or building packages. A missing substitute is a
//! warning, not evidence that a package is incompatible.
use std::collections::BTreeSet;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use yukimi_config::{branch, edit, sekka};

use crate::{
    nix,
    setup::{Kind, Setup},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub inputs: Vec<String>,
    pub sekka: bool,
    pub follow: Option<(String, String)>,
}

/// The lock is immutable. The source identity includes the fetched tree and
/// its revision metadata; root independently verifies it and the lock changes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Report {
    pub source: String,
    pub lock: PathBuf,
    pub builds: Vec<String>,
}

impl Report {
    pub fn warning(&self) -> Option<String> {
        if self.builds.is_empty() {
            return None;
        }
        let mut names = self.builds.iter().take(8).map(|name| format!("• {name}")).collect::<Vec<_>>();
        if self.builds.len() > names.len() {
            names.push(format!("…and {} more", self.builds.len() - names.len()));
        }
        Some(format!(
            "These packages need local builds for this update:\n\n{}\n\n\
             Yukimi checked the proposed configuration, but has not built these packages. \
             They may fail to compile, which would stop the update. A missing download does not \
             mean a package is incompatible.\n\nContinue with the update and build them?",
            names.join("\n")
        ))
    }
}

fn command() -> Command {
    let mut c = nix::command();
    // Evaluation must not start an import-from-derivation build. Plain C
    // diagnostics also keep Nix's dry-run headings stable across locales.
    c.args(["--option", "allow-import-from-derivation", "false", "--log-format", "raw"]);
    c.env("LC_ALL", "C");
    c
}

fn output(c: Command, run: &mut impl FnMut(Command) -> Result<Output, String>) -> Result<Output, String> {
    let out = run(c)?;
    if !out.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&out.stderr));
        return Err(nix::last_error(&stderr));
    }
    Ok(out)
}

#[derive(Deserialize, Serialize)]
struct Snapshot {
    root: PathBuf,
    flake: PathBuf,
    info: serde_json::Value,
}

fn snapshot(flake: &Path, run: &mut impl FnMut(Command) -> Result<Output, String>) -> Result<(Snapshot, bool), String> {
    // Resolve the CLI installable first: getFlake on a bare absolute path
    // uses the path fetcher, even when the updater sees a Git worktree.
    let mut c = command();
    c.args(["flake", "metadata", "--json", "--no-update-lock-file"]).arg(flake);
    let metadata: serde_json::Value = serde_json::from_slice(&output(c, run)?.stdout).map_err(|e| e.to_string())?;
    let url = metadata["resolvedUrl"].as_str().ok_or("Nix did not resolve the configuration")?;
    let git = metadata["resolved"]["type"] == "git";
    let expression = format!(
        "let f = builtins.getFlake {}; in {{ root = f.sourceInfo.outPath; flake = f.outPath; \
         info = builtins.removeAttrs f.sourceInfo [ \"outPath\" ]; }}",
        edit::nix_string(url)
    );
    let mut c = command();
    c.args(["eval", "--json", "--impure", "--no-update-lock-file", "--expr", &expression]);
    let snapshot: Snapshot = serde_json::from_slice(&output(c, run)?.stdout).map_err(|e| e.to_string())?;
    if !snapshot.root.starts_with("/nix/store")
        || !snapshot.root.is_dir()
        || !snapshot.flake.starts_with(&snapshot.root)
    {
        return Err("Nix did not return a configuration snapshot".into());
    }
    Ok((snapshot, git))
}

fn git_command() -> Command {
    let mut c = Command::new("git");
    // The root validator may read a configuration owned by the caller. No
    // hooks, checkout or original index writes are needed for the private copy.
    c.args(["-c", "safe.directory=*", "-c", "core.hooksPath=/dev/null"]);
    for key in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        c.env_remove(key);
    }
    c.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_TERMINAL_PROMPT", "0");
    c
}

/// The same authorized flake transformation used by the updater.
pub fn prepare_flake(text: &str, request: &Request) -> Result<String, String> {
    let mut text = if request.sekka { sekka::prepare(text)? } else { text.to_owned() };
    if let Some((input, followed)) = &request.follow {
        let path = ["inputs", input.as_str(), "url"];
        let url = edit::string_value(&text, &path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("flake.nix doesn't give {input}'s address as inputs.{input}.url"))?;
        let url = if input == "sekka" && followed == "main" {
            sekka::EARLY.to_owned()
        } else {
            branch::with(&url, followed).ok_or_else(|| format!("{input} cannot follow {followed}"))?
        };
        text = edit::set_string_value(&text, &path, &url).map_err(|e| e.to_string())?;
    }
    Ok(text)
}

/// A private, disposable copy of the source Nix actually sees (including
/// Git's tracked-file selection). It is never used as root's build source.
pub struct Candidate {
    root: PathBuf,
    pub dir: PathBuf,
    pub source: String,
    pub inputs: Vec<String>,
}

impl Drop for Candidate {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Candidate {
    pub fn prepare(
        setup: &Setup,
        request: &Request,
        run: &mut impl FnMut(Command) -> Result<Output, String>,
    ) -> Result<Self, String> {
        if setup.kind != Kind::Flake {
            return Err("Compatibility checks need a flake configuration".into());
        }
        let (snapshot, git) = snapshot(&setup.dir, run)?;
        let source = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "yukimi-preflight-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&root).map_err(|e| e.to_string())?;
        let dir = root.join(snapshot.flake.strip_prefix(&snapshot.root).map_err(|e| e.to_string())?);
        let mut candidate = Self { root, dir, source, inputs: request.inputs.clone() };
        if git {
            let mut c = git_command();
            c.arg("-C").arg(&setup.dir).args(["rev-parse", "--show-toplevel"]);
            let original = String::from_utf8(output(c, run)?.stdout).map_err(|e| e.to_string())?;
            let mut c = git_command();
            // Borrow objects read-only, but keep an independent HEAD, index,
            // configuration and refs. This also supports linked worktrees and
            // shallow clones without copying an unbounded repository history.
            c.args(["clone", "--shared", "--no-checkout", "--template=", "--"])
                .arg(original.trim_end_matches('\n'))
                .arg(&candidate.root);
            output(c, run)?;
            if let Some(revision) = snapshot.info["rev"].as_str().or_else(|| snapshot.info["dirtyRev"].as_str()) {
                let revision = revision.strip_suffix("-dirty").unwrap_or(revision);
                if ![40, 64].contains(&revision.len()) || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("Nix returned an invalid Git revision".into());
                }
                let mut c = git_command();
                c.arg("-C").arg(&candidate.root).args(["reset", "--mixed", revision, "--"]);
                output(c, run)?;
            }
        }
        copy(&snapshot.root, &candidate.root, &mut 0, &mut 0).map_err(|e| e.to_string())?;
        if git {
            // Include additions already selected by Nix, even when ignored by
            // .gitignore, and retain unstaged edits/deletions from the snapshot.
            let mut c = git_command();
            c.arg("-C").arg(&candidate.root).args(["add", "--intent-to-add", "--force", "--all", "--", "."]);
            output(c, run)?;
            let (copied, _) = self::snapshot(&candidate.dir, run)?;
            if serde_json::to_string(&copied).map_err(|e| e.to_string())? != candidate.source {
                return Err("The Git configuration changed while preparing the compatibility check; check again".into());
            }
        }
        // Never write through a copied symlink back into the real
        // configuration or another location outside this private directory.
        for name in ["flake.nix", "flake.lock"] {
            if !std::fs::symlink_metadata(candidate.dir.join(name)).map_err(|e| e.to_string())?.is_file() {
                return Err(format!("Cannot check an update with a linked {name}"));
            }
        }
        let file = candidate.dir.join("flake.nix");
        let text = prepare_flake(&std::fs::read_to_string(&file).map_err(|e| e.to_string())?, request)?;
        std::fs::write(file, &text).map_err(|e| e.to_string())?;
        if request.sekka {
            let following = sekka::following(&text);
            candidate.inputs.retain(|input| !following.contains(&input.as_str()));
            if !candidate.inputs.iter().any(|input| input == "sekka") {
                candidate.inputs.push("sekka".into());
            }
        }
        if let Some((input, _)) = &request.follow
            && !candidate.inputs.contains(input)
        {
            candidate.inputs.push(input.clone());
        }
        if candidate.inputs.is_empty() {
            return Err("No update inputs were selected".into());
        }
        // Normalize migration/branch changes without refreshing unrelated
        // inputs. Root repeats this independently before accepting a lock.
        let mut c = command();
        c.args(["flake", "lock"]).arg(&candidate.dir);
        output(c, run)?;
        Ok(candidate)
    }
}

fn copy(from: &Path, to: &Path, count: &mut usize, bytes: &mut u64) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        *count += 1;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if *count > 20_000 {
            return Err(std::io::Error::other("Configuration has too many files to check"));
        }
        if kind.is_dir() {
            std::fs::DirBuilder::new().mode(0o700).create(&target)?;
            copy(&entry.path(), &target, count, bytes)?;
        } else if kind.is_file() {
            *bytes += entry.metadata()?.len();
            if *bytes > 128 * 1024 * 1024 {
                return Err(std::io::Error::other("Configuration is too large to check"));
            }
            std::fs::copy(entry.path(), &target)?;
            let executable = entry.metadata()?.permissions().mode() & 0o111 != 0;
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }))?;
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, target)?;
        } else {
            return Err(std::io::Error::other("Configuration contains a special file"));
        }
    }
    Ok(())
}

/// Nix's JSON build result lists requested outputs, not the complete build
/// plan. Read only its explicit dry-run build section, never warning paths
/// or cached downloads. Refuse malformed paths in the build section.
pub fn planned_builds(stderr: &str) -> Result<Vec<String>, String> {
    let mut builds = BTreeSet::new();
    let mut in_builds = false;
    for line in crate::log::strip_ansi(stderr).lines() {
        let line = line.trim();
        if line.ends_with("will be built:") {
            in_builds = true;
        } else if in_builds && line.starts_with("/nix/store/") && line.ends_with(".drv") {
            builds.insert(line.to_owned());
        } else if !line.is_empty() {
            if in_builds && line.starts_with("/nix/store/") {
                return Err("Nix's proposed build list could not be read".into());
            }
            in_builds = false;
        }
    }
    Ok(builds.into_iter().collect())
}

/// Small generated configuration derivations and fixed-output downloads are
/// expected. Warn about other uncached derivations; this deliberately errs
/// toward review and does not attempt to predict compiler errors.
pub fn build_names(value: &serde_json::Value) -> Result<Vec<String>, String> {
    let derivations =
        value.get("derivations").unwrap_or(value).as_object().ok_or("Nix's derivations could not be read")?;
    let mut names = BTreeSet::new();
    for (path, info) in derivations {
        let env = info["env"].as_object().ok_or("Nix's derivation settings could not be read")?;
        let structured: serde_json::Value = env
            .get("__json")
            .and_then(|s| s.as_str())
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| format!("Invalid derivation settings: {e}"))?
            .unwrap_or(serde_json::Value::Null);
        let setting = |name: &str| structured.get(name).or_else(|| env.get(name));
        let yes = |v: Option<&serde_json::Value>| v.is_some_and(|v| v == "1" || v == true);
        let fixed = info["outputs"]
            .as_object()
            .ok_or("Nix's derivation outputs could not be read")?
            .values()
            .any(|out| out.get("hash").is_some());
        if fixed || yes(setting("preferLocalBuild")) {
            continue;
        }
        let name = setting("name")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or_else(|| path.rsplit('/').next()?.get(33..)?.strip_suffix(".drv").map(str::to_owned))
            .ok_or("Nix's package name could not be read")?;
        names.insert(name);
    }
    Ok(names.into_iter().collect())
}

pub fn check(
    setup: &Setup,
    request: &Request,
    run: &mut impl FnMut(Command) -> Result<Output, String>,
) -> Result<Report, String> {
    let candidate = Candidate::prepare(setup, request, run)?;
    let mut c = command();
    c.args(["flake", "update", "--refresh", "--flake"]).arg(&candidate.dir).args(&candidate.inputs);
    output(c, run)?;
    // Match the updater: a flake with exactly one system may use a name
    // different from the machine's hostname.
    let mut c = command();
    c.args(["eval", "--json", "--no-update-lock-file", "--apply", "builtins.attrNames"])
        .arg(format!("{}#nixosConfigurations", candidate.dir.display()));
    let names: Vec<String> = serde_json::from_slice(&output(c, run)?.stdout).map_err(|e| e.to_string())?;
    let host = if names.contains(&setup.host) {
        &setup.host
    } else if names.len() == 1 {
        &names[0]
    } else {
        return Err(format!("The flake has no configuration named {:?} (it has {})", setup.host, names.join(", ")));
    };
    let host = edit::nix_string(host);
    let mut c = command();
    c.args(["build", "--dry-run", "--json", "--no-link", "--no-update-lock-file"])
        .arg(format!("{}#nixosConfigurations.{host}.config.system.build.toplevel", candidate.dir.display()));
    let out = output(c, run)?;
    // Malformed/empty stdout must never look like a completed check.
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    if !result.as_array().is_some_and(|a| a.len() == 1 && a[0]["drvPath"].as_str().is_some()) {
        return Err("Nix did not return a proposed system".into());
    }
    let drvs = planned_builds(&String::from_utf8_lossy(&out.stderr))?;
    let downloads = String::from_utf8_lossy(&out.stderr).contains("will be fetched");
    let present = result[0]["outputs"].as_object().is_some_and(|outputs| {
        !outputs.is_empty() && outputs.values().all(|path| path.as_str().is_some_and(|path| Path::new(path).exists()))
    });
    if drvs.is_empty() && !downloads && !present {
        return Err("Nix did not report whether the proposed system needs building".into());
    }
    let mut builds = Vec::new();
    for batch in drvs.chunks(100) {
        let mut c = command();
        c.args(["derivation", "show"]).args(batch);
        let out = output(c, run)?;
        builds.extend(build_names(&serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?)?);
    }
    builds.sort();
    builds.dedup();
    let mut c = command();
    c.args(["store", "add-path"]).arg(candidate.dir.join("flake.lock"));
    let lock = String::from_utf8(output(c, run)?.stdout).map_err(|e| e.to_string())?;
    let lock = PathBuf::from(lock.trim());
    if !lock.starts_with("/nix/store") || !lock.is_file() {
        return Err("Nix did not keep the checked lock".into());
    }
    Ok(Report { source: candidate.source.clone(), lock, builds })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dry_run_separates_builds_from_downloads_and_warning_paths() {
        let cargo = "/nix/store/0123456789abcdfghijklmnpqrsvwxyzab-cargo-geiger.drv";
        let output = format!(
            "warning: unrelated /nix/store/example.drv\nthese 2 derivations will be built:\n  {cargo}\n  {cargo}\nthese 2 paths will be fetched (1 MiB download):\n  /nix/store/download\n"
        );
        assert_eq!(planned_builds(&output).unwrap(), [cargo]);
        assert!(planned_builds("this derivation will be built:\n /nix/store/invalid\n").is_err());
    }

    #[test]
    fn warns_about_uncached_packages_not_configuration_or_source_downloads() {
        let drvs = json!({"derivations": {
            "cargo.drv": {"env": {"name": "cargo-geiger-0.13.0"}, "outputs": {"out": {"path":"out"}}},
            "config.drv": {"env": {"name": "etc", "preferLocalBuild":"1"}, "outputs": {"out":{}}},
            "download.drv": {"env": {"name":"source"}, "outputs": {"out":{"hash":"sha256-x"}}},
            "structured.drv": {"env": {"__json":"{\"name\":\"system-path\",\"preferLocalBuild\":true}"}, "outputs":{"out":{}}}
        }});
        assert_eq!(build_names(&drvs).unwrap(), ["cargo-geiger-0.13.0"]);
        assert!(build_names(&json!({"broken.drv": {}})).is_err());
    }

    #[test]
    fn source_build_warning_does_not_claim_incompatibility() {
        let mut report = Report { source: "/nix/store/source".into(), lock: "/nix/store/lock".into(), builds: vec![] };
        assert!(report.warning().is_none());
        report.builds.push("cargo-geiger-0.13.0".into());
        let warning = report.warning().unwrap();
        assert!(warning.contains("cargo-geiger-0.13.0"));
        assert!(warning.contains("has not built"));
        assert!(warning.contains("does not mean a package is incompatible"));
    }

    #[test]
    fn prepared_branch_is_the_authorized_one() {
        let flake = r#"{ inputs.sekka.url = "github:bitemyapp/sekka/nightly"; }"#;
        let request = Request { sekka: true, follow: Some(("sekka".into(), "main".into())), ..Default::default() };
        let text = prepare_flake(flake, &request).unwrap();
        assert_eq!(edit::string_value(&text, &["inputs", "sekka", "url"]).unwrap().as_deref(), Some(sekka::EARLY));
    }
}
