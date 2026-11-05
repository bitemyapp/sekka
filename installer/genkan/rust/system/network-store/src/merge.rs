// SPDX-License-Identifier: GPL-3.0-or-later
//! Moving profiles between NetworkManager's plaintext directory, the open
//! store and an old store, without losing one. A profile both sides have in
//! different versions keeps one in place and the other in
//! `.sekka-conflicts/` (NetworkManager ignores hidden names). Every step is
//! repeatable: a run killed anywhere is finished by the next.

use std::fs::{self, Metadata, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, fchown, lchown, symlink};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::files;
use crate::skeleton::strip;

pub const CONFLICTS: &str = ".sekka-conflicts";

/// Where profiles are coming from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// NetworkManager's plaintext directory, adopted into the open store.
    Plaintext,
    /// An old store (sealed to a TPM that came back), merged into the open one.
    Rotated,
    /// The open store, exported back to the plaintext directory.
    Store,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    File(Vec<u8>),
    Link(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub content: Content,
    pub modified: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// Nothing of that name there yet: copy it in.
    Copy,
    /// The same, or a stand-in for what's there: nothing to keep.
    Drop,
    /// Copy it in, setting the old one aside.
    SourceWins,
    /// Keep what's there, setting this one aside.
    DestinationWins,
}

/// What to do with `source`'s profile `s`, given `d` of the same name where
/// it's going.
pub fn decide(source: Source, s: &Entry, d: Option<&Entry>) -> Outcome {
    let Some(d) = d else { return Outcome::Copy };
    if s.content == d.content {
        return Outcome::Drop;
    }
    let newer = if s.modified > d.modified {
        Outcome::SourceWins
    } else {
        Outcome::DestinationWins
    };
    match (source, &s.content, &d.content) {
        // The secret-less copy put back while the store couldn't be opened,
        // untouched since: the store's profile is the real one.
        (Source::Plaintext, Content::File(s), Content::File(d)) if *s == strip(d) => Outcome::Drop,
        (Source::Plaintext | Source::Store, ..) => newer,
        // What's there is this profile's secret-less copy, and the old
        // store still has the secrets.
        (Source::Rotated, Content::File(s), Content::File(d))
            if strip(s) == *d && strip(s) != *s =>
        {
            Outcome::SourceWins
        }
        (Source::Rotated, ..) => Outcome::DestinationWins,
    }
}

/// What a merge did.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub copied: usize,
    pub conflicts: usize,
}

/// A file as it was first seen, to tell whether it changed since.
#[derive(PartialEq)]
struct Stamp(u64, u64, u64, i64, i64);

impl Stamp {
    fn of(meta: &Metadata) -> Stamp {
        Stamp(
            meta.dev(),
            meta.ino(),
            meta.size(),
            meta.mtime(),
            meta.mtime_nsec(),
        )
    }
}

fn read(path: &Path, meta: &Metadata) -> io::Result<Entry> {
    let content = if meta.file_type().is_symlink() {
        Content::Link(fs::read_link(path)?)
    } else {
        Content::File(fs::read(path)?)
    };
    Ok(Entry {
        content,
        modified: meta.modified()?,
    })
}

/// Profiles NetworkManager reads: regular files and its symlinks
/// (`.nmmeta`), not hidden names or directories.
fn movable(name: &str, meta: &Metadata) -> bool {
    !name.starts_with('.') && (meta.is_file() || meta.file_type().is_symlink())
}

/// Write `entry` as `dir/name` with `meta`'s owner, mode and time.
fn place(dir: &Path, name: &str, entry: &Entry, meta: &Metadata) -> io::Result<()> {
    let target = dir.join(name);
    let tmp = files::temporary(&target);
    let _ = fs::remove_file(&tmp);
    match &entry.content {
        Content::Link(to) => {
            symlink(to, &tmp)?;
            lchown(&tmp, Some(meta.uid()), Some(meta.gid()))?;
        }
        Content::File(bytes) => {
            let mode = meta.mode() & 0o7777;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(bytes)?;
            fchown(&file, Some(meta.uid()), Some(meta.gid()))?;
            file.set_permissions(Permissions::from_mode(mode))?;
            file.set_modified(entry.modified)?;
            file.sync_all()?;
        }
    }
    fs::rename(&tmp, &target)?;
    files::sync_dir(dir)
}

