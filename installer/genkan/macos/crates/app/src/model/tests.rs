// SPDX-License-Identifier: GPL-3.0-or-later
//! Every journey the app can take, driven through the model with the
//! simulation standing in for the Mac: effects run inline, so each test is
//! the exact sequence of states a person would see.
use super::*;
use crate::env::Environment;
use crate::sim::{Scenario, Simulation};

struct Journey {
    env: Simulation,
    model: Model,
    copied: Vec<String>,
}

impl Journey {
    fn start(scenario: Scenario) -> Self {
        Self::with(Simulation::new(scenario, 0.0))
    }
    fn with(env: Simulation) -> Self {
        let (model, effects) = Model::new("0.1.0".parse().unwrap(), env.user());
        let mut j = Self {
            env,
            model,
            copied: Vec::new(),
        };
        j.settle(effects);
        j
    }
    fn settle(&mut self, effects: Vec<Effect>) {
        self.copied.extend(crate::sim::settle(&self.env, &mut self.model, effects));
    }
    fn send(&mut self, msg: Msg) -> &mut Self {
        let effects = self.model.update(msg);
        self.settle(effects);
        self
    }
    fn password(&mut self, p: &str) -> &mut Self {
        self.send(Msg::Authorize {
            owner: "chris".into(),
            password: Secret::new(p.as_bytes().to_vec()),
        })
    }
    fn reach_review(&mut self) -> &mut Self {
        assert_eq!(self.model.screen, Screen::Plan);
        self.send(Msg::StartDownload);
        assert_eq!(self.model.screen, Screen::Review, "{:?}", self.model.plan_error);
        self
    }
    fn confirm(&mut self) -> &mut Self {
        self.send(Msg::SetBackup(true));
        for a in self.model.plan.clone().unwrap().acknowledgements {
            self.send(Msg::SetAcknowledged(a, true));
        }
        self
    }
}

#[test]
fn the_whole_journey_ends_in_recovery() {
    let mut j = Journey::start(Scenario::Normal);
    assert_eq!(j.model.screen, Screen::Plan);
    assert!(j.model.checks().iter().all(|c| c.state != CheckState::Pending));
    let (min, max) = j.model.size_range().unwrap();
    assert!(min <= j.model.size && j.model.size <= max);
    j.reach_review();
    assert!(j.model.changes()[0].starts_with("macOS shrinks"));
    // Nothing installs before the backup and acknowledgements.
    j.send(Msg::Install);
    assert!(j.model.authorization.is_none());
    j.confirm().send(Msg::Install);
    assert!(j.model.authorization.as_ref().is_some_and(|a| a.owner == "chris"));
    j.password("correct horse");
    assert!(j.model.authorization.is_none());
    assert_eq!(j.model.screen, Screen::Recovery);
    assert_eq!(j.model.step(), Step::Recovery);
    assert!(j.model.stages().iter().all(|(_, s)| *s == CheckState::Good));
}

#[test]
fn the_plan_waits_for_the_release_and_its_download_size_in_any_order() {
    let env = Simulation::new(Scenario::Normal, 0.0);
    let (mut m, _) = Model::new("0.1.0".parse().unwrap(), env.user());
    assert!(m.update(Msg::HelperStatus(env.helper_status())).is_empty());
    assert!(m.update(Msg::Engine(env.bundled_engine())).is_empty());
    // The catalog before the inspection: there's nothing to choose from yet.
    assert!(m.update(Msg::Catalog(env.catalog())).is_empty());
    let effects = m.update(Msg::Inspected(env.inspect()));
    let release = m.release.clone().expect("the release is chosen once both are known");
    assert!(matches!(effects.as_slice(), [Effect::MeasureDownload(r)] if r.id == release.id));
    // The plan's limits and its button depend on what is left to download.
    assert_eq!(m.screen, Screen::Checking);
    assert_eq!(m.size_range(), None);
    m.update(Msg::ToDownload(release.download_bytes()));
    assert_eq!(m.screen, Screen::Plan);
    let (_, max) = m.size_range().unwrap();
    assert_eq!(max, capacity(&release, m.candidate().unwrap(), release.download_bytes()));
}

