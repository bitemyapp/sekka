// SPDX-License-Identifier: GPL-3.0-or-later
//! What the helper's own inspection, as root, says about a stopped
//! installation that its journal can't: that Recovery's steps took effect,
//! or that nothing of the installation is left on the Mac.
//!
//! Recovery keeps no copy of the helper's journal; it writes its own receipt
//! on the stub, which anyone who can write the stub could also write, so the
//! helper never reads it as evidence. What it trusts is what the engine
//! reads from the disk and the boot policy under the helper, compared with
//! the parts the installation's own records name. Anything it can't tell
//! for certain leaves the journal as it is.
use sekka_mac_contract::{
    ALIGN, Action, Event, Extent, Fact, Inspection, InstallState, Installation, InstallationKind,
    InstallationState, IssueCode, Partition, Plan, Record, Stage, Uuid, Verification,
};

/// What looking at the Mac found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Finding {
    /// Recovery finished: the records that bring the journal up to date.
    RecoveryFinished(Vec<Event>),
    /// Nothing of the installation is on the disk; what was checked.
    Removed(Vec<Fact>),
    /// Nothing to record, and why.
    Unchanged(String),
}

/// Every fact the installation's completed stages recorded, in order.
pub(crate) fn recorded_facts(records: &[Record]) -> Vec<Fact> {
    records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Completed { facts, .. } => Some(facts.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The last value recorded under `name`.
pub(crate) fn fact(facts: &[Fact], name: &str) -> Option<String> {
    facts
        .iter()
        .rev()
        .find(|f| f.name.as_str() == name)
        .map(|f| f.value.to_string())
}

/// The stage the journal says started and has no result.
pub(crate) fn open_stage(records: &[Record]) -> Option<Stage> {
    match records.last().map(|r| &r.event) {
        Some(Event::Intent { stage }) => Some(*stage),
        _ => None,
    }
}

fn evidence(pairs: &[(&str, String)]) -> Vec<Fact> {
    pairs
        .iter()
        .map(|(name, value)| Fact::new(name, value).expect("fact names and values are plain"))
        .collect()
}

/// Look at `inspection` for the installation whose records these are. The
/// caller has already established that it isn't running.
pub(crate) fn look(records: &[Record], state: &InstallState, plan: &Plan, inspection: &Inspection) -> Finding {
    let Some(installation) = records.first().map(|r| r.installation) else {
        return Finding::Unchanged("its record is empty".into());
    };
    if inspection.disk.uuid != plan.disk.disk {
        return Finding::Unchanged("the inspection doesn't show the disk its plan was made for".into());
    }
    if let Err(e) = inspection.check() {
        return Finding::Unchanged(format!("the inspection doesn't agree with itself ({e})"));
    }
    if inspection.issues.iter().any(|i| i.code == IssueCode::AmbiguousDisk) {
        return Finding::Unchanged("the inspection found more than one internal disk or APFS store".into());
    }
    let ours: Vec<&Installation> = inspection
        .installations
        .iter()
        .filter(|i| i.installation_id == Some(installation))
        .collect();
    let waiting = matches!(
        state,
        InstallState::WaitingForRecovery | InstallState::EnrollmentUncertain
    );
    let complete = |i: &Installation| i.kind == InstallationKind::Sekka && i.state == InstallationState::Complete;
    match ours.as_slice() {
        [found] if waiting && complete(found) => match recovery_finished(records, plan, inspection, found) {
            Ok(events) => Finding::RecoveryFinished(events),
            Err(why) => Finding::Unchanged(why.into()),
        },
        [] if !state.allows_new_installation() => match gone(plan, inspection) {
            Ok(facts) => Finding::Removed(facts),
            Err(why) => Finding::Unchanged(why.into()),
        },
        [] => Finding::Unchanged("its record already lets another installation begin".into()),
        [found] => Finding::Unchanged(format!(
            "the inspection finds it on the disk ({:?}), as far as its record goes",
            found.state
        )),
        _ => Finding::Unchanged("more than one installation on the disk names it".into()),
    }
}

/// Whether a partition lies wholly inside an extent.
fn within(extent: Extent, p: &Partition) -> bool {
    p.offset >= extent.offset && p.offset.saturating_add(p.size) <= extent.end()
}

fn overlaps(extent: Extent, p: &Partition) -> bool {
    p.offset < extent.end() && extent.offset < p.offset.saturating_add(p.size)
}

/// Where the engine may put a plan's parts: its target, and the rest of the
/// MiB before it. Free space starts wherever the partition before it ends,
/// and the plan rounds its offset up to the next MiB; the engine creates the
/// stub at the start of the free space, so up to a MiB before the plan's
/// offset (`stage_stub_partition` accepts exactly that).
fn space(plan: &Plan) -> Extent {
    let offset = plan.target.offset.saturating_sub(ALIGN - 1);
    Extent {
        offset,
        size: plan.target.end() - offset,
    }
}

/// Where a stage recorded creating a part: its `{part}-offset` and
/// `{part}-size` facts.
fn recorded_extent(facts: &[Fact], part: &str) -> Option<Extent> {
    let number = |what| fact(facts, &format!("{part}-{what}"))?.parse().ok();
    Some(Extent {
        offset: number("offset")?,
        size: number("size")?,
    })
}

/// The partitions an installation the inspection reports is made of, or
/// none if it names a part that isn't on the disk.
fn parts<'a>(found: &Installation, inspection: &'a Inspection) -> Option<Vec<&'a Partition>> {
    let disk = &inspection.disk;
    let stub = match found.stub_group {
        Some(group) => Some(
            disk.containers
                .iter()
                .find(|c| c.volume_groups.iter().any(|g| g.uuid == group))?
                .partition,
        ),
        None => None,
    };
    [stub, found.esp, found.root]
        .into_iter()
        .flatten()
        .map(|uuid| disk.partitions.iter().find(|p| p.uuid == uuid))
        .collect()
}

