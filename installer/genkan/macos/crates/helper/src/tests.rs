// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper end to end, with macOS replaced by a fake platform and the
//! engine by a scripted thread that checks, at every acknowledgement, that
//! the journal on disk already holds the intent it acknowledges.
use super::*;
use crate::engine::EngineProcess;
use sekka_mac_contract::*;
use sekka_mac_journal::{FsStorage, InstallLock, Journal};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PASSWORD: &[u8] = b"correct horse battery";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Script {
    Happy,
    /// Fail this stage, reporting whether the disk changed.
    Fail(Stage, DiskChanged),
    /// Stop writing in the middle of this stage, as a crash would.
    Die(Stage),
    /// Skip a stage.
    Skip(Stage),
    /// Wait inside this stage until terminated.
    Hang(Stage),
    /// Before this stage, wait until terminated, then announce it anyway:
    /// a cancel that arrives while the stage's intent is on its way.
    AnnounceAfterStop(Stage),
    /// Before this stage, wait until terminated, then end without
    /// announcing it: the signal lands between stages.
    EndBefore(Stage),
    /// Refuse before any stage began, saying this.
    Refuse(&'static str),
}

/// The parts the scripted engine creates, as its checkpoints record them.
struct Parts {
    stub_partition: Uuid,
    stub_container: Uuid,
    stub_group: Uuid,
    esp: Uuid,
    root: Uuid,
}

fn parts() -> Parts {
    let u = |s: &str| s.parse().unwrap();
    Parts {
        stub_partition: u("5a0c2d4e-1111-4a2b-8c3d-000000000001"),
        stub_container: u("5a0c2d4e-1111-4a2b-8c3d-000000000002"),
        stub_group: u("5a0c2d4e-1111-4a2b-8c3d-000000000003"),
        esp: u("5a0c2d4e-1111-4a2b-8c3d-000000000004"),
        root: u("5a0c2d4e-1111-4a2b-8c3d-000000000005"),
    }
}

/// Where the free space starts on [`free_space`]'s Mac: where its macOS
/// container ends, 24576 bytes past a MiB boundary as on the reference
/// J514S, so a plan for it starts at the next MiB.
const FREE_START: u64 = 524312576 + 300 * GIB;

/// Where the engine creates the stub, ESP and root for `plan`: one after
/// another from the start of the space, which behind a shrunk container is
/// the plan's offset, and in free space is where the free space starts,
/// short of the plan's offset.
fn layout(plan: &Plan) -> [Extent; 3] {
    let shrinks = plan.actions.iter().any(|a| matches!(a, Action::ShrinkContainer { .. }));
    let stub = if shrinks { plan.target.offset } else { FREE_START };
    let a = plan.allocation;
    [
        Extent { offset: stub, size: a.stub },
        Extent { offset: stub + a.stub, size: a.esp },
        Extent { offset: stub + a.stub + a.esp, size: a.root },
    ]
}

/// What the engine records for each stage, as the real one does for the
/// parts it creates.
fn checkpoint_facts(stage: Stage, plan: &Plan) -> Vec<Fact> {
    let (p, [stub, esp, root]) = (parts(), layout(plan));
    let f = |name, value: &dyn std::fmt::Display| Fact::new(name, value).unwrap();
    match stage {
        Stage::StubPartition => vec![
            f("stub-partition", &p.stub_partition),
            f("stub-container", &p.stub_container),
            f("stub-offset", &stub.offset),
            f("stub-size", &stub.size),
        ],
        Stage::StubSystem => vec![f("stub-volume-group", &p.stub_group)],
        Stage::Partitions => vec![
            f("esp-partition", &p.esp),
            f("esp-offset", &esp.offset),
            f("esp-size", &esp.size),
            f("root-partition", &p.root),
            f("root-offset", &root.offset),
            f("root-size", &root.size),
        ],
        Stage::Esp => vec![f("esp-inventory", &esp_inventory())],
        _ => vec![Fact::new("checked", stage.title()).unwrap()],
    }
}

/// The inventory the fake engine's Esp stage records.
fn esp_inventory() -> Digest {
    Digest::of(b"the new ESP's files")
}

struct Fake {
    dir: PathBuf,
    inspection: Mutex<Inspection>,
    /// Run once, during the next inspection, as something else might act
    /// while the Mac is being inspected.
    during_inspection: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// How the next inspection fails, if it does.
    inspection_fails: Mutex<Option<PlatformError>>,
    /// The engine this copy of the app carries, and those already unpacked
    /// into the helper's cache, which `sekka_mac_engine::unpack` reuses by
    /// digest whatever the app carries.
    bundled: Mutex<Digest>,
    unpacked: Mutex<Vec<Digest>>,
    inspected_with: Mutex<Vec<Digest>>,
    /// What the engine found in the helper's staging when it started.
    staged_at_start: Mutex<Vec<(String, Vec<u8>)>>,
    script: Mutex<Script>,
    startup_disk_ok: AtomicBool,
    /// Every acknowledgement found its intent already in the journal.
    ordering_held: Arc<AtomicBool>,
    engine_runs: Arc<Mutex<u32>>,
    startup_runs: Arc<Mutex<u32>>,
}

impl Platform for Fake {
    fn now(&self) -> Timestamp {
        at()
    }
    fn engine(&self, expected: &Digest) -> Result<EngineInstall, PlatformError> {
        let mut unpacked = self.unpacked.lock().unwrap();
        if !unpacked.contains(expected) {
            if *self.bundled.lock().unwrap() != *expected {
                return Err(PlatformError::Engine("This installer carries a different engine.".into()));
            }
            unpacked.push(*expected);
        }
        Ok(EngineInstall {
            root: self.dir.join("engine").join(expected.hex()),
            digest: *expected,
        })
    }
    fn inspect(&self, engine: &EngineInstall) -> Result<Inspection, PlatformError> {
        self.inspected_with.lock().unwrap().push(engine.digest);
        let meanwhile = self.during_inspection.lock().unwrap().take();
        if let Some(meanwhile) = meanwhile {
            meanwhile();
        }
        if let Some(e) = self.inspection_fails.lock().unwrap().take() {
            return Err(e);
        }
        Ok(self.inspection.lock().unwrap().clone())
    }
    fn authenticate_owner(&self, user: &str, password: &[u8]) -> Result<(), PlatformError> {
        if user == "owner" && password == PASSWORD {
            Ok(())
        } else {
            Err(PlatformError::Authorization("That password isn't right for owner.".into()))
        }
    }
    fn user_staging(&self, _uid: u32) -> Result<PathBuf, PlatformError> {
        Ok(self.dir.join("user"))
    }
    fn start_engine(
        &self,
        _: &EngineInstall,
        run: crate::EngineRun<'_>,
        _installation: Uuid,
        plan: &Path,
        owner: &str,
        password: Secret,
        lock: &InstallLock,
    ) -> Result<Box<dyn EngineProcess>, PlatformError> {
        assert_eq!(owner, "owner");
        assert_eq!(&password[..], PASSWORD);
        // The engine is handed the install lock, held while it starts.
        let path = Paths::at(&self.dir.join("state")).lock();
        let handed = std::fs::File::from(std::os::fd::AsFd::as_fd(lock).try_clone_to_owned().unwrap());
        let ino = |m: std::fs::Metadata| std::os::unix::fs::MetadataExt::ino(&m);
        assert_eq!(ino(handed.metadata().unwrap()), ino(std::fs::metadata(&path).unwrap()));
        assert!(InstallLock::try_acquire(&path).unwrap().is_none());
        let plan: Plan = canonical::from_slice(&std::fs::read(plan).unwrap(), limits::PLAN, PlanSchema::VALUE).unwrap();
        let (mode, script) = match run {
            crate::EngineRun::Execute { assets } => {
                let mut found: Vec<(String, Vec<u8>)> = std::fs::read_dir(assets)
                    .unwrap()
                    .map(|e| {
                        let e = e.unwrap();
                        (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap())
                    })
                    .collect();
                found.sort();
                *self.staged_at_start.lock().unwrap() = found;
                *self.engine_runs.lock().unwrap() += 1;
                (EngineMode::Execute, *self.script.lock().unwrap())
            }
            crate::EngineRun::StartupDisk { esp_inventory: inventory } => {
                assert_eq!(*inventory, esp_inventory(), "the retry checks the ESP against its own record");
                *self.startup_runs.lock().unwrap() += 1;
                let script = match *self.script.lock().unwrap() {
                    Script::Refuse(why) => Script::Refuse(why),
                    _ if self.startup_disk_ok.load(Ordering::SeqCst) => Script::Happy,
                    _ => Script::Fail(Stage::StartupDisk, DiskChanged::No),
                };
                (EngineMode::StartupDisk, script)
            }
        };
        Ok(Box::new(ScriptedEngine::start(
            plan,
            mode,
            script,
            self.dir.join("state").join("installations"),
            self.ordering_held.clone(),
        )))
    }
}

fn at() -> Timestamp {
    "2026-11-07T00:00:00Z".parse().unwrap()
}

fn failure(changed: DiskChanged) -> Failure {
    Failure {
        code: "apple-tool".parse().unwrap(),
        message: "Apple's tool failed.".parse().unwrap(),
        apple_diagnostic: Some("Error -69606".parse().unwrap()),
        disk_changed: changed,
    }
}

/// What every scripted engine writes to its diagnostic log, naming its
/// owner and their home folder as a traceback might.
const ENGINE_LOG: [&str; 2] = [
    "Traceback (most recent call last):",
    "  File \"/Users/owner/Library/x.py\", line 1: KeyError: 'owner'",
];

struct ScriptedEngine {
    lines: Receiver<Vec<u8>>,
    acks: Option<Sender<u64>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ScriptedEngine {
    fn start(plan: Plan, mode: EngineMode, script: Script, journals: PathBuf, ordering: Arc<AtomicBool>) -> Self {
        let (line_tx, lines) = channel();
        let (acks, ack_rx) = channel::<u64>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let digest = plan.digest().unwrap();
            let shrinks = plan.actions.iter().any(|a| matches!(a, Action::ShrinkContainer { .. }));
            let mut seq = 0;
            let mut send = |body: EngineBody| -> u64 {
                seq += 1;
                let line = canonical::to_vec(&EngineMessage {
                    schema: EngineSchema,
                    seq,
                    message: body,
                })
                .unwrap();
                let _ = line_tx.send(line);
                seq
            };
            send(EngineBody::Hello {
                engine: "0.9.2+sekka.1".parse().unwrap(),
                asahi: "0.9.2".parse().unwrap(),
                adapter: Digest::of(b"adapter"),
                mode,
                protocol: ENGINE_PROTOCOL,
            });
            if let Script::Refuse(why) = script {
                send(EngineBody::Failure {
                    stage: None,
                    failure: Failure {
                        code: "installation-changed".parse().unwrap(),
                        message: why.parse().unwrap(),
                        apple_diagnostic: None,
                        disk_changed: DiskChanged::No,
                    },
                });
                send(EngineBody::Completion {
                    outcome: EngineOutcome::Failed,
                });
                return;
            }
            let stages = if mode == EngineMode::StartupDisk {
                vec![Stage::StartupDisk]
            } else {
                macos_stages(shrinks)
            };
            for stage in stages {
                if script == Script::Skip(stage) {
                    continue;
                }
                if matches!(script, Script::AnnounceAfterStop(s) | Script::EndBefore(s) if s == stage) {
                    while !stopped.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    if script == Script::EndBefore(stage) {
                        return;
                    }
                }
                let n = send(EngineBody::Intent { stage, plan: digest });
                // Wait for the acknowledgement; the journal must already
                // hold this intent when it arrives.
                let acked = loop {
                    match ack_rx.recv_timeout(Duration::from_millis(20)) {
                        Ok(a) => break Some(a),
                        Err(RecvTimeoutError::Timeout) if stopped.load(Ordering::SeqCst) => break None,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break None,
                    }
                };
                let Some(acked) = acked else { return };
                assert_eq!(acked, n);
                if !journal_has_intent(&journals, stage) {
                    ordering.store(false, Ordering::SeqCst);
                }
                match script {
                    Script::Die(s) if s == stage => return,
                    Script::Hang(s) if s == stage => {
                        while !stopped.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        return;
                    }
                    Script::Fail(s, changed) if s == stage => {
                        send(EngineBody::Failure {
                            stage: Some(stage),
                            failure: failure(changed),
                        });
                        send(EngineBody::Completion {
                            outcome: EngineOutcome::Failed,
                        });
                        return;
                    }
                    _ => {}
                }
                send(EngineBody::Progress { stage, done: 1, total: 1 });
                send(EngineBody::Checkpoint {
                    stage,
                    plan: digest,
                    facts: checkpoint_facts(stage, &plan),
                    verification: Verification::Inspected,
                });
            }
            send(EngineBody::Completion {
                outcome: EngineOutcome::WaitingForRecovery,
            });
        });
        Self {
            lines,
            acks: Some(acks),
            stop,
            thread: Some(thread),
        }
    }
}

