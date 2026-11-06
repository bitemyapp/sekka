// SPDX-License-Identifier: GPL-3.0-or-later
//! Request handling.
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use sekka_mac_contract::{
    ArtifactRole, Begin, DiskChanged, EngineOutcome, Event, Fact, Failure, HELPER_PROTOCOL,
    HelperRefusal, HelperRefusalCode, HelperRequest, HelperRequestSchema, HelperResponse,
    HelperResponseSchema, InstallState, Progress, Release, Request, Response, Stage, Summary,
    Text, Transcript, Uuid, Verification, Version, canonical, derive_plan, limits,
};
use sekka_mac_journal::{FsStorage, InstallLock, Journal, JournalError};
use sekka_mac_stage::{Expect, StagingDir};
use sekka_mac_trust::{AcceptedStore, TrustConfig};

use crate::engine::{DriveError, Observation, drive};
use crate::{Paths, Platform, Secret, redact};

/// How many engine log lines are kept for diagnostics.
const LOG_LINES: usize = 2000;

pub struct Helper<P: Platform> {
    platform: Arc<P>,
    paths: Paths,
    trust: TrustConfig,
    version: Version,
    active: Mutex<Option<Arc<Active>>>,
}

/// The installation this helper process is running.
struct Active {
    installation: Uuid,
    owner: String,
    cancel: AtomicBool,
    shared: Mutex<Shared>,
}

#[derive(Default)]
struct Shared {
    running: bool,
    /// A stage that changes the disk is in progress (cancelling isn't safe).
    mutating: bool,
    progress: Option<Progress>,
    log: VecDeque<String>,
    terminate: Option<Box<dyn FnMut() + Send>>,
}

fn refuse(code: HelperRefusalCode, message: impl AsRef<str>) -> Response {
    let text: String = message.as_ref().chars().take(2000).collect();
    Response::Refused(HelperRefusal {
        code,
        message: text.parse().unwrap_or_else(|_| "The request was refused.".parse().unwrap()),
    })
}

impl<P: Platform> Helper<P> {
    pub fn new(platform: P, paths: Paths, trust: TrustConfig, version: Version) -> std::io::Result<Self> {
        for dir in [paths.root().to_path_buf(), paths.journals()] {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
        }
        Ok(Self {
            platform: Arc::new(platform),
            paths,
            trust,
            version,
            active: Mutex::new(None),
        })
    }

    /// Answer one request: bytes in (a canonical request), bytes out (a
    /// canonical response). Never panics on hostile input.
    pub fn handle(&self, uid: u32, request: &[u8], password: Option<Secret>) -> Vec<u8> {
        let (id, response) =
            match canonical::from_slice::<HelperRequest>(request, limits::HELPER_MESSAGE, HelperRequestSchema::VALUE) {
                Ok(r) => (r.id, self.answer(uid, r.request, password)),
                Err(e) => (0, refuse(HelperRefusalCode::Malformed, e.to_string())),
            };
        let reply = HelperResponse {
            schema: HelperResponseSchema,
            id,
            response,
        };
        canonical::to_vec(&reply).unwrap_or_else(|_| {
            canonical::to_vec(&HelperResponse {
                schema: HelperResponseSchema,
                id,
                response: refuse(HelperRefusalCode::Internal, "The reply was too large."),
            })
            .expect("a refusal serializes")
        })
    }

    fn answer(&self, uid: u32, request: Request, password: Option<Secret>) -> Response {
        match request {
            Request::Hello { protocol, .. } if protocol != HELPER_PROTOCOL => refuse(
                HelperRefusalCode::Protocol,
                "This copy of Sekka Installer and its helper don't match. Quit the app and open it again; if that doesn't help, download it again.",
            ),
            Request::Hello { .. } => Response::Hello {
                helper: self.version.clone(),
                protocol: HELPER_PROTOCOL,
            },
            Request::Status => self.status(),
            Request::Begin(begin) => match password {
                Some(password) => self.begin(uid, *begin, password),
                None => refuse(HelperRefusalCode::Authorization, "The owner's password is needed to install."),
            },
            Request::Cancel { installation } => self.cancel(installation),
            Request::Events { installation, after } => self.events(installation, after),
            Request::RetryStartupDisk { installation } => match password {
                Some(password) => self.retry_startup_disk(installation, password),
                None => refuse(HelperRefusalCode::Authorization, "The owner's password is needed."),
            },
            Request::Diagnostics { installation } => self.diagnostics(installation),
        }
    }