/// A free name in `dir`'s conflicts for `name`.
fn conflict_path(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let conflicts = dir.join(CONFLICTS);
    files::ensure_dir(&conflicts, 0o700)?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut path = conflicts.join(format!("{name}.{secs}"));
    let mut n = 1;
    while fs::symlink_metadata(&path).is_ok() {
        path = conflicts.join(format!("{name}.{secs}.{n}"));
        n += 1;
    }
    Ok(path)
}

/// Move every profile in `from` into `to`. With `remove`, each is then
/// unlinked from `from`, but only if it didn't change meanwhile.
pub fn merge(source: Source, from: &Path, to: &Path, remove: bool) -> io::Result<Report> {
    files::remove_temporaries(to)?;
    let mut report = Report::default();
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(report);
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if !movable(&name, &meta) {
            continue;
        }
        let before = Stamp::of(&meta);
        let s = read(&path, &meta)?;
        let there = to.join(&name);
        let d = match fs::symlink_metadata(&there) {
            Ok(m) => Some(read(&there, &m)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        match decide(source, &s, d.as_ref()) {
            Outcome::Copy => {
                place(to, &name, &s, &meta)?;
                report.copied += 1;
            }
            Outcome::SourceWins => {
                fs::rename(&there, conflict_path(to, &name)?)?;
                place(to, &name, &s, &meta)?;
                report.copied += 1;
                report.conflicts += 1;
            }
            Outcome::DestinationWins => {
                let aside = conflict_path(to, &name)?;
                place(
                    aside.parent().unwrap_or(to),
                    &aside.file_name().unwrap().to_string_lossy(),
                    &s,
                    &meta,
                )?;
                report.conflicts += 1;
            }
            Outcome::Drop => {}
        }
        crate::crash_point("merge-placed");
        if remove && fs::symlink_metadata(&path).map(|m| Stamp::of(&m)).ok() == Some(before) {
            fs::remove_file(&path)?;
        }
    }
    if remove {
        files::sync_dir(from)?;
    }
    Ok(report)
}

/// Whether any profile is in `dir`.
#[cfg(test)]
fn any(dir: &Path) -> bool {
    fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        fs::symlink_metadata(e.path()).is_ok_and(|m| movable(&e.file_name().to_string_lossy(), &m))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::Duration;

    fn at(content: &str, secs: u64) -> Entry {
        Entry {
            content: Content::File(content.into()),
            modified: UNIX_EPOCH + Duration::from_secs(secs),
        }
    }

    const REAL: &str = "[connection]\nid=Home\n[wifi-security]\npsk=correct horse\n";
    const STAND_IN: &str = "[connection]\nid=Home\n[wifi-security]\n";

    #[test]
    fn nothing_there_is_copied_and_the_same_is_dropped() {
        assert_eq!(decide(Source::Plaintext, &at(REAL, 1), None), Outcome::Copy);
        assert_eq!(
            decide(Source::Plaintext, &at(REAL, 1), Some(&at(REAL, 9))),
            Outcome::Drop
        );
        assert_eq!(
            decide(Source::Rotated, &at(REAL, 9), Some(&at(REAL, 1))),
            Outcome::Drop
        );
    }

    #[test]
    fn an_untouched_stand_in_gives_way_to_the_stores_profile() {
        // Put back at a later boot, so it's newer, but it's only a stand-in.
        assert_eq!(
            decide(Source::Plaintext, &at(STAND_IN, 9), Some(&at(REAL, 1))),
            Outcome::Drop
        );
    }

    #[test]
    fn otherwise_the_newer_version_wins_and_the_other_is_kept_aside() {
        let typed_again = "[connection]\nid=Home\n[wifi-security]\npsk=new horse\n";
        assert_eq!(
            decide(Source::Plaintext, &at(typed_again, 9), Some(&at(REAL, 1))),
            Outcome::SourceWins
        );
        assert_eq!(
            decide(Source::Plaintext, &at(typed_again, 1), Some(&at(REAL, 9))),
            Outcome::DestinationWins
        );
        assert_eq!(
            decide(Source::Store, &at(REAL, 9), Some(&at(typed_again, 1))),
            Outcome::SourceWins
        );
        // Equal times: what's there stays.
        assert_eq!(
            decide(Source::Plaintext, &at(typed_again, 5), Some(&at(REAL, 5))),
            Outcome::DestinationWins
        );
    }

    #[test]
    fn an_old_store_restores_secrets_only_over_their_stand_in() {
        assert_eq!(
            decide(Source::Rotated, &at(REAL, 1), Some(&at(STAND_IN, 9))),
            Outcome::SourceWins
        );
        let typed_again = "[connection]\nid=Home\n[wifi-security]\npsk=new horse\n";
        assert_eq!(
            decide(Source::Rotated, &at(REAL, 9), Some(&at(typed_again, 1))),
            Outcome::DestinationWins
        );
        // A secret-less profile there that isn't this one's stand-in stays.
        assert_eq!(
            decide(
                Source::Rotated,
                &at(REAL, 9),
                Some(&at("[connection]\nid=Other\n", 1))
            ),
            Outcome::DestinationWins
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sekka-network-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("from")).unwrap();
        fs::create_dir_all(dir.join("to")).unwrap();
        dir
    }

    fn write(path: &Path, content: &str, secs: u64) {
        fs::write(path, content).unwrap();
        fs::set_permissions(path, Permissions::from_mode(0o600)).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn adoption_moves_profiles_and_keeps_both_versions_of_a_clash() {
        let dir = scratch("adopt");
        let (from, to) = (dir.join("from"), dir.join("to"));
        write(&from.join("Home.nmconnection"), REAL, 100);
        write(&from.join("Office.nmconnection"), "office new", 300);
        write(&to.join("Office.nmconnection"), "office old", 200);
        write(&from.join("Cafe.nmconnection"), STAND_IN, 900);
        write(&to.join("Cafe.nmconnection"), REAL, 1);
        symlink("/dev/null", from.join("1234.nmmeta")).unwrap();
        fs::create_dir(from.join("lost+found")).unwrap();
        write(&from.join(".hidden"), "x", 1);

        let report = merge(Source::Plaintext, &from, &to, true).unwrap();
        assert_eq!(
            report,
            Report {
                copied: 3,
                conflicts: 1
            }
        );
        assert_eq!(
            fs::read_to_string(to.join("Home.nmconnection")).unwrap(),
            REAL
        );
        let home = fs::metadata(to.join("Home.nmconnection")).unwrap();
        assert_eq!(home.mode() & 0o777, 0o600);
        assert_eq!(
            home.modified().unwrap(),
            UNIX_EPOCH + Duration::from_secs(100)
        );
        assert_eq!(
            fs::read_to_string(to.join("Office.nmconnection")).unwrap(),
            "office new"
        );
        assert_eq!(
            fs::read_to_string(to.join("Cafe.nmconnection")).unwrap(),
            REAL
        );
        assert_eq!(
            fs::read_link(to.join("1234.nmmeta")).unwrap(),
            Path::new("/dev/null")
        );
        let aside: Vec<String> = fs::read_dir(to.join(CONFLICTS))
            .unwrap()
            .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        assert_eq!(aside, ["office old"]);
        let left: Vec<String> = fs::read_dir(&from)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            left.len(),
            2,
            "only the directory and the hidden file stay: {left:?}"
        );
        assert!(!any(&from));
        // A second run finds nothing more to do.
        assert_eq!(
            merge(Source::Plaintext, &from, &to, true).unwrap(),
            Report::default()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_profile_changed_while_it_was_copied_stays_for_the_next_run() {
        let dir = scratch("changed");
        let (from, to) = (dir.join("from"), dir.join("to"));
        write(&from.join("Home.nmconnection"), REAL, 100);
        // As if NetworkManager rewrote it between the copy and the unlink.
        let report = merge(Source::Plaintext, &from, &to, false).unwrap();
        assert_eq!(report.copied, 1);
        write(&from.join("Home.nmconnection"), "rewritten", 200);
        merge(Source::Plaintext, &from, &to, true).unwrap();
        assert_eq!(
            fs::read_to_string(to.join("Home.nmconnection")).unwrap(),
            "rewritten"
        );
        assert!(!from.join("Home.nmconnection").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