fn journal_has_intent(journals: &Path, stage: Stage) -> bool {
    let Ok(ids) = sekka_mac_journal::installations(&FsStorage, journals) else { return false };
    ids.into_iter().any(|id| {
        Journal::open(FsStorage, journals, id)
            .map(|j| matches!(&j.records().last().unwrap().event, Event::Intent { stage: s } if *s == stage))
            .unwrap_or(false)
    })
}

impl EngineProcess for ScriptedEngine {
    fn next_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.lines.recv().ok())
    }
    fn ack(&mut self, seq: u64) -> io::Result<()> {
        self.acks
            .as_ref()
            .ok_or_else(|| io::Error::other("closed"))?
            .send(seq)
            .map_err(|_| io::Error::other("closed"))
    }
    fn terminator(&self) -> Box<dyn FnMut() + Send> {
        let stop = self.stop.clone();
        Box::new(move || stop.store(true, Ordering::SeqCst))
    }
    fn wait(&mut self) -> io::Result<bool> {
        // As a real engine sees end of input, the script sees its
        // acknowledgements end.
        self.acks = None;
        Ok(self.thread.take().map(|t| t.join().is_ok()).unwrap_or(true))
    }
    fn log(&mut self) -> Vec<String> {
        ENGINE_LOG.iter().map(|l| l.to_string()).collect()
    }
}

fn engine_digest() -> Digest {
    Digest::of(b"engine archive")
}

struct World {
    _tmp: tempfile::TempDir,
    helper: Helper<Fake>,
    catalog: SignedCatalog,
    begin: Begin,
    /// The plan the owner reviewed.
    plan: Plan,
}

fn file(dir: &Path, name: &str, bytes: &[u8]) -> (PathBuf, Artifact) {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    (
        path,
        Artifact {
            url: format!("https://downloads.sekka.computer/mac/v1/{name}").parse().unwrap(),
            size: bytes.len() as u64,
            digest: Digest::of(bytes),
        },
    )
}

fn sign(catalog: Catalog) -> SignedCatalog {
    let key = sekka_mac_trust::signing::key(&sekka_mac_trust::signing::development_seed());
    sekka_mac_trust::signing::sign(catalog, sekka_mac_trust::DEVELOPMENT_KEY_ID, &key).unwrap()
}

fn trust() -> sekka_mac_trust::TrustConfig {
    sekka_mac_trust::TrustConfig {
        roots: vec![sekka_mac_trust::development_root()],
        hosts: sekka_mac_trust::PRODUCTION_HOSTS.to_vec(),
        clock_skew: 600,
    }
}

fn fake(dir: &Path, script: Script) -> Fake {
    Fake {
        dir: dir.into(),
        inspection: Mutex::new(samples::inspection()),
        during_inspection: Mutex::new(None),
        inspection_fails: Mutex::new(None),
        bundled: Mutex::new(engine_digest()),
        unpacked: Mutex::new(Vec::new()),
        inspected_with: Mutex::new(Vec::new()),
        staged_at_start: Mutex::new(Vec::new()),
        script: Mutex::new(script),
        startup_disk_ok: AtomicBool::new(true),
        ordering_held: Arc::new(AtomicBool::new(true)),
        engine_runs: Arc::new(Mutex::new(0)),
        startup_runs: Arc::new(Mutex::new(0)),
    }
}

