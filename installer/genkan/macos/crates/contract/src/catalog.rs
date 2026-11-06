// SPDX-License-Identifier: GPL-3.0-or-later
//! The release catalog: which Sekka releases a channel offers, for which
//! Macs, and the exact bytes each is made of.
//!
//! A catalog is public. It is signed as a whole ([`SignedCatalog`]); the
//! signature check, trust roots and rollback protection live in
//! `sekka-mac-trust`. This module only defines the document and its internal
//! consistency.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{
    Digest, Error, GIB, GitRevision, HardwareProfile, HttpsUrl, Slug, Support, Text, Timestamp,
    Version, invalid, schema_tag,
};

schema_tag!(CatalogSchema, "sekka.mac.catalog/1");
schema_tag!(SignedCatalogSchema, "sekka.mac.signed-catalog/1");

/// The catalog and the signature over its canonical bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCatalog {
    pub schema: SignedCatalogSchema,
    /// Which trust root signed it.
    pub key_id: Slug,
    /// Ed25519 over [`Catalog::signed_bytes`], base64.
    pub signature: String,
    pub catalog: Catalog,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: CatalogSchema,
    /// `computer.sekka.installer`: a catalog for another product is refused.
    pub product: Slug,
    /// `development`, `candidate` or `stable`.
    pub channel: Slug,
    /// Increases with every catalog published on a channel. The app keeps
    /// the highest it has accepted and refuses anything lower.
    pub serial: u64,
    pub issued_at: Timestamp,
    pub expires_at: Timestamp,
    /// An older app is told to update rather than install.
    pub minimum_app_version: Version,
    /// The app–helper–engine protocol this catalog's releases need.
    pub protocol: u32,
    pub profiles: Vec<HardwareProfile>,
    pub releases: Vec<Release>,
    /// A line shown above the releases ("Development builds: not for
    /// your only computer").
    pub notice: Option<Text>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    /// `sekka-j514s-4882dc33`.
    pub id: Slug,
    pub title: Text,
    /// The profile it is built for, by id.
    pub profile: Slug,
    pub support: Support,
    pub source: SourceIdentity,
    pub artifacts: Artifacts,
    pub image: ImageFacts,
    pub allocation: AllocationPolicy,
    /// Apple inputs the engine fetches itself (restore image parts for the
    /// stub), and how it authenticates them.
    pub apple_inputs: Vec<AppleInput>,
    /// What the owner does by hand after the app's part, in order.
    pub human_steps: Vec<HumanStep>,
    pub published: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    /// `bitemyapp/sekka`.
    pub repository: Text,
    pub revision: GitRevision,
    /// The digest of the installed flake.lock the image ships.
    pub lock: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifacts {
    /// The OS package: `esp/` and `root.img`, stored.
    pub payload: Artifact,
    /// Asahi's `installer_data.json` for this release.
    pub installer_data: Artifact,
    /// The image manifest (`sekka.mac.manifest`): sizes, digests, the ESP
    /// inventory and the source it was built from.
    pub manifest: Artifact,
    /// The engine archive the app must carry: the pinned Asahi installer
    /// with Sekka's adapter. The app compares it with its own bundled copy.
    pub engine: Artifact,
    /// The Recovery app, as a zip, placed on the new system's stub.
    pub recovery_app: Artifact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: HttpsUrl,
    pub size: u64,
    pub digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFacts {
    pub root_image: SizedDigest,
    /// The digest of the ESP tree's file inventory (path, size, digest).
    pub esp_inventory: Digest,
    pub esp_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizedDigest {
    pub size: u64,
    pub digest: Digest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationPolicy {
    /// Below this, Sekka has no room to update (stub, ESP, root image and
    /// room for one more generation).
    pub minimum: u64,
    /// What the app suggests when the disk allows it.
    pub recommended: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleInput {
    pub kind: AppleInputKind,
    /// `14.8.3`.
    pub version: Version,
    /// About how much the engine downloads, for the staging estimate.
    pub approximate_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppleInputKind {
    /// The parts of Apple's restore image the stub needs. The engine fetches
    /// them from Apple and authenticates them against Apple's own signed
    /// build manifest, as Asahi's installer does; there is no Sekka checksum
    /// for Apple's bytes.
    StubRestoreImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HumanStep {
    /// Shut the Mac down from the app's last screen.
    ShutDown,
    /// Press and hold the power button until "Loading startup options".
    HoldPowerButton,
    /// Choose the new Sekka volume and Continue.
    ChooseVolume,
    /// Sign in and approve the new system's startup software in Recovery.
    AuthorizeInRecovery,
    /// Create the Linux account at Sekka's first start.
    CreateAccount,
}

impl Catalog {
    /// The bytes the signature covers: a domain line, then the catalog's
    /// canonical form.
    pub fn signed_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = b"sekka.mac.catalog/1 signature\n".to_vec();
        bytes.extend(crate::canonical::to_vec(self)?);
        Ok(bytes)
    }

    pub fn check(&self) -> Result<(), Error> {
        if self.product.as_str() != PRODUCT {
            return Err(invalid(format!(
                "this catalog is for {}, not Sekka's installer",
                self.product
            )));
        }
        if self.expires_at <= self.issued_at {
            return Err(invalid("a catalog expires after it is issued"));
        }
        if self.expires_at.unix() - self.issued_at.unix() > MAX_LIFETIME {
            return Err(invalid("a catalog is valid for at most 180 days"));
        }
        let mut profiles = BTreeSet::new();
        for profile in &self.profiles {
            profile.check()?;
            if !profiles.insert(profile.id.clone()) {
                return Err(invalid(format!("profile {} is listed twice", profile.id)));
            }
        }
        let mut releases = BTreeSet::new();
        for release in &self.releases {
            if !releases.insert(release.id.clone()) {
                return Err(invalid(format!("release {} is listed twice", release.id)));
            }
            let profile = self
                .profiles
                .iter()
                .find(|p| p.id == release.profile)
                .ok_or_else(|| invalid(format!("release {} names no listed profile", release.id)))?;
            if release.support > profile.support {
                return Err(invalid(format!(
                    "release {} claims more support than its profile",
                    release.id
                )));
            }
            release.check()?;
        }
        Ok(())
    }

    pub fn profile(&self, id: &Slug) -> Option<&HardwareProfile> {
        self.profiles.iter().find(|p| &p.id == id)
    }
}

/// The product every catalog must name.
pub const PRODUCT: &str = "computer.sekka.installer";
/// The longest a catalog can be valid, in seconds.
pub const MAX_LIFETIME: i64 = 180 * 86_400;

impl Release {
    fn check(&self) -> Result<(), Error> {
        let a = &self.artifacts;
        for (name, artifact, limit) in [
            ("payload", &a.payload, 64 * GIB),
            ("installer data", &a.installer_data, 1024 * 1024),
            ("manifest", &a.manifest, 4 * 1024 * 1024),
            ("engine", &a.engine, GIB),
            ("Recovery app", &a.recovery_app, 256 * 1024 * 1024),
        ] {
            if artifact.size == 0 || artifact.size > limit {
                return Err(invalid(format!(
                    "release {}: the {name} is {} bytes",
                    self.id, artifact.size
                )));
            }
        }
        let image = &self.image;
        if image.root_image.size == 0
            || image.root_image.size % 4096 != 0
            || image.root_image.size > a.payload.size
        {
            return Err(invalid(format!(
                "release {}: the root image's size doesn't fit its package",
                self.id
            )));
        }
        let floor = image.root_image.size + image.esp_bytes;
        if self.allocation.minimum < floor || self.allocation.recommended < self.allocation.minimum
        {
            return Err(invalid(format!(
                "release {}: its minimum allocation must hold the image and be at most the recommendation",
                self.id
            )));
        }
        let steps = &self.human_steps;
        let required = [
            HumanStep::ShutDown,
            HumanStep::HoldPowerButton,
            HumanStep::ChooseVolume,
            HumanStep::AuthorizeInRecovery,
        ];
        if steps.len() < required.len() || steps[..required.len()] != required {
            return Err(invalid(format!(
                "release {}: the Recovery steps are part of every Mac release, in order",
                self.id
            )));
        }
        Ok(())
    }

    /// Bytes downloaded before installing: the release's own artifacts
    /// (the engine is carried by the app) and Apple's inputs.
    pub fn download_bytes(&self) -> u64 {
        let a = &self.artifacts;
        a.payload.size
            + a.installer_data.size
            + a.manifest.size
            + a.recovery_app.size
            + self.apple_inputs.iter().map(|i| i.approximate_bytes).sum::<u64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples::{catalog as sample_catalog, release as sample_release};

    #[test]
    fn the_sample_catalog_is_consistent() {
        sample_catalog().check().unwrap();
    }

    #[test]
    fn inconsistent_catalogs_are_refused() {
        let mut c = sample_catalog();
        c.product = "org.example.installer".parse().unwrap();
        assert!(c.check().is_err());

        let mut c = sample_catalog();
        c.expires_at = c.issued_at.plus(200 * 86_400);
        assert!(c.check().is_err());

        let mut c = sample_catalog();
        c.releases.push(sample_release());
        assert!(c.check().is_err());

        let mut c = sample_catalog();
        c.releases[0].profile = "j614s".parse().unwrap();
        assert!(c.check().is_err());

        let mut c = sample_catalog();
        c.releases[0].support = Support::Qualified;
        assert!(c.check().is_err());

        let mut c = sample_catalog();
        c.releases[0].allocation.minimum = GIB;
        assert!(c.check().is_err());

        let mut c = sample_catalog();
        c.releases[0].human_steps.remove(3);
        assert!(c.check().is_err());

        let mut c = sample_catalog();
        c.releases[0].artifacts.engine.size = 0;
        assert!(c.check().is_err());
    }

    #[test]
    fn signed_bytes_are_domain_separated_and_canonical() {
        let c = sample_catalog();
        let bytes = c.signed_bytes().unwrap();
        assert!(bytes.starts_with(b"sekka.mac.catalog/1 signature\n{"));
        let body = &bytes[b"sekka.mac.catalog/1 signature\n".len()..];
        let back: Catalog =
            crate::canonical::from_slice(body, crate::limits::CATALOG, CatalogSchema::VALUE).unwrap();
        assert_eq!(back, c);
    }
}
