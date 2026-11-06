// SPDX-License-Identifier: GPL-3.0-or-later
//! The documents Sekka's Mac installer passes between its parts, and the
//! rules for reading them.
//!
//! | Document | Schema | Written by | Read by |
//! | --- | --- | --- | --- |
//! | [`HardwareProfile`] | `sekka.mac.profile/1` | Sekka (reviewed source) | app, helper, engine |
//! | [`Catalog`] in a [`SignedCatalog`] | `sekka.mac.catalog/1` | release tooling | app, helper |
//! | [`Inspection`] | `sekka.mac.inspection/1` | engine | app, helper |
//! | [`Plan`] | `sekka.mac.plan/1` | engine, recomputed by helper | app, helper |
//! | [`Confirmation`] | `sekka.mac.confirmation/1` | app | helper |
//! | [`Record`] | `sekka.mac.record/1` | helper, Recovery | helper, app, Recovery |
//! | [`EngineMessage`] lines | `sekka.mac.engine/1` | engine | helper, app |
//! | [`RecoveryConfig`] | `sekka.mac.recovery/1` | engine | Recovery app |
//! | [`Handoff`] | `sekka.mac.handoff/1` | engine | Linux first-boot setup |
//!
//! Every document is canonical JSON ([`canonical`]) with a `schema` field
//! naming its kind and version, checked before anything else. A reader
//! refuses a version it doesn't know rather than guessing at it. Public
//! documents (catalogs, profiles) and private ones (plans, records) are
//! separate types, and no type here has a field for a password or any other
//! secret: those travel only over the helper's private connection and
//! Recovery's prompt, never in a document.
#![forbid(unsafe_code)]

pub mod canonical;
mod catalog;
mod engine;
mod handoff;
mod helper;
mod ids;
mod inspection;
mod plan;
mod profile;
mod random;
mod record;
mod recovery;
#[cfg(any(test, feature = "samples"))]
pub mod samples;
mod state;

pub use catalog::*;
pub use engine::*;
pub use handoff::*;
pub use helper::*;
pub use ids::*;
pub use inspection::*;
pub use plan::*;
pub use profile::*;
pub use record::*;
pub use recovery::*;
pub use state::*;

/// Why a document was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("the document is {actual} bytes; at most {limit} are accepted")]
    TooLarge { limit: usize, actual: usize },
    #[error("the document isn't JSON: {0}")]
    Malformed(String),
    #[error("expected a {expected} document, found {found:?}")]
    UnsupportedSchema { expected: String, found: String },
    #[error("the document isn't in canonical form")]
    NotCanonical,
    #[error("{0}")]
    Invalid(String),
}

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

/// A zero-sized `schema` field that serializes as one fixed string and
/// refuses any other. The crate using it needs `serde`.
#[macro_export]
macro_rules! schema_tag {
    ($name:ident, $value:literal) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name;
        impl $name {
            pub const VALUE: &'static str = $value;
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str($value)
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                if s == $value {
                    Ok($name)
                } else {
                    Err(<D::Error as serde::de::Error>::custom(format!(
                        "unsupported schema {s:?}; expected {}",
                        $value
                    )))
                }
            }
        }
    };
}

/// Size limits for each document, checked before parsing.
pub mod limits {
    pub const PROFILE: usize = 64 * 1024;
    pub const CATALOG: usize = 1024 * 1024;
    pub const INSPECTION: usize = 512 * 1024;
    pub const PLAN: usize = 64 * 1024;
    pub const CONFIRMATION: usize = 4 * 1024;
    pub const RECORD: usize = 64 * 1024;
    pub const ENGINE_LINE: usize = 64 * 1024;
    pub const RECOVERY: usize = 64 * 1024;
    pub const HANDOFF: usize = 16 * 1024;
    pub const HELPER_MESSAGE: usize = 1024 * 1024;
}

/// One GiB, in bytes. Sizes in documents are always bytes.
pub const GIB: u64 = 1024 * 1024 * 1024;
/// One gigabyte, as Finder and Disk Utility count.
pub const GB: u64 = 1_000_000_000;
