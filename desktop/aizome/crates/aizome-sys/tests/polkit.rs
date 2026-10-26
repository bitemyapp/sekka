//! The polkit agent against a fake polkitd on a private bus and a fake
//! helper (a script speaking the helper's lines, or a socket), so no real
//! authentication happens.

mod common;

use aizome_sys::polkit::{AuthEvent, Helper, PolkitAgent};
use common::eventually;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zbus::zvariant::{OwnedValue, Value};

const AGENT_PATH: &str = "/org/aizome/PolicyKit1/AuthenticationAgent";

#[derive(Default)]
struct Registrations {
    agents: Vec<(String, String, String)>,
    unregistered: usize,
}

struct Authority {
    log: Arc<Mutex<Registrations>>,
}

type Subject = (String, HashMap<String, OwnedValue>);

#[zbus::interface(name = "org.freedesktop.PolicyKit1.Authority")]
impl Authority {
    fn register_authentication_agent(&self, subject: Subject, locale: String, path: String) {
        let session =
            subject.1.get("session-id").and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default();
        assert_eq!(subject.0, "unix-session");
        self.log.lock().unwrap().agents.push((session, locale, path));
    }
    fn unregister_authentication_agent(&self, _subject: Subject, _path: String) {
        self.log.lock().unwrap().unregistered += 1;
    }
}

