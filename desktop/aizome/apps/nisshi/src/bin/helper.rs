//! `aizome-nisshi-helper`: Nisshi's reader. The window starts it as you,
//! or through pkexec as root when you unlock the system's logs, and asks
//! it for entries, boots and log files over its standard input and output
//! (`nisshi::protocol`). It only reads: each request is checked and turned
//! into journalctl's arguments here, from a fixed set of options; files
//! are read only under /var/log. It ends when the window closes its input.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use nisshi::protocol::{FileInfo, Op, Ready, Request, VERSION, line};
use nisshi::query;

/// Requests that may run at once (follows included).
const RUNNING: usize = 8;
/// The most of a log file one read gives.
const LARGEST_READ: u64 = 32 << 20;

type Running = Arc<Mutex<HashMap<u64, Arc<Mutex<Option<Child>>>>>>;

fn main() {
    // Nothing this starts gains privileges (no setuid program run by
    // journalctl can, either).
    // SAFETY: prctl with these arguments only sets a flag on this process.
    unsafe {
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
    }
    // Ended with the window, should it end without closing this one's
    // input (it closes it otherwise, and the reads stop first).
    // SAFETY: as above, a setting of this process.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0);
    }
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let root = uid == 0;
    let env = |name: &str| if root { None } else { std::env::var_os(name) };
    let journalctl = find_journalctl(env("NISSHI_JOURNALCTL"));
    let log_dir = env("NISSHI_LOG_DIR").map_or_else(|| PathBuf::from("/var/log"), PathBuf::from);

    let (tx, rx) = channel::<String>();
    let writer = std::thread::spawn(move || write_out(rx));
    let ready = Ready {
        version: VERSION,
        uid,
        system: root || can_read_system_journal(),
        journalctl: journalctl.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
    };
    let _ = tx.send(line(0, 'r', &serde_json::to_string(&ready).unwrap_or_default()));

    let running: Running = Arc::default();
    for input in std::io::stdin().lock().lines() {
        let Ok(input) = input else { break };
        if input.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&input) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(line(0, 'x', &format!("unreadable request: {e}")));
                continue;
            }
        };
        let id = request.id;
        if request.op == Op::Cancel {
            if let Some(child) = running.lock().unwrap().remove(&id)
                && let Some(c) = child.lock().unwrap().as_mut()
            {
                let _ = c.kill();
            }
            continue;
        }
        if running.lock().unwrap().len() >= RUNNING {
            let _ = tx.send(line(id, 'x', "too many reads at once"));
            continue;
        }
        let tx = tx.clone();
        match request.op {
            Op::Query(q) => match q.validate() {
                Ok(()) => run(id, journalctl.clone(), q.args(), 'e', tx, running.clone()),
                Err(e) => send_failure(&tx, id, &e),
            },
            Op::Entry { cursor } => match query::entry_args(&cursor) {
                Ok(args) => run(id, journalctl.clone(), args, 'e', tx, running.clone()),
                Err(e) => send_failure(&tx, id, &e),
            },
            Op::Boots => run(id, journalctl.clone(), query::boots_args(), 'b', tx, running.clone()),
            Op::Files => {
                let dir = log_dir.clone();
                std::thread::spawn(move || {
                    for f in list_files(&dir) {
                        let _ = tx.send(line(id, 'f', &serde_json::to_string(&f).unwrap_or_default()));
                    }
                    let _ = tx.send(line(id, 'd', ""));
                });
            }
            Op::Read { path, max } => {
                let dir = log_dir.clone();
                std::thread::spawn(move || match read_file(&dir, Path::new(&path), max.min(LARGEST_READ), id, &tx) {
                    Ok(()) => {
                        let _ = tx.send(line(id, 'd', ""));
                    }
                    Err(e) => send_failure(&tx, id, &e),
                });
            }
            Op::Cancel => unreachable!(),
        }
    }
    // The window has gone: so do the reads.
    for (_, child) in running.lock().unwrap().drain() {
        if let Some(c) = child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
    drop(tx);
    let _ = writer.join();
}

