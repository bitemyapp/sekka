//! SVC-05: with no list of people (`user_list = false`, or nobody listed)
//! the user name is typed; a mistyped name must be correctable without
//! restarting the greeter.
//!
//! The greeter runs live (`live = true`), so its own link thread connects
//! to `$GREETD_SOCK`; the socket is a fake greetd in this process, under
//! the test's private runtime directory, answering from a script and
//! recording what it was asked. Never real greetd.
//!
//! 1. PAM rejects the password: the login begins again for the same name,
//!    and the typed name's password view once offered no way back to the
//!    name (no Escape, no "Log in as someone else"). Escape must bring the
//!    name back, as typed, to be corrected and logged in.
//! 2. `CreateSession` itself fails: the field says "Press Return to try
//!    again", but Return once did nothing (it looked for the person in the
//!    stage, and a typed name has none there). Return must ask greetd
//!    again, for the same name, and Escape must bring the name back.
//!
//! Each scenario runs in a subprocess so that `GREETD_SOCK` is set before
//! anything starts.

use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aizome_greeter::app::Greeter;
use aizome_greeter::config::{Config, State};
use aizome_greeter::greetd::{AuthMessageType, ErrorType, Request, Response, read_msg, write_msg};
use aizome_greeter::login::{Progress, Screen};
use aizome_greeter::sessions::{Kind, Session};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;

const W: f64 = 1440.0;
const H: f64 = 900.0;

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join(format!("svc05-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run_child(scenario: &str) {
    let dir = scratch(scenario);
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "repro_svc_05_child", "--ignored", "--nocapture"])
        .env("GREETD_SOCK", dir.join("greetd.sock"))
        .env("REPRO_SVC_05_DIR", &dir)
        .env("REPRO_SVC_05_SCENARIO", scenario);
    let ok = c.status().unwrap().success();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "scenario {scenario}");
}

#[test]
fn a_rejected_password_for_a_mistyped_name_lets_the_name_be_typed_again() {
    run_child("auth-error");
}

#[test]
fn a_session_that_cant_be_created_can_be_tried_again() {
    run_child("create-fails");
}

type Log = Arc<Mutex<Vec<Request>>>;

/// A greetd that answers `script` and records each request.
fn fake_greetd(path: PathBuf, mut script: impl FnMut(&Request) -> Response + Send + 'static) -> Log {
    let listener = UnixListener::bind(&path).unwrap();
    let log: Log = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        while let Ok(r) = read_msg::<Request>(&mut s) {
            let answer = script(&r);
            seen.lock().unwrap().push(r);
            if write_msg(&mut s, &answer).is_err() {
                return;
            }
        }
    });
    log
}

fn greeter(dir: &std::path::Path) -> Harness<Greeter> {
    let cfg = Config {
        user_list: false,
        default_session: Some("aizome".into()),
        power: false,
        state: dir.join("state.toml"),
        ..Config::default()
    };
    let session = Session {
        id: "aizome".into(),
        name: "Aizome".into(),
        comment: String::new(),
        exec: vec!["aizome-session".into()],
        desktop_names: vec!["Aizome".into()],
        kind: Kind::Wayland,
        path: PathBuf::from("/s/aizome.desktop"),
    };
    // Live: the link thread talks to $GREETD_SOCK (the fake).
    let g = Greeter::with(cfg, State::default(), Vec::new(), vec![session], "ganymede".into(), true, false);
    let mut h = Harness::new(g);
    h.set_outputs(vec![aizome_kit::Output {
        id: 1,
        name: "eDP-1".into(),
        connector: Some("eDP-1".into()),
        description: None,
        make: "SDC".into(),
        model: "Panel".into(),
        frame: Rect::new(0.0, 0.0, W, H),
        scale: 1.0,
        integer_scale: 1,
        physical_size: None,
        refresh_hz: 60.0,
        work_area: None,
    }]);
    for k in h.windows() {
        h.resize(k, Size::new(W, H));
    }
    h
}

fn progress(g: &Greeter) -> Option<Progress<'_>> {
    match g.login.screen() {
        Screen::Login { progress, .. } => Some(progress),
        Screen::Unstarted | Screen::People { .. } | Screen::Name { .. } => None,
    }
}

fn asking(g: &Greeter) -> bool {
    matches!(progress(g), Some(Progress::Asking { .. }))
}

fn wait(h: &mut Harness<Greeter>, what: &str, f: impl Fn(&Greeter) -> bool) {
    assert!(h.settle(Duration::from_secs(5), f), "waiting for {what}: {:?}", h.program().login.screen());
}

fn enter(h: &mut Harness<Greeter>, w: WindowKey) {
    h.named(w, NamedKey::Enter, Modifiers::NONE);
}

