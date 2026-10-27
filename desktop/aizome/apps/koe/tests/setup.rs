//! Setting dictation up in Koe's window, with no terminal: a model
//! downloaded only when asked (from a server on this computer, with a
//! catalog of the test's own), the switch, the shortcuts, settings saved
//! as they change, and a model removed. The service is a recorder: no
//! daemon runs, and nothing in the session is touched.
use aizome_kit::testing::Harness;
use koe::{
    app::{Koe, Msg, Paths},
    config::{Backend, Config},
    models::Model,
    protocol::{Phase, Status},
    service::{self, Service},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// What the window asked of the service, once it's done.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<&'static str>>,
    held: AtomicBool,
}
impl Recorder {
    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
    /// Starting and stopping take until `release`.
    fn hold(&self) {
        self.held.store(true, Ordering::Relaxed);
    }
    fn release(&self) {
        self.held.store(false, Ordering::Relaxed);
    }
    fn call(&self, what: &'static str) -> anyhow::Result<()> {
        while self.held.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.calls.lock().unwrap().push(what);
        Ok(())
    }
}
impl Service for Recorder {
    fn start(&self) -> anyhow::Result<()> {
        self.call("start")
    }
    fn stop(&self) -> anyhow::Result<()> {
        self.call("stop")
    }
}

/// Serve `bytes` to every request, `pause` between 64 KiB chunks (a
/// minute: a server that stops sending).
fn model_server(bytes: Arc<Vec<u8>>, pause: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let bytes = bytes.clone();
            std::thread::spawn(move || {
                let (mut buf, mut request) = ([0u8; 4096], Vec::new());
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len());
                if s.write_all(head.as_bytes()).is_err() {
                    return;
                }
                for chunk in bytes.chunks(64 * 1024) {
                    if s.write_all(chunk).is_err() {
                        return;
                    }
                    std::thread::sleep(pause);
                }
            });
        }
    });
    format!("http://{addr}")
}

struct Rig {
    dir: tempfile::TempDir,
    h: Harness<Koe>,
    service: Arc<Recorder>,
    bytes: Arc<Vec<u8>>,
}

