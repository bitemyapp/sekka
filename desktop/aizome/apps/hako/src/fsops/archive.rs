//! Compressing and extracting through `bsdtar` (libarchive), which reads
//! and writes zip, tar (gzip, bzip2, xz, zstd), 7z, cpio, ISO 9660, rar
//! and more.
//!
//! Both work as copies do: the archive (or the extracted folder) is made
//! under a hidden temporary name and renamed into place when it's whole;
//! cancelling kills `bsdtar` and removes what it made; pausing stops it
//! (`SIGSTOP`) until resumed. Extraction never writes outside its folder:
//! `bsdtar` refuses absolute paths and `..` unless asked (`-P`), and is
//! never asked. An archive of one item extracts to that item; one of
//! several, to a folder named after the archive.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use super::delete;
use super::names;
use super::sys;
use super::{Env, Report};

/// Formats Hako makes archives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Zip,
    TarGz,
    TarXz,
    TarZst,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Zip, Format::TarGz, Format::TarXz, Format::TarZst];

    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => ".zip",
            Format::TarGz => ".tar.gz",
            Format::TarXz => ".tar.xz",
            Format::TarZst => ".tar.zst",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Zip => "Zip",
            Format::TarGz => "Tar (gzip)",
            Format::TarXz => "Tar (xz)",
            Format::TarZst => "Tar (zstd)",
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            Format::Zip => &["--format", "zip"],
            Format::TarGz => &["--format", "pax", "-z"],
            Format::TarXz => &["--format", "pax", "-J"],
            Format::TarZst => &["--format", "pax", "--zstd"],
        }
    }
}

/// Extensions of files `bsdtar` can extract.
const ARCHIVES: &[&str] = &[
    ".zip",
    ".tar",
    ".tar.gz",
    ".tgz",
    ".tar.bz2",
    ".tbz2",
    ".tar.xz",
    ".txz",
    ".tar.zst",
    ".tzst",
    ".tar.lz",
    ".tar.lzma",
    ".7z",
    ".rar",
    ".cpio",
    ".iso",
    ".xar",
    ".cab",
    ".lha",
    ".lzh",
    ".ar",
    ".deb",
    ".jar",
    ".apk",
];

/// Whether `name` looks like an archive Extract can open.
pub fn is_archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ARCHIVES.iter().any(|e| lower.ends_with(e) && lower.len() > e.len())
}

/// The name of an archive without its archive extension: `photos.tar.gz`
/// → `photos`.
pub fn archive_stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    ARCHIVES
        .iter()
        .filter(|e| lower.ends_with(*e) && lower.len() > e.len())
        .map(|e| &name[..name.len() - e.len()])
        .min_by_key(|s| s.len())
        .unwrap_or(name)
}

/// `bsdtar`: `$HAKO_BSDTAR`, else the first on `$PATH`.
pub fn bsdtar() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("HAKO_BSDTAR").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join("bsdtar")).find(|p| p.is_file())
}

/// The name a new archive of `items` takes in `dir`: the item's own name
/// for one item, "Archive" for several, numbered if taken.
pub fn archive_name(dir: &Path, items: &[PathBuf], format: Format) -> OsString {
    let base = match items {
        [one] => one.file_name().map(names::display).unwrap_or_else(|| "Archive".into()),
        _ => "Archive".into(),
    };
    names::free_name(dir, &OsString::from(format!("{base}{}", format.extension())), false)
}

/// Make an archive at `archive` of `items` (which share a folder).
pub fn compress(items: &[PathBuf], archive: &Path, format: Format, env: &mut Env) -> Report {
    let mut report = Report::default();
    let fail = |report: &mut Report, why: String| {
        for i in items {
            report.failed.push((i.clone(), why.clone()));
        }
    };
    let Some(tar) = bsdtar() else {
        fail(&mut report, "bsdtar (libarchive) isn't installed".into());
        return report;
    };
    let Some(parent) = items.first().and_then(|i| i.parent()).map(Path::to_path_buf) else { return report };
    if items.iter().any(|i| i.parent() != Some(parent.as_path())) {
        fail(&mut report, "the items aren't all in one folder".into());
        return report;
    }
    env.progress.preparing = true;
    env.report();
    for i in items {
        match delete::measure(i, env.control, u64::MAX) {
            Some((n, b)) => {
                env.progress.items_total += n;
                env.progress.bytes_total += b;
            }
            None => {
                report.cancelled = true;
                return report;
            }
        }
    }
    env.progress.preparing = false;
    let dir = archive.parent().unwrap_or(Path::new("/"));
    let partial = dir.join(names::partial_name(archive.file_name().unwrap_or_default()));
    let mut cmd = Command::new(tar);
    cmd.arg("-c").arg("-v").args(format.args()).arg("-f").arg(&partial).arg("-C").arg(&parent).arg("--");
    for i in items {
        cmd.arg(i.file_name().unwrap_or_default());
    }
    match run(cmd, env, 'a') {
        Ok(()) => match sys::rename_noreplace(&partial, archive) {
            Ok(()) => {
                let _ = sys::sync_dir(dir);
                report.done.push((parent.clone(), archive.to_path_buf()));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                fail(&mut report, super::describe(&e));
            }
        },
        Err(Ended::Cancelled) => {
            let _ = std::fs::remove_file(&partial);
            report.cancelled = true;
        }
        Err(Ended::Failed(why)) => {
            let _ = std::fs::remove_file(&partial);
            fail(&mut report, why);
        }
    }
    report
}

