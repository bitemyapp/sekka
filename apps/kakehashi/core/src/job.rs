//! A write as the window follows it: the window prepares the plan (on
//! macOS, a copy of the image the helper can read), starts the helper
//! through the system's prompt, reads the lines the helper appends to its
//! progress file, and learns how the helper exited.
//!
//! How the write ended comes from the helper's reports. Two endings say
//! nothing was written on the window's own word: [`Preparing::failed`],
//! before the helper was started, and [`Authorizing::exited`], when the
//! helper reported nothing. The second holds because the helper's first act
//! is to report, and it does nothing if that report can't be written
//! ([`execute`](crate::write::execute)). Once the helper has reported, only
//! its reports decide ([`Running::exited`]). Reports come only from the
//! helper's file ([`ProgressFile`]), and the helper's exit comes with the
//! lines still unread ([`ProgressFile::finish`]), so it can't overtake them.
//!
//! ```
//! # use kakehashi_core::{Drive, choice::{DriveList, DriveListing, ImageId}, write::Plan};
//! # use std::time::Instant;
//! # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
//! # let mut list = DriveList::new();
//! # let listing = list.request(Instant::now()).unwrap();
//! # list.listed(DriveListing::new(listing, Ok(vec![stick.clone()])));
//! # list.choose(&stick);
//! # let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
//! # let (review, shown) = list.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).unwrap();
//! # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap(), shown).unwrap();
//! # let dir = kakehashi_core::job::JobDir::new().unwrap();
//! # let path = dir.progress();
//! use kakehashi_core::job::{End, Event, Job, JobState, ProgressFile, Ran};
//!
//! let mut job = Job::start(&plan, &dir);
//! let mut reports = ProgressFile::new(&dir);
//! job.handle(Event::Authorizing, Instant::now());
//! // The helper reports that it is writing, and is killed before the
//! // window reads the line.
//! std::fs::write(&path, concat!(
//!     r#"{"stage":"preparing"}"#, "\n",
//!     r#"{"stage":"writing","done":1073741824,"total":4294967296}"#, "\n",
//! )).unwrap();
//! let exit = reports.finish(Ran::Failed("the helper was killed".into()));
//! job.handle(Event::Exited(exit), Instant::now());
//! let JobState::Ended(End::Failed(failure)) = job.stored() else { panic!() };
//! assert!(failure.touched(), "the drive was partly written");
//! ```

// Every arm names its variants, so a new state, event or reason is
// handled where it matters rather than falling into a catch-all.
#![deny(clippy::wildcard_enum_match_arm)]

use crate::drive::Drive;
use crate::write::{Plan, Progress};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How a run of the helper ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ran {
    /// The helper ran and exited with this code; its progress file says how
    /// the write went.
    Exited(i32),
    /// The person dismissed the prompt.
    Declined,
    /// The helper could not be started, or was killed.
    Failed(String),
}

impl Ran {
    /// What to say when the helper ended without a final report.
    fn message(self) -> String {
        match self {
            Ran::Exited(code) => format!("The helper stopped (code {code}) without saying why."),
            Ran::Declined => "The administrator prompt was dismissed.".to_owned(),
            Ran::Failed(message) => message,
        }
    }
}

/// A line the helper wrote to its progress file. Only [`ProgressFile`]
/// makes one, so the window can't report for the helper:
///
/// ```compile_fail,E0423
/// use kakehashi_core::{job::Report, write::Progress};
/// let _ = Report(Progress::Failed { message: "the helper was killed".into(), touched: false });
/// ```
///
/// A report the helper wrote:
///
/// ```
/// use kakehashi_core::{job::{JobDir, ProgressFile}, write::Progress};
/// let dir = JobDir::new().unwrap();
/// std::fs::write(dir.progress(), "{\"stage\":\"preparing\"}\n").unwrap();
/// let reports = ProgressFile::new(&dir).read();
/// assert_eq!(reports[0].progress(), &Progress::Preparing);
/// ```
#[derive(Debug)]
pub struct Report(Progress);

impl Report {
    pub fn progress(&self) -> &Progress {
        &self.0
    }
}

