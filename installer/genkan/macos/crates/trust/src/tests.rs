// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use sekka_mac_contract::{
    AllocationPolicy, AppleInput, AppleInputKind, Artifact, Artifacts, CatalogSchema, GIB,
    HumanStep, ImageFacts, PRODUCT, Release, SizedDigest, SourceIdentity, Support,
};

fn now() -> Timestamp {
    "2026-11-07T00:00:00Z".parse().unwrap()
}

fn release(id: &str, host: &str) -> Release {
    let d = |s: &str| Digest::of(s.as_bytes());
    let artifact = |name: &str, size| Artifact {
        url: format!("https://{host}/mac/v1/{name}").parse().unwrap(),
        size,
        digest: d(name),
    };
    Release {
        id: id.parse().unwrap(),
        title: "Sekka for the 14-inch MacBook Pro (M3 Pro)".parse().unwrap(),
        profile: "j514s".parse().unwrap(),
        support: Support::Development,
        source: SourceIdentity {
            repository: "bitemyapp/sekka".parse().unwrap(),
            revision: "4882dc33652612e7475f467f6373d8b607e5407c".parse().unwrap(),
            lock: d("lock"),
        },
        artifacts: Artifacts {
            payload: artifact("payload.zip", 12 * GIB),
            installer_data: artifact("installer_data.json", 2048),
            manifest: artifact("manifest.json", 8192),
            engine: artifact("engine.tar.gz", 40 << 20),
            recovery_app: artifact("recovery.zip", 4 << 20),
        },
        image: ImageFacts {
            root_image: SizedDigest {
                size: 11 * GIB,
                digest: d("root"),
            },
            esp_inventory: d("esp"),
            esp_bytes: GIB,
        },
        allocation: AllocationPolicy {
            minimum: 40 * GIB,
            recommended: 120 * GIB,
        },
        apple_inputs: vec![AppleInput {
            kind: AppleInputKind::StubRestoreImage,
            version: "14.8.3".parse().unwrap(),
            approximate_bytes: GIB,
        }],
        human_steps: vec![
            HumanStep::ShutDown,
            HumanStep::HoldPowerButton,
            HumanStep::ChooseVolume,
            HumanStep::AuthorizeInRecovery,
            HumanStep::CreateAccount,
        ],
        published: "2026-11-06T21:00:00Z".parse().unwrap(),
    }
}

fn catalog(serial: u64) -> Catalog {
    Catalog {
        schema: CatalogSchema,
        product: PRODUCT.parse().unwrap(),
        channel: "development".parse().unwrap(),
        serial,
        issued_at: "2026-11-06T21:00:00Z".parse().unwrap(),
        expires_at: "2026-12-06T21:00:00Z".parse().unwrap(),
        minimum_app_version: "0.1.0".parse().unwrap(),
        protocol: 1,
        profiles: vec![sekka_mac_contract::j514s()],
        releases: vec![release("sekka-j514s-4882dc33", "downloads.sekka.computer")],
        notice: None,
    }
}

fn dev_config() -> TrustConfig {
    TrustConfig {
        roots: vec![development_root()],
        hosts: PRODUCTION_HOSTS.to_vec(),
        clock_skew: 600,
    }
}

fn sign(c: Catalog) -> Vec<u8> {
    let key = signing::key(&signing::development_seed());
    canonical::to_vec(&signing::sign(c, DEVELOPMENT_KEY_ID, &key).unwrap()).unwrap()
}

#[test]
fn the_development_key_matches_its_published_half() {
    let key = signing::key(&signing::development_seed());
    assert_eq!(key.verifying_key().to_bytes(), DEVELOPMENT_PUBLIC_KEY);
}

#[test]
fn a_signed_catalog_verifies() {
    let v = verify(&sign(catalog(1)), &dev_config(), now()).unwrap();
    assert_eq!(v.catalog().serial, 1);
}

#[test]
fn production_builds_trust_no_development_key() {
    let config = TrustConfig {
        roots: PRODUCTION_ROOTS.to_vec(),
        ..dev_config()
    };
    assert!(matches!(
        verify(&sign(catalog(1)), &config, now()),
        Err(TrustError::UnknownKey(_))
    ));
}

#[test]
fn any_change_breaks_the_signature() {
    let key = signing::key(&signing::development_seed());
    let mut signed = signing::sign(catalog(1), DEVELOPMENT_KEY_ID, &key).unwrap();
    signed.catalog.releases[0].artifacts.payload.digest = Digest::of(b"substituted");
    let bytes = canonical::to_vec(&signed).unwrap();
    assert_eq!(verify(&bytes, &dev_config(), now()).unwrap_err(), TrustError::BadSignature);

    let mut signed = signing::sign(catalog(1), DEVELOPMENT_KEY_ID, &key).unwrap();
    signed.signature = signed.signature.replace('A', "B");
    let bytes = canonical::to_vec(&signed).unwrap();
    assert!(verify(&bytes, &dev_config(), now()).is_err());
}

#[test]
fn another_key_is_refused() {
    let other = signing::key(&[9; 32]);
    let signed = signing::sign(catalog(1), DEVELOPMENT_KEY_ID, &other).unwrap();
    let bytes = canonical::to_vec(&signed).unwrap();
    assert_eq!(verify(&bytes, &dev_config(), now()).unwrap_err(), TrustError::BadSignature);
}