/// The sample Mac with its macOS container already shrunk to 300 GiB, and
/// the space behind it free from [`FREE_START`].
fn free_space() -> Inspection {
    let mut i = samples::inspection();
    i.disk.partitions[1].size = 300 * GIB;
    i.disk.containers[0].size = 300 * GIB;
    i.disk.containers[0].free = 100 * GIB;
    i.candidates = vec![Candidate::FreeSpace {
        extent: Extent {
            offset: FREE_START,
            size: 194 * GIB,
        },
    }];
    i.layout_digest = i.disk.layout_digest().unwrap();
    i.check().unwrap();
    i
}

/// Installing into 100 GiB made by shrinking the sample Mac's container.
fn world(script: Script) -> World {
    let place = Place::ShrinkContainer {
        container: "c54f285b-fe6e-4d0c-9bc6-567c62bd7c20".parse().unwrap(),
    };
    world_at(script, samples::inspection(), place)
}

/// Installing into 100 GiB of [`free_space`]'s free space.
fn free_world(script: Script) -> World {
    world_at(script, free_space(), Place::FreeSpace { offset: FREE_START })
}

fn world_at(script: Script, inspection: Inspection, place: Place) -> World {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("user");
    std::fs::create_dir(&user).unwrap();
    std::fs::set_permissions(&user, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let (payload_path, payload) = file(&user, "payload.zip", &vec![7u8; 64 * 1024]);
    let (data_path, data) = file(&user, "installer_data.json", b"{\"os_list\":[]}");
    let (manifest_path, manifest) = file(&user, "manifest.json", b"{\"schema\":\"sekka.mac.manifest/1\"}");
    let (recovery_path, recovery) = file(&user, "recovery.zip", &vec![3u8; 4096]);

    let mut catalog = samples::catalog();
    let release = &mut catalog.releases[0];
    release.artifacts = Artifacts {
        payload,
        installer_data: data,
        manifest,
        engine: Artifact {
            url: "https://downloads.sekka.computer/mac/v1/engine.tar.gz".parse().unwrap(),
            size: 1024,
            digest: engine_digest(),
        },
        recovery_app: recovery,
    };
    release.image.root_image.size = 4096 * 4;
    let signed = sign(catalog.clone());

    let request = PlanRequest {
        release: "sekka-j514s-4882dc33".parse().unwrap(),
        place,
        sekka: 100 * GIB,
    };
    let plan = derive_plan(&inspection, &catalog, &request, 0).unwrap();
    let staged = [
        (ArtifactRole::Payload, payload_path),
        (ArtifactRole::InstallerData, data_path),
        (ArtifactRole::Manifest, manifest_path),
        (ArtifactRole::RecoveryApp, recovery_path),
    ]
    .into_iter()
    .map(|(role, path)| StagedFile {
        role,
        size: std::fs::metadata(&path).unwrap().len(),
        digest: Digest::of(&std::fs::read(&path).unwrap()),
        path: path.to_str().unwrap().parse().unwrap(),
    })
    .collect();
    let begin = Begin {
        app: "0.1.0".parse().unwrap(),
        catalog: signed.clone(),
        request,
        plan: plan.digest().unwrap(),
        confirmation: Confirmation::for_plan(&plan, true).unwrap(),
        staged,
        owner: "owner".parse().unwrap(),
    };
    let fake = fake(tmp.path(), script);
    *fake.inspection.lock().unwrap() = inspection;
    let helper = Helper::new(fake, Paths::at(&tmp.path().join("state")), trust(), "0.1.0".parse().unwrap()).unwrap();
    World {
        _tmp: tmp,
        helper,
        catalog: signed,
        begin,
        plan,
    }
}

impl World {
    fn ask(&self, request: Request, password: Option<&[u8]>) -> Response {
        let bytes = canonical::to_vec(&HelperRequest {
            schema: HelperRequestSchema,
            id: 7,
            request,
        })
        .unwrap();
        // The person running the tests owns the staged files, as the app's
        // user owns its staging folder.
        let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(self._tmp.path()).unwrap());
        let reply = self
            .helper
            .handle(uid, &bytes, password.map(|p| Secret::new(p.to_vec())));
        let reply: HelperResponse =
            canonical::from_slice(&reply, limits::HELPER_MESSAGE, HelperResponseSchema::VALUE).unwrap();
        assert_eq!(reply.id, 7);
        reply.response
    }
    fn begin(&self) -> Response {
        self.ask(Request::Begin(Box::new(self.begin.clone())), Some(PASSWORD))
    }
    fn begun(&self) -> Uuid {
        match self.begin() {
            Response::Begun { installation } => installation,
            other => panic!("{other:?}"),
        }
    }
    /// Poll as the app does until the installation stops running.
    fn finish(&self, id: Uuid) -> (InstallState, Vec<Record>) {
        for _ in 0..500 {
            match self.ask(Request::Events { installation: id, after: 0 }, None) {
                Response::Events {
                    running: false,
                    state,
                    records,
                    ..
                } => return (state, records),
                Response::Events { .. } => std::thread::sleep(Duration::from_millis(10)),
                other => panic!("{other:?}"),
            }
        }
        panic!("the installation never finished");
    }
    /// Poll until the journal holds a record matching `event`.
    fn wait_for(&self, id: Uuid, event: impl Fn(&Event) -> bool) {
        for _ in 0..500 {
            if let Response::Events { records, .. } = self.ask(Request::Events { installation: id, after: 0 }, None)
                && records.iter().any(|r| event(&r.event))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the record never appeared");
    }
    /// Ask the helper to look at the Mac for an installation.
    fn reconcile(&self, id: Uuid) -> (InstallState, Vec<Record>) {
        match self.ask(Request::Reconcile { installation: id }, None) {
            Response::Events { state, records, running: false, .. } => (state, records),
            other => panic!("{other:?}"),
        }
    }
    fn fake(&self) -> &Fake {
        self.helper.platform_for_tests()
    }
    fn journals(&self) -> PathBuf {
        self.helper.paths_for_tests().journals()
    }
    /// The helper process starts again (after a crash, a restart of the
    /// Mac, or an update of the app): nothing in memory survives, while the
    /// engines it unpacked stay in its cache, and the Mac is as it was.
    fn restarted(self, script: Script) -> World {
        let fake = fake(self._tmp.path(), script);
        *fake.unpacked.lock().unwrap() = self.fake().unpacked.lock().unwrap().clone();
        *fake.inspection.lock().unwrap() = self.fake().inspection.lock().unwrap().clone();
        let helper = Helper::new(fake, self.helper.paths_for_tests().clone(), trust(), "0.1.0".parse().unwrap()).unwrap();
        World { helper, ..self }
    }
    fn lines(&self, id: Uuid) -> Vec<String> {
        let Response::Diagnostics { lines } = self.ask(Request::Diagnostics { installation: id }, None) else { panic!() };
        lines.into_iter().map(|l| l.to_string()).collect()
    }
}

fn refused(response: Response) -> (HelperRefusalCode, String) {
    match response {
        Response::Refused(r) => (r.code, r.message.to_string()),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A journal as a helper that stopped (or a Mac that restarted) inside
/// `open` leaves it: every stage before it finished, as the engine records
/// them, and that stage's intent with no result.
fn abandoned(w: &World, open: Stage) -> Uuid {
    let id = Uuid::random().unwrap();
    let mut j = Journal::create(
        FsStorage,
        &w.journals(),
        id,
        at(),
        Event::Opened {
            plan: Box::new(w.plan.clone()),
            plan_digest: w.plan.digest().unwrap(),
            app: "0.1.0".parse().unwrap(),
            helper: "0.1.0".parse().unwrap(),
        },
    )
    .unwrap();
    let shrinks = w.plan.actions.iter().any(|a| matches!(a, Action::ShrinkContainer { .. }));
    let stages = std::iter::once(Stage::Import).chain(macos_stages(shrinks));
    for stage in stages.take_while(|s| *s != open) {
        let (facts, verification) = match stage {
            Stage::Import => (vec![Fact::new("owner", "owner").unwrap()], Verification::SourceHashed),
            _ => (checkpoint_facts(stage, &w.plan), Verification::Inspected),
        };
        j.append(at(), Event::Intent { stage }).unwrap();
        j.append(at(), Event::Completed { stage, facts, verification }).unwrap();
    }
    j.append(at(), Event::Intent { stage: open }).unwrap();
    id
}

/// This Mac with room made for `plan` and nothing of Sekka in it: the
/// container shrunk to where the plan starts, or the free space as it was.
fn room_for(plan: &Plan) -> Inspection {
    if plan.actions.iter().any(|a| matches!(a, Action::ShrinkContainer { .. })) {
        let mut i = samples::inspection();
        let macos = &mut i.disk.partitions[1];
        macos.size = plan.target.offset - macos.offset;
        i.disk.containers[0].size = macos.size;
        i.disk.containers[0].free = 100 * GIB;
        i.candidates = vec![Candidate::FreeSpace { extent: plan.target }];
        i.layout_digest = i.disk.layout_digest().unwrap();
        i.check().unwrap();
        i
    } else {
        free_space()
    }
}

/// This Mac after macOS's part of `plan`: the stub, ESP and root where the
/// engine put them, which it reports as an installation in `state` naming
/// `id`.
fn installed(plan: &Plan, id: Option<Uuid>, state: InstallationState) -> Inspection {
    let mut i = room_for(plan);
    let ([stub, esp, root], p) = (layout(plan), parts());
    let partition = |uuid, kind, at: Extent| Partition {
        uuid,
        kind,
        name: "Sekka".parse().unwrap(),
        offset: at.offset,
        size: at.size,
    };
    i.disk.partitions.extend([
        partition(p.stub_partition, PartitionKind::Apfs, stub),
        partition(p.esp, PartitionKind::Efi, esp),
        partition(p.root, PartitionKind::Linux, root),
    ]);
    i.disk.containers.push(Container {
        uuid: p.stub_container,
        partition: p.stub_partition,
        size: stub.size,
        free: 0,
        volume_groups: vec![VolumeGroup {
            uuid: p.stub_group,
            name: "Sekka".parse().unwrap(),
            system_volume: None,
            data_volume: None,
            os: GroupOs::Stub {
                for_system: "Sekka".parse().unwrap(),
            },
        }],
        resize: None,
        snapshots: 0,
    });
    i.candidates.clear();
    i.installations.push(Installation {
        kind: InstallationKind::Sekka,
        name: "Sekka".parse().unwrap(),
        state,
        stub_group: Some(p.stub_group),
        esp: Some(p.esp),
        root: Some(p.root),
        installation_id: id,
    });
    i.issues.push(Issue {
        code: IssueCode::ExistingSekka,
        blocking: true,
        detail: "Sekka is already installed on this Mac (Sekka).".parse().unwrap(),
    });
    i.layout_digest = i.disk.layout_digest().unwrap();
    i.check().unwrap();
    i
}

/// This Mac with Sekka's partitions deleted by hand and the space left
/// unallocated.
fn emptied(plan: &Plan) -> Inspection {
    room_for(plan)
}

fn fact_of(event: &Event, name: &str) -> Option<String> {
    let (Event::Completed { facts, .. } | Event::Removed { facts }) = event else { return None };
    facts.iter().find(|f| f.name.as_str() == name).map(|f| f.value.to_string())
}

#[test]
fn hello_checks_the_protocol() {
    let w = world(Script::Happy);
    assert!(matches!(
        w.ask(Request::Hello { app: "0.1.0".parse().unwrap(), protocol: HELPER_PROTOCOL }, None),
        Response::Hello { protocol: HELPER_PROTOCOL, .. }
    ));
    assert_eq!(
        refused(w.ask(Request::Hello { app: "0.1.0".parse().unwrap(), protocol: 99 }, None)).0,
        HelperRefusalCode::Protocol
    );
    let garbage = w.helper.handle(501, b"{\"schema\":\"nope\"}", None);
    let reply: HelperResponse = canonical::from_slice(&garbage, limits::HELPER_MESSAGE, HelperResponseSchema::VALUE).unwrap();
    assert!(matches!(reply.response, Response::Refused(HelperRefusal { code: HelperRefusalCode::Malformed, .. })));
}

#[test]
fn a_complete_installation_waits_for_recovery_with_every_stage_journaled_first() {
    let w = world(Script::Happy);
    let id = w.begun();
    let (state, records) = w.finish(id);
    assert_eq!(state, InstallState::WaitingForRecovery);
    assert!(w.fake().ordering_held.load(Ordering::SeqCst), "an intent was acknowledged before it was durable");
    let completed: Vec<Stage> = records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Completed { stage, .. } => Some(*stage),
            _ => None,
        })
        .collect();
    let mut expected = vec![Stage::Import];
    expected.extend(macos_stages(true));
    assert_eq!(completed, expected);
    // The engine ran from the helper's own copies of the downloads. Once it
    // is done they are removed (they hold their space from macOS), and only
    // the plan stays.
    let found: Vec<String> = w.fake().staged_at_start.lock().unwrap().iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(found, ["installer_data.json", "manifest.json", "payload.zip", "plan.json", "recovery.zip"]);
    let staged = w.helper.paths_for_tests().staging(id);
    let left: Vec<_> = std::fs::read_dir(&staged).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left, ["plan.json"]);
    // Diagnostics name no account.
    let Response::Diagnostics { lines } = w.ask(Request::Diagnostics { installation: id }, None) else { panic!() };
    assert!(lines.iter().all(|l| !l.as_str().contains("owner")), "{lines:?}");
    // And status lists it.
    let Response::Status { installations } = w.ask(Request::Status, None) else { panic!() };
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].state, InstallState::WaitingForRecovery);
}

