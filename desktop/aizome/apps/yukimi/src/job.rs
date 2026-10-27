// SPDX-License-Identifier: MIT OR Apache-2.0
//! Running an operation: its steps run in a thread of their own, and what
//! the window shows of it (what Nix is doing right now, how far along it
//! is, and its log) comes from there now and then, with only the log lines
//! that are new, so a busy build never keeps the window from answering.
//!
//! The worker and the words for failures are the GTK Yukimi's (Sekka's
//! `ui/job.rs`); [`Job`] is what the window keeps of one while it runs.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use yukimi_system::log::Progress;
use yukimi_system::nix;

use crate::ops::{Operation, Step};

/// How long Nix may say nothing before the window mentions it.
pub const QUIET: Duration = Duration::from_secs(180);
/// How often the window hears how things are going.
pub const EVERY: Duration = Duration::from_millis(150);
/// How many log lines the window keeps.
pub const KEPT_LINES: usize = 1000;
/// How long a finished job's sheet stays, so its outcome can be read.
pub const LINGER: Duration = Duration::from_millis(900);

/// What the window shows, taken from the progress now and then.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub current: Option<String>,
    pub summary: String,
    pub fraction: Option<f64>,
    /// Lines logged since the last snapshot.
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JobEvent {
    /// A step starts a command; `as_root` when it asks for permission first.
    Started {
        as_root: bool,
    },
    Progress(Snapshot),
    RecoveringAuthentication,
    AuthorizationFailed(AuthorizationFailure),
    Done(Result<String, String>),
}

/// These failures are identified from pkexec's complete output and exit
/// status, before the privileged helper has run. Never infer this from a
/// substring of a build's log (or from exit status 126/127 alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorizationFailure {
    Unavailable,
    Cancelled,
    Denied,
}

impl AuthorizationFailure {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unavailable => {
                "The administrator password dialog is unavailable. Nothing was changed. Try again when it is available."
            }
            Self::Cancelled => "Cancelled. Nothing was changed.",
            Self::Denied => "Administrator permission was denied. Nothing was changed.",
        }
    }
}

fn authorization_failure(error: &nix::Error) -> Option<AuthorizationFailure> {
    let nix::Error::CommandFailed { code, stdout, stderr: Some(stderr), .. } = error else { return None };
    if !stdout.is_empty() {
        return None;
    }
    match (*code, stderr.trim()) {
        (Some(127), "Error executing command as another user: No authentication agent found.") => {
            Some(AuthorizationFailure::Unavailable)
        }
        (Some(127), diagnostic)
            if !diagnostic.contains('\n')
                && diagnostic.starts_with(&format!("Error checking for authorization {}: ", crate::ops::ACTION)) =>
        {
            Some(AuthorizationFailure::Unavailable)
        }
        (Some(127), diagnostic)
            if !diagnostic.contains('\n') && diagnostic.starts_with("Error getting authority: ") =>
        {
            Some(AuthorizationFailure::Unavailable)
        }
        (Some(126), "Error executing command as another user: Request dismissed") => {
            Some(AuthorizationFailure::Cancelled)
        }
        (Some(127), "Error executing command as another user: Not authorized\n\nThis incident has been reported.") => {
            Some(AuthorizationFailure::Denied)
        }
        _ => None,
    }
}

/// Only the helper can confirm restoration. Unknown failures must not
/// claim that configuration or the running system was rolled back.
pub fn explain(error: &str, system: bool, stopped: bool) -> String {
    if error == crate::ops::NO_HELPER || error.contains("System changes had already begun") {
        return error.to_owned();
    }
    if stopped {
        return if system {
            format!("Stopped. {error}\n\nCheck Details for the operation's outcome before trying again.")
        } else {
            "Stopped. Check Details for the operation's outcome.".to_owned()
        };
    }
    error.to_owned()
}

impl Snapshot {
    /// A later snapshot taken in: its state, and its lines after these.
    fn merge(&mut self, later: Snapshot) {
        self.current = later.current;
        self.summary = later.summary;
        self.fraction = later.fraction.or(self.fraction);
        self.lines.extend(later.lines);
    }
}

