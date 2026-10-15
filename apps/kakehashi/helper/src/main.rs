//! kakehashi-helper: writes one drive for Kakehashi's window, with the
//! administrator rights the system's own prompt gave it (pkexec on Linux,
//! authopen device authorization on macOS, UAC on Windows). On macOS the
//! helper runs as the user; only Apple’s authopen broker elevates.
//!
//! Usage: `kakehashi-helper --plan PLAN.json --progress PROGRESS`, or
//! `kakehashi-helper --drives` to print the drives as the window sees them
//! (as JSON; it reads only, and is how tests make a plan).
//!
//! It checks the plan's drive against the drive as it is now, writes,
//! reads back and ejects (kakehashi_core::write), appending one JSON
//! progress object per line to PROGRESS, which the window created. It
//! stops between chunks once `PROGRESS.cancel` exists.

#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::{Context, Result, bail, ensure};
use kakehashi_core::image::Cancelled;
use kakehashi_core::write::{Plan, Progress, execute};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

struct Args {
    plan: PathBuf,
    progress: PathBuf,
}

fn args() -> Result<Args> {
    let mut args = std::env::args_os().skip(1);
    let (mut plan, mut progress) = (None, None);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--plan") => plan = args.next().map(PathBuf::from),
            Some("--progress") => progress = args.next().map(PathBuf::from),
            _ => bail!("usage: kakehashi-helper --plan PLAN.json --progress PROGRESS"),
        }
    }
    Ok(Args { plan: plan.context("--plan is missing")?, progress: progress.context("--progress is missing")? })
}

/// The progress file the window made, opened to append. Running with
/// administrator rights, never follow a link there or write to a file with
/// other names: the window's file is a fresh, single regular file.
fn progress_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).with_context(|| format!("could not open {}", path.display()))?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "{} is not a regular file", path.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(meta.nlink() == 1, "{} has other names", path.display());
    }
    Ok(file)
}

fn emit(out: &mut File, progress: &Progress) {
    if let Ok(line) = serde_json::to_string(progress) {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--drives") {
        return match kakehashi_core::platform::drives().and_then(|d| Ok(serde_json::to_string_pretty(&d)?)) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e:#}");
                ExitCode::FAILURE
            }
        };
    }
    let args = match args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e:#}");
            return ExitCode::from(2);
        }
    };
    let mut out = match progress_file(&args.progress) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("{e:#}");
            return ExitCode::from(2);
        }
    };
    let plan: Result<Plan> = fs::read_to_string(&args.plan)
        .context("could not read the plan")
        .and_then(|text| serde_json::from_str(&text).context("the plan is not one Kakehashi wrote"));
    let plan = match plan {
        Ok(plan) => plan,
        Err(e) => {
            emit(&mut out, &Progress::Failed { message: format!("{e:#}"), touched: false });
            return ExitCode::FAILURE;
        }
    };

    let cancel = PathBuf::from(format!("{}.cancel", args.progress.display()));
    let mut last = Instant::now() - Duration::from_secs(1);
    let mut touched = false;
    let result = execute(
        &plan,
        &mut |progress| {
            // A few lines a second are plenty; stage changes always go out.
            let moving = matches!(progress, Progress::Writing { .. } | Progress::Verifying { .. });
            let finished = match progress {
                Progress::Writing { done, total } | Progress::Verifying { done, total } => done == total,
                _ => true,
            };
            if !moving || finished || last.elapsed() >= Duration::from_millis(200) {
                emit(&mut out, &progress);
                last = Instant::now();
            }
        },
        &|| cancel.exists(),
        &mut touched,
    );
    match result {
        Ok(outcome) => {
            emit(&mut out, &Progress::Finished { ejected: outcome.ejected, note: outcome.note });
            ExitCode::SUCCESS
        }
        Err(e) => {
            let message = if e.is::<Cancelled>() {
                "Stopped before the drive was finished.".to_owned()
            } else {
                format!("{e:#}")
            };
            emit(&mut out, &Progress::Failed { message, touched });
            ExitCode::FAILURE
        }
    }
}
