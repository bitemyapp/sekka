//! Nisshi with its real reader, `aizome-nisshi-helper`, run as you over a
//! stand-in journalctl that answers from the made-up boot, and a stand-in
//! pkexec: the window reads the boot, its boots and files, follows new
//! messages, reads a message's fields and a log file, and unlocks (or is
//! refused). Nothing here reads this computer's journal or asks polkit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_kit::testing::Harness;
use nisshi::app::{Access, Msg, Nisshi, Request, Unlock};
use nisshi::filter::Scope;
use nisshi::fixture;

const WAIT: Duration = Duration::from_secs(20);

/// A folder of the test's own, with the stand-ins in it.
struct Stage {
    dir: PathBuf,
}

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

impl Stage {
    fn new(name: &str) -> Stage {
        let dir = std::env::temp_dir().join(format!("nisshi-reader-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fixture::stage(&dir).unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let d = dir.display();
        // pkexec: refuses, or runs the helper (as you) as `answer` says.
        script(
            &dir.join("pkexec"),
            &format!(
                r#"#!/bin/sh
export PATH='{path}'
case "$(cat '{d}/answer' 2>/dev/null)" in
  cancel) echo 'Error executing command as another user: Request dismissed' >&2; exit 126 ;;
  deny) printf 'Error executing command as another user: Not authorized\n\nThis incident has been reported.\n' >&2; exit 127 ;;
esac
[ "$1" = --disable-internal-agent ] && shift
exec "$@"
"#
            ),
        );
        Stage { dir }
    }

    fn answer(&self, a: &str) {
        std::fs::write(self.dir.join("answer"), a).unwrap();
    }

    fn asked(&self) -> String {
        std::fs::read_to_string(self.dir.join("asked")).unwrap_or_default()
    }
}

/// The variables the window's reader takes from a test. Every test sets
/// the same names to its own stage's paths; they run one at a time.
fn start(stage: &Stage) -> Harness<Nisshi> {
    // SAFETY: the tests in this file hold LOCK, so no other thread reads
    // or writes the environment meanwhile.
    unsafe {
        std::env::set_var("NISSHI_HELPER", env!("CARGO_BIN_EXE_aizome-nisshi-helper"));
        std::env::set_var("NISSHI_JOURNALCTL", stage.dir.join("journalctl"));
        std::env::set_var("NISSHI_LOG_DIR", stage.dir.join("log"));
        std::env::set_var("NISSHI_PKEXEC", stage.dir.join("pkexec"));
    }
    let mut h = Harness::new(Nisshi::live(Request::default()));
    assert!(h.settle(WAIT, |n| !n.loading && n.store.entries.len() > 500), "the boot is read");
    // The view follows its messages (after a moment).
    h.advance(Duration::from_millis(200));
    h
}

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn reads_the_boot_then_follows_it() {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let stage = Stage::new("follow");
    let mut h = start(&stage);
    let total = fixture::journal().len();
    // The newest last, all but those written while following, then those.
    assert!(h.settle(WAIT, |n| n.store.entries.len() == total), "followed: {}", h.program().store.entries.len());
    h.advance(Duration::from_millis(200));
    let n = h.program();
    assert!(n.store.complete, "fewer than a page: nothing older");
    assert_eq!(n.view.rows.len(), total);
    let times: Vec<i64> = n.store.entries.iter().map(|e| e.time).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "oldest first");
    assert_eq!(n.store.entries.last().unwrap().message.as_ref(), "Started AI background workload.");
    // The compositor's colours came as journald gives them (bytes), and
    // its WARN is a warning.
    let edid = n.store.entries.iter().find(|e| e.message.contains("no EDID")).unwrap();
    assert!(edid.styles.is_some() && !edid.message.contains('\u{1b}'));
    assert_eq!(edid.priority, nisshi::entry::Priority::Warning);
    // A launch's output, under its own identifier, is Koe's.
    let koe = n.store.entries.iter().find(|e| e.message.contains("daemon did not become ready")).unwrap();
    assert_eq!((&*koe.source, koe.launch.as_deref()), ("Koe", Some("aizome-launch-f332d86ed3fac3deb4589d0998c3a08e")));
    assert_eq!(n.boots.len(), 4);
    assert_eq!(n.boots[0].index, 0, "this boot first");
    let asked = stage.asked();
    assert!(asked.contains("--boot=0") && asked.contains("--reverse") && asked.contains("--lines=50000"), "{asked}");
    assert!(asked.contains("--follow") && asked.contains("--after-cursor=s="), "{asked}");
    // The user's own reader can read the journal it was given, but whether
    // that is the system's depends on this computer: it's said either way.
    assert_ne!(n.access, Access::Unknown);
}

