//! Kakehashi's window: an image, a drive, and the write.
//!
//! Slow work (listing builds and drives, checksums, downloads, the helper)
//! runs on threads that send messages; a short timer in the window takes
//! them in and redraws.

use crate::elevate::{self, Ran};
use crate::when;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::progress::Progress as Bar;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, ExternalPaths, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
};
use kakehashi_core::image::{self, Cancelled};
use kakehashi_core::nightly::{self, Build, Fetch, NIGHTLY};
use kakehashi_core::write::{Plan, Progress, TAIL};
use kakehashi_core::{Drive, human, platform};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// How often the drive list is refreshed, so a stick plugged in appears.
const REFRESH: Duration = Duration::from_secs(2);
/// How often progress from a thread is sent on.
const REPORT: Duration = Duration::from_millis(100);

enum Msg {
    Builds(Result<Vec<Build>, String>),
    Drives(Result<Vec<Drive>, String>),
    Hashed { path: PathBuf, done: u64 },
    HashDone { path: PathBuf, result: Result<(String, Option<(String, PathBuf)>), String> },
    Holds { path: PathBuf, drive: Option<String> },
    Fetch { id: String, fetch: Fetch },
    Fetched { id: String, result: Result<PathBuf, String> },
    Authorizing,
    Job(Progress),
    JobEnded(Ran),
}

enum Loading<T> {
    Pending,
    Ready(T),
    Failed(String),
}

enum Check {
    Hashing {
        done: u64,
    },
    /// Matches the checksum from `source`.
    Verified {
        source: String,
    },
    /// No checksum to compare with.
    Unknown {
        sha: String,
    },
    Mismatch {
        sha: String,
        want: String,
        source: String,
    },
    Failed(String),
}

struct Image {
    path: PathBuf,
    bytes: u64,
    sha: Option<String>,
    /// The whole drive the image is on, which is never offered.
    holds: Option<String>,
    check: Check,
    cancel: Arc<AtomicBool>,
}

struct Download {
    build: Build,
    fetch: Option<Fetch>,
    /// Bytes and time when downloading began, for the rate.
    from: Option<(u64, Instant)>,
    cancel: Arc<AtomicBool>,
}

enum Stage {
    PreparingImage,
    Authorizing,
    Running(Progress),
    Declined,
}

struct Job {
    drive: Drive,
    image: String,
    stage: Stage,
    /// The writing or verifying phase's start, for the rate.
    phase: Option<(bool, u64, Instant)>,
    cancel: PathBuf,
    cancelling: bool,
    ended: bool,
}

pub struct Kakehashi {
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    builds: Loading<Vec<Build>>,
    image: Option<Image>,
    download: Option<Download>,
    drives: Loading<Vec<Drive>>,
    listing: bool,
    listed: Instant,
    show_all: bool,
    chosen: Option<String>,
    job: Option<Job>,
    /// A download or preparation that failed, shown under the image.
    error: Option<String>,
    downloads: PathBuf,
    _poll: Task<()>,
    _appearance: gpui_kit::Subscription,
}

fn rate(done: u64, from: Option<(u64, Instant)>) -> Option<(f64, Duration)> {
    let (start, at) = from?;
    let elapsed = at.elapsed();
    (elapsed > Duration::from_secs(1) && done > start).then(|| ((done - start) as f64 / elapsed.as_secs_f64(), elapsed))
}

fn speed_and_left(done: u64, total: u64, from: Option<(u64, Instant)>) -> String {
    match rate(done, from) {
        Some((per_second, _)) => {
            let left = total.saturating_sub(done) as f64 / per_second;
            format!("{}/s, about {} left", human(per_second as u64), when::duration(left as u64))
        }
        None => String::new(),
    }
}