#[test]
fn files_an_earlier_run_downloaded_are_checked_before_the_review() {
    let mut j = Journey::start(Scenario::Normal);
    // Every file is on the disk already, but none is checked in this run.
    j.send(Msg::ToDownload(0));
    assert!(j.model.staged.is_empty());
    j.send(Msg::Review);
    assert_eq!(j.model.screen, Screen::Review);
    assert_eq!(j.model.staged.len(), 4, "the review installs the files it checked");
}

#[test]
fn sizes_stay_within_the_release_and_the_disk() {
    let mut j = Journey::start(Scenario::Normal);
    let (min, max) = j.model.size_range().unwrap();
    j.send(Msg::SetSize(u64::MAX));
    assert_eq!(j.model.size, max / ALIGN_FOR_TESTS * ALIGN_FOR_TESTS);
    j.send(Msg::SetSize(1));
    assert_eq!(j.model.size, min);
}
const ALIGN_FOR_TESTS: u64 = sekka_mac_contract::ALIGN;

#[test]
fn other_macs_are_told_why() {
    let j = Journey::start(Scenario::Unsupported);
    assert_eq!(j.model.screen, Screen::Unsupported);
    let refusals = j.model.refusals();
    assert!(matches!(refusals.as_slice(), [sekka_mac_contract::Refusal::UnknownMachine { .. }, ..]));
    assert!(crate::text::refusal(&refusals[0]).contains("Mac15,7"));
}

#[test]
fn an_app_too_old_for_the_catalog_is_told_to_update_before_any_download() {
    let j = Journey::start(Scenario::Outdated);
    assert_eq!(j.model.screen, Screen::NeedsUpdate);
    assert!(j.model.release.is_none());
    assert_eq!(j.model.to_download, None, "nothing was measured for a download");
    let row = j.model.checks().into_iter().find(|c| c.label == "Release").unwrap();
    assert_eq!(row.state, CheckState::Bad);
}

#[test]
fn a_release_this_app_cant_install_is_never_offered() {
    let another_engine = Digest::of(b"another engine");
    let other_protocol = || crate::sim::signed_catalog(|c| c.protocol = HELPER_PROTOCOL + 1);
    for (engine, catalog) in [
        (Ok(another_engine), Ok(crate::sim::signed_catalog(|_| {}))),
        (Simulation::new(Scenario::Normal, 0.0).bundled_engine(), Ok(other_protocol())),
    ] {
        let env = Simulation::new(Scenario::Normal, 0.0);
        let (mut m, _) = Model::new("0.1.0".parse().unwrap(), env.user());
        let mut effects = m.update(Msg::HelperStatus(env.helper_status()));
        effects.extend(m.update(Msg::Engine(engine)));
        effects.extend(m.update(Msg::Catalog(catalog)));
        effects.extend(m.update(Msg::Inspected(env.inspect())));
        assert!(effects.is_empty(), "nothing is measured or downloaded: {effects:?}");
        assert!(m.needs_update());
        assert_eq!(m.screen, Screen::NeedsUpdate);
    }
}

#[test]
fn a_mac_on_battery_is_not_ready() {
    let j = Journey::start(Scenario::NotReady);
    assert_eq!(j.model.screen, Screen::NotReady);
    let power = j.model.checks().into_iter().find(|c| c.label == "Power").unwrap();
    assert_eq!(power.state, CheckState::Bad);
}

#[test]
fn offline_and_low_space_stop_early() {
    assert_eq!(Journey::start(Scenario::Offline).model.screen, Screen::NoRelease);
    assert_eq!(Journey::start(Scenario::LowSpace).model.screen, Screen::NotReady);
}

#[test]
fn the_review_names_the_startup_disk_and_promises_nothing_it_cant_keep() {
    let mut j = Journey::start(Scenario::Normal);
    j.reach_review();
    let changes = j.model.changes();
    assert!(changes[0].starts_with("macOS shrinks"));
    assert!(!changes[0].contains("nothing is changed"), "{}", changes[0]);
    let last = changes.last().unwrap();
    assert!(last.contains("startup disk") && last.contains("Macintosh HD"), "{last}");
    // What a failure left is said for the stage it failed in.
    let (_, resized) = crate::text::stopped(&InstallState::Failed {
        stage: Stage::Shrink,
        disk_changed: DiskChanged::Yes,
    });
    assert!(resized.contains("None of Sekka's partitions were created"), "{resized}");
    let (_, started) = crate::text::stopped(&InstallState::MutationStarted { stage: Stage::Shrink });
    assert!(started.contains("may have changed"), "{started}");
}

