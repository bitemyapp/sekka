//! SHB-03: the shell's polkit dialog, with polkit's helper unreachable (its
//! socket isn't there). The old dialog restarted the conversation on every
//! failure, at once, and the agent failed it at once, so the two spun
//! without end. The real agent (aizome-sys) serves a fake polkitd on a
//! private dbus-daemon; its events go to the real shell, headless, as its
//! message sender would deliver them. Nobody types: the dialog should settle
//! on "unavailable", not start attempt after attempt. Never the real
//! polkitd, the system bus or polkit-agent-helper-1.
//!
//! The dialog's window is also given back when the compositor takes it
//! away: the dialog shows again on a screen there is, and still answers.

mod common;

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aizome_kit::testing::Harness;
use aizome_shell::auth::{AuthMsg, Submit};
use aizome_shell::shell::{Msg, Shell};
use aizome_sys::polkit::{AuthEvent, AuthRequest, Helper, PolkitAgent, Said, said};
use common::*;
use zbus::zvariant::{OwnedValue, Value};

const AGENT_PATH: &str = "/org/aizome/PolicyKit1/AuthenticationAgent";

/// A private dbus-daemon that allows everything and activates nothing.
struct TestBus {
    child: Child,
    address: String,
    dir: PathBuf,
}

impl TestBus {
    fn start(name: &str) -> Option<TestBus> {
        let dir = std::env::temp_dir().join(format!("repro-shb-03-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                dir.display()
            ),
        )
        .ok()?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        let address = line.trim().to_owned();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(TestBus { child, address, dir })
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

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

/// The real agent on a private bus with a fake polkitd, its events in a
/// channel.
struct Polkit {
    polkitd: zbus::blocking::Connection,
    agent_name: String,
    events: mpsc::Receiver<AuthEvent>,
    _agent: PolkitAgent,
    bus: TestBus,
}

impl Polkit {
    /// `None` (the test skips) without dbus-daemon or a passwd entry.
    fn start(name: &str, helper: impl FnOnce(&TestBus) -> Helper) -> Option<Polkit> {
        let Some(bus) = TestBus::start(name) else {
            eprintln!("skipping: no dbus-daemon");
            return None;
        };
        if !std::fs::read_to_string("/etc/passwd")
            .is_ok_and(|p| p.lines().any(|l| l.split(':').nth(2) == Some(&me().to_string())))
        {
            eprintln!("skipping: no passwd entry for this uid");
            return None;
        }
        let registered = Arc::new(Mutex::new(0usize));
        let polkitd = zbus::blocking::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name("org.freedesktop.PolicyKit1")
            .unwrap()
            .serve_at("/org/freedesktop/PolicyKit1/Authority", Authority { registered: registered.clone() })
            .unwrap()
            .build()
            .unwrap();
        let agent_bus = aizome_sys::Bus::at(&bus.address).unwrap();
        let agent_name = agent_bus.unique_name().unwrap();
        let (tx, events) = mpsc::channel::<AuthEvent>();
        let agent = PolkitAgent::register_with(agent_bus, "7".into(), helper(&bus), move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while *registered.lock().unwrap() == 0 {
            assert!(Instant::now() < deadline, "the agent never registered");
            std::thread::sleep(Duration::from_millis(20));
        }
        Some(Polkit { polkitd, agent_name, events, _agent: agent, bus })
    }

    /// polkitd asks; its call stays pending while the dialog is up.
    fn ask(&self) -> JoinHandle<zbus::Result<zbus::message::Message>> {
        let polkitd = self.polkitd.clone();
        let agent_name = self.agent_name.clone();
        std::thread::spawn(move || {
            let identity: (&str, HashMap<&str, Value>) = ("unix-user", HashMap::from([("uid", Value::from(me()))]));
            let details: HashMap<&str, &str> = HashMap::new();
            polkitd.call_method(
                Some(agent_name.as_str()),
                AGENT_PATH,
                Some("org.freedesktop.PolicyKit1.AuthenticationAgent"),
                "BeginAuthentication",
                &(
                    "org.freedesktop.udisks2.filesystem-mount",
                    "Authentication is required to mount sekka-nightly.iso.",
                    "drive-removable-media",
                    details,
                    "cookie-1",
                    vec![identity],
                ),
            )
        })
    }

    /// The shell's loop until `done`, or `limit` passes: what the agent
    /// says goes to the shell as its message sender would deliver it.
    fn pump(&self, h: &mut Harness<Shell>, limit: Duration, mut done: impl FnMut(&Harness<Shell>, &AuthEvent) -> bool) {
        let start = Instant::now();
        while start.elapsed() < limit {
            let Ok(e) = self.events.recv_timeout(Duration::from_millis(50)) else { continue };
            h.send(Msg::Auth(e.clone().into()));
            if done(h, &e) {
                return;
            }
        }
    }
}

#[test]
fn an_unreachable_helper_does_not_spin_the_dialog() {
    // polkit 126's socket-activated helper, its socket missing.
    let Some(polkit) = Polkit::start("spin", |bus| Helper::Socket(bus.dir.join("agent-helper.socket"))) else {
        return;
    };
    let mut h = scene(false);
    let call = polkit.ask();

    // The shell's loop, two and a half seconds of it, nobody typing. The
    // attempts start at 0, 0.5 and 1.5 s, and the third failure is the last.
    let start = Instant::now();
    let mut request: Option<AuthRequest> = None;
    let mut failed = 0usize;
    polkit.pump(&mut h, Duration::from_millis(2500), |_, e| {
        match e {
            AuthEvent::Begin(r) => request = Some(r.clone()),
            AuthEvent::Heard(heard) => {
                if let Some(Ok(fresh)) = request.as_ref().map(|r| r.check(heard.clone())) {
                    match said(fresh) {
                        Said::Retrying { failed: Some(_), .. } | Said::Unavailable { .. } => failed += 1,
                        Said::Retrying { failed: None, .. }
                        | Said::Prompt(_)
                        | Said::Error(_)
                        | Said::Info(_)
                        | Said::Refused { .. } => {}
                    }
                }
            }
            AuthEvent::Done { .. } => {}
        }
        failed >= 500
    });
    let took = start.elapsed();
    let dialog = h.program().auth_view().map(|a| (a.note, a.submit));
    // Dismiss it; polkitd's call ends.
    h.send(Msg::AuthUi(AuthMsg::Cancel));
    let _ = call.join();
    assert!(dialog.is_some(), "the dialog opened");
    eprintln!("{failed} failed attempts in {took:?}; the dialog shows {dialog:?}");
    assert!(
        failed <= 3,
        "the dialog's request was attempted {failed} times in {took:?} with no one typing; it shows {dialog:?}"
    );
    assert_eq!(
        dialog.map(|(_, submit)| submit),
        Some(Submit::TryAgain),
        "the dialog settles on Try Again: {failed} failed attempts"
    );
}

#[test]
fn the_dialog_shows_again_when_its_output_goes() {
    // A helper that asks once and takes "secret".
    let helper = |bus: &TestBus| {
        let script = bus.dir.join("polkit-agent-helper-1");
        std::fs::write(
            &script,
            "#!/bin/sh\nread cookie\necho \"PAM_PROMPT_ECHO_OFF Password: \"\nread password\n\
             if [ \"$password\" = secret ]; then echo SUCCESS; else echo FAILURE; fi\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Helper::Program(script)
    };
    let Some(polkit) = Polkit::start("window", helper) else { return };
    let mut h = scene(false);
    let call = polkit.ask();
    polkit.pump(&mut h, Duration::from_secs(5), |h, _| {
        h.program().auth_view().is_some_and(|v| v.submit == Submit::Answer && v.prompt.starts_with("Password"))
    });
    let first = window_titled(&h, "Authenticate");
    // Its output went: the compositor closes the layer.
    h.request_close(first);
    let windows: Vec<_> = h.windows().into_iter().filter(|w| h.title(*w) == "Authenticate").collect();
    assert_eq!(windows.len(), 1, "one dialog window");
    let second = windows[0];
    assert_ne!(second, first, "the dialog has a window that's there");
    assert!(h.texts(second).iter().any(|t| t.contains("mount sekka-nightly.iso")));
    // And it still answers polkit.
    h.type_text(second, "secret");
    assert!(h.click_text(second, "Authenticate"));
    polkit.pump(&mut h, Duration::from_secs(5), |_, e| matches!(e, AuthEvent::Done { .. }));
    assert!(h.program().auth_view().is_none(), "the dialog closed");
    assert!(call.join().unwrap().is_ok(), "polkitd heard yes");
    drop(polkit.bus);
}

/// Should 8 of the review: a refusal shows what PAM's modules said, as the
/// lock screen does, so a lockout doesn't read as a wrong password.
#[test]
fn the_dialog_shows_pams_reason_for_a_refusal() {
    let helper = |bus: &TestBus| {
        let script = bus.dir.join("polkit-agent-helper-1");
        std::fs::write(
            &script,
            "#!/bin/sh\nread cookie\necho \"PAM_PROMPT_ECHO_OFF Password: \"\nread password\n\
             echo \"PAM_ERROR_MSG The account is locked due to 3 failed logins.\"\necho FAILURE\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Helper::Program(script)
    };
    let Some(polkit) = Polkit::start("pam", helper) else { return };
    let mut h = scene(false);
    let call = polkit.ask();
    polkit.pump(&mut h, Duration::from_secs(5), |h, _| {
        h.program().auth_view().is_some_and(|v| v.submit == Submit::Answer && v.prompt.starts_with("Password"))
    });
    let window = window_titled(&h, "Authenticate");
    h.type_text(window, "secret");
    assert!(h.click_text(window, "Authenticate"));
    let locked = |h: &Harness<Shell>| {
        h.program().auth_view().and_then(|v| v.note).is_some_and(|(note, _)| note.contains("account is locked"))
    };
    polkit.pump(&mut h, Duration::from_secs(5), |h, _| locked(h));
    let note = h.program().auth_view().and_then(|v| v.note);
    h.send(Msg::AuthUi(AuthMsg::Cancel));
    let _ = call.join();
    assert_eq!(note, Some(("The account is locked due to 3 failed logins.".into(), true)));
}