impl Kakehashi {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Night or Day, as the system changes.
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            gpui_kit::component::Theme::sync_system_appearance(Some(window), cx);
        });
        let (tx, rx) = mpsc::channel();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REPORT).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        });
        let downloads = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir);
        let mut this = Kakehashi {
            tx,
            rx,
            builds: Loading::Pending,
            image: None,
            download: None,
            drives: Loading::Pending,
            listing: false,
            listed: Instant::now() - REFRESH,
            show_all: false,
            chosen: None,
            job: None,
            error: None,
            downloads,
            _poll: poll,
            _appearance: appearance,
        };
        this.load_builds();
        this
    }

    fn load_builds(&mut self) {
        self.builds = Loading::Pending;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Builds(nightly::builds(NIGHTLY).map_err(|e| format!("{e:#}"))));
        });
    }

    fn list_drives(&mut self) {
        if self.listing {
            return;
        }
        self.listing = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Drives(platform::drives().map_err(|e| format!("{e:#}"))));
        });
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            self.apply(msg);
            changed = true;
        }
        let busy = self.job.as_ref().is_some_and(|j| !j.ended);
        if !busy && self.listed.elapsed() >= REFRESH {
            self.listed = Instant::now();
            self.list_drives();
        }
        if changed {
            cx.notify();
        }
    }

    fn apply(&mut self, msg: Msg) {
        match msg {
            Msg::Builds(result) => {
                self.builds = match result {
                    Ok(builds) => Loading::Ready(builds),
                    Err(e) => Loading::Failed(e),
                }
            }
            Msg::Drives(result) => {
                self.listing = false;
                self.drives = match result {
                    Ok(drives) => Loading::Ready(drives),
                    Err(e) => Loading::Failed(e),
                };
                // A chosen drive that was unplugged is no longer chosen.
                if let (Some(name), Loading::Ready(drives)) = (&self.chosen, &self.drives)
                    && !drives.iter().any(|d| &d.name == name)
                {
                    self.chosen = None;
                }
            }
            Msg::Hashed { path, done } => {
                if let Some(image) = self.image.as_mut().filter(|i| i.path == path) {
                    image.check = Check::Hashing { done };
                }
            }
            Msg::HashDone { path, result } => {
                if let Some(image) = self.image.as_mut().filter(|i| i.path == path) {
                    image.check = match result {
                        Ok((sha, Some((want, source)))) => {
                            let source = source.display().to_string();
                            if sha == want {
                                image.sha = Some(sha);
                                Check::Verified { source }
                            } else {
                                Check::Mismatch { sha, want, source }
                            }
                        }
                        Ok((sha, None)) => {
                            image.sha = Some(sha.clone());
                            Check::Unknown { sha }
                        }
                        Err(e) => Check::Failed(e),
                    };
                }
            }
            Msg::Holds { path, drive } => {
                if let Some(image) = self.image.as_mut().filter(|i| i.path == path) {
                    image.holds = drive;
                }
            }
            Msg::Fetch { id, fetch } => {
                if let Some(download) = self.download.as_mut().filter(|d| d.build.id == id) {
                    if let Fetch::Downloading { done, .. } = fetch
                        && download.from.is_none()
                    {
                        download.from = Some((done, Instant::now()));
                    }
                    download.fetch = Some(fetch);
                }
            }
            Msg::Fetched { id, result } => {
                let Some(download) = self.download.take_if(|d| d.build.id == id) else { return };
                match result {
                    Ok(path) => {
                        let source = "the list of builds at downloads.sekka.computer".to_owned();
                        self.choose_image(path, Some((download.build.sha256.clone(), source)));
                    }
                    Err(e) => {
                        self.image = None;
                        self.error = Some(e);
                    }
                }
            }
            Msg::Authorizing => {
                if let Some(job) = self.job.as_mut() {
                    job.stage = Stage::Authorizing;
                }
            }
            Msg::Job(progress) => {
                if let Some(job) = self.job.as_mut() {
                    match &progress {
                        Progress::Writing { done, .. } | Progress::Verifying { done, .. } => {
                            let verifying = matches!(progress, Progress::Verifying { .. });
                            if job.phase.is_none_or(|(v, _, _)| v != verifying) {
                                job.phase = Some((verifying, *done, Instant::now()));
                            }
                        }
                        _ => {}
                    }
                    job.stage = Stage::Running(progress);
                }
            }
            Msg::JobEnded(ran) => {
                if let Some(job) = self.job.as_mut() {
                    job.ended = true;
                    let reported =
                        matches!(job.stage, Stage::Running(Progress::Finished { .. } | Progress::Failed { .. }));
                    if !reported {
                        job.stage = match ran {
                            Ran::Declined => Stage::Declined,
                            Ran::Failed(message) => Stage::Running(Progress::Failed { message, touched: false }),
                            Ran::Exited(code) => Stage::Running(Progress::Failed {
                                message: format!("The helper stopped (code {code}) without saying why."),
                                touched: !matches!(job.stage, Stage::PreparingImage | Stage::Authorizing),
                            }),
                        };
                    }
                }
                self.listed = Instant::now() - REFRESH;
            }
        }
    }

    /// Take `path` as the image: its checksum is checked against `known`, or
    /// a checksum file beside it, and the drive it is on is found.
    fn choose_image(&mut self, path: PathBuf, known: Option<(String, String)>) {
        if let Some(old) = &self.image {
            old.cancel.store(true, Ordering::Relaxed);
        }
        self.error = None;
        let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let cancel = Arc::new(AtomicBool::new(false));
        let check = match &known {
            Some((_, source)) => Check::Verified { source: source.clone() },
            None => Check::Hashing { done: 0 },
        };
        self.image = Some(Image {
            path: path.clone(),
            bytes,
            sha: known.as_ref().map(|(sha, _)| sha.clone()),
            holds: None,
            check,
            cancel: cancel.clone(),
        });
        let tx = self.tx.clone();
        let p = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Holds { drive: platform::image_drive(&p), path: p });
        });
        if known.is_some() {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let expected = image::expected_sha256(&path);
            let mut last = Instant::now();
            let result = image::sha256_file(
                &path,
                &mut |done| {
                    if last.elapsed() >= REPORT {
                        last = Instant::now();
                        let _ = tx.send(Msg::Hashed { path: path.clone(), done });
                    }
                },
                &|| cancel.load(Ordering::Relaxed),
            );
            let result = match (result, expected) {
                (Ok(sha), Ok(expected)) => Ok((sha, expected)),
                (Err(e), _) if e.is::<Cancelled>() => return,
                (Err(e), _) | (_, Err(e)) => Err(format!("{e:#}")),
            };
            let _ = tx.send(Msg::HashDone { path, result });
        });
    }

    fn pick_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.choose_image(path, None);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn start_download(&mut self, build: Build) {
        if let Some(image) = &self.image {
            image.cancel.store(true, Ordering::Relaxed);
        }
        self.image = None;
        self.error = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.download = Some(Download { build: build.clone(), fetch: None, from: None, cancel: cancel.clone() });
        let (tx, dir) = (self.tx.clone(), self.downloads.clone());
        std::thread::spawn(move || {
            let mut last = Instant::now() - REPORT;
            let id = build.id.clone();
            let result = nightly::download(
                &build,
                NIGHTLY,
                &dir,
                &mut |fetch| {
                    if last.elapsed() >= REPORT {
                        last = Instant::now();
                        let _ = tx.send(Msg::Fetch { id: id.clone(), fetch });
                    }
                },
                &|| cancel.load(Ordering::Relaxed),
            );
            let result = match result {
                Ok(path) => Ok(path),
                Err(e) if e.is::<Cancelled>() => Err("The download was stopped; starting it again resumes it.".into()),
                Err(e) => Err(format!("{e:#}")),
            };
            let _ = tx.send(Msg::Fetched { id: build.id, result });
        });
    }

    /// The drives offered, and those that are not with why.
    fn sorted_drives(&self) -> (Vec<&Drive>, Vec<(&Drive, &'static str)>) {
        let Loading::Ready(drives) = &self.drives else { return (Vec::new(), Vec::new()) };
        let holds = self.image.as_ref().and_then(|i| i.holds.as_deref());
        let mut offered = Vec::new();
        let mut not = Vec::new();
        for drive in drives {
            match drive.excluded(holds, self.show_all) {
                None => offered.push(drive),
                Some(why) => not.push((drive, why.reason())),
            }
        }
        (offered, not)
    }

    fn chosen_drive(&self) -> Option<&Drive> {
        let (offered, _) = self.sorted_drives();
        offered.into_iter().find(|d| Some(&d.name) == self.chosen.as_ref())
    }

    /// What stops writing now, if anything.
    fn blocked(&self) -> Option<String> {
        let Some(image) = &self.image else { return Some("Choose an image first.".into()) };
        match &image.check {
            Check::Hashing { .. } => return Some("Checking the image…".into()),
            Check::Mismatch { .. } => return Some("The image doesn't match its checksum.".into()),
            Check::Failed(_) => return Some("The image could not be read.".into()),
            Check::Verified { .. } | Check::Unknown { .. } => {}
        }
        let Some(drive) = self.chosen_drive() else { return Some("Choose a drive.".into()) };
        if image.bytes.div_ceil(4096) * 4096 + TAIL > drive.bytes {
            return Some(format!(
                "The image ({}) doesn't fit on {} ({}).",
                human(image.bytes),
                drive.label(),
                human(drive.bytes)
            ));
        }
        None
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(image), Some(drive)) = (&self.image, self.chosen_drive().cloned()) else { return };
        let name = image.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let view = cx.entity();
        let title = format!("Erase {}?", drive.label());
        let description = format!(
            "{} ({}, {}, {}) will be erased and replaced with {}. Everything on it now will be lost; this can't be undone.",
            drive.label(),
            human(drive.bytes),
            drive.name,
            drive.serial_hint(),
            name
        );
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .title(title.clone())
                .description(description.clone())
                .ok_text("Erase and write")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancel")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.start_write();
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn start_write(&mut self) {
        let (Some(image), Some(drive)) = (&self.image, self.chosen_drive().cloned()) else { return };
        let Some(sha) = image.sha.clone() else { return };
        let plan = Plan { image: image.path.clone(), sha256: sha, drive: drive.clone() };
        let started = (|| -> anyhow::Result<(tempfile::TempDir, PathBuf, PathBuf)> {
            let dir = tempfile::Builder::new().prefix("kakehashi-").tempdir()?;
            let plan_path = dir.path().join("plan.json");
            let progress = dir.path().join("progress");
            File::create(&progress)?;
            Ok((dir, plan_path, progress))
        })();
        let (dir, plan_path, progress) = match started {
            Ok(started) => started,
            Err(e) => {
                self.error = Some(format!("Could not prepare the write: {e:#}"));
                return;
            }
        };
        let cancel = PathBuf::from(format!("{}.cancel", progress.display()));
        self.job = Some(Job {
            drive,
            image: image.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            stage: Stage::PreparingImage,
            phase: None,
            cancel: cancel.clone(),
            cancelling: false,
            ended: false,
        });
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // The worker owns the staged image until the helper exits, then
            // removes it on success, cancellation and failure alike.
            let dir = dir;
            let prepared = (|| -> anyhow::Result<()> {
                #[cfg(target_os = "macos")]
                let plan = Plan {
                    image: crate::staged_image::prepare(&plan.image, &plan.sha256, dir.path(), &|| cancel.exists())?,
                    ..plan
                };
                if cancel.exists() {
                    anyhow::bail!(Cancelled);
                }
                fs::write(&plan_path, serde_json::to_string(&plan)?)?;
                Ok(())
            })();
            if let Err(e) = prepared {
                let message = if e.is::<Cancelled>() {
                    "Stopped before writing; nothing was written.".to_owned()
                } else {
                    format!("{e:#}")
                };
                let _ = tx.send(Msg::JobEnded(Ran::Failed(message)));
                return;
            }
            let helper = match elevate::helper_path() {
                Ok(helper) => helper,
                Err(e) => {
                    let _ = tx.send(Msg::JobEnded(Ran::Failed(format!("could not find the helper: {e}"))));
                    return;
                }
            };
            let _ = tx.send(Msg::Authorizing);
            let (p, q) = (plan_path.clone(), progress.clone());
            let runner = std::thread::spawn(move || elevate::run(&helper, &p, &q));
            let mut offset = 0u64;
            let mut pending = String::new();
            loop {
                let finished = runner.is_finished();
                read_progress(&progress, &mut offset, &mut pending, &tx);
                if finished {
                    break;
                }
                std::thread::sleep(REPORT);
            }
            let ran = runner.join().unwrap_or(Ran::Failed("the helper's thread failed".into()));
            drop(dir);
            let _ = tx.send(Msg::JobEnded(ran));
        });
    }

    fn cancel_write(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.cancelling = true;
            let _ = File::create(&job.cancel);
        }
    }
}

