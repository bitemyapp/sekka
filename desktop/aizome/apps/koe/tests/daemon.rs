use koe::{
    app::{Koe, Page, Paths},
    config::{Backend, Config, OutputMode},
    ipc,
    protocol::{Action, Phase, Request, Response, VocabularyRequest as V},
    service::Service,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

/// A service the window can't start or stop: the harness's daemon.
struct Unmanaged;
impl Service for Unmanaged {
    fn start(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener, UnixStream},
};

struct Harness {
    _dir: tempfile::TempDir,
    child: Child,
    socket: PathBuf,
    config: PathBuf,
    bin: PathBuf,
    clipboard: PathBuf,
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn script(path: &Path, body: &str) {
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("sh"))
        .find(|p| p.is_file())
        .unwrap();
    fs::write(path, format!("#!{}\n{body}\n", shell.display())).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
impl Harness {
    async fn start(endpoint: String, desktop: Option<&Path>) -> Self {
        Self::configured(endpoint, desktop, |_| {}).await
    }
    async fn configured(endpoint: String, desktop: Option<&Path>, configure: impl FnOnce(&mut Config)) -> Self {
        Self::with_shell(endpoint, desktop, None, configure).await
    }
    async fn with_shell(
        endpoint: String,
        desktop: Option<&Path>,
        shell: Option<&Path>,
        configure: impl FnOnce(&mut Config),
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let raw = dir.path().join("samples.raw");
        let samples: Vec<u8> =
            (0..16000).flat_map(|i| if i % 2 == 0 { 5000_i16 } else { -5000_i16 }.to_ne_bytes()).collect();
        fs::write(&raw, samples).unwrap();
        script(&bin.join("pw-record"), "cat \"$KOE_TEST_PCM\"\nexec sleep 60");
        script(&bin.join("wl-copy"), "cat > \"$KOE_TEST_CLIPBOARD\"");
        script(&bin.join("pw-play"), "cat > \"$KOE_TEST_PLAYBACK\"");
        let clipboard = dir.path().join("clipboard.txt");
        let config_path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.engine.backend = Backend::Http;
        config.tray.enabled = false;
        config.engine.http.endpoint = endpoint;
        config.output.mode = if desktop.is_some() { OutputMode::Paste } else { OutputMode::None };
        config.output.notify = false;
        config.output.paste_delay_ms = 10;
        config.replacements.push(koe::config::Replacement { from: "seka".into(), to: "Sekka".into() });
        configure(&mut config);
        fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
        let socket = dir.path().join("runtime/control.sock");
        let mut command = Command::new(env!("CARGO_BIN_EXE_aizome-koe"));
        command
            .args(["--config"])
            .arg(&config_path)
            .arg("--socket")
            .arg(&socket)
            .arg("daemon")
            .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
            .env("KOE_TEST_PCM", raw)
            .env("KOE_TEST_CLIPBOARD", &clipboard)
            .env("KOE_TEST_PLAYBACK", dir.path().join("played.raw"))
            .env("XDG_DATA_HOME", dir.path().join("data"))
            .env_remove("AIZOME_SOCKET")
            .env_remove("AIZOME_SHELL_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(socket) = desktop {
            command.env("AIZOME_SOCKET", socket);
        }
        if let Some(socket) = shell {
            command.env("AIZOME_SHELL_SOCKET", socket);
        }
        let child = command.spawn().unwrap();
        let harness = Self { _dir: dir, child, socket, config: config_path, bin, clipboard };
        tokio::time::timeout(Duration::from_secs(5), async {
            while !harness.socket.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        harness
    }
    async fn call(&self, action: Action) -> Response {
        ipc::request(&self.socket, &Request::new(action)).await.unwrap()
    }
    async fn vocabulary(&self, command: V) -> Response {
        ipc::request(&self.socket, &Request::vocabulary(command)).await.unwrap()
    }
    fn vocabulary_root(&self) -> PathBuf {
        self._dir.path().join("data/aizome/koe/vocabulary")
    }
    /// Koe's window on this daemon's data, on `page`. Its switch changes
    /// nothing: the harness runs the daemon.
    fn window(&self, page: Page) -> aizome_kit::testing::Harness<Koe> {
        self.window_with(page, Arc::new(Unmanaged))
    }
    /// Koe's window, starting and stopping the daemon with `service`.
    fn window_with(&self, page: Page, service: Arc<dyn Service>) -> aizome_kit::testing::Harness<Koe> {
        let data = self._dir.path().join("data/aizome/koe");
        let paths = Paths {
            config: self.config.clone(),
            socket: self.socket.clone(),
            models: data.join("models"),
            vocabulary: data.join("vocabulary"),
            settings: self._dir.path().join("settings.toml"),
            model_source: "http://127.0.0.1:9".into(),
            desktop: false,
        };
        let mut app = Koe::new(paths, service);
        app.page = page;
        aizome_kit::testing::Harness::new(app)
    }
    /// Run an `aizome-koe` command against this daemon's data, as a script would.
    fn koe(&self, args: &[&str]) -> serde_json::Value {
        let output = Command::new(env!("CARGO_BIN_EXE_aizome-koe"))
            .arg("--config")
            .arg(&self.config)
            .args(args)
            .env("XDG_DATA_HOME", self._dir.path().join("data"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    }
    async fn wait_for(&self, phase: Phase) -> Response {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let r = self.call(Action::Status).await;
                if r.status.phase == phase {
                    return r;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("never reached {phase:?}"))
    }
    async fn record_audio(&self) {
        assert!(self.call(Action::Start).await.ok);
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.call(Action::Status).await.status.elapsed_ms < 1000 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
}
async fn http_server(delay: Duration, response_body: &'static str) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![];
        let mut buffer = [0; 4096];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let len: usize =
                    header.lines().find_map(|l| l.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                if request.len() >= end + 4 + len {
                    break;
                }
            }
        }
        tokio::time::sleep(delay).await;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        request
    });
    (format!("http://{addr}/inference"), task)
}

#[tokio::test]
async fn teaching_keeps_audio_only_after_explicit_save_and_never_delivers() {
    let (endpoint, server) = http_server(Duration::ZERO, r#"{"text":"Open Isome settings."}"#).await;
    let h = Harness::configured(endpoint, None, |config| config.output.mode = OutputMode::Clipboard).await;
    let token = "teaching-owner".to_string();
    assert!(h.vocabulary(V::TeachStart { token: token.clone(), pack: "sekka".into(), word: "aizome".into() }).await.ok);
    assert!(!h.call(Action::Start).await.ok);
    assert!(!h.vocabulary(V::TeachCancel { token: "wrong-owner".into() }).await.ok);
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.call(Action::Status).await.status.elapsed_ms < 1000 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(h.vocabulary(V::TeachStop { token: token.clone() }).await.ok);
    assert!(h.wait_for(Phase::Idle).await.status.teaching);
    let reply = h.vocabulary(V::TeachPoll { token: token.clone() }).await;
    let preview = reply.vocabulary.unwrap().preview.unwrap();
    assert_eq!(preview.heard, "Open Isome settings.");
    assert_eq!(preview.corrected, "Open Aizome settings.");
    assert!(!h.call(Action::Last).await.ok);
    assert!(!h.call(Action::PasteLast).await.ok);
    assert!(!h.clipboard.exists());
    assert!(!h.vocabulary_root().join("examples").exists());
    let request = server.await.unwrap();
    assert!(String::from_utf8_lossy(&request).contains("Aizome, Koe, Sekka"));
    assert!(!h.vocabulary(V::TeachSave { token: token.clone(), transcript: String::new() }).await.ok);
    assert!(!h.vocabulary_root().join("examples").exists());
    assert!(h.vocabulary(V::TeachSave { token, transcript: "Open Aizome settings.".into() }).await.ok);
    assert!(!h.call(Action::Status).await.status.teaching);
    let store = koe::vocabulary::Store::new(h.vocabulary_root());
    let examples = store.list_examples("sekka", "aizome").unwrap();
    assert_eq!(examples.len(), 1);
    let data: serde_json::Value = serde_json::from_slice(
        &fs::read(h.vocabulary_root().join("examples").join(&examples[0].id).join("example.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(data["transcript"], "Open Aizome settings.");
    assert_eq!(data["model"], "http:whisper-1");
    assert_eq!(data["sample_rate"], 16000);
    assert!(!h.clipboard.exists());
    assert!(!h.call(Action::Last).await.ok);
}

#[tokio::test]
async fn abandoned_and_cancelled_teaching_never_saves_audio() {
    let h = Harness::start("http://127.0.0.1:9/inference".into(), None).await;
    let start = || V::TeachStart { token: "lease-owner".into(), pack: "sekka".into(), word: "aizome".into() };
    assert!(h.vocabulary(start()).await.ok);
    assert!(h.vocabulary(V::TeachCancel { token: "lease-owner".into() }).await.ok);
    assert_eq!(h.call(Action::Status).await.status.phase, Phase::Idle);
    assert!(!h.vocabulary_root().join("examples").exists());
    assert!(h.vocabulary(start()).await.ok);
    tokio::time::timeout(Duration::from_secs(11), async {
        while h.call(Action::Status).await.status.teaching {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(h.call(Action::Status).await.status.phase, Phase::Idle);
    assert!(!h.vocabulary_root().join("examples").exists());
    assert!(!h.vocabulary(V::TeachSave { token: "lease-owner".into(), transcript: "Never retained".into() }).await.ok);
}

#[tokio::test]
async fn edited_vocabulary_applies_to_next_dictation_without_restart() {
    let (endpoint, server) = http_server(Duration::ZERO, r#"{"text":"Open Aizoma settings."}"#).await;
    let h = Harness::start(endpoint, None).await;
    let mut library = h.vocabulary(V::Library).await.vocabulary.unwrap().library.unwrap();
    library.packs[1].words[0].aliases.push("Aizoma".into());
    assert!(h.vocabulary(V::SaveLibrary { library }).await.ok);
    h.record_audio().await;
    assert!(h.call(Action::Stop).await.ok);
    h.wait_for(Phase::Idle).await;
    assert_eq!(h.call(Action::Last).await.text.unwrap(), "Open Aizome settings.");
    assert!(!h.vocabulary_root().join("examples").exists());
    server.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_window_records_reviews_saves_and_exports() {
    use aizome_kit::{
        prelude::*,
        sidestep_ui::{Key, Modifiers},
        testing::Harness as Window,
    };
    use koe::{app::Msg, vocab};
    // Pump the real toolkit's worker replies without synthesizing host input.
    async fn settle(ui: &mut Window<Koe>, button: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                ui.advance(Duration::from_millis(20));
                ui.resize(ui.window(), Size::new(960.0, 740.0));
                if ui.inspected(ui.window()).iter().any(|node| {
                    node["role"] == "button"
                        && node["text"] == button
                        && node["disabled"] != true
                        && node["visible"] == true
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("button never became ready: {button}"));
    }
    let (endpoint, server) = http_server(Duration::ZERO, r#"{"text":"Open Aizoma settings."}"#).await;
    let h = Harness::start(endpoint, None).await;
    let mut ui = h.window(Page::Vocabulary);
    let window = ui.window();
    settle(&mut ui, "Record example").await;
    assert!(ui.texts(window).iter().any(|t| t.contains("next dictation")));
    if let Some(dir) = std::env::var_os("KOE_TEST_SCREENSHOTS") {
        ui.save_png(window, PathBuf::from(dir).join("koe-vocabulary.png"), 1.0);
    }
    assert!(ui.click_text(window, "Record example"));
    settle(&mut ui, "Stop recording").await;
    assert!(h.call(Action::Status).await.status.teaching);
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.call(Action::Status).await.status.elapsed_ms < 1000 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(ui.click_text(window, "Stop recording"));
    h.wait_for(Phase::Idle).await;
    settle(&mut ui, "Save example").await;
    assert!(ui.texts(window).iter().any(|t| t.contains("Review the complete transcript")));
    assert!(ui.texts(window).iter().any(|t| t == "Open Aizoma settings."));
    // Editing through the actual text widget preserves the complete utterance.
    let editor = ui.find_id(window, "correct-transcript").unwrap();
    ui.click(window, editor.center());
    ui.key(window, Key::Character("a".into()), Modifiers::SUPER);
    ui.type_text(window, "Open Aizome settings.");
    assert!(ui.click_text(window, "Add correction: Aizoma → Aizome"));
    settle(&mut ui, "Save example").await;
    let store = koe::vocabulary::Store::new(h.vocabulary_root());
    assert!(store.load().unwrap().word("sekka", "aizome").unwrap().aliases.contains(&"Aizoma".into()));
    assert!(!h.vocabulary_root().join("examples").exists());
    assert!(ui.click_text(window, "Play sample"));
    settle(&mut ui, "Save example").await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while fs::metadata(h._dir.path().join("played.raw")).map_or(0, |m| m.len()) != 32000 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    if let Some(dir) = std::env::var_os("KOE_TEST_SCREENSHOTS") {
        ui.save_png(window, PathBuf::from(dir).join("koe-teaching-review.png"), 1.0);
    }
    assert!(ui.click_text(window, "Save example"));
    settle(&mut ui, "Record example").await;
    assert!(!h.call(Action::Status).await.status.teaching);
    assert_eq!(store.list_examples("sekka", "aizome").unwrap().len(), 1);
    let path = h._dir.path().join("sekka.koe.json");
    ui.send(Msg::Vocab(vocab::Msg::ExportTo(Some(path.clone()))));
    settle(&mut ui, "Record example").await;
    assert!(path.exists());
    assert!(ui.texts(window).iter().any(|t| t.contains("No recordings are included")));
    assert!(!h.clipboard.exists());
    server.await.unwrap();
}
/// The vocabulary window once its library and the selected word's
/// examples have arrived. Moving the pointer where nothing reacts to it
/// delivers worker replies without other input.
async fn loaded_window(h: &Harness) -> aizome_kit::testing::Harness<Koe> {
    use aizome_kit::prelude::*;
    let mut ui = h.window(Page::Vocabulary);
    let window = ui.window();
    ui.resize(window, Size::new(960.0, 740.0));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !ui.inspected(window).iter().any(|node| node["text"] == "Record example" && node["disabled"] != true) {
            ui.move_to(window, Point::new(950.0, 735.0));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the vocabulary never loaded");
    tokio::time::sleep(Duration::from_millis(200)).await;
    ui.move_to(window, Point::new(950.0, 735.0));
    ui
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_window_polls_without_disabling_or_repainting() {
    use aizome_kit::prelude::*;
    let h = Harness::start("http://127.0.0.1:9/inference".into(), None).await;
    let mut ui = loaded_window(&h).await;
    let window = ui.window();
    let shown = ui.inspected(window);
    ui.take_damage(window);
    // Status every tick; the library and the word's examples every three seconds.
    for _ in 0..12 {
        ui.advance(Duration::from_millis(500));
        assert_eq!(ui.inspected(window), shown, "a read in flight changed the window");
        tokio::time::sleep(Duration::from_millis(100)).await;
        ui.move_to(window, Point::new(950.0, 735.0));
        assert_eq!(ui.inspected(window), shown, "an unchanged reply changed the window");
    }
    assert_eq!(ui.take_damage(window), vec![]);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_window_shows_command_line_changes_and_keeps_typing() {
    use aizome_kit::{
        prelude::*,
        sidestep_ui::{Key, Modifiers},
    };
    let h = Harness::start("http://127.0.0.1:9/inference".into(), None).await;
    let mut ui = loaded_window(&h).await;
    let window = ui.window();
    // An unsaved edit in progress…
    let field = ui.find_id(window, "pack-description").unwrap();
    ui.click(window, field.center());
    ui.key(window, Key::Character("a".into()), Modifiers::SUPER);
    ui.type_text(window, "Desktop names");
    // …while an agent adds a word.
    let word = h.koe(&["words", "add", "sekka", "Fusen", "--alias", "who sen"]);
    assert_eq!(word["id"], "fusen");
    for _ in 0..8 {
        ui.advance(Duration::from_millis(500));
        tokio::time::sleep(Duration::from_millis(100)).await;
        ui.move_to(window, Point::new(950.0, 735.0));
        if ui.find_id(window, "word:fusen").is_some() {
            break;
        }
    }
    assert!(ui.find_id(window, "word:fusen").is_some());
    assert!(ui.texts(window).iter().any(|t| t.contains("changed outside this window")));
    let description = ui.inspected(window).into_iter().find(|node| node["name"] == "pack-description").unwrap();
    assert_eq!(description["text"], "Desktop names");
    // Saving builds on the agent's change rather than failing as stale.
    assert!(ui.click_text(window, "Save pack"));
    let store = koe::vocabulary::Store::new(h.vocabulary_root());
    tokio::time::timeout(Duration::from_secs(5), async {
        while store.load().unwrap().packs[1].description != "Desktop names" {
            ui.move_to(window, Point::new(950.0, 735.0));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(store.load().unwrap().word("sekka", "fusen").is_ok());
}
#[tokio::test]
async fn complete_dictation_pause_resume_last_and_shutdown() {
    let (endpoint, server) = http_server(Duration::ZERO, r#"{"text":" hello seka 日本語 "}"#).await;
    let mut h = Harness::start(endpoint, None).await;
    let mut stream = UnixStream::connect(&h.socket).await.unwrap();
    ipc::write_json(&mut stream, &Request::new(Action::Subscribe)).await.unwrap();
    let mut events = BufReader::new(stream);
    assert!(ipc::read_line(&mut events).await.unwrap().is_some());
    h.record_audio().await;
    assert!(!h.call(Action::Start).await.ok);
    assert_eq!(h.call(Action::Pause).await.status.phase, Phase::Paused);
    assert_eq!(h.call(Action::Resume).await.status.phase, Phase::Recording);
    assert!(h.call(Action::Stop).await.ok);
    let done = h.wait_for(Phase::Idle).await;
    assert!(done.status.has_last);
    assert_eq!(h.call(Action::Last).await.text.unwrap(), "hello Sekka 日本語");
    let request = server.await.unwrap();
    assert!(request.windows(4).any(|w| w == b"RIFF"));
    assert!(h.call(Action::ClearLast).await.ok);
    assert!(!h.call(Action::Last).await.ok);
    assert!(h.call(Action::Shutdown).await.ok);
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.child.try_wait().unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!h.socket.exists());
}
#[tokio::test]
async fn cancellation_prevents_late_results() {
    let (endpoint, server) = http_server(Duration::from_millis(350), r#"{"text":"must never arrive"}"#).await;
    let h = Harness::start(endpoint, None).await;
    h.record_audio().await;
    h.call(Action::Stop).await;
    h.wait_for(Phase::Transcribing).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(h.call(Action::Cancel).await.status.phase, Phase::Idle);
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert!(!h.call(Action::Status).await.status.has_last);
    assert!(!h.call(Action::Last).await.ok);
    server.abort();
    h.record_audio().await;
    assert!(h.call(Action::Cancel).await.ok);
}
#[tokio::test]
async fn protocol_limits_single_instance_and_stale_socket() {
    let h = Harness::start("http://127.0.0.1:9/inference".into(), None).await;
    assert_eq!(fs::metadata(&h.socket).unwrap().permissions().mode() & 0o777, 0o600);
    let second = Command::new(env!("CARGO_BIN_EXE_aizome-koe"))
        .arg("--config")
        .arg(&h.config)
        .arg("--socket")
        .arg(&h.socket)
        .arg("daemon")
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));
    let response =
        ipc::request(&h.socket, &Request { action: Action::Status, version: 999, vocabulary: None }).await.unwrap();
    assert!(!response.ok);
    let mut bad = UnixStream::connect(&h.socket).await.unwrap();
    bad.write_all(&vec![b'x'; 65537]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(h.call(Action::Status).await.ok);
    assert!(!h.call(Action::Stop).await.ok);
    let status = h.call(Action::Status).await.status;
    assert_eq!(status.sequence, h.call(Action::Status).await.status.sequence);
}
#[tokio::test]
async fn microphone_failure_recovers_without_wedging() {
    let h = Harness::start("http://127.0.0.1:9/inference".into(), None).await;
    script(&h.bin.join("pw-record"), "exit 1");
    assert!(h.call(Action::Start).await.ok);
    let error = h.wait_for(Phase::Error).await;
    assert!(error.status.error.unwrap().contains("pw-record stopped"));
    assert_eq!(h.call(Action::Cancel).await.status.phase, Phase::Idle);
}
#[tokio::test]
async fn aizome_pastes_terminal_text_and_checks_focus() {
    for change_focus in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("compositor.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let calls = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let history = calls.clone();
        let compositor = tokio::spawn(async move {
            let mut focus_reads = 0;
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let line = ipc::read_line(&mut BufReader::new(&mut stream)).await.unwrap().unwrap();
                let value: serde_json::Value = serde_json::from_slice(&line).unwrap();
                history.lock().unwrap().push(value.clone());
                let response = match value["request"].as_str().unwrap() {
                    "focused-window" => {
                        focus_reads += 1;
                        serde_json::json!({"response":"window","window":{"id":if change_focus && focus_reads > 1 { 8 } else { 7 }, "app_id":"foot", "kit_app":false}})
                    }
                    "input-state" => {
                        serde_json::json!({"response":"input-state","state":{"pressed_keys":[]}})
                    }
                    "recovery-state" => {
                        serde_json::json!({"response":"recovery","state":{"locked":false}})
                    }
                    "synthesize" => serde_json::json!({"response":"ok"}),
                    other => panic!("unexpected request {other}"),
                };
                ipc::write_json(&mut stream, &response).await.unwrap();
            }
        });
        let (endpoint, _) = http_server(Duration::ZERO, r#"{"text":"dictated text"}"#).await;
        let h = Harness::start(endpoint, Some(&socket)).await;
        h.record_audio().await;
        h.call(Action::Stop).await;
        h.wait_for(if change_focus { Phase::Error } else { Phase::Idle }).await;
        let calls = calls.lock().unwrap();
        let paste = calls.iter().find(|v| v["request"] == "synthesize");
        if change_focus {
            assert!(paste.is_none());
            assert!(!h.clipboard.exists());
        } else {
            assert_eq!(paste.unwrap()["events"][0]["keys"], "ctrl+shift+v");
            assert_eq!(fs::read_to_string(&h.clipboard).unwrap(), "dictated text");
        }
        compositor.abort();
    }
}

#[tokio::test]
async fn held_shortcut_waits_for_release_and_remains_cancellable_and_focus_guarded() {
    #[derive(Default)]
    struct DesktopState {
        held: bool,
        locked: bool,
        changed_focus: bool,
        input_reads: usize,
        pastes: usize,
    }
    for outcome in ["release", "cancel", "focus", "lock", "timeout"] {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("compositor.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let state = std::sync::Arc::new(std::sync::Mutex::new(DesktopState { held: true, ..Default::default() }));
        let desktop = state.clone();
        let compositor = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                // Cancelling delivery can close a connection before a request
                // is written or its reply is read.
                let Some(line) = ipc::read_line(&mut BufReader::new(&mut stream)).await.unwrap() else {
                    continue;
                };
                let request: serde_json::Value = serde_json::from_slice(&line).unwrap();
                let response = {
                    let mut state = desktop.lock().unwrap();
                    match request["request"].as_str().unwrap() {
                        "focused-window" => serde_json::json!({"response":"window","window":{
                            "id":if state.changed_focus { 8 } else { 7 }, "app_id":"foot", "kit_app":false
                        }}),
                        "recovery-state" => {
                            serde_json::json!({"response":"recovery","state":{"locked":state.locked}})
                        }
                        "input-state" => {
                            state.input_reads += 1;
                            let keys = if state.held { vec![125, 56, 32] } else { vec![] };
                            serde_json::json!({"response":"input-state","state":{"pressed_keys":keys}})
                        }
                        "synthesize" => {
                            assert!(!state.held && !state.locked && !state.changed_focus);
                            assert_eq!(request["events"][0]["keys"], "ctrl+shift+v");
                            state.pastes += 1;
                            serde_json::json!({"response":"ok"})
                        }
                        other => panic!("unexpected request {other}"),
                    }
                };
                let _ = ipc::write_json(&mut stream, &response).await;
            }
        });
        let (endpoint, server) = http_server(Duration::ZERO, r#"{"text":"retained transcript"}"#).await;
        let h = Harness::start(endpoint, Some(&socket)).await;
        h.record_audio().await;
        assert!(h.call(Action::Stop).await.ok);
        h.wait_for(Phase::Delivering).await;
        // Recognition has finished, but the shortcut is still held.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let waiting = h.call(Action::Status).await.status;
        assert_eq!(waiting.phase, Phase::Delivering);
        assert!(waiting.error.is_none() && waiting.has_last);
        assert!(state.lock().unwrap().input_reads > 0);
        assert!(!h.clipboard.exists());
        assert_eq!(state.lock().unwrap().pastes, 0);
        match outcome {
            "release" => state.lock().unwrap().held = false,
            "cancel" => {
                assert_eq!(h.call(Action::Cancel).await.status.phase, Phase::Idle);
                state.lock().unwrap().held = false;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            "focus" => state.lock().unwrap().changed_focus = true,
            "lock" => state.lock().unwrap().locked = true,
            "timeout" => {
                tokio::time::timeout(Duration::from_secs(7), async {
                    while h.call(Action::Status).await.status.phase != Phase::Error {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                })
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        if matches!(outcome, "focus" | "lock" | "timeout") {
            let failed = h.wait_for(Phase::Error).await.status;
            assert!(failed.has_last);
            let error = failed.error.unwrap();
            assert!(error.contains(match outcome {
                "focus" => "focused window changed",
                "lock" => "desktop is locked",
                _ => "keyboard stayed pressed for 5 seconds",
            }));
            assert!(!h.clipboard.exists());
            assert_eq!(state.lock().unwrap().pastes, 0);
            {
                let mut state = state.lock().unwrap();
                state.changed_focus = false;
                state.locked = false;
            }
            // Retrying is asynchronous too: Cancel must interrupt a held-key wait.
            let retry = h.call(Action::PasteLast).await;
            assert!(retry.ok);
            assert_eq!(retry.status.phase, Phase::Delivering);
            assert!(retry.status.error.is_none());
            assert_eq!(h.call(Action::Cancel).await.status.phase, Phase::Idle);
            state.lock().unwrap().held = false;
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(!h.clipboard.exists());
            assert_eq!(state.lock().unwrap().pastes, 0);
            assert!(h.call(Action::PasteLast).await.ok);
        }
        let done = h.wait_for(Phase::Idle).await.status;
        assert!(done.error.is_none());
        if outcome == "cancel" {
            assert!(!h.clipboard.exists());
            assert_eq!(state.lock().unwrap().pastes, 0);
        } else {
            assert_eq!(fs::read_to_string(&h.clipboard).unwrap(), "retained transcript");
            assert_eq!(state.lock().unwrap().pastes, 1);
        }
        server.await.unwrap();
        compositor.abort();
    }
}

#[tokio::test]
async fn automatic_submission_on_duration_and_silence() {
    for silence in [false, true] {
        let (endpoint, server) = http_server(Duration::from_millis(50), r#"{"text":"automatic stop"}"#).await;
        let h = Harness::configured(endpoint, None, |c| {
            if silence {
                c.audio.silence_seconds = 0.2;
            } else {
                c.audio.max_seconds = 1;
            }
        })
        .await;
        if silence {
            let pcm: Vec<u8> = (0..16000)
                .flat_map(|i| {
                    let sample: i16 = if i < 4800 { 5000 } else { 0 };
                    sample.to_ne_bytes()
                })
                .collect();
            fs::write(h._dir.path().join("samples.raw"), pcm).unwrap();
        }
        assert!(h.call(Action::Start).await.ok);
        let done = h.wait_for(Phase::Idle).await;
        assert!(done.status.has_last);
        assert!(done.status.elapsed_ms <= 1000);
        assert_eq!(h.call(Action::Last).await.text.unwrap(), "automatic stop");
        server.await.unwrap();
    }
}

#[tokio::test]
async fn malformed_server_response_is_reported_without_echoing_private_text() {
    let (endpoint, server) = http_server(Duration::ZERO, r#"{"private":"secret speech not to log"}"#).await;
    let h = Harness::start(endpoint, None).await;
    h.record_audio().await;
    h.call(Action::Stop).await;
    let response = h.wait_for(Phase::Error).await;
    let error = response.status.error.unwrap();
    assert!(error.contains("text field"));
    assert!(!error.contains("secret speech"));
    assert!(h.call(Action::Cancel).await.ok);
    server.await.unwrap();
}

#[tokio::test]
async fn second_toggle_stops_capture_and_announces_transcription_before_result() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.sock");
    let listener = UnixListener::bind(&shell_path).unwrap();
    let (sent, mut notifications) = tokio::sync::mpsc::channel(16);
    let shell = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let line = ipc::read_line(&mut BufReader::new(&mut stream)).await.unwrap().unwrap();
            let value: serde_json::Value = serde_json::from_slice(&line).unwrap();
            sent.send(value).await.unwrap();
            ipc::write_json(&mut stream, &serde_json::json!({"type":"reply","ok":true})).await.unwrap();
        }
    });
    let (endpoint, server) = http_server(Duration::from_millis(700), r#"{"text":"private recognized words"}"#).await;
    let h = Harness::with_shell(endpoint, None, Some(&shell_path), |c| c.output.notify = true).await;
    assert_eq!(h.call(Action::Toggle).await.status.phase, Phase::Recording);
    let listening = tokio::time::timeout(Duration::from_secs(3), notifications.recv()).await.unwrap().unwrap();
    assert_eq!(listening["summary"], "Listening");
    tokio::time::sleep(Duration::from_millis(150)).await;
    let stopped = h.call(Action::Toggle).await;
    assert!(stopped.ok);
    assert_eq!(stopped.status.phase, Phase::Transcribing);
    let transcribing = tokio::time::timeout(Duration::from_secs(3), notifications.recv()).await.unwrap().unwrap();
    assert_eq!(transcribing["summary"], "Transcribing");
    assert!(transcribing["body"].as_str().unwrap().contains("Recording stopped"));
    let busy = h.call(Action::Toggle).await;
    assert!(!busy.ok);
    assert!(busy.error.unwrap().contains("recording has stopped"));
    let reminder = tokio::time::timeout(Duration::from_secs(3), notifications.recv()).await.unwrap().unwrap();
    assert_eq!(reminder["summary"], "Transcribing");
    assert!(reminder["body"].as_str().unwrap().contains("microphone is off"));
    h.wait_for(Phase::Idle).await;
    let complete = tokio::time::timeout(Duration::from_secs(3), notifications.recv()).await.unwrap().unwrap();
    assert_eq!(complete["summary"], "Dictation complete");
    for value in [listening, transcribing, reminder, complete] {
        assert!(!value.to_string().contains("private recognized words"));
    }
    server.await.unwrap();
    shell.abort();
}

#[tokio::test]
async fn saved_settings_apply_to_the_next_dictation_without_restart() {
    let (endpoint, server) = http_server(Duration::ZERO, r#"{"text":"Open seka settings"}"#).await;
    let h = Harness::start(endpoint, None).await;
    // As Koe's window saves them, while the daemon runs.
    let mut config = Config::load(&h.config).unwrap();
    config.replacements.push(koe::config::Replacement { from: "settings".into(), to: "preferences".into() });
    config.output.append_space = true;
    config.save(&h.config).unwrap();
    // It looks once a second, and takes them between dictations.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    h.record_audio().await;
    assert!(h.call(Action::Stop).await.ok);
    h.wait_for(Phase::Idle).await;
    assert_eq!(h.call(Action::Last).await.text.unwrap(), "Open Sekka preferences ");
    server.await.unwrap();
}

/// Asks a daemon the test started to quit, however the test ends.
struct Quits(PathBuf);
impl Drop for Quits {
    fn drop(&mut self) {
        let _ = ipc::request_blocking(&self.0, &Request::new(Action::Shutdown), Duration::from_secs(2));
    }
}

#[tokio::test]
async fn command_line_turns_dictation_on_and_off_without_a_unit() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    script(&bin.join("pw-record"), "exec sleep 60");
    let config_path = dir.path().join("koe.toml");
    let mut config = Config::default();
    config.engine.backend = Backend::Http;
    config.tray.enabled = false;
    config.output.mode = OutputMode::None;
    config.output.notify = false;
    config.save(&config_path).unwrap();
    let socket = dir.path().join("runtime/control.sock");
    let _quits = Quits(socket.clone());
    // Another configuration and socket than the session's: no unit, so it
    // runs a daemon of its own, which inherits this environment.
    let koe = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_aizome-koe"))
            .arg("--config")
            .arg(&config_path)
            .arg("--socket")
            .arg(&socket)
            .args(args)
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .env("XDG_DATA_HOME", dir.path().join("data"))
            .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
            .env_remove("AIZOME_SOCKET")
            .env_remove("AIZOME_SHELL_SOCKET")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let text = |out: &std::process::Output| String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!koe(&["enabled"]).status.success());
    let cancel = koe(&["record", "cancel"]);
    assert!(!cancel.status.success());
    assert!(String::from_utf8_lossy(&cancel.stderr).contains("dictation is off"));

    let on = koe(&["enable"]);
    assert!(on.status.success(), "{}", String::from_utf8_lossy(&on.stderr));
    assert!(text(&on).contains("⌥❖D"), "{}", text(&on));
    assert!(koe(&["enabled"]).status.success());
    let settings = aizome_config::Settings::load_from(&dir.path().join("config/aizome/settings.toml")).unwrap();
    assert_eq!(koe::service::chords(&settings.keys.bindings, koe::service::TOGGLE), ["Super+Alt+D"]);
    let status = koe(&["status"]);
    assert!(status.status.success());
    assert!(text(&status).contains(r#""phase":"idle""#));
    assert!(koe(&["record", "toggle"]).status.success());
    assert!(koe(&["record", "cancel"]).status.success());

    let off = koe(&["disable"]);
    assert!(off.status.success(), "{}", String::from_utf8_lossy(&off.stderr));
    assert!(!koe(&["enabled"]).status.success());
    assert!(ipc::request(&socket, &Request::new(Action::Status)).await.is_err());
    assert!(!Config::load(&config_path).unwrap().enabled);

    // Settings that can't be read are off at login; the running service
    // stops all the same, and the command says what it couldn't save.
    assert!(koe(&["enable"]).status.success());
    assert!(ipc::request(&socket, &Request::new(Action::Status)).await.is_ok());
    fs::write(&config_path, "[engine]\nlanguage = 3\n").unwrap();
    let off = koe(&["disable"]);
    assert!(!off.status.success());
    assert!(String::from_utf8_lossy(&off.stderr).contains("koe.toml"), "{}", String::from_utf8_lossy(&off.stderr));
    assert!(ipc::request(&socket, &Request::new(Action::Status)).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn starting_again_with_usual_settings_stops_a_recording_service_first() {
    let h = Harness::start("http://127.0.0.1:9/inference".into(), None).await;
    h.record_audio().await;
    // The settings stop being readable while it records.
    fs::write(&h.config, "[engine]\nlanguage = 3\n").unwrap();
    // This daemon is the window's own to stop, as one it started would be.
    let service = koe::service::Session { config: h.config.clone(), socket: h.socket.clone(), unit: false };
    let mut ui = h.window_with(Page::Dictation, Arc::new(service));
    let window = ui.window();
    let has = |ui: &aizome_kit::testing::Harness<Koe>, text: &str| ui.texts(window).iter().any(|t| t.contains(text));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !has(&ui, "Koe's settings can't be read") {
            ui.advance(Duration::from_millis(50));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the window never said the settings can't be read");
    assert_eq!(h.call(Action::Status).await.status.phase, Phase::Recording);
    ui.send(koe::app::Msg::Reset);
    tokio::time::timeout(Duration::from_secs(10), async {
        while !has(&ui, "won't start at login") {
            // Off, only once nothing is listening.
            if has(&ui, "Dictation is off") && !has(&ui, "still running") {
                assert!(ipc::request(&h.socket, &Request::new(Action::Status)).await.is_err());
            }
            ui.advance(Duration::from_millis(50));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the service never stopped");
    assert!(ipc::request(&h.socket, &Request::new(Action::Status)).await.is_err());
    assert!(!Config::load(&h.config).unwrap().enabled);
}