#[test]
fn downloads_left_by_an_earlier_run_are_removed_when_the_helper_starts() {
    let w = world(Script::Happy);
    let staged = w.helper.paths_for_tests().staging(Uuid::random().unwrap());
    std::fs::create_dir_all(&staged).unwrap();
    for name in ["payload.zip", "recovery.zip", "plan.json"] {
        std::fs::write(staged.join(name), b"x").unwrap();
    }
    let _w = w.restarted(Script::Happy);
    let left: Vec<_> = std::fs::read_dir(&staged).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left, ["plan.json"]);
}

#[test]
fn a_wrong_password_changes_nothing() {
    let w = world(Script::Happy);
    let r = w.ask(Request::Begin(Box::new(w.begin.clone())), Some(b"wrong"));
    assert_eq!(refused(r).0, HelperRefusalCode::Authorization);
    let r = w.ask(Request::Begin(Box::new(w.begin.clone())), None);
    assert_eq!(refused(r).0, HelperRefusalCode::Authorization);
    let Response::Status { installations } = w.ask(Request::Status, None) else { panic!() };
    assert!(installations.is_empty());
}

#[test]
fn a_changed_mac_needs_a_new_review() {
    let w = world(Script::Happy);
    {
        let mut i = w.fake().inspection.lock().unwrap();
        i.disk.partitions[2].size -= GIB;
        i.layout_digest = i.disk.layout_digest().unwrap();
    }
    let (code, message) = refused(w.begin());
    assert_eq!(code, HelperRefusalCode::PlanChanged);
    assert!(message.contains("review"), "{message}");
    assert_eq!(*w.fake().engine_runs.lock().unwrap(), 0);
}

#[test]
fn status_and_events_answer_while_a_begin_inspects() {
    let w = world(Script::Happy);
    let (inspecting, wait) = channel();
    let (go, gate) = channel::<()>();
    *w.fake().during_inspection.lock().unwrap() = Some(Box::new(move || {
        inspecting.send(()).unwrap();
        gate.recv().unwrap();
    }));
    std::thread::scope(|s| {
        let begun = s.spawn(|| w.begun());
        wait.recv().unwrap();
        // The slow checks hold nothing that Status waits for, and a second
        // Begin is refused rather than queued.
        assert!(matches!(w.ask(Request::Status, None), Response::Status { .. }));
        assert_eq!(refused(w.begin()).0, HelperRefusalCode::Busy);
        go.send(()).unwrap();
        let id = begun.join().unwrap();
        assert_eq!(w.finish(id).0, InstallState::WaitingForRecovery);
    });
}

