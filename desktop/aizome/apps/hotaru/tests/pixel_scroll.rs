use aizome_hotaru::app::{Hotaru, Msg};
use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::testing::{ExactFrameCheck, Harness};
use aizome_kit::widget::{LayoutCx, PaintCx, Widget};
use hotaru_core::{Snapshot, Terminal};
use std::{sync::Arc, time::Duration};

struct Clipped {
    app: Hotaru,
    offset: Point,
    overlay: bool,
}
struct Viewport {
    offset: Point,
}
impl Widget for Viewport {
    fn name(&self) -> &'static str {
        "pixel-scroll-fixture-viewport"
    }
    fn clips(&self) -> bool {
        true
    }
    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        cx.layout_child(0, Constraints::tight(Size::new(960., 640.)));
        cx.place_child(0, self.offset);
        if cx.child_count() > 1 {
            cx.layout_child(1, Constraints::tight(Size::new(200., 100.)));
            cx.place_child(1, Point::new(80., 100.));
        }
        bc.constrain(Size::new(960., 640.))
    }
}
struct Overlay;
impl Widget for Overlay {
    fn name(&self) -> &'static str {
        "pixel-scroll-fixture-overlay"
    }
    fn paint(&self, cx: &mut PaintCx) {
        cx.canvas.fill_rect(cx.bounds(), Color::rgb8(40, 60, 90));
    }
}
impl Program for Clipped {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        self.app.info()
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.app.launched(cx);
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        self.app.update(msg, cx);
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        let mut children = vec![self.app.view(w)];
        if self.overlay {
            children.push(Element::new(Overlay, vec![]));
        }
        Element::new(Viewport { offset: self.offset }, children)
    }
}

fn show(h: &mut Harness<Hotaru>, snapshot: Snapshot) {
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::Frame(id, Arc::new(snapshot)));
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane()
            .unwrap()
            .displayed_snapshot()
            .is_some_and(|shown| Arc::ptr_eq(shown, &app.active_pane().unwrap().snapshot))
    }));
    h.advance(Duration::ZERO);
}
fn exact(h: &mut Harness<Hotaru>, scale: f64) -> ExactFrameCheck {
    let w = h.window();
    let result = h.check_frame_exact(w, scale);
    assert_eq!(result.differing_pixels, 0, "partial raster must equal every full-frame channel: {result:?}");
    result
}

#[test]
fn row_scroll_preserves_exact_pixels_and_reduces_raster_work() {
    let mut h = Harness::new(Hotaru::preview());
    let w = h.window();
    h.advance(Duration::ZERO);
    let id = h.program().active_pane().unwrap().id;
    h.focus_window(w, true);
    h.send(Msg::FocusPane(id));
    let mut term = Terminal::new(80, 20, 256);
    term.feed(b"\x1b[2 q");
    for n in 0..80 {
        term.feed(format!("{n:03} 日本語 😀 e\u{301} != -> styled \x1b[3{}mcolor\x1b[0m\r\n", n % 8).as_bytes());
    }
    show(&mut h, term.snapshot());
    exact(&mut h, 1.0);
    let mut total_copied = 0;
    let mut total_rasterized = 0;
    let mut total_frame = 0;
    for offset in [1, 3, 2, 4, 0, 5, 7, 6, 0] {
        show(&mut h, term.snapshot_scrolled(offset));
        let frame = exact(&mut h, 1.0);
        eprintln!("offset={offset}: {frame:?}");
        assert!(frame.moves > 0, "eligible history scrolling should copy rows");
        total_copied += frame.copied_pixels;
        total_rasterized += frame.rasterized_pixels;
        total_frame += frame.total_pixels;
    }
    assert!(total_copied > 0);
    assert!(total_rasterized < total_frame / 2, "scroll should rasterize substantially less than a full window");
    // Several coherent layouts can arrive before an actual buffer is painted.
    // Pending damage and all preceding copies must compose in order.
    for offset in [1, 2, 4, 3] {
        show(&mut h, term.snapshot_scrolled(offset));
    }
    exact(&mut h, 1.0);
    // A translation can coexist with edited retained rows. Only unchanged row
    // and span identities may survive the copy; the edited destination repaints.
    show(&mut h, term.snapshot());
    exact(&mut h, 1.0);
    term.feed(b"\x1b[10;1H\x1b[31mrewritten retained row\x1b[0m");
    show(&mut h, term.snapshot_scrolled(1));
    assert!(exact(&mut h, 1.0).moves > 0);
    // The previous cursor can be a hollow outline in an inactive window.
    h.focus_window(w, false);
    exact(&mut h, 1.0);
    show(&mut h, term.snapshot_scrolled(2));
    assert!(exact(&mut h, 1.0).moves > 0);
    h.focus_window(w, true);
    exact(&mut h, 1.0);
    // Cursor styles and visibility changes must remove copied old cursor pixels.
    for code in [b"\x1b[4 q".as_slice(), b"\x1b[?25l", b"\x1b[?25h\x1b[6 q"] {
        term.feed(code);
        show(&mut h, term.snapshot_scrolled(1));
        exact(&mut h, 1.0);
        show(&mut h, term.snapshot());
        exact(&mut h, 1.0);
    }
}

