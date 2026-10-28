//! Lock surfaces through real Wayland output changes. The desktop test
//! additionally runs the shell, isolated from the user's login session.

mod common;

use aizome_ipc::{Request, Response, Synthetic, Transform};
use common::desktop::*;
use common::*;

fn configure(
    c: &Compositor,
    name: &str,
    enabled: Option<bool>,
    scale: Option<f64>,
    mode: Option<&str>,
    transform: Option<Transform>,
) {
    c.ok(Request::OutputConfig {
        name: name.into(),
        enabled,
        scale,
        mode: mode.map(str::to_owned),
        transform,
        position: None,
    });
}

fn capture(c: &Compositor, output: &str) -> Image {
    let path = c.dir.join(format!("{output}.png"));
    c.ok(Request::Screenshot {
        output: Some(output.into()),
        path: Some(path.to_string_lossy().into_owned()),
        window: None,
        area: None,
        clipboard: false,
    });
    Image::load(&path)
}

#[test]
fn lock_surface_refits_mode_scale_and_rotation() {
    let c = Compositor::start(Options { renderer: Some("pixman"), ..Default::default() });
    let _lock = LockClient::spawn(&c, 0x7e2553);
    wait("initial lock frame", || near(capture(&c, "HEADLESS-1").px(1278, 798), rgb(0x7e2553)));
    for (mode, scale, transform, width, height) in [
        ("1600x1000", 1.0, Transform::Normal, 1600, 1000),
        ("1600x1000", 0.5, Transform::Normal, 1600, 1000),
        ("1600x1000", 1.0, Transform::_90, 1000, 1600),
    ] {
        configure(&c, "HEADLESS-1", None, Some(scale), Some(mode), Some(transform));
        wait("lock covers resized output", || {
            let img = capture(&c, "HEADLESS-1");
            img.width == width
                && img.height == height
                && near(img.px(width as i32 - 2, height as i32 - 2), rgb(0x7e2553))
        });
    }
}

fn lock_count(c: &Compositor, shell: &str) -> Option<usize> {
    let state = shell_state(c, shell)?;
    Some(state["surfaces"].as_array()?.iter().filter(|s| s.as_str().is_some_and(|s| s.starts_with("Lock {"))).count())
}

fn field_pixels(img: &Image) -> Option<Vec<[u8; 3]>> {
    // Screenshots themselves file a notification, which shifts the block
    // upwards. Find the field's 2px gold rail rather than assuming its y.
    let x = (img.width as i32 - 260) / 2;
    let y = (0..img.height as i32 - 34).find(|y| (0..34).all(|dy| near(img.px(x, y + dy), rgb(0xe0af68))))?;
    // Exclude the empty field's blinking caret, but include its placeholder
    // and masked input. Only public fixture text is typed by this test.
    Some((y..y + 34).flat_map(|y| (x + 16..x + 250).map(move |x| img.px(x, y))).collect())
}

fn can_type(c: &Compositor, output: &str, pointer_x: f64) {
    wait("visible password field", || field_pixels(&capture(c, output)).is_some());
    // Switch displays by moving the pointer; no click to focus the field.
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x: pointer_x, y: 30.0 }] });
    key(c, "escape");
    c.settle();
    let before = field_pixels(&capture(c, output)).unwrap();
    key(c, "a");
    wait("typing reaches the lock field", || field_pixels(&capture(c, output)).is_some_and(|p| p != before));
    key(c, "escape");
    wait("Escape clears the lock field", || field_pixels(&capture(c, output)).is_some_and(|p| p == before));
}