#[test]
fn a_wrong_password_is_asked_for_again_in_place() {
    let mut j = Journey::start(Scenario::WrongPassword);
    j.reach_review().confirm().send(Msg::Install).password("wrong");
    let auth = j.model.authorization.clone().unwrap();
    assert!(auth.error.unwrap().contains("isn't right"));
    assert!(!auth.busy);
    assert_eq!(j.model.screen, Screen::Review);
    j.password("right");
    assert_eq!(j.model.screen, Screen::Recovery);
}

#[test]
fn an_empty_password_never_reaches_the_helper() {
    let mut j = Journey::start(Scenario::Normal);
    j.reach_review().confirm().send(Msg::Install).password("");
    assert!(j.model.authorization.as_ref().unwrap().error.is_some());
    assert_eq!(j.model.screen, Screen::Review);
}

#[test]
fn a_stopped_download_can_be_resumed() {
    let mut j = Journey::start(Scenario::DownloadFails);
    j.send(Msg::StartDownload);
    assert_eq!(j.model.screen, Screen::Downloading);
    assert!(!j.model.downloading);
    assert!(j.model.download_error.as_ref().unwrap().contains("kept"));
}

#[test]
fn a_charger_unplugged_during_the_download_is_named_before_the_review() {
    let mut j = Journey::start(Scenario::Unplugged);
    assert_eq!(j.model.screen, Screen::Plan);
    j.send(Msg::StartDownload);
    assert_eq!(j.model.screen, Screen::NotReady, "{:?}", j.model.plan_error);
    let blocking: Vec<IssueCode> = j.model.inspection().unwrap().blocking().map(|i| i.code).collect();
    assert_eq!(blocking, [IssueCode::OnBattery]);
    assert!(j.model.plan.is_none());
}

#[test]
fn a_failed_look_after_the_download_offers_a_way_on() {
    let mut j = Journey::start(Scenario::Normal);
    let release = j.model.release.clone().unwrap();
    j.model.update(Msg::StartDownload);
    let files = j
        .env
        .download(&release, &std::sync::atomic::AtomicBool::new(false), &mut |_| {})
        .unwrap();
    let effects = j.model.update(Msg::Downloaded(Ok(files)));
    assert!(matches!(effects.as_slice(), [Effect::Inspect]));
    assert_eq!(j.model.screen, Screen::Downloading);
    j.send(Msg::Inspected(Err("The engine stopped.".into())));
    assert_eq!(j.model.screen, Screen::Plan);
    assert!(j.model.plan_error.as_ref().unwrap().contains("The engine stopped."));
    assert!(j.model.size_range().is_some(), "the plan keeps the last good look at the disk");
    // Continue looks again, and the review follows this time.
    j.send(Msg::Review);
    assert_eq!(j.model.screen, Screen::Review);
    assert!(j.model.plan_error.is_none());
}

#[test]
fn the_largest_size_after_the_download_still_leaves_room_for_apples_files() {
    let mut j = Journey::start(Scenario::Normal);
    j.reach_review().send(Msg::BackToPlan).send(Msg::SetSize(u64::MAX));
    assert_eq!(j.model.to_download, Some(0));
    let release = j.model.release.clone().unwrap();
    assert!(release.reserved() > 0);
    let Some(Candidate::ShrinkContainer { available, adjacent_free, .. }) = j.model.candidate().cloned() else {
        panic!("the sample shrinks macOS")
    };
    assert!(j.model.size + release.reserved() <= available + adjacent_free);
    j.send(Msg::Review);
    assert_eq!(j.model.screen, Screen::Review, "{:?}", j.model.plan_error);
    assert_eq!(j.model.plan.as_ref().unwrap().allocation.sekka, j.model.size);
}

