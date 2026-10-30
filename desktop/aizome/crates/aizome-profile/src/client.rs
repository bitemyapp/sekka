//! A short-lived recorder process keeps exporter/symbol heap and code pages
//! outside Myaku. The GUI calls this on a temporary control thread.
use crate::{Failure, Message, Mode, Options, Outcome};
use std::io::{self, Read};
use std::os::{fd::AsRawFd, unix::process::CommandExt};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct Recorder(Child);
impl Drop for Recorder {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            // Only signal an unreaped child: a cached numeric PID can be reused.
            unsafe {
                libc::kill(self.0.id() as i32, libc::SIGINT);
            }
            let until = Instant::now() + Duration::from_secs(2);
            while matches!(self.0.try_wait(), Ok(None)) {
                if Instant::now() >= until {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

pub fn record_process(
    options: Options,
    stop: Arc<AtomicBool>,
    mut progress: impl FnMut(String),
) -> Result<Outcome, Failure> {
    let fail = |e: io::Error| Failure { message: format!("Recorder: {e}"), path: None };
    if stop.load(Ordering::Acquire) {
        return Err(fail(io::Error::other("cancelled")));
    }
    let executable = std::env::current_exe().map_err(fail)?;
    let executable = executable
        .parent()
        .ok_or_else(|| fail(io::Error::other("missing executable directory")))?
        .join("aizome-profile");
    let mut command = Command::new(executable);
    command
        .arg("--json")
        .arg(if options.mode == Mode::Cpu { "cpu" } else { "timeline" })
        .arg(options.seconds.to_string());
    for target in options.targets {
        command.arg(format!("{}:{}", target.pid, target.start_ticks));
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::other("capture owner exited"));
            }
            Ok(())
        });
    }
    let mut child = Recorder(command.spawn().map_err(fail)?);
    let mut pipe = child.0.stdout.take().unwrap();
    if unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
        return Err(fail(io::Error::last_os_error()));
    }
    let until = Instant::now() + Duration::from_secs(options.seconds.min(60) + 10);
    let mut pending = Vec::new();
    let mut finished = None;
    let mut cancelled = false;
    loop {
        let mut buf = [0u8; 4096];
        for _ in 0..16 {
            match pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => pending.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(fail(e)),
            }
            if pending.len() > 64 * 1024 {
                return Err(fail(io::Error::other("invalid recorder response size")));
            }
        }
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            let message: Message = serde_json::from_slice(&pending[..end]).map_err(|e| fail(io::Error::other(e)))?;
            pending.drain(..=end);
            match message {
                Message::Progress { message } => progress(message),
                Message::Finished { result } => finished = Some(result),
            }
        }
        if let Some(status) = child.0.try_wait().map_err(fail)? {
            // Final bytes can arrive between read and waitpid. With the child
            // reaped its pipe is at EOF; include any partially read line.
            if let Some(result) = finished {
                return result;
            }
            pipe.take((64 * 1024usize).saturating_sub(pending.len()) as u64).read_to_end(&mut pending).map_err(fail)?;
            for line in pending.split(|b| *b == b'\n') {
                if let Ok(Message::Finished { result }) = serde_json::from_slice(line) {
                    return result;
                }
            }
            return Err(fail(io::Error::other(format!("exited without a result ({status})"))));
        }
        if stop.load(Ordering::Acquire) && !cancelled {
            unsafe {
                libc::kill(child.0.id() as i32, libc::SIGINT);
            }
            cancelled = true;
        }
        if Instant::now() >= until {
            return Err(fail(io::Error::other("capture did not finish within its deadline")));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