impl Rig {
    fn new(pause: Duration) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let bytes: Arc<Vec<u8>> = Arc::new((0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect());
        let model =
            Model { name: "base".into(), sha256: format!("{:x}", Sha256::digest(&*bytes)), bytes: bytes.len() as u64 };
        let paths = Paths {
            config: dir.path().join("koe.toml"),
            socket: dir.path().join("runtime/control.sock"),
            models: dir.path().join("models"),
            vocabulary: dir.path().join("vocabulary"),
            settings: dir.path().join("settings.toml"),
            model_source: model_server(bytes.clone(), pause),
            desktop: false,
        };
        let service = Arc::new(Recorder::default());
        let app = Koe::new(paths, service.clone()).with_catalog(vec![model]);
        Rig { dir, h: Harness::new(app), service, bytes }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn config(&self) -> Config {
        Config::load(&self.path("koe.toml")).unwrap()
    }
    fn has(&mut self, text: &str) -> bool {
        let w = self.h.window();
        self.h.texts(w).iter().any(|t| t.contains(text))
    }
    /// Run the window's timers and workers until `done`.
    fn settle(&mut self, what: &str, mut done: impl FnMut(&mut Rig) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !done(self) {
            if Instant::now() > deadline {
                let w = self.h.window();
                panic!("never {what}: {:?}", self.h.texts(w));
            }
            self.h.advance(Duration::from_millis(50));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    /// Click the last control labelled `label`: an alert's, over the page.
    fn click_last(&mut self, label: &str) -> bool {
        let w = self.h.window();
        let found = self
            .h
            .semantics(w)
            .into_iter()
            .filter(|(s, _)| s.label.as_deref() == Some(label))
            .map(|(_, r)| r)
            .next_back();
        found.map(|r| self.h.click(w, r.center())).is_some()
    }
    fn click_id(&mut self, id: &str) {
        let w = self.h.window();
        let r = self.h.find_id(w, id).unwrap_or_else(|| panic!("no {id}"));
        self.h.click(w, r.center());
    }
    /// Run the window's timers and workers for `d`.
    fn wait(&mut self, d: Duration) {
        let until = Instant::now() + d;
        while Instant::now() < until {
            self.h.advance(Duration::from_millis(50));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    /// The partial and whole models in the models' folder.
    fn models(&self) -> Vec<PathBuf> {
        fs::read_dir(self.path("models")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default()
    }
}

#[test]
fn setting_up_downloads_only_when_asked_then_turns_dictation_on() {
    let mut r = Rig::new(Duration::from_millis(2));
    r.settle("off", |r| r.has("Dictation is off"));
    assert!(!r.path("models").exists(), "nothing downloads before it's asked for");
    assert!(!r.path("koe.toml").exists());

    r.click_id("set-up");
    r.settle("on", |r| r.service.calls() == ["start"]);
    let config = r.config();
    assert!(config.enabled);
    assert_eq!(config.engine.model, r.path("models/ggml-base.bin"));
    assert_eq!(fs::read(&config.engine.model).unwrap(), *r.bytes);
    assert_eq!(fs::read_dir(r.path("models")).unwrap().count(), 1, "a partial download was left behind");
    let settings = aizome_config::Settings::load_from(&r.path("settings.toml")).unwrap();
    assert_eq!(service::chords(&settings.keys.bindings, service::TOGGLE), ["Super+Alt+D"]);
    assert_eq!(service::chords(&settings.keys.bindings, service::CANCEL), ["Super+Alt+Escape"]);
    // The recorder runs no daemon, and the window says so.
    r.settle("told the service isn't running", |r| r.has("its service isn't running"));
    assert!(r.has("In use"));

    // Off again: the service stops, and won't start at login.
    r.click_id("dictation-switch");
    r.settle("off", |r| r.service.calls() == ["start", "stop"]);
    assert!(!r.config().enabled);
    r.settle("told it's off", |r| r.has("won't start at login"));

    // The model can go, freeing its space, with dictation off.
    assert!(r.click_last("Remove…"));
    assert!(r.has("Remove Base?"));
    assert!(r.click_last("Remove"));
    r.settle("removed", |r| !r.path("models/ggml-base.bin").exists());
    r.settle("offered again", |r| r.has("Download a Model and Turn On"));
}

#[test]
fn a_cancelled_download_leaves_nothing_behind() {
    // Slow enough to cancel: about a second for the whole model.
    let mut r = Rig::new(Duration::from_millis(20));
    r.settle("off", |r| r.has("Dictation is off"));
    r.click_id("download:base");
    r.settle("downloading", |r| r.has("Cancel"));
    r.h.send(Msg::CancelDownload);
    r.settle("stopped", |r| r.has("Stopped downloading Base"));
    assert!(r.models().is_empty(), "{:?}", r.models());
    assert!(r.service.calls().is_empty());
    assert!(!r.path("koe.toml").exists(), "a download alone changes no setting");
}

#[test]
fn cancelling_doesnt_wait_for_a_server_that_stopped_sending() {
    let mut r = Rig::new(Duration::from_secs(60));
    r.settle("off", |r| r.has("Dictation is off"));
    r.click_id("download:base");
    // Its partial file: the server has answered, and then stopped.
    r.settle("partly downloaded", |r| r.models().len() == 1);
    r.h.send(Msg::CancelDownload);
    // Long before the read times out, a minute on.
    r.settle("stopped", |r| r.has("Stopped downloading Base"));
    assert!(r.models().is_empty(), "{:?}", r.models());
}

#[test]
fn closing_during_a_download_waits_until_its_partial_file_is_gone() {
    let mut r = Rig::new(Duration::from_secs(60));
    r.settle("off", |r| r.has("Dictation is off"));
    r.click_id("download:base");
    // Its partial file: the server has answered, and then stopped.
    r.settle("partly downloaded", |r| r.models().len() == 1);
    let w = r.h.window();
    r.h.request_close(w);
    assert!(r.has("Stop downloading Base?"));
    assert!(r.click_last("Stop and Close"));
    // The program ends when its window closes, and would take the
    // download's thread with it before the partial file was removed.
    r.settle("closed", |r| r.h.windows().is_empty());
    assert!(r.models().is_empty(), "{:?}", r.models());
}

#[test]
fn settings_changed_while_the_service_starts_or_stops_keep_the_switchs_choice() {
    let mut r = Rig::new(Duration::from_millis(2));
    fs::create_dir_all(r.path("models")).unwrap();
    fs::write(r.path("models/ggml-base.bin"), &*r.bytes).unwrap();
    r.settle("ready to turn on", |r| r.h.program().ready_to_recognise());
    for (on, language, code) in [(true, 2, "ja"), (false, 3, "zh")] {
        r.service.hold();
        r.h.send(Msg::Enable(on));
        r.settle("switching", |r| r.h.program().switching == Some(on));
        assert_eq!(r.config().enabled, on, "the choice is saved before the service is asked");
        // Another setting changes while the service starts (or stops).
        r.h.send(Msg::Language(language));
        r.service.release();
        r.settle("switched", |r| r.h.program().switching.is_none());
        let config = r.config();
        assert_eq!((config.enabled, config.engine.language.as_str()), (on, code));
        assert_eq!(r.h.program().config.enabled, on);
    }
    assert_eq!(r.service.calls(), ["start", "stop"]);
}

#[test]
fn a_service_running_with_dictation_off_says_so_and_stops() {
    let mut r = Rig::new(Duration::from_millis(2));
    r.settle("off", |r| r.has("Dictation is off"));
    // Heard from the service (a stop that failed, or koe.toml edited).
    r.wait(Duration::from_millis(200));
    r.h.send(Msg::Status(Some(Status::default())));
    assert!(r.has("But its service is still running"));
    r.click_id("stop-service");
    r.settle("stopped", |r| r.service.calls() == ["stop"]);
    r.settle("off", |r| r.has("won't start at login") && !r.has("still running"));
}

#[test]
fn turning_off_stops_recording_even_when_the_choice_cannot_be_saved() {
    let mut r = Rig::new(Duration::from_millis(2));
    let mut config = Config { enabled: true, ..Config::default() };
    config.engine.backend = Backend::Http;
    config.save(&r.path("koe.toml")).unwrap();
    r.settle("on", |r| r.h.program().config.enabled);
    // Block the atomic write, including when the tests run as root.
    fs::create_dir(r.path("koe.toml.tmp")).unwrap();
    r.h.send(Msg::Status(Some(Status { phase: Phase::Recording, ..Status::default() })));
    r.click_id("dictation-switch");
    r.settle("stopped", |r| r.service.calls() == ["stop"] && r.h.program().switching.is_none());
    assert!(r.h.program().status.is_none());
    assert!(r.config().enabled, "the saved choice could not change");
    assert!(r.has("service has stopped"));
    assert!(r.has("couldn't save") && r.has("may start again at login"));
    assert!(!r.has("won't start at login"));
}

#[test]
fn unreadable_settings_still_offer_to_stop_recording_without_replacing_them() {
    let mut r = Rig::new(Duration::from_millis(2));
    let broken = "[engine]\nlanguage = 3\n";
    fs::write(r.path("koe.toml"), broken).unwrap();
    r.settle("error shown", |r| r.has("Koe's settings can't be read"));
    r.h.send(Msg::Status(Some(Status { phase: Phase::Recording, ..Status::default() })));
    r.click_id("stop-service");
    r.settle("stopped", |r| r.service.calls() == ["stop"] && r.h.program().switching.is_none());
    assert!(r.h.program().status.is_none());
    assert_eq!(fs::read_to_string(r.path("koe.toml")).unwrap(), broken);
    assert!(r.has("service has stopped") && r.has("couldn't save"));
    assert!(!r.has("won't start at login"));
}

#[test]
fn turning_on_still_requires_saving_the_choice() {
    let mut r = Rig::new(Duration::from_millis(2));
    let mut config = Config::default();
    config.engine.backend = Backend::Http;
    config.save(&r.path("koe.toml")).unwrap();
    r.settle("ready", |r| r.h.program().ready_to_recognise());
    fs::create_dir(r.path("koe.toml.tmp")).unwrap();
    r.click_id("dictation-switch");
    r.settle("save error", |r| r.has("Couldn't save Koe's settings"));
    assert!(!r.config().enabled);
    assert!(r.h.program().switching.is_none());
    assert!(r.service.calls().is_empty());
}

#[test]
fn a_model_that_isnt_the_catalogs_is_downloaded_again() {
    let mut r = Rig::new(Duration::from_millis(2));
    fs::create_dir_all(r.path("models")).unwrap();
    fs::write(r.path("models/ggml-base.bin"), b"not a model").unwrap();
    r.settle("shown as incomplete", |r| r.has("Incomplete"));
    assert!(r.click_last("Download Again"));
    r.settle("downloaded", |r| r.has("Base is downloaded"));
    assert_eq!(fs::read(r.path("models/ggml-base.bin")).unwrap(), *r.bytes);
    // The model the settings already name, now whole: nothing else changes,
    // and dictation stays off.
    assert_eq!(r.h.program().config.engine.model, r.path("models/ggml-base.bin"));
    assert!(r.h.program().ready_to_recognise());
    assert!(!r.path("koe.toml").exists());
    assert!(r.service.calls().is_empty());
}

#[test]
fn settings_are_saved_as_they_change() {
    let mut r = Rig::new(Duration::from_millis(2));
    r.settle("off", |r| r.has("Dictation is off"));
    r.h.send(Msg::Language(2));
    r.h.send(Msg::Translate(true));
    r.h.send(Msg::Output(1));
    r.h.send(Msg::Silence(3));
    r.h.send(Msg::AppendSpace(true));
    r.h.send(Msg::Notify(false));
    let config = r.config();
    assert_eq!(config.engine.language, "ja");
    assert!(config.engine.translate);
    assert_eq!(config.output.mode, koe::config::OutputMode::Clipboard);
    assert_eq!(config.audio.silence_seconds, 2.0);
    assert!(config.output.append_space && !config.output.notify);
    assert!(!config.enabled);
    // Readable only by its owner: it can name a server's credential variable.
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(r.path("koe.toml")).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0);
    // A change made elsewhere (aizome-koe, an agent) shows.
    let mut changed = r.config();
    changed.engine.language = "fr".into();
    std::thread::sleep(Duration::from_millis(20));
    changed.save(&r.path("koe.toml")).unwrap();
    r.settle("French", |r| r.h.program().config.engine.language == "fr");
}

#[test]
fn unreadable_settings_say_why_and_start_again_when_asked() {
    let mut r = Rig::new(Duration::from_millis(2));
    fs::write(r.path("koe.toml"), "[engine]\nlanguage = 3\n").unwrap();
    r.settle("error shown", |r| r.has("Koe's settings can't be read"));
    // Nothing else may overwrite it meanwhile.
    r.h.send(Msg::Translate(true));
    assert_eq!(fs::read_to_string(r.path("koe.toml")).unwrap(), "[engine]\nlanguage = 3\n");
    assert!(r.click_last("Use Usual Settings…"));
    // The service may be running, recording even, with the old settings.
    r.service.hold();
    assert!(r.click_last("Use Usual Settings"));
    r.settle("readable", |r| r.h.program().config_error.is_none());
    assert_eq!(r.config(), Config::default());
    assert_eq!(fs::read_to_string(r.path("koe.toml.broken")).unwrap(), "[engine]\nlanguage = 3\n");
    // The usual settings have dictation off: the service stops, as the
    // switch stops it, before the window says so.
    r.wait(Duration::from_millis(500));
    assert!(r.has("Turning dictation off"));
    assert!(!r.has("Dictation is off"));
    r.service.release();
    r.settle("off", |r| r.has("Dictation is off"));
    assert_eq!(r.service.calls(), ["stop"]);
}