fn send_failure(tx: &Sender<String>, id: u64, why: &str) {
    let _ = tx.send(line(id, 'x', why));
}

/// Write what the reads send, flushing once nothing more is waiting: a
/// burst goes out in few writes, a single line at once.
fn write_out(rx: Receiver<String>) {
    let mut out = std::io::BufWriter::with_capacity(256 << 10, std::io::stdout().lock());
    while let Ok(first) = rx.recv() {
        if out.write_all(first.as_bytes()).is_err() {
            return;
        }
        while let Ok(more) = rx.try_recv() {
            if out.write_all(more.as_bytes()).is_err() {
                return;
            }
        }
        if out.flush().is_err() {
            return;
        }
    }
}

/// journalctl: the running system's, else the usual places. A test may
/// name one, but only when this doesn't run as root.
fn find_journalctl(named: Option<OsString>) -> Option<PathBuf> {
    if let Some(p) = named {
        return Some(p.into());
    }
    let fixed = ["/run/current-system/sw/bin/journalctl", "/usr/bin/journalctl", "/bin/journalctl"];
    fixed.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// Run journalctl, sending each line of its output as `kind`.
fn run(id: u64, journalctl: Option<PathBuf>, args: Vec<String>, kind: char, tx: Sender<String>, running: Running) {
    let Some(journalctl) = journalctl else {
        return send_failure(&tx, id, "journalctl wasn't found");
    };
    let mut spawned = Command::new(&journalctl);
    spawned
        .args(&args)
        .env_clear()
        .env("LC_ALL", "C.UTF-8")
        .env("SYSTEMD_COLORS", "0")
        .env("SYSTEMD_PAGER", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // journalctl ends with this helper, however it ends: a follow never
    // would on its own.
    let me = std::process::id();
    // SAFETY: between fork and exec this calls only prctl, getppid and
    // _exit, which are async-signal-safe.
    let spawned = unsafe {
        spawned.pre_exec(move || {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0);
            if libc::getppid() as u32 != me {
                libc::_exit(1);
            }
            Ok(())
        })
    }
    .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return send_failure(&tx, id, &format!("journalctl couldn't start: {e}")),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let slot = Arc::new(Mutex::new(Some(child)));
    running.lock().unwrap().insert(id, slot.clone());
    std::thread::spawn(move || {
        // What journalctl says when it fails, for the window.
        let complaint = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(e) = stderr {
                let _ = e.take(8192).read_to_string(&mut s);
            }
            s
        });
        if let Some(out) = stdout {
            if kind == 'b' {
                let mut all = String::new();
                let _ = BufReader::new(out).take(16 << 20).read_to_string(&mut all);
                let all = all.trim();
                if !all.is_empty() {
                    let _ = tx.send(line(id, 'b', all));
                }
            } else {
                // Lines in bundles of up to 64 KiB: one message for many.
                let mut reader = BufReader::with_capacity(256 << 10, out);
                let mut bundle = String::with_capacity(64 << 10);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let text = String::from_utf8_lossy(&buf);
                    let text = text.trim_end_matches(['\n', '\r']);
                    if text.is_empty() {
                        continue;
                    }
                    bundle.push_str(&id.to_string());
                    bundle.push(' ');
                    bundle.push(kind);
                    bundle.push(' ');
                    bundle.push_str(text);
                    bundle.push('\n');
                    // Send once nothing more is ready, or the bundle is big.
                    if bundle.len() >= 64 << 10 || reader.buffer().is_empty() {
                        if tx.send(std::mem::take(&mut bundle)).is_err() {
                            break;
                        }
                        bundle.reserve(64 << 10);
                    }
                }
                if !bundle.is_empty() {
                    let _ = tx.send(bundle);
                }
            }
        }
        let status = slot.lock().unwrap().as_mut().map(Child::wait);
        let cancelled = running.lock().unwrap().remove(&id).is_none();
        let complaint = complaint.join().unwrap_or_default();
        match status {
            Some(Ok(s)) if !s.success() && !cancelled => {
                let why = complaint.trim();
                send_failure(&tx, id, if why.is_empty() { "journalctl failed" } else { why });
            }
            _ => {
                let _ = tx.send(line(id, 'd', ""));
            }
        }
    });
}