/// Send on whatever whole lines the helper appended since `offset`.
fn read_progress(path: &Path, offset: &mut u64, pending: &mut String, tx: &mpsc::Sender<Msg>) {
    let Ok(mut file) = File::open(path) else { return };
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return;
    }
    let mut new = String::new();
    if let Ok(n) = file.read_to_string(&mut new) {
        *offset += n as u64;
        pending.push_str(&new);
        while let Some(end) = pending.find('\n') {
            let line: String = pending.drain(..=end).collect();
            if let Ok(progress) = serde_json::from_str::<Progress>(line.trim()) {
                let _ = tx.send(Msg::Job(progress));
            }
        }
    }
}

impl Render for Kakehashi {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, foreground) = (theme.background, theme.foreground);
        let body = match &self.job {
            Some(_) => v_flex().gap_4().child(self.render_job(cx)),
            None => v_flex()
                .gap_4()
                .child(self.render_image(cx))
                .child(self.render_drives(cx))
                .child(self.render_write(window, cx)),
        };
        v_flex()
            .id("kakehashi")
            .size_full()
            .bg(background)
            .text_color(foreground)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if this.job.is_none()
                    && let Some(path) = paths.paths().first()
                {
                    this.download = None;
                    this.choose_image(path.clone(), None);
                    cx.notify();
                }
            }))
            .child(self.render_header(cx))
            .child(div().id("body").flex_1().overflow_y_scroll().px_6().pb_6().child(body))
    }
}

