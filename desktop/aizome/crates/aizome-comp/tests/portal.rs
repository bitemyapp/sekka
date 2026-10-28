//! Aizome's file chooser (`aizome-portal --file-chooser`, what the portal
//! backend runs for each request) under the real compositor, over an app
//! that exported its window as a portal request names it: the chooser is
//! that window's dialog, has the keys, and answers with what was chosen.
//!
//! Ignored unless `AIZOME_PORTAL_TEST_BIN` names the `aizome-portal` under
//! test (`cargo build -p aizome-portal`).

mod common;

use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::process::{Child, Stdio};

use common::desktop::*;
use common::*;

fn chooser(c: &Compositor, bin: &str, request: serde_json::Value) -> Child {
    let mut child = command(c, bin)
        .arg("--file-chooser")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(c.dir.join("chooser.log")).unwrap())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&serde_json::to_vec(&request).unwrap()).unwrap();
    child
}

fn answer(child: Child) -> serde_json::Value {
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "the chooser failed: {}", out.status);
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
#[ignore = "requires AIZOME_PORTAL_TEST_BIN pointing at the aizome-portal under test"]
fn the_chooser_is_the_exporting_windows_dialog() {
    let bin = std::env::var("AIZOME_PORTAL_TEST_BIN").expect("aizome-portal binary");
    let c = Compositor::start(Options { outputs: &["1280x800@1"], renderer: Some("pixman"), ..Default::default() });
    let documents = c.dir.join("home/Documents");
    std::fs::create_dir_all(&documents).unwrap();
    std::fs::write(documents.join("letter.txt"), "Dear Aizome").unwrap();

    let app = c.client(Spec::new("org.example.Editor", 0x26a269));
    let parent = c.wait_window("org.example.Editor");
    app.send(Cmd::Export);
    wait("the exported handle", || app.seen().exported.is_some());
    let handle = app.seen().exported.unwrap();

    let child = chooser(
        &c,
        &bin,
        serde_json::json!({
            "mode": "open",
            "app_id": "org.example.Editor",
            "parent_window": format!("wayland:{handle}"),
            "title": "Open Letter",
            "current_folder": documents.as_os_str().as_bytes(),
        }),
    );
    let dialog = c.wait_window("org.aizome.FileChooser");
    assert_eq!(dialog.parent, Some(parent.id), "the chooser isn't the editor's dialog: {dialog:?}");
    assert_eq!(dialog.title, "Open Letter");
    wait("the chooser focused", || c.window("org.aizome.FileChooser").is_some_and(|w| w.focused));
    // The list has the keys: the first file, then Return opens it.
    tap(&c, "down");
    tap(&c, "return");
    let a = answer(child);
    let uri = format!("file://{}", documents.join("letter.txt").display());
    assert_eq!(a["uris"], serde_json::json!([uri]), "{a}");
    wait("the chooser gone", || c.window("org.aizome.FileChooser").is_none());

    // Without a handle the compositor knows, it stands on its own; Escape
    // cancels.
    let child = chooser(&c, &bin, serde_json::json!({ "mode": "save", "parent_window": "wayland:no-such-handle" }));
    let dialog = c.wait_window("org.aizome.FileChooser");
    assert_eq!(dialog.parent, None);
    wait("the chooser focused", || c.window("org.aizome.FileChooser").is_some_and(|w| w.focused));
    tap(&c, "escape");
    assert_eq!(answer(child), serde_json::Value::Null);
}