#[test]
fn an_altered_or_older_catalog_is_refused() {
    let w = world(Script::Happy);
    let mut begin = w.begin.clone();
    begin.catalog.catalog.releases[0].allocation.recommended += 1;
    let r = w.ask(Request::Begin(Box::new(begin)), Some(PASSWORD));
    assert_eq!(refused(r).0, HelperRefusalCode::Catalog);

    let mut newer = w.catalog.catalog.clone();
    newer.serial = 9;
    let verified = sekka_mac_trust::verify_signed(sign(newer), &sekka_mac_trust::TrustConfig {
        roots: vec![sekka_mac_trust::development_root()],
        hosts: sekka_mac_trust::PRODUCTION_HOSTS.to_vec(),
        clock_skew: 600,
    }, "2026-11-07T00:00:00Z".parse().unwrap()).unwrap();
    sekka_mac_trust::AcceptedStore::open(&w.helper.paths_for_tests().accepted())
        .unwrap()
        .accept(&verified, "2026-11-07T00:00:00Z".parse().unwrap())
        .unwrap();
    let (code, message) = refused(w.begin());
    assert_eq!(code, HelperRefusalCode::Catalog);
    assert!(message.contains("older"), "{message}");
}

#[test]
fn imported_files_are_named_by_their_role_not_the_apps_path() {
    let w = world(Script::Happy);
    // The app stages the installer data under another file's name; the
    // helper still files it as installer_data.json.
    let mut begin = w.begin.clone();
    let data = begin.staged[1].path.to_string();
    let renamed = std::path::Path::new(&data).with_file_name("recovery-copy.zip");
    std::fs::rename(&data, &renamed).unwrap();
    begin.staged[1].path = renamed.to_str().unwrap().parse().unwrap();
    let id = match w.ask(Request::Begin(Box::new(begin)), Some(PASSWORD)) {
        Response::Begun { installation } => installation,
        other => panic!("{other:?}"),
    };
    let (state, _) = w.finish(id);
    assert_eq!(state, InstallState::WaitingForRecovery);
    let found = w.fake().staged_at_start.lock().unwrap().clone();
    assert!(found.contains(&("installer_data.json".into(), b"{\"os_list\":[]}".to_vec())), "{found:?}");
    assert!(found.iter().all(|(name, _)| name != "recovery-copy.zip"));
}

#[test]
fn a_tampered_download_stops_before_the_engine() {
    let w = world(Script::Happy);
    let payload = w.begin.staged[0].path.to_string();
    let mut bytes = std::fs::read(&payload).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&payload, bytes).unwrap();
    let id = w.begun();
    let (state, _) = w.finish(id);
    assert_eq!(
        state,
        InstallState::Failed {
            stage: Stage::Import,
            disk_changed: DiskChanged::No
        }
    );
    assert_eq!(*w.fake().engine_runs.lock().unwrap(), 0);
    assert!(state.allows_new_installation());
}

#[test]
fn a_resize_that_changed_nothing_allows_trying_again() {
    let w = world(Script::Fail(Stage::Shrink, DiskChanged::No));
    let (state, records) = w.finish(w.begun());
    assert_eq!(
        state,
        InstallState::Failed {
            stage: Stage::Shrink,
            disk_changed: DiskChanged::No
        }
    );
    let failure = records.iter().find_map(|r| match &r.event {
        Event::Failed { failure, .. } => Some(failure.clone()),
        _ => None,
    });
    assert_eq!(failure.unwrap().apple_diagnostic.unwrap().as_str(), "Error -69606");
    *w.fake().script.lock().unwrap() = Script::Happy;
    let (state, _) = w.finish(w.begun());
    assert_eq!(state, InstallState::WaitingForRecovery);
}

#[test]
fn an_engine_that_dies_mid_write_blocks_further_installs() {
    let w = world(Script::Die(Stage::Root));
    let (state, _) = w.finish(w.begun());
    assert_eq!(
        state,
        InstallState::Failed {
            stage: Stage::Root,
            disk_changed: DiskChanged::Unknown
        }
    );
    *w.fake().script.lock().unwrap() = Script::Happy;
    let (code, message) = refused(w.begin());
    assert_eq!(code, HelperRefusalCode::Busy);
    assert!(message.contains("hasn't finished"), "{message}");
}

#[test]
fn a_protocol_violation_is_recorded_and_stops_the_run() {
    let w = world(Script::Skip(Stage::Shrink));
    let (state, _) = w.finish(w.begun());
    // The engine tried to partition without shrinking. Nothing was
    // acknowledged, so the shrink is recorded as failed with the disk as it
    // was, and the Mac can be planned for again.
    assert_eq!(
        state,
        InstallState::Failed {
            stage: Stage::Shrink,
            disk_changed: DiskChanged::No
        }
    );
}

#[test]
fn cancelling_is_allowed_only_before_the_disk_changes() {
    let w = world(Script::Hang(Stage::AppleInputs));
    let id = w.begun();
    // Wait for the engine to be inside the preparation stage.
    for _ in 0..200 {
        if let Response::Events { state: InstallState::AssetsVerified, progress: Some(_), .. } =
            w.ask(Request::Events { installation: id, after: 0 }, None)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(w.ask(Request::Cancel { installation: id }, None), Response::Cancelled);
    let (state, _) = w.finish(id);
    assert_eq!(state, InstallState::Cancelled);

    let w = world(Script::Hang(Stage::Shrink));
    let id = w.begun();
    for _ in 0..200 {
        if let Response::Events { state: InstallState::MutationStarted { .. }, .. } =
            w.ask(Request::Events { installation: id, after: 0 }, None)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let (code, message) = refused(w.ask(Request::Cancel { installation: id }, None));
    assert_eq!(code, HelperRefusalCode::State);
    assert!(message.contains("isn't safe"), "{message}");
    // Let the test end: stop the hanging engine directly.
    w.helper.terminate_for_tests(id);
    w.finish(id);
}

#[test]
fn a_cancel_between_apple_inputs_and_the_first_disk_stage_is_a_cancel() {
    // Whether the engine's next intent is already on its way when the
    // cancel lands, or the signal stops it before it writes one, the first
    // stage that changes the disk was never acknowledged: nothing changed,
    // and the record says cancelled, not that resizing failed.
    for script in [Script::AnnounceAfterStop(Stage::Shrink), Script::EndBefore(Stage::Shrink)] {
        let w = world(script);
        let id = w.begun();
        w.wait_for(id, |e| matches!(e, Event::Completed { stage: Stage::AppleInputs, .. }));
        assert_eq!(w.ask(Request::Cancel { installation: id }, None), Response::Cancelled);
        let (state, records) = w.finish(id);
        assert_eq!(state, InstallState::Cancelled, "{script:?}");
        assert!(
            records.iter().all(|r| !matches!(
                r.event,
                Event::Intent { stage: Stage::Shrink } | Event::Failed { stage: Stage::Shrink, .. }
            )),
            "{script:?}: {records:?}"
        );
        *w.fake().script.lock().unwrap() = Script::Happy;
        assert_eq!(w.finish(w.begun()).0, InstallState::WaitingForRecovery, "{script:?}");
    }
}

#[test]
fn an_installation_stopped_during_a_download_can_be_cancelled_after_a_restart() {
    for open in [Stage::Import, Stage::AppleInputs] {
        let w = world(Script::Happy);
        let id = abandoned(&w, open);
        assert_eq!(refused(w.begin()).0, HelperRefusalCode::Busy, "{open:?}: it blocks another");
        assert_eq!(w.ask(Request::Cancel { installation: id }, None), Response::Cancelled);
        let (state, records) = w.finish(id);
        assert_eq!(state, InstallState::Cancelled);
        assert!(
            records.iter().any(|r| matches!(
                &r.event,
                Event::Failed { stage, failure } if *stage == open && failure.code.as_str() == "interrupted"
            )),
            "{open:?}: {records:?}"
        );
        assert_eq!(w.finish(w.begun()).0, InstallState::WaitingForRecovery, "{open:?}");
    }
}

#[test]
fn a_retry_needs_the_esp_inventory_its_record_kept() {
    // A record whose Esp stage kept no inventory (an earlier helper's): the
    // engine couldn't check the ESP, so the retry is refused before the
    // password is checked, and nothing runs.
    let w = world(Script::Happy);
    let id = Uuid::random().unwrap();
    let mut j = Journal::create(
        FsStorage,
        &w.journals(),
        id,
        at(),
        Event::Opened {
            plan: Box::new(w.plan.clone()),
            plan_digest: w.plan.digest().unwrap(),
            app: "0.1.0".parse().unwrap(),
            helper: "0.1.0".parse().unwrap(),
        },
    )
    .unwrap();
    let shrinks = w.plan.actions.iter().any(|a| matches!(a, Action::ShrinkContainer { .. }));
    let stages = std::iter::once(Stage::Import).chain(macos_stages(shrinks));
    for stage in stages.take_while(|s| *s != Stage::StartupDisk) {
        let (facts, verification) = match stage {
            Stage::Import => (vec![Fact::new("owner", "owner").unwrap()], Verification::SourceHashed),
            Stage::Esp => (vec![Fact::new("checked", stage.title()).unwrap()], Verification::ReadBack),
            _ => (checkpoint_facts(stage, &w.plan), Verification::Inspected),
        };
        j.append(at(), Event::Intent { stage }).unwrap();
        j.append(at(), Event::Completed { stage, facts, verification }).unwrap();
    }
    j.append(at(), Event::Intent { stage: Stage::StartupDisk }).unwrap();
    j.append(
        at(),
        Event::Failed {
            stage: Stage::StartupDisk,
            failure: Failure {
                code: "bless-failed".parse().unwrap(),
                message: "bless couldn't set the startup disk".parse().unwrap(),
                apple_diagnostic: None,
                disk_changed: DiskChanged::No,
            },
        },
    )
    .unwrap();
    assert_eq!(j.state(), &InstallState::StartupDiskFailed);
    let passwords: [&[u8]; 2] = [PASSWORD, b"nope"];
    for password in passwords {
        let (code, message) = refused(w.ask(Request::RetryStartupDisk { installation: id }, Some(password)));
        assert_eq!(code, HelperRefusalCode::State, "{message}");
        assert!(message.contains("boot partition"), "{message}");
    }
    assert_eq!(*w.fake().startup_runs.lock().unwrap(), 0);
}

#[test]
fn only_the_startup_disk_is_retried() {
    let w = world(Script::Fail(Stage::StartupDisk, DiskChanged::No));
    let id = w.begun();
    let (state, _) = w.finish(id);
    assert_eq!(state, InstallState::StartupDiskFailed);
    w.fake().startup_disk_ok.store(false, Ordering::SeqCst);
    let Response::Events { state, .. } = w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)) else { panic!() };
    assert_eq!(state, InstallState::StartupDiskFailed);
    w.fake().startup_disk_ok.store(true, Ordering::SeqCst);
    assert_eq!(
        refused(w.ask(Request::RetryStartupDisk { installation: id }, Some(b"nope"))).0,
        HelperRefusalCode::Authorization
    );
    let Response::Events { state, .. } = w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)) else { panic!() };
    assert_eq!(state, InstallState::WaitingForRecovery);
    assert_eq!(*w.fake().engine_runs.lock().unwrap(), 1, "retrying never reruns the installation");
    assert_eq!(*w.fake().startup_runs.lock().unwrap(), 2, "only the startup-disk stage, twice");
    assert!(w.fake().ordering_held.load(Ordering::SeqCst));
    let (code, _) = refused(w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)));
    assert_eq!(code, HelperRefusalCode::State);
}

