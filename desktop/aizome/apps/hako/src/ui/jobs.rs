//! Operations that take a while, each on a thread of its own: what they
//! are, how far they've come, the questions they're waiting on, and
//! what they did (for undo, and for the views to show).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aizome_kit::{MsgSender, WindowKey};

use crate::fsops::archive::{self, Format};
use crate::fsops::transfer::{self, Mode};
use crate::fsops::trash::{self, Trash};
use crate::fsops::undo::{self, Record, Step, items_label};
use crate::fsops::{Asker, Conflict, Control, Env, ErrorChoice, Failure, Progress, Report, Reporter, Resolution};

/// What a job does.
#[derive(Clone, Debug)]
pub enum Work {
    Transfer {
        sources: Vec<PathBuf>,
        dest: PathBuf,
        mode: Mode,
    },
    Duplicate {
        sources: Vec<PathBuf>,
    },
    Trash {
        paths: Vec<PathBuf>,
    },
    Restore {
        entries: Vec<trash::Entry>,
    },
    Erase {
        entries: Vec<trash::Entry>,
    },
    EmptyTrash,
    Compress {
        items: Vec<PathBuf>,
        archive: PathBuf,
        format: Format,
    },
    Extract {
        archive: PathBuf,
        dest: PathBuf,
    },
    /// Undo (or redo, `redo`) a step.
    Invert {
        step: Step,
        redo: bool,
    },
}

fn dest_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

impl Work {
    /// What it's called while it runs: "Copying 3 items to “Downloads”".
    pub fn title(&self) -> String {
        match self {
            Work::Transfer { sources, dest, mode } => {
                let verb = if *mode == Mode::Copy { "Copying" } else { "Moving" };
                format!("{verb} {} to “{}”", what(sources), dest_name(dest))
            }
            Work::Duplicate { sources } => format!("Duplicating {}", what(sources)),
            Work::Trash { paths } => format!("Moving {} to the Trash", what(paths)),
            Work::Restore { entries } => match entries.as_slice() {
                [one] => format!("Putting back “{}”", one.display_name()),
                _ => format!("Putting back {} items", entries.len()),
            },
            Work::Erase { entries } => format!(
                "Deleting {} for good",
                if entries.len() == 1 { "1 item".to_owned() } else { format!("{} items", entries.len()) }
            ),
            Work::EmptyTrash => "Emptying the Trash".into(),
            Work::Compress { archive, .. } => format!("Compressing into “{}”", dest_name(archive)),
            Work::Extract { archive, .. } => format!("Extracting “{}”", dest_name(archive)),
            Work::Invert { step, redo } => format!("{} {}", if *redo { "Redoing" } else { "Undoing" }, step.label),
        }
    }
}

fn what(paths: &[PathBuf]) -> String {
    match paths {
        [one] => format!("“{}”", dest_name(one)),
        _ => format!("{} items", paths.len()),
    }
}

/// A question a job waits on.
#[derive(Clone, Debug, PartialEq)]
pub enum Question {
    Conflict(Conflict),
    Failure(Failure),
}

/// The person's answer, and whether it goes for the rest of the job's
/// questions of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Conflict(Resolution, bool),
    Error(ErrorChoice, bool),
}

/// What a job did.
#[derive(Clone, Debug)]
pub struct Done {
    pub report: Report,
    /// The step that undoes it (or, for an undo, redoes it).
    pub step: Option<Step>,
    /// An undo (false) or redo (true) this was.
    pub inverted: Option<bool>,
    /// Folders whose listings changed.
    pub changed: Vec<PathBuf>,
}

/// Messages from jobs to the program.
#[derive(Clone, Debug)]
pub enum JobEvent {
    Progress(Progress),
    Ask(Question),
    Done(Arc<Done>),
}

/// A job, as the program keeps it.
pub struct Job {
    pub id: u64,
    pub title: String,
    pub window: WindowKey,
    pub control: Arc<Control>,
    pub progress: Progress,
    pub question: Option<Question>,
    pub sheet: Option<WindowKey>,
    pub answers: mpsc::Sender<Answer>,
    pub started: Instant,
}

impl Job {
    pub fn paused(&self) -> bool {
        self.control.is_paused()
    }

    /// "12 MB of 340 MB", "3 of 12 items", "Preparing…".
    pub fn status(&self) -> String {
        let p = &self.progress;
        if self.question.is_some() {
            return "Waiting for an answer".into();
        }
        if self.paused() {
            return "Paused".into();
        }
        if p.preparing {
            return format!("Preparing ({} items)…", p.items_total);
        }
        if p.bytes_total > 0 {
            let mut s = format!(
                "{} of {}",
                crate::model::format::size(p.bytes_done),
                crate::model::format::size(p.bytes_total)
            );
            let secs = self.started.elapsed().as_secs_f64();
            if secs > 1.0 && p.bytes_done > 0 {
                let rate = p.bytes_done as f64 / secs;
                let left = (p.bytes_total.saturating_sub(p.bytes_done)) as f64 / rate;
                s.push_str(&format!(" — {}/s, {} left", crate::model::format::size(rate as u64), remaining(left)));
            }
            s
        } else if p.items_total > 0 {
            format!("{} of {} items", p.items_done.min(p.items_total), p.items_total)
        } else {
            "Working…".into()
        }
    }
}