#[test]
fn channels_and_key_windows_are_enforced() {
    let mut c = catalog(1);
    c.channel = "stable".parse().unwrap();
    assert!(matches!(
        verify(&sign(c), &dev_config(), now()),
        Err(TrustError::WrongChannel { .. })
    ));
    let mut c = catalog(1);
    c.issued_at = "2031-01-01T00:00:00Z".parse().unwrap();
    c.expires_at = "2031-02-01T00:00:00Z".parse().unwrap();
    assert!(matches!(
        verify(&sign(c), &dev_config(), "2031-01-02T00:00:00Z".parse().unwrap()),
        Err(TrustError::KeyOutOfWindow(_))
    ));
}

#[test]
fn clocks_and_expiry_are_explained() {
    let behind: Timestamp = "2026-11-01T00:00:00Z".parse().unwrap();
    assert!(matches!(
        verify(&sign(catalog(1)), &dev_config(), behind),
        Err(TrustError::ClockBehind { .. })
    ));
    // A few minutes of skew is tolerated.
    let close: Timestamp = "2026-11-06T20:55:00Z".parse().unwrap();
    assert!(verify(&sign(catalog(1)), &dev_config(), close).is_ok());
    let later: Timestamp = "2027-01-01T00:00:00Z".parse().unwrap();
    assert!(matches!(
        verify(&sign(catalog(1)), &dev_config(), later),
        Err(TrustError::Expired(_))
    ));
}

#[test]
fn artifacts_must_come_from_sekka() {
    let mut c = catalog(1);
    c.releases[0] = release("sekka-j514s-4882dc33", "example.org");
    assert!(matches!(
        verify(&sign(c), &dev_config(), now()),
        Err(TrustError::UntrustedHost(_))
    ));
}

#[test]
fn unknown_schemas_and_altered_encodings_are_refused() {
    let bytes = sign(catalog(1));
    let text = String::from_utf8(bytes.clone()).unwrap();
    let newer = text.replace("sekka.mac.signed-catalog/1", "sekka.mac.signed-catalog/2");
    assert!(matches!(
        verify(newer.as_bytes(), &dev_config(), now()),
        Err(TrustError::Unreadable(m)) if m.contains("signed-catalog/2")
    ));
    let spaced = text.replacen(",", ", ", 1);
    assert!(verify(spaced.as_bytes(), &dev_config(), now()).is_err());
}

#[test]
fn rollback_and_conflicts_are_refused_and_the_store_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accepted.json");
    let mut store = AcceptedStore::open(&path).unwrap();
    let v2 = verify(&sign(catalog(2)), &dev_config(), now()).unwrap();
    store.accept(&v2, now()).unwrap();
    store.accept(&v2, now()).unwrap();

    let mut store = AcceptedStore::open(&path).unwrap();
    assert_eq!(store.accepted("development"), Some(2));
    let v1 = verify(&sign(catalog(1)), &dev_config(), now()).unwrap();
    assert_eq!(
        store.accept(&v1, now()).unwrap_err(),
        TrustError::Rollback { offered: 1, accepted: 2 }
    );
    let mut other = catalog(2);
    other.notice = Some("Different".parse().unwrap());
    let v2b = verify(&sign(other), &dev_config(), now()).unwrap();
    assert_eq!(store.accept(&v2b, now()).unwrap_err(), TrustError::Conflict(2));
    let v3 = verify(&sign(catalog(3)), &dev_config(), now()).unwrap();
    store.accept(&v3, now()).unwrap();
    assert_eq!(AcceptedStore::open(&path).unwrap().accepted("development"), Some(3));
}

#[test]
fn a_bad_release_is_withdrawn_by_a_newer_catalog_without_it() {
    // Withdrawing is publishing the next serial without the release, with
    // a notice saying why. Once a Mac has seen it, the catalog that offered
    // the bad release is a rollback and is refused, whoever serves it.
    let dir = tempfile::tempdir().unwrap();
    let mut store = AcceptedStore::open(&dir.path().join("accepted.json")).unwrap();
    let offered = verify(&sign(catalog(4)), &dev_config(), now()).unwrap();
    store.accept(&offered, now()).unwrap();
    let mut withdrawal = catalog(5);
    withdrawal.releases.clear();
    withdrawal.notice = Some(
        "The last development build was withdrawn: it can't start on some Macs."
            .parse()
            .unwrap(),
    );
    let withdrawn = verify(&sign(withdrawal), &dev_config(), now()).unwrap();
    store.accept(&withdrawn, now()).unwrap();
    assert!(withdrawn.catalog().releases.is_empty());
    assert_eq!(
        store.accept(&offered, now()).unwrap_err(),
        TrustError::Rollback { offered: 4, accepted: 5 }
    );
}

#[test]
fn a_damaged_store_is_reported_not_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accepted.json");
    std::fs::write(&path, b"{\"schema\":\"sekka.mac.accepted/1\",").unwrap();
    assert!(matches!(AcceptedStore::open(&path), Err(TrustError::StoreDamaged(_))));
}
