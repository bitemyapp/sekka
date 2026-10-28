//! Linux I/O boundaries for Hotaru.
//!
//! The interface thread must never wait for a clipboard peer or terminal process.
//! PTYs expose nonblocking descriptors; clipboard transfers advance by a bounded
//! byte budget and close on cancellation, timeout, or peer failure. The caller
//! supplies its poll/epoll reactor. No clipboard contents enter diagnostics.

#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(target_os = "linux"))]
compile_error!("Hotaru supports Linux only");

pub mod clipboard;
mod fd;
pub mod launch;
pub mod pty;

pub use clipboard::{Cancellation, ClipboardReader, ClipboardWriter, TransferState};
pub use pty::{Pty, PtyCommand, PtySize};
