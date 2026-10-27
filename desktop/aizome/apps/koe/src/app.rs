//! Koe, the program: one window of two pages. Dictation sets dictation up
//! without a terminal (a speech model, downloaded only when asked; the
//! switch that runs the service, now and at each login; how it types; the
//! shortcuts) and shows what the service is doing. Vocabulary and Teaching
//! is `vocab`'s.
use crate::{
    config::{self, Backend, Config},
    ipc,
    models::{self, Installed, Model},
    protocol::{Action, Request, Response, Status},
    service::{self, Service},
    vocab::{self, Vocabulary},
    vocabulary::Store,
};
use aizome_kit::{MsgSender, prelude::*};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const APP_ID: &str = "org.aizome.Koe";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    Dictation,
    Vocabulary,
}

impl Page {
    pub const ALL: [Page; 2] = [Page::Dictation, Page::Vocabulary];

    pub fn name(self) -> &'static str {
        match self {
            Page::Dictation => "Dictation",
            Page::Vocabulary => "Vocabulary",
        }
    }

    /// As `--page` takes it.
    pub fn arg(self) -> &'static str {
        match self {
            Page::Dictation => "dictation",
            Page::Vocabulary => "vocabulary",
        }
    }

    pub fn parse(s: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.arg() == s)
    }
}

/// Where Koe keeps things, and what it may touch.
#[derive(Clone, Debug)]
pub struct Paths {
    /// koe.toml.
    pub config: PathBuf,
    /// The service's control socket.
    pub socket: PathBuf,
    pub models: PathBuf,
    /// The vocabulary store's folder.
    pub vocabulary: PathBuf,
    /// Aizome's settings.toml, which has the shortcuts.
    pub settings: PathBuf,
    /// Where models are downloaded from (`models::source`).
    pub model_source: String,
    /// Tell the compositor when settings.toml changes, and open Settings
    /// (not in tests).
    pub desktop: bool,
}

impl Paths {
    pub fn user(config: PathBuf, socket: PathBuf) -> anyhow::Result<Paths> {
        Ok(Paths {
            config,
            socket,
            models: models::directory()?,
            vocabulary: config::data_dir()?.join("vocabulary"),
            settings: aizome_config::Settings::path(),
            model_source: models::source(),
            desktop: true,
        })
    }
}

/// A download under way.
pub struct Download {
    pub model: Model,
    pub done: u64,
    cancel: Arc<AtomicBool>,
    /// Use it and turn dictation on when it's done (setting up).
    then_enable: bool,
}

