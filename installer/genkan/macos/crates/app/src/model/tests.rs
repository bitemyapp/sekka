// SPDX-License-Identifier: GPL-3.0-or-later
//! Every journey the app can take, driven through the model with the
//! simulation standing in for the Mac: effects run inline, so each test is
//! the exact sequence of states a person would see.
use super::*;
use crate::env::{Environment, HelperProblem, Registration, ToDownload};
use crate::sim::{Scenario, Simulation, inspection_failed};

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
    /// Install and begin, hearing the helper's answer and its first poll;
    /// the polls after that are the test's to make, as the run goes on.
    fn begin_unwatched(&mut self) -> Uuid {
        self.reach_review().confirm().send(Msg::Install);
        let effects = self.model.update(Msg::Authorize {
            owner: "chris".into(),
            password: Secret::new(b"right".to_vec()),
        });
        let Some(Effect::Begin(begin, password)) = effects.into_iter().next() else { panic!("the helper is asked to begin") };
        let id = self.env.begin(*begin, password).unwrap();
        for effect in self.model.update(Msg::Begun(Ok(id))) {
            if let Effect::Poll { installation, after, .. } = effect {
                let _ = self.model.update(Msg::Events(self.env.events(installation, after)));
            }
        }
        id
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
    m.update(Msg::ToDownload(ToDownload::all(&release)));
    assert_eq!(m.screen, Screen::Plan);
    let (_, max) = m.size_range().unwrap();
    assert_eq!(max, capacity(&release, m.candidate().unwrap(), release.download_bytes()));
}

#[test]
fn files_an_earlier_run_downloaded_are_checked_before_the_review() {
    let mut j = Journey::start(Scenario::Normal);
    // Every file is on the disk already, but none is checked in this run.
    j.send(Msg::ToDownload(ToDownload::NOTHING));
    assert!(j.model.staged.is_empty());
    j.send(Msg::Review);
    assert_eq!(j.model.screen, Screen::Review);
    assert_eq!(j.model.staged.len(), 4, "the review installs the files it checked");
}