#[test]
fn fields_and_files() {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let stage = Stage::new("fields");
    let mut h = start(&stage);
    let seq = h.program().store.entries.iter().find(|e| e.message.contains("kex_exchange_identification")).unwrap().seq;
    h.send(Msg::Select(seq));
    h.advance(Duration::from_millis(200));
    assert!(h.settle(WAIT, |n| n.fields.is_some()), "its fields are read");
    let fields = &h.program().fields.as_ref().unwrap().1;
    assert!(fields.iter().any(|(k, v)| k == "SYSLOG_IDENTIFIER" && v == "sshd"));
    assert_eq!(fields[0].0, "MESSAGE");
    // The log files: text ones only, under the folder.
    assert!(h.settle(WAIT, |n| !n.files.is_empty()));
    let files: Vec<String> = h.program().files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with("log/sub/app.log"));
    h.send(Msg::OpenFile(files[0].clone()));
    assert!(h.settle(WAIT, |n| !n.loading && n.file_store.entries.len() == 2));
    h.advance(Duration::from_millis(200));
    let n = h.program();
    assert_eq!(n.filter.scope, Scope::File(files[0].clone()));
    assert_eq!(n.view.rows.len(), 2);
    let e = &n.file_store.entries[1];
    assert_eq!(e.message.as_ref(), "ERROR: no disk");
    assert!(e.priority.is_error());
    assert_eq!(e.time, fixture::NOW - 299 * 1_000_000);
}

#[test]
fn unlocking_cancelled_refused_and_given() {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let stage = Stage::new("unlock");
    let mut h = start(&stage);
    stage.answer("cancel");
    h.send(Msg::Unlock);
    assert_eq!(h.program().unlock, Unlock::Asking);
    assert!(h.settle(WAIT, |n| n.unlock != Unlock::Asking));
    assert_eq!(h.program().unlock, Unlock::Locked, "cancelling leaves it as it was");
    assert_eq!(h.program().notice.as_ref().map(|n| n.0.as_str()), Some("Unlocking was cancelled."));
    stage.answer("deny");
    h.send(Msg::Unlock);
    assert!(h.settle(WAIT, |n| n.unlock != Unlock::Asking));
    assert_eq!(h.program().unlock, Unlock::Refused("An administrator's permission was refused.".into()));
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "Try Again"), "{:?}", h.texts(w));
    // Given: everything is read again, through the unlocked reader.
    stage.answer("allow");
    let before = stage.asked().matches("--list-boots").count();
    h.send(Msg::Unlock);
    assert!(h.settle(WAIT, |n| n.unlock == Unlock::Unlocked));
    assert!(h.settle(WAIT, |n| !n.loading && n.store.entries.len() > 500));
    assert_eq!(h.program().access, Access::System);
    assert!(stage.asked().matches("--list-boots").count() > before, "read again once unlocked");
    assert!(h.texts(w).iter().any(|t| t == "Unlocked"));
    // Locking again goes back to the user's own reader.
    h.send(Msg::Lock);
    assert_eq!(h.program().unlock, Unlock::Locked);
    assert!(h.settle(WAIT, |n| !n.loading && n.store.entries.len() > 500));
}

#[test]
fn a_new_user_with_nothing_of_their_own_yet() {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let stage = Stage::new("empty");
    let path = std::env::var("PATH").unwrap_or_default();
    let d = stage.dir.display();
    // journalctl as it answers someone whose own journal has no file yet.
    script(
        &stage.dir.join("journalctl"),
        &format!(
            r#"#!/bin/sh
export PATH='{path}'
echo "$*" >> '{d}/asked'
case " $* " in
  *" --list-boots "*) echo '[]'; exit 0 ;;
  *" --follow "*) cat '{d}/follow.jsonl'; exec sleep 60 ;;
esac
echo 'No journal files were opened due to insufficient permissions.' >&2
exit 1
"#
        ),
    );
    // SAFETY: as in `start`, under LOCK.
    unsafe {
        std::env::set_var("NISSHI_HELPER", env!("CARGO_BIN_EXE_aizome-nisshi-helper"));
        std::env::set_var("NISSHI_JOURNALCTL", stage.dir.join("journalctl"));
        std::env::set_var("NISSHI_LOG_DIR", stage.dir.join("log"));
    }
    let mut h = Harness::new(Nisshi::live(Request::default()));
    // An empty boot, not a failure; and what's written next comes in.
    assert!(h.settle(WAIT, |n| !n.loading && n.store.complete && n.store.entries.len() == 3));
    h.advance(Duration::from_millis(200));
    assert_eq!(h.program().notice, None);
    assert_eq!(h.program().view.rows.len(), 3);
    assert!(stage.asked().contains("--follow --lines=0"), "{}", stage.asked());
}

#[test]
fn closing_the_window_ends_its_reads() {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let stage = Stage::new("close");
    let h = start(&stage);
    let pid = stage.dir.join("follow.pid");
    let started = std::time::Instant::now();
    while !pid.exists() && started.elapsed() < WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = std::fs::read_to_string(&pid).expect("the journal followed").trim().to_owned();
    let proc = PathBuf::from(format!("/proc/{pid}"));
    assert!(proc.exists());
    // The window goes; journalctl following the journal must not stay.
    drop(h);
    let gone = std::time::Instant::now();
    while proc.exists() && gone.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!proc.exists(), "journalctl {pid} outlived the window");
}
