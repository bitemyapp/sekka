//! What can go wrong reading and changing what's installed and which app
//! opens what. `aizome-sys` turns it into its own `Error`, variant for
//! variant.

use std::fmt;

/// What went wrong with an application, a desktop entry or a default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The system configuration pins this file (a `mimeapps.list` linked
    /// into `/nix/store` by Home Manager), so it can't be changed here.
    ManagedBySystem(String),
    /// An argument is not acceptable: a malformed desktop entry or `Exec`
    /// line, a string that is not a MIME type, an app that is not a
    /// terminal.
    InvalidArgument(String),
    /// No installed application has this ID.
    NotFound(String),
    /// Reading or writing a file failed, and how.
    Io { kind: std::io::ErrorKind, message: String },
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ManagedBySystem(what) => write!(f, "Managed by the system configuration ({what})"),
            Error::InvalidArgument(what) => f.write_str(what),
            Error::NotFound(what) => write!(f, "Not found: {what}"),
            Error::Io { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        match error.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(error.to_string()),
            std::io::ErrorKind::ReadOnlyFilesystem => Error::ManagedBySystem(error.to_string()),
            kind => Error::Io { kind, message: error.to_string() },
        }
    }
}
