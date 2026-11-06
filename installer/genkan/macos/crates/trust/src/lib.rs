// SPDX-License-Identifier: GPL-3.0-or-later
//! Whether to believe a release catalog.
//!
//! A catalog is accepted only when all of these hold, checked in this order
//! so the first failure is the one a person can act on:
//!
//! 1. It parses: canonical bytes, known schema, within size.
//! 2. Its key is one of this build's trust roots, valid for the catalog's
//!    channel and for when the catalog says it was issued (keys rotate by
//!    validity window; a retired key signs nothing new).
//! 3. The Ed25519 signature over [`Catalog::signed_bytes`] verifies
//!    (strictly: no malleable encodings).
//! 4. It is internally consistent ([`Catalog::check`]) and every artifact
//!    is on one of the trusted delivery hosts.
//! 5. This computer's clock is within the catalog's validity, allowing a
//!    little skew; a clock that is behind is reported as such, not as a
//!    bad catalog.
//! 6. It is not older than a catalog already accepted on its channel
//!    ([`AcceptedStore`]); one with the same serial must be the same
//!    catalog.
//!
//! Signing is for test keys only in this source tree: [`signing`] exists
//! for the release tooling and tests, and production signing keys never
//! live in the repository.
#![forbid(unsafe_code)]

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use sekka_mac_contract::{
    Catalog, Digest, SignedCatalog, SignedCatalogSchema, Timestamp, canonical, limits,
};

mod store;
pub use store::AcceptedStore;

/// A key that may sign catalogs.
#[derive(Clone, Debug)]
pub struct TrustRoot {
    pub id: &'static str,
    pub public_key: [u8; 32],
    /// Channels it may sign for.
    pub channels: &'static [&'static str],
    /// Catalogs issued before this are not its to sign.
    pub valid_from: Timestamp,
    /// Catalogs issued after this are refused: the key is retired.
    pub valid_until: Timestamp,
}

/// What a build trusts: its keys and where release bytes may come from.
#[derive(Clone, Debug)]
pub struct TrustConfig {
    pub roots: Vec<TrustRoot>,
    /// Hosts artifact URLs may name.
    pub hosts: Vec<&'static str>,
    /// How far ahead of this computer's clock a catalog's issue time may be.
    pub clock_skew: i64,
}

/// The development key: its private half is in this repository
/// (`keys/development.key`) for tests and local catalogs, so anything it
/// signs proves nothing. Only development builds trust it, and only for
/// the `development` channel.
pub const DEVELOPMENT_KEY_ID: &str = "sekka-development-2026";
pub const DEVELOPMENT_PUBLIC_KEY: [u8; 32] = include!("../keys/development.pub.rs");

/// Sekka's production catalog keys. None exists yet: a production key is
/// generated offline by the maintainers and added here in a reviewed
/// change, with its validity window. Until then an installed (production)
/// build trusts no catalog at all, which is the intended state while
/// publication is inhibited.
pub const PRODUCTION_ROOTS: &[TrustRoot] = &[];

/// Where Sekka publishes Mac release bytes.
pub const PRODUCTION_HOSTS: &[&str] = &["downloads.sekka.computer"];

impl TrustConfig {
    /// This build's trust: production roots, plus the development key
    /// unless this is a production build.
    pub fn for_this_build() -> Self {
        let mut roots = PRODUCTION_ROOTS.to_vec();
        if !cfg!(feature = "production") {
            roots.push(development_root());
        }
        Self {
            roots,
            hosts: PRODUCTION_HOSTS.to_vec(),
            clock_skew: 10 * 60,
        }
    }
}

pub fn development_root() -> TrustRoot {
    TrustRoot {
        id: DEVELOPMENT_KEY_ID,
        public_key: DEVELOPMENT_PUBLIC_KEY,
        channels: &["development"],
        valid_from: Timestamp::from_unix(1_790_000_000),
        valid_until: Timestamp::from_unix(1_900_000_000),
    }
}

/// Why a catalog was refused. Each says what a person can do about it.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrustError {
    #[error("the release catalog couldn't be read: {0}")]
    Unreadable(String),
    #[error("the release catalog is signed by a key this installer doesn't trust ({0})")]
    UnknownKey(String),
    #[error("the key {key} may not sign the {channel} channel")]
    WrongChannel { key: String, channel: String },
    #[error("the key {0} was not valid when this catalog was issued")]
    KeyOutOfWindow(String),
    #[error("the release catalog's signature doesn't verify; it may have been altered")]
    BadSignature,
    #[error("the release catalog is inconsistent: {0}")]
    Inconsistent(String),
    #[error("the release catalog names {0}, which isn't a Sekka download location")]
    UntrustedHost(String),
    #[error(
        "this Mac's clock says {now}, but the release catalog was issued at {issued}; check the date and time in System Settings"
    )]
    ClockBehind { now: Timestamp, issued: Timestamp },
    #[error("the release catalog expired at {0}; check for a newer installer")]
    Expired(Timestamp),
    #[error(
        "this catalog (number {offered}) is older than one this Mac already accepted (number {accepted}); it may be a replay"
    )]
    Rollback { offered: u64, accepted: u64 },
    #[error("a different catalog with the same number ({0}) was already accepted")]
    Conflict(u64),
    #[error("the record of accepted catalogs is damaged: {0}")]
    StoreDamaged(String),
}

