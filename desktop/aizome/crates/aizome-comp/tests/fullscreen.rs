//! Fullscreen requests through the client's real xdg-shell lifecycle.
mod common;
use aizome_ipc::Rect;
use common::*;

#[test]
fn a_fullscreen_request_survives_an_older_first_buffer() {
    for scale in [1, 2] {
        let outputs =
            [format!("{}x{}@{scale}", 1280 * scale, 800 * scale), format!("{}x{}@{scale}", 1024 * scale, 768 * scale)];
        let c = Compositor::start(Options { outputs: &[&outputs[0], &outputs[1]], ..Default::default() });
        // Enter, withdraw, and change output before the first buffer; that
        // buffer still uses the previous configure in every case.
        for (initial, requested) in [(None, Some(1)), (Some(1), None), (Some(1), Some(0))] {
            let spec = Spec::new("late-fullscreen", 0x2255aa).defer_mapping();
            let spec = if let Some(output) = initial { spec.fullscreen(Some(output)) } else { spec };
            let a = c.client(spec);
            a.wait_seen("initial configure", |s| s.configures > 0 && s.fullscreen == initial.is_some());
            a.send(Cmd::FullscreenThenMap(requested.is_some(), requested));
            c.wait_window("late-fullscreen");
            let expected =
                requested.map(|i| if i == 0 { Rect::new(0, 0, 1280, 800) } else { Rect::new(1280, 0, 1024, 768) });
            c.wait_windows("latest fullscreen request survives mapping", |ws| {
                ws[0].fullscreen == requested.is_some() && expected.is_none_or(|r| ws[0].geometry == r)
            });
            a.wait_seen("client reaches latest accepted state", |s| {
                s.fullscreen == requested.is_some() && expected.is_none_or(|r| s.size == (r.w, r.h))
            });
            c.settle();
            assert_eq!(c.windows()[0].fullscreen, requested.is_some());
            if let Some(rect) = expected {
                assert_eq!(c.windows()[0].geometry, rect);
            }
            assert_eq!(a.seen().fullscreen, requested.is_some());
            drop(a);
            c.wait_windows("closed", |ws| ws.is_empty());
        }
    }
}

#[test]
fn a_client_can_start_fullscreen_on_its_requested_output() {
    let c = Compositor::start(Options { outputs: &["1280x800@1", "1024x768@1"], ..Default::default() });
    let a = c.client(Spec::new("initial-fullscreen", 0x2255aa).fullscreen(Some(1)));
    let w = c.wait_window("initial-fullscreen");
    a.wait_seen("initial fullscreen configure", |s| s.fullscreen && s.size == (1024, 768));
    c.wait_windows("fullscreen on second display", |ws| ws[0].geometry == Rect::new(1280, 0, 1024, 768));
    let shot = c.screenshot();
    assert_px(&shot, 1280, 0, rgb(0x2255aa), "no decoration above fullscreen");
    a.send(Cmd::Fullscreen(false, None));
    a.wait_seen("leave fullscreen", |s| !s.fullscreen);
    c.wait_windows("normal frame restored", |ws| ws[0].id == w.id && ws[0].geometry.y > 0);
}

#[test]
fn a_fullscreen_window_can_change_outputs_and_restore_its_original_geometry() {
    let c = Compositor::start(Options { outputs: &["1280x800@1", "1024x768@1"], ..Default::default() });
    let a = c.client(Spec::new("move-fullscreen", 0x2255aa));
    let w = c.wait_window("move-fullscreen");
    a.send(Cmd::Fullscreen(true, Some(1)));
    a.wait_seen("fullscreen on second output", |s| s.fullscreen && s.size == (1024, 768));
    c.wait_windows("second output", |ws| ws[0].geometry.x == 1280);
    a.send(Cmd::Fullscreen(true, Some(0)));
    a.wait_seen("fullscreen on first output", |s| s.fullscreen && s.size == (1280, 800));
    c.wait_windows("first output", |ws| ws[0].geometry == Rect::new(0, 0, 1280, 800));
    a.send(Cmd::Fullscreen(false, None));
    a.wait_seen("restored client size", |s| !s.fullscreen && s.size == (w.geometry.w, w.geometry.h));
    c.wait_windows("restored geometry", |ws| ws[0].geometry == w.geometry);
}

#[test]
fn recovery_declines_fullscreen_consistently_before_and_after_mapping() {
    let c = Compositor::start(Options {
        outputs: &["1280x800@1", "1024x768@1"],
        logic: Some(std::path::Path::new("/nonexistent/libaizome_comp_logic.so")),
        ..Default::default()
    });
    let a = c.client(Spec::new("fullscreen-without-logic", 0x2255aa).fullscreen(Some(1)));
    let w = c.wait_window("fullscreen-without-logic");
    let initial = a.seen();
    assert!(!initial.fullscreen, "recovery cannot grant fullscreen: {initial:?}");
    assert_eq!((w.geometry.w, w.geometry.h), (400, 300), "normal client size in recovery");
    let configures = initial.configures;
    a.send(Cmd::Fullscreen(true, Some(1)));
    let after = a.wait_seen("recovery acknowledges the mapped request", |s| s.configures > configures);
    assert!(!after.fullscreen);
    assert_eq!(c.windows()[0].geometry, w.geometry);
}
