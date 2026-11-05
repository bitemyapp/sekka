//! SYB-09: the polkit agent reported a helper that couldn't be reached, or
//! one that exited without a verdict, as `Failed` at once ("that attempt
//! failed: let the person try again with authenticate"), and the shell did
//! try again at once (aizome-shell/src/auth.rs's Failed handler), so helpers
//! were started as fast as the loop ran. The driver here does what that
//! handler did, through a queue as the shell's message loop does: it asks
//! for another attempt on every failure the agent reports. The agent must
//! still wait between attempts that end with no answer, and stop after a
//! few. A fake polkitd on a private bus and fake helpers (a stub script
//! that reads the cookie and exits, or a socket path that isn't there);
//! never the real polkitd or polkit-agent-helper-1.

mod common;

use aizome_sys::polkit::{AuthEvent, AuthRequest, Helper, PolkitAgent, Said, said};
use common::eventually;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::zvariant::{OwnedValue, Value};

const AGENT_PATH: &str = "/org/aizome/PolicyKit1/AuthenticationAgent";

struct Authority {
    registered: Arc<Mutex<usize>>,
}

type Subject = (String, HashMap<String, OwnedValue>);

#[zbus::interface(name = "org.freedesktop.PolicyKit1.Authority")]
impl Authority {
    fn register_authentication_agent(&self, _subject: Subject, _locale: String, _path: String) {
        *self.registered.lock().unwrap() += 1;
    }
    fn unregister_authentication_agent(&self, _subject: Subject, _path: String) {}
}

fn me() -> u32 {
    // SAFETY: getuid has no failure mode.
    unsafe { libc::getuid() }
}

fn has_passwd_entry() -> bool {
    std::fs::read_to_string("/etc/passwd")
        .is_ok_and(|p| p.lines().any(|l| l.split(':').nth(2) == Some(&me().to_string())))
}

fn begin(polkitd: &zbus::blocking::Connection, agent: &str, cookie: &str) -> zbus::Result<()> {
    let identity: (&str, HashMap<&str, Value>) = ("unix-user", HashMap::from([("uid", Value::from(me()))]));
    let details: HashMap<&str, &str> = HashMap::new();
    polkitd
        .call_method(
            Some(agent),
            AGENT_PATH,
            Some("org.freedesktop.PolicyKit1.AuthenticationAgent"),
            "BeginAuthentication",
            &(
                "org.freedesktop.udisks2.filesystem-mount",
                "Authentication is required to mount the disk",
                "drive-removable-media",
                details,
                cookie,
                vec![identity],
            ),
        )
        .map(drop)
}

/// What happened while the driver retried as the shell did.
struct Run {
    failed: usize,
    errors: Vec<String>,
    prompts: usize,
    unavailable: bool,
    took: Duration,
}

/// Serve the agent with `helper`, have polkitd ask, and answer like the
/// shell's old dialog for `window`: authenticate on Begin and again on each
/// failure, until 200 failures or the window passes. Then cancel.
fn drive(helper: Helper, window: Duration) -> Option<Run> {
    let bus = common::TestBus::start()?;
    if !has_passwd_entry() {
        eprintln!("skipping: no passwd entry for this uid");
        return None;
    }
    let registered = Arc::new(Mutex::new(0));
    let polkitd = bus
        .builder()
        .name("org.freedesktop.PolicyKit1")
        .unwrap()
        .serve_at("/org/freedesktop/PolicyKit1/Authority", Authority { registered: registered.clone() })
        .unwrap()
        .build()
        .unwrap();
    let agent_bus = bus.bus();
    let agent_name = agent_bus.unique_name().unwrap();
    let (tx, rx) = mpsc::channel::<AuthEvent>();
    let agent = PolkitAgent::register_with(agent_bus, "7".into(), helper, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    assert!(eventually(|| *registered.lock().unwrap() >= 1));
    let call = std::thread::spawn({
        let polkitd = polkitd.clone();
        move || begin(&polkitd, &agent_name, "cookie-1")
    });
    let mut request: Option<AuthRequest> = None;
    let mut run = Run { failed: 0, errors: Vec::new(), prompts: 0, unavailable: false, took: Duration::ZERO };
    let start = Instant::now();
    while start.elapsed() < window && run.failed < 200 {
        let Ok(event) = rx.recv_timeout(Duration::from_millis(50)) else { continue };
        match event {
            AuthEvent::Begin(r) => {
                let id = r.default_identity().cloned().unwrap();
                r.authenticate(&id);
                request = Some(r);
            }
            AuthEvent::Heard(heard) => {
                let r = request.as_ref().unwrap();
                let Ok(fresh) = r.check(heard) else { continue };
                let failed = match said(fresh) {
                    Said::Retrying { failed: Some(text), .. } => Some(text),
                    Said::Unavailable { failed } => {
                        run.unavailable = true;
                        Some(failed)
                    }
                    Said::Refused => Some("refused".into()),
                    Said::Prompt(_) => {
                        run.prompts += 1;
                        None
                    }
                    // The floor holding the start this driver asked for.
                    Said::Retrying { failed: None, .. } | Said::Error(_) | Said::Info(_) => None,
                };
                if let Some(text) = failed {
                    run.failed += 1;
                    if run.errors.len() < 3 {
                        run.errors.push(text);
                    }
                    // The old shell's handler: clear, say so, start again at once.
                    let id = r.default_identity().cloned().unwrap();
                    r.authenticate(&id);
                }
            }
            AuthEvent::Done { .. } => {}
        }
    }
    run.took = start.elapsed();
    if let Some(r) = &request {
        r.cancel();
    }
    let _ = call.join();
    drop(agent);
    Some(run)
}

fn script(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let script = dir.join("polkit-agent-helper-1");
    // A helper that can't do its job (not setuid, PAM refusing before any
    // prompt): it takes the cookie and exits without SUCCESS or FAILURE.
    std::fs::write(&script, "#!/bin/sh\nread cookie\necho \"$1\" >> \"$(dirname \"$0\")/spawns\"\nexit 1\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

// Attempts start at 0, 0.5 and 1.5 s; the third failure stops them, and
// the driver's next request waits out a two-second floor, past the window.

#[test]
fn a_helper_that_exits_without_a_verdict_is_not_respawned_without_end() {
    let dir = std::env::temp_dir().join(format!("repro-syb-09-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let helper = script(&dir);
    let Some(run) = drive(Helper::Program(helper), Duration::from_millis(2500)) else { return };
    let spawns = std::fs::read_to_string(dir.join("spawns")).unwrap_or_default().lines().count();
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("{spawns} helpers started in {:?}, {} failures reported: {:?}", run.took, run.failed, run.errors);
    assert!(
        spawns <= 3,
        "{spawns} helper processes started in {:?} with no one typing ({} failures, {} prompts)",
        run.took,
        run.failed,
        run.prompts
    );
    assert!(spawns >= 1 && run.unavailable, "the attempts settle as unavailable: {spawns} started, {:?}", run.errors);
}

#[test]
fn a_missing_helper_socket_is_not_retried_without_end() {
    let missing = std::env::temp_dir().join(format!("repro-syb-09-none-{}/agent-helper.socket", std::process::id()));
    let Some(run) = drive(Helper::Socket(missing), Duration::from_millis(2500)) else { return };
    eprintln!("{} attempts failed in {:?}: {:?}", run.failed, run.took, run.errors);
    assert!(
        run.failed <= 3,
        "{} connection attempts failed in {:?} with no one typing (errors shown: {:?})",
        run.failed,
        run.took,
        run.errors
    );
    assert!(run.unavailable, "the attempts settle as unavailable: {:?}", run.errors);
}