/// What to show of the progress, with the lines logged since `seen`.
fn snapshot(progress: &Progress, seen: &mut u64) -> Snapshot {
    let lines = progress.since(*seen).map(str::to_owned).collect();
    *seen = progress.logged;
    Snapshot { current: progress.current(), summary: progress.summary(), fraction: progress.fraction(), lines }
}

/// Run the steps in order, telling `send` how it goes. `finish` turns the
/// last step's output into the report (Yukimi asks the running machine
/// whether an update needs a restart to finish).
pub fn work(steps: Vec<Step>, stop: nix::Stop, send: impl Fn(JobEvent), finish: impl FnOnce(String) -> String) {
    let mut out = String::new();
    for step in steps {
        if stop.requested() {
            send(JobEvent::Done(Err("Stopped".into())));
            return;
        }
        let mut run = match step() {
            Ok(Some(run)) => run,
            Ok(None) => continue,
            Err(e) => {
                send(JobEvent::Done(Err(e)));
                return;
            }
        };
        stop.signal(!run.as_root);
        send(JobEvent::Started { as_root: run.as_root });
        let recovery_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let (mut last, mut seen) = (Instant::now() - EVERY, 0u64);
            let mut pending: Option<Snapshot> = None;
            let result = nix::stream_command(&mut run.command, &stop, |progress| {
                let newer = snapshot(progress, &mut seen);
                match &mut pending {
                    Some(p) => p.merge(newer),
                    None => pending = Some(newer),
                }
                if last.elapsed() >= EVERY {
                    last = Instant::now();
                    if let Some(p) = pending.take() {
                        send(JobEvent::Progress(p));
                    }
                }
            });
            if let Some(p) = pending.take() {
                send(JobEvent::Progress(p));
            }
            match result {
                Ok((output, progress)) => {
                    send(JobEvent::Progress(snapshot(&progress, &mut seen)));
                    out = output;
                    break;
                }
                Err(error) => {
                    if run.as_root
                        && let Some(failure) = authorization_failure(&error)
                    {
                        if stop.requested() {
                            send(JobEvent::AuthorizationFailed(AuthorizationFailure::Cancelled));
                            return;
                        }
                        if failure == AuthorizationFailure::Unavailable && Instant::now() < recovery_deadline {
                            send(JobEvent::RecoveringAuthentication);
                            // Each attempt asks polkit again, so readiness is checked
                            // by the authority itself, without a stale preflight flag.
                            for _ in 0..5 {
                                if stop.requested() {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(50));
                            }
                            if stop.requested() {
                                send(JobEvent::AuthorizationFailed(AuthorizationFailure::Cancelled));
                                return;
                            }
                            continue;
                        }
                        send(JobEvent::AuthorizationFailed(failure));
                    } else {
                        send(JobEvent::Done(Err(error.to_string())));
                    }
                    return;
                }
            }
        }
    }
    send(JobEvent::Done(Ok(finish(out))));
}

/// The report a system change ends with, with whether it needs a restart
/// (or a new session) to finish, as the running machine says.
pub fn finish_report(out: String) -> String {
    let mut report: serde_json::Value = serde_json::from_str(out.trim()).unwrap_or_default();
    yukimi_system::completion::finish_update(&mut report);
    if report.get("completion").is_some() { report.to_string() } else { out }
}

/// How a job ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// What it did, in words.
    Done(String),
    /// Why it didn't, in words.
    Failed(String),
}

/// An operation under way (or just finished), as the window shows it.
pub struct Job {
    pub operation: Operation,
    /// What is happening now.
    pub status: String,
    /// Downloads and builds, in a line.
    pub summary: String,
    /// How far along, once Nix has said what to expect.
    pub fraction: Option<f64>,
    /// The last lines of Nix's log.
    pub log: VecDeque<String>,
    /// How many lines were logged in all.
    pub logged: usize,
    pub outcome: Option<Outcome>,
    pub retry_authentication: bool,
    /// The log shows.
    pub details: bool,
    /// When Nix last said something.
    pub heard: Instant,
    pub stop: nix::Stop,
    /// A number for this job, so a late message from an earlier one is
    /// told apart.
    pub id: u64,
}