#[test]
fn scale_resize_palette_and_ime_choose_safe_repaint_paths() {
    let mut h = Harness::new(Hotaru::preview());
    let w = h.window();
    h.advance(Duration::ZERO);
    let id = h.program().active_pane().unwrap().id;
    h.focus_window(w, true);
    h.send(Msg::FocusPane(id));
    let mut term = Terminal::new(70, 16, 256);
    term.feed(b"\x1b[2 q");
    for n in 0..60 {
        term.feed(format!("{n:03} \x1b[31;4m日本語 text\x1b[0m\r\n").as_bytes());
    }
    show(&mut h, term.snapshot());
    exact(&mut h, 1.);
    for scale in [1., 1.25, 2., 1.] {
        h.output_scale(w, scale);
        h.advance(Duration::ZERO);
        assert!(
            h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().font_metrics().unwrap().scale == scale)
        );
        h.advance(Duration::ZERO);
        exact(&mut h, scale);
        for offset in [1, 3, 2, 0] {
            show(&mut h, term.snapshot_scrolled(offset));
            exact(&mut h, scale);
        }
        term.feed(b"\x1b]4;1;#ffaa88\x07");
        show(&mut h, term.snapshot_scrolled(1));
        let result = exact(&mut h, scale);
        assert_eq!(result.moves, 0, "changed resolved palette must repaint");
        term.feed(b"\x1b]104;1\x07");
        show(&mut h, term.snapshot());
        exact(&mut h, scale);
        h.ime(w, None, "かな");
        exact(&mut h, scale);
        show(&mut h, term.snapshot_scrolled(1));
        assert_eq!(exact(&mut h, scale).moves, 0, "preedit cannot be copied");
        h.ime(w, None, "");
        // Do not paint the IME removal first. Its queued damage must survive
        // the following scroll and remove both original and translated pixels.
        show(&mut h, term.snapshot_scrolled(2));
        exact(&mut h, scale);
        h.resize(w, aizome_kit::Size::new(1000., 700.));
        h.advance(Duration::ZERO);
        exact(&mut h, scale);
        show(&mut h, term.snapshot());
        exact(&mut h, scale);
        h.resize(w, aizome_kit::Size::new(960., 640.));
        h.advance(Duration::ZERO);
        exact(&mut h, scale);
    }
}