/// The engine reports the installation complete: its stub's boot policy
/// names a custom boot object, which only Recovery's two steps (allowing
/// the startup software, then installing it) leave. Its stub, ESP and root
/// must be the partitions its own stages created, at the offsets and sizes
/// they recorded, inside the space its plan gave them.
fn recovery_finished(
    records: &[Record],
    plan: &Plan,
    inspection: &Inspection,
    found: &Installation,
) -> Result<Vec<Event>, &'static str> {
    let (Some(group), Some(esp), Some(root)) = (found.stub_group, found.esp, found.root) else {
        return Err("the inspection doesn't name all of its parts");
    };
    let facts = recorded_facts(records);
    let recorded = |name| fact(&facts, name).and_then(|v| Uuid::parse_any(&v).ok());
    if recorded("stub-volume-group") != Some(group)
        || recorded("esp-partition") != Some(esp)
        || recorded("root-partition") != Some(root)
    {
        return Err("the installation on the disk isn't made of the parts its record names");
    }
    let located = parts(found, inspection).and_then(|ps| <[&Partition; 3]>::try_from(ps).ok());
    let Some([stub_part, esp_part, root_part]) = located else {
        return Err("the inspection doesn't find all of its parts on the disk");
    };
    if recorded("stub-partition") != Some(stub_part.uuid) {
        return Err("the installation on the disk isn't made of the parts its record names");
    }
    let created = [(stub_part, "stub"), (esp_part, "esp"), (root_part, "root")]
        .iter()
        .all(|(p, name)| recorded_extent(&facts, name) == Some(Extent { offset: p.offset, size: p.size }));
    if !created {
        return Err("its parts aren't where its own stages created them");
    }
    if ![stub_part, esp_part, root_part].iter().all(|p| within(space(plan), p)) {
        return Err("its parts aren't where its plan put them");
    }
    let facts = evidence(&[
        ("source", "helper-inspection".into()),
        ("inspection-state", "complete".into()),
        ("stub-group", group.to_string()),
        ("esp-partition", esp.to_string()),
        ("root-partition", root.to_string()),
        ("layout", inspection.layout_digest.to_string()),
    ]);
    // Each of Recovery's steps, from wherever the record left it.
    let open = open_stage(records);
    let mut events = Vec::new();
    for stage in [Stage::Enrollment, Stage::StartupSelection] {
        let done = records
            .iter()
            .any(|r| matches!(&r.event, Event::Completed { stage: s, .. } if *s == stage));
        if done {
            continue;
        }
        if open != Some(stage) {
            events.push(Event::Intent { stage });
        }
        events.push(Event::Completed {
            stage,
            facts: facts.clone(),
            verification: Verification::PolicyReadBack,
        });
    }
    Ok(events)
}

/// Nothing of the installation is on its plan's disk: no partition where
/// the plan put Sekka, and nothing there that could be an unidentified part
/// of it. (No installation naming it was found; the caller checked.)
///
/// One partition may cover that space: the macOS container the plan shrank,
/// from before the installation, when it was never shrunk or has been grown
/// back over the space (the usual last step of removing a system by hand).
/// Partitions can't overlap, so then nothing else is there.
fn gone(plan: &Plan, inspection: &Inspection) -> Result<Vec<Fact>, &'static str> {
    let disk = &inspection.disk;
    let target = plan.target;
    let shrunk = plan.actions.iter().find_map(|a| match a {
        Action::ShrinkContainer { container, .. } => Some(*container),
        _ => None,
    });
    let macos = shrunk
        .and_then(|c| disk.containers.iter().find(|k| k.uuid == c))
        .map(|k| k.partition);
    let mut regrown = false;
    for p in disk.partitions.iter().filter(|p| overlaps(target, p)) {
        if Some(p.uuid) == macos && p.offset < target.offset {
            regrown = true;
        } else {
            return Err("a partition still lies where its plan put Sekka");
        }
    }
    for i in &inspection.installations {
        let unidentified = i.kind == InstallationKind::Sekka && i.installation_id.is_none();
        if !unidentified && i.state != InstallationState::Partial {
            continue;
        }
        match parts(i, inspection) {
            Some(ps) if !ps.is_empty() && !ps.iter().any(|p| overlaps(target, p)) => {}
            _ => return Err("an unidentified Sekka or partial installation may be where its plan put Sekka"),
        }
    }
    Ok(evidence(&[
        ("source", "helper-inspection".into()),
        ("disk", disk.uuid.to_string()),
        ("layout", inspection.layout_digest.to_string()),
        ("target-offset", target.offset.to_string()),
        ("target-size", target.size.to_string()),
        (
            "target-extent",
            if regrown { "macos-container" } else { "unallocated" }.into(),
        ),
        ("installation-id", "not-found".into()),
    ]))
}
