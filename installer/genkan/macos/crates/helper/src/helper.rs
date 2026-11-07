// SPDX-License-Identifier: GPL-3.0-or-later
//! Request handling.
//!
//! Requests arrive concurrently, one thread per connection. Those that can
//! write an installation's record or start the engine (Begin, Cancel of a
//! stopped installation, RetryStartupDisk, Reconcile) take the install lock
//! first and refuse if it is held; a run holds it until it ends, and so
//! does each engine it starts, until that engine exits, even if this
//! process stops first. Nothing slow (PAM, unpacking the engine, the root
//! inspection) happens while holding `active`, which Status and Events
//! read.
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use sekka_mac_contract::{
    ArtifactRole, Begin, Digest, DiskChanged, EngineOutcome, Event, Fact, Failure, HELPER_PROTOCOL,
    HelperRefusal, HelperRefusalCode, HelperRequest, HelperRequestSchema, HelperResponse,
    HelperResponseSchema, InstallState, Progress, Release, Request, Response, Stage, Summary,
    Text, Transcript, Uuid, Verification, Version, canonical, derive_plan, limits,
};
use sekka_mac_journal::{FsStorage, InstallLock, Journal, JournalError};
use sekka_mac_stage::{Expect, StagingDir};
use sekka_mac_trust::{AcceptedStore, TrustConfig};

use crate::engine::{DriveError, Observation, drive};
use crate::reconcile::{Finding, fact, open_stage, recorded_facts};
use crate::{EngineInstall, Paths, Platform, PlatformError, Secret, redact};

/// How many engine log lines are kept for diagnostics.
const LOG_LINES: usize = 2000;
/// The most a [`Text`] holds, in bytes.
const TEXT_BYTES: usize = 2048;
/// What a run's staging keeps once its downloads are removed.
const PLAN_FILE: &str = "plan.json";

pub struct Helper<P: Platform> {
    platform: Arc<P>,
    paths: Paths,
    trust: TrustConfig,
    version: Version,
    active: Mutex<Option<Arc<Active>>>,
    /// Held while an engine is found or unpacked: two requests unpacking at
    /// once would share the engine cache's working names.
    unpacking: Mutex<()>,
}

/// The installation this helper process last ran or looked at: whether its
/// engine is running, and its log for diagnostics.
struct Active {
    installation: Uuid,
    /// The account that authorized it; empty when this process never ran
    /// it (the journal records it).
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

impl Active {
    fn new(installation: Uuid, owner: &str, running: bool, mutating: bool, log: VecDeque<String>) -> Self {
        Self {
            installation,
            owner: owner.into(),
            cancel: AtomicBool::new(false),
            shared: Mutex::new(Shared {
                running,
                mutating,
                log,
                ..Shared::default()
            }),
        }
    }

    fn log(&self, line: String) {
        let mut s = self.shared.lock().unwrap();
        if s.log.len() == LOG_LINES {
            s.log.pop_front();
        }
        s.log.push_back(line);
    }

    /// What the engine wrote to its own diagnostic log, after it exited:
    /// tracebacks and Apple's tools' output that its transcript leaves out.
    /// Redacted, like every line, when diagnostics are copied.
    fn engine_log(&self, lines: Vec<String>) {
        for line in lines {
            self.log(format!("stderr: {line}"));
        }
    }

    /// What the engine reports, for Events and diagnostics.
    fn observe(&self, o: Observation) {
        match o {
            Observation::Stage(stage) => {
                self.shared.lock().unwrap().progress = Some(Progress {
                    stage,
                    done: 0,
                    total: 0,
                });
            }
            Observation::Progress(p) => self.shared.lock().unwrap().progress = Some(p),
            Observation::Log(level, line) => self.log(format!("{level:?}: {line}")),
            Observation::Refusal(why) => self.log(format!("Refused: {why}")),
        }
    }

