//! A chooser as a process of its own: the request on its standard input,
//! its answer on its standard output, and an end the service can force
//! when the request is closed.

use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The most a chooser may answer: thousands of URIs fit.
pub const MAX_ANSWER: usize = 16 << 20;

/// How a chooser ended.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// It answered (its output).
    Answered(Vec<u8>),
    /// The request was closed first, and the chooser ended.
    Closed,
    /// It couldn't start, failed, or answered too much.
    Failed(String),
}

/// A request's cancellation, shared by the service and the worker.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

fn nonblocking(fd: &impl AsRawFd) {
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

/// End the chooser and anything it started, and reap it.
fn abort(child: &mut Child) {
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();
}

/// Run `command` with `input` on its standard input until it exits or
/// `cancel` says the request closed.
pub fn run(command: &mut Command, input: &[u8], cancel: &Cancel) -> Outcome {
    if cancel.is_cancelled() {
        return Outcome::Closed;
    }
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    // Its own process group, so that closing the request ends all of it.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("the chooser couldn't start: {e}")),
    };
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    stdin.iter().for_each(nonblocking);
    stdout.iter().for_each(nonblocking);
    let (mut written, mut output) = (0, Vec::new());
    let mut buffer = [0; 8192];
    loop {
        if cancel.is_cancelled() {
            abort(&mut child);
            return Outcome::Closed;
        }
        if let Some(pipe) = &mut stdin {
            match pipe.write(&input[written..]) {
                Ok(n) => written += n,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                // It closed its input.
                Err(_) => written = input.len(),
            }
            if written == input.len() {
                stdin = None;
            }
        }
        if let Some(pipe) = &mut stdout {
            loop {
                match pipe.read(&mut buffer) {
                    Ok(0) => {
                        stdout = None;
                        break;
                    }
                    Ok(n) => output.extend_from_slice(&buffer[..n]),
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
                if output.len() > MAX_ANSWER {
                    abort(&mut child);
                    return Outcome::Failed("the chooser answered too much".into());
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(pipe) = &mut stdout {
                    while let Ok(n @ 1..) = pipe.read(&mut buffer) {
                        output.extend_from_slice(&buffer[..n]);
                        if output.len() > MAX_ANSWER {
                            return Outcome::Failed("the chooser answered too much".into());
                        }
                    }
                }
                return if status.success() {
                    Outcome::Answered(output)
                } else {
                    Outcome::Failed(format!("the chooser stopped: {status}"))
                };
            }
            Ok(None) => {
                let mut fds: Vec<_> = stdin
                    .iter()
                    .map(|p| libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLOUT, revents: 0 })
                    .chain(stdout.iter().map(|p| libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLIN, revents: 0 }))
                    .collect();
                // Exits and cancellation aren't polled: look again shortly.
                unsafe {
                    libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 25);
                }
            }
            Err(e) => {
                abort(&mut child);
                return Outcome::Failed(format!("the chooser couldn't be followed: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.arg("-c").arg(script);
        c
    }

    #[test]
    fn hands_over_the_request_and_takes_the_answer() {
        let out = run(&mut sh("cat"), b"{\"a\":1}", &Cancel::default());
        assert_eq!(out, Outcome::Answered(b"{\"a\":1}".to_vec()));
    }

    #[test]
    fn a_failure_is_a_failure() {
        assert!(matches!(run(&mut sh("exit 3"), b"", &Cancel::default()), Outcome::Failed(_)));
        assert!(matches!(
            run(&mut Command::new("/nonexistent/aizome-portal"), b"", &Cancel::default()),
            Outcome::Failed(_)
        ));
    }

    #[test]
    fn closing_ends_the_chooser_and_what_it_started() {
        let cancel = Cancel::default();
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            c.cancel();
        });
        let started = Instant::now();
        // A child that would outlive the shell, holding the pipe open.
        let out = run(&mut sh("sleep 30 & sleep 30"), b"", &cancel);
        assert_eq!(out, Outcome::Closed);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
