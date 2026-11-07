// SPDX-License-Identifier: GPL-3.0-or-later
//! The engine as a process: Asahi's installer with Sekka's adapter,
//! unpacked from the archive the app carries, started with a fixed command
//! line and a cleared environment, read line by line.
//!
//! The app runs `inspect` as the person using it, before asking for any
//! password; the helper runs `inspect` again as root, then `execute`. Both
//! unpack the same archive, after checking its digest, into a directory of
//! their own.
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::RawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use sekka_mac_contract::{
    Ack, Digest, EngineBody, EngineOutcome, Hasher, Inspection, Transcript, canonical, limits,
};

/// The search path every engine process gets.
pub const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("{0}")]
    Missing(String),
    #[error("this copy of Sekka Installer carries a different engine than the release needs; download the latest Sekka Installer")]
    WrongDigest,
    #[error("the installer's engine couldn't be unpacked: {0}")]
    Unpack(String),
    #[error("the installer's engine couldn't be started: {0}")]
    Start(String),
    #[error("{0}")]
    Inspection(String),
}

/// An unpacked engine.
#[derive(Clone, Debug)]
pub struct EngineInstall {
    pub root: PathBuf,
    pub digest: Digest,
}

/// The digest of a file, read in full.
pub fn digest_file(path: &Path) -> io::Result<Digest> {
    let mut f = fs::File::open(path)?;
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(hasher.finish());
        }
        hasher.update(&buf[..n]);
    }
}

/// The largest engine archive accepted.
pub const ARCHIVE_LIMIT: u64 = 1024 * 1024 * 1024;

/// An engine unpacked earlier under `cache` from an archive with this
/// digest. The cache is private to whoever unpacked it (the helper's is
/// root's alone), so what is there was checked when it was made.
pub fn cached(cache: &Path, digest: &Digest) -> Option<EngineInstall> {
    let root = cache.join(digest.hex());
    let complete = fs::read_to_string(root.join(".complete")).ok()?;
    (complete == digest.to_string()).then_some(EngineInstall { root, digest: *digest })
}

