// SPDX-License-Identifier: MIT OR Apache-2.0
//! NixOS configuration, read and changed the way a careful person would.
//!
//! - [`edit`]: find a setting in a Nix file (`environment.systemPackages`,
//!   `networking.hostName`, `imports`) and change only that setting: set a
//!   list, add an import, take an entry out of a package list. The rest of
//!   the file, its comments and formatting, stays byte for byte, and every
//!   change is parsed again before it is returned.
//! - [`packages`]: the file in which Yukimi keeps what it installs for
//!   everyone, `yukimi.nix`.
//! - [`branch`]: which branch a flake input's address follows, and the same
//!   address following another.
//! - [`lock`]: `flake.lock`: which revision of each input a system is built
//!   from, how old it is, and whether a new lock file changes only what it
//!   should.
pub mod branch;
pub mod edit;
pub mod lock;
pub mod packages;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("the file does not parse as Nix: {0}")]
    Syntax(String),
    #[error("the file is not a NixOS module (a set of settings, or a function returning one)")]
    NotAModule,
    #[error("`{0}` is set to something other than a plain list of strings, so it is left alone")]
    NotAStringList(String),
    #[error("`{0}` is set to something other than a plain list, so it is left alone")]
    NotAList(String),
    #[error("`{0}` is not set to a plain string, so it is left alone")]
    NotAString(String),
    #[error("not a valid attribute name in Nixpkgs: {0:?}")]
    BadAttribute(String),
    #[error("flake.lock could not be read: {0}")]
    Lock(String),
}

pub type Result<T> = std::result::Result<T, Error>;
