//! A write as the window follows it: the window prepares the plan (on
//! macOS, a copy of the image the helper can read), starts the helper
//! through the system's prompt, reads the lines the helper appends to its
//! progress file, and learns how the helper exited.
//!
//! How the write ended comes from the helper's reports. Before the helper
//! starts, the window may end the job as not written
//! ([`Preparing::failed`]); once the helper has reported, only its final
//! report or, without one, the stage it last reported decides whether the
//! drive may have been changed ([`Running::exited`]). Reports are read from
//! the helper's file ([`ProgressFile`]); the window can't make one up.
//!
//! ```
//! # use kakehashi_core::{Drive, choice::DriveList, write::Plan};
//! # use std::time::Instant;
//! # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
//! # let mut list = DriveList::new();
//! # let listing = list.request(Instant::now()).unwrap();
//! # list.listed(kakehashi_core::choice::DriveListing::new(listing, Ok(vec![stick.clone()])));
//! # list.choose(&stick);
//! # let image = kakehashi_core::choice::ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
//! # let (review, _) = list.open_review(None, image.clone()).unwrap();
//! # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap());
//! # let dir = tempfile::tempdir().unwrap();
//! # let path = dir.path().join("progress");
//! use kakehashi_core::job::{End, Event, Job, JobState, ProgressFile, Ran, Write};
//!
//! let mut job = Job::new(Write::new(&plan, path.with_extension("cancel")));
//! job.handle(Event::Authorizing, Instant::now());
//! // The helper reports that it is writing, then is killed.
//! std::fs::write(&path, concat!(
//!     r#"{"stage":"preparing"}"#, "\n",
//!     r#"{"stage":"writing","done":1073741824,"total":4294967296}"#, "\n",
//! )).unwrap();
//! for report in ProgressFile::new(path).read() {
//!     job.handle(Event::Report(report), Instant::now());
//! }
//! job.handle(Event::Exited(Ran::Failed("the helper was killed".into())), Instant::now());
//! let JobState::Ended(End::Failed(failure)) = job.stored() else { panic!() };
//! assert!(failure.touched(), "the drive was partly written");
//! ```

use crate::drive::Drive;
use crate::write::{Plan, Progress};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
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

/// A line the helper wrote to its progress file. Only
/// [`ProgressFile::read`] makes one, so the window can't report for the
/// helper:
///
/// ```compile_fail,E0423
/// use kakehashi_core::{job::Report, write::Progress};
/// let _ = Report(Progress::Failed { message: "the helper was killed".into(), touched: false });
/// ```
#[derive(Debug)]
pub struct Report(Progress);

impl Report {
    pub fn progress(&self) -> &Progress {
        &self.0
    }
}

/// The helper's progress file, read as it grows: one JSON object per line.
#[derive(Debug)]
pub struct ProgressFile {
    path: PathBuf,
    offset: u64,
    /// A line not finished yet.
    pending: String,
}

impl ProgressFile {
    pub fn new(path: PathBuf) -> ProgressFile {
        ProgressFile { path, offset: 0, pending: String::new() }
    }

    /// The whole lines the helper appended since the last read.
    pub fn read(&mut self) -> Vec<Report> {
        let mut reports = Vec::new();
        let Ok(mut file) = File::open(&self.path) else { return reports };
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
    /// The write of `plan`, stopped by creating `cancel`.
    pub fn new(plan: &Plan, cancel: PathBuf) -> Write {
        Write {
            drive: plan.drive().clone(),
            image: plan.image().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            cancel,
            cancelling: false,
        }
    }

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
            (Progress::Writing { done, .. } | Progress::Verifying { done, .. }, _) => Some((*done, now)),
            _ => None,
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
        with_typed(pub);
    }
}

impl<'s> Preparing<'s> {
    /// The plan is written; the helper is being started.
    pub fn authorizing(self) -> Authorizing<'s> {
        Authorizing::enter(self.ctx, ())
    }

    /// Preparing failed or was stopped. The helper never started, so the
    /// drive was not touched.
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

    /// The helper ended without reporting. It reports before it looks at
    /// the drive, so the drive was not touched.
    pub fn exited(self, ran: Ran) -> Ended<'s> {
        let end = match ran {
            Ran::Declined => End::Declined,
            ran => End::Failed(Failure { message: ran.message(), touched: false }),
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
    /// # use kakehashi_core::{Drive, choice::DriveList, write::Plan};
    /// # use std::time::Instant;
    /// # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
    /// # let mut list = DriveList::new();
    /// # let listing = list.request(Instant::now()).unwrap();
    /// # list.listed(kakehashi_core::choice::DriveListing::new(listing, Ok(vec![stick.clone()])));
    /// # list.choose(&stick);
    /// # let image = kakehashi_core::choice::ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
    /// # let (review, _) = list.open_review(None, image.clone()).unwrap();
    /// # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap());
    /// # let dir = tempfile::tempdir().unwrap();
    /// # let path = dir.path().join("progress");
    /// # std::fs::write(&path, "{\"stage\":\"writing\",\"done\":1073741824,\"total\":4294967296}\n").unwrap();
    /// use kakehashi_core::job::{Event, Job, JobAny, ProgressFile, Write};
    /// let mut job = Job::new(Write::new(&plan, path.with_extension("cancel")));
    /// job.handle(Event::Authorizing, Instant::now());
    /// for report in ProgressFile::new(path).read() {
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
    /// # use kakehashi_core::{Drive, choice::DriveList, write::Plan};
    /// # use std::time::Instant;
    /// # let stick = Drive { name: "sdb".into(), bytes: 32_000_000_000, serial: "4C53".into(), usb_or_removable: true, ..Drive::default() };
    /// # let mut list = DriveList::new();
    /// # let listing = list.request(Instant::now()).unwrap();
    /// # list.listed(kakehashi_core::choice::DriveListing::new(listing, Ok(vec![stick.clone()])));
    /// # list.choose(&stick);
    /// # let image = kakehashi_core::choice::ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
    /// # let (review, _) = list.open_review(None, image.clone()).unwrap();
    /// # let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap());
    /// # let dir = tempfile::tempdir().unwrap();
    /// # let path = dir.path().join("progress");
    /// # std::fs::write(&path, "{\"stage\":\"writing\",\"done\":1073741824,\"total\":4294967296}\n").unwrap();
    /// use kakehashi_core::job::{End, Event, Job, JobAny, JobState, ProgressFile, Ran, Write};
    /// let mut job = Job::new(Write::new(&plan, path.with_extension("cancel")));
    /// job.handle(Event::Authorizing, Instant::now());
    /// for report in ProgressFile::new(path).read() {
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
    /// The helper exited. Every line it wrote was sent before this.
    Exited(Ran),
}