#[test]
fn clipped_negative_origins_and_covering_siblings_are_exact() {
    for (offset, overlay) in [
        (Point::new(-40., -110.), false),
        (Point::new(16., 8.), false),
        (Point::new(-40., -110.), true),
        (Point::new(-40.5, -110.5), false),
    ] {
        let mut h = Harness::new(Clipped { app: Hotaru::preview(), offset, overlay });
        let w = h.window();
        h.advance(Duration::ZERO);
        let id = h.program().app.active_pane().unwrap().id;
        h.focus_window(w, true);
        h.send(Msg::FocusPane(id));
        let mut term = Terminal::new(70, 16, 256);
        term.feed(b"\x1b[2 q");
        for n in 0..60 {
            term.feed(format!("{n:03} 日本語 e\u{301} != \x1b[4munderlined\x1b[0m\r\n").as_bytes());
        }
        for scroll in [0, 1, 3, 2, 0] {
            h.send(Msg::Frame(id, Arc::new(term.snapshot_scrolled(scroll))));
            assert!(h.settle(Duration::from_secs(5), |app| {
                app.app
                    .active_pane()
                    .unwrap()
                    .displayed_snapshot()
                    .is_some_and(|shown| Arc::ptr_eq(shown, &app.app.active_pane().unwrap().snapshot))
            }));
            h.advance(Duration::ZERO);
            let check = h.check_frame_exact(w, 1.);
            assert_eq!(check.differing_pixels, 0, "offset={offset:?} overlay={overlay}: {check:?}");
            if overlay || offset.x.fract() != 0. {
                assert_eq!(check.moves, 0);
            } else if scroll != 0 {
                assert!(check.moves > 0, "aligned clipped region can move: {check:?}");
            }
        }
    }
}

#[test]
fn selection_find_and_link_hover_never_leave_copied_overlays() {
    use aizome_kit::sidestep_ui::{Key, Modifiers};
    let mut h = Harness::new(Hotaru::preview());
    let w = h.window();
    h.advance(Duration::ZERO);
    let id = h.program().active_pane().unwrap().id;
    h.focus_window(w, true);
    h.send(Msg::FocusPane(id));
    let mut term = Terminal::new(70, 16, 256);
    term.feed(b"\x1b[2 q");
    for n in 0..60 {
        term.feed(
            format!("{n:03} \x1b]8;id=link;https://example.test/\x1b\\needle\x1b]8;;\x1b\\ 日本語\r\n").as_bytes(),
        );
    }
    show(&mut h, term.snapshot());
    exact(&mut h, 1.);
    let pane = h.program().active_pane().unwrap();
    let metrics = pane.font_metrics().unwrap();
    let point = pane.rect.origin() + Vec2::new(10. + metrics.cell_width() * 4.5, 10. + metrics.cell_height() * 2.5);
    h.drag(w, point, point + Vec2::new(metrics.cell_width() * 4., 0.), 4);
    assert!(h.program().active_pane().unwrap().selection.is_some());
    show(&mut h, term.snapshot_scrolled(1));
    assert_eq!(exact(&mut h, 1.).moves, 0, "selection is a redraw context");
    h.send(Msg::Input(id, Vec::new()));
    show(&mut h, term.snapshot());
    exact(&mut h, 1.);
    h.key(w, Key::Unidentified, Modifiers::SUPER);
    h.move_to(w, point);
    assert!(h.texts(w).iter().any(|s| s == "https://example.test/"));
    exact(&mut h, 1.);
    show(&mut h, term.snapshot_scrolled(1));
    assert_eq!(exact(&mut h, 1.).moves, 0, "old hover underline must not move");
    h.key(w, Key::Unidentified, Modifiers::NONE);
    h.send(Msg::Find);
    h.send(Msg::Search("needle".into()));
    assert!(h.settle(Duration::from_secs(5), |app| app.search_status().contains(" of ")));
    show(&mut h, term.snapshot_scrolled(2));
    assert_eq!(exact(&mut h, 1.).moves, 0, "matched and selected Find spans must redraw");
    h.send(Msg::CloseFind);
    show(&mut h, term.snapshot());
    exact(&mut h, 1.);
    show(&mut h, term.snapshot_scrolled(1));
    assert!(exact(&mut h, 1.).moves > 0, "copying resumes once overlay context is gone");
}