/// Extract `archive` into `dest`: to the one item it holds, or a folder
/// named after it.
pub fn extract(archive: &Path, dest: &Path, env: &mut Env) -> Report {
    let mut report = Report::default();
    let Some(tar) = bsdtar() else {
        report.failed.push((archive.to_path_buf(), "bsdtar (libarchive) isn't installed".into()));
        return report;
    };
    // How many entries, for the progress.
    env.progress.preparing = true;
    env.report();
    let mut list = Command::new(&tar);
    list.arg("-t").arg("-f").arg(archive);
    match list.stderr(Stdio::piped()).output() {
        Ok(out) if out.status.success() => {
            env.progress.items_total = out.stdout.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count() as u64
        }
        Ok(out) => {
            let why =
                String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("it can't be read").to_owned();
            report.failed.push((archive.to_path_buf(), clean_message(&why)));
            return report;
        }
        Err(e) => {
            report.failed.push((archive.to_path_buf(), super::describe(&e)));
            return report;
        }
    }
    env.progress.preparing = false;
    let name = archive.file_name().map(names::display).unwrap_or_else(|| "Archive".into());
    let partial = dest.join(names::partial_name(&OsString::from(archive_stem(&name))));
    if let Err(e) = std::fs::create_dir(&partial) {
        report.failed.push((archive.to_path_buf(), super::describe(&e)));
        return report;
    }
    let mut cmd = Command::new(&tar);
    cmd.arg("-x").arg("-v").arg("-f").arg(archive).arg("-C").arg(&partial).arg("--no-same-owner");
    match run(cmd, env, 'x') {
        Ok(()) => {}
        Err(Ended::Cancelled) => {
            delete::remove_quietly(&partial);
            report.cancelled = true;
            return report;
        }
        Err(Ended::Failed(why)) => {
            delete::remove_quietly(&partial);
            report.failed.push((archive.to_path_buf(), why));
            return report;
        }
    }
    let inside: Vec<_> =
        std::fs::read_dir(&partial).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    let result = if let [one] = inside.as_slice() {
        let is_dir = std::fs::symlink_metadata(one).is_ok_and(|m| m.is_dir());
        let to = dest.join(names::free_name(dest, one.file_name().unwrap_or_default(), is_dir));
        let r = sys::rename_noreplace(one, &to);
        let _ = std::fs::remove_dir(&partial);
        r.map(|()| to)
    } else {
        let to = dest.join(names::free_name(dest, &OsString::from(archive_stem(&name)), true));
        sys::rename_noreplace(&partial, &to).map(|()| to)
    };
    match result {
        Ok(to) => report.done.push((archive.to_path_buf(), to)),
        Err(e) => {
            delete::remove_quietly(&partial);
            report.failed.push((archive.to_path_buf(), super::describe(&e)));
        }
    }
    report
}

enum Ended {
    Cancelled,
    Failed(String),
}

/// `bsdtar: x: Can't …` → `x: Can't …`.
fn clean_message(line: &str) -> String {
    line.strip_prefix("bsdtar: ").unwrap_or(line).to_owned()
}

