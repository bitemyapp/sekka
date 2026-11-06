// SPDX-License-Identifier: GPL-3.0-or-later
//! Every journey the app can take, driven through the model with the
//! simulation standing in for the Mac: effects run inline, so each test is
//! the exact sequence of states a person would see.
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::env::Environment;
use crate::sim::{Scenario, Simulation};

fn run(env: &Simulation, effect: Effect) -> Vec<Msg> {
    match effect {
        Effect::Inspect => vec![Msg::Inspected(env.inspect())],
        Effect::LoadCatalog => vec![Msg::Catalog(env.catalog())],
        Effect::HelperStatus => vec![
            Msg::HelperStatus(env.helper_status()),
            Msg::Registration(Ok(env.registration())),
        ],
        Effect::MeasureDownload(r) => vec![Msg::ToDownload(env.still_to_download(&r))],
        Effect::Download(r) => {
            let mut msgs = Vec::new();
            let result = env.download(&r, &AtomicBool::new(false), &mut |p| msgs.push(Msg::DownloadProgress(p)));
            msgs.push(Msg::Downloaded(result));
            msgs
        }
        Effect::CheckRegistration => vec![Msg::Registration(Ok(env.registration()))],
        Effect::RegisterHelper => vec![Msg::Registration(env.register_helper())],
        Effect::Begin(b, p) => vec![Msg::Begun(env.begin(*b, p))],
        Effect::Poll { installation, after, .. } => vec![Msg::Events(env.events(installation, after))],
        Effect::Cancel(id) => vec![Msg::CancelAnswered(env.cancel(id))],
        Effect::RetryStartupDisk(id, p) => vec![Msg::Retried(env.retry_startup_disk(id, p))],
        Effect::Diagnostics(id) => vec![Msg::Diagnostics(env.diagnostics(id))],
        Effect::StopDownload
        | Effect::RemoveStaging
        | Effect::OpenLoginItems
        | Effect::CopyToPasteboard(_)
        | Effect::ShutDown => vec![],
    }
}

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
        let mut queue: VecDeque<Effect> = effects.into();
        let mut steps = 0;
        while let Some(effect) = queue.pop_front() {
            steps += 1;
            assert!(steps < 10_000, "the model never settles");
            if let Effect::CopyToPasteboard(text) = &effect {
                self.copied.push(text.clone());
            }
            for msg in run(&self.env, effect) {
                queue.extend(self.model.update(msg));
            }
        }
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
    fn to_review(&mut self) -> &mut Self {
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
    j.to_review();
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
    j.to_review().confirm().send(Msg::Install).password("wrong");
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
    j.to_review().confirm().send(Msg::Install).password("");
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
    j.to_review().confirm().send(Msg::Install).password("right");
    assert_eq!(j.model.screen, Screen::Stopped);
    assert_eq!(j.model.disk_changed(), Some(DiskChanged::No));
    assert!(j.model.stopped_actions().contains(&sekka_mac_contract::NextAction::Reinspect));
    let failure = j.model.failure().unwrap();
    assert!(failure.apple_diagnostic.as_ref().unwrap().as_str().contains("-69606"));
}

#[test]
fn an_interrupted_write_is_never_retried_by_the_app() {
    let mut j = Journey::start(Scenario::InstallFails);
    j.to_review().confirm().send(Msg::Install).password("right");
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
    j.to_review().confirm().send(Msg::Install).password("right");
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
    j.to_review().confirm().send(Msg::Install);
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