#[test]
fn a_failed_resize_says_nothing_changed_and_offers_a_new_plan() {
    let mut j = Journey::start(Scenario::ResizeFails);
    j.reach_review().confirm().send(Msg::Install).password("right");
    assert_eq!(j.model.screen, Screen::Stopped);
    assert_eq!(j.model.disk_changed(), Some(DiskChanged::No));
    assert!(j.model.stopped_actions().contains(&sekka_mac_contract::NextAction::Reinspect));
    let failure = j.model.failure().unwrap();
    assert!(failure.apple_diagnostic.as_ref().unwrap().as_str().contains("-69606"));
}

#[test]
fn an_interrupted_write_is_never_retried_by_the_app() {
    let mut j = Journey::start(Scenario::InstallFails);
    j.reach_review().confirm().send(Msg::Install).password("right");
    assert_eq!(j.model.screen, Screen::Stopped);
    assert_eq!(j.model.disk_changed(), Some(DiskChanged::Unknown));
    let actions = j.model.stopped_actions();
    assert!(!actions.contains(&sekka_mac_contract::NextAction::Reinspect));
    assert!(actions.contains(&sekka_mac_contract::NextAction::InspectActualState));
    j.send(Msg::CopyDiagnostics);
    assert!(j.copied[0].contains("Simulated diagnostics"));
    assert!(!j.copied[0].contains(" chris "));
}

#[test]
fn only_the_startup_disk_is_retried() {
    let mut j = Journey::start(Scenario::StartupDiskFails);
    j.reach_review().confirm().send(Msg::Install).password("right");
    assert_eq!(j.model.state, Some(InstallState::StartupDiskFailed));
    assert_eq!(j.model.screen, Screen::Stopped);
    j.send(Msg::RetryStartupDisk);
    assert!(j.model.authorization.as_ref().unwrap().retry);
    j.password("right");
    assert_eq!(j.model.screen, Screen::Recovery);
}

#[test]
fn reopening_reconnects_to_an_installation() {
    let j = Journey::start(Scenario::WaitingForRecovery);
    assert_eq!(j.model.screen, Screen::Recovery);
    assert!(j.model.installation.is_some());
    // Recovery asks for the administrator the record names.
    assert_eq!(j.model.owner.as_deref(), Some("chris"));
    assert_eq!(j.model.owner_name().as_deref(), Some("Chris Allen"));
    // This branch's helper can't read the boot policy, so whether Recovery
    // is done already is said to be unknown.
    assert!(j.model.recovery_unconfirmed());

    // A helper that looks, and finds Recovery's step still to do.
    let j = Journey::with(Simulation::new(Scenario::WaitingForRecovery, 0.0).reconciling());
    assert_eq!(j.model.screen, Screen::Recovery);
    assert_eq!(j.model.state, Some(InstallState::WaitingForRecovery));
    assert!(!j.model.recovery_unconfirmed());
}

#[test]
fn the_administrator_who_authorized_is_the_one_recovery_and_a_retry_ask_for() {
    let mut j = Journey::start(Scenario::StartupDiskFails);
    j.reach_review().confirm().send(Msg::Install).send(Msg::Authorize {
        owner: "admin".into(),
        password: Secret::new(b"right".to_vec()),
    });
    assert_eq!(j.model.screen, Screen::Stopped);
    assert_eq!(j.model.owner.as_deref(), Some("admin"));
    // Another administrator than the app's user: named as typed.
    assert_eq!(j.model.owner_name().as_deref(), Some("admin"));
    assert!(crate::text::human_step(sekka_mac_contract::HumanStep::AuthorizeInRecovery, "Sekka", j.model.owner_name().as_deref()).contains("admin"));

    j.send(Msg::RetryStartupDisk);
    assert_eq!(j.model.authorization.as_ref().unwrap().owner, "admin");
    // Another name is refused in the sheet, not silently swapped for the
    // recorded one.
    let effects = j.model.update(Msg::Authorize {
        owner: "chris".into(),
        password: Secret::new(b"right".to_vec()),
    });
    assert!(effects.is_empty());
    let auth = j.model.authorization.clone().unwrap();
    assert!(auth.error.unwrap().contains("admin"));
    assert!(!auth.busy);
    j.send(Msg::Authorize {
        owner: "Admin".into(),
        password: Secret::new(b"right".to_vec()),
    });
    assert_eq!(j.model.screen, Screen::Recovery);
}

