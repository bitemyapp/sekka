// SPDX-License-Identifier: MIT OR Apache-2.0
//! A change to the system as a transaction: take a restore point, change
//! only what it covers, build, then commit, or put everything back.
//!
//! [`RestorePoint::attempt`] is the only way through. It runs the change
//! and then either commits it or restores the point (a panic before the
//! commit restores it too), so a change can't end with neither. What the
//! change alters must be covered first: the configuration's files when the
//! point is taken, root's channels when the change is about to update them
//! ([`Changing::cover_channels`]), which is the only way to update them.
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::ErrorKind::NotFound;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use yukimi_system::channels::ROOT_CHANNELS;
use yukimi_system::generations::generation_number;
use yukimi_system::setup::Setup;

use crate::process::{self, Failure, Result};

/// Everything a change may alter, as it was before.
#[must_use = "a restore point does nothing until a change is attempted from it"]
pub struct RestorePoint {
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
    channels: Option<Channels>,
    /// Root's channels profile; elsewhere only in tests.
    channels_profile: PathBuf,
    /// Committed or put back: nothing is left to do when it's dropped.
    settled: bool,
}

impl RestorePoint {
    /// Covers `files` as they are now. One that doesn't exist yet is
    /// removed again when the point is restored.
    pub fn take(files: &[PathBuf]) -> Result<RestorePoint> {
        let mut covered = Vec::new();
        for path in files {
            let contents = match std::fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == NotFound => None,
                // Covered as absent, it would be deleted when restoring.
                Err(e) => return Err(Failure(format!("{} couldn't be read: {e}", path.display()))),
            };
            covered.push((path.clone(), contents));
        }
        let mut point = RestorePoint::nothing();
        point.files = covered;
        Ok(point)
    }

    /// Covers nothing, for a step that commits before it changes anything.
    pub fn nothing() -> RestorePoint {
        RestorePoint { files: Vec::new(), channels: None, channels_profile: ROOT_CHANNELS.into(), settled: false }
    }

    /// Runs `work`, the change up to its plan, then commits: from then on
    /// the work finishes even if Yukimi goes away. If `work` fails, or a
    /// stop comes before the commit, everything the point covers is put
    /// back, and the failure says what couldn't be.
    pub fn attempt<T>(mut self, work: impl FnOnce(&mut Changing<'_>) -> Result<T>) -> Result<Committed<T>> {
        let done = work(&mut Changing { point: &mut self }).and_then(|planned| {
            process::commit()?;
            Ok(planned)
        });
        self.settled = true;
        match done {
            Ok(planned) => Ok(Committed { planned }),
            Err(Failure(message)) => {
                let left = self.restore();
                Err(Failure(if left.is_empty() {
                    message
                } else {
                    format!("{message}. These couldn't be put back: {}", left.join("; "))
                }))
            }
        }
    }

    /// Puts back everything covered, without starting a program (a stop
    /// refuses to start one). Returns what couldn't be put back.
    fn restore(&self) -> Vec<String> {
        let mut left = Vec::new();
        for (path, contents) in &self.files {
            let put = match contents {
                Some(bytes) => std::fs::write(path, bytes),
                None => std::fs::remove_file(path).or_else(|e| if e.kind() == NotFound { Ok(()) } else { Err(e) }),
            };
            if let Err(e) = put {
                left.push(format!("{}: {e}", path.display()));
            }
        }
        if let Some(channels) = &self.channels
            && let Err(e) = channels.restore()
        {
            left.push(format!("root's channels ({}): {e}", channels.profile.display()));
        }
        left
    }
}

/// A change that panics before its commit still puts back what it covered.
impl Drop for RestorePoint {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.restore();
        }
    }
}

/// A change under way, inside [`RestorePoint::attempt`]: what it may
/// alter is what the restore point covers.
///
/// Root's channels are updated only through
/// [`cover_channels`](Self::cover_channels), which records them first.
/// Updating them in an attempt that covers only files, as Yukimi once did,
/// doesn't compile:
///
/// ```compile_fail,E0599
/// use yukimi_helper::change::RestorePoint;
/// # fn main() -> yukimi_helper::process::Result<()> {
/// let files = [std::path::PathBuf::from("/etc/nixos/configuration.nix")];
/// RestorePoint::take(&files)?.attempt(|changing| changing.update_channels())?;
/// # Ok(()) }
/// ```
///
/// ```no_run
/// use yukimi_helper::change::RestorePoint;
/// # fn main() -> yukimi_helper::process::Result<()> {
/// let files = [std::path::PathBuf::from("/etc/nixos/configuration.nix")];
/// RestorePoint::take(&files)?.attempt(|changing| changing.cover_channels()?.update_channels())?;
/// # Ok(()) }
/// ```
pub struct Changing<'p> {
    point: &'p mut RestorePoint,
}