/// How the helper exited, with the lines it wrote that weren't read yet.
/// Only [`ProgressFile::finish`] makes one, so the exit can't be taken in
/// before the last of the helper's reports, as it would be if the window
/// sent it on its own:
///
/// ```compile_fail,E0308
/// # use kakehashi_core::{Drive, choice::{DriveList, DriveListing, ImageId}, write::Plan};
/// # use std::time::Instant;
/// # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
/// # let mut list = DriveList::new();
/// # let listing = list.request(Instant::now()).unwrap();
/// # list.listed(DriveListing::new(listing, Ok(vec![stick.clone()])));
/// # list.choose(&stick);
/// # let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
/// # let (review, shown) = list.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).unwrap();
/// # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap(), shown).unwrap();
/// use kakehashi_core::job::{Event, Job, Ran};
/// let dir = kakehashi_core::job::JobDir::new().unwrap();
/// let mut job = Job::start(&plan, &dir);
/// job.handle(Event::Authorizing, Instant::now());
/// // The helper was killed; its last lines are still in the file.
/// job.handle(Event::Exited(Ran::Failed("the helper was killed".into())), Instant::now());
/// ```
///
/// The exit, with the rest of the file:
///
/// ```
/// # use kakehashi_core::{Drive, choice::{DriveList, DriveListing, ImageId}, write::Plan};
/// # use std::time::Instant;
/// # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
/// # let mut list = DriveList::new();
/// # let listing = list.request(Instant::now()).unwrap();
/// # list.listed(DriveListing::new(listing, Ok(vec![stick.clone()])));
/// # list.choose(&stick);
/// # let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
/// # let (review, shown) = list.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).unwrap();
/// # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap(), shown).unwrap();
/// use kakehashi_core::job::{Event, Job, ProgressFile, Ran};
/// let dir = kakehashi_core::job::JobDir::new().unwrap();
/// let reports = ProgressFile::new(&dir);
/// let mut job = Job::start(&plan, &dir);
/// job.handle(Event::Authorizing, Instant::now());
/// job.handle(Event::Exited(reports.finish(Ran::Failed("the helper was killed".into()))), Instant::now());
/// assert!(job.is_ended());
/// ```
#[derive(Debug)]
pub struct Exit {
    tail: Vec<Report>,
    ran: Ran,
}

/// The job's private directory, removed when it is dropped: the plan the
/// helper reads, the progress file it appends to, and the marker that stops
/// it.
#[derive(Debug)]
pub struct JobDir {
    dir: tempfile::TempDir,
}

impl JobDir {
    /// A new private directory, with the empty progress file the helper
    /// appends to.
    pub fn new() -> std::io::Result<JobDir> {
        let dir = tempfile::Builder::new().prefix("kakehashi-").tempdir()?;
        File::create(dir.path().join("progress"))?;
        Ok(JobDir { dir })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Where the window writes the plan.
    pub fn plan(&self) -> PathBuf {
        self.dir.path().join("plan.json")
    }

    /// The progress file the helper appends to.
    pub fn progress(&self) -> PathBuf {
        self.dir.path().join("progress")
    }

    /// The marker that stops the helper: the progress file's name with
    /// `.cancel`, as the helper looks for it.
    pub fn cancel(&self) -> PathBuf {
        self.dir.path().join("progress.cancel")
    }
}

/// The helper's progress file, read as it grows: one JSON object per line.
/// It borrows the job's directory, so the directory can't be removed while
/// the helper's last lines are still to be read:
///
/// ```compile_fail,E0505
/// use kakehashi_core::job::{JobDir, ProgressFile, Ran};
/// let dir = JobDir::new().unwrap();
/// let reports = ProgressFile::new(&dir);
/// // The helper exited: remove its directory, then read what it wrote last.
/// drop(dir);
/// let _exit = reports.finish(Ran::Failed("the helper was killed".into()));
/// ```
///
/// Read what it wrote last, then remove the directory:
///
/// ```
/// use kakehashi_core::job::{JobDir, ProgressFile, Ran};
/// let dir = JobDir::new().unwrap();
/// let reports = ProgressFile::new(&dir);
/// let _exit = reports.finish(Ran::Failed("the helper was killed".into()));
/// drop(dir);
/// ```
#[derive(Debug)]
pub struct ProgressFile<'d> {
    dir: &'d JobDir,
    offset: u64,
    /// A line not finished yet.
    pending: String,
}

