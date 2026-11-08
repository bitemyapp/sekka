// SPDX-License-Identifier: MIT OR Apache-2.0
//! Prepared configuration edits. The client freezes a store snapshot in the
//! operation directory; the worker checks its base and writes under Yukimi's lock.
use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::OpenOptionsExt;

type Files = BTreeMap<PathBuf, Vec<u8>>;

/// Only ordinary files and directories: neither snapshots nor target paths
/// may follow a symlink outside the configuration. Bound memory and traversal.
pub fn files(root: &Path) -> Result<Files> {
    fn visit(root: &Path, relative: &Path, files: &mut Files, total: &mut u64, entries: &mut usize) -> Result<()> {
        *entries += 1;
        if relative.components().count() > 16 || *entries > 2048 {
            return Err(Failure("Configuration has too many files or directories".into()));
        }
        let path = root.join(relative);
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                visit(root, &relative.join(entry?.file_name()), files, total, entries)?;
            }
        } else if metadata.is_file() {
            let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&path)?;
            let mut bytes = Vec::new();
            file.take(LARGEST_KEPT + 1).read_to_end(&mut bytes)?;
            *total += bytes.len() as u64;
            if bytes.len() as u64 > LARGEST_KEPT || *total > 16 * LARGEST_KEPT {
                return Err(Failure(format!("Configuration file is too large: {}", path.display())));
            }
            files.insert(relative.to_owned(), bytes);
        } else {
            return Err(Failure(format!("Configuration must contain regular files, not links: {}", path.display())));
        }
        Ok(())
    }
    let mut found = Files::new();
    visit(root, Path::new(""), &mut found, &mut 0, &mut 0)?;
    Ok(found)
}

fn writable(name: &Path) -> bool {
    name.extension().is_some_and(|ext| ext == "nix")
        && name.components().all(|part| matches!(part, std::path::Component::Normal(_)))
        && name.to_str().is_some_and(|name| {
            !name.split('/').any(|part| part.starts_with('.'))
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
        })
        && !matches!(name.to_str(), Some("flake.nix" | "yukimi.nix" | "hardware-configuration.nix"))
}

fn snapshots(path: &Path) -> Result<(Files, Files)> {
    let before = files(&path.join("before"))?;
    let after = files(&path.join("after"))?;
    if !before.contains_key(Path::new("flake.nix")) || !before.contains_key(Path::new("configuration.nix")) {
        return Err(Failure("A prepared configuration needs flake.nix and configuration.nix".into()));
    }
    for name in before.keys().chain(after.keys()) {
        if before.get(name) != after.get(name) && (!writable(name) || !after.contains_key(name)) {
            return Err(Failure(format!("Cannot replace or remove {} in a prepared configuration", name.display())));
        }
    }
    Ok((before, after))
}

/// Copy before detaching the worker, so closing the agent cannot remove its
/// inputs. Only a top-level immutable store path is accepted from the client.
pub fn capture(args: &[String], directory: &Path) -> Result<Vec<String>> {
    let mut args = args.to_vec();
    if args.first().is_some_and(|s| s == "change") {
        let change = Change::parse(&args[1..])?;
        if let Some(path) = change.configuration {
            if path.parent() != Some(Path::new("/nix/store")) || std::fs::canonicalize(&path)? != path {
                return Err(Failure("--configuration needs an immutable snapshot in /nix/store".into()));
            }
            let (before, after) = snapshots(&path)?;
            let captured = directory.join("configuration");
            for (part, files) in [("before", before), ("after", after)] {
                for (name, bytes) in files {
                    let target = captured.join(part).join(name);
                    std::fs::create_dir_all(target.parent().unwrap())?;
                    std::fs::write(target, bytes)?;
                }
            }
            let index = args.iter().position(|a| a == "--configuration").unwrap();
            args[index + 1] = captured.to_string_lossy().into_owned();
        }
    }
    Ok(args)
}

pub struct Plan {
    pub edits: Vec<(PathBuf, Vec<u8>)>,
}

impl Plan {
    /// Called only after the worker acquired the system-operation lock.
    pub fn prepare(path: &Path, current: &Path) -> Result<Self> {
        let (before, after) = snapshots(path)?;
        if files(current)? != before {
            return Err(Failure("The configuration changed after preparation; review the current files and prepare again. Nothing was changed.".into()));
        }
        Ok(Self {
            edits: after
                .into_iter()
                .filter(|(name, bytes)| before.get(name) != Some(bytes))
                .map(|(name, bytes)| (current.join(name), bytes))
                .collect(),
        })
    }

    pub fn install(&self, setup: &Setup) -> Result<()> {
        for (path, bytes) in &self.edits {
            check_stopped()?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            write_file(setup, path, std::str::from_utf8(bytes)?)?;
        }
        Ok(())
    }
}