/// Unpack `archive` under `cache/<digest>` once, refusing it unless its
/// digest is `expected` (when given), and reuse an engine already unpacked
/// for `expected` (an installation can be retried after the app that
/// installed it was replaced).
///
/// The archive in the app bundle is read exactly once: opened without
/// following links, refused unless it is a regular file, and copied into
/// the private cache while it is hashed. Only that private copy is checked
/// and extracted, so nothing the person running the app can change (the
/// bundle is theirs) can differ between the bytes hashed and the bytes
/// unpacked. bsdtar refuses absolute paths and `..`.
pub fn unpack(archive: &Path, cache: &Path, expected: Option<&Digest>) -> Result<EngineInstall, EngineError> {
    if let Some(found) = expected.and_then(|d| cached(cache, d)) {
        return Ok(found);
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(cache)
        .map_err(|e| EngineError::Unpack(e.to_string()))?;
    let missing = |e: io::Error| EngineError::Missing(format!("This copy of Sekka Installer has no usable engine ({e}). Download it again."));
    let mut source = open_regular(archive).map_err(missing)?;
    let copy = cache.join(format!(".archive-{}", std::process::id()));
    let _ = fs::remove_file(&copy);
    let digest = (|| -> io::Result<Digest> {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&copy)?;
        let mut hasher = Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0u64;
        loop {
            let n = source.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > ARCHIVE_LIMIT {
                return Err(io::Error::other("the archive is too large"));
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        out.sync_all()?;
        Ok(hasher.finish())
    })();
    drop(source);
    let digest = match digest {
        Ok(d) => d,
        Err(e) => {
            let _ = fs::remove_file(&copy);
            return Err(missing(e));
        }
    };
    if expected.is_some_and(|e| *e != digest) {
        let _ = fs::remove_file(&copy);
        return Err(EngineError::WrongDigest);
    }
    if let Some(found) = cached(cache, &digest) {
        let _ = fs::remove_file(&copy);
        return Ok(found);
    }
    let root = cache.join(digest.hex());
    let tmp = cache.join(format!(".unpack-{}", digest.hex()));
    let _ = fs::remove_dir_all(&tmp);
    let made = fs::DirBuilder::new().mode(0o700).create(&tmp);
    let out = made.and_then(|_| {
        Command::new("/usr/bin/tar")
            .args(["-x", "-z", "--no-same-owner", "-f"])
            .arg(&copy)
            .arg("-C")
            .arg(&tmp)
            .env_clear()
            .env("PATH", PATH)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
    });
    let _ = fs::remove_file(&copy);
    let out = out.map_err(|e| EngineError::Unpack(e.to_string()))?;
    if !out.status.success() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(EngineError::Unpack(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    fs::write(tmp.join(".complete"), digest.to_string()).map_err(|e| EngineError::Unpack(e.to_string()))?;
    let _ = fs::remove_dir_all(&root);
    fs::rename(&tmp, &root).map_err(|e| EngineError::Unpack(e.to_string()))?;
    Ok(EngineInstall { root, digest })
}

/// Open a regular file for reading without following a link at its last
/// component, and without blocking on a FIFO put in its place.
fn open_regular(path: &Path) -> io::Result<fs::File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    // SAFETY: clearing O_NONBLOCK on our own descriptor.
    unsafe {
        let fd = std::os::fd::AsRawFd::as_raw_fd(&file);
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
    }
    Ok(file)
}

/// The engine's own description of how to start it (`engine-lock.json`
/// at its root, inside the digest-checked archive): the bundled Python and
/// the adapter's entry point, both relative to the root.
#[derive(serde::Deserialize)]
struct Entry {
    runtime: String,
    entry: String,
}

fn inside(rel: &str) -> bool {
    !rel.is_empty() && !rel.starts_with('/') && rel.split('/').all(|c| !c.is_empty() && c != "..")
}

/// `RUNTIME -I ENTRY ARGS…`, as Asahi's own `install.sh` starts its
/// Python, with nothing inherited from the caller's environment. The entry
/// is run by path: the bundled Python.framework's signature seals its
/// site-packages, so the adapter isn't installed there.
pub fn command(engine: &EngineInstall, args: &[&str]) -> Result<Command, EngineError> {
    let bytes = fs::read(engine.root.join("engine-lock.json"))
        .map_err(|e| EngineError::Missing(format!("The installer's engine is incomplete: {e}")))?;
    let lock: Entry = serde_json::from_slice(&bytes)
        .map_err(|e| EngineError::Missing(format!("The installer's engine is incomplete: {e}")))?;
    if !inside(&lock.runtime) || !inside(&lock.entry) {
        return Err(EngineError::Missing("The installer's engine names a program outside itself.".into()));
    }
    let root = &engine.root;
    let mut command = Command::new(root.join(&lock.runtime));
    command
        .arg("-I")
        .arg(root.join(&lock.entry))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", PATH)
        .env("LC_ALL", "C")
        .env("DYLD_FRAMEWORK_PATH", root.join("Frameworks"))
        .env("DYLD_LIBRARY_PATH", root.join("Frameworks/Python.framework/Versions/Current/lib"))
        .env("PYTHONDONTWRITEBYTECODE", "1");
    Ok(command)
}

/// Give an engine started for an installation its two private
/// descriptors: the owner's password on 3, which it reads to its end, and
/// the install lock on 4, which it keeps open until it exits. The lock
/// belongs to the open file, so the engine holds it for as long as it runs,
/// even if the helper that started it stops first, and nothing claims it
/// meanwhile to inspect or install. The engine starts its own tools without
/// either. Both descriptors must stay open until `command` is spawned.
pub fn inherit(command: &mut Command, password: RawFd, lock: RawFd) {
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        command.pre_exec(move || {
            // Copies above both targets first, so neither dup2 below closes
            // the other's source. The copies end at exec; 3 and 4 don't.
            let password = libc::fcntl(password, libc::F_DUPFD_CLOEXEC, 5);
            let lock = libc::fcntl(lock, libc::F_DUPFD_CLOEXEC, 5);
            if password < 0 || lock < 0 || libc::dup2(password, 3) < 0 || libc::dup2(lock, 4) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// A running engine, however it was started.
pub trait EngineProcess: Send {
    /// The next line of its transcript, without the newline; `None` once it
    /// has closed its output.
    fn next_line(&mut self) -> io::Result<Option<Vec<u8>>>;
    fn ack(&mut self, seq: u64) -> io::Result<()>;
    /// Something another thread can call to ask it to stop. Only used while
    /// no stage that changes the disk is running: disk tools are never
    /// killed mid-operation.
    fn terminator(&self) -> Box<dyn FnMut() + Send>;
    /// Wait for it to exit; whether it exited successfully.
    fn wait(&mut self) -> io::Result<bool>;
}

/// An engine started as a child process: transcript on its stdout,
/// acknowledgements on its stdin.
pub struct ChildEngine {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl ChildEngine {
    pub fn new(mut child: Child) -> io::Result<Self> {
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("the engine has no output"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }
}

impl EngineProcess for ChildEngine {
    fn next_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        let n = (&mut self.stdout)
            .take(limits::ENGINE_LINE as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if line.last() != Some(&b'\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a transcript line is too long or unterminated",
            ));
        }
        line.pop();
        Ok(Some(line))
    }
    fn ack(&mut self, seq: u64) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("the engine's input is closed"))?;
        let mut bytes = canonical::to_vec(&Ack { ack: seq }).map_err(io::Error::other)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes)?;
        stdin.flush()
    }
    fn terminator(&self) -> Box<dyn FnMut() + Send> {
        let pid = self.child.id() as libc::pid_t;
        Box::new(move || {
            // SAFETY: signalling our own child by its pid, only while it is
            // alive and outside a disk-changing stage.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        })
    }
    fn wait(&mut self) -> io::Result<bool> {
        // End of input tells the engine no further stage is acknowledged.
        self.stdin = None;
        Ok(self.child.wait()?.success())
    }
}

/// Run `inspect` and return what it found, checked against the protocol.
pub fn inspect(engine: &EngineInstall) -> Result<Inspection, EngineError> {
    let child = command(engine, &["inspect"])?
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| EngineError::Start(e.to_string()))?;
    let mut process = ChildEngine::new(child).map_err(|e| EngineError::Start(e.to_string()))?;
    // Inspection changes nothing, so a stuck one can be stopped.
    let mut stop = process.terminator();
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if finished.recv_timeout(INSPECT_TIMEOUT).is_err() {
            stop();
        }
    });
    let result = read_inspection(&mut process);
    let _ = done.send(());
    let _ = watchdog.join();
    result
}

/// How long an inspection may take.
pub const INSPECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Read an inspection transcript to its end.
pub fn read_inspection(process: &mut dyn EngineProcess) -> Result<Inspection, EngineError> {
    let mut transcript = Transcript::inspect();
    let mut inspection = None;
    let mut failure = None;
    while let Some(line) = process
        .next_line()
        .map_err(|e| EngineError::Inspection(e.to_string()))?
    {
        let message = transcript
            .accept(&line)
            .map_err(|e| EngineError::Inspection(format!("The engine's inspection didn't make sense: {e}")))?;
        match message.message {
            EngineBody::Inspection(i) => inspection = Some(*i),
            EngineBody::Failure { failure: f, .. } => failure = Some(f.message.to_string()),
            _ => {}
        }
        if transcript.outcome().is_some() {
            break;
        }
    }
    let _ = process.wait();
    if let Some(f) = failure {
        return Err(EngineError::Inspection(f));
    }
    match (transcript.outcome(), inspection) {
        (Some(EngineOutcome::Inspected), Some(i)) => Ok(i),
        _ => Err(EngineError::Inspection("The engine couldn't inspect this Mac.".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A real archive, unpacked by the real tar, started through
    /// `engine-lock.json` with a cleared environment: here the "Python" is a
    /// shell script that prints a canned inspection transcript.
    #[test]
    fn unpack_start_and_read_an_inspection() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("bin")).unwrap();
        let mut lines = String::new();
        let hello = sekka_mac_contract::EngineMessage {
            schema: sekka_mac_contract::EngineSchema,
            seq: 1,
            message: EngineBody::Hello {
                engine: "0.9.2+sekka.1".parse().unwrap(),
                asahi: "0.9.2".parse().unwrap(),
                adapter: Digest::of(b"a"),
                mode: sekka_mac_contract::EngineMode::Inspect,
                protocol: sekka_mac_contract::ENGINE_PROTOCOL,
            },
        };
        let body = sekka_mac_contract::EngineMessage {
            schema: sekka_mac_contract::EngineSchema,
            seq: 2,
            message: EngineBody::Inspection(Box::new(sekka_mac_contract::samples::inspection())),
        };
        let done = sekka_mac_contract::EngineMessage {
            schema: sekka_mac_contract::EngineSchema,
            seq: 3,
            message: EngineBody::Completion {
                outcome: EngineOutcome::Inspected,
            },
        };
        for m in [hello, body, done] {
            lines.push_str(&String::from_utf8(canonical::to_vec(&m).unwrap()).unwrap());
            lines.push('\n');
        }
        fs::write(src.join("transcript"), lines).unwrap();
        // The arguments must be exactly the fixed ones, and the environment
        // must be clear.
        let script = "#!/bin/sh\n[ \"$1\" = -I ] && [ \"${2##*/}\" = __main__.py ] && [ \"$3\" = inspect ] && [ $# -eq 3 ] || exit 3\n[ -z \"$HOME\" ] || exit 4\nwhile IFS= read -r l; do printf '%s\\n' \"$l\"; done < transcript\n";
        fs::write(src.join("bin/python3"), script).unwrap();
        fs::set_permissions(src.join("bin/python3"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir_all(src.join("sekka_engine")).unwrap();
        fs::write(src.join("sekka_engine/__main__.py"), b"").unwrap();
        fs::write(src.join("engine-lock.json"), br#"{"entry":"sekka_engine/__main__.py","runtime":"bin/python3","schema":"x"}"#).unwrap();
        // Started as the lock says, with a cleared environment.
        let unpacked = EngineInstall {
            root: src.clone(),
            digest: Digest::of(b"tree"),
        };
        assert_eq!(inspect(&unpacked).unwrap(), sekka_mac_contract::samples::inspection());
        let archive = tmp.path().join("engine.tar.gz");
        let tar = if Path::new("/usr/bin/tar").exists() { "/usr/bin/tar" } else { "tar" };
        assert!(Command::new(tar).arg("-czf").arg(&archive).arg("-C").arg(&src).arg(".").status().unwrap().success());
        if !Path::new("/usr/bin/tar").exists() {
            // `unpack` uses macOS's tar by its absolute path; elsewhere the
            // rest is still worth checking.
            return;
        }
        let engine = unpack(&archive, &tmp.path().join("cache"), None).unwrap();
        assert_eq!(engine.digest, digest_file(&archive).unwrap());
        assert!(unpack(&archive, &tmp.path().join("cache"), Some(&Digest::of(b"other"))).is_err());
        let found = inspect(&engine).unwrap();
        assert_eq!(found, sekka_mac_contract::samples::inspection());
    }

    #[test]
    fn an_engine_holds_the_install_lock_until_it_exits() {
        use std::os::fd::AsRawFd;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("installing.lock");
        let open = || {
            fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&path)
                .unwrap()
        };
        let lock = open();
        lock.try_lock().unwrap();
        let (password, mut writing) = io::pipe().unwrap();
        writing.write_all(b"correct horse").unwrap();
        drop(writing);
        // Prints the password from 3, then waits for its input to end with
        // 4 still open.
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "cat <&3; read line || :"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        inherit(&mut command, password.as_raw_fd(), lock.as_raw_fd());
        let mut child = command.spawn().unwrap();
        drop(password);
        let mut read = [0u8; 13];
        child.stdout.take().unwrap().read_exact(&mut read).unwrap();
        assert_eq!(&read, b"correct horse");
        // The helper that started it is gone; the engine still holds it.
        drop(lock);
        assert!(matches!(open().try_lock(), Err(fs::TryLockError::WouldBlock)));
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
        assert!(open().try_lock().is_ok());
    }

    #[test]
    fn the_archive_is_read_once_and_never_through_a_link_or_a_fifo() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real.tar.gz");
        fs::write(&real, b"archive").unwrap();
        let link = tmp.path().join("link.tar.gz");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(open_regular(&link).is_err());
        let fifo = tmp.path().join("fifo.tar.gz");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: making a FIFO at a fresh path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        // Refused at once, not blocked waiting for a writer.
        assert!(open_regular(&fifo).is_err());
        assert!(matches!(
            unpack(&fifo, &tmp.path().join("cache"), None),
            Err(EngineError::Missing(_))
        ));
        assert!(open_regular(&real).is_ok());
        // A wrong digest leaves nothing behind but the private cache.
        assert_eq!(
            unpack(&real, &tmp.path().join("cache"), Some(&Digest::of(b"other"))).unwrap_err(),
            EngineError::WrongDigest
        );
        assert_eq!(fs::read_dir(tmp.path().join("cache")).unwrap().count(), 0);
    }

    #[test]
    fn an_engine_unpacked_before_is_reused_by_its_digest() {
        let tmp = tempfile::tempdir().unwrap();
        let digest = Digest::of(b"an engine");
        let root = tmp.path().join(digest.hex());
        fs::create_dir_all(&root).unwrap();
        assert!(cached(tmp.path(), &digest).is_none(), "not complete yet");
        fs::write(root.join(".complete"), digest.to_string()).unwrap();
        assert_eq!(cached(tmp.path(), &digest).unwrap().root, root);
        // The app's bundled archive is gone or different: the installation's
        // own engine is still found.
        assert_eq!(unpack(&tmp.path().join("missing"), tmp.path(), Some(&digest)).unwrap().root, root);
        fs::write(root.join(".complete"), Digest::of(b"x").to_string()).unwrap();
        assert!(cached(tmp.path(), &digest).is_none(), "a marker for another digest doesn't count");
    }

    #[test]
    fn engines_naming_programs_outside_themselves_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("engine-lock.json"), br#"{"runtime":"../../bin/sh","entry":"x.py"}"#).unwrap();
        let engine = EngineInstall {
            root: tmp.path().into(),
            digest: Digest::of(b"x"),
        };
        assert!(command(&engine, &["inspect"]).is_err());
    }
}
