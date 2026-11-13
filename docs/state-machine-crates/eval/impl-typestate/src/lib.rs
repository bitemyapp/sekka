//! Baseline B: hand-written typestate. Each state is its own type with
//! private fields; transitions consume it and return the next state (or a
//! small enum of the possible ones); a runtime wrapper enum (`Any`) routes
//! the externally driven events to them. The typed API is public so the
//! compile-fail suite can probe what it rejects.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;
