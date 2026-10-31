use aizome_hotaru::app::{Hotaru, Msg};
use aizome_kit::testing::Harness;
use hotaru_core::Terminal;
use std::{sync::Arc, time::Duration};
fn show(h: &mut Harness<Hotaru>, term: &Terminal) {
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::Frame(id, Arc::new(term.snapshot())));
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane()
            .unwrap()
            .displayed_snapshot()
            .is_some_and(|shown| Arc::ptr_eq(shown, &app.active_pane().unwrap().snapshot))
    }));
    h.advance(Duration::ZERO);
}
fn image(h: &mut Harness<Hotaru>) -> Vec<u8> {
    let w = h.window();
    h.render(w, 1.).to_rgba8().unwrap()
}
#[test]
fn cursor_shapes_blink_focus_input_and_damage_are_native() {
    let mut h = Harness::new(Hotaru::preview());
    let w = h.window();
    h.advance(Duration::ZERO);
    let mut term = Terminal::new(80, 24, 0);
    term.feed(b"\x1b[1 q");
    show(&mut h, &term);
    let id = h.program().active_pane().unwrap().id;
    h.focus_window(w, true);
    h.send(Msg::FocusPane(id));
    h.advance(Duration::ZERO);
    let on = image(&mut h);
    let _ = h.check_frame(w);
    h.take_damage(w);
    h.advance(Duration::from_millis(600));
    let off = image(&mut h);
    assert_ne!(on, off, "blinking block must disappear");
    let frame = h.check_frame(w);
    assert_eq!(frame.differs, 0.0, "{frame:?}");
    assert!(frame.damaged > 0.0 && frame.damaged < 2000.0, "blink damage is cell-bounded: {frame:?}");
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::PasteText(id, Arc::from("x")));
    assert!(image(&mut h) == on, "paste input resets blink even without PTY output");
    for _ in 0..1000 {
        h.send(Msg::Input(id, vec![]));
    }
    assert!(h.platform.pending_timers() <= 2, "input floods replace, rather than accumulate, timers");
    h.advance(Duration::ZERO);
    h.focus_window(w, false);
    h.advance(Duration::ZERO);
    let hollow = image(&mut h);
    assert!(hollow != on, "hollow differs from block");
    assert!(hollow != off, "hollow differs from hidden");
    assert_eq!(h.platform.pending_timers(), 0);
    h.advance(Duration::from_secs(30));
    assert!(image(&mut h) == hollow);
    h.focus_window(w, true);
    h.advance(Duration::ZERO);
    let mut images = Vec::new();
    for style in [2, 4, 6] {
        term.feed(format!("\x1b[{style} q").as_bytes());
        show(&mut h, &term);
        let a = image(&mut h);
        h.advance(Duration::from_secs(2));
        assert!(a == image(&mut h));
        images.push(a);
        assert_eq!(h.platform.pending_timers(), 0);
    }
    assert!(images[0] != images[1]);
    assert!(images[1] != images[2]);
    assert!(images[0] != images[2]);
    term.feed(b"\x1b[?25l");
    show(&mut h, &term);
    assert!(image(&mut h) == off);
    term.feed(b"\x1b[1 q\x1b[?25h");
    show(&mut h, &term);
    let old = h.platform.timer_tokens();
    h.send(Msg::CloseTab);
    h.advance(Duration::ZERO);
    for token in old {
        h.deliver_timer(token);
    }
    assert_eq!(h.platform.pending_timers(), 0, "destroyed panes retain no blink timer");
}

#[test]
fn wide_cursor_ime_and_physical_scale_remain_cell_aligned() {
    let mut h = Harness::new(Hotaru::preview());
    let w = h.window();
    h.advance(Duration::ZERO);
    let id = h.program().active_pane().unwrap().id;
    h.focus_window(w, true);
    h.send(Msg::FocusPane(id));
    h.advance(Duration::ZERO);
    let mut term = Terminal::new(80, 24, 20);
    term.feed("界A\x1b[H\x1b[2 q".as_bytes());
    show(&mut h, &term);
    for scale in [1., 1.25, 2.] {
        h.output_scale(w, scale);
        h.advance(Duration::ZERO);
        assert!(h.settle(Duration::from_secs(5), |app| {
            app.active_pane().unwrap().font_metrics().is_some_and(|m| m.scale == scale)
        }));
        h.advance(Duration::ZERO);
        term.feed(b"\x1b[H");
        show(&mut h, &term);
        let head = image(&mut h);
        term.feed(b"\x1b[2G");
        show(&mut h, &term);
        let tail = image(&mut h);
        assert!(head == tail, "head and continuation cursor paint must cover the same wide glyph at scale{scale}");
        term.feed(b"\x1b[?25l");
        show(&mut h, &term);
        let hidden = image(&mut h);
        assert!(hidden != head);
        h.ime(w, None, "かな");
        assert!(image(&mut h) != hidden, "preedit overrides DEC visibility");
        assert_eq!(h.platform.pending_timers(), 0);
        h.ime(w, None, "");
        assert!(image(&mut h) == hidden);
        term.feed(b"\x1b[?25h");
        show(&mut h, &term);
    }
    for _ in 0..30 {
        term.feed(b"row\r\n");
    }
    let scrolled = Arc::new(term.snapshot_scrolled(2));
    h.send(Msg::Frame(id, scrolled));
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane().unwrap().displayed_snapshot().is_some_and(|s| !s.cursor.in_viewport)
    }));
    h.advance(Duration::ZERO);
    h.ime(w, None, "かな");
    assert_eq!(h.platform.pending_timers(), 0, "preedit outside the active viewport has no blink wakeups");
}
