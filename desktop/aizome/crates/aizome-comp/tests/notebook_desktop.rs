//! Notebook activation regressions through the real shell, Settings,
//! Wayland input, and compositor. Everything runs in private directories.
mod common;

use std::path::Path;
use std::process::Stdio;

use aizome_ipc::{DeskSel, Request, Synthetic};
use common::desktop::*;
use common::*;

fn start() -> (Compositor, Program, Program, String, u64) {
    let shell = std::env::var("AIZOME_NOTEBOOK_SHELL").expect("shell binary");
    let settings = std::env::var("AIZOME_NOTEBOOK_SETTEI").expect("Settings binary");
    let c = Compositor::start(Options::default());
    let inspect = c.dir.join("inspect");
    std::fs::create_dir(&inspect).unwrap();
    // Pane launches must use the specified build too, not the login
    // session's installed Settings executable.
    let mut paths = vec![Path::new(&settings).parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let sh = Program(
        command(&c, &shell)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("AIZOME_KIT_INSPECT", &inspect)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(c.dir.join("shell.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    wait("the private shell", || shell_state(&c, &shell).is_some());
    let app = program(&c, &settings, &["--pane", "appearance"], "settings");
    let window = c.wait_window("org.aizome.Settei").id;
    c.ok(Request::AddDesk { name: "Notebook".into(), folder: None, index: None });
    c.ok(Request::MoveToDesk { id: Some(window), desk: DeskSel::Name("Notebook".into()), follow: false });
    c.ok(Request::Minimise { id: Some(window) });
    c.wait_windows("Settings hidden on another Desk", |ws| {
        ws.iter().any(|w| w.id == window && w.minimised && !w.focused)
    });
    (c, sh, app, shell, window)
}

#[test]
#[ignore = "requires AIZOME_NOTEBOOK_SHELL and AIZOME_NOTEBOOK_SETTEI; see docs/ipc.md"]
fn a_palette_settings_result_raises_the_existing_window_and_opens_its_pane() {
    let (c, _shell, _settings, shell, window) = start();
    shell_msg(&c, &shell, serde_json::json!({"request":"palette", "scope":"settings", "query":"Caps Lock"}));
    wait("settings palette", || shell_state(&c, &shell).is_some_and(|s| s["palette"] == "Caps Lock"));
    wait_layer_keyboard_focus(&c);
    key(&c, "return");
    let windows = c.wait_windows("existing Settings raised on Keyboard", |ws| {
        ws.iter().any(|w| w.id == window && w.focused && !w.minimised && w.title == "Keyboard")
    });
    assert_eq!(windows.iter().filter(|w| w.app_id == "org.aizome.Settei").count(), 1);
    keep(&c.screenshot(), "notebook-settings-activation");
}

#[test]
#[ignore = "requires AIZOME_NOTEBOOK_SHELL and AIZOME_NOTEBOOK_SETTEI; see docs/ipc.md"]
fn clicking_a_notification_without_an_action_raises_its_hidden_app() {
    let (c, _shell, _settings, shell, window) = start();
    shell_msg(
        &c,
        &shell,
        serde_json::json!({
            "request":"notify", "app":"Settei", "summary":"Notebook notification", "body":"Open Settings"
        }),
    );
    let mut target = None;
    wait("notification banner's semantic bounds", || {
        target = std::fs::read_dir(c.dir.join("inspect")).unwrap().flatten().find_map(|entry| {
            let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
            if doc["title"] != "Menu bar" {
                return None;
            }
            let node = doc["elements"].as_array()?.iter().find(|e| {
                e["visible"] == true && e["text"].as_str().is_some_and(|s| s.starts_with("Notebook notification"))
            })?;
            let r = node["shown"].as_array()?;
            Some((r[0].as_f64()? + r[2].as_f64()? / 2.0, r[1].as_f64()? + r[3].as_f64()? / 2.0))
        });
        target.is_some()
    });
    let (x, y) = target.unwrap();
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x, y }, Synthetic::Click { button: "left".into() }],
    });
    c.wait_windows("notification's app raised", |ws| ws.iter().any(|w| w.id == window && w.focused && !w.minimised));
    wait("notification read", || {
        shell_state(&c, &shell).is_some_and(|s| {
            s["notes"].as_array().is_some_and(|notes| !notes.iter().any(|n| n == "Notebook notification"))
        })
    });
    keep(&c.screenshot(), "notebook-notification-activation");
}
