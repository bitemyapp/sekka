//! Opt-in, memory-only system-output captions. The backend and its pipes live
//! on a worker; the window polls a bounded queue and never performs recognition.
use aizome_kit::{column, prelude::*, row};
use serde::Deserialize;
use std::{
    collections::VecDeque, os::unix::process::CommandExt, path::PathBuf, process::Stdio, thread::JoinHandle,
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, BufReader},
    process::Command,
    sync::{mpsc, oneshot},
};

const MAX_LINE: usize = 16 * 1024;
const RECENT_CUES: usize = 6;
const QUEUED_EVENTS: usize = 16;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Ready,
    Cue { text: String, speaker: Option<String> },
    Gap { message: String },
    Error { message: String },
}

#[derive(Clone, Debug)]
enum Msg {
    Start,
    Stop,
    Tick,
    Diarize(bool),
    Font(i32),
    Settings,
    Opened(Result<(), String>),
}

struct Worker {
    cancel: Option<oneshot::Sender<()>>,
    events: mpsc::Receiver<Event>,
    thread: Option<JoinHandle<Result<(), String>>>,
}

impl Worker {
    fn start(config: PathBuf, diarize: bool) -> std::io::Result<Self> {
        let (cancel, stopped) = oneshot::channel();
        let (tx, events) = mpsc::channel(QUEUED_EVENTS);
        let thread = std::thread::Builder::new().name("koe-caption-window".into()).spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("Couldn't start the caption worker: {e}"))?;
            runtime.block_on(capture(config, diarize, tx, stopped))
        })?;
        Ok(Self { cancel: Some(cancel), events, thread: Some(thread) })
    }

    fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }

    fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|thread| thread.is_finished())
    }

    fn join(&mut self) -> Result<(), String> {
        self.thread.take().map_or(Ok(()), |thread| {
            thread.join().unwrap_or_else(|_| Err("The caption worker stopped unexpectedly.".into()))
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
        // Normal Stop/Close waits asynchronously through Tick. This also covers
        // Quit and display disconnection, where there may be no further updates.
        let _ = self.join();
    }
}

/// Kill the whole private process group, including a system-output recorder.
/// The Tokio child below separately reaps the backend process.
struct CaptureGroup(u32);

impl Drop for CaptureGroup {
    fn drop(&mut self) {
        // SAFETY: this is the positive PID returned for our freshly spawned
        // process-group leader; its negative addresses that group, not ours.
        unsafe { libc::kill(-(self.0 as libc::pid_t), libc::SIGKILL) };
    }
}

async fn capture(
    config: PathBuf,
    diarize: bool,
    tx: mpsc::Sender<Event>,
    mut stopped: oneshot::Receiver<()>,
) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| format!("Couldn't locate Koe: {e}"))?;
    let mut command = Command::new(executable);
    command.arg("--config").arg(config).args(["captions", "--json"]);
    if diarize {
        command.arg("--diarize");
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    command.as_std_mut().process_group(0);
    let mut child = command.spawn().map_err(|e| format!("Couldn't start captions: {e}"))?;
    let group = child.id().map(CaptureGroup);
    let result = if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::with_capacity(1024);
        tokio::select! {
            biased;
            _ = &mut stopped => Ok(()),
            result = async {
                while let Some(event) = next_event(&mut reader, &mut line).await? {
                    if let Event::Error { message } = event {
                        return Err(message);
                    }
                    if tx.send(event).await.is_err() {
                        return Ok(());
                    }
                }
                let status = child.wait().await.map_err(|e| format!("Couldn't wait for captions: {e}"))?;
                Err(if status.success() {
                    "System-output capture ended. Check Sound Settings, then press Start to try again.".into()
                } else {
                    format!("Caption backend exited ({status}). Open Koe Settings to select a downloaded local model, and check Sound Settings for an output device.")
                })
            } => result,
        }
    } else {
        Err("Caption backend has no output pipe.".into())
    };
    drop(group);
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