fn remaining(secs: f64) -> String {
    let s = secs.round() as u64;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s.div_ceil(60)),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// Start `work` on a thread; what it says comes to `send` as job `id`'s
/// events.
pub fn start<Msg: Send + 'static>(
    id: u64,
    work: Work,
    trash: Trash,
    tx: MsgSender<Msg>,
    wrap: impl Fn(u64, JobEvent) -> Msg + Send + Sync + 'static,
) -> (Arc<Control>, mpsc::Sender<Answer>) {
    let control = Arc::new(Control::new());
    let (atx, arx) = mpsc::channel::<Answer>();
    let c = control.clone();
    let wrap = Arc::new(wrap);
    let _ = std::thread::Builder::new().name(format!("hako-job-{id}")).spawn(move || {
        let send = {
            let (tx, wrap) = (tx.clone(), wrap.clone());
            move |e: JobEvent| tx.send(wrap(id, e))
        };
        let mut asker = UiAsker { send: &send, answers: arx, conflicts: None, errors: None };
        let mut reporter = Throttle { send: &send, last: None };
        let mut env = Env::new(&c, &mut asker, &mut reporter);
        let done = run(work, &trash, &mut env);
        send(JobEvent::Done(Arc::new(done)));
    });
    (control, atx)
}

/// Carry out `work`.
pub fn run(work: Work, trash: &Trash, env: &mut Env) -> Done {
    let parents = |ps: &[PathBuf]| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = ps.iter().filter_map(|p| p.parent().map(PathBuf::from)).collect();
        v.sort();
        v.dedup();
        v
    };
    match work {
        Work::Transfer { sources, dest, mode } => {
            let report = transfer::transfer(&sources, &dest, mode, env, Some(trash));
            let mut changed = parents(&sources);
            changed.push(dest.clone());
            let step = match mode {
                Mode::Copy => made(&report, "Copy"),
                Mode::Move => {
                    let pairs: Vec<(PathBuf, PathBuf)> = report.done.iter().filter(|(a, b)| a != b).cloned().collect();
                    let label =
                        format!("Move of {}", items_label(&pairs.iter().map(|p| p.0.clone()).collect::<Vec<_>>()));
                    Some(Step { record: Record::Move { pairs }, label })
                }
            };
            Done { report, step, inverted: None, changed }
        }
        Work::Duplicate { sources } => {
            let report = transfer::duplicate(&sources, env);
            Done { step: made(&report, "Duplicate"), changed: parents(&sources), report, inverted: None }
        }
        Work::Trash { paths } => {
            let (report, items) = transfer::trash_items(&paths, trash, env);
            let label = format!(
                "Move to Trash of {}",
                items_label(&items.iter().map(|t| t.original.clone()).collect::<Vec<_>>())
            );
            let mut changed = parents(&paths);
            changed.extend(items.iter().filter_map(|t| t.file.parent().map(PathBuf::from)));
            Done { report, step: Some(Step { record: Record::Trash { items }, label }), inverted: None, changed }
        }
        Work::Restore { entries } => {
            let report = transfer::restore(&entries, trash, env);
            let paths: Vec<PathBuf> = report.done.iter().map(|(_, to)| to.clone()).collect();
            let mut changed = parents(&paths);
            changed.extend(entries.iter().filter_map(|e| e.file.parent().map(PathBuf::from)));
            let label = format!("Put Back of {}", items_label(&paths));
            Done { report, step: Some(Step { record: Record::Made { paths }, label }), inverted: None, changed }
        }
        Work::Erase { entries } => {
            let report = transfer::erase(&entries, trash, env);
            let changed = entries.iter().filter_map(|e| e.file.parent().map(PathBuf::from)).collect();
            Done { report, step: None, inverted: None, changed }
        }
        Work::EmptyTrash => {
            let changed = trash.cans().iter().map(|c| c.files()).collect();
            let report = transfer::empty_trash(trash, env);
            Done { report, step: None, inverted: None, changed }
        }
        Work::Compress { items, archive: to, format } => {
            let report = archive::compress(&items, &to, format, env);
            let step = report
                .done
                .first()
                .map(|_| Step { record: Record::Made { paths: vec![to.clone()] }, label: "Compress".into() });
            Done { report, step, inverted: None, changed: parents(std::slice::from_ref(&to)) }
        }
        Work::Extract { archive: from, dest } => {
            let report = archive::extract(&from, &dest, env);
            Done { step: made(&report, "Extract"), report, inverted: None, changed: vec![dest] }
        }
        Work::Invert { step, redo } => {
            let (report, record) = undo::invert(&step.record, trash, env);
            let mut touched: Vec<PathBuf> = report.done.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect();
            touched.extend(report.failed.iter().map(|(a, _)| a.clone()));
            let changed = parents(&touched);
            Done { report, step: Some(Step { record, label: step.label }), inverted: Some(redo), changed }
        }
    }
}