impl<'d> ProgressFile<'d> {
    pub fn new(dir: &'d JobDir) -> ProgressFile<'d> {
        ProgressFile { dir, offset: 0, pending: String::new() }
    }

    /// The whole lines the helper appended since the last read.
    pub fn read(&mut self) -> Vec<Report> {
        let mut reports = Vec::new();
        let Ok(mut file) = File::open(self.dir.progress()) else { return reports };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return reports;
        }
        let mut new = String::new();
        if let Ok(n) = file.read_to_string(&mut new) {
            self.offset += n as u64;
            self.pending.push_str(&new);
            while let Some(end) = self.pending.find('\n') {
                let line: String = self.pending.drain(..=end).collect();
                if let Ok(progress) = serde_json::from_str::<Progress>(line.trim()) {
                    reports.push(Report(progress));
                }
            }
        }
        reports
    }

    /// The helper exited as `ran`: that, with whatever it wrote since the
    /// last read. Call it once the helper's process is gone; it releases
    /// the directory.
    pub fn finish(mut self, ran: Ran) -> Exit {
        Exit { tail: self.read(), ran }
    }
}

/// What the job's states share: the write as the window started it.
#[derive(Debug)]
pub struct Write {
    drive: Drive,
    image: String,
    /// The marker that stops the helper at its next check.
    cancel: PathBuf,
    cancelling: bool,
}

impl Write {
    /// The drive written, as the person confirmed it.
    pub fn drive(&self) -> &Drive {
        &self.drive
    }

    /// The image's file name.
    pub fn image(&self) -> &str {
        &self.image
    }

    /// Whether Stop was pressed.
    pub fn cancelling(&self) -> bool {
        self.cancelling
    }

    fn stop(&mut self) {
        self.cancelling = true;
        // After the helper exited the directory is gone, and so is the need.
        let _ = File::create(&self.cancel);
    }
}

/// The helper's last report, and where the writing or verifying pass it
/// belongs to began.
#[derive(Debug)]
pub struct Pass {
    last: Progress,
    from: Option<(u64, Instant)>,
}

impl Pass {
    /// The helper's last report.
    pub fn last(&self) -> &Progress {
        &self.last
    }

    /// Bytes done when this writing or verifying pass was first reported,
    /// and when, for its rate.
    pub fn from(&self) -> Option<(u64, Instant)> {
        self.from
    }

    fn after(previous: Option<Pass>, Report(progress): Report, now: Instant) -> Pass {
        let from = match (&progress, previous) {
            (Progress::Writing { .. }, Some(Pass { last: Progress::Writing { .. }, from }))
            | (Progress::Verifying { .. }, Some(Pass { last: Progress::Verifying { .. }, from })) => from,
            (Progress::Writing { done, .. } | Progress::Verifying { done, .. }, None | Some(_)) => Some((*done, now)),
            (Progress::Preparing | Progress::Finished { .. } | Progress::Failed { .. }, None | Some(_)) => None,
        };
        Pass { last: progress, from }
    }

    fn is_final(&self) -> bool {
        matches!(self.last, Progress::Finished { .. } | Progress::Failed { .. })
    }
}

/// How a write ended.
#[derive(Debug)]
pub enum End {
    /// Written, read back and matched.
    Finished {
        ejected: bool,
        note: Option<String>,
    },
    Failed(Failure),
    /// The person dismissed the prompt, so the helper never ran.
    Declined,
}

/// A write that did not finish. Only the job's transitions make one, from
/// what the helper reported, so the window can't decide by itself that a
/// drive the helper had started on was left untouched:
///
/// ```compile_fail,E0451
/// use kakehashi_core::job::Failure;
/// let _ = Failure { message: "the helper was killed".into(), touched: false };
/// ```
///
/// The failure the job ended with:
///
/// ```
/// use kakehashi_core::job::{End, Failure, Job, JobState};
/// fn failure(job: &Job) -> Option<&Failure> {
///     match job.stored() {
///         JobState::Ended(End::Failed(failure)) => Some(failure),
///         _ => None,
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Failure {
    message: String,
    touched: bool,
}

impl Failure {
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The drive may have been changed: it is partly written.
    pub fn touched(&self) -> bool {
        self.touched
    }
}

