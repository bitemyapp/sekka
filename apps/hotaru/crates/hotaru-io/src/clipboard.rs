//! Readiness-driven clipboard pipe transfers.
//!
//! Wayland clipboard pipes have finite capacity. A compositor and its client can
//! deadlock if either waits for the other while dispatching selection requests.
//! These state machines never wait: schedule `pump` after descriptor readiness,
//! plus a timer for the deadline, and keep dispatching the Wayland connection.

use crate::fd::{BlockSigpipe, nonblocking};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_SYSCALLS_PER_PUMP: usize = 32;
const WRITE_CHUNK: usize = 64 * 1024;
const READ_CHUNK: usize = 16 * 1024;

#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferState {
    Pending,
    Complete,
    Cancelled,
    TimedOut,
    ReceiverClosed,
    LimitExceeded,
    Failed,
}

struct Transfer {
    fd: Option<OwnedFd>,
    cancellation: Cancellation,
    deadline: Instant,
    state: TransferState,
}

impl Transfer {
    fn new(fd: OwnedFd, timeout: Duration) -> io::Result<Self> {
        nonblocking(fd.as_fd())?;
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "clipboard timeout is too large",
            )
        })?;
        Ok(Self {
            fd: Some(fd),
            cancellation: Cancellation::default(),
            deadline,
            state: TransferState::Pending,
        })
    }

    fn finish(&mut self, state: TransferState) -> TransferState {
        self.fd.take();
        self.state = state;
        state
    }

    fn check(&mut self) -> TransferState {
        if self.state != TransferState::Pending {
            self.state
        } else if self.cancellation.is_cancelled() {
            self.finish(TransferState::Cancelled)
        } else if Instant::now() >= self.deadline {
            self.finish(TransferState::TimedOut)
        } else {
            TransferState::Pending
        }
    }
}

/// Owns a pipe write end and an immutable snapshot. Cloning an `Arc` for another
/// request does not copy a multi-MiB selection. The snapshot is released when the
/// transfer ends, including on cancellation and timeout.
pub struct ClipboardWriter {
    transfer: Transfer,
    payload: Option<Arc<[u8]>>,
    written: usize,
    length: usize,
}

impl ClipboardWriter {
    /// Takes exclusive responsibility for this descriptor's blocking mode and
    /// lifetime. Other duplicated descriptors share its O_NONBLOCK flag.
    pub fn new(fd: OwnedFd, payload: Arc<[u8]>, timeout: Duration) -> io::Result<Self> {
        let length = payload.len();
        Ok(Self {
            transfer: Transfer::new(fd, timeout)?,
            payload: Some(payload),
            written: 0,
            length,
        })
    }

    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.transfer.fd.as_ref().map(AsFd::as_fd)
    }

    pub fn cancellation(&self) -> Cancellation {
        self.transfer.cancellation.clone()
    }

    pub fn deadline(&self) -> Instant {
        self.transfer.deadline
    }

    pub fn state(&self) -> TransferState {
        self.transfer.state
    }

    pub fn bytes_written(&self) -> usize {
        self.written
    }

    fn finish(&mut self, state: TransferState) -> TransferState {
        self.payload.take();
        self.transfer.finish(state)
    }

    /// Performs at most `byte_budget` bytes and 32 nonblocking write attempts.
    /// `Pending` includes EAGAIN: wait for readiness instead of calling in a loop.
    /// A final call closes this pipe writer. The reader observes EOF once all
    /// duplicated or inherited write descriptors have also closed.
    pub fn pump(&mut self, byte_budget: usize) -> io::Result<TransferState> {
        let state = self.transfer.check();
        if state != TransferState::Pending {
            return Ok(self.finish(state));
        }
        if self.written == self.length {
            return Ok(self.finish(TransferState::Complete));
        }
        if byte_budget == 0 {
            return Ok(TransferState::Pending);
        }
        let sigpipe = match BlockSigpipe::new() {
            Ok(guard) => guard,
            Err(error) => {
                self.finish(TransferState::Failed);
                return Err(error);
            }
        };
        let mut remaining = byte_budget;
        for _ in 0..MAX_SYSCALLS_PER_PUMP {
            let state = self.transfer.check();
            if state != TransferState::Pending {
                return Ok(self.finish(state));
            }
            let count = remaining.min(WRITE_CHUNK).min(self.length - self.written);
            let payload = self
                .payload
                .as_ref()
                .expect("pending transfer owns its data");
            let fd = self
                .transfer
                .fd
                .as_ref()
                .expect("pending transfer owns its fd");
            // SAFETY: fd is live and nonblocking; the immutable Arc owns the
            // entire requested readable slice for the duration of the syscall.
            let result = unsafe {
                libc::write(
                    fd.as_raw_fd(),
                    payload[self.written..].as_ptr().cast(),
                    count,
                )
            };
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(TransferState::Pending);
                }
                if error.raw_os_error() == Some(libc::EPIPE) {
                    sigpipe.consume_generated_signal();
                    return Ok(self.finish(TransferState::ReceiverClosed));
                }
                self.finish(TransferState::Failed);
                return Err(error);
            }
            if result == 0 {
                self.finish(TransferState::Failed);
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "clipboard write made no progress",
                ));
            }
            self.written += result as usize;
            remaining -= result as usize;
            if self.written == self.length {
                return Ok(self.finish(TransferState::Complete));
            }
            if remaining == 0 {
                break;
            }
        }
        Ok(TransferState::Pending)
    }
}

