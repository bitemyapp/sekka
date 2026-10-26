use crate::{
    audio::{self, Capture, Meter},
    config::{Config, OutputMode},
    desktop,
    engine::Recognizer,
    ipc::{self, SocketGuard},
    protocol::{
        Action, Phase, Request, Response, Status, TeachingPreview, VERSION, VocabularyReply, VocabularyRequest,
    },
    text::Processor,
    vocabulary::{self, Store},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    future::pending,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::BufReader,
    net::UnixStream,
    sync::{Semaphore, mpsc, oneshot, watch},
    task::JoinHandle,
};

type Command = (Request, oneshot::Sender<Response>, oneshot::Receiver<()>);
struct Teaching {
    token: String,
    pack: String,
    word: String,
    spelling: String,
    revision: u64,
    samples: Vec<f32>,
    preview: Option<TeachingPreview>,
    heartbeat: Instant,
}
struct State {
    config: Config,
    status: Status,
    updates: watch::Sender<Status>,
    recognizer: Recognizer,
    processor: Processor,
    capture: Option<Capture>,
    meter_rx: mpsc::Receiver<Meter>,
    samples: Vec<f32>,
    inference: Option<JoinHandle<Result<String>>>,
    delivery: Option<JoinHandle<Result<()>>>,
    cancelled: Arc<AtomicBool>,
    last: Option<String>,
    target: Option<u64>,
    reported_phase: Phase,
    transcription_started: Option<Instant>,
    store: Store,
    session_prompt: String,
    teaching: Option<Teaching>,
    playback: Option<JoinHandle<Result<()>>>,
}
impl State {
    fn publish(&mut self) {
        if self.status.phase != self.reported_phase {
            // State transitions only: no audio, recognized text or 10 Hz meter logs.
            let phase = serde_json::to_value(self.status.phase).expect("phase serializes");
            eprintln!("Koe: {}", phase.as_str().expect("phase is a string"));
            self.reported_phase = self.status.phase;
        }
        self.status.sequence += 1;
        self.status.has_last = self.last.is_some();
        self.status.model_loaded = self.recognizer.loaded();
        self.status.gpu = self.recognizer.on_gpu();
        self.status.teaching = self.teaching.is_some();
        self.updates.send_replace(self.status.clone());
    }
    fn fail(&mut self, error: anyhow::Error) {
        self.status.phase = Phase::Error;
        self.status.level = 0.0;
        let message = format!("{error:#}");
        eprintln!("Koe: {message}");
        self.status.error = Some(message.clone());
        self.samples.clear();
        if self.config.output.notify && self.teaching.is_none() {
            desktop::notify("Dictation needs attention", message);
        }
        self.publish();
    }
    fn begin_capture(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel(4);
        let mut audio = self.config.audio.clone();
        if self.teaching.is_some() {
            audio.max_seconds = audio.max_seconds.min(vocabulary::MAX_CLIP_SECONDS);
            audio.silence_seconds = 0.0;
        }
        let remaining = audio.max_seconds as usize * audio::RATE - self.samples.len();
        ensure!(remaining > 0, "recording has reached audio.max_seconds; stop to transcribe");
        self.capture = Some(Capture::start(audio, remaining, tx)?);
        self.meter_rx = rx;
        self.status.phase = Phase::Recording;
        self.status.error = None;
        Ok(())
    }
    async fn stop_capture(&mut self) -> Result<()> {
        if let Some(capture) = self.capture.take() {
            self.samples.extend(capture.finish().await?);
            self.status.elapsed_ms = (self.samples.len() * 1000 / audio::RATE) as u64;
        }
        self.status.level = 0.0;
        Ok(())
    }
    fn recognize(&mut self) -> Result<()> {
        ensure!(self.samples.len() * 1000 / audio::RATE >= self.config.audio.min_ms as usize, "recording is too short");
        ensure!(
            audio::has_signal(&self.samples, self.config.audio.silence_threshold),
            "no speech detected; check microphone level"
        );
        let samples = std::mem::take(&mut self.samples);
        if let Some(teaching) = &mut self.teaching {
            teaching.samples = samples.clone();
        }
        let recognizer = self.recognizer.with_prompt(self.session_prompt.clone());
        self.cancelled = Arc::new(AtomicBool::new(false));
        let cancel = self.cancelled.clone();
        self.transcription_started = Some(Instant::now());
        self.status.transcription_ms = None;
        self.inference = Some(tokio::spawn(async move { recognizer.transcribe(samples, cancel).await }));
        self.status.phase = Phase::Transcribing;
        if self.config.output.notify && self.teaching.is_none() {
            desktop::notify("Transcribing", "Recording stopped. Turning your speech into text…");
        }
        Ok(())
    }
    async fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(task) = self.inference.take() {
            task.abort();
        }
        if let Some(task) = self.delivery.take() {
            task.abort();
        }
        let _ = self.stop_capture().await;
        self.samples.clear();
        self.teaching = None;
        if let Some(task) = self.playback.take() {
            task.abort();
        }
        self.status.phase = Phase::Idle;
        self.status.error = None;
        self.status.elapsed_ms = 0;
        self.status.level = 0.0;
    }
    /// Nothing under way that new settings could change halfway through.
    fn idle(&self) -> bool {
        matches!(self.status.phase, Phase::Idle | Phase::Error)
            && self.teaching.is_none()
            && self.capture.is_none()
            && self.inference.is_none()
            && self.delivery.is_none()
    }
    /// Take settings saved since the daemon started. New recognition
    /// settings drop the resident model; the returned warmup loads the
    /// new one. The tray keeps its settings until the next start.
    fn reconfigure(&mut self, config: Config) -> Result<Option<JoinHandle<Result<()>>>> {
        self.processor = Processor::new(&config.replacements, config.output.append_space)?;
        let warmup = (config.engine != self.config.engine).then(|| {
            self.recognizer = Recognizer::new(config.engine.clone());
            let recognizer = self.recognizer.clone();
            tokio::spawn(async move { recognizer.warmup().await })
        });
        self.config = config;
        eprintln!("Koe: settings changed");
        Ok(warmup)
    }
    fn prepare_vocabulary(&mut self) -> Result<()> {
        let pipeline = self.store.load()?.pipeline(
            &self.config.engine.prompt,
            &self.config.replacements,
            self.config.output.append_space,
        )?;
        self.session_prompt = pipeline.prompt;
        self.processor = pipeline.processor;
        Ok(())
    }

    async fn vocabulary(&mut self, request: VocabularyRequest) -> Result<VocabularyReply> {
        use VocabularyRequest as V;
        let mut reply = VocabularyReply::default();
        if let V::TeachStart { token, pack, word } = request {
            vocabulary::valid_id(&token)?;
            ensure!(
                self.teaching.is_none() && matches!(self.status.phase, Phase::Idle | Phase::Error),
                "finish or cancel the current recording first"
            );
            let library = self.store.load()?;
            let spelling = library.word(&pack, &word)?.spelling.clone();
            self.prepare_vocabulary()?;
            self.samples.clear();
            self.status.elapsed_ms = 0;
            self.target = None;
            self.teaching = Some(Teaching {
                token,
                pack,
                word,
                spelling,
                revision: library.revision,
                samples: vec![],
                preview: None,
                heartbeat: Instant::now(),
            });
            if let Err(error) = self.begin_capture() {
                self.teaching = None;
                return Err(error);
            }
            return Ok(reply);
        }
        let token = match &request {
            V::TeachStop { token }
            | V::TeachPoll { token }
            | V::TeachCancel { token }
            | V::TeachSave { token, .. }
            | V::TeachPlay { token } => token,
            _ => bail!("not a teaching command"),
        };
        ensure!(
            self.teaching.as_ref().is_some_and(|t| t.token == *token),
            "teaching session ended; record a new example"
        );
        self.teaching.as_mut().unwrap().heartbeat = Instant::now();
        match request {
            V::TeachStop { .. } => {
                self.command(Action::Stop).await?;
            }
            V::TeachCancel { .. } => self.cancel().await,
            V::TeachPoll { .. } => reply.preview = self.teaching.as_ref().unwrap().preview.clone(),
            V::TeachSave { transcript, .. } => {
                ensure!(self.status.phase == Phase::Idle, "wait for recognition to finish");
                let teaching = self.teaching.as_ref().unwrap();
                let preview = teaching.preview.as_ref().context("record an example first")?;
                self.store.save_example(
                    vocabulary::Example {
                        version: 1,
                        id: vocabulary::new_id()?,
                        pack_id: teaching.pack.clone(),
                        word_id: teaching.word.clone(),
                        spelling: teaching.spelling.clone(),
                        transcript: transcript.trim().into(),
                        heard: preview.heard.clone(),
                        model: match self.config.engine.backend {
                            crate::config::Backend::Local => {
                                self.config.engine.model.file_name().unwrap_or_default().to_string_lossy().into_owned()
                            }
                            crate::config::Backend::Http => {
                                format!("http:{}", self.config.engine.http.model)
                            }
                        },
                        language: self.config.engine.language.clone(),
                        vocabulary_revision: teaching.revision,
                        created_ms: 0,
                        sample_rate: 0,
                        duration_ms: 0,
                        audio_sha256: String::new(),
                    },
                    &teaching.samples,
                )?;
                self.cancel().await;
                reply.message = Some("Example saved locally for future training.".into());
            }
            V::TeachPlay { .. } => {
                let teaching = self.teaching.as_ref().unwrap();
                ensure!(teaching.preview.is_some(), "wait for recognition to finish");
                if let Some(task) = self.playback.take() {
                    task.abort();
                }
                let samples = teaching.samples.clone();
                self.playback = Some(tokio::spawn(async move { audio::play(&samples).await }));
            }
            _ => unreachable!(),
        }
        Ok(reply)
    }
    async fn command(&mut self, action: Action) -> Result<Option<String>> {
        let action = if action == Action::Toggle {
            match self.status.phase {
                Phase::Recording | Phase::Paused => Action::Stop,
                Phase::Idle | Phase::Error => Action::Start,
                Phase::Transcribing => {
                    if self.config.output.notify {
                        desktop::notify(
                            "Transcribing",
                            "Your microphone is off. Wait for the result, or cancel dictation.",
                        );
                    }
                    bail!("recording has stopped; transcription is still running")
                }
                _ => bail!("dictation is busy; wait or cancel"),
            }
        } else {
            action
        };
        match action {
            Action::Start => {
                ensure!(self.teaching.is_none(), "save or discard the teaching example first");
                ensure!(matches!(self.status.phase, Phase::Idle | Phase::Error), "dictation is already active");
                self.prepare_vocabulary()?;
                self.samples.clear();
                self.status.elapsed_ms = 0;
                self.target =
                    if desktop::uses_aizome(&self.config.output) && self.config.output.mode == OutputMode::Paste {
                        let target = desktop::focused_window().await?;
                        ensure!(
                            !self.config.output.require_same_window || target.is_some(),
                            "focus the window to dictate into first"
                        );
                        target
                    } else {
                        None
                    };
                if self.config.engine.backend == crate::config::Backend::Local {
                    ensure!(self.config.engine.model.is_file(), "there's no speech model yet; download one in Koe");
                }
                self.begin_capture()?;
                if self.config.output.notify {
                    desktop::notify("Listening", "Speak, then press the dictation shortcut again to finish.");
                }
            }
            Action::Stop => {
                ensure!(matches!(self.status.phase, Phase::Recording | Phase::Paused), "no recording to stop");
                self.status.phase = Phase::Finishing;
                self.publish();
                if let Err(e) = self.stop_capture().await.and_then(|_| self.recognize()) {
                    let message = format!("{e:#}");
                    self.fail(e);
                    bail!("{message}");
                }
            }
            Action::Pause => {
                ensure!(self.status.phase == Phase::Recording, "not recording");
                if let Err(e) = self.stop_capture().await {
                    let message = format!("{e:#}");
                    self.fail(e);
                    bail!("{message}");
                } else {
                    self.status.phase = Phase::Paused;
                }
            }
            Action::Resume => {
                ensure!(self.status.phase == Phase::Paused, "not paused");
                self.begin_capture()?;
            }
            Action::Cancel | Action::Shutdown => self.cancel().await,
            Action::Status | Action::Subscribe => (),
            Action::Last => return Ok(Some(self.last.clone().context("no transcript in memory")?)),
            Action::CopyLast => desktop::copy(self.last.as_deref().context("no transcript in memory")?).await?,
            Action::PasteLast => {
                ensure!(
                    self.teaching.is_none() && matches!(self.status.phase, Phase::Idle | Phase::Error),
                    "finish or cancel dictation before pasting the last transcript"
                );
                let text = self.last.clone().context("no transcript in memory")?;
                let mut output = self.config.output.clone();
                output.mode = OutputMode::Paste;
                let target = if desktop::uses_aizome(&output) { desktop::focused_window().await? } else { None };
                self.status.phase = Phase::Delivering;
                self.status.error = None;
                self.delivery = Some(tokio::spawn(async move { desktop::deliver(&text, &output, target).await }));
            }
            Action::ClearLast => self.last = None,
            Action::Unload => {
                ensure!(
                    matches!(self.status.phase, Phase::Idle | Phase::Error),
                    "cancel or finish dictation before unloading"
                );
                self.recognizer.unload()?;
            }
            Action::Toggle => unreachable!(),
            Action::Vocabulary => bail!("vocabulary command is required"),
        }
        Ok(None)
    }
}
async fn serve(
    stream: UnixStream,
    tx: mpsc::Sender<Command>,
    mut updates: watch::Receiver<Status>,
    store: Store,
    config: Arc<Config>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let line =
        tokio::time::timeout(Duration::from_secs(3), ipc::read_line(&mut reader)).await??.context("empty request")?;
    let request: Request = serde_json::from_slice(&line)?;
    if request.version != VERSION {
        let mut response = Response::status(updates.borrow().clone());
        response.ok = false;
        response.error = Some(format!("unsupported protocol version; expected {VERSION}"));
        return ipc::write_json(&mut writer, &response).await;
    }
    ensure!((request.action == Action::Vocabulary) == request.vocabulary.is_some(), "invalid vocabulary request");
    if let Some(command) = request.vocabulary.as_ref().filter(|v| v.is_store_command()) {
        let command = command.clone();
        // Exporting recordings and editing packs must not block microphone controls.
        let result = tokio::task::spawn_blocking(move || store.dispatch(command, &config)).await?;
        let mut response = Response::status(updates.borrow().clone());
        match result {
            Ok(reply) => response.vocabulary = Some(reply),
            Err(error) => {
                response.ok = false;
                response.error = Some(format!("{error:#}"));
            }
        }
        return ipc::write_json(&mut writer, &response).await;
    }
    let subscribe = request.action == Action::Subscribe;
    let (reply, response) = oneshot::channel();
    let (flushed, flush) = oneshot::channel();
    tx.send((request, reply, flush)).await?;
    ipc::write_json(&mut writer, &response.await?).await?;
    let _ = flushed.send(());
    if subscribe {
        loop {
            tokio::select! {
                result = updates.changed() => {
                    result?;
                    let status = updates.borrow_and_update().clone();
                    ipc::write_json(&mut writer, &Response::status(status)).await?;
                }
                _ = ipc::read_line(&mut reader) => break,
            }
        }
    }
    Ok(())
}