impl Download {
    /// Whether dictation turns on when it's done.
    pub fn setting_up(&self) -> bool {
        self.then_enable
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Page(Page),
    /// Another launch, with the page it asked for.
    Raise(Option<Page>),
    Tick,
    /// The service's status as it changes (`None`: it isn't running).
    Status(Option<Status>),
    Enable(bool),
    /// The switch went (or didn't), and anything to say about shortcuts.
    Switched(bool, Result<Option<String>, String>),
    /// Download the recommended model and turn dictation on.
    SetUp,
    Download(String),
    Progress(String, u64),
    Downloaded(String, Result<PathBuf, String>),
    CancelDownload,
    UseModel(String),
    AskRemove(String),
    RemoveModel(String),
    Language(usize),
    Translate(bool),
    Gpu(bool),
    Output(usize),
    Silence(usize),
    AppendSpace(bool),
    Notify(bool),
    AddShortcuts,
    ShortcutsAdded(Result<Vec<String>, String>),
    /// Open a pane of Settings (`--pane`).
    OpenSettings(&'static str),
    /// The models' folder, in Hako.
    ShowModels,
    /// Clear the service's error.
    Dismiss,
    AskReset,
    Reset,
    /// Close, stopping a download.
    Close,
    Vocab(vocab::Msg),
}

/// The model the configuration and the catalog recommend.
pub const RECOMMENDED: &str = "base";

/// Languages offered by name (Whisper knows more; any code works in
/// koe.toml, and shows here as itself).
pub const LANGUAGES: [(&str, &str); 21] = [
    ("auto", "Detect automatically"),
    ("en", "English"),
    ("ja", "Japanese"),
    ("zh", "Chinese"),
    ("ko", "Korean"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("sv", "Swedish"),
    ("pl", "Polish"),
    ("cs", "Czech"),
    ("uk", "Ukrainian"),
    ("ru", "Russian"),
    ("tr", "Turkish"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("vi", "Vietnamese"),
    ("id", "Indonesian"),
];

/// Seconds of quiet that finish a dictation, as offered (0: never).
pub const SILENCES: [f32; 6] = [0.0, 1.0, 1.5, 2.0, 3.0, 5.0];

type Stamp = (std::time::SystemTime, u64, u64);

fn stamp(path: &Path) -> Option<Stamp> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len(), meta.ino()))
}

pub struct Koe {
    pub paths: Paths,
    service: Arc<dyn Service>,
    pub page: Page,
    pub config: Config,
    /// Why koe.toml can't be read, while it can't.
    pub config_error: Option<String>,
    config_stamp: Option<Stamp>,
    /// The service's, while it runs.
    pub status: Option<Status>,
    /// The models offered (the bundled catalog; tests give their own).
    pub catalog: Vec<Model>,
    pub installed: BTreeMap<String, Installed>,
    /// Free space where models go.
    pub free: Option<u64>,
    pub download: Option<Download>,
    /// Turning dictation on (true) or off.
    pub switching: Option<bool>,
    /// The tick the service was started at, until it's heard from (or
    /// five seconds pass).
    pub started: Option<u64>,
    /// settings.toml's [keys.bindings].
    pub bindings: BTreeMap<String, String>,
    /// The chords for starting or stopping, and for cancelling.
    pub shortcuts: (Vec<String>, Vec<String>),
    settings_stamp: Option<Stamp>,
    /// What the last action said, and whether it went wrong.
    pub message: Option<(String, bool)>,
    pub vocab: Vocabulary,
    pub window: WindowKey,
    ticks: u64,
    pub instance: Option<std::os::unix::net::UnixListener>,
    /// Closing once the download has stopped: its thread removes the
    /// partial file, and the program mustn't end first.
    closing: bool,
}

impl Koe {
    pub fn new(paths: Paths, service: Arc<dyn Service>) -> Koe {
        let vocab =
            Vocabulary::new(Store::new(paths.vocabulary.clone()), paths.socket.clone(), Arc::new(Config::default()));
        let mut koe = Koe {
            paths,
            service,
            page: Page::Dictation,
            config: Config::default(),
            config_error: None,
            config_stamp: None,
            status: None,
            catalog: models::catalog().models,
            installed: BTreeMap::new(),
            free: None,
            download: None,
            switching: None,
            started: None,
            bindings: BTreeMap::new(),
            shortcuts: (vec![], vec![]),
            settings_stamp: None,
            message: None,
            vocab,
            window: WindowKey::NONE,
            ticks: 0,
            instance: None,
            closing: false,
        };
        koe.read_config(true);
        koe.read_shortcuts(true);
        koe.read_models();
        koe
    }

    /// Offer these models instead of the bundled catalog's (tests).
    pub fn with_catalog(mut self, catalog: Vec<Model>) -> Koe {
        self.catalog = catalog;
        self.read_models();
        self
    }

    /// koe.toml again, if it changed (or `always`).
    fn read_config(&mut self, always: bool) {
        let now = stamp(&self.paths.config);
        if !always && now == self.config_stamp {
            return;
        }
        self.config_stamp = now;
        match Config::load(&self.paths.config) {
            Ok(mut config) => {
                // The usual model is in this window's folder (the same one,
                // but in tests).
                if config.engine.model == config::Engine::default().model {
                    config.engine.model = self.paths.models.join("ggml-base.bin");
                }
                self.config = config;
                self.config_error = None;
                self.vocab.set_config(Arc::new(self.config.clone()));
            }
            Err(error) => self.config_error = Some(format!("{error:#}")),
        }
    }