impl Changing<'_> {
    fn check_covered(&self, path: &Path) -> Result<()> {
        if self.point.files.iter().any(|(covered, _)| covered == path) {
            Ok(())
        } else {
            Err(Failure(format!("{} isn't covered by the restore point", path.display())))
        }
    }

    /// Writes a covered file of the configuration. A new file belongs to
    /// whoever owns its directory, so a configuration kept in someone's
    /// home stays theirs, and in a Git work tree, Git is told about it (as
    /// intended to be added), or a flake there wouldn't see it.
    pub fn write_file(&mut self, setup: &Setup, path: &Path, contents: &str) -> Result<()> {
        self.check_covered(path)?;
        let new = !path.exists();
        std::fs::write(path, contents)?;
        if !new {
            return Ok(());
        }
        if let Some(meta) = path.parent().and_then(|dir| std::fs::metadata(dir).ok()) {
            use std::os::unix::fs::MetadataExt;
            let _ = std::os::unix::fs::chown(path, Some(meta.uid()), Some(meta.gid()));
        }
        if setup.git {
            let mut git = process::command("git");
            // Root works in someone else's work tree here, which Git refuses
            // unless told it is safe.
            git.args(["-c", "safe.directory=*", "-C"]).arg(path.parent().unwrap_or(&setup.dir));
            git.args(["add", "--intent-to-add", "--"]).arg(path);
            if !process::wait(git.stdout(Stdio::null()).stderr(Stdio::null()))?.status.success() {
                return Err(Failure(format!(
                    "{} is in a Git repository, and Git couldn't be told about {}",
                    setup.dir.display(),
                    path.display()
                )));
            }
        }
        Ok(())
    }

    /// Runs a program that rewrites a covered file (`nix flake update`
    /// rewrites flake.lock).
    pub fn rewrite(&mut self, path: &Path, program: Command, what: &str) -> Result<()> {
        self.check_covered(path)?;
        process::run(program, what)
    }

    /// Covers root's channels: records which generation of their profile
    /// is current, so that a failure or a stop puts it back.
    pub fn cover_channels(&mut self) -> Result<ChannelsCovered<'_>> {
        if self.point.channels.is_none() {
            self.point.channels = Some(Channels::record(&self.point.channels_profile)?);
        }
        Ok(ChannelsCovered { point: PhantomData })
    }
}

/// Root's channels, recorded in the restore point.
pub struct ChannelsCovered<'c> {
    point: PhantomData<&'c mut RestorePoint>,
}

impl ChannelsCovered<'_> {
    /// `nix-channel --update`: a new generation of root's channels profile,
    /// with each channel downloaded anew.
    pub fn update_channels(self) -> Result<()> {
        let mut c = process::command("nix-channel");
        c.arg("--update");
        process::run(c, "Updating the channels")
    }
}

/// A committed change: from here on it is finished, not put back.
#[must_use = "a committed change goes on to activation"]
pub struct Committed<T> {
    planned: T,
}

impl<T> Committed<T> {
    /// What the change planned before it committed.
    pub fn into_planned(self) -> T {
        self.planned
    }
}

/// Root's channels profile as it was: which generation it pointed at, and
/// which generations it had.
struct Channels {
    profile: PathBuf,
    current: Option<PathBuf>,
    generations: BTreeSet<OsString>,
}

impl Channels {
    fn record(profile: &Path) -> std::io::Result<Channels> {
        let current = match std::fs::read_link(profile) {
            Ok(target) => Some(target),
            Err(e) if e.kind() == NotFound => None,
            Err(e) => return Err(e),
        };
        Ok(Channels { profile: profile.to_owned(), current, generations: generations(profile)? })
    }

    /// Points the profile back at the generation it pointed at, as Nix
    /// switches generations, and deletes the generations made since.
    fn restore(&self) -> std::io::Result<()> {
        match &self.current {
            Some(target) => {
                let name = self.profile.file_name().unwrap_or_default().to_string_lossy();
                let temporary = self.profile.with_file_name(format!(".{name}.yukimi-restore"));
                let _ = std::fs::remove_file(&temporary);
                std::os::unix::fs::symlink(target, &temporary)?;
                std::fs::rename(&temporary, &self.profile)?;
            }
            None => match std::fs::remove_file(&self.profile) {
                Err(e) if e.kind() != NotFound => return Err(e),
                _ => {}
            },
        }
        for made in generations(&self.profile)?.difference(&self.generations) {
            std::fs::remove_file(self.profile.with_file_name(made))?;
        }
        Ok(())
    }
}