#[test]
fn a_startup_disk_stopped_part_way_is_retried_once_no_engine_runs() {
    // The helper or the Mac restarted while the startup disk was being set,
    // in execute or in an earlier retry.
    for retried in [false, true] {
        let w = world(Script::Happy);
        let id = abandoned(&w, Stage::StartupDisk);
        if retried {
            let mut j = Journal::open(FsStorage, &w.journals(), id).unwrap();
            let failure = failure(DiskChanged::No);
            j.append(at(), Event::Failed { stage: Stage::StartupDisk, failure }).unwrap();
            j.append(at(), Event::Intent { stage: Stage::StartupDisk }).unwrap();
        }
        let w = w.restarted(Script::Happy);
        let Response::Events { state, records, .. } = w.ask(Request::Events { installation: id, after: 0 }, None) else {
            panic!()
        };
        assert_eq!(state, InstallState::MutationStarted { stage: Stage::StartupDisk });
        assert_eq!(state.permitted()[0], NextAction::RetryStartupDisk);
        // While the engine that was setting it may still be running, nothing
        // is tried or recorded.
        let engine = InstallLock::try_acquire(&w.helper.paths_for_tests().lock()).unwrap().unwrap();
        let (code, _) = refused(w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)));
        assert_eq!(code, HelperRefusalCode::Busy);
        drop(engine);
        // Nor with the wrong password.
        let (code, _) = refused(w.ask(Request::RetryStartupDisk { installation: id }, Some(b"nope")));
        assert_eq!(code, HelperRefusalCode::Authorization);
        let Response::Events { records: unchanged, .. } = w.ask(Request::Events { installation: id, after: 0 }, None) else {
            panic!()
        };
        assert_eq!(unchanged, records);
        let Response::Events { state, records: after, .. } =
            w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD))
        else {
            panic!()
        };
        assert_eq!(state, InstallState::WaitingForRecovery, "retried: {retried}");
        assert!(matches!(
            &after[records.len()].event,
            Event::Failed { stage: Stage::StartupDisk, failure }
                if failure.code.as_str() == "interrupted" && failure.disk_changed == DiskChanged::Unknown
        ));
        assert!(matches!(after[records.len() + 1].event, Event::Intent { stage: Stage::StartupDisk }));
        assert_eq!(*w.fake().startup_runs.lock().unwrap(), 1);
        assert_eq!(*w.fake().engine_runs.lock().unwrap(), 0, "only that stage runs");
    }
}

#[test]
fn a_retry_the_engine_refuses_says_why() {
    let w = world(Script::Fail(Stage::StartupDisk, DiskChanged::No));
    let id = w.begun();
    let (_, records) = w.finish(id);
    let why = "No startup volume on this Mac holds this installation any more.";
    *w.fake().script.lock().unwrap() = Script::Refuse(why);
    let (code, message) = refused(w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)));
    assert_eq!(code, HelperRefusalCode::State);
    assert_eq!(message, why);
    // Nothing was recorded; the reason is in the diagnostics too.
    let Response::Events { state, records: now, running, .. } =
        w.ask(Request::Events { installation: id, after: 0 }, None)
    else {
        panic!()
    };
    assert_eq!((state, now.len(), running), (InstallState::StartupDiskFailed, records.len(), false));
    assert!(w.lines(id).iter().any(|l| l.contains(why)));
}

#[test]
fn diagnostics_name_no_account_after_a_restart() {
    let w = world(Script::Refuse("owner isn't one of this Mac's administrators who can approve startup changes in Recovery."));
    let id = w.begun();
    let (state, _) = w.finish(id);
    assert!(matches!(state, InstallState::Failed { stage: Stage::AppleInputs, .. }), "{state:?}");
    let w = w.restarted(Script::Happy);
    let lines = w.lines(id);
    assert!(lines.iter().any(|l| l.contains("(account) isn't one of")), "{lines:?}");
    assert!(lines.iter().all(|l| !l.contains("owner")), "{lines:?}");
}

#[test]
fn the_engines_own_log_reaches_the_diagnostics_without_names() {
    // A stage stopped with only its exception's text in the transcript; the
    // traceback is in the engine's own log.
    let w = world(Script::Die(Stage::Root));
    let id = w.begun();
    w.finish(id);
    let lines = w.lines(id);
    let at = lines.iter().position(|l| l == "stderr: Traceback (most recent call last):").expect("the engine's log is kept");
    assert_eq!(
        lines[at + 1],
        "stderr:   File \"/Users/…/Library/x.py\", line 1: KeyError: '(account)'"
    );
    assert!(lines.iter().all(|l| !l.contains("owner")), "{lines:?}");
    // A retry's too, and a failed inspection's when the helper looks at
    // the Mac for an installation.
    let w = world(Script::Fail(Stage::StartupDisk, DiskChanged::No));
    let id = w.begun();
    w.finish(id);
    let before = w.lines(id).iter().filter(|l| l.starts_with("stderr: ")).count();
    let Response::Events { .. } = w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)) else { panic!() };
    assert_eq!(w.lines(id).iter().filter(|l| l.starts_with("stderr: ")).count(), before + ENGINE_LOG.len());
    let w = world(Script::Die(Stage::Root));
    let id = w.begun();
    w.finish(id);
    let w = w.restarted(Script::Happy);
    *w.fake().inspection_fails.lock().unwrap() = Some(PlatformError::Inspection {
        message: "This Mac couldn't be inspected: 'Volumes'".into(),
        log: vec!["KeyError: 'Volumes' for owner".into()],
    });
    let (code, message) = refused(w.ask(Request::Reconcile { installation: id }, None));
    assert_eq!(code, HelperRefusalCode::State);
    assert!(message.contains("'Volumes'"), "{message}");
    assert!(w.lines(id).contains(&"stderr: KeyError: 'Volumes' for (account)".to_string()));
}