    fn active(&self, installation: Uuid) -> Option<Arc<Active>> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .filter(|a| a.installation == installation)
            .cloned()
    }

    fn open(&self, installation: Uuid) -> Result<Journal, JournalError> {
        Journal::open(FsStorage, &self.paths.journals(), installation)
    }

    fn status(&self) -> Response {
        let ids = match sekka_mac_journal::installations(&FsStorage, &self.paths.journals()) {
            Ok(ids) => ids,
            Err(e) => return refuse(HelperRefusalCode::Internal, e.to_string()),
        };
        let mut installations = Vec::new();
        for id in ids {
            let running = self
                .active(id)
                .is_some_and(|a| a.shared.lock().unwrap().running);
            match self.open(id) {
                Ok(j) => {
                    let first = &j.records()[0];
                    let Event::Opened { plan, .. } = &first.event else { continue };
                    installations.push(Summary {
                        installation: id,
                        release: plan.release.release.clone(),
                        state: j.state().clone(),
                        opened: first.at,
                        running,
                    });
                }
                Err(e) => {
                    return refuse(
                        HelperRefusalCode::State,
                        format!("The record of installation {id} can't be read ({e}). Nothing more will be installed until it is looked at; choose Copy Diagnostics."),
                    );
                }
            }
        }
        installations.sort_by_key(|s| s.opened);
        Response::Status { installations }
    }

    /// Every earlier installation must leave the disk in a state that allows
    /// another; a damaged record blocks too.
    fn check_no_unfinished(&self) -> Result<(), Response> {
        let ids = sekka_mac_journal::installations(&FsStorage, &self.paths.journals())
            .map_err(|e| refuse(HelperRefusalCode::Internal, e.to_string()))?;
        for id in ids {
            match self.open(id) {
                Ok(j) if j.state().allows_new_installation() => {}
                Ok(j) => {
                    return Err(refuse(
                        HelperRefusalCode::Busy,
                        format!(
                            "An earlier installation hasn't finished ({}). Finish or look at it before starting another.",
                            describe(j.state())
                        ),
                    ));
                }
                Err(e) => {
                    return Err(refuse(
                        HelperRefusalCode::Busy,
                        format!("An earlier installation's record can't be read ({e}). Nothing more will be installed until it is looked at."),
                    ));
                }
            }
        }
        Ok(())
    }

    fn begin(&self, uid: u32, begin: Begin, password: Secret) -> Response {
        let mut active = self.active.lock().unwrap();
        if active
            .as_ref()
            .is_some_and(|a| a.shared.lock().unwrap().running)
        {
            return refuse(HelperRefusalCode::Busy, "An installation is already running.");
        }
        let lock = match InstallLock::try_acquire(&self.paths.lock()) {
            Ok(Some(lock)) => lock,
            Ok(None) => return refuse(HelperRefusalCode::Busy, "An installation is already running."),
            Err(e) => return refuse(HelperRefusalCode::Internal, e.to_string()),
        };
        if let Err(response) = self.check_no_unfinished() {
            return response;
        }
        let now = self.platform.now();

        // The catalog, verified with the helper's own trust and store.
        let verified = match sekka_mac_trust::verify_signed(begin.catalog.clone(), &self.trust, now) {
            Ok(v) => v,
            Err(e) => return refuse(HelperRefusalCode::Catalog, e.to_string()),
        };
        let mut store = match AcceptedStore::open(&self.paths.accepted()) {
            Ok(s) => s,
            Err(e) => return refuse(HelperRefusalCode::Catalog, e.to_string()),
        };
        if let Err(e) = store.check(&verified) {
            return refuse(HelperRefusalCode::Catalog, e.to_string());
        }
        let catalog = verified.catalog();
        if catalog.protocol != HELPER_PROTOCOL {
            return refuse(
                HelperRefusalCode::Protocol,
                "This release needs a newer Sekka Installer. Download the latest one.",
            );
        }
        if begin.app < catalog.minimum_app_version {
            return refuse(
                HelperRefusalCode::Protocol,
                "This release needs a newer Sekka Installer. Download the latest one.",
            );
        }
        let Some(release) = catalog.releases.iter().find(|r| r.id == begin.request.release).cloned() else {
            return refuse(HelperRefusalCode::Catalog, "The catalog doesn't offer that release.");
        };

        // The owner, before anything slow.
        let owner = begin.owner.to_string();
        if let Err(e) = self.platform.authenticate_owner(&owner, &password) {
            return refuse(HelperRefusalCode::Authorization, e.to_string());
        }

        // The engine this release needs, and the Mac as root sees it.
        let engine = match self.platform.engine(&release.artifacts.engine.digest) {
            Ok(e) => e,
            Err(e) => return refuse(HelperRefusalCode::Download, e.to_string()),
        };
        let inspection = match self.platform.inspect(&engine) {
            Ok(i) => i,
            Err(e) => return refuse(HelperRefusalCode::PlanChanged, e.to_string()),
        };
        let plan = match derive_plan(&inspection, catalog, &begin.request, 0) {
            Ok(p) => p,
            Err(e) => {
                return refuse(
                    HelperRefusalCode::PlanChanged,
                    format!("This Mac changed since the plan was reviewed ({e}). Check it again and review the new plan."),
                );
            }
        };
        let digest = match plan.digest() {
            Ok(d) => d,
            Err(e) => return refuse(HelperRefusalCode::Internal, e.to_string()),
        };
        if digest != begin.plan {
            return refuse(
                HelperRefusalCode::PlanChanged,
                "This Mac or the release changed since the plan was reviewed. Check this Mac again and review the new plan.",
            );
        }
        if let Err(e) = begin.confirmation.approves(&plan) {
            return refuse(HelperRefusalCode::PlanChanged, e.to_string());
        }
        let staged = match staged_files(&begin, &release) {
            Ok(s) => s,
            Err(e) => return refuse(HelperRefusalCode::Download, e),
        };
        let user_root = match self.platform.user_staging(uid) {
            Ok(p) => p,
            Err(e) => return refuse(HelperRefusalCode::Download, e.to_string()),
        };
        if let Err(e) = store.accept(&verified, now) {
            return refuse(HelperRefusalCode::Catalog, e.to_string());
        }

        let installation = match Uuid::random() {
            Ok(id) => id,
            Err(e) => return refuse(HelperRefusalCode::Internal, e.to_string()),
        };
        let journal = match Journal::create(
            FsStorage,
            &self.paths.journals(),
            installation,
            now,
            Event::Opened {
                plan: Box::new(plan.clone()),
                plan_digest: digest,
                app: begin.app.clone(),
                helper: self.version.clone(),
            },
        ) {
            Ok(j) => j,
            Err(e) => return refuse(HelperRefusalCode::Internal, e.to_string()),
        };

        let state = Arc::new(Active {
            installation,
            owner: owner.clone(),
            cancel: AtomicBool::new(false),
            shared: Mutex::new(Shared {
                running: true,
                ..Shared::default()
            }),
        });
        *active = Some(state.clone());
        drop(active);

        let run = Run {
            platform: self.platform.clone(),
            paths: self.paths.clone(),
            active: state,
            journal,
            engine,
            shrinks: plan
                .actions
                .iter()
                .any(|a| matches!(a, sekka_mac_contract::Action::ShrinkContainer { .. })),
            plan_digest: digest,
            plan_bytes: canonical::to_vec(&plan).unwrap_or_default(),
            staged,
            user_root,
            uid,
            owner,
            password,
            _lock: lock,
        };
        std::thread::Builder::new()
            .name(format!("installation {installation}"))
            .spawn(move || run.run())
            .expect("the installation thread starts");
        Response::Begun { installation }
    }

    fn events(&self, installation: Uuid, after: u64) -> Response {
        // Whether it is running is read before the journal: a run that ends
        // in between then shows as running one more time, never as stopped
        // with its last records missing.
        let (running, progress) = match self.active(installation) {
            Some(a) => {
                let s = a.shared.lock().unwrap();
                (s.running, s.progress)
            }
            None => (false, None),
        };
        let journal = match self.open(installation) {
            Ok(j) => j,
            Err(e) => return refuse(HelperRefusalCode::State, e.to_string()),
        };
        Response::Events {
            records: journal
                .records()
                .iter()
                .filter(|r| r.sequence > after)
                .take(256)
                .cloned()
                .collect(),
            state: journal.state().clone(),
            running,
            progress,
        }
    }

    fn cancel(&self, installation: Uuid) -> Response {
        if let Some(active) = self.active(installation) {
            let mut shared = active.shared.lock().unwrap();
            if shared.running {
                if shared.mutating {
                    return refuse(
                        HelperRefusalCode::State,
                        "Sekka is changing the disk right now, and stopping in the middle isn't safe. This step will finish or fail on its own.",
                    );
                }
                active.cancel.store(true, Ordering::SeqCst);
                if let Some(terminate) = shared.terminate.as_mut() {
                    terminate();
                }
                return Response::Cancelled;
            }
        }
        // Not running in this process: an installation stopped before the
        // disk changed can be closed.
        let mut journal = match self.open(installation) {
            Ok(j) => j,
            Err(e) => return refuse(HelperRefusalCode::State, e.to_string()),
        };
        match journal.state() {
            InstallState::Planned | InstallState::AssetsVerified => {
                match journal.append(self.platform.now(), Event::Cancelled) {
                    Ok(_) => Response::Cancelled,
                    Err(e) => refuse(HelperRefusalCode::State, e.to_string()),
                }
            }
            InstallState::Cancelled => Response::Cancelled,
            other => refuse(
                HelperRefusalCode::State,
                format!("This installation can't be cancelled now: {}.", describe(other)),
            ),
        }
    }

    fn retry_startup_disk(&self, installation: Uuid, password: Secret) -> Response {
        if self
            .active(installation)
            .is_some_and(|a| a.shared.lock().unwrap().running)
        {
            return refuse(HelperRefusalCode::Busy, "The installation is still running.");
        }
        let _lock = match InstallLock::try_acquire(&self.paths.lock()) {
            Ok(Some(lock)) => lock,
            Ok(None) => return refuse(HelperRefusalCode::Busy, "An installation is already running."),
            Err(e) => return refuse(HelperRefusalCode::Internal, e.to_string()),
        };
        let mut journal = match self.open(installation) {
            Ok(j) => j,
            Err(e) => return refuse(HelperRefusalCode::State, e.to_string()),
        };
        if *journal.state() != InstallState::StartupDiskFailed {
            return refuse(
                HelperRefusalCode::State,
                "Only setting the startup disk can be tried again, and only after it failed.",
            );
        }
        // The facts every completed stage recorded identify the target; the
        // platform checks they still name what is on the disk.
        let facts: Vec<Fact> = journal
            .records()
            .iter()
            .filter_map(|r| match &r.event {
                Event::Completed { facts, .. } => Some(facts.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        let owner = self.active(installation).map(|a| a.owner.clone());
        let Some(owner) = owner.or_else(|| fact(&facts, "owner")) else {
            return refuse(HelperRefusalCode::State, "The installation doesn't record its owner.");
        };
        if let Err(e) = self.platform.authenticate_owner(&owner, &password) {
            return refuse(HelperRefusalCode::Authorization, e.to_string());
        }
        let now = self.platform.now();
        if let Err(e) = journal.append(now, Event::Intent { stage: Stage::StartupDisk }) {
            return refuse(HelperRefusalCode::Internal, e.to_string());
        }
        let event = match self.platform.set_startup_disk(&facts, &owner, &password) {
            Ok(facts) => Event::Completed {
                stage: Stage::StartupDisk,
                facts,
                verification: Verification::PolicyReadBack,
            },
            Err(failure) => Event::Failed {
                stage: Stage::StartupDisk,
                failure,
            },
        };
        if let Err(e) = journal.append(self.platform.now(), event) {
            return refuse(HelperRefusalCode::Internal, e.to_string());
        }
        self.events(installation, 0)
    }

    fn diagnostics(&self, installation: Uuid) -> Response {
        let journal = match self.open(installation) {
            Ok(j) => j,
            Err(e) => {
                return Response::Diagnostics {
                    lines: vec![text(&format!("The record can't be read: {e}"))],
                };
            }
        };
        let owner = self.active(installation).map(|a| a.owner.clone()).unwrap_or_default();
        let names = [owner.as_str()];
        let mut lines = vec![
            text(&format!("Sekka Installer helper {}", self.version)),
            text(&format!("Installation {installation}: {}", describe(journal.state()))),
        ];
        for r in journal.records() {
            let line = match &r.event {
                Event::Opened { plan, .. } => format!(
                    "{} {} opened: release {}, {} bytes for Sekka",
                    r.sequence, r.at, plan.release.release, plan.allocation.sekka
                ),
                Event::Intent { stage } => format!("{} {} starting {stage:?}", r.sequence, r.at),
                Event::Completed {
                    stage,
                    facts,
                    verification,
                } => format!(
                    "{} {} finished {stage:?} ({verification:?}): {}",
                    r.sequence,
                    r.at,
                    facts
                        .iter()
                        .filter(|f| f.name.as_str() != "owner")
                        .map(|f| format!("{}={}", f.name, f.value))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Event::Failed { stage, failure } => format!(
                    "{} {} FAILED {stage:?} [{}] {} {} (disk changed: {:?})",
                    r.sequence,
                    r.at,
                    failure.code,
                    failure.message,
                    failure.apple_diagnostic.as_ref().map(|d| d.to_string()).unwrap_or_default(),
                    failure.disk_changed
                ),
                Event::Cancelled => format!("{} {} cancelled", r.sequence, r.at),
            };
            lines.push(text(&redact(&line, &names)));
        }
        if let Some(active) = self.active(installation) {
            lines.push(text("Engine log:"));
            for line in &active.shared.lock().unwrap().log {
                lines.push(text(&redact(line, &names)));
            }
        }
        Response::Diagnostics { lines }
    }
}

fn text(s: &str) -> Text {
    let clean: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(2000)
        .collect();
    clean.parse().expect("cleaned text")
}

fn fact(facts: &[Fact], name: &str) -> Option<String> {
    facts
        .iter()
        .rev()
        .find(|f| f.name.as_str() == name)
        .map(|f| f.value.to_string())
}

/// A plain sentence for a state, for messages.
pub(crate) fn describe(state: &InstallState) -> String {
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
    }
}

/// Which staged file is which, refusing duplicates, missing roles and
/// sizes that differ from the catalog's before anything is copied.
fn staged_files(begin: &Begin, release: &Release) -> Result<Vec<(&'static str, String, Expect)>, String> {
    let a = &release.artifacts;
    let want = [
        (ArtifactRole::Payload, "payload.zip", &a.payload),
        (ArtifactRole::InstallerData, "installer_data.json", &a.installer_data),
        (ArtifactRole::Manifest, "manifest.json", &a.manifest),
        (ArtifactRole::RecoveryApp, "recovery.zip", &a.recovery_app),
    ];
    if begin.staged.len() != want.len() {
        return Err("The downloads handed over aren't the release's files.".into());
    }
    let mut out = Vec::new();
    for (role, name, artifact) in want {
        let mut matching = begin.staged.iter().filter(|s| s.role == role);
        let (Some(file), None) = (matching.next(), matching.next()) else {
            return Err("The downloads handed over aren't the release's files.".into());
        };
        if file.size != artifact.size {
            return Err(format!("The downloaded {name} is the wrong size."));
        }
        out.push((name, file.path.to_string(), Expect::from(artifact)));
    }
    Ok(out)
}

/// One installation, run on its own thread.
struct Run<P: Platform> {
    platform: Arc<P>,
    paths: Paths,
    active: Arc<Active>,
    journal: Journal,
    engine: crate::EngineInstall,
    shrinks: bool,
    plan_digest: sekka_mac_contract::Digest,
    plan_bytes: Vec<u8>,
    /// The helper's own name for each file (by its role, never by the
    /// app's path), where the app staged it, and what it must be.
    staged: Vec<(&'static str, String, Expect)>,
    user_root: std::path::PathBuf,
    uid: u32,
    owner: String,
    password: Secret,
    _lock: InstallLock,
}

impl<P: Platform> Run<P> {
    fn log(&self, line: String) {
        let mut s = self.active.shared.lock().unwrap();
        if s.log.len() == LOG_LINES {
            s.log.pop_front();
        }
        s.log.push_back(line);
    }

    fn fail(&mut self, stage: Stage, code: &str, message: &str, disk_changed: DiskChanged) {
        self.log(format!("{stage:?} failed: {message}"));
        let failure = Failure {
            code: code.parse().unwrap_or_else(|_| "failed".parse().unwrap()),
            message: text(message),
            apple_diagnostic: None,
            disk_changed,
        };
        let _ = self
            .journal
            .append(self.platform.now(), Event::Failed { stage, failure });
    }

    fn run(mut self) {
        self.install();
        let mut s = self.active.shared.lock().unwrap();
        s.running = false;
        s.mutating = false;
        s.progress = None;
        s.terminate = None;
    }

    fn install(&mut self) {
        let now = self.platform.now();
        if let Err(e) = self.journal.append(now, Event::Intent { stage: Stage::Import }) {
            self.log(format!("The record couldn't be written: {e}"));
            return;
        }
        let dir = self.paths.staging(self.journal.installation());
        let staging = match StagingDir::open(&dir) {
            Ok(s) => s,
            Err(e) => return self.fail(Stage::Import, "staging", &e.to_string(), DiskChanged::No),
        };
        let mut facts = vec![Fact::new("owner", &self.owner).expect("account names are text")];
        for (name, source, expect) in self.staged.clone() {
            if self.active.cancel.load(Ordering::SeqCst) {
                return self.cancelled(Stage::Import);
            }
            match sekka_mac_stage::import(
                Path::new(&source),
                &self.user_root,
                self.uid,
                &staging,
                name,
                &expect,
                &self.active.cancel,
            ) {
                Ok(file) => {
                    facts.push(Fact::new(&name.replace('_', "-"), file.digest).expect("digest is text"));
                }
                Err(sekka_mac_stage::StageError::Cancelled) => return self.cancelled(Stage::Import),
                Err(e) => return self.fail(Stage::Import, "download", &e.to_string(), DiskChanged::No),
            }
        }
        let plan_path = dir.join("plan.json");
        if let Err(e) = fs::write(&plan_path, &self.plan_bytes) {
            return self.fail(Stage::Import, "staging", &e.to_string(), DiskChanged::No);
        }
        if let Err(e) = self.journal.append(
            self.platform.now(),
            Event::Completed {
                stage: Stage::Import,
                facts,
                verification: Verification::SourceHashed,
            },
        ) {
            self.log(format!("The record couldn't be written: {e}"));
            return;
        }

        let password = std::mem::take(&mut self.password);
        let mut process = match self.platform.start_engine(&self.engine, self.journal.installation(), &plan_path, &dir, &self.owner, password) {
            Ok(p) => p,
            Err(e) => {
                let _ = self
                    .journal
                    .append(self.platform.now(), Event::Intent { stage: Stage::AppleInputs });
                return self.fail(Stage::AppleInputs, "engine", &e.to_string(), DiskChanged::No);
            }
        };
        self.active.shared.lock().unwrap().terminate = Some(process.terminator());

        let mut transcript = Transcript::execute(self.plan_digest, self.shrinks);
        let active = self.active.clone();
        let platform = self.platform.clone();
        let result = {
            let gate = active.clone();
            let mut begin = move |stage: Stage| {
                let mut s = gate.shared.lock().unwrap();
                if gate.cancel.load(Ordering::SeqCst) {
                    return false;
                }
                s.mutating = stage.mutates();
                true
            };
            let mut observe = |o: Observation| {
                let mut s = active.shared.lock().unwrap();
                match o {
                    Observation::Stage(stage) => {
                        s.progress = Some(Progress {
                            stage,
                            done: 0,
                            total: 0,
                        });
                    }
                    Observation::Progress(p) => s.progress = Some(p),
                    Observation::Log(level, line) => {
                        if s.log.len() == LOG_LINES {
                            s.log.pop_front();
                        }
                        s.log.push_back(format!("{level:?}: {line}"));
                    }
                }
            };
            drive(
                process.as_mut(),
                &mut transcript,
                &mut self.journal,
                &|| platform.now(),
                &mut begin,
                &mut observe,
            )
        };
        self.active.shared.lock().unwrap().terminate = None;
        let clean_exit = process.wait().unwrap_or(false);
        match result {
            Ok(EngineOutcome::WaitingForRecovery) if clean_exit => {
                self.log("macOS's part is finished; Recovery is next.".into());
            }
            Ok(outcome) => self.log(format!("The engine finished: {outcome:?}")),
            Err(e) => {
                let cancelled = self.active.cancel.load(Ordering::SeqCst);
                self.log(format!("{e}"));
                let code = match e {
                    DriveError::Protocol(_) => "engine-protocol",
                    DriveError::Cancelled => "cancelled",
                    _ => "engine-stopped",
                };
                match transcript.open_stage() {
                    Some(stage) if cancelled && !stage.mutates() => self.cancelled(stage),
                    Some(stage) => {
                        let changed = if stage.mutates() { DiskChanged::Unknown } else { DiskChanged::No };
                        self.fail(stage, code, &e.to_string(), changed);
                    }
                    // Between stages: the next stage never got its
                    // acknowledgement, so it changed nothing. Recording it
                    // as failed keeps the journal's account complete; the
                    // state machine counts earlier stages' changes.
                    None => {
                        let Some(next) = transcript.next_stage() else { return };
                        if self
                            .journal
                            .append(self.platform.now(), Event::Intent { stage: next })
                            .is_err()
                        {
                            return;
                        }
                        if cancelled && !self.disk_changed() {
                            self.cancelled(next);
                        } else {
                            self.fail(next, code, &e.to_string(), DiskChanged::No);
                        }
                    }
                }
            }
        }
    }

    /// Whether a stage that changes the disk has started.
    fn disk_changed(&self) -> bool {
        self.journal
            .records()
            .iter()
            .any(|r| matches!(&r.event, Event::Intent { stage } if stage.mutates()))
    }

    fn cancelled(&mut self, stage: Stage) {
        self.fail(stage, "cancelled", "Cancelled before anything on the disk changed.", DiskChanged::No);
        let _ = self.journal.append(self.platform.now(), Event::Cancelled);
    }
}

#[cfg(test)]
impl<P: Platform> Helper<P> {
    pub(crate) fn platform_for_tests(&self) -> &P {
        &self.platform
    }
    pub(crate) fn paths_for_tests(&self) -> &Paths {
        &self.paths
    }
    pub(crate) fn terminate_for_tests(&self, installation: Uuid) {
        if let Some(active) = self.active(installation)
            && let Some(terminate) = active.shared.lock().unwrap().terminate.as_mut()
        {
            terminate();
        }
    }
}