/// The start's results, one at a time, from a simulation: what the model
/// asks for next is returned rather than run.
fn start(scenario: Scenario) -> (Simulation, Model, Vec<Effect>) {
    let env = Simulation::new(scenario, 0.0);
    let status = env.helper_status();
    let inspection = env.inspect();
    let (m, effects) = hear(&env, status, inspection);
    (env, m, effects)
}

/// The start's results, with the helper's installations and this Mac's
/// inspection as given.
fn hear(
    env: &Simulation,
    status: Result<Vec<Summary>, crate::env::HelperProblem>,
    inspection: Result<Inspection, String>,
) -> (Model, Vec<Effect>) {
    let (mut m, _) = Model::new("0.1.0".parse().unwrap(), env.user());
    let mut effects = m.update(Msg::HelperStatus(status));
    effects.extend(m.update(Msg::Engine(env.bundled_engine())));
    effects.extend(m.update(Msg::Catalog(env.catalog())));
    effects.extend(m.update(Msg::Inspected(inspection)));
    for e in std::mem::take(&mut effects) {
        match e {
            Effect::MeasureDownload(r) => effects.extend(m.update(Msg::ToDownload(env.still_to_download(&r)))),
            other => effects.push(other),
        }
    }
    (m, effects)
}

const SIM_ID: Uuid = Uuid::from_bytes([0x5e; 16]);

/// One installation the helper knows, stopped in `state`.
fn known(state: InstallState) -> Result<Vec<Summary>, crate::env::HelperProblem> {
    Ok(vec![Summary {
        installation: SIM_ID,
        release: "sekka-j514s-4882dc33".parse().unwrap(),
        state,
        opened: sekka_mac_contract::Timestamp::now(),
        running: false,
    }])
}

/// What the helper on this branch answers when asked to reconcile.
fn cant_look() -> HelperRefusal {
    HelperRefusal {
        code: HelperRefusalCode::State,
        message: "This helper can't look at the Mac for an installation yet.".parse().unwrap(),
    }
}

/// An installation on the disk, as this Mac's own inspection reports it.
fn on_disk(kind: InstallationKind, state: InstallationState, id: Option<Uuid>) -> sekka_mac_contract::Installation {
    sekka_mac_contract::Installation {
        kind,
        name: "Sekka".parse().unwrap(),
        state,
        stub_group: None,
        esp: None,
        root: None,
        installation_id: id,
    }
}

#[test]
fn reopening_after_recovery_finished_shows_sekka_installed() {
    let (_, m, effects) = start(Scenario::RecoveryFinished);
    // The record still waits for Recovery, and the app's own inspection
    // can't read the boot policy: the helper is asked to look before
    // anything is shown.
    assert_eq!(m.on_disk(SIM_ID).map(|x| x.state), Some(InstallationState::Unknown));
    assert!(matches!(effects.as_slice(), [Effect::Reconcile(_)]), "{effects:?}");
    assert_eq!(m.screen, Screen::Checking);

    let j = Journey::with(Simulation::new(Scenario::RecoveryFinished, 0.0).reconciling());
    assert_eq!(j.model.screen, Screen::Existing);
    assert_eq!(j.model.state, Some(InstallState::WaitingForSetup));
    assert!(j.model.installed());
    assert_eq!(j.model.step(), Step::Account);
    assert_eq!(j.model.unfinished().map(|s| s.state.clone()), Some(InstallState::WaitingForSetup));
}

#[test]
fn reopening_after_recovery_with_a_helper_that_cant_look_says_what_isnt_known() {
    let j = Journey::start(Scenario::RecoveryFinished);
    assert_eq!(j.model.screen, Screen::Recovery);
    assert_eq!(j.model.state, Some(InstallState::WaitingForRecovery));
    assert!(j.model.recovery_unconfirmed());
    assert!(crate::text::RECOVERY_UNCONFIRMED.contains("Sekka is installed"));
}