/// Unlike `lines`, this cannot accumulate an unbounded backend response.
async fn next_event<R: AsyncBufRead + Unpin>(reader: &mut R, line: &mut Vec<u8>) -> Result<Option<Event>, String> {
    line.clear();
    loop {
        let bytes = reader.fill_buf().await.map_err(|e| format!("Couldn't read captions: {e}"))?;
        if bytes.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err("Caption backend ended in the middle of a message.".into());
        }
        let newline = bytes.iter().position(|b| *b == b'\n');
        let count = newline.map_or(bytes.len(), |i| i + 1);
        if line.len() + count > MAX_LINE {
            return Err("Caption backend sent an oversized message; capture has stopped.".into());
        }
        line.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if newline.is_some() {
            return serde_json::from_slice(line)
                .map(Some)
                .map_err(|e| format!("Caption backend sent an invalid message: {e}"));
        }
    }
}

struct Captions {
    config: PathBuf,
    window: WindowKey,
    worker: Option<Worker>,
    poll: Option<TimerHandle>,
    ready: bool,
    stopping: bool,
    closing: bool,
    diarize: bool,
    font: i32,
    recent: VecDeque<(String, Option<String>)>,
    notice: Option<String>,
}

impl Captions {
    fn stop(&mut self) {
        self.recent.clear();
        self.ready = false;
        if let Some(worker) = &mut self.worker {
            worker.stop();
            self.stopping = true;
        }
    }

    fn tick(&mut self, cx: &mut Cx<Msg>) {
        let Some(worker) = &mut self.worker else { return };
        // Bound work per frame even if the producer is faster than the view.
        for _ in 0..QUEUED_EVENTS {
            let Ok(event) = worker.events.try_recv() else { break };
            if self.stopping {
                continue;
            }
            match event {
                Event::Ready => self.ready = true,
                Event::Cue { text, speaker } if !text.trim().is_empty() => {
                    if self.recent.len() == RECENT_CUES {
                        self.recent.pop_front();
                    }
                    self.recent.push_back((text, speaker));
                }
                Event::Gap { message } => {
                    self.recent.clear();
                    self.notice = Some(format!("Caption gap: {message}"));
                }
                Event::Error { message } => self.notice = Some(message),
                Event::Cue { .. } => {}
            }
        }
        if worker.finished() {
            let result = worker.join();
            self.worker = None;
            if let Some(timer) = self.poll.take() {
                cx.cancel(timer);
            }
            self.ready = false;
            self.stopping = false;
            self.recent.clear();
            if let Err(error) = result {
                self.notice = Some(error);
            }
            if self.closing {
                cx.close_window(self.window);
            }
        }
    }
}

