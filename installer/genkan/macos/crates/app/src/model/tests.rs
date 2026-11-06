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
        let env = Simulation::new(scenario, 0.0);
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