#[test]
fn a_complete_sekka_is_shown_installed_even_when_the_helper_cant_say_so() {
    let env = Simulation::new(Scenario::RecoveryFinished, 0.0);
    let complete = || {
        let mut i = env.inspect().unwrap();
        i.installations[0].state = InstallationState::Complete;
        Ok(i)
    };
    let (mut m, effects) = hear(&env, env.helper_status(), complete());
    assert!(matches!(effects.as_slice(), [Effect::Reconcile(_)]), "{effects:?}");
    let effects = m.update(Msg::Reconciled(Err(cant_look())));
    assert_eq!(m.screen, Screen::Existing, "never Finish in Recovery");
    assert!(m.installed());
    assert!(effects.is_empty(), "the helper is asked once: {effects:?}");

    // Nor when it looks and changes nothing.
    let (mut m, _) = hear(&env, env.helper_status(), complete());
    let unchanged = env.events(SIM_ID, 0).unwrap();
    assert_eq!(unchanged.state, InstallState::WaitingForRecovery);
    m.update(Msg::Reconciled(Ok(unchanged)));
    assert_eq!(m.screen, Screen::Existing);
}

#[test]
fn an_installation_removed_by_hand_lets_a_new_one_begin() {
    let mut j = Journey::with(Simulation::new(Scenario::RemovedByHand, 0.0).reconciling());
    assert_eq!(j.model.screen, Screen::Plan);
    assert_eq!(j.model.notice.as_deref(), Some(crate::text::REMOVED));
    assert!(j.model.installation.is_none());
    assert!(j.model.unfinished().is_none());
    j.reach_review().confirm().send(Msg::Install).password("right");
    assert_eq!(j.model.screen, Screen::Recovery);
}

#[test]
fn an_installation_removed_by_hand_is_named_when_the_helper_cant_close_it() {
    let (_, _, effects) = start(Scenario::RemovedByHand);
    assert!(matches!(effects.as_slice(), [Effect::Reconcile(_)]), "{effects:?}");

    let mut j = Journey::start(Scenario::RemovedByHand);
    // Never the Recovery steps for an installation that isn't there.
    assert_eq!(j.model.screen, Screen::Missing);
    assert_eq!(j.model.step(), Step::Check);
    assert_eq!(j.model.state, Some(InstallState::WaitingForRecovery));
    assert!(j.model.plan.is_none());
    j.send(Msg::CopyDiagnostics);
    assert!(j.copied[0].contains("Simulated diagnostics"), "the helper's own report");
    // Checking again asks the helper again, and finds the same.
    j.send(Msg::CheckAgain);
    assert_eq!(j.model.screen, Screen::Missing);

    // A helper that looks and leaves the record as it was is no better.
    let env = Simulation::new(Scenario::RemovedByHand, 0.0);
    let (mut m, _) = hear(&env, env.helper_status(), env.inspect());
    m.update(Msg::Reconciled(env.events(SIM_ID, 0).map_err(|_| cant_look())));
    assert_eq!(m.screen, Screen::Missing);

    // Every state that names itself on the disk, and isn't there.
    for state in [
        InstallState::MutationStarted { stage: Stage::Root },
        InstallState::Failed {
            stage: Stage::Recovery,
            disk_changed: DiskChanged::Unknown,
        },
        InstallState::StartupDiskFailed,
        InstallState::EnrollmentUncertain,
    ] {
        let (mut m, effects) = hear(&env, known(state.clone()), env.inspect());
        assert!(matches!(effects.as_slice(), [Effect::Reconcile(_)]), "{state:?}: {effects:?}");
        m.update(Msg::Reconciled(Err(cant_look())));
        assert_eq!(m.screen, Screen::Missing, "{state:?}");
    }
}

#[test]
fn an_installation_that_stopped_before_naming_itself_on_the_disk_is_never_called_removed() {
    let env = Simulation::new(Scenario::Normal, 0.0);
    for state in [
        InstallState::Planned,
        InstallState::AssetsVerified,
        InstallState::MutationStarted { stage: Stage::Shrink },
        InstallState::MutationStarted { stage: Stage::StubPartition },
        InstallState::MutationStarted { stage: Stage::Esp },
        InstallState::Failed {
            stage: Stage::Shrink,
            disk_changed: DiskChanged::Unknown,
        },
        InstallState::Failed {
            stage: Stage::Shrink,
            disk_changed: DiskChanged::Yes,
        },
        InstallState::Failed {
            stage: Stage::Partitions,
            disk_changed: DiskChanged::Unknown,
        },
    ] {
        // Nothing on the disk names it, as nothing would yet.
        let (m, effects) = hear(&env, known(state.clone()), env.inspect());
        assert!(effects.iter().all(|e| !matches!(e, Effect::Reconcile(_))), "{state:?}: {effects:?}");
        assert_eq!(m.screen, Screen::Stopped, "{state:?}");
    }
}

