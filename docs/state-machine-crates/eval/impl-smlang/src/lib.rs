//! The six targets with smlang 0.8.0 (Boost.SML-style `statemachine!` DSL, flat).
//! One module per target; each exports its machine type (`pointer::Pointer`,
//! `wrap::Wrap`, `polkit::Polkit`, `transfer::Transfer`, `lock::Lock`,
//! `install::Install`) and `IMPLEMENTED`. See `IMPLEMENTING.md`.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;
