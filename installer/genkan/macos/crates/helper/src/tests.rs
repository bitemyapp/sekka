// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper end to end, with macOS replaced by a fake platform and the
//! engine by a scripted thread that checks, at every acknowledgement, that
//! the journal on disk already holds the intent it acknowledges.
use super::*;
use crate::engine::EngineProcess;
use sekka_mac_contract::*;
use sekka_mac_journal::FsStorage;
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
}

struct Fake {
    dir: PathBuf,
    inspection: Mutex<Inspection>,
    script: Mutex<Script>,
    startup_disk_ok: AtomicBool,
    /// Every acknowledgement found its intent already in the journal.
    ordering_held: Arc<AtomicBool>,
    engine_runs: Arc<Mutex<u32>>,
}

impl Platform for Fake {
    fn now(&self) -> Timestamp {
        "2026-11-07T00:00:00Z".parse().unwrap()
    }
    fn engine(&self, expected: &Digest) -> Result<EngineInstall, PlatformError> {
        if *expected != engine_digest() {
            return Err(PlatformError::Engine("This installer carries a different engine.".into()));
        }
        Ok(EngineInstall {
            root: self.dir.join("engine"),
            digest: *expected,
        })
    }
    fn inspect(&self, _: &EngineInstall) -> Result<Inspection, PlatformError> {
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
        _installation: Uuid,
        plan: &Path,
        assets: &Path,
        owner: &str,
        password: Secret,
    ) -> Result<Box<dyn EngineProcess>, PlatformError> {
        assert_eq!(owner, "owner");
        assert_eq!(&password[..], PASSWORD);
        assert!(assets.join("payload.zip").exists());
        *self.engine_runs.lock().unwrap() += 1;
        let plan: Plan = canonical::from_slice(&std::fs::read(plan).unwrap(), limits::PLAN, PlanSchema::VALUE).unwrap();
        Ok(Box::new(ScriptedEngine::start(
            plan,
            *self.script.lock().unwrap(),
            self.dir.join("state").join("installations"),
            self.ordering_held.clone(),
        )))
    }
    fn set_startup_disk(&self, _: &[Fact], owner: &str, password: &[u8]) -> Result<Vec<Fact>, Failure> {
        assert_eq!((owner, password), ("owner", PASSWORD));
        if self.startup_disk_ok.load(Ordering::SeqCst) {
            Ok(vec![Fact::new("selected", "sekka").unwrap()])
        } else {
            Err(failure(DiskChanged::No))
        }
    }
}

fn failure(changed: DiskChanged) -> Failure {
    Failure {
        code: "apple-tool".parse().unwrap(),
        message: "Apple's tool failed.".parse().unwrap(),
        apple_diagnostic: Some("Error -69606".parse().unwrap()),
        disk_changed: changed,
    }
}

struct ScriptedEngine {
    lines: Receiver<Vec<u8>>,
    acks: Option<Sender<u64>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ScriptedEngine {
    fn start(plan: Plan, script: Script, journals: PathBuf, ordering: Arc<AtomicBool>) -> Self {
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
                mode: EngineMode::Execute,
                protocol: ENGINE_PROTOCOL,
            });
            for stage in macos_stages(shrinks) {
                if script == Script::Skip(stage) {
                    continue;
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
                    facts: vec![Fact::new("checked", stage.title()).unwrap()],
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
        sekka_mac_journal::Journal::open(FsStorage, journals, id)
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
}

fn engine_digest() -> Digest {
    Digest::of(b"engine archive")
}

struct World {
    _tmp: tempfile::TempDir,
    helper: Helper<Fake>,
    catalog: SignedCatalog,
    begin: Begin,
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

fn world(script: Script) -> World {
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
        place: Place::ShrinkContainer {
            container: "c54f285b-fe6e-4d0c-9bc6-567c62bd7c20".parse().unwrap(),
        },
        sekka: 100 * GIB,
    };
    let plan = derive_plan(&samples::inspection(), &catalog, &request, 0).unwrap();
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
    let fake = Fake {
        dir: tmp.path().into(),
        inspection: Mutex::new(samples::inspection()),
        script: Mutex::new(script),
        startup_disk_ok: AtomicBool::new(true),
        ordering_held: Arc::new(AtomicBool::new(true)),
        engine_runs: Arc::new(Mutex::new(0)),
    };
    let trust = sekka_mac_trust::TrustConfig {
        roots: vec![sekka_mac_trust::development_root()],
        hosts: sekka_mac_trust::PRODUCTION_HOSTS.to_vec(),
        clock_skew: 600,
    };
    let helper = Helper::new(fake, Paths::at(&tmp.path().join("state")), trust, "0.1.0".parse().unwrap()).unwrap();
    World {
        _tmp: tmp,
        helper,
        catalog: signed,
        begin,
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
    fn fake(&self) -> &Fake {
        self.helper.platform_for_tests()
    }
}

fn refused(response: Response) -> (HelperRefusalCode, String) {
    match response {
        Response::Refused(r) => (r.code, r.message.to_string()),
        other => panic!("expected a refusal, got {other:?}"),
    }
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
    // The imported downloads are the helper's own copies.
    let staged = w.helper.paths_for_tests().staging(id);
    assert!(staged.join("payload.zip").exists() && staged.join("plan.json").exists());
    // Diagnostics name no account.
    let Response::Diagnostics { lines } = w.ask(Request::Diagnostics { installation: id }, None) else { panic!() };
    assert!(lines.iter().all(|l| !l.as_str().contains("owner")), "{lines:?}");
    // And status lists it.
    let Response::Status { installations } = w.ask(Request::Status, None) else { panic!() };
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].state, InstallState::WaitingForRecovery);
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
    let staged = w.helper.paths_for_tests().staging(id);
    assert_eq!(std::fs::read(staged.join("installer_data.json")).unwrap(), b"{\"os_list\":[]}");
    assert!(!staged.join("recovery-copy.zip").exists());
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
    assert_eq!(*w.fake().engine_runs.lock().unwrap(), 1, "retrying never reruns the engine");
    let (code, _) = refused(w.ask(Request::RetryStartupDisk { installation: id }, Some(PASSWORD)));
    assert_eq!(code, HelperRefusalCode::State);
}
