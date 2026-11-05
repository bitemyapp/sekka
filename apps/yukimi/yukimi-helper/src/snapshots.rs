// SPDX-License-Identifier: MIT OR Apache-2.0
//! Keeping each generation's configuration, so that going back to a
//! generation brings back the choices that made it: the helper's side of
//! [`yukimi_system::saved`]. A copy is kept for a [`GenerationId`], never
//! for a bare number, since Nix gives a deleted generation's number to the
//! next one.
use std::path::{Path, PathBuf};

use yukimi_system::saved::{GenerationId, Saving, Snapshot};
use yukimi_system::setup::Setup;

use crate::process::Result;

/// Files bigger than this aren't configuration, and aren't kept with it.
pub const LARGEST_KEPT: u64 = 1 << 20;

/// Copy a configuration directory: its files and folders, not hidden ones
/// (such as a Git directory), build results or large files. Each file keeps
/// its permissions, so a file only its owner may read (in a configuration
/// kept in a home directory, say) stays that way.
pub fn copy_configuration(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.starts_with('.') || text.starts_with("result") {
            continue;
        }
        let (from, to) = (entry.path(), to.join(&name));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_configuration(&from, &to)?;
        } else if kind.is_file() && entry.metadata()?.len() <= LARGEST_KEPT {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Keeps the configuration as `generation`'s, in `root`, unless a copy
/// made for this very generation is kept already. The configuration as it
/// is belongs to the running generation; one kept for an earlier
/// generation with the same number is replaced.
///
/// Keeping it by the generation's number alone, which kept such a stale
/// copy, doesn't compile:
///
/// ```compile_fail,E0308
/// # let root = std::env::temp_dir();
/// let setup = yukimi_system::setup::Setup::detect();
/// // The running generation's number, from the profile's link name.
/// yukimi_helper::snapshots::keep(&setup, &root, 2)?;
/// # Ok::<(), yukimi_helper::process::Failure>(())
/// ```
///
/// ```
/// use yukimi_system::saved::{GenerationId, Snapshot};
/// let dir = std::env::temp_dir().join(format!("yukimi-keep-doc-{}", std::process::id()));
/// let (nixos, profiles, root) = (dir.join("nixos"), dir.join("profiles"), dir.join("configurations"));
/// std::fs::create_dir_all(&nixos).unwrap();
/// std::fs::create_dir_all(&profiles).unwrap();
/// std::fs::write(nixos.join("flake.lock"), "lock-C").unwrap();
/// std::os::unix::fs::symlink("/nix/store/s2", profiles.join("system-2-link")).unwrap();
/// std::os::unix::fs::symlink("system-2-link", profiles.join("system")).unwrap();
/// let setup = yukimi_system::setup::Setup::detect_in(&nixos, Default::default(), "acer");
/// let running = GenerationId::current(&profiles.join("system")).unwrap();
/// yukimi_helper::snapshots::keep(&setup, &root, &running)?;
/// let kept = Snapshot::of(&root, &running).unwrap();
/// assert_eq!(std::fs::read_to_string(kept.dir().join("flake.lock")).unwrap(), "lock-C");
/// # std::fs::remove_dir_all(&dir).unwrap();
/// # Ok::<(), yukimi_helper::process::Failure>(())
/// ```
pub fn keep(setup: &Setup, root: &Path, generation: &GenerationId) -> Result<()> {
    if let Some(kept) = Snapshot::of(root, generation) {
        // A copy from before copies recorded their system, taken for this
        // generation by its time: from now on it says so itself.
        let _ = kept.record_system();
        return Ok(());
    }
    save(setup, root, generation)
}

/// Keeps the configuration as `generation`'s, in `root`, in place of any
/// copy: it is the configuration that just built it.
pub fn save(setup: &Setup, root: &Path, generation: &GenerationId) -> Result<()> {
    let saving = Saving::begin(root, generation)?;
    copy_configuration(&setup.dir, saving.dir())?;
    saving.finish()?;
    Ok(())
}

/// Put back the files Yukimi changes (the lock file, its own file, the main
/// configuration file) as a kept configuration has them. A file someone
/// changed since the running generation was built is left as it is, so
/// that nothing written by hand is lost. Returns the files left alone.
pub fn restore_choices(setup: &Setup, kept: &Snapshot, running: Option<&Snapshot>) -> Vec<PathBuf> {
    let mut files = vec![setup.dir.join("flake.nix"), setup.dir.join("flake.lock")];
    files.extend(setup.main.clone());
    files.extend(setup.packages_file());
    let mut left = Vec::new();
    for file in files {
        let Ok(relative) = file.strip_prefix(&setup.dir) else { continue };
        let now = std::fs::read(&file).ok();
        let then = std::fs::read(kept.dir().join(relative)).ok();
        if now == then {
            continue;
        }
        // Unchanged since the running generation was made from it?
        let untouched = running.is_some_and(|r| std::fs::read(r.dir().join(relative)).ok() == now);
        if !untouched {
            left.push(file);
            continue;
        }
        let _ = match then {
            Some(bytes) => std::fs::write(&file, bytes),
            None => std::fs::remove_file(&file),
        };
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Fixture {
            let dir = std::env::temp_dir().join(format!("yukimi-snapshots-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            for d in ["nixos", "profiles", "configurations"] {
                std::fs::create_dir_all(dir.join(d)).unwrap();
            }
            Fixture { dir }
        }
        fn nixos(&self) -> PathBuf {
            self.dir.join("nixos")
        }
        fn root(&self) -> PathBuf {
            self.dir.join("configurations")
        }
        fn profile(&self) -> PathBuf {
            self.dir.join("profiles/system")
        }
        fn setup(&self) -> Setup {
            Setup::detect_in(&self.nixos(), Default::default(), "acer")
        }
        /// Generation `n`, a link to `system`, made anew.
        fn generation(&self, n: u32, system: &str) -> GenerationId {
            let link = self.dir.join(format!("profiles/system-{n}-link"));
            let _ = std::fs::remove_file(&link);
            symlink(system, link).unwrap();
            GenerationId::read(&self.profile(), n).unwrap()
        }
        fn kept_lock(&self, generation: &GenerationId) -> Option<String> {
            let kept = Snapshot::of(&self.root(), generation)?;
            std::fs::read_to_string(kept.dir().join("flake.lock")).ok()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// YUK-10: Nix gives a deleted generation's number to the next build;
    /// the configuration kept for the old one isn't the new one's.
    #[test]
    fn a_reused_generation_number_gets_its_own_copy() {
        let f = Fixture::new("reused");
        std::fs::write(f.nixos().join("flake.lock"), "lock-B").unwrap();
        let built = f.generation(2, "/nix/store/s1");
        save(&f.setup(), &f.root(), &built).unwrap();
        // nix-collect-garbage -d, then nixos-rebuild switch from lock-C.
        std::fs::write(f.nixos().join("flake.lock"), "lock-C").unwrap();
        let rebuilt = f.generation(2, "/nix/store/s2");
        assert_eq!(f.kept_lock(&rebuilt), None, "the copy for the deleted generation 2 isn't the new one's");
        keep(&f.setup(), &f.root(), &rebuilt).unwrap();
        assert_eq!(f.kept_lock(&rebuilt).as_deref(), Some("lock-C"));
        // Kept, not replaced, while it is this generation's.
        std::fs::write(f.nixos().join("flake.lock"), "lock-D").unwrap();
        keep(&f.setup(), &f.root(), &rebuilt).unwrap();
        assert_eq!(f.kept_lock(&rebuilt).as_deref(), Some("lock-C"));
    }

    #[test]
    fn configurations_copy_without_hidden_or_large_files() {
        let f = Fixture::new("copy");
        let (from, to) = (f.nixos(), f.dir.join("saved"));
        std::fs::create_dir_all(from.join("modules")).unwrap();
        std::fs::create_dir_all(from.join(".git")).unwrap();
        std::fs::write(from.join("configuration.nix"), "{ }").unwrap();
        std::fs::write(from.join("modules/desk.nix"), "{ }").unwrap();
        std::fs::write(from.join(".git/HEAD"), "ref").unwrap();
        std::fs::write(from.join("wallpaper.png"), vec![0u8; (LARGEST_KEPT + 1) as usize]).unwrap();
        copy_configuration(&from, &to).unwrap();
        assert_eq!(std::fs::read_to_string(to.join("configuration.nix")).unwrap(), "{ }");
        assert!(to.join("modules/desk.nix").is_file());
        assert!(!to.join(".git").exists() && !to.join("wallpaper.png").exists());
        // A private file stays private.
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(from.join("secrets.nix"), "{ }").unwrap();
        std::fs::set_permissions(from.join("secrets.nix"), std::fs::Permissions::from_mode(0o600)).unwrap();
        copy_configuration(&from, &to).unwrap();
        assert_eq!(std::fs::metadata(to.join("secrets.nix")).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn going_back_keeps_what_was_written_by_hand() {
        let f = Fixture::new("restore");
        let (older, running) = (f.generation(1, "/nix/store/s1"), f.generation(2, "/nix/store/s2"));
        let config = f.nixos();
        let (older_copy, running_copy) =
            (Saving::begin(&f.root(), &older).unwrap(), Saving::begin(&f.root(), &running).unwrap());
        let (older_dir, running_dir) = (older_copy.dir().to_owned(), running_copy.dir().to_owned());
        // Migration changes flake.nix too; it must go back with its lock.
        std::fs::write(older_dir.join("flake.nix"), "old repositories").unwrap();
        std::fs::write(running_dir.join("flake.nix"), "sekka").unwrap();
        std::fs::write(config.join("flake.nix"), "sekka").unwrap();
        // flake.lock: unchanged since generation 2, so it goes back to 1's.
        std::fs::write(older_dir.join("flake.lock"), "lock 1").unwrap();
        std::fs::write(running_dir.join("flake.lock"), "lock 2").unwrap();
        std::fs::write(config.join("flake.lock"), "lock 2").unwrap();
        // configuration.nix: edited by hand since generation 2, so it stays.
        std::fs::write(older_dir.join("configuration.nix"), "{ a = 1; }").unwrap();
        std::fs::write(running_dir.join("configuration.nix"), "{ a = 2; }").unwrap();
        std::fs::write(config.join("configuration.nix"), "{ a = 3; }").unwrap();
        // yukimi.nix: generation 1 had none.
        std::fs::write(running_dir.join("yukimi.nix"), "{ }").unwrap();
        std::fs::write(config.join("yukimi.nix"), "{ }").unwrap();
        let (kept, from) = (older_copy.finish().unwrap(), running_copy.finish().unwrap());
        let left = restore_choices(&f.setup(), &kept, Some(&from));
        assert_eq!(std::fs::read_to_string(config.join("flake.nix")).unwrap(), "old repositories");
        assert_eq!(std::fs::read_to_string(config.join("flake.lock")).unwrap(), "lock 1");
        assert_eq!(std::fs::read_to_string(config.join("configuration.nix")).unwrap(), "{ a = 3; }");
        assert!(!config.join("yukimi.nix").exists());
        assert_eq!(left, vec![config.join("configuration.nix")]);
    }
}