#[test]
fn another_releases_files_are_room_for_the_download_not_part_of_it() {
    let mut j = Journey::start(Scenario::Normal);
    let release = j.model.release.clone().unwrap();
    // Every file an earlier run left is another release's: staging removes
    // them before it downloads, so they are room, and none of their bytes
    // is this release's.
    j.send(Msg::ToDownload(ToDownload {
        fetch: release.download_bytes(),
        space: 0,
    }));
    assert_eq!(j.model.screen, Screen::Plan);
    assert_eq!(j.model.to_fetch(), release.download_bytes(), "the plan says all of it is downloaded");
    let (_, max) = j.model.size_range().unwrap();
    assert_eq!(max, capacity(&release, j.model.candidate().unwrap(), 0));
    // So the download is a download, which can be paused, not a check.
    let effects = j.model.update(Msg::StartDownload);
    assert!(matches!(effects.as_slice(), [Effect::Download(_)]));
    assert!(!j.model.checking_download());
    j.settle(effects);
    assert_eq!(j.model.screen, Screen::Review);
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
fn free_space_beside_a_full_macos_is_said_before_anything_is_downloaded() {
    // A large free region after a nearly full macOS: the downloads and
    // Apple's files would go into macOS, which isn't resized.
    let plan_for = |free: u64| {
        let env = Simulation::new(Scenario::Normal, 0.0);
        let (mut m, _) = Model::new("0.1.0".parse().unwrap(), env.user());
        m.update(Msg::HelperStatus(env.helper_status()));
        m.update(Msg::Engine(env.bundled_engine()));
        m.update(Msg::Catalog(env.catalog()));
        let mut inspection = env.inspect().unwrap();
        let group = inspection.macos.volume_group;
        let macos = inspection
            .disk
            .containers
            .iter_mut()
            .find(|k| k.volume_groups.iter().any(|g| g.uuid == group))
            .unwrap();
        macos.free = free;
        inspection.candidates = vec![Candidate::FreeSpace {
            extent: sekka_mac_contract::Extent {
                offset: 300 * sekka_mac_contract::GIB,
                size: 190 * sekka_mac_contract::GIB,
            },
        }];
        m.update(Msg::Inspected(Ok(inspection)));
        let release = m.release.clone().unwrap();
        m.update(Msg::ToDownload(release.download_bytes()));
        assert_eq!(m.screen, Screen::Plan);
        (m, release)
    };
    let (mut m, release) = plan_for(sekka_mac_contract::GIB);
    let error = m.plan_error.clone().expect("said on the plan screen");
    assert!(error.starts_with("macOS needs") && error.contains("nothing is downloaded until it has room"), "{error}");
    assert!(m.macos_room().is_err());
    assert!(m.update(Msg::StartDownload).is_empty());
    assert!(m.update(Msg::Review).is_empty());
    assert_eq!((m.screen, m.downloading), (Screen::Plan, false));
    // With room for the downloads, Apple's files and the margin, it goes on.
    let enough = release.download_bytes() + release.reserved() + sekka_mac_contract::MACOS_MARGIN;
    let (mut m, _) = plan_for(enough);
    assert_eq!((m.plan_error.as_deref(), m.macos_room()), (None, Ok(())));
    assert!(matches!(m.update(Msg::StartDownload).as_slice(), [Effect::Download(_)]));
    // Once downloaded, only Apple's files are still to come.
    let words = text::plan_error(&PlanError::MacosNeedsRoom {
        needed: 2,
        free: 1,
        downloads: 0,
    });
    assert!(words.contains("for Apple's startup files") && !words.contains("downloaded"), "{words}");
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
fn a_look_before_the_review_that_loses_the_release_says_why() {
    let mut j = Journey::start(Scenario::Normal);
    j.reach_review().send(Msg::BackToPlan);
    j.model.update(Msg::Review);
    // The release's profile doesn't admit what this look found.
    let mut i = j.env.inspect().unwrap();
    i.machine.product_type = "Mac15,7".parse().unwrap();
    j.send(Msg::Inspected(Ok(i)));
    assert!(j.model.release.is_none());
    assert_eq!(j.model.screen, Screen::Unsupported, "never left on the downloading page");
}

#[test]
fn the_largest_size_after_the_download_still_leaves_room_for_apples_files() {
    let mut j = Journey::start(Scenario::Normal);
    j.reach_review().send(Msg::BackToPlan).send(Msg::SetSize(u64::MAX));
    assert_eq!(j.model.to_download, Some(ToDownload::NOTHING));
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
    // The retry's records follow the failure's, as the helper's journal
    // keeps them: the stage failed, and then it was done.
    let sequences: Vec<u64> = j.model.records.iter().map(|r| r.sequence).collect();
    assert_eq!(sequences, (1..=sequences.len() as u64).collect::<Vec<_>>());
    let tail: Vec<&sekka_mac_contract::Event> = j.model.records.iter().rev().take(3).map(|r| &r.event).collect();
    assert!(
        matches!(
            tail.as_slice(),
            [
                sekka_mac_contract::Event::Completed { stage: Stage::StartupDisk, .. },
                sekka_mac_contract::Event::Intent { stage: Stage::StartupDisk },
                sekka_mac_contract::Event::Failed { stage: Stage::StartupDisk, .. },
            ]
        ),
        "{tail:?}"
    );
    assert!(j.model.stages().iter().all(|(_, s)| *s == CheckState::Good));
}

#[test]
fn a_refused_retry_says_why_instead_of_that_everything_is_in_place() {
    let mut j = Journey::start(Scenario::StartupDiskRefused);
    j.reach_review().confirm().send(Msg::Install).password("right");
    assert_eq!(j.model.state, Some(InstallState::StartupDiskFailed));
    let (_, before) = j.model.stopped_words().unwrap();
    assert!(before.contains("Everything else is in place"), "{before}");
    let recorded = j.model.records.len();
    j.send(Msg::RetryStartupDisk).password("right");
    assert!(j.model.authorization.is_none());
    assert_eq!(j.model.screen, Screen::Stopped);
    assert_eq!(j.model.records.len(), recorded, "nothing was tried, so nothing was recorded");
    assert_eq!(j.model.notice.as_deref(), Some(crate::text::RETRY_REFUSED));
    // The page gives the helper's reason, and no longer says the rest was
    // checked and is in place.
    let (_, after) = j.model.stopped_words().unwrap();
    assert!(after.contains("isn't as it was left"), "{after}");
    assert!(!after.contains("Everything else is in place"), "{after}");
    assert!(j.model.report(&[]).contains("isn't as it was left"));
}

#[test]
fn a_retry_that_fails_again_shows_this_failure() {
    let mut j = Journey::start(Scenario::StartupDiskFailsAgain);
    j.reach_review().confirm().send(Msg::Install).password("right");
    let (first, _) = j.model.failed().map(|(at, f)| (at, f.clone())).unwrap();
    j.send(Msg::RetryStartupDisk).password("right");
    assert!(j.model.authorization.is_none());
    assert_eq!(j.model.screen, Screen::Stopped);
    assert_eq!(j.model.state, Some(InstallState::StartupDiskFailed));
    // Said, and the page shows the failure it came to this time.
    assert_eq!(j.model.notice.as_deref(), Some(crate::text::RETRY_FAILED));
    let (again, failure) = j.model.failed().unwrap();
    assert!(again > first);
    assert!(failure.message.as_str().contains("reading it back"), "{}", failure.message);
    // Trying once more works, and says nothing of the last time.
    j.send(Msg::RetryStartupDisk);
    assert!(j.model.notice.is_none());
    j.password("right");
    assert_eq!(j.model.screen, Screen::Recovery);
    assert!(j.model.notice.is_none());
}

#[test]
fn reopening_reconnects_to_an_installation() {
    let j = Journey::start(Scenario::WaitingForRecovery);
    assert_eq!(j.model.screen, Screen::Recovery);
    assert!(j.model.installation.is_some());
    // Recovery asks for the administrator the record names.
    assert_eq!(j.model.owner.as_deref(), Some("chris"));
    assert_eq!(j.model.owner_name().as_deref(), Some("Chris Allen"));
    // The helper looked, and found Recovery's step still to do.
    assert_eq!(j.model.state, Some(InstallState::WaitingForRecovery));
    assert!(!j.model.recovery_unconfirmed());

    // A helper that couldn't look can't say whether Recovery is done
    // already, and the page says so.
    let j = Journey::with(Simulation::new(Scenario::WaitingForRecovery, 0.0).refusing_reconcile(inspection_failed()));
    assert_eq!(j.model.screen, Screen::Recovery);
    assert!(j.model.recovery_unconfirmed());
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

    // recoveryOS lists accounts by full name.
    assert_eq!(j.model.owner_name().as_deref(), Some("Sam Admin"));
    assert!(
        crate::text::human_step(sekka_mac_contract::HumanStep::AuthorizeInRecovery, "Sekka", j.model.owner_name().as_deref())
            .contains("Sign in as Sam Admin,")
    );

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

    let j = Journey::start(Scenario::RecoveryFinished);
    assert_eq!(j.model.screen, Screen::Existing);
    assert_eq!(j.model.state, Some(InstallState::WaitingForSetup));
    assert!(j.model.installed());
    assert_eq!(j.model.step(), Step::Account);
    assert_eq!(j.model.unfinished().map(|s| s.state.clone()), Some(InstallState::WaitingForSetup));
}

#[test]
fn reopening_after_recovery_with_a_helper_that_cant_look_says_what_isnt_known() {
    let j = Journey::with(Simulation::new(Scenario::RecoveryFinished, 0.0).refusing_reconcile(inspection_failed()));
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
    let effects = m.update(Msg::Reconciled(Err(inspection_failed())));
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
    let mut j = Journey::start(Scenario::RemovedByHand);
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

    let mut j = Journey::with(Simulation::new(Scenario::RemovedByHand, 0.0).refusing_reconcile(inspection_failed()));
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
    m.update(Msg::Reconciled(env.events(SIM_ID, 0).map_err(|_| inspection_failed())));
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
        m.update(Msg::Reconciled(Err(inspection_failed())));
        assert_eq!(m.screen, Screen::Missing, "{state:?}");
    }
}

#[test]
fn every_installation_that_stands_in_the_way_is_looked_at_by_the_helper() {
    let env = Simulation::new(Scenario::Normal, 0.0);
    // One that never started changing the disk is cancelled instead.
    for state in [InstallState::Planned, InstallState::AssetsVerified] {
        let (m, effects) = hear(&env, known(state.clone()), env.inspect());
        assert!(effects.iter().all(|e| !matches!(e, Effect::Reconcile(_))), "{state:?}: {effects:?}");
        assert_eq!(m.screen, Screen::Stopped, "{state:?}");
        assert!(m.stopped_actions().contains(&sekka_mac_contract::NextAction::Cancel));
    }
    for state in [
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
        // Nothing on the disk names it, as nothing would yet: the helper
        // judges by what is in the plan's space, which the app can't.
        let (mut m, effects) = hear(&env, known(state.clone()), env.inspect());
        assert!(matches!(effects.as_slice(), [Effect::Reconcile(_)]), "{state:?}: {effects:?}");
        // It finds something there, or can't tell: the installation stops
        // the way, and isn't called gone.
        let unchanged = Events {
            records: Vec::new(),
            state: state.clone(),
            running: false,
            progress: None,
        };
        m.update(Msg::Reconciled(Ok(unchanged)));
        assert_eq!(m.screen, Screen::Stopped, "{state:?}");
        assert!(m.stopped_actions().contains(&sekka_mac_contract::NextAction::InspectActualState));
        // Only the helper closes it, when nothing of it is in its space.
        let (mut m, _) = hear(&env, known(state.clone()), env.inspect());
        let removed = Events {
            records: Vec::new(),
            state: InstallState::Removed,
            running: false,
            progress: None,
        };
        m.update(Msg::Reconciled(Ok(removed)));
        assert_eq!(m.screen, Screen::Plan, "{state:?}");
        assert_eq!(m.notice.as_deref(), Some(crate::text::REMOVED));
    }
    // A finished one lets another begin, and the helper doesn't look at it
    // again.
    let (_, effects) = hear(&env, known(InstallState::WaitingForSetup), env.inspect());
    assert!(effects.iter().all(|e| !matches!(e, Effect::Reconcile(_))), "{effects:?}");
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
    let (m, effects) = hear(&env, known(InstallState::WaitingForSetup), unread());
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(m.screen, Screen::Existing);
    assert!(m.installed());

    // On a disk the inspection accounts for, a finished installation that
    // is gone doesn't stand in the way.
    let (m, _) = hear(&env, known(InstallState::WaitingForSetup), env.inspect());
    assert_eq!(m.screen, Screen::Plan);

    // Nor is an unfinished one called gone beside something unidentified.
    let (mut m, _) = hear(&env, known(InstallState::WaitingForRecovery), unread());
    m.update(Msg::Reconciled(Err(inspection_failed())));
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
    let mut j = Journey::start(Scenario::WaitingForRecovery);
    assert_eq!(j.model.reconciling, Some(Reconciling::Answered(SIM_ID)));
    let effects = j.model.update(Msg::CheckAgain);
    j.settle(effects);
    assert_eq!(j.model.screen, Screen::Recovery);
    assert_eq!(j.model.reconciling, Some(Reconciling::Answered(SIM_ID)));
    // Partial, as this Mac's own look found it, is still the helper's to
    // judge: that look, as the person using the app, misses what it can't
    // open.
    let env = Simulation::new(Scenario::WaitingForRecovery, 0.0);
    let mut partial = env.inspect().unwrap();
    partial.installations[0].state = InstallationState::Partial;
    let (_, effects) = hear(&env, env.helper_status(), Ok(partial));
    assert!(matches!(effects.as_slice(), [Effect::Reconcile(_)]), "{effects:?}");
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
fn continuing_from_the_plan_looks_again_without_a_download_page() {
    let mut j = Journey::start(Scenario::Normal);
    j.reach_review().send(Msg::BackToPlan);
    let effects = j.model.update(Msg::Review);
    assert!(matches!(effects.as_slice(), [Effect::Inspect]));
    assert!(j.model.rechecking && !j.model.downloading);
    assert_eq!(j.model.step(), Step::Install, "not back to the download");
    j.settle(effects);
    assert_eq!(j.model.screen, Screen::Review);

    // Files an earlier run left whole are only checked, which can't be
    // paused.
    let mut j = Journey::start(Scenario::Normal);
    j.send(Msg::ToDownload(ToDownload::NOTHING));
    let effects = j.model.update(Msg::Review);
    assert!(matches!(effects.as_slice(), [Effect::Download(_)]));
    assert!(j.model.checking_download());
    j.settle(effects);
    assert!(!j.model.checking_download());
    assert_eq!(j.model.screen, Screen::Review);
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
fn a_run_can_be_cancelled_until_the_disk_starts_to_change() {
    // Kept downloading Apple's files, which changes nothing.
    let mut j = Journey::with(Simulation::new(Scenario::Normal, 0.0).holding_at(Stage::AppleInputs));
    let id = j.begin_unwatched();
    assert_eq!(j.model.screen, Screen::Installing);
    assert!(j.model.cancel_allowed());
    j.send(Msg::CancelInstallation);
    assert!(j.model.notice.is_none(), "{:?}", j.model.notice);
    // The run closes itself as cancelled, and its next poll says so.
    j.send(Msg::Events(j.env.events(id, 0)));
    assert!(!j.model.running);
    assert_eq!(j.model.screen, Screen::Plan);
    assert_eq!(j.model.notice.as_deref(), Some(crate::text::CANCELLED));
    assert!(j.model.installation.is_none());

    // Once the disk is changing, the helper doesn't stop it, and says why.
    let mut j = Journey::with(Simulation::new(Scenario::Normal, 0.0).holding_at(Stage::Shrink));
    let id = j.begin_unwatched();
    assert!(!j.model.cancel_allowed());
    j.send(Msg::CancelInstallation);
    assert!(j.model.notice.as_deref().unwrap().contains("changing the disk right now"));
    assert_eq!(j.model.screen, Screen::Installing);
    assert!(j.env.events(id, 0).unwrap().running);
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
fn nothing_is_planned_while_the_helpers_installations_arent_known() {
    // It can't read one of its records, so it lists none: one may be
    // unfinished, and it would refuse another installation at the end.
    let mut j = Journey::start(Scenario::RecordUnreadable);
    assert_eq!(j.model.screen, Screen::HelperUnknown);
    assert_eq!(j.model.step(), Step::Check);
    assert!(j.model.plan.is_none() && !j.model.downloading);
    assert!(matches!(j.model.helper_problem(), Some(HelperProblem::Other(said)) if said.contains("can't be read")));
    // What it said, naming the record, is in the diagnostics.
    j.send(Msg::CopyDiagnostics);
    assert!(j.copied[0].contains("can't be read") && j.copied[0].contains(&SIM_ID.to_string()), "{}", j.copied[0]);
    // Checking again asks again, and stops in the same place.
    j.send(Msg::CheckAgain);
    assert_eq!(j.model.screen, Screen::HelperUnknown);

    // Nor while a helper that is allowed to run doesn't answer.
    let env = Simulation::new(Scenario::Normal, 0.0);
    let (m, effects) = hear(&env, Err(HelperProblem::Unreachable("The connection was interrupted.".into())), env.inspect());
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(m.screen, Screen::HelperUnknown);
}

#[test]
fn the_password_is_asked_for_once_the_helper_has_said_what_it_knows() {
    // Never set up: nothing was installed from here, so a plan is offered,
    // and the helper is set up and allowed when it is needed.
    let mut j = Journey::start(Scenario::NeedsApproval);
    assert_eq!(j.model.helper_problem(), Some(&HelperProblem::NotRegistered));
    assert_eq!(j.model.screen, Screen::Plan);
    j.reach_review().confirm().send(Msg::Install);
    assert_eq!(j.model.screen, Screen::Approval);
    assert!(j.model.authorization.is_none());
    // Allowed: its installations are asked for, and only then the password.
    j.send(Msg::Tick);
    assert!(matches!(&j.model.helper, Some(Ok(list)) if list.is_empty()));
    assert!(j.model.authorization.is_some());

    // When it knows one that stands in the way, that one comes first, and
    // no password is asked for: here the helper looks, finds nothing of
    // it left, and closes it.
    let mut j = Journey::with(Simulation::new(Scenario::RemovedByHand, 0.0).registered(Registration::Absent));
    assert_eq!(j.model.screen, Screen::Plan);
    j.reach_review().confirm().send(Msg::Install).send(Msg::Tick);
    assert!(j.model.authorization.is_none());
    // The downloads are kept, so the review comes back, of the Mac as it
    // is now.
    assert_eq!(j.model.screen, Screen::Review);
    assert_eq!(j.model.notice.as_deref(), Some(crate::text::REMOVED));
    j.confirm().send(Msg::Install);
    assert!(j.model.authorization.is_some());
    j.password("right");
    assert_eq!(j.model.screen, Screen::Recovery);

    // Checking again comes back to the review, and opens no sheet by
    // itself.
    let mut j = Journey::start(Scenario::Normal);
    j.reach_review().send(Msg::CheckAgain);
    assert_eq!(j.model.screen, Screen::Review);
    assert!(j.model.authorization.is_none());
}

#[test]
fn an_installation_is_found_again_once_its_helper_may_run() {
    // Turned off in Login Items: the record waiting for Recovery can't be
    // read, and nothing else is offered meanwhile.
    let mut j = Journey::start(Scenario::HelperOff);
    assert_eq!(j.model.screen, Screen::HelperUnknown);
    assert!(j.model.installation.is_none() && j.model.plan.is_none());
    let effects = j.model.update(Msg::Tick);
    assert!(matches!(effects.as_slice(), [Effect::CheckRegistration]), "it waits for the owner: {effects:?}");
    j.settle(effects);
    assert_eq!(j.model.screen, Screen::HelperUnknown);
    // Allowed again: this Mac is checked again, and the installation found.
    j.send(Msg::OpenLoginItems).send(Msg::Tick);
    assert_eq!(j.model.screen, Screen::Recovery);
    assert_eq!(j.model.installation, Some(SIM_ID));
    assert_eq!(j.model.state, Some(InstallState::WaitingForRecovery));

    // Not set up, with Sekka on the disk: whether it is unfinished is the
    // helper's to say, so it is set up from the page, then allowed.
    let mut j = Journey::with(Simulation::new(Scenario::WaitingForRecovery, 0.0).registered(Registration::Absent));
    assert_eq!(j.model.screen, Screen::HelperUnknown);
    assert_eq!(j.model.helper_problem(), Some(&HelperProblem::NotRegistered));
    j.send(Msg::RegisterHelper);
    assert_eq!(j.model.registration, Some(Registration::NeedsApproval));
    assert_eq!(j.model.screen, Screen::HelperUnknown);
    j.send(Msg::Tick);
    assert_eq!(j.model.screen, Screen::Recovery);
    assert_eq!(j.model.installation, Some(SIM_ID));
}

#[test]
fn every_scenario_settles_somewhere_sensible() {
    for s in Scenario::ALL {
        let j = Journey::start(s);
        assert_ne!(j.model.screen, Screen::Checking, "{s:?}");
        assert_eq!(Scenario::parse(s.name()), Some(s));
    }
}