/// The generations of `profile`: the `<profile>-<n>-link` links beside it.
fn generations(profile: &Path) -> std::io::Result<BTreeSet<OsString>> {
    let (Some(dir), Some(name)) = (profile.parent(), profile.file_name().and_then(|n| n.to_str())) else {
        return Ok(BTreeSet::new());
    };
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(e),
    };
    let mut links = BTreeSet::new();
    for entry in entries {
        let entry = entry?;
        let file = entry.file_name();
        if file.to_str().and_then(|f| generation_number(f, name)).is_some() && entry.file_type()?.is_symlink() {
            links.insert(file);
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Dir {
            let dir = std::env::temp_dir().join(format!("yukimi-change-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("profiles")).unwrap();
            Dir(dir)
        }
        fn channels(&self) -> PathBuf {
            self.0.join("profiles/channels")
        }
        /// What `nix-channel --update` does to the profile: a new generation.
        fn update_channels(&self, n: u32) {
            symlink(format!("/nix/store/env-{n}"), self.0.join(format!("profiles/channels-{n}-link"))).unwrap();
            let _ = std::fs::remove_file(self.channels());
            symlink(format!("channels-{n}-link"), self.channels()).unwrap();
        }
        fn point(&self, files: &[PathBuf]) -> RestorePoint {
            let mut point = RestorePoint::take(files).unwrap();
            point.channels_profile = self.channels();
            point
        }
        fn links(&self) -> Vec<String> {
            let mut links: Vec<String> = std::fs::read_dir(self.0.join("profiles"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            links.sort();
            links
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_failed_change_puts_files_back_as_they_were() {
        let _phase = process::phase_for_test();
        let dir = Dir::new("files");
        let (kept, created) = (dir.0.join("kept"), dir.0.join("created"));
        std::fs::write(&kept, "before").unwrap();
        let point = dir.point(&[kept.clone(), created.clone()]);
        let setup = Setup::detect_in(&dir.0, Default::default(), "acer");
        let failed = point.attempt(|changing| -> Result<()> {
            changing.write_file(&setup, &kept, "after")?;
            changing.write_file(&setup, &created, "new")?;
            // Only covered files can be written.
            assert!(changing.write_file(&setup, &dir.0.join("elsewhere"), "x").is_err());
            Err(Failure("Building the new system failed".into()))
        });
        assert_eq!(failed.err().map(|f| f.0).as_deref(), Some("Building the new system failed"));
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "before");
        assert!(!created.exists() && !dir.0.join("elsewhere").exists());
    }

    /// YUK-09: root's channels go back with the files when the build fails
    /// or Yukimi stops the change, and stay updated once it commits.
    #[test]
    fn a_failed_or_stopped_change_puts_roots_channels_back() {
        let _phase = process::phase_for_test();
        let dir = Dir::new("channels");
        dir.update_channels(1);
        let failed = dir.point(&[]).attempt(|changing| -> Result<()> {
            changing.cover_channels()?;
            dir.update_channels(2);
            Err(Failure("Building the new system failed".into()))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read_link(dir.channels()).unwrap(), Path::new("channels-1-link"));
        assert_eq!(dir.links(), ["channels", "channels-1-link"]);

        // A stop that arrives once the work is done still comes before the commit.
        let stopped = dir.point(&[]).attempt(|changing| {
            changing.cover_channels()?;
            dir.update_channels(2);
            process::stop();
            Ok(())
        });
        assert_eq!(stopped.err().map(|f| f.0).as_deref(), Some("Stopped"));
        assert_eq!(std::fs::read_link(dir.channels()).unwrap(), Path::new("channels-1-link"));
        assert_eq!(dir.links(), ["channels", "channels-1-link"]);
        drop(_phase);

        let _phase = process::phase_for_test();
        let committed = dir.point(&[]).attempt(|changing| {
            changing.cover_channels()?;
            dir.update_channels(2);
            Ok("planned")
        });
        assert_eq!(committed.ok().map(Committed::into_planned), Some("planned"));
        assert_eq!(std::fs::read_link(dir.channels()).unwrap(), Path::new("channels-2-link"));
    }

    #[test]
    fn channels_that_were_never_updated_are_removed_again() {
        let _phase = process::phase_for_test();
        let dir = Dir::new("first-channels");
        let failed = dir.point(&[]).attempt(|changing| -> Result<()> {
            changing.cover_channels()?;
            dir.update_channels(1);
            Err(Failure("Updating the channels failed".into()))
        });
        assert!(failed.is_err());
        assert!(dir.links().is_empty(), "{:?}", dir.links());
    }

    #[test]
    fn a_change_that_panics_still_puts_back_what_it_covered() {
        let _phase = process::phase_for_test();
        let dir = Dir::new("panic");
        let file = dir.0.join("configuration.nix");
        std::fs::write(&file, "before").unwrap();
        dir.update_channels(1);
        let point = dir.point(std::slice::from_ref(&file));
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = point.attempt(|changing| -> Result<()> {
                changing.cover_channels()?;
                dir.update_channels(2);
                std::fs::write(&file, "after").unwrap();
                panic!("a bug in the change");
            });
        }));
        assert!(panicked.is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "before");
        assert_eq!(dir.links(), ["channels", "channels-1-link"]);
    }
}