impl Job {
    pub fn new(operation: Operation, id: u64) -> Job {
        Job {
            operation,
            status: "Starting…".to_owned(),
            summary: String::new(),
            fraction: None,
            log: VecDeque::new(),
            logged: 0,
            outcome: None,
            retry_authentication: false,
            details: false,
            heard: Instant::now(),
            stop: nix::Stop::new(false),
            id,
        }
    }

    pub fn running(&self) -> bool {
        self.outcome.is_none()
    }

    /// Nix hasn't said anything for a while.
    pub fn quiet(&self, now: Instant) -> bool {
        self.running() && now.saturating_duration_since(self.heard) >= QUIET
    }

    /// Take an event from the worker. Returns the outcome when it ends.
    pub fn take(&mut self, event: JobEvent, now: Instant) -> Option<&Outcome> {
        match event {
            JobEvent::Started { as_root } => {
                self.heard = now;
                if !self.stop.requested() {
                    self.status = if as_root { "Waiting for permission…" } else { "Working…" }.to_owned();
                }
                None
            }
            JobEvent::Progress(snapshot) => {
                self.heard = now;
                if !self.stop.requested() {
                    self.status = snapshot.current.unwrap_or_else(|| "Working…".to_owned());
                }
                self.summary = snapshot.summary;
                if snapshot.fraction.is_some() {
                    self.fraction = snapshot.fraction;
                }
                self.logged += snapshot.lines.len();
                self.log.extend(snapshot.lines);
                while self.log.len() > KEPT_LINES {
                    self.log.pop_front();
                }
                None
            }
            JobEvent::RecoveringAuthentication => {
                self.status = "Waiting for the administrator password dialog…".into();
                self.heard = now;
                None
            }
            JobEvent::AuthorizationFailed(failure) => {
                self.retry_authentication = failure == AuthorizationFailure::Unavailable;
                self.status = match failure {
                    AuthorizationFailure::Cancelled => "Cancelled",
                    AuthorizationFailure::Denied => "Permission denied",
                    AuthorizationFailure::Unavailable => "Permission unavailable",
                }
                .into();
                self.outcome = Some(Outcome::Failed(failure.message().into()));
                self.outcome.as_ref()
            }
            JobEvent::Done(result) => {
                let stopped = self.stop.requested();
                self.outcome = Some(match result {
                    Ok(out) => {
                        self.fraction = Some(1.0);
                        let text = self.operation.outcome(&out);
                        self.status = text.clone();
                        Outcome::Done(text)
                    }
                    Err(message) => {
                        self.status = if stopped { "Stopped" } else { "That did not work" }.to_owned();
                        // The log shows with the error, for whoever wants to see why.
                        self.details = true;
                        Outcome::Failed(explain(&message, self.operation.is_system(), stopped))
                    }
                });
                self.outcome.as_ref()
            }
        }
    }