impl Job {
    /// Takes in what the worker sent. Events the worker never sends in that
    /// state are taken the cautious way: a report or exit before
    /// `Authorizing` as coming from the helper, and `NotStarted` after it as
    /// the helper's exit.
    pub fn handle(&mut self, event: Event, now: Instant) {
        self.with_typed(|state| {
            let next: JobAny<'_> = match (state, event) {
                (JobAny::Running(s), Event::Report(r)) => s.report(r, now).into(),
                (JobAny::Authorizing(s), Event::Report(r)) => s.report(r, now).into(),
                (JobAny::Preparing(s), Event::Report(r)) => s.authorizing().report(r, now).into(),
                (JobAny::Preparing(s), Event::Authorizing) => s.authorizing().into(),
                (JobAny::Preparing(s), Event::NotStarted(message)) => s.failed(message).into(),
                (JobAny::Preparing(s), Event::Exited(ran)) => s.authorizing().exited(ran).into(),
                (JobAny::Authorizing(s), Event::Exited(ran)) => s.exited(ran).into(),
                (JobAny::Authorizing(s), Event::NotStarted(message)) => s.exited(Ran::Failed(message)).into(),
                (JobAny::Running(s), Event::Exited(ran)) => s.exited(ran).into(),
                (JobAny::Running(s), Event::NotStarted(message)) => s.exited(Ran::Failed(message)).into(),
                (state @ (JobAny::Authorizing(_) | JobAny::Running(_)), Event::Authorizing) => state,
                (state @ JobAny::Ended(_), _) => state,
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
                _ => {}
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

    fn job(cancel: PathBuf) -> Job {
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
        let (review, _) = list.open_review(None, image.clone()).unwrap();
        let plan = Plan::new(list.confirm(review, None, Some(image)).unwrap());
        Job::new(Write::new(&plan, cancel))
    }

    fn report(progress: Progress) -> Event {
        Event::Report(Report(progress))
    }

    fn run(events: Vec<Event>) -> Job {
        let mut job = job("/nonexistent/progress.cancel".into());
        for event in events {
            job.handle(event, Instant::now());
        }
        job
    }

    fn failure(job: &Job) -> (&str, bool) {
        match job.stored() {
            JobState::Ended(End::Failed(f)) => (f.message(), f.touched()),
            other => panic!("not failed: {other:?}"),
        }
    }

    const WRITING: Progress = Progress::Writing { done: 1 << 30, total: 4 << 30 };

    #[test]
    fn the_write_is_the_plans() {
        let job = job("/nonexistent/progress.cancel".into());
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
                Event::Exited(Ran::Failed("the helper was killed".into())),
            ]);
            assert_eq!(failure(&job), ("the helper was killed", true), "after {last:?}");
            let job = run(vec![Event::Authorizing, report(last.clone()), Event::Exited(Ran::Exited(1))]);
            assert!(failure(&job).1, "after {last:?}");
        }
    }

    #[test]
    fn the_helpers_final_report_decides() {
        let job = run(vec![
            Event::Authorizing,
            report(WRITING),
            report(Progress::Finished { ejected: true, note: None }),
            Event::Exited(Ran::Failed("the helper was killed".into())),
        ]);
        assert!(matches!(job.stored(), JobState::Ended(End::Finished { ejected: true, .. })));
        let job = run(vec![
            Event::Authorizing,
            report(Progress::Preparing),
            report(Progress::Failed { message: "gone".into(), touched: false }),
            Event::Exited(Ran::Exited(1)),
        ]);
        assert_eq!(failure(&job), ("gone", false));
    }

    #[test]
    fn nothing_is_touched_before_the_helper_reports() {
        let job = run(vec![Event::NotStarted("could not find the helper".into())]);
        assert_eq!(failure(&job), ("could not find the helper", false));
        let job = run(vec![Event::Authorizing, Event::Exited(Ran::Failed("not authorized".into()))]);
        assert_eq!(failure(&job), ("not authorized", false));
        let job = run(vec![Event::Authorizing, Event::Exited(Ran::Exited(2))]);
        assert_eq!(failure(&job), ("The helper stopped (code 2) without saying why.", false));
        let job = run(vec![Event::Authorizing, Event::Exited(Ran::Declined)]);
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
        job.handle(Event::Exited(Ran::Exited(0)), Instant::now());
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
        let dir = tempfile::tempdir().unwrap();
        let cancel = dir.path().join("progress.cancel");
        let mut j = job(cancel.clone());
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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("progress");
        let mut file = ProgressFile::new(path.clone());
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
