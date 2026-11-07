// SPDX-License-Identifier: GPL-3.0-or-later
//! Consistent sample documents for tests: the J514S profile, a development
//! catalog with one release, an inspection of a J514S like the reference
//! one with macOS alone on its disk, and the plan for 100 GiB of it.
//! Compiled for this crate's tests and, with the `samples` feature, for
//! other crates' tests. Nothing in a real build uses them.
use crate::*;

pub fn release() -> Release {
    let d = |s: &str| Digest::of(s.as_bytes());
    let artifact = |name: &str, size| Artifact {
        url: format!("https://downloads.sekka.computer/mac/v1/{name}").parse().unwrap(),
        size,
        digest: d(name),
    };
    Release {
        id: "sekka-j514s-4882dc33".parse().unwrap(),
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
            engine: artifact("engine.tar.gz", 40 * 1024 * 1024),
            recovery_app: artifact("recovery.zip", 4 * 1024 * 1024),
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
            approximate_bytes: 1024 * 1024 * 1024,
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

pub fn catalog() -> Catalog {
    Catalog {
        schema: CatalogSchema,
        product: PRODUCT.parse().unwrap(),
        channel: "development".parse().unwrap(),
        serial: 1,
        issued_at: "2026-11-06T21:00:00Z".parse().unwrap(),
        expires_at: "2026-12-06T21:00:00Z".parse().unwrap(),
        minimum_app_version: "0.1.0".parse().unwrap(),
        protocol: 1,
        profiles: vec![j514s()],
        releases: vec![release()],
        notice: None,
    }
}

pub fn inspection() -> Inspection {
    let u = |s: &str| Uuid::parse_any(s).unwrap();
    let gib = crate::GIB;
    let disk = Disk {
        device: "disk0".parse().unwrap(),
        uuid: u("0d4e3e3b-0b3a-4b0a-9a0a-000000000000"),
        size: 500 * gib,
        block_size: 4096,
        partitions: vec![
            Partition {
                uuid: u("d5819e8f-c8c3-4e98-b399-89b992e8dab9"),
                kind: PartitionKind::AppleIsc,
                name: "iBootSystemContainer".parse().unwrap(),
                offset: 24576,
                size: 524288000,
            },
            Partition {
                uuid: u("b2e3bc98-c2cf-409e-bfc0-f5ce86a8c5d9"),
                kind: PartitionKind::Apfs,
                name: "Container".parse().unwrap(),
                offset: 524312576,
                size: 494 * gib,
            },
            Partition {
                uuid: u("1711f7a0-113b-4b89-bc86-e6fa3c675fca"),
                kind: PartitionKind::AppleRecovery,
                name: "RecoveryOSContainer".parse().unwrap(),
                offset: 524312576 + 494 * gib,
                size: 5 * gib,
            },
        ],
        containers: vec![Container {
            uuid: u("c54f285b-fe6e-4d0c-9bc6-567c62bd7c20"),
            partition: u("b2e3bc98-c2cf-409e-bfc0-f5ce86a8c5d9"),
            size: 494 * gib,
            free: 300 * gib,
            volume_groups: vec![VolumeGroup {
                uuid: u("8e945eba-968d-4906-966b-e07ec593e169"),
                name: "Macintosh HD".parse().unwrap(),
                system_volume: Some(u("11111111-2222-4333-8444-555555555555")),
                data_volume: Some(u("11111111-2222-4333-8444-666666666666")),
                os: GroupOs::Macos {
                    version: "27.0.1".parse().unwrap(),
                },
            }],
            resize: Some(ResizeLimits {
                minimum: 176 * gib,
                recommended: 187 * gib,
            }),
            snapshots: 0,
        }],
    };
    Inspection {
        schema: InspectionSchema,
        machine: Machine {
            product_type: "Mac15,6".parse().unwrap(),
            device_class: "j514sap".parse().unwrap(),
            chip_id: 0x6030,
            board_id: 0x44,
            product_name: "MacBook Pro (14-inch, Nov 2023)".parse().unwrap(),
            soc_name: "Apple M3 Pro".parse().unwrap(),
            system_firmware: "mBoot-20457.1.29".parse().unwrap(),
            binding: machine_binding("00000000-0000-4000-8000-000000000000").unwrap(),
        },
        macos: Macos {
            version: "27.0.1".parse().unwrap(),
            build: "26A434".parse().unwrap(),
            system_firmware_version: "27.0".parse().unwrap(),
            volume_group: u("8e945eba-968d-4906-966b-e07ec593e169"),
            internal_boot: true,
        },
        power: Power {
            has_battery: true,
            on_ac: true,
            battery_percent: Some(80),
        },
        filevault: FileVault::On,
        layout_digest: disk.layout_digest().unwrap(),
        candidates: vec![Candidate::ShrinkContainer {
            container: u("c54f285b-fe6e-4d0c-9bc6-567c62bd7c20"),
            partition: u("b2e3bc98-c2cf-409e-bfc0-f5ce86a8c5d9"),
            current_size: 494 * gib,
            available: 318 * gib,
            available_recommended: 307 * gib,
            adjacent_free: 0,
        }],
        disk,
        installations: vec![],
        issues: vec![],
    }
}


/// The plan for 100 GiB of the sample inspection's container.
pub fn plan() -> Plan {
    derive_plan(
        &inspection(),
        &catalog(),
        &PlanRequest {
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            place: Place::ShrinkContainer {
                container: "c54f285b-fe6e-4d0c-9bc6-567c62bd7c20".parse().unwrap(),
            },
            sekka: 100 * GIB,
        },
        0,
    )
    .expect("the sample plan derives")
}
