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
    /// Setting the startup disk fails, and the retry is refused before
    /// anything is tried: the installation's ESP changed since.
    StartupDiskRefused,
    /// Setting the startup disk fails, and fails again the first time it
    /// is retried.
    StartupDiskFailsAgain,
    /// The app reopens with macOS's part done.
    WaitingForRecovery,
    /// The app reopens after Recovery finished; the helper's record waits
    /// for Recovery until the helper looks at the Mac and records its
    /// steps.
    RecoveryFinished,
    /// The app reopens after the owner removed an unfinished installation
    /// in Disk Utility; the helper looks, finds nothing of it, and closes
    /// its record.
    RemovedByHand,
    /// The app reopens to an installation whose helper stopped while
    /// downloading Apple's files, before anything on the disk changed.
    StoppedEarly,
    /// Sekka is already installed.
    Existing,
    /// The helper must be allowed in Login Items first.
    NeedsApproval,
    /// The helper can't read the record of an earlier installation (one
    /// cut off as it was being opened), and won't list any until it is
    /// looked at.
    RecordUnreadable,
    /// The app reopens with macOS's part done, and the helper turned off in
    /// Login Items since.
    HelperOff,
}

impl Scenario {
    pub const ALL: [Scenario; 22] = [
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
        Scenario::StartupDiskRefused,
        Scenario::StartupDiskFailsAgain,
        Scenario::WaitingForRecovery,
        Scenario::RecoveryFinished,
        Scenario::RemovedByHand,
        Scenario::StoppedEarly,
        Scenario::Existing,
        Scenario::NeedsApproval,
        Scenario::RecordUnreadable,
        Scenario::HelperOff,
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
            Scenario::StartupDiskRefused => "startup-disk-refused",
            Scenario::StartupDiskFailsAgain => "startup-disk-fails-again",
            Scenario::WaitingForRecovery => "waiting-for-recovery",
            Scenario::RecoveryFinished => "recovery-finished",
            Scenario::RemovedByHand => "removed-by-hand",
            Scenario::StoppedEarly => "stopped-early",
            Scenario::Existing => "existing",
            Scenario::NeedsApproval => "needs-approval",
            Scenario::RecordUnreadable => "record-unreadable",
            Scenario::HelperOff => "helper-off",
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
    /// What the helper answers when asked to reconcile, when it refuses
    /// ([`Simulation::refusing_reconcile`]). Otherwise it looks at the Mac,
    /// as the real helper does.
    reconcile_refusal: Option<HelperRefusal>,
    /// A stage the run stays in until it is cancelled
    /// ([`Simulation::holding_at`]).
    hold: Option<Stage>,
    state: Mutex<SimState>,
}

#[derive(Default)]
struct SimState {
    password_tries: u32,
    /// What macOS says of the helper.
    registration: Option<Registration>,
    /// When the owner allowed the helper in Login Items.
    approved_at: Option<Instant>,
    installation: Option<(Uuid, Instant)>,
    /// What retrying the startup disk added after its failure.
    retried: Vec<Event>,
    /// The release's files are "staged".
    downloaded: bool,
    /// The helper looked at the Mac and brought its record up to date.
    reconciled: bool,
    /// The installation the app reopened to was cancelled.
    cancelled: bool,
    /// The run begun here was cancelled in this stage, which changes
    /// nothing.
    cancelled_at: Option<Stage>,
    /// The administrator who authorized the installation.
    owner: Option<String>,
}

const SIM_ID: Uuid = Uuid::from_bytes([0x5e; 16]);

/// The stages a simulated installation runs; its plan shrinks macOS.
fn sim_stages() -> Vec<Stage> {
    let mut stages = vec![Stage::Import];
    stages.extend(macos_stages(true));
    stages
}

/// A stage's failure, as the helper records it.
fn failed(stage: Stage, code: &str, message: &str, apple: Option<&str>, disk_changed: DiskChanged) -> Event {
    Event::Failed {
        stage,
        failure: Failure {
            code: code.parse().unwrap(),
            message: message.parse().unwrap(),
            apple_diagnostic: apple.map(|a| a.parse().unwrap()),
            disk_changed,
        },
    }
}

/// How the helper describes a state in its refusals.
fn describe(state: &InstallState) -> String {
    match state {
        InstallState::Planned => "it was planned and nothing has changed".into(),
        InstallState::AssetsVerified => "its downloads were checked and nothing on the disk has changed".into(),
        InstallState::MutationStarted { stage } => {
            format!("it stopped during or after “{}”, after the disk changed", stage.title())
        }
        InstallState::Failed { stage, disk_changed } => format!(
            "“{}” failed{}",
            stage.title(),
            match disk_changed {
                DiskChanged::No => ", and nothing on the disk changed",
                DiskChanged::Yes => ", after the disk changed",
                DiskChanged::Unknown => "; whether the disk changed isn't known",
            }
        ),
        InstallState::StartupDiskFailed => "setting the startup disk failed; everything else is in place".into(),
        InstallState::WaitingForRecovery => "it is waiting for its step in Recovery".into(),
        InstallState::EnrollmentUncertain => "its step in Recovery started and didn't finish".into(),
        InstallState::WaitingForSetup => "it is installed and waiting for its first start".into(),
        InstallState::Cancelled => "it was cancelled before anything changed".into(),
        InstallState::Removed => "nothing of it is left on the disk".into(),
    }
}

/// What the helper says when another request holds its install lock.
pub fn busy() -> HelperRefusal {
    HelperRefusal {
        code: HelperRefusalCode::Busy,
        message: "Sekka Installer's helper is busy with another request, or an earlier step is still finishing. Try again in a moment."
            .parse()
            .unwrap(),
    }
}

/// What the helper says when its own inspection of the Mac fails.
pub fn inspection_failed() -> HelperRefusal {
    HelperRefusal {
        code: HelperRefusalCode::State,
        message: "This Mac couldn't be inspected (the engine stopped). Nothing was changed.".parse().unwrap(),
    }
}

impl Simulation {
    pub fn new(scenario: Scenario, pace: f64) -> Self {
        Self {
            scenario,
            pace,
            reconcile_refusal: None,
            hold: None,
            state: Mutex::new(SimState {
                registration: Some(match scenario {
                    Scenario::NeedsApproval => Registration::Absent,
                    Scenario::HelperOff => Registration::NeedsApproval,
                    _ => Registration::Ready,
                }),
                ..SimState::default()
            }),
        }
    }