/// Run `bsdtar` (with `-v`), counting the lines that start with `mark`
/// as items done, pausing and cancelling it with the operation.
fn run(mut cmd: Command, env: &mut Env, mark: char) -> Result<(), Ended> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| Ended::Failed(format!("bsdtar didn't start: {}", super::describe(&e))))?;
    let stderr = child.stderr.take().expect("piped");
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    let prefix = format!("{mark} ");
    let mut errors: Vec<String> = Vec::new();
    let mut stopped = false;
    let pid = child.id() as libc::pid_t;
    let status = loop {
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(line) => {
                if let Some(path) = line.strip_prefix(&prefix) {
                    env.progress.items_done += 1;
                    env.progress.current = Some(PathBuf::from(path));
                    env.report();
                } else if !line.trim().is_empty() {
                    errors.push(clean_message(line.trim()));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Ok(s) = child.wait() {
                    break s;
                }
            }
        }
        if env.control.is_cancelled() {
            // SAFETY: signalling our own child.
            unsafe {
                libc::kill(pid, libc::SIGCONT);
                libc::kill(pid, libc::SIGKILL);
            }
            let _ = child.wait();
            let _ = reader.join();
            return Err(Ended::Cancelled);
        }
        let paused = env.control.is_paused();
        if paused != stopped {
            // SAFETY: as above.
            unsafe { libc::kill(pid, if paused { libc::SIGSTOP } else { libc::SIGCONT }) };
            stopped = paused;
        }
        if let Ok(Some(s)) = child.try_wait() {
            // Drain what it said.
            for line in rx.try_iter() {
                if line.strip_prefix(&prefix).is_some() {
                    env.progress.items_done += 1;
                } else if !line.trim().is_empty() {
                    errors.push(clean_message(line.trim()));
                }
            }
            break s;
        }
    };
    let _ = reader.join();
    if status.success() {
        Ok(())
    } else {
        Err(Ended::Failed(errors.last().cloned().unwrap_or_else(|| format!("bsdtar failed ({status})"))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::{TempDir, read};
    use crate::fsops::{Always, Control, ErrorChoice, Resolution};

    fn env_run<R>(f: impl FnOnce(&mut Env) -> R) -> R {
        let control = Control::new();
        let mut asker = Always(Resolution::Cancel, ErrorChoice::Cancel);
        let mut quiet = ();
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        f(&mut env)
    }

    #[test]
    fn names_archives() {
        assert!(is_archive("photos.tar.gz") && is_archive("A.ZIP") && !is_archive(".zip") && !is_archive("notes.txt"));
        assert_eq!(archive_stem("photos.tar.gz"), "photos");
        assert_eq!(archive_stem("photos.zip"), "photos");
        assert_eq!(archive_stem("photos"), "photos");
        let t = TempDir::new("archive-name");
        t.file("Archive.zip", "");
        assert_eq!(archive_name(t.path(), &[t.join("a"), t.join("b")], Format::Zip), "Archive 2.zip");
        assert_eq!(archive_name(t.path(), &[t.join("pics")], Format::TarGz), "pics.tar.gz");
    }

    #[test]
    fn compresses_and_extracts() {
        if bsdtar().is_none() {
            eprintln!("no bsdtar here: skipped");
            return;
        }
        let t = TempDir::new("archive");
        t.file("src/one/a.txt", "alpha");
        t.file("src/one/sub/b.txt", "bravo");
        t.file("src/two.txt", "two");
        for format in Format::ALL {
            let out = t.dir(&format!("out-{format:?}"));
            let archive = out.join(archive_name(&out, &[t.join("src/one"), t.join("src/two.txt")], format));
            let r = env_run(|env| compress(&[t.join("src/one"), t.join("src/two.txt")], &archive, format, env));
            assert!(r.is_clean(), "{format:?}: {r:?}");
            assert!(archive.is_file());
            // Several items: a folder named after the archive.
            let r = env_run(|env| extract(&archive, &out, env));
            assert!(r.is_clean(), "{r:?}");
            let folder = out.join("Archive");
            assert_eq!(r.done[0].1, folder);
            assert_eq!(read(folder.join("one/a.txt")), "alpha");
            assert_eq!(read(folder.join("one/sub/b.txt")), "bravo");
            assert_eq!(read(folder.join("two.txt")), "two");
            // Again: a name of its own.
            let r = env_run(|env| extract(&archive, &out, env));
            assert_eq!(r.done[0].1, out.join("Archive 2"));
        }
        // One item: that item.
        let archive = t.join("one.zip");
        env_run(|env| compress(&[t.join("src/one")], &archive, Format::Zip, env));
        let out = t.dir("single");
        let r = env_run(|env| extract(&archive, &out, env));
        assert_eq!(r.done[0].1, out.join("one"));
        assert_eq!(read(out.join("one/sub/b.txt")), "bravo");
        // A broken archive fails cleanly.
        let bad = t.file("bad.zip", "not a zip at all");
        let r = env_run(|env| extract(&bad, &out, env));
        assert_eq!(r.failed.len(), 1, "{r:?}");
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1, "nothing left behind");
    }

    #[test]
    fn never_writes_outside_its_folder() {
        if bsdtar().is_none() {
            return;
        }
        let t = TempDir::new("archive-escape");
        // An archive whose entry climbs out with "..".
        t.file("x/evil.txt", "evil");
        let archive = t.join("evil.tar");
        let status = Command::new(bsdtar().unwrap())
            .arg("-c")
            .arg("-f")
            .arg(&archive)
            .arg("-C")
            .arg(t.join("x"))
            .arg("-s")
            .arg(",^,../../,")
            .arg("evil.txt")
            .status()
            .unwrap();
        assert!(status.success());
        let out = t.dir("deep/out");
        let _ = env_run(|env| extract(&archive, &out, env));
        assert!(!t.join("evil.txt").exists() && !t.join("deep/evil.txt").exists(), "nothing escaped");
    }
}