aizome_typestate::machine! {
    /// A write as the window follows it.
    pub struct Job over Write {
        /// Where the job is, for drawing it.
        #[derive(Debug)]
        stored JobState;
        /// The job's typed state, holding the write.
        typed JobAny;
        states {
            /// The window prepares the image and the plan; the helper has
            /// not been started.
            Preparing,
            /// The helper is being started through the system's prompt and
            /// has reported nothing yet.
            Authorizing,
            /// The helper has reported.
            Running(Pass),
            /// The helper exited, or never started.
            Ended(End),
        }
        new(private);
        with_typed(pub);
    }
}

impl<'s> Preparing<'s> {
    /// The plan is written; the helper is being started.
    pub fn authorizing(self) -> Authorizing<'s> {
        Authorizing::enter(self.ctx, ())
    }

    /// Preparing failed or was stopped. The helper was never started, so
    /// the drive was not touched.
    pub fn failed(self, message: String) -> Ended<'s> {
        Ended::enter(self.ctx, End::Failed(Failure { message, touched: false }))
    }

    /// Stop: preparation checks the marker between steps.
    pub fn stop(&mut self) {
        self.ctx.stop();
    }
}

impl<'s> Authorizing<'s> {
    /// The helper's first report.
    pub fn report(self, report: Report, now: Instant) -> Running<'s> {
        Running::enter(self.ctx, Pass::after(None, report, now))
    }

    /// The helper ended without reporting. Its first act is to report, and
    /// it does nothing to the drive if it can't, so the drive was not
    /// touched.
    pub fn exited(self, ran: Ran) -> Ended<'s> {
        let end = match ran {
            Ran::Declined => End::Declined,
            ran @ (Ran::Exited(_) | Ran::Failed(_)) => End::Failed(Failure { message: ran.message(), touched: false }),
        };
        Ended::enter(self.ctx, end)
    }
}

impl<'s> Running<'s> {
    /// A later report.
    pub fn report(self, report: Report, now: Instant) -> Running<'s> {
        Running::enter(self.ctx, Pass::after(Some(self.data), report, now))
    }

    /// The helper exited. Its final report says how the write went. Without
    /// one, it stopped after it had started on the drive: the drive may have
    /// been changed, however the helper ended.
    ///
    /// There is no way to end a job whose helper has reported with a
    /// failure the window chose, as the window did when the helper was
    /// killed mid-write (KTL-03):
    ///
    /// ```compile_fail,E0599
    /// # use kakehashi_core::{Drive, choice::{DriveList, DriveListing, ImageId}, write::Plan};
    /// # use std::time::Instant;
    /// # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
    /// # let mut list = DriveList::new();
    /// # let listing = list.request(Instant::now()).unwrap();
    /// # list.listed(DriveListing::new(listing, Ok(vec![stick.clone()])));
    /// # list.choose(&stick);
    /// # let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
    /// # let (review, shown) = list.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).unwrap();
    /// # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap(), shown).unwrap();
    /// # let dir = kakehashi_core::job::JobDir::new().unwrap();
    /// # let path = dir.progress();
    /// # std::fs::write(&path, "{\"stage\":\"writing\",\"done\":1073741824,\"total\":4294967296}\n").unwrap();
    /// use kakehashi_core::job::{Event, Job, JobAny, ProgressFile};
    /// let mut job = Job::start(&plan, &dir);
    /// job.handle(Event::Authorizing, Instant::now());
    /// for report in ProgressFile::new(&dir).read() {
    ///     job.handle(Event::Report(report), Instant::now());
    /// }
    /// job.with_typed(|state| match state {
    ///     // The helper was killed: "not written", as before it started.
    ///     JobAny::Running(running) => (running.failed("the helper was killed".into()).into(), ()),
    ///     other => (other, ()),
    /// });
    /// ```
    ///
    /// The exit is the only way out, and the outcome comes from the
    /// reports:
    ///
    /// ```
    /// # use kakehashi_core::{Drive, choice::{DriveList, DriveListing, ImageId}, write::Plan};
    /// # use std::time::Instant;
    /// # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
    /// # let mut list = DriveList::new();
    /// # let listing = list.request(Instant::now()).unwrap();
    /// # list.listed(DriveListing::new(listing, Ok(vec![stick.clone()])));
    /// # list.choose(&stick);
    /// # let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
    /// # let (review, shown) = list.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).unwrap();
    /// # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap(), shown).unwrap();
    /// # let dir = kakehashi_core::job::JobDir::new().unwrap();
    /// # let path = dir.progress();
    /// # std::fs::write(&path, "{\"stage\":\"writing\",\"done\":1073741824,\"total\":4294967296}\n").unwrap();
    /// use kakehashi_core::job::{End, Event, Job, JobAny, JobState, ProgressFile, Ran};
    /// let mut job = Job::start(&plan, &dir);
    /// job.handle(Event::Authorizing, Instant::now());
    /// for report in ProgressFile::new(&dir).read() {
    ///     job.handle(Event::Report(report), Instant::now());
    /// }
    /// job.with_typed(|state| match state {
    ///     JobAny::Running(running) => (running.exited(Ran::Failed("the helper was killed".into())).into(), ()),
    ///     other => (other, ()),
    /// });
    /// assert!(matches!(job.stored(), JobState::Ended(End::Failed(f)) if f.touched()));
    /// ```
    pub fn exited(self, ran: Ran) -> Ended<'s> {
        let end = match self.data.last {
            Progress::Finished { ejected, note } => End::Finished { ejected, note },
            Progress::Failed { message, touched } => End::Failed(Failure { message, touched }),
            Progress::Preparing | Progress::Writing { .. } | Progress::Verifying { .. } => {
                End::Failed(Failure { message: ran.message(), touched: true })
            }
        };
        Ended::enter(self.ctx, end)
    }

    /// Stop: the helper checks the marker between chunks.
    pub fn stop(&mut self) {
        self.ctx.stop();
    }
}

