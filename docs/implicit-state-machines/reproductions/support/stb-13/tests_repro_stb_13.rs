//! STB-13 (second claim): a modeset the compositor takes longer than the
//! 10 s read timeout to answer comes back as an error, so no "Keep these
//! display settings?" countdown starts, and the change stays live,
//! unconfirmed and unpersisted.
//!
//! Settei's Displays pane, live, against a fake compositor on a socket of
//! the test's own (`AIZOME_SOCKET`): it lists two displays, applies an
//! `output-config` at once and answers it 11 s later. Nothing else in
//! the window is live but the accounts, whose system bus here is the
//! isolation wrapper's address that nothing listens on.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aizome_ipc::{Mode, OutputInfo, Rect, Request, Response, Transform};
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use settei::{AppMsg, Settei, Start};

fn output(name: &str, w: i32, h: i32, hz: i32, x: i32) -> OutputInfo {
    let m = |w, h, r, p| Mode { width: w, height: h, refresh: r, preferred: p };
    OutputInfo {
        name: name.into(),
        make: "ASUSTek COMPUTER INC".into(),
        model: format!("Fake {name}"),
        serial: format!("SER-{name}"),
        enabled: true,
        current_mode: Some(m(w, h, hz, false)),
        modes: vec![m(w, h, hz, false), m(w, h, 60_000, true), m(2560, 1440, hz, false), m(1920, 1080, 60_000, false)],
        scale: 2.0,
        transform: Transform::Normal,
        logical: Rect::new(x, 0, w / 2, h / 2),
        usable: Rect::new(x, 0, w / 2, h / 2),
        physical_size_mm: (597, 336),
        powered: true,
        primary: x == 0,
        ..OutputInfo::default()
    }
}

/// The fake compositor: its outputs and the configs it applied.
struct Fake {
    outputs: Vec<OutputInfo>,
    applied: Vec<Request>,
}

fn serve(listener: UnixListener, fake: Arc<Mutex<Fake>>, slow: Duration) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let fake = fake.clone();
            std::thread::spawn(move || {
                let mut writer = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { return };
                    let response = match serde_json::from_str::<Request>(&line) {
                        Ok(Request::Outputs) => Response::Outputs { outputs: fake.lock().unwrap().outputs.clone() },
                        Ok(r @ Request::OutputConfig { .. }) => {
                            if let Request::OutputConfig { name, mode: Some(m), .. } = &r {
                                let (w, h, hz) = aizome_config::parse_mode(m).unwrap();
                                let mut f = fake.lock().unwrap();
                                if let Some(o) = f.outputs.iter_mut().find(|o| o.name == *name) {
                                    let refresh = hz.unwrap_or(60_000);
                                    o.current_mode = Some(Mode { width: w, height: h, refresh, preferred: false });
                                }
                            }
                            fake.lock().unwrap().applied.push(r);
                            // The modeset done, the answer late.
                            std::thread::sleep(slow);
                            Response::Ok
                        }
                        Ok(Request::ReloadConfig) => Response::Ok,
                        Ok(_) => Response::Error { message: "not in this fake".into() },
                        Err(e) => Response::Error { message: e.to_string() },
                    };
                    let mut text = serde_json::to_string(&response).unwrap();
                    text.push('\n');
                    if writer.write_all(text.as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
}

fn pump(h: &mut Harness<Settei>, wait: Duration, until: impl Fn(&Harness<Settei>) -> bool) -> bool {
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if until(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.send(AppMsg::Nothing);
    }
    until(h)
}

#[test]
fn repro_stb_13_a_slow_modeset_still_asks_to_keep_or_goes_back() {
    // Only inside the isolation wrapper: nothing here may reach a real bus
    // or a real compositor.
    let runtime = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR");
    let system = std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap_or_default();
    assert!(
        std::env::var_os("SEKKA_REPRO").is_some() && system.contains(&runtime) && std::env::var_os("WAYLAND_DISPLAY").is_none(),
        "run through the isolation wrapper (system bus {system:?})"
    );
    let socket = std::path::Path::new(&runtime).join("stb13.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    let fake = Arc::new(Mutex::new(Fake {
        outputs: vec![output("DP-1", 3840, 2160, 144_000, 0), output("DP-2", 3840, 2160, 144_000, 1920)],
        applied: Vec::new(),
    }));
    serve(listener, fake.clone(), Duration::from_secs(11));
    // SAFETY: the only test in this binary; set before any thread reads it.
    unsafe { std::env::set_var("AIZOME_SOCKET", &socket) };

    let dir = std::env::temp_dir().join(format!("settei-repro-stb-13-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.toml");
    let mut h = Harness::new(Settei::new(settei::panes::all(), path.clone(), false, Start::parse("displays")));
    assert!(pump(&mut h, Duration::from_secs(5), |h| h.texts(h.window()).iter().any(|t| t == "Use as")));
    // The main display (DP-1) from 144 Hz to 60 Hz, through its dropdown.
    let w = h.window();
    h.resize(w, aizome_kit::layout::Size::new(940.0, 1800.0));
    assert!(h.click_text(w, "144 Hz"), "{:?}", h.texts(w));
    let popup = h.popups(w).last().map(|(p, _)| *p).expect("the refresh rates");
    h.popup_keyboard(popup, Key::Named(NamedKey::Home), Modifiers::default());
    h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::default());
    h.popup_keyboard(popup, Key::Named(NamedKey::Enter), Modifiers::default());
    let asked_at = Instant::now();
    let asked = pump(&mut h, Duration::from_secs(14), |h| {
        h.program().sheet_window().is_some_and(|s| h.texts(s).iter().any(|t| t == "Keep these display settings?"))
            || h.program().shared.status.as_ref().is_some_and(|s| s.error && s.text.starts_with("Couldn't change the displays"))
    });
    let took = asked_at.elapsed();
    // The re-read after the error lands too.
    pump(&mut h, Duration::from_millis(500), |_| false);
    let status = h.program().shared.status.clone();
    let sheet = h.program().sheet_window().map(|s| h.texts(s));
    let shown = h.texts(w).into_iter().filter(|t| t.ends_with(" Hz")).collect::<Vec<_>>();
    let live = fake.lock().unwrap().outputs[0].current_mode.clone();
    let applied = fake.lock().unwrap().applied.clone();
    let saved = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        sheet.as_ref().is_some_and(|t| t.iter().any(|t| t == "Keep these display settings?"))
            || live.as_ref().is_some_and(|m| m.refresh == 144_000),
        "after {took:?} (settled {asked}): no countdown (sheet {sheet:?}), status {status:?}; DP-1 in the \
         compositor is now {live:?}, the pane shows {shown:?} (applied {applied:?}); settings.toml: {saved:?}"
    );
}
