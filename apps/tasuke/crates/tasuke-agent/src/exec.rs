//! Running a command for a tool: no stdin (or a given one), its own process
//! group, a timeout, stdout and stderr together with their middle cut when
//! long, and a cancel that kills the whole group.

use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The most output a tool returns: the first and last halves are kept.
pub(crate) const OUTPUT_CAP: usize = 32 * 1024;

/// How a command ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Run {
    /// None when it was killed by a signal (the timeout, a cancel).
    pub exit_code: Option<i32>,
    pub output: String,
    pub truncated: bool,
    pub timed_out: bool,
    pub cancelled: bool,
}

/// Output kept as it arrives: the first half of the cap, and a rolling
/// window of the last half.
#[derive(Default)]
struct Capped {
    head: Vec<u8>,
    tail: Vec<u8>,
    total: usize,
}

impl Capped {
    fn push(&mut self, mut data: &[u8]) {
        self.total += data.len();
        let room = (OUTPUT_CAP / 2).saturating_sub(self.head.len());
        let n = room.min(data.len());
        self.head.extend_from_slice(&data[..n]);
        data = &data[n..];
        if data.is_empty() {
            return;
        }
        self.tail.extend_from_slice(data);
        let half = OUTPUT_CAP / 2;
        if self.tail.len() > half * 2 {
            let cut = self.tail.len() - half;
            self.tail.drain(..cut);
        }
    }

    fn finish(mut self) -> (String, bool) {
        let half = OUTPUT_CAP / 2;
        if self.tail.len() > half {
            let cut = self.tail.len() - half;
            self.tail.drain(..cut);
        }
        let kept = self.head.len() + self.tail.len();
        if kept >= self.total {
            let mut all = self.head;
            all.extend_from_slice(&self.tail);
            return (String::from_utf8_lossy(&all).into_owned(), false);
        }
        let head = String::from_utf8_lossy(&self.head);
        let tail = String::from_utf8_lossy(&self.tail);
        (format!("{head}\n… [{} bytes cut from the middle] …\n{tail}", self.total - kept), true)
    }
}

/// Run `argv` with `env` added to this process's environment.
pub(crate) fn run(
    argv: &[String],
    cwd: Option<&std::path::Path>,
    env: &[(&str, &str)],
    stdin: Option<&[u8]>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> io::Result<Run> {
    let Some((program, args)) = argv.split_first() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no command"));
    };
    let (reader, writer) = io::pipe()?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .process_group(0);
    // Nothing inherited may redefine the programs a command names (the
    // policy judged it as written): no BASH_ENV, ENV or exported functions.
    cmd.env_remove("BASH_ENV").env_remove("ENV");
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("BASH_FUNC_") {
            cmd.env_remove(&k);
        }
    }
    if let Some(dir) = cwd.filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn()?;
    // The pipe's write ends now belong to the child alone, so reading ends
    // when it (and whatever it left running with them) closes them.
    drop(cmd);
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let data = data.to_vec();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        });
    }
    let captured = Arc::new(Mutex::new(Capped::default()));
    let reading = {
        let captured = captured.clone();
        let mut reader = reader;
        std::thread::Builder::new().name("tasuke-exec-read".into()).spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => captured.lock().unwrap_or_else(|e| e.into_inner()).push(&buf[..n]),
                }
            }
        })?
    };
    let pgid = child.id() as libc::pid_t;
    let started = Instant::now();
    let mut run = Run::default();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let late = started.elapsed() > timeout;
        if late || cancel.load(Ordering::SeqCst) {
            run.timed_out = late;
            run.cancelled = !late;
            kill_group(pgid);
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    run.exit_code = status.code();
    // A daemon it started in the background may hold the pipe open: give
    // the output a moment to drain, then stop waiting for it.
    let drain_until = Instant::now() + Duration::from_millis(500);
    while !reading.is_finished() && Instant::now() < drain_until {
        std::thread::sleep(Duration::from_millis(10));
    }
    let captured = std::mem::take(&mut *captured.lock().unwrap_or_else(|e| e.into_inner()));
    (run.output, run.truncated) = captured.finish();
    Ok(run)
}

/// SIGTERM to the group, then SIGKILL if anything is left after a second.
fn kill_group(pgid: libc::pid_t) {
    // SAFETY: plain syscalls on a process group this module created.
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        // SAFETY: signal 0 only checks whether the group still exists.
        if unsafe { libc::kill(-pgid, 0) } != 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: as above.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
}

/// What the model sees of a run.
pub(crate) fn describe(run: &Run, timeout: Duration) -> String {
    let mut out = match run.exit_code {
        Some(code) => format!("exit status {code}\n"),
        None if run.timed_out => format!("stopped after {} s (the time limit)\n", timeout.as_secs()),
        None if run.cancelled => "stopped: the person cancelled\n".to_owned(),
        None => "ended by a signal\n".to_owned(),
    };
    if run.output.trim().is_empty() {
        out.push_str("(no output)");
    } else {
        out.push_str(&run.output);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, timeout: Duration) -> Run {
        let argv = vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()];
        run(&argv, None, &[], None, timeout, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn output_and_status() {
        let r = sh("echo out; echo err >&2; exit 3", Duration::from_secs(5));
        assert_eq!(r.exit_code, Some(3));
        assert!(r.output.contains("out") && r.output.contains("err"));
        assert!(!r.truncated && !r.timed_out);
    }

    #[test]
    fn long_output_keeps_its_head_and_tail() {
        let r = sh("echo FIRST; head -c 200000 /dev/zero | tr '\\0' x; echo; echo LAST", Duration::from_secs(10));
        assert!(r.truncated);
        assert!(r.output.starts_with("FIRST"));
        assert!(r.output.trim_end().ends_with("LAST"));
        assert!(r.output.len() < OUTPUT_CAP + 200);
        assert!(r.output.contains("bytes cut from the middle"));
    }

    #[test]
    fn the_timeout_kills_the_group() {
        let t0 = Instant::now();
        let r = sh("sleep 30 & sleep 30", Duration::from_millis(300));
        assert!(r.timed_out);
        assert_eq!(r.exit_code, None);
        assert!(t0.elapsed() < Duration::from_secs(5));
        assert!(describe(&r, Duration::from_secs(1)).contains("time limit"));
    }

    #[test]
    fn a_background_daemon_does_not_hold_the_tool() {
        let t0 = Instant::now();
        let r = sh("(sleep 5 &) ; echo started", Duration::from_secs(10));
        assert_eq!(r.exit_code, Some(0));
        assert!(r.output.contains("started"));
        assert!(t0.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn stdin_is_given_or_empty() {
        let argv = vec!["cat".to_owned()];
        let r = run(&argv, None, &[], Some(b"hello"), Duration::from_secs(5), &AtomicBool::new(false)).unwrap();
        assert_eq!(r.output, "hello");
        let r = sh("cat; echo done", Duration::from_secs(5));
        assert_eq!(r.output.trim(), "done");
    }

    #[test]
    fn cancel_stops_it() {
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c.store(true, Ordering::SeqCst);
        });
        let argv = vec!["sleep".to_owned(), "30".to_owned()];
        let r = run(&argv, None, &[], None, Duration::from_secs(60), &cancel).unwrap();
        assert!(r.cancelled && !r.timed_out);
    }
}
