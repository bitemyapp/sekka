// SPDX-License-Identifier: GPL-3.0-or-later
//! The simulation: the whole journey with nothing real behind it, for
//! development, review and screenshots. It never touches a disk, the helper,
//! the network or an account; every "download" and every stage is a timer.
//! Production builds don't contain it.
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sekka_mac_contract::*;
use sekka_mac_stage::StagedFile;
use sekka_mac_trust::{TrustConfig, Verified};

use crate::env::*;
use crate::model::{Effect, Model, Msg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Everything works.
    Normal,
    /// Another Mac model.
    Unsupported,
    /// On battery.
    NotReady,
    /// The catalog can't be fetched.
    Offline,
    /// The catalog needs a newer Sekka Installer.
    Outdated,
    /// Not enough room.
    LowSpace,
    /// The first password is refused.
    WrongPassword,
    /// The download stops part way.
    DownloadFails,
    /// The charger is unplugged during the download.
    Unplugged,
    /// Apple's resize fails with -69606 and nothing changes.
    ResizeFails,
    /// The root write dies part way.
    InstallFails,
    /// Setting the startup disk fails once.
    StartupDiskFails,
    /// The app reopens with macOS's part done.
    WaitingForRecovery,
    /// The app reopens after Recovery finished; the helper's record still
    /// waits for Recovery until the helper looks at the Mac, which only a
    /// [`Simulation::reconciling`] helper does.
    RecoveryFinished,
    /// The app reopens after the owner removed an unfinished installation
    /// in Disk Utility; only a [`Simulation::reconciling`] helper closes
    /// its record.
    RemovedByHand,
    /// The app reopens to an installation whose helper stopped while
    /// downloading Apple's files, before anything on the disk changed.
    StoppedEarly,
    /// Sekka is already installed.
    Existing,
    /// The helper must be allowed in Login Items first.
    NeedsApproval,
}