/// What the window's write worker sends.
#[derive(Debug)]
pub enum Event {
    /// A line of the helper's progress file.
    Report(Report),
    /// The plan is written; the helper is being started.
    Authorizing,
    /// Preparing failed or was stopped; the helper was never started.
    NotStarted(String),
    /// The helper exited, with the lines it wrote that weren't sent yet.
    Exited(Exit),
}

/// One step of an [`Event`]: an exit is its unread reports, then the exit.
enum Step {
    Report(Report),
    Authorizing,
    NotStarted(String),
    Exited(Ran),
}

impl Job {
    /// Starts following the write of `plan`, from the job's directory `dir`.
    pub fn start<P>(plan: &Plan<P>, dir: &JobDir) -> Job {
        let image = plan.image().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Job::new(Write { drive: plan.drive().clone(), image, cancel: dir.cancel(), cancelling: false })
    }

    /// Takes in what the worker sent.
    pub fn handle(&mut self, event: Event, now: Instant) {
        match event {
            Event::Report(report) => self.step(Step::Report(report), now),
            Event::Authorizing => self.step(Step::Authorizing, now),
            Event::NotStarted(message) => self.step(Step::NotStarted(message), now),
            Event::Exited(Exit { tail, ran }) => {
                for report in tail {
                    self.step(Step::Report(report), now);
                }
                self.step(Step::Exited(ran), now);
            }
        }
    }