#[test]
fn long_text_is_cut_on_a_character_boundary() {
    // A failure message and Apple's diagnostic at their limits, in
    // characters wider than a byte: one diagnostics line can't hold both.
    let w = world(Script::Happy);
    let id = abandoned(&w, Stage::Import);
    let mut j = Journal::open(FsStorage, &w.journals(), id).unwrap();
    j.append(
        at(),
        Event::Failed {
            stage: Stage::Import,
            failure: Failure {
                code: "download".parse().unwrap(),
                message: "é".repeat(1024).parse().unwrap(),
                apple_diagnostic: Some("…".repeat(682).parse().unwrap()),
                disk_changed: DiskChanged::No,
            },
        },
    )
    .unwrap();
    let lines = w.lines(id);
    let failed = lines.iter().find(|l| l.contains("FAILED")).unwrap();
    assert!(failed.len() <= 2048 && failed.len() > 2000, "{}", failed.len());
}

#[test]
fn recovery_finished_is_recorded_from_the_helpers_own_inspection() {
    let w = world(Script::Happy);
    let id = w.begun();
    let (_, before) = w.finish(id);
    *w.fake().inspection.lock().unwrap() = installed(&w.plan, Some(id), InstallationState::Complete);
    let (state, records) = w.reconcile(id);
    assert_eq!(state, InstallState::WaitingForSetup);
    let added: Vec<&Event> = records[before.len()..].iter().map(|r| &r.event).collect();
    assert!(matches!(
        added[..],
        [
            Event::Intent { stage: Stage::Enrollment },
            Event::Completed { stage: Stage::Enrollment, verification: Verification::PolicyReadBack, .. },
            Event::Intent { stage: Stage::StartupSelection },
            Event::Completed { stage: Stage::StartupSelection, verification: Verification::PolicyReadBack, .. },
        ]
    ), "{added:?}");
    assert_eq!(fact_of(added[3], "source").as_deref(), Some("helper-inspection"));
    assert_eq!(fact_of(added[3], "stub-group"), Some(parts().stub_group.to_string()));
    assert!(state.allows_new_installation());
    // Asking again needs no look at the Mac and changes nothing.
    let looked = w.fake().inspected_with.lock().unwrap().len();
    assert_eq!(w.reconcile(id), (state, records));
    assert_eq!(w.fake().inspected_with.lock().unwrap().len(), looked);
}

#[test]
fn recovery_not_finished_changes_nothing() {
    let w = world(Script::Happy);
    let id = w.begun();
    let (_, before) = w.finish(id);
    // Still waiting for Recovery, as the boot policy shows.
    *w.fake().inspection.lock().unwrap() = installed(&w.plan, Some(id), InstallationState::WaitingForRecovery);
    assert_eq!(w.reconcile(id), (InstallState::WaitingForRecovery, before.clone()));
    // Complete, but not made of this installation's parts.
    let mut other = installed(&w.plan, Some(id), InstallationState::Complete);
    other.installations[0].root = Some(other.disk.partitions[2].uuid);
    *w.fake().inspection.lock().unwrap() = other;
    assert_eq!(w.reconcile(id), (InstallState::WaitingForRecovery, before));
    assert!(w.lines(id).iter().any(|l| l.contains("nothing recorded")));
}

#[test]
fn a_free_space_installation_is_found_where_its_engine_put_it() {
    let w = free_world(Script::Happy);
    let id = w.begun();
    let (state, before) = w.finish(id);
    assert_eq!(state, InstallState::WaitingForRecovery);
    // The free space starts short of the plan's aligned offset, and the
    // engine put the stub there.
    let stub: u64 = before.iter().find_map(|r| fact_of(&r.event, "stub-offset")).unwrap().parse().unwrap();
    assert!(stub < w.plan.target.offset && w.plan.target.offset - stub < ALIGN);
    let finished = installed(&w.plan, Some(id), InstallationState::Complete);

    // Parts that aren't where its stages recorded creating them, or a plan
    // that doesn't give them that space, change nothing.
    let mut moved = finished.clone();
    moved.disk.partitions.last_mut().unwrap().offset += 4096;
    moved.layout_digest = moved.disk.layout_digest().unwrap();
    let mut later = w.plan.clone();
    later.target.offset += ALIGN;
    for (plan, inspection, why) in [
        (&w.plan, &moved, "where its own stages created them"),
        (&later, &finished, "where its plan put them"),
    ] {
        let found = crate::reconcile::look(&before, &state, plan, inspection);
        assert!(matches!(&found, crate::reconcile::Finding::Unchanged(w) if w.ends_with(why)), "{found:?}");
    }

    *w.fake().inspection.lock().unwrap() = finished;
    let (state, records) = w.reconcile(id);
    assert_eq!(state, InstallState::WaitingForSetup);
    assert_eq!(records.len(), before.len() + 4);
}

#[test]
fn an_installation_removed_by_hand_is_recorded_as_removed() {
    // Stopped part way, then removed by hand, macOS grown back over the
    // space.
    let w = world(Script::Die(Stage::Root));
    let id = w.begun();
    let (state, before) = w.finish(id);
    assert!(!state.allows_new_installation());
    *w.fake().script.lock().unwrap() = Script::Happy;
    assert_eq!(refused(w.begin()).0, HelperRefusalCode::Busy);
    let (state, records) = w.reconcile(id);
    assert_eq!(state, InstallState::Removed);
    assert_eq!(records.len(), before.len() + 1);
    let removed = &records.last().unwrap().event;
    assert_eq!(fact_of(removed, "source").as_deref(), Some("helper-inspection"));
    assert_eq!(fact_of(removed, "target-extent").as_deref(), Some("macos-container"));
    // Another installation may begin.
    assert_eq!(w.finish(w.begun()).0, InstallState::WaitingForRecovery);

    // Waiting for Recovery, then removed, the space left unallocated.
    let w = world(Script::Happy);
    let id = w.begun();
    w.finish(id);
    *w.fake().inspection.lock().unwrap() = emptied(&w.plan);
    let (state, records) = w.reconcile(id);
    assert_eq!(state, InstallState::Removed);
    assert_eq!(fact_of(&records.last().unwrap().event, "target-extent").as_deref(), Some("unallocated"));
}

#[test]
fn a_free_space_installation_removed_by_hand_is_recorded_as_removed() {
    // Stopped part way, or waiting for Recovery, then removed by hand and
    // macOS grown back over the space it had left free.
    for script in [Script::Die(Stage::Root), Script::Happy] {
        let w = free_world(script);
        let id = w.begun();
        let (state, before) = w.finish(id);
        assert!(!state.allows_new_installation());
        *w.fake().inspection.lock().unwrap() = samples::inspection();
        let (state, records) = w.reconcile(id);
        assert_eq!(state, InstallState::Removed, "{script:?}");
        assert_eq!(records.len(), before.len() + 1);
        assert_eq!(fact_of(&records.last().unwrap().event, "target-extent").as_deref(), Some("macos-container"));
    }
    // Or with the space left free.
    let w = free_world(Script::Happy);
    let id = w.begun();
    w.finish(id);
    *w.fake().inspection.lock().unwrap() = emptied(&w.plan);
    assert_eq!(w.reconcile(id).0, InstallState::Removed);
    // Any other partition grown over the space is still something there.
    let w = free_world(Script::Happy);
    let id = w.begun();
    let (state, before) = w.finish(id);
    let mut other = samples::inspection();
    other.disk.containers[0].volume_groups[0].uuid = "8e945eba-968d-4906-966b-e07ec593e16a".parse().unwrap();
    other.layout_digest = other.disk.layout_digest().unwrap();
    *w.fake().inspection.lock().unwrap() = other;
    assert_eq!(w.reconcile(id), (state, before));
}