#[test]
#[ignore = "requires AIZOME_LOCK_TEST_SHELL pointing at the shell under test"]
fn real_shell_keeps_prompts_through_display_changes() {
    let shell = std::env::var("AIZOME_LOCK_TEST_SHELL").expect("shell binary");
    let c = Compositor::start(Options {
        outputs: &["1280x800@1", "1280x800@1"],
        renderer: Some("pixman"),
        ..Default::default()
    });
    // The built-in panel is off while docked; only the external is present
    // when locking starts. Disabled outputs must stay off.
    configure(&c, "HEADLESS-1", Some(false), None, None, None);
    let _shell = program(&c, &shell, &[], "shell");
    wait("shell ready", || shell_state(&c, &shell).is_some());
    let app = c.client(Spec::new("test.private", 0xcc2222));
    c.wait_window("test.private");
    c.ok(Request::Lock);
    wait("external lock", || lock_count(&c, &shell) == Some(1));
    can_type(&c, "HEADLESS-2", 1300.0);
    let Response::Outputs { outputs } = c.request(Request::Outputs) else { panic!() };
    assert!(!outputs.iter().find(|o| o.name == "HEADLESS-1").unwrap().enabled);

    // Lose all outputs while locked, then return on the laptop. This uses
    // the same wl_output removal/re-advertisement path as physical hotplug.
    configure(&c, "HEADLESS-2", Some(false), None, None, None);
    wait("no stale lock surfaces", || lock_count(&c, &shell) == Some(0));
    configure(&c, "HEADLESS-1", Some(true), None, None, None);
    wait("returning panel lock", || lock_count(&c, &shell) == Some(1));
    can_type(&c, "HEADLESS-1", 30.0);
    keep(&capture(&c, "HEADLESS-1"), "lock-returned-panel");

    // Repeated attach/detach must neither duplicate protocol surfaces nor
    // lose keyboard focus on the monitor that survives.
    for _ in 0..3 {
        configure(&c, "HEADLESS-2", Some(true), None, None, None);
        wait("both prompts", || lock_count(&c, &shell) == Some(2));
        can_type(&c, "HEADLESS-1", 30.0);
        can_type(&c, "HEADLESS-2", 1300.0);
        configure(&c, "HEADLESS-2", Some(false), None, None, None);
        wait("one prompt", || lock_count(&c, &shell) == Some(1));
    }
    can_type(&c, "HEADLESS-1", 30.0);
    c.ok(Request::PowerOffMonitors);
    c.ok(Request::PowerOnMonitors);
    can_type(&c, "HEADLESS-1", 30.0);

    configure(&c, "HEADLESS-2", Some(true), None, None, None);
    wait("second prompt restored", || lock_count(&c, &shell) == Some(2));
    c.ok(Request::MirrorOutput { name: "HEADLESS-2".into(), source: Some("HEADLESS-1".into()) });
    wait("mirror uses source prompt", || lock_count(&c, &shell) == Some(1));
    wait("mirror shows the password field", || {
        let source = field_pixels(&capture(&c, "HEADLESS-1"));
        source.is_some() && field_pixels(&capture(&c, "HEADLESS-2")) == source
    });
    c.ok(Request::MirrorOutput { name: "HEADLESS-2".into(), source: None });
    wait("unmirrored prompt restored", || lock_count(&c, &shell) == Some(2));
    can_type(&c, "HEADLESS-2", 1300.0);
    assert_eq!(c.log_text().matches("locking the session").count(), 1, "display changes keep the existing lock");
    assert!(!c.log_text().contains("unlocking the session"));
    assert!(app.seen().keys.is_empty(), "locked input never reaches an application");
}

#[test]
#[ignore = "requires AIZOME_LOCK_TEST_SHELL pointing at the shell under test"]
fn real_shell_recovers_the_panel_after_undocking_with_the_lid_closed() {
    let shell = std::env::var("AIZOME_LOCK_TEST_SHELL").expect("shell binary");
    let c = Compositor::start(Options {
        outputs: &["eDP-1=1280x800@1", "1280x800@1"],
        renderer: Some("pixman"),
        ..Default::default()
    });
    let _shell = program(&c, &shell, &[], "shell");
    wait("shell ready", || shell_state(&c, &shell).is_some());
    c.ok(Request::Synthesize { events: vec![Synthetic::Lid { closed: true }] });
    c.ok(Request::Lock);
    wait("docked lock", || lock_count(&c, &shell) == Some(1));
    can_type(&c, "HEADLESS-2", 1300.0);
    c.ok(Request::PowerOffMonitors);
    configure(&c, "HEADLESS-2", Some(false), None, None, None);
    c.ok(Request::Synthesize { events: vec![Synthetic::Lid { closed: false }] });
    c.ok(Request::PowerOnMonitors);
    wait("undocked lock", || {
        shell_state(&c, &shell).is_some_and(|s| s["screens"] == serde_json::json!(["eDP-1"]))
            && lock_count(&c, &shell) == Some(1)
    });
    can_type(&c, "eDP-1", 30.0);
    keep(&capture(&c, "eDP-1"), "lock-undocked-laptop");
    assert_eq!(c.log_text().matches("locking the session").count(), 1);
    assert!(!c.log_text().contains("unlocking the session"));
}