/// A titled section.
fn section(title: &str, cx: &Context<Kakehashi>) -> gpui_kit::Div {
    let theme = cx.theme();
    v_flex().gap_3().p_4().rounded_lg().border_1().border_color(theme.border).child(
        div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(title.to_owned()),
    )
}

impl Kakehashi {
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .px_6()
            .pt_6()
            .pb_4()
            .gap_1()
            .child(div().text_xl().font_weight(FontWeight::BOLD).child("Kakehashi"))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Write Sekka, or any disk image, to a USB drive, then read every byte back."),
            )
    }

    fn render_image(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut s = section("1  Image", cx);

        // Sekka's nightly builds.
        let mut nightlies = v_flex().gap_2();
        match &self.builds {
            Loading::Pending => {
                nightlies = nightlies.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child(div().text_sm().child("Looking for nightly builds…")),
                )
            }
            Loading::Failed(e) => {
                nightlies = nightlies.child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.danger)
                                .child(format!("Could not list nightly builds: {e}")),
                        )
                        .child(Button::new("retry").label("Try again").small().on_click(cx.listener(
                            |this, _, _, cx| {
                                this.load_builds();
                                cx.notify();
                            },
                        ))),
                )
            }
            Loading::Ready(builds) if builds.is_empty() => {
                nightlies = nightlies.child(div().text_sm().child("No nightly build has been published yet."))
            }
            Loading::Ready(builds) => {
                let now = when::now();
                let busy = self.download.is_some();
                for (i, build) in builds.iter().enumerate() {
                    let newest = i == 0;
                    let uploaded = when::parse_utc(&build.uploaded);
                    let date = uploaded.map(when::day_and_time).unwrap_or_else(|| build.uploaded.clone());
                    let age = uploaded.map(|t| when::age(now - t)).unwrap_or_default();
                    let chosen = self
                        .image
                        .as_ref()
                        .is_some_and(|img| img.path.file_name().is_some_and(|n| n == build.file_name()));
                    let b = build.clone();
                    nightlies = nightlies.child(
                        h_flex()
                            .justify_between()
                            .gap_3()
                            .p_2()
                            .rounded_md()
                            .when(newest, |row| row.border_1().border_color(theme.primary))
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .when(newest, |row| row.child(Tag::primary().small().child("Newest")))
                                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(date))
                                            .child(div().text_sm().text_color(theme.muted_foreground).child(age)),
                                    )
                                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                                        "{} · revision {}",
                                        human(build.bytes),
                                        &build.revision[..build.revision.len().min(7)]
                                    ))),
                            )
                            .child(
                                Button::new(SharedString::from(format!("get-{}", build.id)))
                                    .label(if chosen { "Chosen" } else { "Download" })
                                    .small()
                                    .when(newest && !chosen, |b| b.primary())
                                    .disabled(busy || chosen)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.start_download(b.clone());
                                        cx.notify();
                                    })),
                            ),
                    );
                }
            }
        }
        s = s.child(nightlies);

        // A download in progress.
        if let Some(download) = &self.download {
            let (label, fraction, detail) = match download.fetch {
                None => ("Starting the download…".to_owned(), 0.0, String::new()),
                Some(Fetch::Checking { done, total }) => (
                    "Checking what is already downloaded…".to_owned(),
                    done as f32 / total.max(1) as f32,
                    format!("{} of {}", human(done), human(total)),
                ),
                Some(Fetch::Downloading { done, total }) => (
                    format!("Downloading {}", download.build.file_name()),
                    done as f32 / total.max(1) as f32,
                    format!("{} of {}  {}", human(done), human(total), speed_and_left(done, total, download.from)),
                ),
            };
            s = s.child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().child(label))
                    .child(Bar::new("download").value(fraction * 100.0))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_xs().text_color(theme.muted_foreground).child(detail))
                            .child(Button::new("stop-download").label("Stop").small().ghost().on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(d) = &this.download {
                                        d.cancel.store(true, Ordering::Relaxed);
                                    }
                                    cx.notify();
                                },
                            ))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("Into {}", self.downloads.display())),
                    ),
            );
        }

        // Or an image of your own.
        s = s.child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("choose").label("Choose a file…").small().disabled(self.download.is_some()).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.pick_file(cx);
                        }),
                    ),
                )
                .child(div().text_xs().text_color(theme.muted_foreground).child("or drop an image onto this window")),
        );

        if let Some(error) = &self.error {
            s = s.child(div().text_sm().text_color(theme.danger).child(error.clone()));
        }

        // The image chosen.
        if let Some(image) = &self.image {
            let name = image.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let status: AnyElement = match &image.check {
                Check::Hashing { done } => v_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!("Checking its SHA-256… {} of {}", human(*done), human(image.bytes))))
                    .child(Bar::new("hash").value(*done as f32 * 100.0 / image.bytes.max(1) as f32))
                    .into_any_element(),
                Check::Verified { source } => h_flex()
                    .gap_2()
                    .child(Tag::success().small().child("Verified"))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("matches {source}")))
                    .into_any_element(),
                Check::Unknown { sha } => v_flex()
                    .gap_1()
                    .child(h_flex().gap_2().child(Tag::warning().small().child("Not verified")).child(
                        div().text_xs().child("There is no checksum beside it to compare with, so Kakehashi can't tell whether it is intact."),
                    ))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("SHA-256 {sha}")))
                    .into_any_element(),
                Check::Mismatch { sha, want, source } => v_flex()
                    .gap_1()
                    .child(h_flex().gap_2().child(Tag::danger().small().child("Doesn't match")).child(
                        div().text_xs().child(format!("{source} says {want}")),
                    ))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("The image's SHA-256 is {sha}. Download it again.")))
                    .into_any_element(),
                Check::Failed(e) => div().text_xs().text_color(theme.danger).child(e.clone()).into_any_element(),
            };
            s = s.child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .bg(theme.secondary)
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(name))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(human(image.bytes))),
                    )
                    .child(status),
            );
        }
        s.into_any_element()
    }

    fn render_drives(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut s = section("2  Drive", cx);
        let (offered, not) = self.sorted_drives();
        match &self.drives {
            Loading::Pending => {
                s = s.child(
                    h_flex().gap_2().child(Spinner::new().small()).child(div().text_sm().child("Looking for drives…")),
                )
            }
            Loading::Failed(e) => {
                s = s.child(div().text_sm().text_color(theme.danger).child(format!("Could not list drives: {e}")))
            }
            Loading::Ready(_) if offered.is_empty() => {
                s = s.child(div().text_sm().child("Plug in a USB drive; it appears here."))
            }
            Loading::Ready(_) => {
                for drive in &offered {
                    let chosen = self.chosen.as_ref() == Some(&drive.name);
                    let name = drive.name.clone();
                    let mut details = vec![human(drive.bytes), drive.name.clone()];
                    if !drive.bus.is_empty() {
                        details.push(drive.bus.clone());
                    }
                    details.push(drive.serial_hint());
                    s = s.child(
                        v_flex()
                            .id(SharedString::from(format!("drive-{}", drive.name)))
                            .gap_0p5()
                            .p_3()
                            .rounded_md()
                            .border_1()
                            .cursor_pointer()
                            .border_color(if chosen { theme.primary } else { theme.border })
                            .when(chosen, |row| row.bg(theme.secondary))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.chosen = Some(name.clone());
                                cx.notify();
                            }))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(drive.label()))
                                    .when(chosen, |row| row.child(Tag::primary().small().child("Chosen"))),
                            )
                            .child(div().text_xs().text_color(theme.muted_foreground).child(details.join(" · ")))
                            .when(!drive.mounts.is_empty(), |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("In use as {}; it will be unmounted.", drive.mounts.join(", "))),
                                )
                            }),
                    );
                }
            }
        }
        s = s.child(
            Switch::new("all")
                .label("Show all external drives, not only USB and removable ones")
                .checked(self.show_all)
                .on_change(cx.listener(|this, value: &bool, _, cx| {
                    this.show_all = *value;
                    cx.notify();
                })),
        );
        if !not.is_empty() {
            let mut list = v_flex().gap_0p5();
            for (drive, why) in not {
                list = list.child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                    "{} ({}, {}): {}",
                    drive.label(),
                    drive.name,
                    human(drive.bytes),
                    why
                )));
            }
            s = s
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child("Not offered"),
                )
                .child(list);
        }
        s.into_any_element()
    }

    fn render_write(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let blocked = self.blocked();
        let mut s = section("3  Write", cx).child(
            Button::new("write")
                .label(match self.chosen_drive() {
                    Some(drive) => format!("Erase {} and write", drive.label()),
                    None => "Erase and write".into(),
                })
                .danger()
                .disabled(blocked.is_some())
                .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
        );
        if let Some(why) = blocked {
            s = s.child(div().text_xs().text_color(theme.muted_foreground).child(why));
        } else {
            s = s.child(div().text_xs().text_color(theme.muted_foreground).child(
                "You'll be asked for an administrator's password. Every byte is read back and compared after writing.",
            ));
        }
        s.into_any_element()
    }

    fn render_job(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(job) = &self.job else { return div().into_any_element() };
        let mut s = section(&format!("Writing {} to {}", job.image, job.drive.label()), cx);
        let running = !job.ended;
        match &job.stage {
            Stage::PreparingImage => {
                s = s.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child(div().text_sm().child("Preparing and checking the image…")),
                )
            }
            Stage::Authorizing => {
                s = s.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child(div().text_sm().child("Waiting for an administrator's permission…")),
                )
            }
            Stage::Declined => s = s.child(div().text_sm().child("Permission wasn't given, so nothing was written.")),
            Stage::Running(progress) => match progress {
                Progress::Preparing => {
                    s = s.child(
                        h_flex()
                            .gap_2()
                            .child(Spinner::new().small())
                            .child(div().text_sm().child("Checking the drive and unmounting it…")),
                    )
                }
                Progress::Writing { done, total } | Progress::Verifying { done, total } => {
                    let verifying = matches!(progress, Progress::Verifying { .. });
                    let from = job.phase.filter(|(v, _, _)| *v == verifying).map(|(_, d, at)| (d, at));
                    s =
                        s.child(div().text_sm().child(if verifying {
                            "Reading every byte back and comparing (2 of 2)"
                        } else {
                            "Writing (1 of 2)"
                        }))
                        .child(Bar::new("job").value(*done as f32 * 100.0 / (*total).max(1) as f32))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(
                            format!("{} of {}  {}", human(*done), human(*total), speed_and_left(*done, *total, from)),
                        ))
                }
                Progress::Finished { ejected, note } => {
                    let tail = if *ejected {
                        "It has been ejected; you can unplug it.".to_owned()
                    } else {
                        note.clone().unwrap_or_else(|| "You can unplug it.".into())
                    };
                    s = s.child(
                        v_flex()
                            .gap_2()
                            .child(h_flex().gap_2().child(Tag::success().child("Done")).child(div().text_sm().child(
                                format!("{} is written, and every byte was read back and matched.", job.drive.label()),
                            )))
                            .child(div().text_sm().child(tail)),
                    )
                }
                Progress::Failed { message, touched } => {
                    s = s.child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(Tag::danger().child("Not written"))
                                    .child(div().flex_1().min_w_0().text_sm().child(message.clone())),
                            )
                            .when(*touched, |c| {
                                c.child(div().text_sm().text_color(theme.muted_foreground).child(
                                    "The drive was partly written: write it again before using it, or format it.",
                                ))
                            }),
                    )
                }
            },
        }
        if running && !matches!(job.stage, Stage::Authorizing) {
            s = s.child(
                Button::new("cancel")
                    .label(if job.cancelling { "Stopping…" } else { "Stop" })
                    .small()
                    .disabled(job.cancelling)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_write();
                        cx.notify();
                    })),
            );
        }
        if job.ended {
            s = s.child(Button::new("again").label("Write another drive").small().primary().on_click(cx.listener(
                |this, _, _, cx| {
                    this.job = None;
                    this.chosen = None;
                    this.listed = Instant::now() - REFRESH;
                    cx.notify();
                },
            )));
        }
        s.into_any_element()
    }
}