    /// The shortcuts in settings.toml again, if it changed (or `always`).
    fn read_shortcuts(&mut self, always: bool) {
        let now = stamp(&self.paths.settings);
        if !always && now == self.settings_stamp {
            return;
        }
        self.settings_stamp = now;
        if let Ok(settings) = aizome_config::Settings::load_from(&self.paths.settings) {
            self.bindings = settings.keys.bindings;
            self.shortcuts =
                (service::chords(&self.bindings, service::TOGGLE), service::chords(&self.bindings, service::CANCEL));
        }
    }

    fn read_models(&mut self) {
        self.installed =
            self.catalog.iter().map(|m| (m.name.clone(), models::installed(&self.paths.models, m))).collect();
        self.free = models::free_bytes(&self.paths.models);
    }

    pub fn model_path(&self, model: &Model) -> PathBuf {
        self.paths.models.join(model.file_name())
    }

    /// The catalog's model called `name`.
    pub fn find(&self, name: &str) -> Option<Model> {
        self.catalog.iter().find(|m| m.name == name).cloned()
    }

    /// The catalog model koe.toml uses, if it's one.
    pub fn model_in_use(&self) -> Option<Model> {
        self.catalog.iter().find(|m| self.model_path(m) == self.config.engine.model).cloned()
    }

    /// Whether recognition has what it needs: a model, or a server.
    pub fn ready_to_recognise(&self) -> bool {
        match self.config.engine.backend {
            Backend::Http => true,
            Backend::Local => match self.model_in_use() {
                Some(m) => self.installed.get(&m.name) == Some(&Installed::Present),
                None => self.config.engine.model.is_file(),
            },
        }
    }

    /// Change the configuration and save it; the service takes it up
    /// within a second, between dictations. Whether it's saved (or needed
    /// no saving). Only the window's thread writes koe.toml (never a
    /// worker), so one change never undoes another.
    fn change(&mut self, edit: impl FnOnce(&mut Config)) -> bool {
        match self.try_change(edit) {
            Ok(()) => true,
            Err(error) => {
                self.say_error(error);
                false
            }
        }
    }

    fn try_change(&mut self, edit: impl FnOnce(&mut Config)) -> Result<(), String> {
        // What's been saved from elsewhere since the last look stays.
        self.read_config(false);
        if let Some(error) = &self.config_error {
            return Err(format!("Couldn't read Koe's settings: {error}"));
        }
        let mut config = self.config.clone();
        edit(&mut config);
        if config == self.config && self.paths.config.exists() {
            return Ok(());
        }
        config.save(&self.paths.config).map_err(|error| format!("Couldn't save Koe's settings: {error:#}"))?;
        self.config = config;
        self.config_stamp = stamp(&self.paths.config);
        self.vocab.set_config(Arc::new(self.config.clone()));
        Ok(())
    }

