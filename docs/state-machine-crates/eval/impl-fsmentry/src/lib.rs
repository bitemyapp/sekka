//! The six targets with fsmentry 0.4.0 (graph DSL generating an entry API of legal-edge handles).
//! One module per target; each exports its machine type (`pointer::Pointer`,
//! `wrap::Wrap`, `polkit::Polkit`, `transfer::Transfer`, `lock::Lock`,
//! `install::Install`) and `IMPLEMENTED`. See `IMPLEMENTING.md`.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;