/// A directory with a helper script that wants the password "secret".
fn helper_script() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aizome-sys-polkit-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("polkit-agent-helper-1");
    std::fs::write(
        &script,
        "#!/bin/sh\nread cookie\necho \"$1 $cookie\" > \"$(dirname \"$0\")/args\"\n\
         echo \"PAM_TEXT_INFO Authenticating $1\"\necho \"PAM_PROMPT_ECHO_OFF Password: \"\nread password\n\
         if [ \"$password\" = secret ]; then echo SUCCESS; else echo \"PAM_ERROR_MSG Authentication failure\"; echo FAILURE; fi\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn rand_suffix() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

fn me() -> u32 {
    // SAFETY: getuid has no failure mode.
    unsafe { libc::getuid() }
}

/// polkitd's call, from its own connection; the agent's reply.
fn begin(polkitd: &zbus::blocking::Connection, agent: &str, cookie: &str) -> zbus::Result<()> {
    let identity: (&str, HashMap<&str, Value>) = ("unix-user", HashMap::from([("uid", Value::from(me()))]));
    let details: HashMap<&str, &str> = HashMap::from([("polkit.subject-pid", "4242")]);
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

fn has_passwd_entry() -> bool {
    std::fs::read_to_string("/etc/passwd")
        .is_ok_and(|p| p.lines().any(|l| l.split(':').nth(2) == Some(&me().to_string())))
}

/// A UI that answers every prompt from a list, retrying after failures.
fn answering_ui(answers: Vec<&'static str>, events: Arc<Mutex<Vec<String>>>) -> impl Fn(AuthEvent) + Send + Sync {
    let answers = Arc::new(Mutex::new(answers.into_iter()));
    let current = Arc::new(Mutex::new(None::<aizome_sys::polkit::AuthRequest>));
    move |event| {
        let mut log = events.lock().unwrap();
        match event {
            AuthEvent::Begin(request) => {
                log.push("begin".into());
                let identity = request.default_identity().cloned().unwrap();
                *current.lock().unwrap() = Some(request.clone());
                std::thread::spawn(move || request.authenticate(&identity));
            }
            AuthEvent::Prompt { text, echo, .. } => {
                log.push(format!("prompt {text} {echo}"));
                let request = current.lock().unwrap().clone().unwrap();
                match answers.lock().unwrap().next() {
                    Some("wait") => {}
                    Some("cancel") | None => {
                        std::thread::spawn(move || request.cancel());
                    }
                    Some(answer) => {
                        std::thread::spawn(move || request.respond(answer));
                    }
                }
            }
            AuthEvent::Info { text, .. } => log.push(format!("info {text}")),
            AuthEvent::Error { text, .. } => log.push(format!("error {text}")),
            AuthEvent::Failed { .. } => {
                log.push("failed".into());
                let request = current.lock().unwrap().clone().unwrap();
                let identity = request.default_identity().cloned().unwrap();
                std::thread::spawn(move || request.authenticate(&identity));
            }
            AuthEvent::Done { authorized, .. } => log.push(format!("done {authorized}")),
        }
    }
}

fn serve_polkitd(bus: &common::TestBus) -> (zbus::blocking::Connection, Arc<Mutex<Registrations>>) {
    let log = Arc::new(Mutex::new(Registrations::default()));
    let polkitd = bus
        .builder()
        .name("org.freedesktop.PolicyKit1")
        .unwrap()
        .serve_at("/org/freedesktop/PolicyKit1/Authority", Authority { log: log.clone() })
        .unwrap()
        .build()
        .unwrap();
    (polkitd, log)
}

#[test]
fn the_conversation_retries_after_a_wrong_password() {
    let bus = test_bus!();
    if !has_passwd_entry() {
        eprintln!("skipping: no passwd entry for this uid");
        return;
    }
    let (polkitd, log) = serve_polkitd(&bus);
    let script = helper_script();
    let events = Arc::new(Mutex::new(Vec::new()));
    let agent_bus = bus.bus();
    let agent_name = agent_bus.unique_name().unwrap();
    let agent = PolkitAgent::register_with(
        agent_bus,
        "7".into(),
        Helper::Program(script.clone()),
        answering_ui(vec!["wrong", "secret"], events.clone()),
    )
    .unwrap();
    assert_eq!(log.lock().unwrap().agents.len(), 1);
    begin(&polkitd, &agent_name, "cookie-42").unwrap();
    let log_lines = events.lock().unwrap().clone();
    let user = log_lines.iter().find_map(|l| l.strip_prefix("info Authenticating ")).unwrap().to_owned();
    assert_eq!(
        log_lines,
        [
            "begin".to_owned(),
            format!("info Authenticating {user}"),
            "prompt Password:  false".into(),
            "error Authentication failure".into(),
            "failed".into(),
            format!("info Authenticating {user}"),
            "prompt Password:  false".into(),
            "done true".into(),
        ]
    );
    // The cookie went to the helper on its input, not its command line.
    let args = std::fs::read_to_string(script.parent().unwrap().join("args")).unwrap();
    assert_eq!(args.trim(), format!("{user} cookie-42"));
    std::fs::remove_dir_all(script.parent().unwrap()).unwrap();
    drop(agent);
}

#[test]
fn dismissing_the_dialog_cancels() {
    let bus = test_bus!();
    if !has_passwd_entry() {
        eprintln!("skipping: no passwd entry for this uid");
        return;
    }
    let (polkitd, _log) = serve_polkitd(&bus);
    let script = helper_script();
    let events = Arc::new(Mutex::new(Vec::new()));
    let agent_bus = bus.bus();
    let agent_name = agent_bus.unique_name().unwrap();
    let _agent = PolkitAgent::register_with(
        agent_bus,
        "7".into(),
        Helper::Program(script.clone()),
        answering_ui(vec!["cancel"], events.clone()),
    )
    .unwrap();
    let result = begin(&polkitd, &agent_name, "cookie-7");
    match result {
        Err(zbus::Error::MethodError(name, _, _)) => {
            assert_eq!(name.as_str(), "org.freedesktop.PolicyKit1.Error.Cancelled")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(events.lock().unwrap().last().unwrap(), "done false");
    std::fs::remove_dir_all(script.parent().unwrap()).unwrap();
}

#[test]
fn polkitd_can_cancel_and_the_socket_helper_works() {
    let bus = test_bus!();
    if !has_passwd_entry() {
        eprintln!("skipping: no passwd entry for this uid");
        return;
    }
    let (polkitd, _log) = serve_polkitd(&bus);
    // A socket-activated helper of our own.
    let dir = std::env::temp_dir().join(format!("aizome-sys-polkit-sock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("agent-helper.socket");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let got = received.clone();
    let server = std::thread::spawn(move || {
        // First conversation: succeeds with "secret".
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut line = String::new();
        for _ in 0..2 {
            line.clear();
            reader.read_line(&mut line).unwrap();
            got.lock().unwrap().push(line.trim().to_owned());
        }
        writer.write_all(b"PAM_PROMPT_ECHO_OFF Password: \n").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        got.lock().unwrap().push(line.trim().to_owned());
        writer.write_all(b"SUCCESS\n").unwrap();
        // Second: left waiting at the prompt until polkitd cancels.
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        for _ in 0..2 {
            line.clear();
            reader.read_line(&mut line).unwrap();
        }
        writer.write_all(b"PAM_PROMPT_ECHO_OFF Password: \n").unwrap();
        line.clear();
        // The agent hangs up when cancelled.
        let n = reader.read_line(&mut line).unwrap_or(0);
        got.lock().unwrap().push(format!("hung up {}", n == 0));
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let agent_bus = bus.bus();
    let agent_name = agent_bus.unique_name().unwrap();
    // Answers "secret" to the first prompt and leaves the second waiting.
    let _agent = PolkitAgent::register_with(
        agent_bus,
        "7".into(),
        Helper::Socket(socket.clone()),
        answering_ui(vec!["secret", "wait"], events.clone()),
    )
    .unwrap();
    begin(&polkitd, &agent_name, "cookie-a").unwrap();
    {
        let got = received.lock().unwrap();
        assert_eq!(got[1], "cookie-a");
        assert_eq!(got[2], "secret");
    }

    // polkitd gives up on the second while the dialog waits.
    let call = std::thread::spawn({
        let polkitd = polkitd.clone();
        let agent_name = agent_name.clone();
        move || begin(&polkitd, &agent_name, "cookie-b")
    });
    assert!(eventually(|| events.lock().unwrap().iter().filter(|e| e.starts_with("prompt")).count() == 2));
    polkitd
        .call_method(
            Some(agent_name.as_str()),
            AGENT_PATH,
            Some("org.freedesktop.PolicyKit1.AuthenticationAgent"),
            "CancelAuthentication",
            &("cookie-b",),
        )
        .unwrap();
    assert!(call.join().unwrap().is_err());
    assert_eq!(events.lock().unwrap().last().unwrap(), "done false");
    server.join().unwrap();
    assert_eq!(received.lock().unwrap().last().unwrap(), "hung up true");
    std::fs::remove_dir_all(&dir).unwrap();
}