    fn say(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), false));
    }

    fn say_error(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), true));
    }

    fn switch(&mut self, on: bool, cx: &mut Cx<Msg>) {
        if self.switching.is_some() || (on && self.config_error.is_some()) {
            return;
        }
        self.message = None;
        // The choice is saved here, before any other setting can be; the
        // worker only starts or stops the service. A failed save mustn't
        // prevent stopping a running microphone.
        let saved = self.try_change(|c| c.enabled = on);
        if on && let Err(error) = &saved {
            self.say_error(error.clone());
            return;
        }
        self.switching = Some(on);
        let (paths, service) = (self.paths.clone(), self.service.clone());
        cx.spawn(move || {
            let result = (|| -> anyhow::Result<Option<String>> {
                // A settings file that can't be read keeps its shortcuts,
                // and dictation goes on regardless.
                let note = match on {
                    true => service::ensure_shortcuts(&paths.settings, paths.desktop)
                        .err()
                        .map(|e| format!("Dictation has no shortcut: {e:#}")),
                    false => saved.err().map(|error| {
                        format!(
                            "Dictation's service has stopped, but Koe couldn't save the off setting. \
                             It may start again at login. {error}"
                        )
                    }),
                };
                if on {
                    service.start()?;
                } else {
                    service.stop()?;
                }
                Ok(note)
            })();
            Msg::Switched(on, result.map_err(|e| format!("{e:#}")))
        });
    }

    fn start_download(&mut self, name: &str, then_enable: bool, cx: &mut Cx<Msg>) {
        if self.download.is_some() {
            return;
        }
        let Some(model) = self.find(name) else { return };
        if self.installed.get(name) == Some(&Installed::Damaged) {
            // Not the catalog's: it goes, and the real one comes.
            if let Err(error) = models::remove(&self.paths.models, &model) {
                self.say_error(format!("{error:#}"));
                return;
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.download = Some(Download { model: model.clone(), done: 0, cancel: cancel.clone(), then_enable });
        self.message = None;
        let (base, dir, tx) = (self.paths.model_source.clone(), self.paths.models.clone(), cx.stream());
        let title = model.title();
        // A thread of its own: a download can take minutes.
        let spawned = std::thread::Builder::new().name("koe-download".into()).spawn(move || {
            let name = model.name.clone();
            let mut last = Instant::now() - Duration::from_secs(1);
            // The window hears how it ended, however it ends: it may be
            // waiting for this to close.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(anyhow::Error::from)
                    .and_then(|runtime| {
                        runtime.block_on(models::download_from(&base, &model, &dir, &cancel, |done| {
                            if last.elapsed() >= Duration::from_millis(100) || done == model.bytes {
                                last = Instant::now();
                                if !tx.send(Msg::Progress(name.clone(), done)) {
                                    cancel.store(true, Ordering::Relaxed);
                                }
                            }
                        }))
                    })
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("the download stopped unexpectedly")));
            tx.send(Msg::Downloaded(name, result.map_err(|e| format!("{e:#}"))));
        });
        if let Err(error) = spawned {
            self.download = None;
            self.say_error(format!("{title} didn't download: {error}"));
        }
    }

    /// Close the window, once a teaching example is discarded.
    fn close(&mut self, cx: &mut Cx<Msg>) {
        if self.vocab.close(cx) {
            cx.close_window(self.window);
        }
    }

    /// Start another of Aizome's apps (`what` names it, if it can't start).
    fn run(&mut self, what: &str, program: &str, args: &[&str]) {
        if !self.paths.desktop {
            return;
        }
        match std::process::Command::new(program).args(args).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(error) => self.say_error(format!("Couldn't open {what}: {error}")),
        }
    }

    /// Ask the service, on a worker.
    fn tell_service(&self, action: Action, cx: &mut Cx<Msg>) {
        let socket = self.paths.socket.clone();
        cx.spawn(move || {
            let _ = ipc::request_blocking(&socket, &Request::new(action), Duration::from_secs(5));
            Msg::Tick
        });
    }
}