impl Scenario {
    pub const ALL: [Scenario; 18] = [
        Scenario::Normal,
        Scenario::Unsupported,
        Scenario::NotReady,
        Scenario::Offline,
        Scenario::Outdated,
        Scenario::LowSpace,
        Scenario::WrongPassword,
        Scenario::DownloadFails,
        Scenario::Unplugged,
        Scenario::ResizeFails,
        Scenario::InstallFails,
        Scenario::StartupDiskFails,
        Scenario::WaitingForRecovery,
        Scenario::RecoveryFinished,
        Scenario::RemovedByHand,
        Scenario::StoppedEarly,
        Scenario::Existing,
        Scenario::NeedsApproval,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scenario::Normal => "normal",
            Scenario::Unsupported => "unsupported",
            Scenario::NotReady => "not-ready",
            Scenario::Offline => "offline",
            Scenario::Outdated => "outdated",
            Scenario::LowSpace => "low-space",
            Scenario::WrongPassword => "wrong-password",
            Scenario::DownloadFails => "download-fails",
            Scenario::Unplugged => "unplugged",
            Scenario::ResizeFails => "resize-fails",
            Scenario::InstallFails => "install-fails",
            Scenario::StartupDiskFails => "startup-disk-fails",
            Scenario::WaitingForRecovery => "waiting-for-recovery",
            Scenario::RecoveryFinished => "recovery-finished",
            Scenario::RemovedByHand => "removed-by-hand",
            Scenario::StoppedEarly => "stopped-early",
            Scenario::Existing => "existing",
            Scenario::NeedsApproval => "needs-approval",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

pub struct Simulation {
    scenario: Scenario,
    /// How fast time passes: 1.0 is a believable pace; screenshots use 0.
    pace: f64,
    /// The helper looks at the Mac when asked to reconcile, as the contract
    /// describes. The helper on this branch refuses, and so does the
    /// simulation's unless it is made [`Simulation::reconciling`].
    reconciles: bool,
    state: Mutex<SimState>,
}

#[derive(Default)]
struct SimState {
    password_tries: u32,
    approved_at: Option<Instant>,
    installation: Option<(Uuid, Instant)>,
    startup_retried: bool,
    /// The release's files are "staged".
    downloaded: bool,
    /// The helper looked at the Mac and brought its record up to date.
    reconciled: bool,
    /// The installation the app reopened to was cancelled.
    cancelled: bool,
    /// The administrator who authorized the installation.
    owner: Option<String>,
}

const SIM_ID: Uuid = Uuid::from_bytes([0x5e; 16]);

impl Simulation {
    pub fn new(scenario: Scenario, pace: f64) -> Self {
        Self {
            scenario,
            pace,
            reconciles: false,
            state: Mutex::new(SimState::default()),
        }
    }

    /// The same simulation, with a helper that reconciles.
    pub fn reconciling(mut self) -> Self {
        self.reconciles = true;
        self
    }

    fn wait(&self, ms: u64) {
        if self.pace > 0.0 {
            std::thread::sleep(Duration::from_millis((ms as f64 * self.pace) as u64));
        }
    }

    /// An installation's records: up to `upto` (stages done, and one that
    /// fails), or all of macOS's part, then `tail`.
    fn records(&self, upto: Option<(usize, Option<Stage>)>, tail: Vec<Event>) -> (Vec<Record>, InstallState) {
        let plan = samples::plan();
        let owner = self.state.lock().unwrap().owner.clone().unwrap_or_else(|| "chris".into());
        let mut out = Vec::new();
        let mut push = |event: Event| {
            out.push(Record {
                schema: RecordSchema,
                installation: SIM_ID,
                sequence: out.len() as u64 + 1,
                previous: None,
                at: Timestamp::now(),
                event,
            })
        };
        push(Event::Opened {
            plan_digest: plan.digest().unwrap(),
            plan: Box::new(plan),
            app: "0.1.0".parse().unwrap(),
            helper: "0.1.0".parse().unwrap(),
        });
        let mut stages = vec![Stage::Import];
        stages.extend(macos_stages(true));
        let (done, failing) = upto.unwrap_or((stages.len(), None));
        for stage in stages.iter().take(done) {
            push(Event::Intent { stage: *stage });
            push(Event::Completed {
                stage: *stage,
                facts: match stage {
                    Stage::Import => vec![Fact::new("owner", &owner).unwrap()],
                    _ => vec![],
                },
                verification: Verification::Inspected,
            });
        }
        if let Some(stage) = failing {
            push(Event::Intent { stage });
            let (changed, message, apple) = match stage {
                Stage::Shrink => (
                    DiskChanged::No,
                    "Apple's resize didn't finish. macOS was left as it was.",
                    Some("Error: -69606: A problem occurred while resizing APFS Container structures"),
                ),
                Stage::StartupDisk => (DiskChanged::No, "macOS didn't set the startup disk.", Some("bless: Couldn't set the startup disk")),
                _ => (DiskChanged::Unknown, "The engine stopped while writing Sekka.", None),
            };
            push(Event::Failed {
                stage,
                failure: Failure {
                    code: "simulated".parse().unwrap(),
                    message: message.parse().unwrap(),
                    apple_diagnostic: apple.map(|a| a.parse().unwrap()),
                    disk_changed: changed,
                },
            });
        } else if done < stages.len() {
            push(Event::Intent { stage: stages[done] });
        }
        for event in tail {
            push(event);
        }
        let state = InstallState::observe(&out).unwrap_or(InstallState::Planned);
        (out, state)
    }

    /// What the helper records when it looks at the Mac, in scenarios where
    /// it finds the record behind.
    fn reconciliation(&self) -> Option<Vec<Event>> {
        let completed = |stage| Event::Completed {
            stage,
            facts: vec![],
            verification: Verification::PolicyReadBack,
        };
        match self.scenario {
            Scenario::RecoveryFinished => Some(vec![
                Event::Intent { stage: Stage::Enrollment },
                completed(Stage::Enrollment),
                Event::Intent { stage: Stage::StartupSelection },
                completed(Stage::StartupSelection),
            ]),
            Scenario::RemovedByHand => Some(vec![Event::Removed {
                facts: vec![Fact::new("target-extent", "free").unwrap()],
            }]),
            _ => None,
        }
    }

    /// The records of the installation the app reopens to.
    fn reopened(&self) -> (Vec<Record>, InstallState) {
        let (reconciled, cancelled) = {
            let s = self.state.lock().unwrap();
            (s.reconciled, s.cancelled)
        };
        let mut tail = if reconciled { self.reconciliation().unwrap_or_default() } else { Vec::new() };
        if cancelled {
            tail.push(Event::Cancelled);
        }
        match self.scenario {
            // Import done, Apple's files started.
            Scenario::StoppedEarly => self.records(Some((1, None)), tail),
            _ => self.records(None, tail),
        }
    }
}

/// Sekka on the disk, as the engine's inspection reports it.
fn sekka(state: InstallationState, installation_id: Option<Uuid>) -> (Installation, Issue) {
    (
        Installation {
            kind: InstallationKind::Sekka,
            name: "Sekka".parse().unwrap(),
            state,
            stub_group: None,
            esp: None,
            root: None,
            installation_id,
        },
        Issue {
            code: IssueCode::ExistingSekka,
            blocking: true,
            detail: "Sekka is already installed on this Mac (Sekka). A second installation isn't offered yet, and the installer won't change the existing one."
                .parse()
                .unwrap(),
        },
    )
}

/// This Mac as the scenario has it, before or after the download.
fn sim_inspection(scenario: Scenario, downloaded: bool) -> Inspection {
    let mut i = samples::inspection();
    let on_battery = scenario == Scenario::NotReady || (scenario == Scenario::Unplugged && downloaded);
    match scenario {
        Scenario::Unsupported => {
            i.machine.product_type = "Mac15,7".parse().unwrap();
            i.machine.device_class = "j516sap".parse().unwrap();
            i.machine.product_name = "MacBook Pro (16-inch, Nov 2023)".parse().unwrap();
            i.machine.soc_name = "Apple M3 Pro".parse().unwrap();
        }
        _ if on_battery => {
            i.power.on_ac = false;
            i.power.battery_percent = Some(41);
            i.issues.push(Issue {
                code: IssueCode::OnBattery,
                blocking: true,
                detail: "This Mac is running on battery.".parse().unwrap(),
            });
        }
        Scenario::LowSpace => {
            if let Candidate::ShrinkContainer { available, available_recommended, .. } = &mut i.candidates[0] {
                *available = 20 * GIB;
                *available_recommended = 18 * GIB;
            }
            i.issues.push(Issue {
                code: IssueCode::NotEnoughSpace,
                blocking: true,
                detail: "macOS can give 21.5 GB; Sekka needs at least 42.9 GB.".parse().unwrap(),
            });
        }
        Scenario::Existing => {
            i.installations.push(sekka(InstallationState::Unknown, None).0);
        }
        // The installation the helper knows, by its identity. The app's
        // inspection runs as the person using it and can't read the boot
        // policy, so it can't tell whether Recovery's step is done.
        Scenario::WaitingForRecovery | Scenario::RecoveryFinished => {
            let (installation, issue) = sekka(InstallationState::Unknown, Some(SIM_ID));
            i.installations.push(installation);
            i.issues.push(issue);
        }
        _ => {}
    }
    i
}

fn sim_catalog(scenario: Scenario) -> Verified {
    signed_catalog(|catalog| {
        if scenario == Scenario::Outdated {
            catalog.minimum_app_version = "99.0.0".parse().unwrap();
        }
    })
}

/// The sample catalog, current and signed with the development key, after
/// `edit`.
pub fn signed_catalog(edit: impl FnOnce(&mut Catalog)) -> Verified {
    let mut catalog = samples::catalog();
    edit(&mut catalog);
    let now = Timestamp::now();
    catalog.issued_at = now.plus(-3600);
    catalog.expires_at = now.plus(30 * 86_400);
    catalog.notice = Some("Development builds: not yet qualified on hardware. Install only on a Mac you have backed up.".parse().unwrap());
    let key = sekka_mac_trust::signing::key(&sekka_mac_trust::signing::development_seed());
    let signed = sekka_mac_trust::signing::sign(catalog, sekka_mac_trust::DEVELOPMENT_KEY_ID, &key).unwrap();
    let config = TrustConfig {
        roots: vec![sekka_mac_trust::development_root()],
        hosts: sekka_mac_trust::PRODUCTION_HOSTS.to_vec(),
        clock_skew: 600,
    };
    sekka_mac_trust::verify_signed(signed, &config, now).expect("the simulated catalog verifies")
}

impl Environment for Simulation {
    fn user(&self) -> (String, String) {
        ("chris".into(), "Chris Allen".into())
    }
    fn inspect(&self) -> Result<Inspection, String> {
        self.wait(900);
        Ok(sim_inspection(self.scenario, self.state.lock().unwrap().downloaded))
    }
    fn bundled_engine(&self) -> Result<Digest, String> {
        Ok(samples::catalog().releases[0].artifacts.engine.digest)
    }
    fn catalog(&self) -> Result<Verified, String> {
        self.wait(600);
        if self.scenario == Scenario::Offline {
            return Err("Sekka's release list couldn't be downloaded. Check your internet connection, then check again.".into());
        }
        Ok(sim_catalog(self.scenario))
    }
    fn helper_status(&self) -> Result<Vec<Summary>, HelperProblem> {
        Ok(match self.scenario {
            Scenario::WaitingForRecovery
            | Scenario::RecoveryFinished
            | Scenario::RemovedByHand
            | Scenario::StoppedEarly => vec![Summary {
                installation: SIM_ID,
                release: "sekka-j514s-4882dc33".parse().unwrap(),
                state: self.reopened().1,
                opened: Timestamp::now(),
                running: false,
            }],
            _ => vec![],
        })
    }
    fn still_to_download(&self, release: &Release) -> u64 {
        if self.state.lock().unwrap().downloaded { 0 } else { release.download_bytes() }
    }
    fn download(
        &self,
        release: &Release,
        cancel: &AtomicBool,
        on: &mut dyn FnMut(DownloadProgress),
    ) -> Result<Vec<Staged>, String> {
        let total = release.download_bytes();
        let steps = 60;
        for n in 0..=steps {
            if cancel.load(Ordering::SeqCst) {
                return Err("The download was paused. What was downloaded is kept.".into());
            }
            if self.scenario == Scenario::DownloadFails && n == steps / 2 {
                return Err("The download stopped and didn't resume after 6 tries (the network connection was lost). What was downloaded is kept.".into());
            }
            on(DownloadProgress {
                role: ArtifactRole::Payload,
                done: total / steps * n,
                total,
                retrying: None,
                verifying: n == steps,
            });
            self.wait(70);
        }
        self.state.lock().unwrap().downloaded = true;
        let a = &release.artifacts;
        Ok([
            (ArtifactRole::Payload, &a.payload, "payload.zip"),
            (ArtifactRole::InstallerData, &a.installer_data, "installer_data.json"),
            (ArtifactRole::Manifest, &a.manifest, "manifest.json"),
            (ArtifactRole::RecoveryApp, &a.recovery_app, "recovery.zip"),
        ]
        .into_iter()
        .map(|(role, artifact, name)| Staged {
            role,
            file: StagedFile {
                path: format!("/simulated/Staging/{name}").into(),
                size: artifact.size,
                digest: artifact.digest,
            },
        })
        .collect())
    }
    fn registration(&self) -> Registration {
        let state = self.state.lock().unwrap();
        match (self.scenario, state.approved_at) {
            (Scenario::NeedsApproval, None) => Registration::Absent,
            (Scenario::NeedsApproval, Some(t)) if t.elapsed() < Duration::from_secs(3) && self.pace > 0.0 => {
                Registration::NeedsApproval
            }
            _ => Registration::Ready,
        }
    }
    fn register_helper(&self) -> Result<Registration, String> {
        self.wait(300);
        let mut state = self.state.lock().unwrap();
        if self.scenario == Scenario::NeedsApproval && state.approved_at.is_none() {
            state.approved_at = Some(Instant::now());
            return Ok(Registration::NeedsApproval);
        }
        drop(state);
        Ok(self.registration())
    }
    fn open_login_items(&self) {}
    fn begin(&self, begin: Begin, password: Secret) -> Result<Uuid, HelperRefusal> {
        self.wait(1200);
        let mut state = self.state.lock().unwrap();
        state.password_tries += 1;
        if password.is_empty()
            || (self.scenario == Scenario::WrongPassword && state.password_tries == 1)
        {
            return Err(HelperRefusal {
                code: HelperRefusalCode::Authorization,
                message: format!("That password isn't right for {}.", begin.owner).parse().unwrap(),
            });
        }
        state.installation = Some((SIM_ID, Instant::now()));
        state.owner = Some(begin.owner.to_string());
        Ok(SIM_ID)
    }
    fn events(&self, _: Uuid, _after: u64) -> Result<Events, String> {
        let started = self.state.lock().unwrap().installation.map(|(_, t)| t);
        let Some(started) = started else {
            let (records, state) = self.reopened();
            return Ok(Events {
                records,
                state,
                running: false,
                progress: None,
            });
        };
        let elapsed = if self.pace > 0.0 { started.elapsed().as_secs_f64() / self.pace } else { 1e9 };
        let mut stages = vec![Stage::Import];
        stages.extend(macos_stages(true));
        let per = 1.6;
        let done = ((elapsed / per) as usize).min(stages.len());
        let failing = match self.scenario {
            Scenario::ResizeFails => Some(Stage::Shrink),
            Scenario::InstallFails => Some(Stage::Root),
            Scenario::StartupDiskFails if !self.state.lock().unwrap().startup_retried => Some(Stage::StartupDisk),
            _ => None,
        };
        if let Some(f) = failing {
            let at = stages.iter().position(|s| *s == f).unwrap();
            if done >= at {
                let (records, state) = self.records(Some((at, Some(f))), Vec::new());
                return Ok(Events {
                    records,
                    state,
                    running: false,
                    progress: None,
                });
            }
        }
        let (records, state) = self.records(Some((done, None)), Vec::new());
        let running = done < stages.len();
        Ok(Events {
            records,
            state: if running { state } else { InstallState::WaitingForRecovery },
            running,
            progress: stages.get(done).map(|s| Progress {
                stage: *s,
                done: ((elapsed % per) / per * 100.0) as u64,
                total: 100,
            }),
        })
    }
    fn cancel(&self, _: Uuid) -> Result<(), HelperRefusal> {
        if self.scenario == Scenario::StoppedEarly {
            self.state.lock().unwrap().cancelled = true;
            return Ok(());
        }
        Err(HelperRefusal {
            code: HelperRefusalCode::State,
            message: "Sekka is changing the disk right now, and stopping in the middle isn't safe. This step will finish or fail on its own."
                .parse()
                .unwrap(),
        })
    }
    fn retry_startup_disk(&self, id: Uuid, _: Secret) -> Result<Events, HelperRefusal> {
        self.wait(800);
        self.state.lock().unwrap().startup_retried = true;
        let (records, _) = self.records(None, Vec::new());
        let _ = id;
        Ok(Events {
            records,
            state: InstallState::WaitingForRecovery,
            running: false,
            progress: None,
        })
    }
    fn reconcile(&self, _: Uuid) -> Result<Events, HelperRefusal> {
        self.wait(700);
        if !self.reconciles {
            // As the helper on this branch answers.
            return Err(HelperRefusal {
                code: HelperRefusalCode::State,
                message: "This helper can't look at the Mac for an installation yet.".parse().unwrap(),
            });
        }
        // Where the record is behind the Mac the helper brings it up to
        // date; elsewhere it finds nothing to change.
        if self.reconciliation().is_some() {
            self.state.lock().unwrap().reconciled = true;
        }
        let (records, state) = self.reopened();
        Ok(Events {
            records,
            state,
            running: false,
            progress: None,
        })
    }
    fn diagnostics(&self, _: Uuid) -> Vec<String> {
        vec!["Simulated diagnostics: nothing real happened.".into()]
    }
    fn remove_staging(&self) {}
    fn copy_to_pasteboard(&self, _: &str) {}
    fn shut_down(&self) -> Result<(), String> {
        Err("The simulation doesn't shut down your Mac.".into())
    }
}

/// Run effects against a simulation inline, feeding results back into the
/// model until nothing is left to do: the model's tests and the screenshots
/// see exactly the states a person would, without threads or timers.
/// Returns whatever was copied to the pasteboard.
pub fn settle(env: &Simulation, model: &mut Model, effects: Vec<Effect>) -> Vec<String> {
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    let mut copied = Vec::new();
    let mut steps = 0;
    while let Some(effect) = queue.pop_front() {
        steps += 1;
        assert!(steps < 10_000, "the model never settles");
        let msgs = match effect {
            Effect::Inspect => vec![Msg::Inspected(env.inspect())],
            Effect::CheckEngine => vec![Msg::Engine(env.bundled_engine())],
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
            Effect::Reconcile(id) => vec![Msg::Reconciled(env.reconcile(id))],
            Effect::Diagnostics(id) => vec![Msg::Diagnostics(env.diagnostics(id))],
            Effect::CopyToPasteboard(text) => {
                copied.push(text);
                vec![]
            }
            Effect::StopDownload | Effect::RemoveStaging | Effect::OpenLoginItems | Effect::ShutDown => vec![],
        };
        for msg in msgs {
            queue.extend(model.update(msg));
        }
    }
    copied
}