#[test]
fn a_finished_installation_this_mac_cant_identify_is_not_offered_a_second() {
    let env = Simulation::new(Scenario::Normal, 0.0);
    // The inspection couldn't read the stub's records, so it reports it
    // unidentified.
    let unread = || {
        let mut i = env.inspect().unwrap();
        i.installations.push(on_disk(InstallationKind::OtherLinux, InstallationState::Unknown, None));
        Ok(i)
    };
    let (mut m, effects) = hear(&env, known(InstallState::WaitingForSetup), unread());
    assert!(matches!(effects.as_slice(), [Effect::Reconcile(_)]), "the helper is asked: {effects:?}");
    m.update(Msg::Reconciled(Err(cant_look())));
    assert_eq!(m.screen, Screen::Existing);
    assert!(m.installed());

    // On a disk the inspection accounts for, a finished installation that
    // is gone doesn't stand in the way, whatever the helper says.
    let (mut m, _) = hear(&env, known(InstallState::WaitingForSetup), env.inspect());
    m.update(Msg::Reconciled(Err(cant_look())));
    assert_eq!(m.screen, Screen::Plan);

    // Nor is an unfinished one called gone beside something unidentified.
    let (mut m, _) = hear(&env, known(InstallState::WaitingForRecovery), unread());
    m.update(Msg::Reconciled(Err(cant_look())));
    assert_eq!(m.screen, Screen::Recovery);
}

#[test]
fn an_installation_found_removed_while_watching_it_is_closed() {
    let mut j = Journey::start(Scenario::InstallFails);
    j.reach_review().confirm().send(Msg::Install).password("right");
    assert_eq!(j.model.screen, Screen::Stopped);
    let mut events = j.env.events(j.model.installation.unwrap(), 0).unwrap();
    events.state = InstallState::Removed;
    j.send(Msg::Events(Ok(events)));
    assert_eq!(j.model.screen, Screen::Plan);
    assert_eq!(j.model.notice.as_deref(), Some(crate::text::REMOVED));
    assert!(j.model.installation.is_none());
}

#[test]
fn the_helper_is_asked_about_recovery_once_in_each_check() {
    let mut j = Journey::with(Simulation::new(Scenario::WaitingForRecovery, 0.0).reconciling());
    assert_eq!(j.model.reconciling, Some(Reconciling::Answered(SIM_ID)));
    let effects = j.model.update(Msg::CheckAgain);
    j.settle(effects);
    assert_eq!(j.model.screen, Screen::Recovery);
    // A partial installation has nothing Recovery could have finished.
    let env = Simulation::new(Scenario::WaitingForRecovery, 0.0);
    let mut partial = env.inspect().unwrap();
    partial.installations[0].state = InstallationState::Partial;
    let (_, effects) = hear(&env, env.helper_status(), Ok(partial));
    assert!(effects.iter().all(|e| !matches!(e, Effect::Reconcile(_))), "{effects:?}");
}

#[test]
fn checking_again_waits_for_an_open_sheet() {
    let mut j = Journey::start(Scenario::StartupDiskFails);
    j.reach_review().confirm().send(Msg::Install).password("right");
    j.send(Msg::RetryStartupDisk);
    assert!(j.model.update(Msg::CheckAgain).is_empty());
    assert_eq!(j.model.screen, Screen::Stopped);
    assert!(j.model.installation.is_some());
    j.password("right");
    assert_eq!(j.model.screen, Screen::Recovery);

    // A retry with nothing to retry says so and leaves the sheet usable.
    let mut j = Journey::start(Scenario::StartupDiskFails);
    j.reach_review().confirm().send(Msg::Install).password("right");
    j.send(Msg::RetryStartupDisk);
    j.model.installation = None;
    let effects = j.model.update(Msg::Authorize {
        owner: "chris".into(),
        password: Secret::new(b"right".to_vec()),
    });
    assert!(effects.is_empty());
    let auth = j.model.authorization.clone().unwrap();
    assert!(!auth.busy);
    assert_eq!(auth.error.as_deref(), Some(crate::text::NOTHING_TO_RETRY));
}