/// Follow the service's status: subscribe, and again half a second after
/// it goes away. `None` once each time it stops answering.
fn follow(socket: PathBuf, tx: MsgSender<Msg>) {
    let _ = std::thread::Builder::new().name("koe-status".into()).spawn(move || {
        use std::io::{BufRead, Write};
        let mut was_up = None;
        loop {
            if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&socket) {
                let mut line = serde_json::to_vec(&Request::new(Action::Subscribe)).unwrap_or_default();
                line.push(b'\n');
                if stream.write_all(&line).is_ok() {
                    for line in std::io::BufReader::new(stream).lines() {
                        let Ok(line) = line else { break };
                        let Ok(response) = serde_json::from_str::<Response>(&line) else { break };
                        if !response.ok {
                            break;
                        }
                        was_up = Some(true);
                        if !tx.send(Msg::Status(Some(response.status))) {
                            return;
                        }
                    }
                }
            }
            if was_up != Some(false) {
                was_up = Some(false);
                if !tx.send(Msg::Status(None)) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

impl Program for Koe {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Koe", "Dictation").icon(Icon::Mic).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(WindowSpec::new("Koe").size(900.0, 720.0).min_size(760.0, 560.0));
        self.vocab.window = Some(self.window);
        cx.every(Duration::from_millis(500), Msg::Tick);
        follow(self.paths.socket.clone(), cx.stream());
        self.vocab.start(cx);
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, cx.stream(), |args| {
                Msg::Raise(args.windows(2).find(|a| a[0] == "--page").and_then(|a| Page::parse(&a[1])))
            });
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Page(page) => self.page = page,
            Msg::Raise(page) => {
                if let Some(page) = page {
                    self.page = page;
                }
                cx.activate(self.window);
            }
            Msg::Tick => {
                self.ticks += 1;
                if self.started.is_some_and(|t| self.ticks > t + 10) {
                    self.started = None;
                }
                if self.ticks.is_multiple_of(2) {
                    self.read_config(false);
                    self.read_shortcuts(false);
                    self.read_models();
                }
                self.vocab.update(vocab::Msg::Tick, cx);
            }
            Msg::Status(status) => {
                if status.is_some() {
                    self.started = None;
                }
                self.status = status.clone();
                self.vocab.set_status(status);
            }
            Msg::Enable(on) => {
                if on && !self.ready_to_recognise() {
                    self.start_download(RECOMMENDED, true, cx);
                } else {
                    self.switch(on, cx);
                }
            }
            Msg::Switched(on, result) => {
                self.switching = None;
                self.read_config(true);
                self.read_shortcuts(true);
                match result {
                    Ok(note) => {
                        self.message = note.map(|n| (n, true));
                        if on && self.status.is_none() {
                            self.started = Some(self.ticks);
                        }
                        if !on {
                            // It no longer answers, whether or not its
                            // status stream has said so yet.
                            self.status = None;
                            self.vocab.set_status(None);
                            if self.message.is_none() {
                                self.say("Dictation is off. Its service has stopped and won't start at login.");
                            }
                        }
                    }
                    Err(error) => self.say_error(if on {
                        format!("Dictation didn't start: {error}")
                    } else {
                        format!("Dictation didn't stop: {error}")
                    }),
                }
            }
            Msg::SetUp => {
                let recommended = self.find(RECOMMENDED);
                match recommended {
                    Some(m) if self.installed.get(&m.name) == Some(&Installed::Present) => {
                        let path = self.model_path(&m);
                        self.change(|c| c.engine.model = path);
                        self.switch(true, cx);
                    }
                    _ => self.start_download(RECOMMENDED, true, cx),
                }
            }
            Msg::Download(name) => self.start_download(&name, false, cx),
            Msg::Progress(name, done) => {
                if let Some(d) = self.download.as_mut().filter(|d| d.model.name == name) {
                    d.done = done;
                }
            }
            Msg::Downloaded(name, result) => {
                let Some(download) = self.download.take_if(|d| d.model.name == name) else {
                    return;
                };
                self.read_models();
                if self.closing {
                    // Stopped (or just finished) as the window closes, its
                    // partial file gone: nothing more to do.
                    self.close(cx);
                    return;
                }
                match result {
                    Ok(path) => {
                        let title = download.model.title();
                        if download.then_enable || !self.ready_to_recognise() {
                            self.change(|c| c.engine.model = path);
                        }
                        if download.then_enable && self.config_error.is_none() {
                            self.switch(true, cx);
                        } else {
                            self.say(format!("{title} is downloaded."));
                        }
                    }
                    Err(_) if download.cancel.load(Ordering::Relaxed) => {
                        self.say(format!("Stopped downloading {}.", download.model.title()))
                    }
                    Err(error) => self.say_error(format!("{} didn't download: {error}", download.model.title())),
                }
            }
            Msg::CancelDownload => {
                if let Some(d) = &self.download {
                    d.cancel.store(true, Ordering::Relaxed);
                }
            }
            Msg::UseModel(name) => {
                if let Some(model) = self.find(&name) {
                    let path = self.model_path(&model);
                    self.change(|c| c.engine.model = path);
                }
            }
            Msg::AskRemove(name) => {
                if let Some(model) = self.find(&name) {
                    cx.alert(
                        self.window,
                        Alert::new(
                            format!("Remove {}?", model.title()),
                            format!(
                                "This frees {}. You can download it again whenever you like.",
                                models::size_text(model.bytes)
                            ),
                        )
                        .cancel_button("Cancel")
                        .destructive_button("Remove", Msg::RemoveModel(name)),
                    );
                }
            }
            Msg::RemoveModel(name) => {
                if let Some(model) = self.find(&name) {
                    if self.config.enabled && self.model_in_use().is_some_and(|m| m.name == name) {
                        return;
                    }
                    match models::remove(&self.paths.models, &model) {
                        Ok(()) => {
                            self.say(format!("Removed {}, freeing {}.", model.title(), models::size_text(model.bytes)))
                        }
                        Err(error) => self.say_error(format!("{error:#}")),
                    }
                    self.read_models();
                }
            }
            Msg::Language(i) => {
                let code = language_options(&self.config.engine.language).get(i).map(|(code, _)| code.clone());
                if let Some(code) = code {
                    self.change(|c| c.engine.language = code);
                }
            }
            Msg::Translate(on) => {
                self.change(|c| c.engine.translate = on);
            }
            Msg::Gpu(on) => {
                self.change(|c| c.engine.gpu = on);
            }
            Msg::Output(i) => {
                let mode = [config::OutputMode::Paste, config::OutputMode::Clipboard, config::OutputMode::None];
                if let Some(mode) = mode.get(i).copied() {
                    self.change(|c| c.output.mode = mode);
                }
            }
            Msg::Silence(i) => {
                if let Some(s) = silence_options(self.config.audio.silence_seconds).get(i).copied() {
                    self.change(|c| c.audio.silence_seconds = s);
                }
            }
            Msg::AppendSpace(on) => {
                self.change(|c| c.output.append_space = on);
            }
            Msg::Notify(on) => {
                self.change(|c| c.output.notify = on);
            }
            Msg::AddShortcuts => {
                let (settings, tell) = (self.paths.settings.clone(), self.paths.desktop);
                cx.spawn(move || {
                    Msg::ShortcutsAdded(service::ensure_shortcuts(&settings, tell).map_err(|e| format!("{e:#}")))
                });
            }
            Msg::ShortcutsAdded(result) => {
                self.read_shortcuts(true);
                if let Err(error) = result {
                    self.say_error(format!("Dictation has no shortcut: {error}"));
                }
            }
            Msg::OpenSettings(pane) => self.run("Settings", "aizome-settei", &["--pane", pane]),
            Msg::ShowModels => {
                let dir = self.paths.models.display().to_string();
                self.run("Files", "aizome-hako", &[&dir]);
            }
            Msg::Dismiss => self.tell_service(Action::Cancel, cx),
            Msg::AskReset => cx.alert(
                self.window,
                Alert::new(
                    "Start again with Koe's usual settings?",
                    format!(
                        "{} will be kept beside it as koe.toml.broken. Downloaded models and your vocabulary stay.",
                        self.paths.config.display()
                    ),
                )
                .cancel_button("Cancel")
                .destructive_button("Use Usual Settings", Msg::Reset),
            ),
            Msg::Reset => {
                let aside = self.paths.config.with_extension("toml.broken");
                let result = std::fs::rename(&self.paths.config, &aside)
                    .map_err(anyhow::Error::from)
                    .and_then(|()| Config::default().save(&self.paths.config));
                if let Err(error) = result {
                    self.say_error(format!("{error:#}"));
                }
                self.read_config(true);
                // The usual settings have dictation off, and the service
                // may still be running (or recording) with the old ones:
                // it stops, as the switch stops it, before the window
                // says it's off.
                if self.config_error.is_none() {
                    self.switch(false, cx);
                }
            }
            Msg::Close => match &self.download {
                // Closing when its thread has stopped and cleaned up.
                Some(d) => {
                    d.cancel.store(true, Ordering::Relaxed);
                    self.closing = true;
                }
                None => self.close(cx),
            },
            Msg::Vocab(msg) => self.vocab.update(msg, cx),
        }
    }

    fn close_requested(&mut self, _window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if self.closing {
            // It closes as soon as the download has stopped.
            return false;
        }
        if let Some(d) = &self.download {
            cx.alert(
                self.window,
                Alert::new(
                    format!("Stop downloading {}?", d.model.title()),
                    "What's downloaded so far is thrown away.",
                )
                .cancel_button("Keep Downloading")
                .destructive_button("Stop and Close", Msg::Close),
            );
            return false;
        }
        self.vocab.close(cx)
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        crate::dictation::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let role = |label: &str, role: MenuRole| MenuItem::role_item(label, role);
        // The app menu as standard, its Settings… the Dictation page.
        let app = Menu::new("Koe")
            .item(role("About Koe", MenuRole::About))
            .separator()
            .item(MenuItem::new("Settings…", Msg::Page(Page::Dictation)).shortcut(Shortcut::cmd(',')))
            .separator()
            .submenu(Menu::new("Services").item(role("No Services Apply", MenuRole::Services).enabled(false)))
            .separator()
            .item(role("Hide Koe", MenuRole::Hide).shortcut(Shortcut::cmd('H')))
            .item(role("Hide Others", MenuRole::HideOthers).shortcut(Shortcut::cmd('H').alt()))
            .separator()
            .item(role("Quit Koe", MenuRole::Quit).shortcut(Shortcut::cmd('Q')));
        let on = self.config.enabled;
        let dictation = Menu::new("Dictation")
            .item(
                MenuItem::new(if on { "Turn Dictation Off" } else { "Turn Dictation On" }, Msg::Enable(!on))
                    .enabled(self.switching.is_none() && self.config_error.is_none() && self.download.is_none()),
            )
            .separator()
            .item(
                MenuItem::new("Keyboard Shortcuts…", Msg::OpenSettings("keyboard.commands"))
                    .enabled(self.paths.desktop),
            )
            .item(MenuItem::new("Sound Settings…", Msg::OpenSettings("sound")).enabled(self.paths.desktop));
        let editable = self.page == Page::Vocabulary && self.vocab.editable();
        let v = |label: &str, msg: vocab::Msg| MenuItem::new(label, Msg::Vocab(msg)).enabled(editable);
        let vocabulary = Menu::new("Vocabulary")
            .item(v("New Pack", vocab::Msg::NewPack))
            .item(v("Import Pack…", vocab::Msg::Import).shortcut(Shortcut::cmd('O')))
            .item(v("Export Pack…", vocab::Msg::Export).shortcut(Shortcut::cmd('E').shift()))
            .item(v("Export Training Data…", vocab::Msg::ExportExamples))
            .separator()
            .item(v("Add Word", vocab::Msg::NewWord).shortcut(Shortcut::cmd('N')));
        let mut view = Menu::new("View");
        for (i, p) in Page::ALL.iter().enumerate() {
            let label = match p {
                Page::Dictation => "Dictation",
                Page::Vocabulary => "Vocabulary and Teaching",
            };
            view = view.item(
                MenuItem::new(label, Msg::Page(*p))
                    .shortcut(Shortcut::cmd(char::from(b'1' + i as u8)))
                    .checked(self.page == *p),
            );
        }
        MenuBar::standard(&self.info()).replace("Koe", app).insert(1, dictation).insert(2, vocabulary).with_menu(view)
    }
}

/// The languages offered: the usual ones, and the configured one if it's
/// another. (code, name).
pub fn language_options(current: &str) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> = LANGUAGES.iter().map(|(c, n)| ((*c).into(), (*n).into())).collect();
    if !LANGUAGES.iter().any(|(c, _)| *c == current) {
        options.push((current.into(), current.into()));
    }
    options
}

/// The pauses offered, and the configured one if it's another.
pub fn silence_options(current: f32) -> Vec<f32> {
    let mut options = SILENCES.to_vec();
    if !options.contains(&current) {
        options.push(current);
    }
    options
}