    /// Ask it to stop: the helper is asked through its input, this user's
    /// own Nix is also signalled.
    pub fn request_stop(&mut self) {
        if self.running() {
            self.stop.request();
            self.status = "Stopping…".to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::ops::Run;

    /// A step that runs a shell script as this user, in place of Nix.
    fn script(text: &str) -> Step {
        let text = text.to_owned();
        Box::new(move || {
            let mut command = Command::new("sh");
            command.args(["-c", &text]);
            Ok(Some(Run { command, as_root: false }))
        })
    }

    fn run(steps: Vec<Step>) -> Vec<JobEvent> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        work(steps, nix::Stop::new(true), move |e| sink.lock().unwrap().push(e), |out| out);
        Arc::try_unwrap(events).unwrap().into_inner().unwrap()
    }

    #[test]
    fn progress_and_the_log_come_through() {
        let h = "0123456789abcdfghijklmnpqrsvwxyz";
        let events = run(vec![
            Box::new(|| Ok(None)),
            script(&format!(
                r#"echo '@nix {{"action":"start","id":2,"level":4,"type":103,"text":"","fields":[]}}' >&2
echo '@nix {{"action":"result","id":2,"type":105,"fields":[1,2,1,0]}}' >&2
echo '@nix {{"action":"start","id":3,"level":4,"type":100,"text":"copying path","fields":["/nix/store/{h}-htop-3.4.1","https://cache.nixos.org","local"]}}' >&2
echo 'a plain line' >&2
echo '{{"ok":true}}'"#
            )),
        ]);
        assert_eq!(events[0], JobEvent::Started { as_root: false });
        let JobEvent::Progress(last) = &events[events.len() - 2] else { panic!("{events:?}") };
        assert_eq!(last.current.as_deref(), Some("Downloading htop"));
        assert_eq!(last.summary, "Downloaded 1 of 2");
        assert_eq!(events.last(), Some(&JobEvent::Done(Ok("{\"ok\":true}\n".to_owned()))));

        let mut job = Job::new(Operation::InstallForMe { attr: "htop".into(), unfree: false }, 1);
        let now = Instant::now();
        let mut outcome = None;
        for event in events {
            outcome = job.take(event, now).cloned();
        }
        assert_eq!(outcome, Some(Outcome::Done("htop is installed for you".to_owned())));
        assert_eq!(job.log.back().map(String::as_str), Some("a plain line"));
        assert_eq!(job.fraction, Some(1.0));
        assert!(!job.quiet(now + QUIET));
    }

    #[test]
    fn failures_are_put_in_words() {
        let events = run(vec![script(
            r#"echo '@nix {"action":"msg","level":0,"msg":"error: builder for firefox failed"}' >&2; exit 1"#,
        )]);
        assert_eq!(events.last(), Some(&JobEvent::Done(Err("error: builder for firefox failed".to_owned()))));
        let mut job = Job::new(Operation::Rollback { generation: 4 }, 1);
        for event in events {
            job.take(event, Instant::now());
        }
        let Some(Outcome::Failed(why)) = &job.outcome else { panic!() };
        assert!(why.starts_with("error: builder for firefox failed"));
        assert_eq!(why, "error: builder for firefox failed");
        assert!(job.details);
        assert_eq!(
            explain("Error executing command as another user: Not authorized", true, false),
            "Error executing command as another user: Not authorized"
        );
        assert!(explain("x", false, true).starts_with("Stopped."));
    }

    #[test]
    fn the_lines_before_a_failure_are_kept() {
        let events = run(vec![script(
            r#"for i in 1 2 3 4 5 6 7 8; do echo "line $i" >&2; done
echo '@nix {"action":"msg","level":0,"msg":"error: it broke"}' >&2; exit 1"#,
        )]);
        let mut job = Job::new(Operation::Rollback { generation: 4 }, 1);
        for event in events {
            job.take(event, Instant::now());
        }
        let lines: Vec<&str> = job.log.iter().map(String::as_str).collect();
        assert_eq!(
            lines,
            ["line 1", "line 2", "line 3", "line 4", "line 5", "line 6", "line 7", "line 8", "error: it broke"]
        );
    }

    #[test]
    fn a_step_that_cannot_start_ends_it() {
        let events = run(vec![Box::new(|| Err(crate::ops::NO_HELPER.to_owned())), script("echo never")]);
        assert_eq!(events, vec![JobEvent::Done(Err(crate::ops::NO_HELPER.to_owned()))]);
        assert_eq!(explain(crate::ops::NO_HELPER, true, false), crate::ops::NO_HELPER);
    }

    #[test]
    fn stopping_stops() {
        let stop = nix::Stop::new(true);
        let events = Arc::new(Mutex::new(Vec::new()));
        let (sink, later) = (events.clone(), stop.clone());
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            later.request();
        });
        work(vec![script("sleep 30"), script("echo never")], stop, move |e| sink.lock().unwrap().push(e), |o| o);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(matches!(events.lock().unwrap().last(), Some(JobEvent::Done(Err(_)))));
    }
    fn root_script(text: &str) -> Step {
        let step = script(text);
        Box::new(move || {
            let mut run = step()?.unwrap();
            run.as_root = true;
            Ok(Some(run))
        })
    }

    #[test]
    fn authentication_retries_only_before_the_helper_starts() {
        let dir = std::env::temp_dir().join(format!("yukimi-auth-retry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let count = dir.join("attempts");
        let helper = dir.join("helper");
        let events = run(vec![root_script(&format!(
            r#"
            echo attempt >> '{count}'
            if [ "$(wc -l < '{count}')" -lt 3 ]; then
                echo 'Error executing command as another user: No authentication agent found.' >&2
                exit 127
            fi
            echo executed >> '{helper}'
            echo '{{"ok":true}}'
        "#,
            count = count.display(),
            helper = helper.display()
        ))]);
        assert_eq!(std::fs::read_to_string(helper).unwrap(), "executed\n");
        assert_eq!(events.iter().filter(|e| matches!(e, JobEvent::RecoveringAuthentication)).count(), 2);
        assert!(matches!(events.last(), Some(JobEvent::Done(Ok(_)))));
        // Even exit 127 and the familiar diagnostic cannot make a helper's
        // build output safe to retry, or turn a build failure into cancellation.
        let events = run(vec![root_script(
            "echo '@nix {\"action\":\"msg\",\"level\":0,\"msg\":\"building\"}' >&2; echo 'Error executing command as another user: No authentication agent found.' >&2; exit 127",
        )]);
        assert!(!events.iter().any(|e| matches!(e, JobEvent::RecoveringAuthentication)));
        assert!(matches!(events.last(), Some(JobEvent::Done(Err(_)))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn authentication_cancellation_and_denial_are_distinct() {
        for (text, code, expected) in [
            ("Error executing command as another user: Request dismissed", 126, AuthorizationFailure::Cancelled),
            (
                "Error executing command as another user: Not authorized\n\nThis incident has been reported.",
                127,
                AuthorizationFailure::Denied,
            ),
        ] {
            let events = run(vec![root_script(&format!("printf '%s\\n' '{text}' >&2; exit {code}"))]);
            assert_eq!(events.last(), Some(&JobEvent::AuthorizationFailed(expected)));
        }
        let error = nix::Error::CommandFailed {
            message: "126 files failed".into(),
            code: Some(126),
            stdout: String::new(),
            stderr: Some("126 files failed\n".into()),
        };
        assert_eq!(authorization_failure(&error), None);
        assert_eq!(explain("126 files failed", true, false), "126 files failed");
    }

    #[test]
    fn incomplete_or_non_utf8_output_never_causes_a_retry() {
        for extra in [
            "printf '\\377'",             // stdout must not be mistaken for empty output
            "printf '\\377\\n' >&2",      // invalid stderr must not truncate the diagnostic
            "head -c 8192 /dev/zero >&2", // stderr was too long to retain completely
        ] {
            let events = run(vec![root_script(&format!(
                "echo 'Error executing command as another user: No authentication agent found.' >&2; {extra}; exit 127"
            ))]);
            assert!(!events.iter().any(|e| matches!(e, JobEvent::RecoveringAuthentication)));
            assert!(matches!(events.last(), Some(JobEvent::Done(Err(_)))));
        }
    }

    #[test]
    fn unavailable_agent_exposes_retry_without_a_rollback_claim() {
        let mut job = Job::new(Operation::Rollback { generation: 4 }, 1);
        job.take(JobEvent::AuthorizationFailed(AuthorizationFailure::Unavailable), Instant::now());
        assert!(job.retry_authentication);
        assert_eq!(job.outcome, Some(Outcome::Failed(AuthorizationFailure::Unavailable.message().into())));
    }

    #[test]
    fn stopping_authentication_recovery_does_not_launch_another_attempt() {
        let stop = nix::Stop::new(false);
        let mut events = Vec::new();
        let sink = std::sync::Mutex::new(&mut events);
        work(
            vec![root_script(
                "echo 'Error executing command as another user: No authentication agent found.' >&2; exit 127",
            )],
            stop.clone(),
            |event| {
                if matches!(event, JobEvent::RecoveringAuthentication) {
                    stop.request();
                }
                sink.lock().unwrap().push(event);
            },
            |out| out,
        );
        assert_eq!(events.iter().filter(|e| matches!(e, JobEvent::RecoveringAuthentication)).count(), 1);
        assert_eq!(events.last(), Some(&JobEvent::AuthorizationFailed(AuthorizationFailure::Cancelled)));
    }

    #[test]
    fn recovery_has_a_deadline_and_preserves_the_safe_retry_action() {
        let started = Instant::now();
        let events = run(vec![root_script(
            "echo 'Error executing command as another user: No authentication agent found.' >&2; exit 127",
        )]);
        assert!(started.elapsed() < Duration::from_secs(8));
        assert_eq!(events.last(), Some(&JobEvent::AuthorizationFailed(AuthorizationFailure::Unavailable)));
    }
}
