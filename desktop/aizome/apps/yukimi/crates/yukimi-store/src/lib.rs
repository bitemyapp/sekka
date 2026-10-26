// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Nix store, read the way Nix itself reads it.
//!
//! - [`path`]: store paths: their hash, package name, version and output, and
//!   Nix's version ordering.
//! - [`db`]: the store database (`/nix/var/nix/db/db.sqlite`): which paths are
//!   valid, their sizes and references, loaded into a [`db::Graph`] for
//!   closures and reverse dependencies.
//! - [`roots`]: garbage-collector roots: what keeps each path alive, sorted
//!   into the kinds a person cares about (system generations, profiles,
//!   build results).
//!
//! Everything here only reads. The store database is world-readable on
//! NixOS, so none of it needs root.
pub mod db;
pub mod path;
pub mod roots;

pub use db::{Graph, PathId, StoreDb};
pub use path::{StorePath, compare_versions};
pub use roots::{Root, RootKind};

/// Where the store lives.
pub const STORE_DIR: &str = "/nix/store";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the store database could not be read: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
