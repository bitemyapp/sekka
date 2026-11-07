// SPDX-License-Identifier: GPL-3.0-or-later
//! The installation plan: exactly what will change on this Mac, bound to
//! the Mac, its disk layout and the release's bytes.
//!
//! A plan is derived, never authored: [`derive_plan`] computes it from an
//! inspection, a verified catalog and what the person chose, with nothing
//! random or time-dependent in it. The app derives it to show the review;
//! the helper derives it again from its own inspection as root and refuses
//! to start unless the two digests agree; the engine checks its actions
//! against Asahi's own view of the disk before changing anything. A
//! [`Confirmation`] names the plan by digest, so a plan that changed in
//! any way needs a new review.
use serde::{Deserialize, Serialize};

use crate::{
    Admission, Candidate, Catalog, Digest, Error, Extent, GitRevision, HumanStep, Inspection,
    Issue, Refusal, Release, Slug, Text, Uuid, canonical, invalid, schema_tag,
};

schema_tag!(PlanSchema, "sekka.mac.plan/1");
schema_tag!(ConfirmationSchema, "sekka.mac.confirmation/1");

/// Partitions start and end on 1 MiB boundaries, as Asahi's installer
/// aligns them.
pub const ALIGN: u64 = 1024 * 1024;
/// The stub: a minimal macOS whose Recovery boots Sekka. Asahi's 2.5 GB,
/// aligned down.
pub const STUB_BYTES: u64 = 2_500_000_000 / ALIGN * ALIGN;
/// The EFI system partition, from the release's installer data.
pub const ESP_BYTES: u64 = crate::GIB;
/// The name the new volume has in Startup Disk and the startup options.
pub const VOLUME_NAME: &str = "Sekka";
/// What a plan for free space leaves free in the macOS container beside
/// the downloads and Apple's inputs, which go there while Sekka installs:
/// the engine fetches Apple's files only into the container's free space
/// less this. The engine's `contract.MACOS_MARGIN` is the same number
/// (both are held to `tests/vectors/expected.json`).
pub const MACOS_MARGIN: u64 = crate::GIB;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: PlanSchema,
    pub release: ReleaseBinding,
    pub machine: MachineBinding,
    pub disk: DiskBinding,
    pub allocation: Allocation,
    /// In order. Nothing else changes the disk.
    pub actions: Vec<Action>,
    /// Where the new partitions go: the stub, the ESP and the root, in that
    /// order, filling this extent.
    pub target: Extent,
    pub volume_name: Text,
    pub human_steps: Vec<HumanStep>,
    /// What the owner must acknowledge on the review, besides the backup.
    pub acknowledgements: Vec<Acknowledgement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseBinding {
    pub channel: Slug,
    pub catalog_serial: u64,
    pub release: Slug,
    pub profile: Slug,
    pub profile_digest: Digest,
    pub source_revision: GitRevision,
    pub payload: Digest,
    pub installer_data: Digest,
    pub manifest: Digest,
    pub engine: Digest,
    pub recovery_app: Digest,
    pub root_image: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineBinding {
    pub product_type: Text,
    pub device_class: Text,
    pub chip_id: u32,
    pub system_firmware: Text,
    pub binding: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskBinding {
    pub disk: Uuid,
    pub layout_digest: Digest,
    /// The volume group of the macOS that runs the installation, whose
    /// owner authorizes it.
    pub macos_volume_group: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allocation {
    /// Everything Sekka takes: stub, ESP and root.
    pub sekka: u64,
    pub stub: u64,
    pub esp: u64,
    pub root: u64,
    /// The macOS container's size afterwards, when it shrinks.
    pub macos_container_after: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action {
    /// Apple's `diskutil apfs resizeContainer`, nothing else.
    ShrinkContainer { container: Uuid, from: u64, to: u64 },
    CreateStub { size: u64 },
    CreateEsp { size: u64 },
    CreateRoot { size: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Acknowledgement {
    /// This Mac's system firmware hasn't been tested with this boot stack.
    UntestedSystemFirmware,
    /// The profile lists a known problem with this macOS version.
    KnownMacosIssue,
    /// The release is a development build, not a qualified one.
    DevelopmentRelease,
}

/// What the person chose on the plan screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub release: Slug,
    pub place: Place,
    /// Bytes for Sekka in all; aligned down to [`ALIGN`].
    pub sekka: u64,
}

/// Which candidate, by what identifies it rather than its position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Place {
    FreeSpace { offset: u64 },
    ShrinkContainer { container: Uuid },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirmation {
    pub schema: ConfirmationSchema,
    pub plan: Digest,
    /// "I have a current backup of this Mac."
    pub backup: bool,
    /// The Recovery and setup steps, as shown on the review.
    pub human_steps: Vec<HumanStep>,
    pub acknowledgements: Vec<Acknowledgement>,
}

/// Why no plan could be made, in terms a person can act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("this Mac isn't one this release supports")]
    NotAdmitted(Vec<Refusal>),
    #[error("something on this Mac stops installation")]
    Blocked(Vec<Issue>),
    #[error("the catalog has no release {0}")]
    UnknownRelease(String),
    #[error("that space is no longer available; check this Mac again")]
    PlaceGone,
    #[error("Sekka needs at least {minimum} bytes here")]
    TooSmall { minimum: u64 },
    #[error("at most {maximum} bytes are available here")]
    TooLarge { maximum: u64 },
    /// Free space can take Sekka, but macOS, which isn't resized, has too
    /// little free for what goes there while Sekka installs ([`macos_room`]):
    /// `needed` in all, `downloads` of it Sekka's own downloads still to
    /// come.
    #[error("macOS needs {needed} bytes free while Sekka installs and has {free}")]
    MacosNeedsRoom { needed: u64, free: u64, downloads: u64 },
    #[error("{0}")]
    Invalid(String),
}
impl From<Error> for PlanError {
    fn from(e: Error) -> Self {
        Self::Invalid(e.to_string())
    }
}

impl Plan {
    pub fn digest(&self) -> Result<Digest, Error> {
        canonical::digest(self)
    }

    /// The plan's own arithmetic: actions, allocation and target agree.
    pub fn check(&self) -> Result<(), Error> {
        let a = &self.allocation;
        if a.stub + a.esp + a.root != a.sekka || a.sekka != self.target.size {
            return Err(invalid("the plan's allocation doesn't add up"));
        }
        if [self.target.offset, self.target.size, a.stub, a.esp, a.root]
            .iter()
            .any(|v| v % ALIGN != 0)
        {
            return Err(invalid("the plan's partitions aren't aligned"));
        }
        let creates: Vec<&Action> = self
            .actions
            .iter()
            .filter(|a| !matches!(a, Action::ShrinkContainer { .. }))
            .collect();
        let expected = [
            Action::CreateStub { size: a.stub },
            Action::CreateEsp { size: a.esp },
            Action::CreateRoot { size: a.root },
        ];
        if creates.len() != 3 || creates.iter().zip(&expected).any(|(x, y)| *x != y) {
            return Err(invalid("the plan creates the stub, the ESP and the root, in order"));
        }
        match self.actions.first() {
            Some(Action::ShrinkContainer { from, to, .. }) => {
                if to >= from || a.macos_container_after != Some(*to) || self.actions.len() != 4 {
                    return Err(invalid("the plan's shrink is inconsistent"));
                }
            }
            _ => {
                if a.macos_container_after.is_some() || self.actions.len() != 3 {
                    return Err(invalid("the plan's shrink is inconsistent"));
                }
            }
        }
        Ok(())
    }
}

impl Confirmation {
    pub fn for_plan(plan: &Plan, backup: bool) -> Result<Self, Error> {
        Ok(Self {
            schema: ConfirmationSchema,
            plan: plan.digest()?,
            backup,
            human_steps: plan.human_steps.clone(),
            acknowledgements: plan.acknowledgements.clone(),
        })
    }

    /// Whether this confirmation approves exactly this plan.
    pub fn approves(&self, plan: &Plan) -> Result<(), Error> {
        if !self.backup {
            return Err(invalid("the owner hasn't confirmed a current backup"));
        }
        if self.plan != plan.digest()? {
            return Err(invalid(
                "the plan changed since it was reviewed; review it again",
            ));
        }
        if self.human_steps != plan.human_steps || self.acknowledgements != plan.acknowledgements {
            return Err(invalid("the review didn't show this plan's steps"));
        }
        Ok(())
    }
}

/// The acknowledgements an admission requires.
pub fn acknowledgements_for(admission: &Admission, release: &Release) -> Vec<Acknowledgement> {
    let mut out: Vec<Acknowledgement> = admission
        .acknowledgements
        .iter()
        .filter_map(|r| match r {
            Refusal::UntestedSystemFirmware { .. } => Some(Acknowledgement::UntestedSystemFirmware),
            Refusal::KnownIssue { .. } => Some(Acknowledgement::KnownMacosIssue),
            _ => None,
        })
        .collect();
    if release.support == crate::Support::Development {
        out.push(Acknowledgement::DevelopmentRelease);
    }
    out.sort();
    out.dedup();
    out
}

/// The most Sekka can have from a candidate, with `staging` bytes still to
/// be downloaded into the macOS container first. Those, and the release's
/// Apple inputs (which the engine always downloads into the same container
/// before shrinking it), come out of what the container can give.
pub fn capacity(release: &Release, candidate: &Candidate, staging: u64) -> u64 {
    // One alignment unit is held back for where the new partitions start.
    let raw = match candidate {
        Candidate::FreeSpace { extent } => extent.size,
        Candidate::ShrinkContainer {
            available,
            adjacent_free,
            ..
        } => available.saturating_sub(staging + release.reserved()) + adjacent_free,
    };
    (raw / ALIGN * ALIGN).saturating_sub(ALIGN)
}

/// Whether macOS has room for what goes there while Sekka installs beside
/// it in free space: the `staging` bytes still to download, then the
/// release's Apple inputs, with [`MACOS_MARGIN`] beside them. A shrunk
/// macOS keeps that room in what [`capacity`] leaves it, so only free space
/// can lack it. Nothing about it depends on Sekka's size, so the plan screen
/// knows before anything is downloaded.
pub fn macos_room(
    inspection: &Inspection,
    release: &Release,
    candidate: &Candidate,
    staging: u64,
) -> Result<(), PlanError> {
    if !matches!(candidate, Candidate::FreeSpace { .. }) {
        return Ok(());
    }
    let free = inspection
        .disk
        .containers
        .iter()
        .find(|k| k.volume_groups.iter().any(|g| g.uuid == inspection.macos.volume_group))
        .map_or(0, |k| k.free);
    let needed = staging + release.reserved() + MACOS_MARGIN;
    if free < needed {
        return Err(PlanError::MacosNeedsRoom {
            needed,
            free,
            downloads: staging,
        });
    }
    Ok(())
}

/// The size the plan screen starts at: the release's recommendation, within
/// what this place allows.
pub fn suggested_size(release: &Release, candidate: &Candidate, staging: u64) -> Option<u64> {
    let max = capacity(release, candidate, staging);
    let min = release.allocation.minimum.div_ceil(ALIGN) * ALIGN;
    (max >= min).then(|| release.allocation.recommended.clamp(min, max) / ALIGN * ALIGN)
}

/// Derive the plan for this request. Deterministic: the same inspection,
/// catalog and request always give the same plan, byte for byte.
pub fn derive_plan(
    inspection: &Inspection,
    catalog: &Catalog,
    request: &PlanRequest,
    staging: u64,
) -> Result<Plan, PlanError> {
    let release = catalog
        .releases
        .iter()
        .find(|r| r.id == request.release)
        .ok_or_else(|| PlanError::UnknownRelease(request.release.to_string()))?;
    let profile = catalog
        .profile(&release.profile)
        .ok_or_else(|| PlanError::Invalid("the release names no profile".into()))?;
    let admission = profile.admit(&inspection.machine_facts());
    if !admission.admitted() {
        return Err(PlanError::NotAdmitted(admission.refusals));
    }
    let blocking: Vec<Issue> = inspection.blocking().cloned().collect();
    if !blocking.is_empty() {
        return Err(PlanError::Blocked(blocking));
    }
    let candidate = inspection
        .candidates
        .iter()
        .find(|c| match (c, &request.place) {
            (Candidate::FreeSpace { extent }, Place::FreeSpace { offset }) => extent.offset == *offset,
            (Candidate::ShrinkContainer { container, .. }, Place::ShrinkContainer { container: want }) => {
                container == want
            }
            _ => false,
        })
        .ok_or(PlanError::PlaceGone)?;

    let sekka = request.sekka / ALIGN * ALIGN;
    let minimum = release.allocation.minimum.div_ceil(ALIGN) * ALIGN;
    let maximum = capacity(release, candidate, staging);
    if sekka < minimum {
        return Err(PlanError::TooSmall { minimum });
    }
    if sekka > maximum {
        return Err(PlanError::TooLarge { maximum });
    }
    let root = sekka - STUB_BYTES - ESP_BYTES;

    let (actions, target, macos_container_after) = match candidate {
        Candidate::FreeSpace { extent } => {
            let offset = extent.offset.div_ceil(ALIGN) * ALIGN;
            if offset + sekka > extent.end() {
                return Err(PlanError::TooLarge {
                    maximum: (extent.end().saturating_sub(offset)) / ALIGN * ALIGN,
                });
            }
            // Nothing is taken from macOS, so what goes there meanwhile (the
            // downloads still to come, then Apple's files) must fit in what
            // it has free, with the room the engine keeps beside them.
            macos_room(inspection, release, candidate, staging)?;
            (Vec::new(), Extent { offset, size: sekka }, None)
        }
        Candidate::ShrinkContainer {
            container,
            partition,
            current_size,
            available,
            adjacent_free,
            ..
        } => {
            let part = inspection
                .disk
                .partitions
                .iter()
                .find(|p| &p.uuid == partition)
                .ok_or(PlanError::PlaceGone)?;
            // Sekka ends where the container's free neighbor ends, and
            // starts on an aligned boundary; the container shrinks to end
            // there. Nothing past that end moves.
            let end = part.offset + current_size + adjacent_free;
            let offset = (end - sekka) / ALIGN * ALIGN;
            let to = offset
                .checked_sub(part.offset)
                .filter(|to| to < current_size)
                .ok_or(PlanError::TooLarge { maximum })?;
            if current_size - to > available.saturating_sub(staging + release.reserved()) {
                return Err(PlanError::TooLarge { maximum });
            }
            (
                vec![Action::ShrinkContainer {
                    container: *container,
                    from: *current_size,
                    to,
                }],
                Extent { offset, size: sekka },
                Some(to),
            )
        }
    };
    let mut actions = actions;
    actions.extend([
        Action::CreateStub { size: STUB_BYTES },
        Action::CreateEsp { size: ESP_BYTES },
        Action::CreateRoot { size: root },
    ]);

    let a = &release.artifacts;
    let m = &inspection.machine;
    let plan = Plan {
        schema: PlanSchema,
        release: ReleaseBinding {
            channel: catalog.channel.clone(),
            catalog_serial: catalog.serial,
            release: release.id.clone(),
            profile: profile.id.clone(),
            profile_digest: canonical::digest(profile)?,
            source_revision: release.source.revision.clone(),
            payload: a.payload.digest,
            installer_data: a.installer_data.digest,
            manifest: a.manifest.digest,
            engine: a.engine.digest,
            recovery_app: a.recovery_app.digest,
            root_image: release.image.root_image.digest,
        },
        machine: MachineBinding {
            product_type: m.product_type.clone(),
            device_class: m.device_class.clone(),
            chip_id: m.chip_id,
            system_firmware: m.system_firmware.clone(),
            binding: m.binding,
        },
        disk: DiskBinding {
            disk: inspection.disk.uuid,
            layout_digest: inspection.layout_digest,
            macos_volume_group: inspection.macos.volume_group,
        },
        allocation: Allocation {
            sekka,
            stub: STUB_BYTES,
            esp: ESP_BYTES,
            root,
            macos_container_after,
        },
        actions,
        target,
        volume_name: VOLUME_NAME.parse().unwrap(),
        human_steps: release.human_steps.clone(),
        acknowledgements: acknowledgements_for(&admission, release),
    };
    plan.check()?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples::{catalog as sample_catalog, inspection as sample};
    use crate::{GIB, IssueCode};

    fn request(size: u64) -> PlanRequest {
        PlanRequest {
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            place: Place::ShrinkContainer {
                container: "c54f285b-fe6e-4d0c-9bc6-567c62bd7c20".parse().unwrap(),
            },
            sekka: size,
        }
    }

    #[test]
    fn a_shrink_plan_is_deterministic_and_consistent() {
        let plan = derive_plan(&sample(), &sample_catalog(), &request(100 * GIB), 0).unwrap();
        let again = derive_plan(&sample(), &sample_catalog(), &request(100 * GIB), 0).unwrap();
        assert_eq!(plan.digest().unwrap(), again.digest().unwrap());
        assert_eq!(plan.allocation.sekka, 100 * GIB);
        // The container starts 24 KiB past a MiB boundary; Sekka starts on
        // one, so macOS keeps 24 KiB less than an even 394 GiB.
        let to = 394 * GIB - 24576;
        assert_eq!(plan.allocation.macos_container_after, Some(to));
        assert!(matches!(plan.actions[0], Action::ShrinkContainer { to: t, .. } if t == to));
        assert_eq!(plan.target.offset % ALIGN, 0);
        assert!(plan.target.end() <= 524312576 + 494 * GIB);
        assert_eq!(plan.target.offset, 524312576 + to);
        assert_eq!(
            plan.acknowledgements,
            vec![Acknowledgement::KnownMacosIssue, Acknowledgement::DevelopmentRelease]
        );
        plan.check().unwrap();
        let bytes = canonical::to_vec(&plan).unwrap();
        let back: Plan = canonical::from_slice(&bytes, crate::limits::PLAN, PlanSchema::VALUE).unwrap();
        assert_eq!(back, plan);
    }

    #[test]
    fn sizes_are_bounded_by_the_release_and_the_disk() {
        let c = sample_catalog();
        assert!(matches!(
            derive_plan(&sample(), &c, &request(10 * GIB), 0),
            Err(PlanError::TooSmall { .. })
        ));
        assert!(matches!(
            derive_plan(&sample(), &c, &request(400 * GIB), 0),
            Err(PlanError::TooLarge { .. })
        ));
        // Downloading first takes room the container could otherwise give.
        assert!(derive_plan(&sample(), &c, &request(310 * GIB), 0).is_ok());
        assert!(matches!(
            derive_plan(&sample(), &c, &request(310 * GIB), 20 * GIB),
            Err(PlanError::TooLarge { .. })
        ));
        // So do Apple's inputs, even with nothing left to download: the
        // largest plan still leaves the engine room for them.
        let release = &c.releases[0];
        let candidate = &sample().candidates[0];
        let max = capacity(release, candidate, 0);
        let Candidate::ShrinkContainer { available, .. } = candidate else { panic!() };
        assert!(max + release.reserved() <= *available);
        assert!(derive_plan(&sample(), &c, &request(max), 0).is_ok());
        assert!(derive_plan(&sample(), &c, &request(available - ALIGN), 0).is_err());
    }

    #[test]
    fn a_changed_layout_changes_the_plan() {
        let c = sample_catalog();
        let plan = derive_plan(&sample(), &c, &request(100 * GIB), 0).unwrap();
        let mut moved = sample();
        moved.disk.partitions[2].size -= GIB;
        moved.layout_digest = moved.disk.layout_digest().unwrap();
        let replanned = derive_plan(&moved, &c, &request(100 * GIB), 0).unwrap();
        assert_ne!(plan.digest().unwrap(), replanned.digest().unwrap());
        let confirmation = Confirmation::for_plan(&plan, true).unwrap();
        confirmation.approves(&plan).unwrap();
        assert!(confirmation.approves(&replanned).is_err());
    }

    #[test]
    fn confirmations_need_the_backup_and_the_same_steps() {
        let plan = derive_plan(&sample(), &sample_catalog(), &request(100 * GIB), 0).unwrap();
        assert!(Confirmation::for_plan(&plan, false).unwrap().approves(&plan).is_err());
        let mut c = Confirmation::for_plan(&plan, true).unwrap();
        c.acknowledgements.clear();
        assert!(c.approves(&plan).is_err());
    }

    #[test]
    fn blocking_issues_and_other_macs_stop_planning() {
        let c = sample_catalog();
        let mut s = sample();
        s.issues.push(crate::Issue {
            code: IssueCode::OnBattery,
            blocking: true,
            detail: "Connect a charger.".parse().unwrap(),
        });
        assert!(matches!(
            derive_plan(&s, &c, &request(100 * GIB), 0),
            Err(PlanError::Blocked(_))
        ));
        let mut s = sample();
        s.machine.chip_id = 0x6031;
        assert!(matches!(
            derive_plan(&s, &c, &request(100 * GIB), 0),
            Err(PlanError::NotAdmitted(_))
        ));
    }

    #[test]
    fn free_space_needs_no_shrink() {
        let c = sample_catalog();
        let mut s = sample();
        s.disk.partitions[1].size = 300 * GIB;
        s.disk.containers[0].size = 300 * GIB;
        s.candidates = vec![Candidate::FreeSpace {
            extent: Extent {
                offset: 524312576 + 300 * GIB,
                size: 194 * GIB,
            },
        }];
        s.layout_digest = s.disk.layout_digest().unwrap();
        s.check().unwrap();
        let r = PlanRequest {
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            place: Place::FreeSpace {
                offset: 524312576 + 300 * GIB,
            },
            sekka: 100 * GIB,
        };
        let plan = derive_plan(&s, &c, &r, 0).unwrap();
        assert_eq!(plan.actions.len(), 3);
        assert_eq!(plan.target.offset % ALIGN, 0);
        assert!(plan.target.offset >= 524312576 + 300 * GIB);
    }

    #[test]
    fn free_space_keeps_room_in_macos_for_what_goes_there_meanwhile() {
        // macOS keeps what the engine needs beside Apple's files, as the
        // engine counts it: the downloads still to come, Apple's inputs and
        // MACOS_MARGIN.
        let c = sample_catalog();
        let reserved = c.releases[0].reserved();
        let mut s = sample();
        s.disk.partitions[1].size = 300 * GIB;
        s.disk.containers[0].size = 300 * GIB;
        s.candidates = vec![Candidate::FreeSpace {
            extent: Extent {
                offset: 524312576 + 300 * GIB,
                size: 194 * GIB,
            },
        }];
        s.layout_digest = s.disk.layout_digest().unwrap();
        let r = PlanRequest {
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            place: Place::FreeSpace {
                offset: 524312576 + 300 * GIB,
            },
            sekka: 100 * GIB,
        };
        let staging = 6 * GIB;
        let enough = staging + reserved + MACOS_MARGIN;
        s.disk.containers[0].free = enough;
        assert!(derive_plan(&s, &c, &r, staging).is_ok());
        s.disk.containers[0].free = enough - 1;
        assert_eq!(
            derive_plan(&s, &c, &r, staging),
            Err(PlanError::MacosNeedsRoom {
                needed: enough,
                free: enough - 1,
                downloads: staging
            })
        );
        // Known before the downloads from the place alone, whatever Sekka's
        // size; a shrunk macOS keeps the room in what capacity leaves it.
        let release = &c.releases[0];
        assert_eq!(macos_room(&s, release, &s.candidates[0], staging), derive_plan(&s, &c, &r, staging).map(|_| ()));
        let shrink = sample();
        assert!(matches!(shrink.candidates[0], Candidate::ShrinkContainer { .. }));
        assert_eq!(macos_room(&shrink, release, &shrink.candidates[0], 400 * GIB), Ok(()));
        // Once downloaded, the downloads are in what macOS uses.
        s.disk.containers[0].free = reserved + MACOS_MARGIN;
        assert!(derive_plan(&s, &c, &r, 0).is_ok());
        // With no container holding the running macOS, nothing is known free.
        s.macos.volume_group = "0d4e3e3b-0b3a-4b0a-9a0a-000000000001".parse().unwrap();
        assert!(matches!(derive_plan(&s, &c, &r, 0), Err(PlanError::MacosNeedsRoom { free: 0, .. })));
    }
}
