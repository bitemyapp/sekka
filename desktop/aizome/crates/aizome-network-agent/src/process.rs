//! Bounded, cancellable workers. Credentials travel only through pipes,
//! never arguments, environment variables, files or diagnostic output.
use crate::{AgentError, Result};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub const MAX_BYTES: usize = 1024 * 1024;
/// How long NetworkManager waits for GetSecrets, and for SaveSecrets and
/// DeleteSecrets. It gives up after that without cancelling.
pub const GET_SECRETS_TIMEOUT: Duration = Duration::from_secs(120);
pub const SAVE_SECRETS_TIMEOUT: Duration = Duration::from_secs(60);

/// One request's cancellation and deadline, shared by all of its steps.
#[derive(Clone)]
pub struct Cancel {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl Default for Cancel {
    fn default() -> Self {
        Self::within(GET_SECRETS_TIMEOUT)
    }
}

impl Cancel {
    pub fn within(timeout: Duration) -> Self {
        Self { cancelled: Arc::default(), deadline: Instant::now() + timeout }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(AgentError::AgentCanceled("Network authentication was cancelled".into()))
        } else if Instant::now() >= self.deadline {
            Err(AgentError::AgentCanceled("Network authentication timed out".into()))
        } else {
            Ok(())
        }
    }
}

fn nonblocking(fd: &impl AsRawFd) {
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

/// Read what the pipe holds now; true once it is closed.
fn drain(pipe: &mut impl Read, output: &mut Vec<u8>) -> bool {
    let mut buffer = [0; 8192];
    while output.len() <= MAX_BYTES {
        match pipe.read(&mut buffer) {
            Ok(0) => return true,
            Ok(n) => output.extend_from_slice(&buffer[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return e.kind() != ErrorKind::WouldBlock,
        }
    }
    false
}

/// Ends a worker and everything it started, then reaps it.
fn abort(child: &mut Child) {
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();
}

/// Runs a worker with `input` on its standard input and returns its
/// output. A non-zero exit is reported as `failed`: Aizome's own workers
/// answer a cancelled dialog in their output, while VPN helpers exit
/// non-zero when the person cancels.
pub fn run(command: &mut Command, input: Vec<u8>, cancel: &Cancel, failed: AgentError) -> Result<Vec<u8>> {
    cancel.check()?;
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    // A plugin's descendants must not outlive cancellation either.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|_| AgentError::Failed("Could not start credential helper".into()))?;
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    stdin.iter().for_each(nonblocking);
    stdout.iter().for_each(nonblocking);
    let mut written = 0;
    let mut output = Vec::new();
    let status = loop {
        if let Err(e) = cancel.check() {
            abort(&mut child);
            return Err(e);
        }
        if let Some(pipe) = &mut stdin {
            match pipe.write(&input[written..]) {
                Ok(n) => written += n,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                // The worker closed its input.
                Err(_) => written = input.len(),
            }
            if written == input.len() {
                stdin = None;
            }
        }
        if let Some(pipe) = &mut stdout
            && drain(pipe, &mut output)
        {
            stdout = None;
        }
        if output.len() > MAX_BYTES {
            abort(&mut child);
            return Err(AgentError::Failed("Invalid credential helper response".into()));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let mut fds: Vec<_> = stdin
                    .iter()
                    .map(|p| libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLOUT, revents: 0 })
                    .chain(stdout.iter().map(|p| libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLIN, revents: 0 }))
                    .collect();
                // Exits aren't polled: look again shortly.
                unsafe {
                    libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 25);
                }
            }
            Err(_) => {
                abort(&mut child);
                return Err(AgentError::Failed("Credential helper failed".into()));
            }
        }
    };
    // All the worker wrote is in the pipe now. What it started keeps
    // running, and may hold the pipe open, so take only what is there:
    // a VPN helper may have opened a browser for single sign-on.
    if let Some(pipe) = &mut stdout {
        drain(pipe, &mut output);
    }
    cancel.check()?;
    if output.len() > MAX_BYTES {
        return Err(AgentError::Failed("Invalid credential helper response".into()));
    }
    if !status.success() {
        return Err(failed);
    }
    Ok(output)
}
