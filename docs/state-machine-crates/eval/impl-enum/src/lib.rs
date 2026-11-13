//! Baseline A: a plain `enum` per machine with `match`, data in the
//! variants, `mem::replace` for owned moves. This is the reference the
//! differential property tests compare every implementation against.

pub mod install;
pub mod lock;
pub mod pointer;
pub mod polkit;
pub mod transfer;
pub mod wrap;