fn click_id(h: &mut Harness<Greeter>, w: WindowKey, id: &str) {
    let r = h.find_id(w, id).unwrap_or_else(|| panic!("no {id} showing: {:?}", h.texts(w)));
    h.click(w, Point::new((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0));
}

fn secret() -> Response {
    Response::AuthMessage { auth_message_type: AuthMessageType::Secret, auth_message: "Password: ".into() }
}

fn created(log: &Log, name: &str) -> usize {
    log.lock().unwrap().iter().filter(|r| matches!(r, Request::CreateSession { username } if username == name)).count()
}

fn cancels(log: &Log) -> usize {
    log.lock().unwrap().iter().filter(|r| **r == Request::CancelSession).count()
}

/// Back to the name with Escape: the name field shows, holding what was
/// typed, and greetd lets the login go.
fn back_to_the_name(h: &mut Harness<Greeter>, w: WindowKey, log: &Log) {
    let before = cancels(log);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    let user = h.program().login.user().map(str::to_owned);
    assert!(
        h.find_id(w, "greeter-name").is_some(),
        "no way back to the name: after Escape the login is still for {user:?} and the screen shows only {:?}",
        h.texts(w)
    );
    assert!(matches!(h.program().login.screen(), Screen::Name { typed: "alcie" }), "{:?}", h.program().login.screen());
    assert!(
        h.settle(Duration::from_secs(5), |_| cancels(log) > before),
        "greetd wasn't told to let the login go: {:?}",
        log.lock().unwrap()
    );
}

#[test]
#[ignore = "run in a subprocess with GREETD_SOCK set"]
fn repro_svc_05_child() {
    let dir = PathBuf::from(std::env::var_os("REPRO_SVC_05_DIR").unwrap());
    let sock = PathBuf::from(std::env::var_os("GREETD_SOCK").unwrap());
    let scenario = std::env::var("REPRO_SVC_05_SCENARIO").unwrap();
    let log = match scenario.as_str() {
        // PAM asks for a password, then rejects it (as for an unknown user:
        // PAM doesn't say which).
        "auth-error" => fake_greetd(sock, |r| match r {
            Request::CreateSession { .. } => secret(),
            Request::PostAuthMessageResponse { response: Some(_) } => {
                Response::Error { error_type: ErrorType::AuthError, description: "pam_authenticate: AUTH_ERR".into() }
            }
            Request::PostAuthMessageResponse { response: None }
            | Request::StartSession { .. }
            | Request::CancelSession => Response::Success,
        }),
        // greetd can't create the session at all.
        "create-fails" => fake_greetd(sock, |r| match r {
            Request::CreateSession { .. } => {
                Response::Error { error_type: ErrorType::Error, description: "pam_start: user unknown".into() }
            }
            Request::PostAuthMessageResponse { .. } | Request::StartSession { .. } | Request::CancelSession => {
                Response::Success
            }
        }),
        other => panic!("unknown scenario {other}"),
    };
    let mut h = greeter(&dir);
    let w = h.program().main_window().expect("the login's surface");
    assert!(matches!(h.program().login.screen(), Screen::Name { typed: "" }));
    // The name, mistyped.
    h.type_text(w, "alcie");
    enter(&mut h, w);
    match scenario.as_str() {
        "auth-error" => {
            wait(&mut h, "the password question", asking);
            click_id(&mut h, w, "greeter-password");
            h.type_text(w, "hunter2");
            enter(&mut h, w);
            wait(&mut h, "the rejection and the question again", |g| g.login.failed() == 1 && asking(g));
            eprintln!("greetd was asked: {:?}", log.lock().unwrap());
            eprintln!("showing: {:?}", h.texts(w));
            assert!(h.texts(w).iter().any(|t| t == "Log in as someone else"), "{:?}", h.texts(w));
            // Back, to type the name again.
            back_to_the_name(&mut h, w, &log);
            for _ in 0..5 {
                h.named(w, NamedKey::Backspace, Modifiers::NONE);
            }
            h.type_text(w, "alice");
            enter(&mut h, w);
            wait(&mut h, "the corrected name's password question", |g| g.login.user() == Some("alice") && asking(g));
            assert_eq!(created(&log, "alice"), 1, "{:?}", log.lock().unwrap());
            eprintln!("then greetd was asked: {:?}", log.lock().unwrap());
        }
        "create-fails" => {
            wait(&mut h, "the failure", |g| progress(g) == Some(Progress::Failed));
            let asked = created(&log, "alcie");
            eprintln!("greetd was asked: {:?}; message: {:?}", log.lock().unwrap(), h.program().login.message());
            // "Press Return to try again".
            click_id(&mut h, w, "greeter-password");
            h.type_text(w, "x");
            enter(&mut h, w);
            let again = h.settle(Duration::from_secs(5), |_| created(&log, "alcie") > asked);
            assert!(
                again,
                "Return did nothing: {:?}, greetd asked again: {again}, user {:?}, showing {:?}",
                h.program().login.screen(),
                h.program().login.user(),
                h.texts(w)
            );
            wait(&mut h, "the failure again", |g| progress(g) == Some(Progress::Failed));
            eprintln!("then greetd was asked: {:?}", log.lock().unwrap());
            // And Escape brings the name back.
            back_to_the_name(&mut h, w, &log);
        }
        _ => unreachable!(),
    }
}