/// What changes when the settings file is saved (by rename: a new inode).
fn stamp(path: &Path) -> Option<(std::time::SystemTime, u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len(), meta.ino()))
}

/// Serve on `path` until asked to quit. `settings` is the file `config`
/// came from: changes saved there apply once nothing is under way.
pub async fn run(config: Config, path: &Path, settings: Option<PathBuf>) -> Result<()> {
    let (_guard, listener) = SocketGuard::bind(path)?;
    let (updates, rx) = watch::channel(Status::default());
    let (tx, mut commands) = mpsc::channel::<Command>(32);
    let permits = Arc::new(Semaphore::new(64));
    let (_meter_tx, meter_rx) = mpsc::channel(4);
    let store = Store::user()?;
    let mut shared_config = Arc::new(config.clone());
    let mut state = State {
        recognizer: Recognizer::new(config.engine.clone()),
        processor: Processor::new(&config.replacements, config.output.append_space)?,
        config,
        status: Status::default(),
        updates,
        capture: None,
        meter_rx,
        samples: vec![],
        inference: None,
        delivery: None,
        cancelled: Arc::new(AtomicBool::new(false)),
        last: None,
        target: None,
        reported_phase: Phase::Idle,
        transcription_started: None,
        store: store.clone(),
        session_prompt: String::new(),
        teaching: None,
        playback: None,
    };
    let tray_task = state.config.tray.enabled.then(|| {
        tokio::spawn(crate::tray::run(path.to_owned(), settings.clone(), rx.clone(), state.config.output.notify))
    });
    let recognizer = state.recognizer.clone();
    let mut warmup = Some(tokio::spawn(async move { recognizer.warmup().await }));
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);
    let mut teaching_watchdog = tokio::time::interval(Duration::from_secs(1));
    teaching_watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut settings_check = tokio::time::interval(Duration::from_secs(1));
    settings_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut seen = settings.as_deref().and_then(stamp);
    let mut saved: Option<Config> = None;
    eprintln!("Koe listening on {}", path.display());
    loop {
        tokio::select! {
            _ = settings_check.tick(), if settings.is_some() => {
                let file = settings.as_deref().expect("checked");
                let now = stamp(file);
                if now != seen {
                    seen = now;
                    match Config::load(file) {
                        Ok(config) if config != state.config => saved = Some(config),
                        Ok(_) => saved = None,
                        Err(error) => eprintln!("Koe: {error:#}; keeping the settings in use"),
                    }
                }
                if state.idle() && let Some(config) = saved.take() {
                    match state.reconfigure(config) {
                        Ok(task) => {
                            if task.is_some() { warmup = task; }
                            shared_config = Arc::new(state.config.clone());
                            state.publish();
                        }
                        Err(error) => eprintln!("Koe: {error:#}; keeping the settings in use"),
                    }
                }
            }
            _ = teaching_watchdog.tick(), if state.teaching.is_some() => {
                if state.teaching.as_ref().is_some_and(|t| t.heartbeat.elapsed() > Duration::from_secs(8)) {
                    state.cancel().await;
                    state.publish();
                }
            }
            result = async { match state.playback.as_mut() { Some(task) => task.await, None => pending().await } } => {
                state.playback.take();
                if result.is_err() || result.is_ok_and(|r| r.is_err()) { eprintln!("Koe: teaching playback failed"); }
            }
            result = async { match warmup.as_mut() { Some(task) => task.await, None => pending().await } } => {
                warmup.take();
                match result.context("model warmup task failed").and_then(|v| v) {
                    Ok(()) => if state.recognizer.loaded() { eprintln!("Koe: model ready"); },
                    Err(error) => eprintln!("Koe: model warmup failed: {error:#}"),
                }
                state.publish();
            }
            _ = &mut interrupt => break,
            _ = terminate.recv() => break,
            connection = listener.accept() => {
                let (stream, _) = connection?;
                if let Ok(permit) = permits.clone().try_acquire_owned() {
                    let tx = tx.clone(); let rx = rx.clone(); let store = store.clone(); let config = shared_config.clone();
                    tokio::spawn(async move { let _permit = permit; let _ = serve(stream, tx, rx, store, config).await; });
                }
            }
            Some((request, reply, flushed)) = commands.recv() => {
                let mut vocabulary = None;
                let polling = matches!(request.vocabulary, Some(VocabularyRequest::TeachPoll { .. }));
                let result = if let Some(command) = request.vocabulary {
                    state.vocabulary(command).await.map(|result| { vocabulary = Some(result); None })
                } else { state.command(request.action).await };
                // Read-only polling must not generate events or wake the shell.
                if !polling && !matches!(request.action, Action::Status | Action::Subscribe | Action::Last) { state.publish(); }
                let mut response = Response::status(state.status.clone());
                response.vocabulary = vocabulary;
                match result {
                    Ok(text) => response.text = text,
                    Err(e) => { response.ok = false; response.error = Some(format!("{e:#}")); }
                }
                let _ = reply.send(response);
                if request.action == Action::Shutdown {
                    let _ = tokio::time::timeout(Duration::from_secs(1), flushed).await;
                    break;
                }
            }
            Some(meter) = state.meter_rx.recv(), if state.capture.is_some() => {
                state.status.elapsed_ms = ((state.samples.len() + meter.samples) * 1000 / audio::RATE) as u64;
                state.status.level = meter.level;
                state.publish();
            }
            captured = async { match state.capture.as_mut() { Some(c) => (&mut c.task).await, None => pending().await } } => {
                state.capture.take();
                match captured.context("capture task failed").and_then(|v| v) {
                    Ok(samples) => {
                        state.samples.extend(samples);
                        state.status.elapsed_ms = (state.samples.len() * 1000 / audio::RATE) as u64;
                        state.status.level = 0.0;
                        if let Err(e) = state.recognize() { state.fail(e); }
                        else { state.publish(); }
                    }
                    Err(e) => state.fail(e),
                }
            }
            result = async { match state.inference.as_mut() { Some(t) => t.await, None => pending().await } } => {
                state.inference.take();
                state.status.transcription_ms = state.transcription_started.take().map(|t| t.elapsed().as_millis() as u64);
                if let Some(ms) = state.status.transcription_ms { eprintln!("Koe: transcription took {ms} ms"); }
                match result.context("transcription task failed").and_then(|v| v) {
                    Ok(text) => {
                        if let Some(teaching) = &mut state.teaching {
                            let corrected = state.processor.apply(&text).trim().to_owned();
                            if text.len() > 4096 || corrected.len() > 4096 {
                                state.fail(anyhow::anyhow!("example transcript exceeds 4096 bytes; record a shorter example"));
                                continue;
                            }
                            teaching.preview = Some(TeachingPreview {
                                corrected, heard: text.trim().into(),
                                duration_ms: state.status.elapsed_ms,
                            });
                            state.status.phase = Phase::Idle;
                            state.status.error = None;
                            state.publish();
                            continue;
                        }
                        let text = state.processor.apply(&text);
                        if text.trim().is_empty() { state.fail(anyhow::anyhow!("no speech recognized")); continue; }
                        if serde_json::to_vec(&text)?.len() > 48 * 1024 { state.fail(anyhow::anyhow!("encoded transcript exceeds 48 KiB")); continue; }
                        state.last = Some(text.clone());
                        state.status.phase = Phase::Delivering;
                        state.publish();
                        let output = state.config.output.clone(); let target = state.target;
                        state.delivery = Some(tokio::spawn(async move { desktop::deliver(&text, &output, target).await }));
                    }
                    Err(e) => state.fail(e),
                }
            }
            result = async { match state.delivery.as_mut() { Some(t) => t.await, None => pending().await } } => {
                state.delivery.take();
                match result.context("delivery task failed").and_then(|v| v) {
                    Ok(()) => {
                        state.status.phase = Phase::Idle; state.status.error = None;
                        if state.config.output.notify { desktop::notify("Dictation complete", "Your transcript is ready."); }
                        state.publish();
                    }
                    Err(e) => state.fail(e),
                }
            }
        }
    }
    state.cancel().await;
    // Dropping all senders closes the tray's watch stream and unregisters SNI.
    drop(state);
    if let Some(task) = tray_task {
        let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
    }
    eprintln!("Koe stopped");
    Ok(())
}
