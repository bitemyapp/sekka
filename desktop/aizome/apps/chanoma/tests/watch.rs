//! Chanoma's watcher (`watch`), with this test's own program as the child
//! it watches: a crash leaves its note and no core dump, a crash is
//! started again and told what happened, and one that keeps crashing
//! isn't started a fourth time.

use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use aizome_chanoma::watch;

/// The child's arguments: the test below, alone.
fn child_args() -> Vec<OsString> {
    ["the_child", "--exact", "--nocapture", "--test-threads=1"].map(OsString::from).to_vec()
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("chanoma-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Beside the note: what the child does.
fn what(note: &Path) -> PathBuf {
    note.with_extension("what")
}

/// The child, when there's a note (it's watched): it counts its starts,
/// then does what's beside the note.
#[test]
fn the_child() {
    let Some(note) = std::env::var_os(watch::NOTE).map(PathBuf::from) else { return };
    watch::guard();
    let starts = note.with_extension("starts");
    let n = std::fs::read_to_string(&starts).unwrap_or_default().len();
    std::fs::write(&starts, "x".repeat(n + 1)).unwrap();
    let told = std::env::var(watch::AFTER_CRASH).ok();
    match std::fs::read_to_string(what(&note)).unwrap_or_default().trim() {
        // Crashes, as a core does, while it plays.
        "crash" => crash(),
        // Crashes the first time; once started again, keeps what it was
        // told and leaves.
        "crash-once" => match told {
            Some(told) => {
                std::fs::write(note.with_extension("told"), told).unwrap();
                std::process::exit(0);
            }
            None => crash(),
        },
        _ => std::process::exit(3),
    }
}

fn crash() -> ! {
    watch::playing("Flycast", "Crazy Taxi");
    // SAFETY: a signal to itself, which its handler takes.
    unsafe { libc::raise(libc::SIGSEGV) };
    unreachable!("the handler leaves the signal to end it");
}

#[test]
fn a_crash_leaves_its_note_and_no_core_dump() {
    let d = dir("note");
    let note = d.join("child.note");
    std::fs::write(&note, "").unwrap();
    std::fs::write(what(&note), "crash").unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(child_args())
        .env(watch::NOTE, &note)
        .env_remove(watch::CORE_DUMPS)
        .env_remove(watch::AFTER_CRASH)
        .status()
        .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGSEGV), "{status:?}");
    assert!(!status.core_dumped(), "a crash dumps no core unless asked to");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "Flycast\nCrazy Taxi\nsignal 011\n");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_crash_is_started_again_and_told_what_happened() {
    let d = dir("again");
    let note = d.join("child.note");
    std::fs::write(what(&note), "crash-once").unwrap();
    let code = watch::run_as(&std::env::current_exe().unwrap(), &child_args(), &note);
    assert_eq!(code, 0, "the second start leaves of itself");
    assert_eq!(std::fs::read_to_string(note.with_extension("starts")).unwrap(), "xx");
    assert_eq!(
        std::fs::read_to_string(note.with_extension("told")).unwrap(),
        watch::told("Flycast\nCrazy Taxi\nsignal 011\n")
    );
    assert!(!note.exists(), "the note goes with the watcher");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn one_that_keeps_crashing_isnt_started_a_fourth_time() {
    let d = dir("loop");
    let note = d.join("child.note");
    std::fs::write(what(&note), "crash").unwrap();
    let code = watch::run_as(&std::env::current_exe().unwrap(), &child_args(), &note);
    assert_eq!(code, 1);
    assert_eq!(std::fs::read_to_string(note.with_extension("starts")).unwrap(), "xxx");
    let _ = std::fs::remove_dir_all(&d);
}
