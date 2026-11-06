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
    /// Not enough room.
    LowSpace,
    /// The first password is refused.
    WrongPassword,
    /// The download stops part way.
    DownloadFails,
    /// Apple's resize fails with -69606 and nothing changes.
    ResizeFails,
    /// The root write dies part way.
    InstallFails,
    /// Setting the startup disk fails once.
    StartupDiskFails,
    /// The app reopens with macOS's part done.
    WaitingForRecovery,
    /// Sekka is already installed.
    Existing,
    /// The helper must be allowed in Login Items first.
    NeedsApproval,
}

impl Scenario {
    pub const ALL: [Scenario; 13] = [
        Scenario::Normal,
        Scenario::Unsupported,
        Scenario::NotReady,
        Scenario::Offline,
        Scenario::LowSpace,
        Scenario::WrongPassword,
        Scenario::DownloadFails,
        Scenario::ResizeFails,
        Scenario::InstallFails,
        Scenario::StartupDiskFails,
        Scenario::WaitingForRecovery,
        Scenario::Existing,
        Scenario::NeedsApproval,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scenario::Normal => "normal",
            Scenario::Unsupported => "unsupported",
            Scenario::NotReady => "not-ready",
            Scenario::Offline => "offline",
            Scenario::LowSpace => "low-space",
            Scenario::WrongPassword => "wrong-password",
            Scenario::DownloadFails => "download-fails",
            Scenario::ResizeFails => "resize-fails",
            Scenario::InstallFails => "install-fails",
            Scenario::StartupDiskFails => "startup-disk-fails",
            Scenario::WaitingForRecovery => "waiting-for-recovery",
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
    state: Mutex<SimState>,
}

#[derive(Default)]
struct SimState {
    password_tries: u32,
    approved_at: Option<Instant>,
    installation: Option<(Uuid, Instant)>,
    startup_retried: bool,
}

const SIM_ID: Uuid = Uuid::from_bytes([0x5e; 16]);

impl Simulation {
    pub fn new(scenario: Scenario, pace: f64) -> Self {
        Self {
            scenario,
            pace,
            state: Mutex::new(SimState::default()),
        }
    }

    fn wait(&self, ms: u64) {
        if self.pace > 0.0 {
            std::thread::sleep(Duration::from_millis((ms as f64 * self.pace) as u64));
        }
    }

    fn records(&self, upto: Option<(usize, Option<Stage>)>) -> (Vec<Record>, InstallState) {
        let plan = samples::plan();
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
                facts: vec![],
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
        let state = InstallState::observe(&out).unwrap_or(InstallState::Planned);
        (out, state)
    }
}

fn sim_inspection(scenario: Scenario) -> Inspection {
    let mut i = samples::inspection();
    match scenario {
        Scenario::Unsupported => {
            i.machine.product_type = "Mac15,7".parse().unwrap();
            i.machine.device_class = "j516sap".parse().unwrap();
            i.machine.product_name = "MacBook Pro (16-inch, Nov 2023)".parse().unwrap();
            i.machine.soc_name = "Apple M3 Pro".parse().unwrap();
        }
        Scenario::NotReady => {
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
            i.installations.push(Installation {
                kind: InstallationKind::Sekka,
                name: "Sekka".parse().unwrap(),
                state: InstallationState::Complete,
                stub_group: None,
                esp: None,
                root: None,
                installation_id: None,
            });
        }
        _ => {}
    }
    i
}

fn sim_catalog() -> Verified {
    let mut catalog = samples::catalog();
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
        Ok(sim_inspection(self.scenario))
    }
    fn catalog(&self) -> Result<Verified, String> {
        self.wait(600);
        if self.scenario == Scenario::Offline {
            return Err("Sekka's release list couldn't be downloaded. Check your internet connection, then check again.".into());
        }
        Ok(sim_catalog())
    }
    fn helper_status(&self) -> Result<Vec<Summary>, HelperProblem> {
        Ok(match self.scenario {
            Scenario::WaitingForRecovery => vec![Summary {
                installation: SIM_ID,
                release: "sekka-j514s-4882dc33".parse().unwrap(),
                state: InstallState::WaitingForRecovery,
                opened: Timestamp::now(),
                running: false,
            }],
            _ => vec![],
        })
    }
    fn still_to_download(&self, release: &Release) -> u64 {
        release.download_bytes()
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
        Ok(SIM_ID)
    }
    fn events(&self, _: Uuid, _after: u64) -> Result<Events, String> {
        let started = self.state.lock().unwrap().installation.map(|(_, t)| t);
        let Some(started) = started else {
            let (records, state) = self.records(None);
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
                let (records, state) = self.records(Some((at, Some(f))));
                return Ok(Events {
                    records,
                    state,
                    running: false,
                    progress: None,
                });
            }
        }
        let (records, state) = self.records(Some((done, None)));
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
        let (records, _) = self.records(None);
        let _ = id;
        Ok(Events {
            records,
            state: InstallState::WaitingForRecovery,
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
