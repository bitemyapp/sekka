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
use kakehashi_core::choice::{DriveList, DriveListing, ImageId, Listed, ReviewTicket};
use kakehashi_core::image::{self, Cancelled};
use kakehashi_core::job::{End, Event, Job, JobDir, JobState, ProgressFile};
use kakehashi_core::nightly::{self, Build, Fetch, NIGHTLY};
use kakehashi_core::write::{Plan, Progress, TAIL};
use kakehashi_core::{Drive, human, platform};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// How often progress from a thread is sent on.
const REPORT: Duration = Duration::from_millis(100);

enum Msg {
    Builds(Result<Vec<Build>, String>),
    Drives(DriveListing),
    Hashed { path: PathBuf, done: u64 },
    HashDone { path: PathBuf, result: Result<(String, Option<(String, PathBuf)>), String> },
    Holds { path: PathBuf, drive: Option<String> },
    Fetch { id: String, fetch: Fetch },
    Fetched { id: String, result: Result<PathBuf, String> },
    Job(Event),
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

pub struct Kakehashi {
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    builds: Loading<Vec<Build>>,
    image: Option<Image>,
    download: Option<Download>,
    drives: DriveList,
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
            drives: DriveList::new(),
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

    /// Lists the drives, if a listing is due.
    fn list_drives(&mut self) {
        let Some(ticket) = self.drives.request(Instant::now()) else { return };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Drives(DriveListing::new(ticket, platform::drives().map_err(|e| format!("{e:#}")))));
        });
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            self.apply(msg);
            changed = true;
        }
        let busy = self.job.as_ref().is_some_and(|j| !j.is_ended());
        if !busy {
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
            Msg::Drives(listing) => self.drives.listed(listing),
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
            Msg::Job(event) => {
                if let Some(job) = self.job.as_mut() {
                    job.handle(event, Instant::now());
                    if job.is_ended() {
                        self.drives.refresh_soon();
                    }
                }
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

    /// The whole drive the image is on, which is never offered.
    fn holds(&self) -> Option<&str> {
        self.image.as_ref().and_then(|i| i.holds.as_deref())
    }

    fn chosen_drive(&self) -> Option<&Drive> {
        self.drives.chosen(self.holds())
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

    /// The image as a write would use it: checked, with its checksum.
    fn image_id(&self) -> Option<ImageId> {
        let image = self.image.as_ref()?;
        match (&image.check, &image.sha) {
            (Check::Verified { .. } | Check::Unknown { .. }, Some(sha)) => {
                Some(ImageId { path: image.path.clone(), sha256: sha.clone() })
            }
            _ => None,
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(image) = self.image_id() else { return };
        let holds = self.holds().map(str::to_owned);
        let Some((review, target)) = self.drives.open_review(holds.as_deref(), image) else { return };
        let (drive, image) = (target.drive(), target.image());
        let name = image.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let view = cx.entity();
        let title = format!("Erase {}?", drive.label());
        let description = format!(
            "{} ({}, {}, {}) will be erased and replaced with {}. Everything on it now will be lost; this can't be undone.",
            drive.label(),
            human(drive.bytes),
            drive.name,
            drive.identity_hint(),
            name
        );
        // OK writes the drive and image described here, found again when it
        // is pressed, and only while this dialog's review is the open one.
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (ok, cancel) = (view.clone(), view.clone());
            alert
                .title(title.clone())
                .description(description.clone())
                .ok_text("Erase and write")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancel")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    ok.update(cx, |this, cx| {
                        this.start_write(review);
                        cx.notify();
                    });
                    true
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.drives.cancel_review(review));
                    true
                })
        });
    }

    /// Writes the drive and image the open dialog described, if they are
    /// still the drive listed under its name and the window's image.
    fn start_write(&mut self, review: ReviewTicket) {
        let image = self.image_id();
        let holds = self.holds().map(str::to_owned);
        let plan = match self.drives.confirm(review, holds.as_deref(), image) {
            Ok(write) => Plan::new(write),
            Err(why) => {
                self.error = Some(why.to_string());
                return;
            }
        };
        let dir = match JobDir::new() {
            Ok(dir) => dir,
            Err(e) => {
                self.error = Some(format!("Could not prepare the write: {e:#}"));
                return;
            }
        };
        self.job = Some(Job::start(&plan, &dir));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // The worker owns the job's directory, with the staged image,
            // until the helper exits, then removes it on success,
            // cancellation and failure alike.
            let dir = dir;
            let cancel = dir.cancel();
            let prepared = (|| -> anyhow::Result<()> {
                #[cfg(target_os = "macos")]
                let plan = {
                    let staged =
                        crate::staged_image::prepare(plan.image(), plan.sha256(), dir.path(), &|| cancel.exists())?;
                    plan.with_image(staged)
                };
                if cancel.exists() {
                    anyhow::bail!(Cancelled);
                }
                fs::write(dir.plan(), serde_json::to_string(&plan)?)?;
                Ok(())
            })();
            if let Err(e) = prepared {
                let message = if e.is::<Cancelled>() {
                    "Stopped before writing; nothing was written.".to_owned()
                } else {
                    format!("{e:#}")
                };
                let _ = tx.send(Msg::Job(Event::NotStarted(message)));
                return;
            }
            let helper = match elevate::helper_path() {
                Ok(helper) => helper,
                Err(e) => {
                    let _ = tx.send(Msg::Job(Event::NotStarted(format!("could not find the helper: {e}"))));
                    return;
                }
            };
            let _ = tx.send(Msg::Job(Event::Authorizing));
            let (p, q) = (dir.plan(), dir.progress());
            let runner = std::thread::spawn(move || elevate::run(&helper, &p, &q));
            let mut reports = ProgressFile::new(&dir);
            while !runner.is_finished() {
                for report in reports.read() {
                    let _ = tx.send(Msg::Job(Event::Report(report)));
                }
                std::thread::sleep(REPORT);
            }
            let ran = runner.join().unwrap_or(Ran::Failed("the helper's thread failed".into()));
            // The helper's last lines come with its exit; the directory goes
            // only after them.
            let exit = reports.finish(ran);
            drop(dir);
            let _ = tx.send(Msg::Job(Event::Exited(exit)));
        });
    }

    fn cancel_write(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.stop();
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
        let (offered, not) = self.drives.sorted(self.holds());
        match self.drives.drives() {
            Listed::Pending => {
                s = s.child(
                    h_flex().gap_2().child(Spinner::new().small()).child(div().text_sm().child("Looking for drives…")),
                )
            }
            Listed::Failed(e) => {
                s = s.child(div().text_sm().text_color(theme.danger).child(format!("Could not list drives: {e}")))
            }
            Listed::Ready(_) if offered.is_empty() => {
                s = s.child(div().text_sm().child("Plug in a USB drive; it appears here."))
            }
            Listed::Ready(_) => {
                for drive in &offered {
                    let chosen = self.drives.is_chosen(drive);
                    let row = (*drive).clone();
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
                                this.drives.choose(&row);
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
                .checked(self.drives.show_all())
                .on_change(cx.listener(|this, value: &bool, _, cx| {
                    this.drives.set_show_all(*value);
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
                    why.reason()
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
        let write = job.ctx();
        let mut s = section(&format!("Writing {} to {}", write.image(), write.drive().label()), cx);
        let finished =
            |ejected: bool, note: &Option<String>| {
                let tail = if ejected {
                    "It has been ejected; you can unplug it.".to_owned()
                } else {
                    note.clone().unwrap_or_else(|| "You can unplug it.".into())
                };
                v_flex()
                    .gap_2()
                    .child(h_flex().gap_2().child(Tag::success().child("Done")).child(div().text_sm().child(format!(
                        "{} is written, and every byte was read back and matched.",
                        write.drive().label()
                    ))))
                    .child(div().text_sm().child(tail))
            };
        let failed = |message: &str, touched: bool| {
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(Tag::danger().child("Not written"))
                        .child(div().flex_1().min_w_0().text_sm().child(message.to_owned())),
                )
                .when(touched, |c| {
                    c.child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("The drive was partly written: write it again before using it, or format it."),
                    )
                })
        };
        match job.stored() {
            JobState::Preparing(()) => {
                s = s.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child(div().text_sm().child("Preparing and checking the image…")),
                )
            }
            JobState::Authorizing(()) => {
                s = s.child(
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child(div().text_sm().child("Waiting for an administrator's permission…")),
                )
            }
            JobState::Ended(End::Declined) => {
                s = s.child(div().text_sm().child("Permission wasn't given, so nothing was written."))
            }
            JobState::Ended(End::Finished { ejected, note }) => s = s.child(finished(*ejected, note)),
            JobState::Ended(End::Failed(failure)) => s = s.child(failed(failure.message(), failure.touched())),
            JobState::Running(pass) => match pass.last() {
                Progress::Preparing => {
                    s = s.child(h_flex().gap_2().child(Spinner::new().small()).child(div().text_sm().child(
                        if cfg!(target_os = "macos") {
                            "Preparing the drive. Allow access in the macOS permission prompt…"
                        } else {
                            "Checking the drive and unmounting it…"
                        },
                    )))
                }
                progress @ (Progress::Writing { done, total } | Progress::Verifying { done, total }) => {
                    let verifying = matches!(progress, Progress::Verifying { .. });
                    s = s
                        .child(div().text_sm().child(if verifying {
                            "Reading every byte back and comparing (2 of 2)"
                        } else {
                            "Writing (1 of 2)"
                        }))
                        .child(Bar::new("job").value(*done as f32 * 100.0 / (*total).max(1) as f32))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                            "{} of {}  {}",
                            human(*done),
                            human(*total),
                            speed_and_left(*done, *total, pass.from())
                        )))
                }
                // The helper's final report, before it has exited.
                Progress::Finished { ejected, note } => s = s.child(finished(*ejected, note)),
                Progress::Failed { message, touched } => s = s.child(failed(message, *touched)),
            },
        }
        if job.can_stop() {
            s = s.child(
                Button::new("cancel")
                    .label(if write.cancelling() { "Stopping…" } else { "Stop" })
                    .small()
                    .disabled(write.cancelling())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_write();
                        cx.notify();
                    })),
            );
        }
        if job.is_ended() {
            s = s.child(Button::new("again").label("Write another drive").small().primary().on_click(cx.listener(
                |this, _, _, cx| {
                    this.job = None;
                    this.drives.clear_choice();
                    this.drives.refresh_soon();
                    cx.notify();
                },
            )));
        }
        s.into_any_element()
    }
}

// The window's regressions: a helper killed mid-write (KTL-03), and a stick
// swapped for another under the confirmation dialog (KTL-04).
#[cfg(test)]
#[path = "repro_ktl_03.rs"]
mod repro_ktl_03;
#[cfg(test)]
#[path = "repro_ktl_04.rs"]
mod repro_ktl_04;
