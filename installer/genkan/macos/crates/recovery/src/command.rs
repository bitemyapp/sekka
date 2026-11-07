// SPDX-License-Identifier: GPL-3.0-or-later
//! Running Apple's other tools (`diskutil`, `bputil -d`, `bless --getBoot`,
//! `mount`), each with a time to answer in. A step waits on them before and
//! after its changes, and one that never answered would leave "Finishing…"
//! on the screen for good.
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long one of them may take: far longer than any does.
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// Run `program` with `args`, the system's PATH and the C locale, and
/// return what it wrote, or why not. One that hasn't finished by `timeout`
/// is stopped, with anything it started.
pub fn output(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    // Both pipes are read as they fill, so that neither stalls the tool.
    fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    }
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let mut status = None;
    // Until it has exited and everything it wrote has been read: something
    // it left running can hold the pipes open after it.
    while status.is_none() || !stdout.is_finished() || !stderr.is_finished() {
        if status.is_none() {
            status = child.try_wait().map_err(|e| format!("{program}: {e}"))?;
        }
        if Instant::now() >= deadline {
            // SAFETY: signalling the process group the tool leads.
            unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            if status.is_none() {
                let _ = child.wait();
            }
            return Err(format!("{program} didn't finish in time."));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let (stdout, stderr) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    if !status.is_some_and(|s| s.success()) {
        return Err(format!("{program} failed: {}", String::from_utf8_lossy(&stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SH: &str = "/bin/sh";

    fn sh() -> bool {
        std::path::Path::new(SH).exists()
    }

    #[test]
    fn what_a_tool_writes_is_returned() {
        if !sh() {
            return;
        }
        assert_eq!(output(SH, &["-c", "echo out; echo err >&2"], Duration::from_secs(10)).unwrap(), "out\n");
        let e = output(SH, &["-c", "echo 'no such volume' >&2; exit 1"], Duration::from_secs(10)).unwrap_err();
        assert_eq!(e, "/bin/sh failed: no such volume");
    }

    #[test]
    fn a_tool_that_doesnt_answer_is_stopped() {
        if !sh() {
            return;
        }
        // Only the shell's own commands: a build sandbox's PATH is empty.
        for script in ["while :; do :; done", "(while :; do :; done) & echo started"] {
            let started = Instant::now();
            let e = output(SH, &["-c", script], Duration::from_millis(300)).unwrap_err();
            assert!(e.contains("didn't finish in time"), "{e}");
            assert!(started.elapsed() < Duration::from_secs(5), "{script}");
        }
    }
}
