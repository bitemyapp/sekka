//! Terminal selection behavior across asynchronous captures, scrolling and resets.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::{Hotaru, Msg};
use crate::terminal_view::{CellPoint, Selection, TextSelection};
use aizome_kit::{Vec2, sidestep_ui::PointerButton, testing::Harness};
use hotaru_core::Terminal;
use hotaru_io::PtyCommand;

fn settle_frame(h: &mut Harness<Hotaru>) {
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane()
            .unwrap()
            .displayed_snapshot()
            .is_some_and(|shown| Arc::ptr_eq(shown, &app.active_pane().unwrap().snapshot))
    }));
}

fn history(text: &str) -> Terminal {
    let mut terminal = Terminal::new(80, 24, 100);
    for _ in 0..60 {
        terminal.feed(format!("{text}\r\n").as_bytes());
    }
    terminal
}

fn oldest_line(terminal: &Terminal) -> TextSelection {
    let first = terminal.snapshot_scrolled(terminal.scrollback_len());
    TextSelection::on(
        Selection { anchor: CellPoint { row: 0, col: 0 }, end: CellPoint { row: 0, col: 6 } },
        &first.position,
    )
}

#[test]
fn primary_capture_must_not_consume_other_panes_clipboard_capture() {
    let mut h = Harness::new(Hotaru::preview());
    let win = h.window();
    let a = h.program().active_pane().unwrap().id;
    let terminal_a = history("pane A");
    h.send(Msg::Frame(a, Arc::new(terminal_a.snapshot())));
    settle_frame(&mut h);
    h.send(Msg::Select(a, oldest_line(&terminal_a)));
    h.send(Msg::Selected(a)); // Primary capture, token 1.

    h.send(Msg::NewTab(win));
    let b = h.program().active_pane().unwrap().id;
    let terminal_b = history("pane B");
    h.send(Msg::Frame(b, Arc::new(terminal_b.snapshot())));
    settle_frame(&mut h);
    h.send(Msg::Select(b, oldest_line(&terminal_b)));
    h.send(Msg::Copy(win)); // Clipboard capture, also token 1.

    // The preview has no PTYs: deliver the two legitimate actor replies in order.
    h.send(Msg::Captured(1, a, Arc::new(terminal_a.content_snapshot())));
    h.send(Msg::Captured(1, b, Arc::new(terminal_b.content_snapshot())));
    let deadline = Instant::now() + Duration::from_secs(5);
    while (h.platform.primary.as_deref() != Some("pane A") || h.platform.clipboard.as_deref() != Some("pane B"))
        && Instant::now() < deadline
    {
        h.settle(Duration::from_millis(10), |_| false);
    }
    assert_eq!(h.platform.primary.as_deref(), Some("pane A"));
    assert_eq!(h.platform.clipboard.as_deref(), Some("pane B"));
}

#[test]
fn reset_during_drag_must_not_reuse_an_old_epoch_anchor() {
    let mut h = Harness::new(Hotaru::preview());
    let win = h.window();
    let id = h.program().active_pane().unwrap().id;
    let mut terminal = history("old text");
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    settle_frame(&mut h);
    let pane = h.program().active_pane().unwrap();
    let metrics = pane.font_metrics().unwrap();
    let origin = h.find_id(win, &format!("terminal-{id}")).unwrap().origin() + Vec2::new(10., 10.);
    let at = |row: f64| origin + Vec2::new(metrics.cell_width(), (row + 0.5) * metrics.cell_height());
    h.press(win, at(2.), PointerButton::Left, 1);
    h.move_to(win, at(4.));
    terminal.reset();
    terminal.feed(b"new text\r\n");
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    settle_frame(&mut h);
    assert!(h.program().active_pane().unwrap().selection.is_none());
    h.move_to(win, at(5.));
    assert!(
        h.program().active_pane().unwrap().selection.is_none(),
        "old drag was restored: {:?}",
        h.program().active_pane().unwrap().selection
    );
}

#[test]
fn find_selection_uses_text_scrolled_out_of_view() {
    // Keep a real PTY owner alive after EOF so an asynchronous implementation
    // can capture the selected text when Find requests it.
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), "i=0; while [ $i -lt 100 ]; do printf 'needle\\n'; i=$((i + 1)); done".into()];
    let mut app = Hotaru::new(command);
    app.settings.wait_after_command = true;
    let mut h = Harness::new(app);
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
    settle_frame(&mut h);
    let win = h.window();
    let pane = h.program().active_pane().unwrap();
    let id = pane.id;
    let selection = TextSelection::on(
        Selection { anchor: CellPoint { row: 0, col: 0 }, end: CellPoint { row: 0, col: 6 } },
        &pane.snapshot.position,
    );
    assert_eq!(selection.text(&pane.snapshot), "needle");
    h.send(Msg::Select(id, selection));
    h.send(Msg::Scroll(id, i32::MAX));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().snapshot.position.first_row == 0));
    settle_frame(&mut h);
    let pane = h.program().active_pane().unwrap();
    assert!(pane.selection.as_ref().unwrap().visible(&pane.snapshot).is_none());
    h.send(Msg::Action(crate::settings::Action::FindSelection));
    h.settle(Duration::from_secs(5), |app| app.window(win).unwrap().search == "needle");
    assert_eq!(h.program().window(win).unwrap().search, "needle");
}

struct ObserveScroll {
    app: Hotaru,
    scrolls: usize,
}
impl aizome_kit::Program for ObserveScroll {
    type Msg = Msg;
    fn info(&self) -> aizome_kit::AppInfo {
        self.app.info()
    }
    fn launched(&mut self, cx: &mut aizome_kit::Cx<Msg>) {
        self.app.launched(cx);
    }
    fn update(&mut self, msg: Msg, cx: &mut aizome_kit::Cx<Msg>) {
        if matches!(msg, Msg::Scroll(_, _)) {
            self.scrolls += 1;
        }
        self.app.update(msg, cx);
    }
    fn view(&self, window: aizome_kit::WindowKey) -> aizome_kit::Element<Msg> {
        self.app.view(window)
    }
}

#[test]
fn autoscroll_continues_while_pointer_moves_outside_pane() {
    let mut h = Harness::new(ObserveScroll { app: Hotaru::preview(), scrolls: 0 });
    let win = h.window();
    let id = h.program().app.active_pane().unwrap().id;
    let terminal = history("some text");
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    assert!(h.settle(Duration::from_secs(5), |p| {
        p.app
            .active_pane()
            .unwrap()
            .displayed_snapshot()
            .is_some_and(|shown| Arc::ptr_eq(shown, &p.app.active_pane().unwrap().snapshot))
    }));
    let pane = h.program().app.active_pane().unwrap();
    let metrics = pane.font_metrics().unwrap();
    let rect = h.find_id(win, &format!("terminal-{id}")).unwrap();
    let origin = rect.origin() + Vec2::new(10., 10.);
    h.press(win, origin + Vec2::new(0., 2.5 * metrics.cell_height()), PointerButton::Left, 1);
    for i in 0..100 {
        h.move_to(win, rect.origin() + Vec2::new(10. + f64::from(i % 2), -20.));
        h.advance(Duration::from_millis(10));
    }
    let while_moving = h.program().scrolls;
    // Also prove the pointer is in the autoscroll zone: pausing it fires
    // the timer in the broken implementation, too.
    h.advance(Duration::from_millis(50));
    h.settle(Duration::from_millis(10), |_| false);
    assert!(h.program().scrolls > 0, "the drag never entered the autoscroll zone");
    assert!(while_moving > 0, "no autoscroll in one second of pointer motion beyond the pane");
}