impl Program for Captions {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Koe.Captions", "Koe Live Captions", "System-output captions")
            .version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(
            WindowSpec::new("Koe Live Captions — system output").size(800.0, 580.0).min_size(680.0, 480.0),
        );
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Start if self.worker.is_none() => {
                self.recent.clear();
                self.notice = None;
                self.ready = false;
                self.stopping = false;
                match Worker::start(self.config.clone(), self.diarize) {
                    Ok(worker) => {
                        self.worker = Some(worker);
                        self.poll = Some(cx.every(Duration::from_millis(100), Msg::Tick));
                    }
                    Err(error) => self.notice = Some(format!("Couldn't start the caption worker: {error}")),
                }
            }
            Msg::Stop => self.stop(),
            Msg::Tick => self.tick(cx),
            Msg::Diarize(on) if self.worker.is_none() => self.diarize = on,
            Msg::Font(delta) => self.font = (self.font + delta).clamp(20, 56),
            Msg::Settings => {
                let config = self.config.clone();
                cx.spawn(move || {
                    let result = std::process::Command::new("aizome-koe")
                        .arg("--config")
                        .arg(config)
                        .args(["--page", "dictation"])
                        .spawn()
                        .map(|mut child| {
                            std::thread::spawn(move || child.wait());
                        })
                        .map_err(|e| format!("Couldn't open Koe Settings: {e}"));
                    Msg::Opened(result)
                });
            }
            Msg::Opened(Err(error)) => self.notice = Some(error),
            Msg::Opened(Ok(())) | Msg::Start | Msg::Diarize(_) => {}
        }
    }

    fn close_requested(&mut self, _window: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        self.closing = true;
        self.stop();
        self.worker.is_none()
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        let active = self.worker.is_some();
        let state = if self.stopping {
            "Stopping system-output capture…"
        } else if self.ready {
            "CAPTURING system output — microphone is not used"
        } else if active {
            "Starting system-output capture…"
        } else {
            "Stopped — no audio is being captured"
        };
        let controls = row![
            button("Start").primary().on_press(Msg::Start).enabled(!active).id("start-captions"),
            button("Stop").on_press(Msg::Stop).enabled(active && !self.stopping).id("stop-captions"),
            spacer(),
            button("Smaller text").on_press(Msg::Font(-4)).enabled(self.font > 20),
            text(format!("{} pt", self.font)),
            button("Larger text").on_press(Msg::Font(4)).enabled(self.font < 56),
        ]
        .spacing(10.0);
        let mut cues: Vec<Element<Msg>> = Vec::with_capacity(self.recent.len().max(1));
        // Newest first keeps current speech visible without stealing scroll focus.
        for (words, speaker) in self.recent.iter().rev() {
            let mut parts = Vec::with_capacity(2);
            if let Some(speaker) = speaker {
                parts.push(text(format!("Detected turn: {speaker}")).size(18.0).color(Color::WHITE).into());
            }
            parts.push(text(words.as_str()).size(self.font as f64).color(Color::WHITE).wrap().into());
            cues.push(column(parts).spacing(6.0).into());
        }
        if cues.is_empty() {
            cues.push(
                text(if active && !self.stopping {
                    "Waiting for speech…"
                } else {
                    "Press Start to caption audio playing on this computer."
                })
                .size(self.font as f64)
                .color(Color::WHITE)
                .wrap()
                .into(),
            );
        }
        let mut parts: Vec<Element<Msg>> = vec![
            text(state).semibold().wrap().id("capture-state").into(),
            controls.into(),
            text("Source: default system audio output (not the microphone). Local recognition; no audio or caption history is saved.")
                .wrap().into(),
            checkbox("Detect speaker turns (English tinydiarize model required)", self.diarize)
                .on_toggle(Msg::Diarize).enabled(!active).id("diarize-captions").into(),
            text("Turn labels mark possible speaker changes, not persistent speaker identities. Recognition can be delayed or wrong.")
                .role(Role::Text2).wrap().into(),
        ];
        if let Some(notice) = &self.notice {
            parts.push(text(notice.as_str()).role(Role::Warning).wrap().id("caption-notice").into());
        }
        parts.push(
            container(scroll(column(cues).spacing(18.0).width(Fill)).height(Fill))
                .fill(Tone::Color(Color::BLACK))
                .padding(18.0)
                .width(Fill)
                .height(Fill)
                .into(),
        );
        parts.push(
            row![
                button("Open Koe Settings").on_press(Msg::Settings),
                text("Choose a downloaded local model there. Dictation can stay off.").role(Role::Text2).wrap(),
            ]
            .spacing(12.0)
            .into(),
        );
        parts.push(
            text("Resize or position this window beside other apps to keep captions visible.")
                .role(Role::Text2)
                .wrap()
                .into(),
        );
        column(parts).spacing(12.0).padding(16.0).width(Fill).height(Fill).into()
    }
}

/// Opening the window never starts capture or downloads a model.
pub fn run(config_path: PathBuf) -> anyhow::Result<()> {
    let app = Captions {
        config: config_path,
        window: WindowKey::NONE,
        worker: None,
        poll: None,
        ready: false,
        stopping: false,
        closing: false,
        diarize: false,
        font: 28,
        recent: VecDeque::with_capacity(RECENT_CUES),
        notice: None,
    };
    aizome_kit::try_run(app).map_err(|error| anyhow::anyhow!("Couldn't open Live Captions: {error}"))?;
    Ok(())
}