    /// Events the worker never sends in that state are taken the cautious
    /// way: a report or exit before `Authorizing` as coming from the
    /// helper, and `NotStarted` after it as the helper's exit.
    fn step(&mut self, step: Step, now: Instant) {
        self.with_typed(|state| {
            let next: JobAny<'_> = match (state, step) {
                (JobAny::Running(s), Step::Report(r)) => s.report(r, now).into(),
                (JobAny::Authorizing(s), Step::Report(r)) => s.report(r, now).into(),
                (JobAny::Preparing(s), Step::Report(r)) => s.authorizing().report(r, now).into(),
                (JobAny::Preparing(s), Step::Authorizing) => s.authorizing().into(),
                (JobAny::Preparing(s), Step::NotStarted(message)) => s.failed(message).into(),
                (JobAny::Preparing(s), Step::Exited(ran)) => s.authorizing().exited(ran).into(),
                (JobAny::Authorizing(s), Step::Exited(ran)) => s.exited(ran).into(),
                (JobAny::Authorizing(s), Step::NotStarted(message)) => s.exited(Ran::Failed(message)).into(),
                (JobAny::Running(s), Step::Exited(ran)) => s.exited(ran).into(),
                (JobAny::Running(s), Step::NotStarted(message)) => s.exited(Ran::Failed(message)).into(),
                (state @ (JobAny::Authorizing(_) | JobAny::Running(_)), Step::Authorizing) => state,
                (
                    state @ JobAny::Ended(_),
                    Step::Report(_) | Step::Authorizing | Step::NotStarted(_) | Step::Exited(_),
                ) => state,
            };
            (next, ())
        });
    }

    /// Stops the write, while preparing or while the helper runs; not while
    /// the system's prompt is up, nor once the helper's final report is in.
    pub fn stop(&mut self) {
        self.with_typed(|mut state| {
            match &mut state {
                JobAny::Preparing(s) => s.stop(),
                JobAny::Running(s) if !s.data.is_final() => s.stop(),
                // The prompt is up, the final report is in, or it is over.
                JobAny::Authorizing(_) | JobAny::Running(_) | JobAny::Ended(_) => {}
            }
            (state, ())
        });
    }

    /// Whether [`stop`](Self::stop) would stop it.
    pub fn can_stop(&self) -> bool {
        match self.stored() {
            JobState::Preparing(()) => true,
            JobState::Running(pass) => !pass.is_final(),
            JobState::Authorizing(()) | JobState::Ended(_) => false,
        }
    }

    /// Whether the job is over: the helper exited or never started.
    pub fn is_ended(&self) -> bool {
        matches!(self.stored(), JobState::Ended(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::choice::{DriveList, DriveListing, ImageId};
    use std::time::Duration;

    fn job(dir: &JobDir) -> Job {
        let stick = Drive {
            name: "sdb".into(),
            bytes: 32_000_000_000,
            serial: "4C53".into(),
            usb_or_removable: true,
            ..Drive::default()
        };
        let mut list = DriveList::new();
        let listing = list.request(Instant::now()).unwrap();
        list.listed(DriveListing::new(listing, Ok(vec![stick.clone()])));
        list.choose(&stick);
        let image = ImageId { path: "/images/sekka.iso".into(), sha256: "0".repeat(64) };
        let (review, shown) = list.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).unwrap();
        let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap(), shown).unwrap();
        Job::start(&plan, dir)
    }

    fn report(progress: Progress) -> Event {
        Event::Report(Report(progress))
    }

    fn exited(ran: Ran) -> Event {
        Event::Exited(Exit { tail: Vec::new(), ran })
    }

    fn run(events: Vec<Event>) -> Job {
        let dir = JobDir::new().unwrap();
        let mut job = job(&dir);
        for event in events {
            job.handle(event, Instant::now());
        }
        job
    }

    fn failure(job: &Job) -> (&str, bool) {
        match job.stored() {
            JobState::Ended(End::Failed(f)) => (f.message(), f.touched()),
            other @ (JobState::Preparing(_) | JobState::Authorizing(_) | JobState::Running(_) | JobState::Ended(_)) => {
                panic!("not failed: {other:?}")
            }
        }
    }

    const WRITING: Progress = Progress::Writing { done: 1 << 30, total: 4 << 30 };

    #[test]
    fn the_write_is_the_plans() {
        let dir = JobDir::new().unwrap();
        let job = job(&dir);
        assert_eq!(job.ctx().drive().serial, "4C53");
        assert_eq!(job.ctx().image(), "sekka.iso");
        assert!(matches!(job.stored(), JobState::Preparing(())));
    }

    #[test]
    fn a_helper_killed_after_reporting_leaves_the_drive_touched() {
        for last in [Progress::Preparing, WRITING, Progress::Verifying { done: 1, total: 2 }] {
            let job = run(vec![
                Event::Authorizing,
                report(last.clone()),
                exited(Ran::Failed("the helper was killed".into())),
            ]);
            assert_eq!(failure(&job), ("the helper was killed", true), "after {last:?}");
            let job = run(vec![Event::Authorizing, report(last.clone()), exited(Ran::Exited(1))]);
            assert!(failure(&job).1, "after {last:?}");
        }
    }

    #[test]
    fn the_exit_brings_the_lines_not_read_yet() {
        let dir = JobDir::new().unwrap();
        let path = dir.progress();
        let mut reports = ProgressFile::new(&dir);
        let mut job = job(&dir);
        job.handle(Event::Authorizing, Instant::now());
        assert!(reports.read().is_empty());
        // The helper reports and is killed between two polls.
        std::fs::write(&path, "{\"stage\":\"preparing\"}\n{\"stage\":\"writing\",\"done\":1,\"total\":2}\n").unwrap();
        job.handle(Event::Exited(reports.finish(Ran::Failed("the helper was killed".into()))), Instant::now());
        assert_eq!(failure(&job), ("the helper was killed", true));
    }

    #[test]
    fn the_helpers_final_report_decides() {
        let job = run(vec![
            Event::Authorizing,
            report(WRITING),
            report(Progress::Finished { ejected: true, note: None }),
            exited(Ran::Failed("the helper was killed".into())),
        ]);
        assert!(matches!(job.stored(), JobState::Ended(End::Finished { ejected: true, .. })));
        let job = run(vec![
            Event::Authorizing,
            report(Progress::Preparing),
            report(Progress::Failed { message: "gone".into(), touched: false }),
            exited(Ran::Exited(1)),
        ]);
        assert_eq!(failure(&job), ("gone", false));
    }

    #[test]
    fn nothing_is_touched_before_the_helper_reports() {
        let job = run(vec![Event::NotStarted("could not find the helper".into())]);
        assert_eq!(failure(&job), ("could not find the helper", false));
        let job = run(vec![Event::Authorizing, exited(Ran::Failed("not authorized".into()))]);
        assert_eq!(failure(&job), ("not authorized", false));
        let job = run(vec![Event::Authorizing, exited(Ran::Exited(2))]);
        assert_eq!(failure(&job), ("The helper stopped (code 2) without saying why.", false));
        let job = run(vec![Event::Authorizing, exited(Ran::Declined)]);
        assert!(matches!(job.stored(), JobState::Ended(End::Declined)));
    }

    #[test]
    fn a_late_not_started_is_the_helpers_exit() {
        let job = run(vec![Event::Authorizing, report(WRITING), Event::NotStarted("x".into())]);
        assert_eq!(failure(&job), ("x", true));
    }

    #[test]
    fn an_ended_job_stays_ended() {
        let mut job = run(vec![Event::NotStarted("stopped".into())]);
        job.handle(report(WRITING), Instant::now());
        job.handle(exited(Ran::Exited(0)), Instant::now());
        assert_eq!(failure(&job), ("stopped", false));
        assert!(job.is_ended() && !job.can_stop());
    }

    #[test]
    fn each_pass_has_its_own_rate_origin() {
        let t0 = Instant::now();
        let mut job = run(vec![Event::Authorizing]);
        job.handle(report(Progress::Writing { done: 8, total: 100 }), t0);
        job.handle(report(Progress::Writing { done: 50, total: 100 }), t0 + Duration::from_secs(1));
        let JobState::Running(pass) = job.stored() else { panic!() };
        assert_eq!(pass.from(), Some((8, t0)));
        let t1 = t0 + Duration::from_secs(2);
        job.handle(report(Progress::Verifying { done: 8, total: 100 }), t1);
        let JobState::Running(pass) = job.stored() else { panic!() };
        assert_eq!(pass.from(), Some((8, t1)));
    }

    #[test]
    fn stop_creates_the_marker_except_while_authorizing_or_done() {
        let dir = JobDir::new().unwrap();
        let cancel = dir.cancel();
        let mut j = job(&dir);
        j.handle(Event::Authorizing, Instant::now());
        assert!(!j.can_stop());
        j.stop();
        assert!(!cancel.exists() && !j.ctx().cancelling());
        j.handle(report(WRITING), Instant::now());
        assert!(j.can_stop());
        j.stop();
        assert!(cancel.exists() && j.ctx().cancelling());
        j.handle(report(Progress::Finished { ejected: true, note: None }), Instant::now());
        assert!(!j.can_stop());
    }

    #[test]
    fn the_progress_file_is_read_in_whole_lines() {
        let dir = JobDir::new().unwrap();
        let path = dir.progress();
        let mut file = ProgressFile::new(&dir);
        assert!(file.read().is_empty(), "no file yet");
        std::fs::write(&path, "{\"stage\":\"preparing\"}\n{\"stage\":\"wri").unwrap();
        let first: Vec<_> = file.read().into_iter().map(|r| r.0).collect();
        assert_eq!(first, [Progress::Preparing]);
        std::fs::write(&path, "{\"stage\":\"preparing\"}\n{\"stage\":\"writing\",\"done\":1,\"total\":2}\nnot json\n")
            .unwrap();
        let next: Vec<_> = file.read().into_iter().map(|r| r.0).collect();
        assert_eq!(next, [Progress::Writing { done: 1, total: 2 }]);
    }
}