    /// The same simulation, with the helper as macOS has it at the start:
    /// not set up, waiting for the owner, or allowed.
    pub fn registered(self, registration: Registration) -> Self {
        self.state.lock().unwrap().registration = Some(registration);
        self
    }

    /// The same simulation, with a run that stays in `stage` until it is
    /// cancelled: at pace 0 a run otherwise finishes as it starts.
    pub fn holding_at(mut self, stage: Stage) -> Self {
        self.hold = Some(stage);
        self
    }

    /// The same simulation, with a helper that refuses to reconcile, as the
    /// real one does when it is busy ([`busy`]) or can't inspect the Mac
    /// ([`inspection_failed`]).
    pub fn refusing_reconcile(mut self, refusal: HelperRefusal) -> Self {
        self.reconcile_refusal = Some(refusal);
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
        let stages = sim_stages();
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
            push(failed(stage, "simulated", message, apple, changed));
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
        // Import done and Apple's files started, when the helper stopped.
        let upto = (self.scenario == Scenario::StoppedEarly).then_some((1, None));
        let mut tail = if reconciled { self.reconciliation().unwrap_or_default() } else { Vec::new() };
        if cancelled {
            // As the helper closes it: the stage its run stopped in changed
            // nothing, and the record says so first.
            if let Some((done, None)) = upto {
                tail.push(failed(
                    sim_stages()[done],
                    "interrupted",
                    "This step stopped part way, when the helper or the Mac restarted. Nothing on the disk changed.",
                    None,
                    DiskChanged::No,
                ));
            }
            tail.push(Event::Cancelled);
        }
        self.records(upto, tail)
    }