/// Bounded receiving side. Oversized or stalled offers close their pipe instead
/// of retaining an unbounded Vec or blocking the UI dispatch thread.
pub struct ClipboardReader {
    transfer: Transfer,
    bytes: Vec<u8>,
    maximum: usize,
}

impl ClipboardReader {
    pub fn new(fd: OwnedFd, maximum: usize, timeout: Duration) -> io::Result<Self> {
        Ok(Self {
            transfer: Transfer::new(fd, timeout)?,
            bytes: Vec::new(),
            maximum,
        })
    }

    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.transfer.fd.as_ref().map(AsFd::as_fd)
    }

    pub fn cancellation(&self) -> Cancellation {
        self.transfer.cancellation.clone()
    }

    pub fn deadline(&self) -> Instant {
        self.transfer.deadline
    }

    pub fn state(&self) -> TransferState {
        self.transfer.state
    }

    /// Includes partial data until `state()` is Complete. Callers must not treat
    /// an incomplete, cancelled, or oversized offer as a successful paste.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns complete or partial bytes; inspect `state()` before consuming.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn pump(&mut self, byte_budget: usize) -> io::Result<TransferState> {
        let mut remaining = byte_budget;
        let mut buffer = [0_u8; READ_CHUNK];
        for _ in 0..MAX_SYSCALLS_PER_PUMP {
            let state = self.transfer.check();
            if state != TransferState::Pending {
                return Ok(state);
            }
            if remaining == 0 {
                return Ok(TransferState::Pending);
            }
            // Read at most one byte beyond the cap to distinguish an exactly
            // full offer followed by EOF from an offer exceeding the cap.
            let capacity = self
                .maximum
                .saturating_sub(self.bytes.len())
                .saturating_add(1);
            let count = remaining.min(buffer.len()).min(capacity);
            let fd = self
                .transfer
                .fd
                .as_ref()
                .expect("pending transfer owns its fd");
            // SAFETY: the live nonblocking descriptor writes at most count
            // bytes into a stack buffer whose length is at least count.
            let result = unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), count) };
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(TransferState::Pending);
                }
                self.transfer.finish(TransferState::Failed);
                return Err(error);
            }
            if result == 0 {
                return Ok(self.transfer.finish(TransferState::Complete));
            }
            let count = result as usize;
            if count > self.maximum - self.bytes.len() {
                return Ok(self.transfer.finish(TransferState::LimitExceeded));
            }
            if self.bytes.try_reserve(count).is_err() {
                self.transfer.finish(TransferState::Failed);
                return Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "clipboard allocation failed",
                ));
            }
            self.bytes.extend_from_slice(&buffer[..count]);
            remaining -= count;
        }
        Ok(TransferState::Pending)
    }
}
