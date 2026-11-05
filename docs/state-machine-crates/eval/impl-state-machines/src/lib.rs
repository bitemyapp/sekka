//! The six targets with state-machines 0.34.0 (one DSL generating a
//! typestate machine and a runtime `Dynamic*` wrapper over it).
//! One module per target; each exports its machine type (`pointer::Pointer`,
//! `wrap::Wrap`, `polkit::Polkit`, `transfer::Transfer`, `lock::Lock`,
//! `install::Install`) and `IMPLEMENTED`. See `IMPLEMENTING.md`.

pub mod install;
pub mod lock;
mod out;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;