    /// The installation as the helper has it now: the one begun in this
    /// run, as far as the clock has taken it, or the one the app reopens
    /// to.
    fn current(&self) -> Events {
        let started = self.state.lock().unwrap().installation.map(|(_, t)| t);
        let Some(started) = started else {
            let (records, state) = self.reopened();
            return Events {
                records,
                state,
                running: false,
                progress: None,
            };
        };
        let elapsed = if self.pace > 0.0 { started.elapsed().as_secs_f64() / self.pace } else { 1e9 };
        let stages = sim_stages();
        let at = |stage| stages.iter().position(|s| *s == stage).unwrap();
        let cancelled_at = self.state.lock().unwrap().cancelled_at;
        if let Some(stage) = cancelled_at {
            let tail = vec![
                failed(stage, "cancelled", "Cancelled before anything on the disk changed.", None, DiskChanged::No),
                Event::Cancelled,
            ];
            let (records, state) = self.records(Some((at(stage), None)), tail);
            return Events {
                records,
                state,
                running: false,
                progress: None,
            };
        }
        let per = 1.6;
        let mut done = ((elapsed / per) as usize).min(stages.len());
        if let Some(stage) = self.hold {
            done = done.min(at(stage));
        }
        let failing = match self.scenario {
            Scenario::ResizeFails => Some(Stage::Shrink),
            Scenario::InstallFails => Some(Stage::Root),
            Scenario::StartupDiskFails | Scenario::StartupDiskRefused | Scenario::StartupDiskFailsAgain => {
                Some(Stage::StartupDisk)
            }
            _ => None,
        };
        if let Some(f) = failing
            && done >= at(f)
        {
            let retried = self.state.lock().unwrap().retried.clone();
            let (records, state) = self.records(Some((at(f), Some(f))), retried);
            return Events {
                records,
                state,
                running: false,
                progress: None,
            };
        }
        let (records, state) = self.records(Some((done, None)), Vec::new());
        let running = done < stages.len();
        Events {
            records,
            state: if running { state } else { InstallState::WaitingForRecovery },
            running,
            progress: stages.get(done).map(|s| Progress {
                stage: *s,
                done: ((elapsed % per) / per * 100.0) as u64,
                total: 100,
            }),
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
        Scenario::WaitingForRecovery | Scenario::RecoveryFinished | Scenario::HelperOff => {
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
        // As the helper refuses for the whole list.
        if self.scenario == Scenario::RecordUnreadable {
            return Err(HelperProblem::Other(format!(
                "The record of installation {SIM_ID} can't be read (records are missing from the sequence). Nothing more will be installed until it is looked at; choose Copy Diagnostics."
            )));
        }
        let begun = self.state.lock().unwrap().installation.is_some();
        let known = begun
            || matches!(
                self.scenario,
                Scenario::WaitingForRecovery
                    | Scenario::RecoveryFinished
                    | Scenario::RemovedByHand
                    | Scenario::StoppedEarly
                    | Scenario::HelperOff
            );
        if !known {
            return Ok(Vec::new());
        }
        let now = self.current();
        Ok(vec![Summary {
            installation: SIM_ID,
            release: "sekka-j514s-4882dc33".parse().unwrap(),
            state: now.state,
            opened: Timestamp::now(),
            running: now.running,
        }])
    }
    fn still_to_download(&self, release: &Release) -> ToDownload {
        if self.state.lock().unwrap().downloaded { ToDownload::NOTHING } else { ToDownload::all(release) }
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
        let mut state = self.state.lock().unwrap();
        // Allowed in Login Items: macOS takes a moment to say so.
        if state.registration == Some(Registration::NeedsApproval)
            && state
                .approved_at
                .is_some_and(|t| self.pace == 0.0 || t.elapsed() >= Duration::from_secs(3))
        {
            state.registration = Some(Registration::Ready);
        }
        state.registration.unwrap_or(Registration::Ready)
    }
    fn register_helper(&self) -> Result<Registration, String> {
        self.wait(300);
        let mut state = self.state.lock().unwrap();
        if state.registration == Some(Registration::Absent) {
            state.registration = Some(Registration::NeedsApproval);
        }
        drop(state);
        Ok(self.registration())
    }
    /// The owner allows the helper there.
    fn open_login_items(&self) {
        self.state.lock().unwrap().approved_at.get_or_insert_with(Instant::now);
    }
    fn begin(&self, begin: Begin, password: Secret) -> Result<Uuid, HelperRefusal> {
        self.wait(1200);
        // As the helper refuses while an earlier installation stands in
        // the way, before it looks at the password.
        let busy = |message: String| HelperRefusal {
            code: HelperRefusalCode::Busy,
            message: message.parse().unwrap(),
        };
        match self.helper_status() {
            Err(_) => {
                return Err(busy(
                    "An earlier installation's record can't be read (records are missing from the sequence). Nothing more will be installed until it is looked at."
                        .into(),
                ));
            }
            Ok(list) => {
                if let Some(s) = list.iter().find(|s| !s.state.allows_new_installation()) {
                    return Err(busy(format!(
                        "An earlier installation hasn't finished ({}). Finish or look at it before starting another.",
                        describe(&s.state)
                    )));
                }
            }
        }
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
        Ok(self.current())
    }
    /// As the helper answers: a run in a stage that changes nothing stops
    /// at once and is closed as cancelled, and one changing the disk isn't
    /// stopped; a stopped installation is closed only if nothing that
    /// changes the disk ever started.
    fn cancel(&self, _: Uuid) -> Result<(), HelperRefusal> {
        let now = self.current();
        if now.running {
            let stage = now.progress.map_or(Stage::Import, |p| p.stage);
            if stage.mutates() {
                return Err(HelperRefusal {
                    code: HelperRefusalCode::State,
                    message: "Sekka is changing the disk right now, and stopping in the middle isn't safe. This step will finish or fail on its own."
                        .parse()
                        .unwrap(),
                });
            }
            self.state.lock().unwrap().cancelled_at = Some(stage);
            return Ok(());
        }
        match now.state {
            InstallState::Planned | InstallState::AssetsVerified => {
                self.state.lock().unwrap().cancelled = true;
                Ok(())
            }
            InstallState::Cancelled => Ok(()),
            other => Err(HelperRefusal {
                code: HelperRefusalCode::State,
                message: format!("This installation can't be cancelled now: {}.", describe(&other)).parse().unwrap(),
            }),
        }
    }
    /// As the helper answers: only after setting the startup disk failed,
    /// and only that stage again, its records added after the failure; or
    /// refused with the engine's reason and nothing recorded, when the
    /// installation isn't as it was left.
    fn retry_startup_disk(&self, _: Uuid, _: Secret) -> Result<Events, HelperRefusal> {
        self.wait(800);
        let refused = |message: &str| HelperRefusal {
            code: HelperRefusalCode::State,
            message: message.parse().unwrap(),
        };
        if self.current().state != InstallState::StartupDiskFailed {
            return Err(refused("Only setting the startup disk can be tried again, and only after it failed."));
        }
        let mut state = self.state.lock().unwrap();
        let outcome = match (self.scenario, state.retried.is_empty()) {
            (Scenario::StartupDiskRefused, _) => {
                return Err(refused(
                    "This installation isn't as it was left: its ESP's files differ from what was installed and verified. Setting the startup disk again isn't safe.",
                ));
            }
            (Scenario::StartupDiskFailsAgain, true) => failed(
                Stage::StartupDisk,
                "startup-disk-not-selected",
                "macOS accepted the startup disk, but reading it back names another volume.",
                None,
                DiskChanged::No,
            ),
            _ => Event::Completed {
                stage: Stage::StartupDisk,
                facts: vec![],
                verification: Verification::Inspected,
            },
        };
        state.retried.extend([Event::Intent { stage: Stage::StartupDisk }, outcome]);
        drop(state);
        Ok(self.current())
    }
    /// As the helper answers: never while its engine runs; with the record
    /// as it stands when it lets another installation begin; otherwise
    /// after looking at the Mac, bringing the record up to date where it is
    /// behind it, and changing nothing elsewhere.
    fn reconcile(&self, _: Uuid) -> Result<Events, HelperRefusal> {
        self.wait(700);
        let now = self.current();
        if now.running {
            return Err(HelperRefusal {
                code: HelperRefusalCode::Busy,
                message: "The installation is still running.".parse().unwrap(),
            });
        }
        if let Some(refusal) = &self.reconcile_refusal {
            return Err(refusal.clone());
        }
        let recovery = matches!(now.state, InstallState::WaitingForRecovery | InstallState::EnrollmentUncertain);
        if !recovery && now.state.allows_new_installation() {
            return Ok(now);
        }
        if self.reconciliation().is_some() {
            self.state.lock().unwrap().reconciled = true;
        }
        Ok(self.current())
    }
    fn full_name(&self, account: &str) -> Option<String> {
        match account {
            "chris" => Some("Chris Allen".into()),
            "admin" => Some("Sam Admin".into()),
            _ => None,
        }
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
            // Asked only while macOS lets it run, as the runtime asks.
            Effect::HelperStatus => vec![
                Msg::HelperStatus(crate::env::status(env)),
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
            Effect::Reconcile { installation, .. } => vec![Msg::Reconciled(env.reconcile(installation))],
            Effect::FullName(account) => vec![Msg::FullName {
                name: env.full_name(&account),
                account,
            }],
            Effect::Diagnostics(id) => vec![Msg::Diagnostics(env.diagnostics(id))],
            Effect::CopyToPasteboard(text) => {
                copied.push(text);
                vec![]
            }
            Effect::OpenLoginItems => {
                env.open_login_items();
                vec![]
            }
            Effect::StopDownload | Effect::RemoveStaging | Effect::ShutDown => vec![],
        };
        for msg in msgs {
            queue.extend(model.update(msg));
        }
    }
    copied
}