/// A catalog that passed every check, with the digest of its signed
/// bytes. Only [`verify`] makes one.
#[derive(Clone, Debug)]
pub struct Verified {
    catalog: Catalog,
    signed: SignedCatalog,
    digest: Digest,
}
impl Verified {
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    /// The catalog as received, to hand to the helper (which verifies it
    /// again with its own store).
    pub fn signed(&self) -> &SignedCatalog {
        &self.signed
    }
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

/// Parse and verify catalog bytes (checks 1–5). Rollback (6) needs the
/// accepted store: [`AcceptedStore::accept`].
pub fn verify(bytes: &[u8], config: &TrustConfig, now: Timestamp) -> Result<Verified, TrustError> {
    let signed: SignedCatalog = canonical::from_slice(bytes, limits::CATALOG, SignedCatalogSchema::VALUE)
        .map_err(|e| TrustError::Unreadable(e.to_string()))?;
    verify_signed(signed, config, now)
}

pub fn verify_signed(
    signed: SignedCatalog,
    config: &TrustConfig,
    now: Timestamp,
) -> Result<Verified, TrustError> {
    let catalog = &signed.catalog;
    let root = config
        .roots
        .iter()
        .find(|r| r.id == signed.key_id.as_str())
        .ok_or_else(|| TrustError::UnknownKey(signed.key_id.to_string()))?;
    if !root.channels.contains(&catalog.channel.as_str()) {
        return Err(TrustError::WrongChannel {
            key: root.id.into(),
            channel: catalog.channel.to_string(),
        });
    }
    if catalog.issued_at < root.valid_from || catalog.issued_at > root.valid_until {
        return Err(TrustError::KeyOutOfWindow(root.id.into()));
    }
    let key = VerifyingKey::from_bytes(&root.public_key).map_err(|_| TrustError::BadSignature)?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&signed.signature)
        .map_err(|_| TrustError::BadSignature)?;
    let signature = Signature::from_slice(&raw).map_err(|_| TrustError::BadSignature)?;
    let message = catalog
        .signed_bytes()
        .map_err(|e| TrustError::Unreadable(e.to_string()))?;
    key.verify_strict(&message, &signature)
        .map_err(|_| TrustError::BadSignature)?;
    catalog
        .check()
        .map_err(|e| TrustError::Inconsistent(e.to_string()))?;
    for release in &catalog.releases {
        let a = &release.artifacts;
        for url in [&a.payload.url, &a.installer_data.url, &a.manifest.url, &a.engine.url, &a.recovery_app.url] {
            let host = url.host().split(':').next().unwrap_or_default();
            if !config.hosts.contains(&host) {
                return Err(TrustError::UntrustedHost(url.host().into()));
            }
        }
    }
    if catalog.issued_at.unix() - config.clock_skew > now.unix() {
        return Err(TrustError::ClockBehind {
            now,
            issued: catalog.issued_at,
        });
    }
    if now >= catalog.expires_at {
        return Err(TrustError::Expired(catalog.expires_at));
    }
    let digest = Digest::of(&message);
    Ok(Verified {
        catalog: catalog.clone(),
        signed,
        digest,
    })
}

/// Signing, for release tooling and tests. Production keys never come from
/// this repository.
pub mod signing {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// A signing key from its 32-byte seed.
    pub fn key(seed: &[u8; 32]) -> SigningKey {
        SigningKey::from_bytes(seed)
    }

    /// The development key's seed, from `keys/development.key`.
    pub fn development_seed() -> [u8; 32] {
        let text = include_str!("../keys/development.key");
        let line = text
            .lines()
            .find(|l| !l.starts_with('#') && !l.trim().is_empty())
            .expect("a seed line");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(line.trim())
            .expect("base64 seed");
        bytes.try_into().expect("32 bytes")
    }

    pub fn sign(catalog: Catalog, key_id: &str, key: &SigningKey) -> Result<SignedCatalog, sekka_mac_contract::Error> {
        let signature = key.sign(&catalog.signed_bytes()?);
        Ok(SignedCatalog {
            schema: SignedCatalogSchema,
            key_id: key_id.parse()?,
            signature: base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
            catalog,
        })
    }

    /// The public half, as `keys/*.pub.rs` writes it.
    pub fn public_key_literal(key: &SigningKey) -> String {
        let bytes = key.verifying_key().to_bytes();
        let items: Vec<String> = bytes.iter().map(|b| format!("0x{b:02x}")).collect();
        format!("[{}]\n", items.join(", "))
    }
}

#[cfg(test)]
mod tests;