fn made(report: &Report, label: &str) -> Option<Step> {
    let paths: Vec<PathBuf> = report.done.iter().map(|(_, to)| to.clone()).collect();
    (!paths.is_empty()).then(|| Step { record: Record::Made { paths }, label: label.to_owned() })
}

/// Asks the person, through the program, and waits for the answer.
struct UiAsker<'a> {
    send: &'a dyn Fn(JobEvent) -> bool,
    answers: mpsc::Receiver<Answer>,
    conflicts: Option<Resolution>,
    errors: Option<ErrorChoice>,
}

impl Asker for UiAsker<'_> {
    fn conflict(&mut self, c: &Conflict) -> Resolution {
        if let Some(r) = self.conflicts {
            return if r == Resolution::Merge && !c.can_merge() { Resolution::KeepBoth } else { r };
        }
        if !(self.send)(JobEvent::Ask(Question::Conflict(c.clone()))) {
            return Resolution::Cancel;
        }
        loop {
            match self.answers.recv() {
                Ok(Answer::Conflict(r, all)) => {
                    if all {
                        self.conflicts = Some(r);
                    }
                    return r;
                }
                Ok(_) => continue,
                Err(_) => return Resolution::Cancel,
            }
        }
    }

    fn error(&mut self, f: &Failure) -> ErrorChoice {
        if let Some(e) = self.errors {
            return e;
        }
        if !(self.send)(JobEvent::Ask(Question::Failure(f.clone()))) {
            return ErrorChoice::Cancel;
        }
        loop {
            match self.answers.recv() {
                Ok(Answer::Error(e, all)) => {
                    if all && e != ErrorChoice::Retry {
                        self.errors = Some(e);
                    }
                    return e;
                }
                Ok(_) => continue,
                Err(_) => return ErrorChoice::Cancel,
            }
        }
    }
}

/// Progress, at most every 50 ms.
struct Throttle<'a> {
    send: &'a dyn Fn(JobEvent) -> bool,
    last: Option<(Instant, bool)>,
}

impl Reporter for Throttle<'_> {
    fn report(&mut self, p: &Progress) {
        let now = Instant::now();
        let due = match self.last {
            None => true,
            Some((t, preparing)) => preparing != p.preparing || now.duration_since(t) >= Duration::from_millis(50),
        };
        if due {
            self.last = Some((now, p.preparing));
            (self.send)(JobEvent::Progress(p.clone()));
        }
    }
}

/// The items' transfer as drops make it: into `dest`, moving within a
/// filesystem and copying across them (or copying when `copy`).
pub fn drop_mode(sources: &[PathBuf], dest: &std::path::Path, copy: bool) -> Mode {
    if copy {
        return Mode::Copy;
    }
    let dev = |p: &std::path::Path| std::fs::metadata(p).ok().map(|m| std::os::unix::fs::MetadataExt::dev(&m));
    let d = dev(dest);
    if sources.iter().all(|s| s.parent().and_then(dev) == d) { Mode::Move } else { Mode::Copy }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::TempDir;
    use crate::fsops::{Always, sys};

    #[test]
    fn runs_work_and_records_its_undo() {
        let t = TempDir::new("jobs");
        let tr =
            Trash { home: t.join("Trash"), home_dir: Some(t.join("home")), uid: sys::uid(), topdirs: Some(Vec::new()) };
        let a = t.file("home/a.txt", "a");
        let dest = t.dir("home/dest");
        let control = Control::new();
        let mut asker = Always(Resolution::KeepBoth, ErrorChoice::Skip);
        let mut quiet = ();
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        let done =
            run(Work::Transfer { sources: vec![a.clone()], dest: dest.clone(), mode: Mode::Move }, &tr, &mut env);
        assert!(done.report.is_clean());
        assert!(done.changed.contains(&dest) && done.changed.contains(&t.join("home")));
        let step = done.step.unwrap();
        assert_eq!(step.label, "Move of “a.txt”");
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        let undone = run(Work::Invert { step, redo: false }, &tr, &mut env);
        assert!(undone.report.is_clean());
        assert!(a.exists());
        assert_eq!(undone.inverted, Some(false));
        assert_eq!(Work::Trash { paths: vec![a.clone()] }.title(), "Moving “a.txt” to the Trash");
        assert_eq!(drop_mode(&[a], &dest, false), Mode::Move);
    }
}