#[test]
fn an_unrelated_empty_container_doesnt_stop_a_removal() {
    // The owner made a data container in Disk Utility, after the Recovery
    // container. The engine reports it as an unknown, partial installation
    // with none of its parts named.
    let w = world(Script::Die(Stage::Root));
    let id = w.begun();
    w.finish(id);
    let mut i = emptied(&w.plan);
    let partition: Uuid = "6b1d3e5f-2222-4a2b-8c3d-000000000001".parse().unwrap();
    let after_recovery = i.disk.partitions[2].offset + i.disk.partitions[2].size;
    i.disk.partitions.push(Partition {
        uuid: partition,
        kind: PartitionKind::Apfs,
        name: "Data".parse().unwrap(),
        offset: after_recovery,
        size: 256 * 1024 * 1024,
    });
    i.disk.containers.push(Container {
        uuid: "6b1d3e5f-2222-4a2b-8c3d-000000000002".parse().unwrap(),
        partition,
        size: 256 * 1024 * 1024,
        free: 255 * 1024 * 1024,
        volume_groups: vec![],
        resize: None,
        snapshots: 0,
    });
    i.installations.push(Installation {
        kind: InstallationKind::Unknown,
        name: "Data".parse().unwrap(),
        state: InstallationState::Partial,
        stub_group: None,
        esp: None,
        root: None,
        installation_id: None,
    });
    i.issues.push(Issue {
        code: IssueCode::UnrecognizedLayout,
        blocking: false,
        detail: "The APFS container disk0s4 (Data) holds no system the installer recognizes.".parse().unwrap(),
    });
    i.layout_digest = i.disk.layout_digest().unwrap();
    i.check().unwrap();
    *w.fake().inspection.lock().unwrap() = i;
    assert_eq!(w.reconcile(id).0, InstallState::Removed);
}

#[test]
fn an_ambiguous_inspection_changes_nothing() {
    let w = world(Script::Die(Stage::Root));
    let id = w.begun();
    let (state, before) = w.finish(id);
    let mut another_disk = samples::inspection();
    another_disk.disk.uuid = "0d4e3e3b-0b3a-4b0a-9a0a-000000000001".parse().unwrap();
    another_disk.layout_digest = another_disk.disk.layout_digest().unwrap();
    let mut two_disks = samples::inspection();
    two_disks.issues.push(Issue {
        code: IssueCode::AmbiguousDisk,
        blocking: true,
        detail: "There is more than one internal disk.".parse().unwrap(),
    });
    let mut unplaced = samples::inspection();
    unplaced.installations.push(Installation {
        kind: InstallationKind::Sekka,
        name: "Sekka".parse().unwrap(),
        state: InstallationState::Partial,
        stub_group: None,
        esp: Some(parts().esp),
        root: None,
        installation_id: None,
    });
    for (case, inspection) in [
        ("another disk", another_disk),
        ("more than one disk", two_disks),
        ("its parts are still there", installed(&w.plan, Some(id), InstallationState::Partial)),
        ("parts of an unidentified Sekka are there", installed(&w.plan, None, InstallationState::Partial)),
        ("an unidentified Sekka that can't be placed", unplaced),
    ] {
        *w.fake().inspection.lock().unwrap() = inspection;
        assert_eq!(w.reconcile(id), (state.clone(), before.clone()), "{case}");
    }
}

#[test]
fn reconcile_is_refused_while_the_engine_runs() {
    let w = world(Script::Hang(Stage::AppleInputs));
    let id = w.begun();
    w.wait_for(id, |e| matches!(e, Event::Intent { stage: Stage::AppleInputs }));
    *w.fake().inspection.lock().unwrap() = emptied(&w.plan);
    assert_eq!(refused(w.ask(Request::Reconcile { installation: id }, None)).0, HelperRefusalCode::Busy);
    assert_eq!(w.ask(Request::Cancel { installation: id }, None), Response::Cancelled);
    assert_eq!(w.finish(id).0, InstallState::Cancelled);
}

#[test]
fn nothing_else_writes_the_record_while_the_mac_is_inspected() {
    let w = world(Script::Happy);
    let id = abandoned(&w, Stage::AppleInputs);
    *w.fake().inspection.lock().unwrap() = samples::inspection();
    let (inspecting, wait) = channel();
    let (go, gate) = channel::<()>();
    *w.fake().during_inspection.lock().unwrap() = Some(Box::new(move || {
        inspecting.send(()).unwrap();
        gate.recv().unwrap();
    }));
    std::thread::scope(|s| {
        let looked = s.spawn(|| w.reconcile(id));
        wait.recv().unwrap();
        // While the Mac is inspected, Status and Events answer, and the
        // owner's cancel or a new installation waits its turn.
        assert!(matches!(w.ask(Request::Status, None), Response::Status { .. }));
        assert!(matches!(w.ask(Request::Events { installation: id, after: 0 }, None), Response::Events { .. }));
        assert_eq!(refused(w.ask(Request::Cancel { installation: id }, None)).0, HelperRefusalCode::Busy);
        assert_eq!(refused(w.begin()).0, HelperRefusalCode::Busy);
        go.send(()).unwrap();
        // Nothing of it was ever on the disk.
        assert_eq!(looked.join().unwrap().0, InstallState::Removed);
    });
}

#[test]
fn an_engine_left_running_by_an_earlier_helper_holds_off_reconcile() {
    // The helper stopped while its engine was shrinking the container, and
    // started again. The engine, which holds the install lock it was
    // handed, is still at it: the container hasn't shrunk yet.
    let w = world(Script::Happy);
    let id = abandoned(&w, Stage::Shrink);
    let w = w.restarted(Script::Happy);
    *w.fake().inspection.lock().unwrap() = samples::inspection();
    let engine = InstallLock::try_acquire(&w.helper.paths_for_tests().lock()).unwrap().unwrap();
    let Response::Events { state, records, .. } = w.ask(Request::Events { installation: id, after: 0 }, None) else {
        panic!()
    };
    assert_eq!(state, InstallState::MutationStarted { stage: Stage::Shrink });
    let (code, message) = refused(w.ask(Request::Reconcile { installation: id }, None));
    assert_eq!(code, HelperRefusalCode::Busy);
    assert!(message.contains("still finishing"), "{message}");
    assert!(w.fake().inspected_with.lock().unwrap().is_empty(), "the Mac isn't inspected meanwhile");
    assert_eq!(refused(w.begin()).0, HelperRefusalCode::Busy);
    // Once it has stopped, the Mac shows nothing of the installation.
    drop(engine);
    let (state, after) = w.reconcile(id);
    assert_eq!(state, InstallState::Removed);
    assert_eq!(after.len(), records.len() + 1);
}

#[test]
fn the_installations_own_engine_is_used_after_an_app_update() {
    let w = world(Script::Fail(Stage::StartupDisk, DiskChanged::No));
    let id = w.begun();
    assert_eq!(w.finish(id).0, InstallState::StartupDiskFailed);
    // A newer Sekka Installer, with a newer engine, replaces the helper.
    let w = w.restarted(Script::Happy);
    *w.fake().bundled.lock().unwrap() = Digest::of(b"a newer engine");
    // The startup disk is retried with the engine that made the
    // installation, still in the helper's cache.
    let Response::Events { state, .. } = w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)) else { panic!() };
    assert_eq!(state, InstallState::WaitingForRecovery);
    // And the Mac is inspected with it.
    *w.fake().inspection.lock().unwrap() = installed(&w.plan, Some(id), InstallationState::Complete);
    let cache = w.fake().unpacked.lock().unwrap().clone();
    w.fake().unpacked.lock().unwrap().clear();
    let (code, _) = refused(w.ask(Request::Reconcile { installation: id }, None));
    assert_eq!(code, HelperRefusalCode::Download, "without its engine, nothing is looked at");
    *w.fake().unpacked.lock().unwrap() = cache;
    assert_eq!(w.reconcile(id).0, InstallState::WaitingForSetup);
    assert_eq!(w.fake().inspected_with.lock().unwrap().last(), Some(&engine_digest()));
}