#[test]
fn the_review_names_the_macos_this_mac_starts() {
    let env = Simulation::new(Scenario::Normal, 0.0);
    let mut i = env.inspect().unwrap();
    // Another macOS, in its own container, listed first.
    let mut other = i.disk.containers[0].clone();
    other.uuid = Uuid::from_bytes([0x0c; 16]);
    other.volume_groups[0].uuid = Uuid::from_bytes([0x0d; 16]);
    other.volume_groups[0].name = "Sequoia".parse().unwrap();
    i.disk.containers[0].volume_groups[0].name = "Work".parse().unwrap();
    i.disk.containers.insert(0, other);
    let (mut m, effects) = hear(&env, Ok(Vec::new()), Ok(i.clone()));
    assert!(effects.is_empty());
    assert_eq!(m.macos_volume(), "Work");
    // Without the running one, the macOS whose space Sekka takes.
    i.macos.volume_group = Uuid::from_bytes([0x0e; 16]);
    m.update(Msg::Inspected(Ok(i)));
    assert_eq!(m.macos_volume(), "Work");
}

#[test]
fn an_installation_stopped_before_any_change_can_be_cancelled() {
    let mut j = Journey::start(Scenario::StoppedEarly);
    assert_eq!(j.model.screen, Screen::Stopped);
    let state = j.model.state.clone().unwrap();
    assert_eq!(state, InstallState::AssetsVerified);
    let (_, explanation) = crate::text::stopped(&state);
    assert!(explanation.contains("before anything on your Mac's disk changed"), "{explanation}");
    assert!(j.model.stopped_actions().contains(&sekka_mac_contract::NextAction::Cancel));
    // Checking again finds the same installation; cancelling is the way on.
    j.send(Msg::CheckAgain);
    assert_eq!(j.model.screen, Screen::Stopped);
    j.send(Msg::CancelInstallation);
    assert_eq!(j.model.screen, Screen::Plan);
    assert_eq!(j.model.notice.as_deref(), Some(crate::text::CANCELLED));
    assert!(j.model.unfinished().is_none());
    assert!(j.model.installation.is_none());
}

#[test]
fn every_stopped_state_is_explained_in_words() {
    let states = [
        InstallState::Planned,
        InstallState::AssetsVerified,
        InstallState::MutationStarted { stage: Stage::Root },
        InstallState::Failed {
            stage: Stage::Shrink,
            disk_changed: DiskChanged::Unknown,
        },
        InstallState::StartupDiskFailed,
        InstallState::WaitingForRecovery,
        InstallState::EnrollmentUncertain,
        InstallState::WaitingForSetup,
        InstallState::Cancelled,
        InstallState::Removed,
    ];
    for state in states {
        let (headline, explanation) = crate::text::stopped(&state);
        assert_ne!(explanation, format!("{state:?}"), "{headline}");
        assert!(explanation.ends_with('.'), "{explanation}");
    }
}

#[test]
fn an_existing_installation_is_recognized_not_replaced() {
    let j = Journey::start(Scenario::Existing);
    assert_eq!(j.model.screen, Screen::Existing);
}

#[test]
fn the_helper_waits_for_approval_in_login_items() {
    let mut j = Journey::start(Scenario::NeedsApproval);
    j.reach_review().confirm().send(Msg::Install);
    assert_eq!(j.model.screen, Screen::Approval);
    j.send(Msg::Tick);
    assert_eq!(j.model.screen, Screen::Review);
    assert!(j.model.authorization.is_some());
}

#[test]
fn every_scenario_settles_somewhere_sensible() {
    for s in Scenario::ALL {
        let j = Journey::start(s);
        assert_ne!(j.model.screen, Screen::Checking, "{s:?}");
        assert_eq!(Scenario::parse(s.name()), Some(s));
    }
}