/// Whether this process can read the system's journal files, not only
/// its user's.
fn can_read_system_journal() -> bool {
    let Ok(id) = std::fs::read_to_string("/etc/machine-id") else { return false };
    let id = id.trim();
    ["/var/log/journal", "/run/log/journal"].iter().any(|base| {
        let Ok(dir) = std::fs::read_dir(Path::new(base).join(id)) else { return false };
        dir.flatten().any(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("system") && name.ends_with(".journal") && readable(&e.path())
        })
    })
}

fn readable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: c is a valid NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::R_OK) == 0 }
}

/// Logs that aren't text, or that Nisshi reads another way.
fn skipped(name: &str) -> bool {
    const BINARY: [&str; 6] = ["wtmp", "btmp", "lastlog", "faillog", "utmp", "lastlog.migrated"];
    BINARY.contains(&name)
        || name.ends_with(".journal")
        || name.ends_with(".journal~")
        || [".gz", ".xz", ".zst", ".bz2", ".lz4", ".zip", ".db", ".sqlite"].iter().any(|x| name.ends_with(x))
}

/// The text logs under the directory, three folders deep, newest first.
fn list_files(dir: &Path) -> Vec<FileInfo> {
    let mut out = Vec::new();
    let mut todo = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(meta) = e.path().symlink_metadata() else { continue };
            if meta.is_dir() {
                if depth < 3 && name != "journal" {
                    todo.push((e.path(), depth + 1));
                }
            } else if meta.is_file() && !skipped(&name) {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_micros() as i64);
                out.push(FileInfo {
                    path: e.path().display().to_string(),
                    size: meta.size(),
                    modified,
                    readable: readable(&e.path()),
                });
            }
        }
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.modified));
    out
}

/// Send the last `max` bytes of a log file under `dir`, a line at a time.
fn read_file(dir: &Path, path: &Path, max: u64, id: u64, tx: &Sender<String>) -> Result<(), String> {
    let base = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let real = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    if !real.starts_with(&base) || real == base {
        return Err(format!("{} isn't a log under {}", path.display(), dir.display()));
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&real)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // What was opened is still under the directory, and a plain file.
    let opened = std::fs::read_link(format!("/proc/self/fd/{}", std::os::fd::AsRawFd::as_raw_fd(&f)))
        .unwrap_or_else(|_| real.clone());
    let meta = f.metadata().map_err(|e| e.to_string())?;
    if !opened.starts_with(&base) || !meta.is_file() {
        return Err(format!("{} isn't a log file", path.display()));
    }
    let len = meta.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut reader = BufReader::with_capacity(256 << 10, f.take(max));
    let mut buf = Vec::new();
    let mut first = true;
    let mut checked = false;
    let mut bundle = String::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
        // Started mid-file: the first line is a part of one.
        if std::mem::take(&mut first) && start > 0 {
            continue;
        }
        if !checked {
            checked = true;
            if buf.contains(&0) {
                return Err(format!("{} isn't text", path.display()));
            }
        }
        let text = String::from_utf8_lossy(&buf);
        bundle.push_str(&line(id, 't', text.trim_end_matches(['\n', '\r'])));
        if bundle.len() >= 64 << 10 {
            tx.send(std::mem::take(&mut bundle)).map_err(|e| e.to_string())?;
        }
    }
    if !bundle.is_empty() {
        tx.send(bundle).map_err(|e| e.to_string())?;
    }
    Ok(())
}