    fn stop(&self) {
        let mut s = self.shared.lock().unwrap();
        s.running = false;
        s.mutating = false;
        s.progress = None;
        s.terminate = None;
    }
}

fn refuse(code: HelperRefusalCode, message: impl AsRef<str>) -> Response {
    Response::Refused(HelperRefusal {
        code,
        message: text(message.as_ref()),
    })
}

impl<P: Platform> Helper<P> {
    pub fn new(platform: P, paths: Paths, trust: TrustConfig, version: Version) -> std::io::Result<Self> {
        for dir in [paths.root().to_path_buf(), paths.journals()] {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
        }
        // Nothing runs yet. Downloads imported for earlier runs are of no
        // use now (a run never resumes, and a retry needs only the plan),
        // and their clones hold their space from macOS until removed.
        if let Ok(entries) = fs::read_dir(paths.all_staging()) {
            for entry in entries.flatten() {
                let named = entry.file_name().to_str().is_some_and(|n| n.parse::<Uuid>().is_ok());
                if named && entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let _ = remove_downloads(&entry.path());
                }
            }
        }
        Ok(Self {
            platform: Arc::new(platform),
            paths,
            trust,
            version,
            active: Mutex::new(None),
            unpacking: Mutex::new(()),
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
            Request::Reconcile { installation } => self.reconcile(installation),
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

    fn running(&self, installation: Uuid) -> bool {
        self.active(installation)
            .is_some_and(|a| a.shared.lock().unwrap().running)
    }

    fn any_running(&self) -> bool {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|a| a.shared.lock().unwrap().running)
    }

    /// Take the install lock, or say why not. A run holds it until it ends;
    /// the other requests that write a record hold it only while they do.
    /// An engine holds it until it exits, so one left running when an
    /// earlier helper process stopped holds it too.
    fn claim(&self) -> Result<InstallLock, Response> {
        match InstallLock::try_acquire(&self.paths.lock()) {
            Ok(Some(lock)) => Ok(lock),
            Ok(None) if self.any_running() => Err(refuse(HelperRefusalCode::Busy, "An installation is already running.")),
            Ok(None) => Err(refuse(
                HelperRefusalCode::Busy,
                "Sekka Installer's helper is busy with another request, or an earlier step is still finishing. Try again in a moment.",
            )),
            Err(e) => Err(refuse(HelperRefusalCode::Internal, e.to_string())),
        }
    }

    /// Mark `installation` as running in this process, keeping the log of
    /// its earlier run here, if any, for diagnostics.
    fn start(&self, installation: Uuid, owner: &str, mutating: bool) -> Arc<Active> {
        let mut active = self.active.lock().unwrap();
        let log = active
            .as_ref()
            .filter(|a| a.installation == installation)
            .map(|a| a.shared.lock().unwrap().log.clone())
            .unwrap_or_default();
        let started = Arc::new(Active::new(installation, owner, true, mutating, log));
        *active = Some(started.clone());
        started
    }

    /// Keep a line for an installation's diagnostics: in its log here, or
    /// in a new one if this process holds none (it restarted). Another
    /// installation's log is never replaced for it.
    fn note(&self, installation: Uuid, line: String) {
        let mut active = self.active.lock().unwrap();
        match active.as_ref() {
            Some(a) if a.installation == installation => a.log(line),
            Some(_) => {}
            None => {
                let noted = Active::new(installation, "", false, false, VecDeque::new());
                noted.log(line);
                *active = Some(Arc::new(noted));
            }
        }
    }

    /// The engine whose archive has this digest; see [`Platform::engine`].
    fn engine(&self, digest: &Digest) -> Result<EngineInstall, PlatformError> {
        let _one = self.unpacking.lock().unwrap();
        self.platform.engine(digest)
    }

    fn open(&self, installation: Uuid) -> Result<Journal, JournalError> {
        Journal::open(FsStorage, &self.paths.journals(), installation)
    }

    /// The account that authorized an installation: from its run here, or
    /// from its record once the helper has restarted.
    fn owner(&self, installation: Uuid, journal: &Journal) -> Option<String> {
        self.active(installation)
            .map(|a| a.owner.clone())
            .filter(|o| !o.is_empty())
            .or_else(|| fact(&recorded_facts(journal.records()), "owner"))
    }

    fn status(&self) -> Response {
        let ids = match sekka_mac_journal::installations(&FsStorage, &self.paths.journals()) {
            Ok(ids) => ids,
            Err(e) => return refuse(HelperRefusalCode::Internal, e.to_string()),
        };
        let mut installations = Vec::new();
        for id in ids {
            let running = self.running(id);
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
        // Held from here until the run ends: a second Begin, a retry or a
        // reconcile is refused meanwhile, while Status and Events answer.
        let lock = match self.claim() {
            Ok(lock) => lock,
            Err(response) => return response,
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
        let engine = match self.engine(&release.artifacts.engine.digest) {
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

        let state = self.start(installation, &owner, false);
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
            lock,
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
        let _lock = match self.claim() {
            Ok(lock) => lock,
            Err(response) => return response,
        };
        let mut journal = match self.open(installation) {
            Ok(j) => j,
            Err(e) => return refuse(HelperRefusalCode::State, e.to_string()),
        };
        match journal.state() {
            InstallState::Planned | InstallState::AssetsVerified => {
                // Its run stopped inside a stage that changes nothing (the
                // helper or the Mac restarted during the download): the
                // record says so before it is closed.
                if let Some(stage) = open_stage(journal.records()) {
                    let failure = Failure {
                        code: "interrupted".parse().unwrap(),
                        message: text("This step stopped part way, when the helper or the Mac restarted. Nothing on the disk changed."),
                        apple_diagnostic: None,
                        disk_changed: DiskChanged::No,
                    };
                    if let Err(e) = journal.append(self.platform.now(), Event::Failed { stage, failure }) {
                        return refuse(HelperRefusalCode::State, e.to_string());
                    }
                }
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
        let lock = match self.claim() {
            Ok(lock) => lock,
            Err(response) => return response,
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
        let Event::Opened { plan, plan_digest, .. } = &journal.records()[0].event else {
            return refuse(HelperRefusalCode::State, "The installation's record has no plan.");
        };
        let (plan, digest) = ((**plan).clone(), *plan_digest);
        let Some(owner) = self.owner(installation, &journal) else {
            return refuse(HelperRefusalCode::State, "The installation doesn't record its owner.");
        };
        // What the boot partition held when it was written and read back;
        // the engine checks it still holds exactly that before it vouches
        // for the installation again.
        let Some(esp_inventory) = esp_inventory(journal.records()) else {
            return refuse(
                HelperRefusalCode::State,
                "The installation's record doesn't say what its boot partition holds, so the startup disk can't be set again safely.",
            );
        };
        if let Err(e) = self.platform.authenticate_owner(&owner, &password) {
            return refuse(HelperRefusalCode::Authorization, e.to_string());
        }
        // The engine that installed it, finding its stub by the
        // installation's identity and checking the target is still there.
        let engine = match self.engine(&plan.release.engine) {
            Ok(e) => e,
            Err(e) => return refuse(HelperRefusalCode::Download, e.to_string()),
        };
        let dir = self.paths.staging(installation);
        let plan_path = dir.join(PLAN_FILE);
        let written = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .and_then(|_| fs::write(&plan_path, canonical::to_vec(&plan).unwrap_or_default()));
        if let Err(e) = written {
            return refuse(HelperRefusalCode::Internal, e.to_string());
        }
        // Running from here: Events says so, the engine's lines are kept for
        // diagnostics, and it can't be cancelled (it is one step, and it
        // changes how the Mac starts up).
        let active = self.start(installation, &owner, true);
        let before = journal.records().len();
        let mut process = match self.platform.start_engine(
            &engine,
            crate::EngineRun::StartupDisk {
                esp_inventory: &esp_inventory,
            },
            installation,
            &plan_path,
            &owner,
            password,
            &lock,
        ) {
            Ok(p) => p,
            Err(e) => {
                active.stop();
                return refuse(HelperRefusalCode::Internal, e.to_string());
            }
        };
        let mut transcript = Transcript::startup_disk(digest);
        let mut refusal = None;
        let result = drive(
            process.as_mut(),
            &mut transcript,
            &mut journal,
            &|| self.platform.now(),
            &mut |_| true,
            &mut |o| {
                if let Observation::Refusal(why) = &o {
                    refusal = Some(why.clone());
                }
                active.observe(o);
            },
            None,
        );
        let _ = process.wait();
        active.engine_log(process.log());
        if let Err(e) = &result {
            active.log(e.to_string());
            if open_stage(journal.records()) == Some(Stage::StartupDisk) {
                let failed = journal.append(
                    self.platform.now(),
                    Event::Failed {
                        stage: Stage::StartupDisk,
                        failure: Failure {
                            code: "engine-stopped".parse().unwrap(),
                            message: text(&e.to_string()),
                            apple_diagnostic: None,
                            disk_changed: DiskChanged::No,
                        },
                    },
                );
                if let Err(e) = failed {
                    active.log(format!("The record couldn't be written: {e}"));
                }
            }
        }
        active.stop();
        if journal.records().len() == before {
            // Refused before trying, so nothing was recorded: say why here.
            let why = refusal
                .or_else(|| result.err().map(|e| e.to_string()))
                .unwrap_or_else(|| "The startup disk wasn't set again, and the engine didn't say why.".into());
            return refuse(HelperRefusalCode::State, why);
        }
        self.events(installation, 0)
    }

    /// Bring a stopped installation's record up to date with the Mac, as
    /// the helper inspects it with the installation's own engine: Recovery's
    /// steps once the boot policy shows they took effect, or that nothing of
    /// it is left. Anything else, or anything uncertain, changes nothing.
    fn reconcile(&self, installation: Uuid) -> Response {
        if self.running(installation) {
            return refuse(HelperRefusalCode::Busy, "The installation is still running.");
        }
        // Held from before the record is read until what was seen is added
        // to it: nothing else writes the record meanwhile, and no engine is
        // changing the disk while it is inspected (one left running when an
        // earlier helper process stopped still holds the lock). Status and
        // Events answer meanwhile.
        let _lock = match self.claim() {
            Ok(lock) => lock,
            Err(response) => return response,
        };
        let mut journal = match self.open(installation) {
            Ok(j) => j,
            Err(e) => return refuse(HelperRefusalCode::State, e.to_string()),
        };
        let state = journal.state().clone();
        let recovery = matches!(state, InstallState::WaitingForRecovery | InstallState::EnrollmentUncertain);
        if !recovery && state.allows_new_installation() {
            return self.events(installation, 0);
        }
        let Event::Opened { plan, .. } = &journal.records()[0].event else {
            return refuse(HelperRefusalCode::State, "The installation's record has no plan.");
        };
        let plan = (**plan).clone();
        // Its own engine, even if this copy of the app carries a newer one.
        let engine = match self.engine(&plan.release.engine) {
            Ok(e) => e,
            Err(e) => return refuse(HelperRefusalCode::Download, e.to_string()),
        };
        let inspection = match self.platform.inspect(&engine) {
            Ok(i) => i,
            Err(e) => {
                if let PlatformError::Inspection { log, .. } = &e {
                    for line in log {
                        self.note(installation, format!("stderr: {line}"));
                    }
                }
                return refuse(
                    HelperRefusalCode::State,
                    format!("This Mac couldn't be inspected ({e}). Nothing was changed."),
                );
            }
        };
        let events = match crate::reconcile::look(journal.records(), &state, &plan, &inspection) {
            Finding::Unchanged(why) => {
                self.note(installation, format!("Looked at this Mac: nothing recorded, because {why}."));
                return self.events(installation, 0);
            }
            Finding::RecoveryFinished(events) => events,
            Finding::Removed(facts) => vec![Event::Removed { facts }],
        };
        for event in events {
            if let Err(e) = journal.append(self.platform.now(), event) {
                return refuse(
                    HelperRefusalCode::State,
                    format!("The installation's record couldn't be brought up to date ({e})."),
                );
            }
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
        let owner = self.owner(installation, &journal).unwrap_or_default();
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
                Event::Removed { facts } => format!(
                    "{} {} found removed: {}",
                    r.sequence,
                    r.at,
                    facts
                        .iter()
                        .map(|f| format!("{}={}", f.name, f.value))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
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

/// `s` as one line of [`Text`]: control characters become spaces, and it is
/// cut on a character boundary to what a `Text` holds.
/// The `esp-inventory` the Esp stage recorded when it completed.
fn esp_inventory(records: &[sekka_mac_contract::Record]) -> Option<Digest> {
    records.iter().rev().find_map(|r| match &r.event {
        Event::Completed {
            stage: Stage::Esp,
            facts,
            ..
        } => fact(facts, "esp-inventory")?.parse().ok(),
        _ => None,
    })
}

fn text(s: &str) -> Text {
    let mut clean = String::with_capacity(s.len().min(TEXT_BYTES));
    for c in s.chars().map(|c| if c.is_control() { ' ' } else { c }) {
        if clean.len() + c.len_utf8() > TEXT_BYTES {
            break;
        }
        clean.push(c);
    }
    clean.parse().unwrap_or_else(|_| "(unreadable)".parse().expect("plain text"))
}

/// Remove an installation's imported downloads from the helper's staging,
/// keeping its plan (a retry of the startup disk rewrites it anyway). The
/// app removes its own copies; on APFS these are clones, which keep the
/// downloads' space from macOS for as long as they exist.
fn remove_downloads(dir: &Path) -> std::io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut first = Ok(());
    for entry in entries {
        let removed = entry.and_then(|entry| {
            if entry.file_name() == PLAN_FILE || entry.file_type()?.is_dir() {
                return Ok(());
            }
            fs::remove_file(entry.path())
        });
        if first.is_ok() {
            first = removed;
        }
    }
    first
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
        InstallState::Removed => "nothing of it is left on the disk".into(),
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
    /// Held until the run ends; the engine it starts holds it too.
    lock: InstallLock,
}

impl<P: Platform> Run<P> {
    fn log(&self, line: String) {
        self.active.log(line);
    }

    fn append(&mut self, event: Event) -> bool {
        match self.journal.append(self.platform.now(), event) {
            Ok(_) => true,
            Err(e) => {
                self.log(format!("The record couldn't be written: {e}"));
                false
            }
        }
    }

    fn fail(&mut self, stage: Stage, code: &str, message: &str, disk_changed: DiskChanged) {
        self.log(format!("{stage:?} failed: {message}"));
        let failure = Failure {
            code: code.parse().unwrap_or_else(|_| "failed".parse().unwrap()),
            message: text(message),
            apple_diagnostic: None,
            disk_changed,
        };
        self.append(Event::Failed { stage, failure });
    }

    fn run(mut self) {
        self.install();
        // Whatever the outcome, nothing reads the downloads again.
        if let Err(e) = remove_downloads(&self.paths.staging(self.journal.installation())) {
            self.log(format!("The imported downloads couldn't all be removed: {e}"));
        }
        self.active.stop();
    }

    fn install(&mut self) {
        if !self.append(Event::Intent { stage: Stage::Import }) {
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
                return self.cancelled(Some(Stage::Import));
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
                Err(sekka_mac_stage::StageError::Cancelled) => return self.cancelled(Some(Stage::Import)),
                Err(e) => return self.fail(Stage::Import, "download", &e.to_string(), DiskChanged::No),
            }
        }
        let plan_path = dir.join(PLAN_FILE);
        if let Err(e) = fs::write(&plan_path, &self.plan_bytes) {
            return self.fail(Stage::Import, "staging", &e.to_string(), DiskChanged::No);
        }
        if !self.append(Event::Completed {
            stage: Stage::Import,
            facts,
            verification: Verification::SourceHashed,
        }) {
            return;
        }

        let password = std::mem::take(&mut self.password);
        let mut process = match self.platform.start_engine(
            &self.engine,
            crate::EngineRun::Execute { assets: &dir },
            self.journal.installation(),
            &plan_path,
            &self.owner,
            password,
            &self.lock,
        ) {
            Ok(p) => p,
            Err(e) => {
                if !self.append(Event::Intent { stage: Stage::AppleInputs }) {
                    return;
                }
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
            let mut observe = |o: Observation| active.observe(o);
            drive(
                process.as_mut(),
                &mut transcript,
                &mut self.journal,
                &|| platform.now(),
                &mut begin,
                &mut observe,
                Some(Stage::AppleInputs),
            )
        };
        self.active.shared.lock().unwrap().terminate = None;
        let clean_exit = process.wait().unwrap_or(false);
        self.active.engine_log(process.log());
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
                // What the journal says started, not the transcript: the
                // engine may have announced a stage the helper never
                // acknowledged (it was cancelled, or its intent couldn't be
                // written), and that stage changed nothing.
                let changed = self.disk_changed();
                match open_stage(self.journal.records()) {
                    Some(stage) if cancelled && !stage.mutates() => self.cancelled(Some(stage)),
                    Some(stage) => {
                        let changed = if stage.mutates() { DiskChanged::Unknown } else { DiskChanged::No };
                        self.fail(stage, code, &e.to_string(), changed);
                    }
                    // Cancelled between stages before anything that changes
                    // the disk began: it is closed as cancelled, with no
                    // record of the stage the engine was about to start.
                    None if cancelled && !changed => self.cancelled(None),
                    // Between stages: the next stage never got its
                    // acknowledgement, so it changed nothing. Recording it
                    // as failed keeps the journal's account complete; the
                    // state machine counts earlier stages' changes.
                    None => {
                        let Some(next) = transcript.next_stage() else { return };
                        if self.append(Event::Intent { stage: next }) {
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

    /// Close the installation as cancelled, recording first that the stage
    /// it was in (one that changes nothing) stopped.
    fn cancelled(&mut self, stage: Option<Stage>) {
        if let Some(stage) = stage {
            self.fail(stage, "cancelled", "Cancelled before anything on the disk changed.", DiskChanged::No);
        }
        self.append(Event::Cancelled);
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
