use std::sync::Arc;

use aizome_hotaru::app::{Hotaru, Msg};
use aizome_hotaru::panes::{Axis, Direction};
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use hotaru_core::Terminal;
use hotaru_io::PtyCommand;
use std::time::{Duration, Instant};

/// Hotaru keeping panes whose program has ended, for tests that read what
/// a finished command left on its screen.
fn keeping(command: PtyCommand) -> Hotaru {
    let mut app = Hotaru::new(command);
    app.settings.wait_after_command = true;
    app
}

fn settle_frame(h: &mut Harness<Hotaru>) {
    assert!(
        h.settle(Duration::from_secs(5), |app| app
            .active_pane()
            .is_some_and(|pane| pane.displayed_snapshot().is_some_and(|shown| Arc::ptr_eq(shown, &pane.snapshot)))),
        "font worker did not present current snapshot"
    );
}

#[test]
fn osc8_super_click_uses_displayed_cell_and_recorded_desktop_opener() {
    use aizome_kit::Vec2;
    use aizome_kit::sidestep_ui::{Key, Modifiers, PointerButton};
    let mut h = Harness::new(Hotaru::preview());
    let win = h.window();
    h.advance(Duration::ZERO);
    let id = h.program().active_pane().unwrap().id;
    let target = "https://example.test/日本?x=$(false)&quote='literal'";
    let mut term = Terminal::new(80, 24, 0);
    term.feed(format!("\x1b]8;id=link;{target}\x1b\\界ab\x1b]8;;\x1b\\ plain").as_bytes());
    h.send(Msg::Frame(id, Arc::new(term.snapshot())));
    settle_frame(&mut h);
    let pane = h.program().active_pane().unwrap();
    let metrics = pane.font_metrics().unwrap();
    let origin = pane.rect.origin() + Vec2::new(10., 10.);
    let at = |col: f64| origin + Vec2::new(col * metrics.cell_width(), metrics.cell_height() * 0.4);
    // The right half of the last link cell must still hit that cell, not the
    // selection boundary after it. Wide continuation cell 1 is also clickable.
    h.key(win, Key::Unidentified, Modifiers::SUPER);
    h.move_to(win, at(3.9));
    assert!(h.texts(win).iter().any(|text| text == target));
    assert!(h.platform.cursors.values().any(|cursor| *cursor == aizome_kit::sidestep_ui::Cursor::Pointer));
    h.click(win, at(3.9));
    h.click(win, at(1.8));
    assert_eq!(h.platform.opened_urls, vec![target, target]);
    h.click(win, at(4.1));
    assert_eq!(h.platform.opened_urls.len(), 2);
    // No modifier preserves ordinary pointer selection and never opens.
    h.key(win, Key::Unidentified, Modifiers::NONE);
    h.click(win, at(0.4));
    assert_eq!(h.platform.opened_urls.len(), 2);
    // Motion cancels even when the pointer returns to its original cell.
    h.key(win, Key::Unidentified, Modifiers::SUPER);
    h.press(win, at(0.4), PointerButton::Left, 1);
    h.move_to(win, at(0.5));
    h.move_to(win, at(0.4));
    h.release(win, at(0.4), PointerButton::Left);
    assert_eq!(h.platform.opened_urls.len(), 2);
    h.press(win, at(0.4), PointerButton::Left, 1);
    h.key(win, Key::Unidentified, Modifiers::NONE);
    h.release(win, at(0.4), PointerButton::Left);
    assert_eq!(h.platform.opened_urls.len(), 2);
    // A newly displayed snapshot cancels an otherwise identical target.
    h.key(win, Key::Unidentified, Modifiers::SUPER);
    h.press(win, at(0.4), PointerButton::Left, 1);
    term.feed(b"\x1b[2;1Hchanged");
    h.send(Msg::Frame(id, Arc::new(term.snapshot())));
    settle_frame(&mut h);
    h.release(win, at(0.4), PointerButton::Left);
    assert_eq!(h.platform.opened_urls.len(), 2);
    h.platform.open_url_rejection = Some(aizome_kit::urls::OpenUrlError::Busy);
    h.click(win, at(0.4));
    assert_eq!(h.platform.opened_urls.len(), 2);
    assert!(h.program().active_pane().unwrap().status.contains("too many"));
    h.platform.open_url_rejection = None;
    h.platform.open_url_result = Err(aizome_kit::urls::OpenUrlError::Exit(Some(3)));
    h.advance(Duration::ZERO);
    h.move_to(win, at(6.5));
    h.move_to(win, at(0.4));
    assert!(h.texts(win).iter().any(|text| text == target), "error fixture starts with a visible hovered target");
    h.click(win, at(0.4));
    assert!(h.settle(Duration::from_secs(1), |app| app.active_pane().unwrap().status.contains("exit status 3")));
    assert!(h.texts(win).iter().any(|text| text.contains("Could not open link:") && text.contains("exit status 3")));
    // A stationary pointer must not let the hovered target mask an opener
    // error. Exercise the actual view, including subsequent idle timer work.
    h.advance(Duration::from_millis(1));
    h.settle(Duration::ZERO, |_| false);
    assert!(h.texts(win).iter().any(|text| text.contains("Could not open link:") && text.contains("exit status 3")));
    assert!(!h.texts(win).iter().any(|text| text == target));
}

#[test]
fn osc8_gesture_consumes_both_mouse_edges_and_preserves_other_clicks() {
    use aizome_kit::sidestep_ui::{Key, Modifiers, PointerButton};
    let mut command = PtyCommand::new(std::env::current_exe().unwrap());
    command.args = ["--ignored", "--exact", "osc8_mouse_child", "--nocapture", "--test-threads=1"]
        .into_iter()
        .map(Into::into)
        .collect();
    command.env.push(("HOTARU_LINK_CHILD".into(), "1".into()));
    let mut h = Harness::new(keeping(command));
    h.advance(Duration::ZERO);
    assert!(
        h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().snapshot.plain_text().contains("LINK-READY"))
    );
    settle_frame(&mut h);
    let win = h.window();
    let pane = h.program().active_pane().unwrap();
    let id = pane.id;
    let metrics = pane.font_metrics().unwrap();
    let origin = pane.rect.origin() + aizome_kit::Vec2::new(10., 10.);
    let at = |col: f64| origin + aizome_kit::Vec2::new(col * metrics.cell_width(), metrics.cell_height() * 0.4);
    h.key(win, Key::Unidentified, Modifiers::SUPER);
    h.click(win, at(1.5));
    h.press(win, at(1.5), PointerButton::Left, 1);
    h.move_to(win, at(1.6));
    h.release(win, at(1.6), PointerButton::Left);
    h.key(win, Key::Unidentified, Modifiers::NONE);
    h.click(win, at(6.5));
    h.send(Msg::Input(id, b"done\n".to_vec()));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
    let text = h.program().active_pane().unwrap().snapshot.plain_text();
    assert_eq!(h.platform.opened_urls, vec!["https://example.test"], "{text}");
    assert!(text.contains("LINK-INPUT-OK"), "{text}");
}

#[test]
#[ignore]
fn osc8_mouse_child() {
    if std::env::var_os("HOTARU_LINK_CHILD").is_none() {
        return;
    }
    use std::io::{Read, Write};
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: this isolated subprocess owns its PTY; termios is initialized
    // before access and each libc call receives a live, correctly sized value.
    unsafe {
        assert_eq!(libc::tcgetattr(0, attributes.as_mut_ptr()), 0);
        let mut attributes = attributes.assume_init();
        libc::cfmakeraw(&mut attributes);
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &attributes), 0);
    }
    std::io::stdout()
        .write_all(
            b"\x1b[2J\x1b[H\x1b[?1000h\x1b[?1006h\x1b]8;;https://example.test\x1b\\link\x1b]8;;\x1b\\ plain LINK-READY",
        )
        .unwrap();
    std::io::stdout().flush().unwrap();
    let mut actual = Vec::new();
    loop {
        let mut byte = [0];
        std::io::stdin().read_exact(&mut byte).unwrap();
        actual.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    assert_eq!(actual, b"\x1b[<0;7;1M\x1b[<0;7;1mdone\n");
    println!("LINK-INPUT-OK");
}

/// Wait until the pane shows its fonts and its grid holds still.
fn settle_grid(h: &mut Harness<Hotaru>) {
    settle_frame(h);
    let grid = |h: &Harness<Hotaru>| h.program().active_pane().map(|pane| (pane.snapshot.cols, pane.snapshot.rows));
    let until = Instant::now() + Duration::from_secs(5);
    let (mut last, mut steady) = (grid(h), 0);
    while steady < 6 && Instant::now() < until {
        h.advance(Duration::from_millis(50));
        h.settle(Duration::from_millis(20), |_| false);
        let now = grid(h);
        steady = if now == last { steady + 1 } else { 0 };
        last = now;
    }
}

/// Wait for `done`, letting the harness clock run: a search refresh after
/// output waits on a timer, which `settle` alone never fires.
fn advancing(h: &mut Harness<Hotaru>, done: impl Fn(&Hotaru) -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    while !done(h.program()) {
        if Instant::now() > until {
            return false;
        }
        h.advance(Duration::from_millis(50));
        h.settle(Duration::from_millis(10), &done);
    }
    true
}

fn settle_search(h: &mut Harness<Hotaru>, expected: &str) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |app| app.search_status() == expected);
        if h.program().search_status() == expected
            && (expected.starts_with('0') || h.program().search_selected().is_some())
        {
            return;
        }
        assert!(Instant::now() < until, "search status {:?}, expected {expected:?}", h.program().search_status());
    }
}

#[test]
fn internal_panes_inherit_reported_cwd_but_external_launch_keeps_its_directory() {
    let mut h = Harness::new(Hotaru::preview());
    let first = h.program().active_pane().unwrap().id;
    h.send(Msg::WorkingDirectory(first, "/tmp/reported-directory".into()));
    h.send(Msg::Split(Axis::Right));
    assert_eq!(
        h.program().active_pane().unwrap().working_directory.as_deref(),
        Some(std::path::Path::new("/tmp/reported-directory"))
    );
    let split = h.program().active_pane().unwrap().id;
    h.send(Msg::WorkingDirectory(split, "/tmp/split-directory".into()));
    h.send(Msg::FocusPane(first));
    h.send(Msg::NewTab);
    assert_eq!(
        h.program().active_pane().unwrap().working_directory.as_deref(),
        Some(std::path::Path::new("/tmp/reported-directory"))
    );
    let mut explicit = PtyCommand::shell();
    explicit.cwd = Some("/tmp/requested-directory".into());
    h.send(Msg::Launch(explicit));
    assert_eq!(
        h.program().active_pane().unwrap().working_directory.as_deref(),
        Some(std::path::Path::new("/tmp/requested-directory"))
    );
}

#[test]
fn new_tabs_and_splits_follow_the_focused_shell_without_directory_reports() {
    let root = std::env::temp_dir().join(format!("hotaru-tab-cwd-{}", std::process::id()));
    let first_dir = root.join("first space 日本");
    let second_dir = root.join("second");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();
    // Exercise both a configured shell with integration disabled and an
    // explicit shell command. Neither fixture emits OSC 7 or reads dotfiles.
    for default_shell in [true, false] {
        let mut command = PtyCommand::new("/bin/sh");
        command.integrate_shell = default_shell;
        command.cwd = Some(root.clone());
        command.args = vec![
            "-c".into(),
            "stty -echo; printf 'START:%s\\n' \"$PWD\"; while IFS= read -r dir; do cd -- \"$dir\" || exit 1; printf 'CHANGED:%s\\n' \"$PWD\"; done".into(),
        ];
        let mut app = keeping(command.clone());
        app.settings.shell = Some("/bin/sh".into());
        app.settings.shell_integration = false;
        let mut h = Harness::new(app);
        let started = |h: &mut Harness<Hotaru>, id, path: &std::path::Path| {
            let expected = format!("START:{}", path.display());
            assert!(
                h.settle(Duration::from_secs(5), |app| app
                    .panes
                    .iter()
                    .any(|pane| { pane.id == id && pane.snapshot.plain_text().contains(&expected) })),
                "pane {id} did not start in {}",
                path.display()
            );
        };
        let cd = |h: &mut Harness<Hotaru>, id, path: &std::path::Path| {
            h.send(Msg::Input(id, format!("{}\n", path.display()).into_bytes()));
            let expected = format!("CHANGED:{}", path.display());
            assert!(h.settle(Duration::from_secs(5), |app| {
                app.panes.iter().any(|pane| pane.id == id && pane.snapshot.plain_text().contains(&expected))
            }));
        };
        let first = h.program().active_pane().unwrap().id;
        started(&mut h, first, &root);
        cd(&mut h, first, &first_dir);
        h.send(Msg::NewTab);
        let second = h.program().active_pane().unwrap().id;
        started(&mut h, second, &first_dir);
        cd(&mut h, second, &second_dir);
        h.send(Msg::SelectTab(first));
        h.send(Msg::NewTab);
        let third = h.program().active_pane().unwrap().id;
        // A later focus change cannot redirect the pending launch's lookup.
        h.send(Msg::SelectTab(second));
        started(&mut h, third, &first_dir);
        h.send(Msg::Split(Axis::Right));
        let split = h.program().active_pane().unwrap().id;
        started(&mut h, split, &second_dir);
        cd(&mut h, split, &first_dir);
        h.send(Msg::FocusPane(second));
        h.send(Msg::NewTab);
        let fourth = h.program().active_pane().unwrap().id;
        started(&mut h, fourth, &second_dir);
        // A nested shell's accepted report takes precedence over the local
        // parent shell's directory (first_dir).
        h.send(Msg::WorkingDirectory(first, second_dir.clone()));
        h.send(Msg::SelectTab(first));
        h.send(Msg::NewTab);
        let reported = h.program().active_pane().unwrap().id;
        started(&mut h, reported, &second_dir);
        h.send(Msg::Launch(command));
        let external = h.program().active_pane().unwrap().id;
        started(&mut h, external, &root);
        h.send(Msg::Input(external, vec![4]));
        assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
        h.send(Msg::NewTab);
        let after_exit = h.program().active_pane().unwrap().id;
        started(&mut h, after_exit, &root);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn actor_accepts_local_cwd_without_overwriting_an_explicit_title() {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), "printf '\\033]2;User title\\007\\033]7;file://localhost/tmp/a%%20b\\007'".into()];
    let mut h = Harness::new(keeping(command));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
    let pane = h.program().active_pane().unwrap();
    assert_eq!(pane.title, "User title");
    assert_eq!(pane.working_directory.as_deref(), Some(std::path::Path::new("/tmp/a b")));
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), "printf '\\033]7;file://remote/ignored\\007'".into()];
    let mut remote = Harness::new(keeping(command));
    assert!(remote.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
    assert!(remote.program().active_pane().unwrap().working_directory.is_none());
    let mut command = PtyCommand::new("/bin/sh");
    command.cwd = Some("/tmp".into());
    command.args = vec!["-c".into(), "printf '\\033]7;\\007'".into()];
    let mut cleared = Harness::new(keeping(command));
    assert!(cleared.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
    assert!(cleared.program().active_pane().unwrap().working_directory.is_none());
}

#[test]
fn find_navigates_owned_history_and_wraps_in_both_directions() {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec![
        "-c".into(),
        "i=0; while [ $i -lt 120 ]; do printf 'Needle %03d\\n' \"$i\"; i=$((i + 1)); done; printf 'history-ready\\n'"
            .into(),
    ];
    let mut h = Harness::new(keeping(command));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
    h.send(Msg::Find);
    h.send(Msg::Search("needle".into()));
    settle_search(&mut h, "1 of 120");
    assert!(h.settle(Duration::from_secs(3), |app| app.search_selected().is_some()));
    let latest = h.program().search_selected().unwrap();
    h.send(Msg::SearchStep(false));
    settle_search(&mut h, "2 of 120");
    assert!(h.settle(Duration::from_secs(3), |app| {
        app.search_selected().is_some_and(|found| found.start.row < latest.start.row)
    }));
    h.send(Msg::SearchStep(true));
    settle_search(&mut h, "1 of 120");
    h.send(Msg::SearchStep(true));
    settle_search(&mut h, "120 of 120");
    assert!(h.settle(Duration::from_secs(3), |app| {
        let pane = app.active_pane().unwrap();
        app.search_selected().is_some_and(|found| found.start.row < pane.snapshot.position.history_rows)
            && pane.snapshot.plain_text().contains("Needle 000")
    }));
    h.send(Msg::Copy);
    let until = Instant::now() + Duration::from_secs(3);
    while h.platform.clipboard.as_deref() != Some("Needle") && Instant::now() < until {
        h.advance(Duration::from_millis(1));
        h.settle(Duration::from_millis(10), |_| false);
    }
    assert_eq!(
        h.platform.clipboard.as_deref(),
        Some("Needle"),
        "status {}; search {}; selected {:?}",
        h.program().active_pane().unwrap().status,
        h.program().search_status(),
        h.program().search_selected()
    );
    let complete = (0..120).map(|n| format!("Needle {n:03}")).collect::<Vec<_>>().join("\n");
    h.send(Msg::Search(complete.clone()));
    settle_search(&mut h, "1 of 1");
    h.platform.clipboard = None;
    h.send(Msg::Copy);
    let until = Instant::now() + Duration::from_secs(3);
    while h.platform.clipboard.as_deref() != Some(complete.as_str()) && Instant::now() < until {
        h.advance(Duration::from_millis(1));
        h.settle(Duration::from_millis(10), |_| false);
    }
    assert_eq!(
        h.platform.clipboard.as_deref(),
        Some(complete.as_str()),
        "match copy must include rows outside the displayed viewport"
    );
}

#[test]
fn find_soft_wrap_wide_text_and_copy_use_complete_owned_match() {
    let mut h = Harness::new(Hotaru::preview());
    let id = h.program().active_pane().unwrap().id;
    let mut terminal = Terminal::new(6, 4, 20);
    terminal.feed("ab界cDE".as_bytes());
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    h.send(Msg::Find);
    h.send(Msg::Search("界cDE".into()));
    settle_search(&mut h, "1 of 1");
    assert!(h.settle(Duration::from_secs(3), |app| app.search_selected().is_some()));
    let found = h.program().search_selected().unwrap();
    assert_eq!((found.start.row, found.start.col, found.end.row, found.end.col), (0, 2, 1, 1));
    h.send(Msg::Copy);
    let until = Instant::now() + Duration::from_secs(3);
    while h.platform.clipboard.as_deref() != Some("界cDE") && Instant::now() < until {
        h.advance(Duration::from_millis(1));
        h.settle(Duration::from_millis(10), |_| false);
    }
    assert_eq!(h.platform.clipboard.as_deref(), Some("界cDE"));
    // A changed active row must invalidate the saved navigation anchor.
    terminal.feed(b"\r\x1b[1;1HZ");
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    assert!(h.program().search_selected().is_none());
}

#[test]
fn obsolete_searches_and_closed_panes_cannot_replace_the_active_query() {
    let mut h = Harness::new(Hotaru::preview());
    let first = h.program().active_pane().unwrap().id;
    let mut old = Terminal::new(4000, 128, 0);
    old.feed(&vec![b'a'; 4000 * 128]);
    h.send(Msg::Frame(first, Arc::new(old.snapshot())));
    h.send(Msg::Find);
    h.send(Msg::Search("aa".into()));
    h.send(Msg::NewTab);
    h.send(Msg::Search("little light".into()));
    settle_search(&mut h, "1 of 1");
    let second = h.program().active_pane().unwrap().id;
    h.send(Msg::SelectTab(first));
    h.send(Msg::CloseTab);
    h.send(Msg::SearchSnapshot(u64::MAX, first, Arc::new(old.content_snapshot())));
    settle_search(&mut h, "1 of 1");
    assert_eq!(h.program().active_pane().unwrap().id, second);
    assert_eq!(h.program().search, "little light");
}

#[test]
fn find_retains_historical_anchors_through_append_and_recovers_after_alternate_and_reflow() {
    let mut command = PtyCommand::new(std::env::current_exe().unwrap());
    command.args = ["--ignored", "--exact", "search_history_child", "--nocapture", "--test-threads=1"]
        .into_iter()
        .map(Into::into)
        .collect();
    command.env.push(("HOTARU_SEARCH_CHILD".into(), "1".into()));
    let mut app = keeping(command);
    app.settings.scrollback = 60;
    let mut h = Harness::new(app);
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane().unwrap().snapshot.plain_text().contains("ready-initial")
    }));
    // Search once the grid has its final size. On a loaded machine the fonts
    // can arrive after the output, and the resize they bring reflows, which
    // rightly drops a selected match's anchor.
    settle_grid(&mut h);
    h.send(Msg::Find);
    h.send(Msg::Search("Mark".into()));
    for _ in 0..20 {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |app| app.search_selected().is_some());
    }
    assert!(h.program().search_selected().is_some());
    for _ in 0..30 {
        h.send(Msg::SearchStep(false));
    }
    assert!(advancing(&mut h, |app| {
        app.search_selected()
            .is_some_and(|found| found.start.row < app.active_pane().unwrap().snapshot.position.history_rows)
    }));
    let absolute = h.program().active_pane().unwrap().snapshot.position.origin
        + h.program().search_selected().unwrap().start.row as u64;
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::Input(id, b"append\n".to_vec()));
    assert!(h.settle(Duration::from_secs(3), |app| {
        app.active_pane().unwrap().snapshot.plain_text().contains("ready-append")
    }));
    for _ in 0..10 {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |_| false);
    }
    assert!(advancing(&mut h, |app| app.search_selected().is_some()), "the match survives appended output");
    let pane = h.program().active_pane().unwrap();
    assert_eq!(pane.snapshot.position.origin + h.program().search_selected().unwrap().start.row as u64, absolute);
    h.send(Msg::Search("Mark".into()));
    for _ in 0..10 {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |_| false);
    }
    h.send(Msg::SearchStep(true));
    assert!(advancing(&mut h, |app| app.search_selected().is_some_and(|found| found.start.row == 0)));
    h.send(Msg::Input(id, b"prune\n".to_vec()));
    assert!(h.settle(Duration::from_secs(3), |app| {
        app.active_pane().unwrap().snapshot.plain_text().contains("ready-prune")
    }));
    for _ in 0..10 {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |_| false);
    }
    assert!(h.program().search_selected().is_none(), "an evicted match cannot remain selected");
    h.send(Msg::SearchStep(false));
    assert!(advancing(&mut h, |app| app.search_selected().is_some()));
    h.send(Msg::Input(id, b"alternate\n".to_vec()));
    assert!(h.settle(Duration::from_secs(3), |app| app.active_pane().unwrap().snapshot.modes.alternate_screen));
    settle_search(&mut h, "1 of 1");
    h.send(Msg::Input(id, b"primary\n".to_vec()));
    assert!(h.settle(Duration::from_secs(3), |app| !app.active_pane().unwrap().snapshot.modes.alternate_screen));
    for _ in 0..10 {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |_| false);
    }
    assert_ne!(h.program().search_status(), "1 of 1");
    let before = h.program().active_pane().unwrap().snapshot.position.epoch.clone();
    h.resize(h.window(), aizome_kit::Size::new(600.0, 420.0));
    h.advance(Duration::from_millis(1));
    assert!(h.settle(Duration::from_secs(3), |app| app.active_pane().unwrap().snapshot.position.epoch != before));
    for _ in 0..10 {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |_| false);
    }
    h.send(Msg::SearchStep(false));
    assert!(advancing(&mut h, |app| app.search_selected().is_some()));
}

#[test]
#[ignore]
fn search_history_child() {
    if std::env::var_os("HOTARU_SEARCH_CHILD").is_none() {
        return;
    }
    use std::io::{BufRead, Write};
    for index in 0..100 {
        println!("Mark {index:04}");
    }
    println!("ready-initial");
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "append" => {
                for index in 100..103 {
                    println!("Mark {index:04}");
                }
                println!("ready-append");
            }
            "prune" => {
                for index in 103..223 {
                    println!("Mark {index:04}");
                }
                println!("ready-prune");
            }
            "alternate" => print!("\x1b[?1049h\x1b[2J\x1b[HMark alternate\r\nready-alternate\r\n"),
            "primary" => print!("\x1b[?1049l"),
            _ => break,
        }
        std::io::stdout().flush().unwrap();
    }
}

#[test]
fn output_scale_recomputes_physical_grid_without_logical_resize() {
    let mut h = Harness::new(Hotaru::preview());
    let window = h.window();
    settle_frame(&mut h);
    for scale in [1.0, 1.25, 2.0, 1.0] {
        h.output_scale(window, scale);
        h.render(window, scale);
        h.advance(Duration::ZERO);
        assert!(
            h.settle(Duration::from_secs(5), |app| {
                app.active_pane().unwrap().font_metrics().is_some_and(|m| m.scale == scale)
            }),
            "font worker did not adopt output scale {scale}"
        );
        let pane = h.program().active_pane().unwrap();
        let metrics = pane.font_metrics().unwrap();
        assert_eq!(metrics.cell_width() * scale, f64::from(metrics.cell_width_px));
        assert_eq!(metrics.cell_height() * scale, f64::from(metrics.cell_height_px));
        assert!(pane.resolved_font_family().is_some_and(|family| !family.is_empty()));
        assert_eq!(h.render_rgba(window, scale).0, (960.0 * scale) as u32);
    }
}

#[test]
#[ignore = "requires pinned HOTARU_FONT_DIR; run font fixture suite explicitly"]
fn packaged_color_emoji_rasterizes_in_the_terminal_grid() {
    assert!(std::env::var_os("HOTARU_FONT_DIR").is_some());
    let mut h = Harness::new(Hotaru::preview());
    h.advance(Duration::ZERO);
    let id = h.program().active_pane().unwrap().id;
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.feed("\x1b[?25l日本語 😀👍🏽👩\u{200d}💻🇯🇵❤\u{fe0f} ❤\u{fe0e} a\u{301}M".as_bytes());
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    settle_frame(&mut h);
    for scale in [1.0, 1.25, 2.0] {
        h.output_scale(h.window(), scale);
        h.render(h.window(), scale);
        h.advance(Duration::ZERO);
        assert!(
            h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().font_metrics().unwrap().scale == scale)
        );
        h.advance(Duration::ZERO);
        let pane = h.program().active_pane().unwrap();
        let metrics = pane.font_metrics().unwrap();
        let pane_rect = pane.rect;
        let (width, _, pixels) = h.render_rgba(h.window(), scale);
        let pad = (10.0 * scale).round();
        let left = (pane_rect.x0 * scale + pad) as usize;
        let top = (pane_rect.y0 * scale + pad) as usize;
        let row_pixels: Vec<_> = (top..top + metrics.cell_height_px as usize)
            .flat_map(|y| (left..left + 40 * metrics.cell_width_px as usize).map(move |x| (y * width as usize + x) * 4))
            .collect();
        // The terminal uses white-on-black; these saturated yellow pixels
        // can only come from the actual color emoji raster, not a tofu box.
        assert!(
            row_pixels.iter().filter(|&&i| pixels[i] > 180 && pixels[i + 1] > 100 && pixels[i + 2] < 90).count() > 20,
            "no emoji color pixels at scale {scale}"
        );
        if let Some(directory) = std::env::var_os("HOTARU_FONT_EVIDENCE") {
            let path = std::path::PathBuf::from(directory).join(format!("grid-{scale}.png"));
            h.save_png(h.window(), path, scale);
        }
    }
}

#[test]
fn native_settings_changes_are_saved_and_invalid_reload_preserves_current_configuration() {
    use aizome_hotaru::settings::{Action, Settings};
    let directory = std::env::temp_dir().join(format!(
        "hotaru-native-config-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("hotaru.toml");
    let mut app = Hotaru::preview();
    app.settings_path = Some(path.clone());
    let mut h = Harness::new(app);
    assert_eq!(h.program().font_size, aizome_hotaru::settings::DEFAULT_FONT_SIZE);
    h.send(Msg::SetFontSize(17.0));
    assert!(
        h.settle(Duration::from_secs(3), |_| Settings::load(&path).is_ok_and(|settings| settings.font_size == 17.0))
    );
    // Zooming is for now: ❖0 comes back to the size Settings keeps.
    h.send(Msg::Action(Action::LargerText));
    h.send(Msg::Action(Action::LargerText));
    assert_eq!(h.program().font_size, 19.0);
    h.send(Msg::Action(Action::ActualSize));
    assert_eq!(h.program().font_size, 17.0);
    h.send(Msg::Action(Action::SmallerText));
    h.advance(Duration::from_millis(50));
    assert_eq!(Settings::load(&path).unwrap().font_size, 17.0);
    std::fs::write(&path, "font_size = 999999").unwrap();
    h.send(Msg::Action(Action::ReloadConfig));
    assert!(h.settle(Duration::from_secs(3), |app| app.active_pane().unwrap().status.contains("not loaded")));
    assert_eq!(h.program().font_size, 16.0);
    drop(h);
    std::fs::remove_dir_all(directory).unwrap();
}

fn temporary_config(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "hotaru-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("hotaru.toml");
    (directory, path)
}

#[test]
fn settings_window_opens_once_changes_everything_live_and_saves() {
    use aizome_hotaru::preferences::Page;
    use aizome_hotaru::settings::{CursorStyle, Settings, Theme};
    let (directory, path) = temporary_config("settings-window");
    let mut app = Hotaru::preview();
    app.settings_path = Some(path.clone());
    let mut h = Harness::new(app);
    let terminal = h.window();
    h.command(terminal, ',');
    let settings = h.program().settings_window.expect("❖, opens Settings");
    h.command(terminal, ',');
    assert_eq!(h.windows().len(), 2, "a second ❖, brings the same window forward");
    for page in [Page::General, Page::Text, Page::Appearance, Page::Shortcuts] {
        h.send(Msg::SettingsPage(page));
        h.advance(Duration::from_millis(1));
        assert!(!h.texts(settings).is_empty());
        if let Some(directory) = std::env::var_os("HOTARU_SCREENSHOTS") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            h.save_png(settings, directory.join(format!("settings-{page:?}.png").to_lowercase()), 2.0);
        }
    }
    h.send(Msg::SettingsPage(Page::Appearance));
    assert!(h.click_text(settings, "Day"));
    if let Some(directory) = std::env::var_os("HOTARU_SCREENSHOTS") {
        h.advance(Duration::from_millis(1));
        h.save_png(settings, std::path::PathBuf::from(directory).join("settings-appearance-day.png"), 2.0);
    }
    h.send(Msg::SetCursorStyle(CursorStyle::Underline));
    h.send(Msg::SetCursorBlink(false));
    h.send(Msg::SettingsPage(Page::Text));
    h.send(Msg::SetLigatures(false));
    h.send(Msg::SetFontSize(15.5));
    h.send(Msg::SettingsPage(Page::General));
    h.send(Msg::SetScrollback(100_000));
    assert!(h.settle(Duration::from_secs(3), |_| Settings::load(&path).is_ok_and(|saved| {
        saved.theme == Theme::Day
            && saved.cursor_style == CursorStyle::Underline
            && !saved.cursor_blink
            && !saved.ligatures()
            && saved.font_size == 15.5
            && saved.scrollback == 100_000
    })));
    let pane = h.program().active_pane().unwrap();
    assert_eq!(pane.snapshot.cursor.shape, hotaru_core::CursorShape::Underline);
    assert!(!pane.snapshot.cursor.blinking);
    assert_eq!(h.program().font_size, 15.5);
    // ❖W in Settings closes Settings, not the pane behind it.
    h.focus_window(settings, true);
    h.command(settings, 'w');
    assert_eq!(h.program().settings_window, None);
    assert_eq!(h.program().panes.len(), 1);
    assert_eq!(h.windows(), [terminal]);
    drop(h);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shortcuts_are_recorded_refused_removed_and_restored() {
    use aizome_hotaru::preferences::Page;
    use aizome_hotaru::settings::{Action, Settings, parse_shortcut};
    use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
    let (directory, path) = temporary_config("shortcuts");
    let mut app = Hotaru::preview();
    app.settings_path = Some(path.clone());
    let mut h = Harness::new(app);
    h.send(Msg::Action(Action::Settings));
    let settings = h.program().settings_window.unwrap();
    h.send(Msg::SettingsPage(Page::Shortcuts));
    h.focus_window(settings, true);
    h.send(Msg::RecordShortcut(Action::ClearScreen));
    h.advance(Duration::from_millis(1));
    assert_eq!(h.focused_name(settings).as_deref(), Some(aizome_hotaru::preferences::RECORDER));
    if let Some(directory) = std::env::var_os("HOTARU_SCREENSHOTS") {
        h.save_png(settings, std::path::PathBuf::from(directory).join("settings-shortcuts-editing.png"), 2.0);
    }
    // Typing alone isn't a shortcut, and ⌃C is the programs'.
    h.key(settings, Key::Character("k".into()), Modifiers::NONE);
    assert_eq!(h.program().recording, Some(Action::ClearScreen));
    h.key(settings, Key::Character("c".into()), Modifiers::CONTROL);
    assert_eq!(h.program().recording, Some(Action::ClearScreen));
    // The menus give way while a shortcut is typed: ❖T isn't New Tab now.
    h.key(settings, Key::Character("t".into()), Modifiers::SUPER | Modifiers::ALT);
    assert_eq!(h.program().tabs.len(), 1);
    // The row stays open for taking the old one away, until Done or Escape.
    assert_eq!(h.program().recording, Some(Action::ClearScreen));
    h.advance(Duration::from_millis(1));
    assert_eq!(h.focused_name(settings).as_deref(), Some(aizome_hotaru::preferences::RECORDER));
    h.named(settings, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().recording, None);
    let new = parse_shortcut("super+alt+t").unwrap();
    assert_eq!(h.program().bindings().bound(new), Some(Action::ClearScreen));
    assert!(h.settle(Duration::from_secs(3), |_| {
        Settings::load(&path).is_ok_and(|saved| saved.keybindings.get("super+alt+t") == Some(&Action::ClearScreen))
    }));
    // Taking a default away silences it in the file.
    let close = parse_shortcut("super+w").unwrap();
    h.send(Msg::RemoveShortcut(Action::ClosePane, close));
    assert_eq!(h.program().bindings().bound(close), None);
    assert_eq!(h.program().settings.keybindings.get("super+w"), Some(&Action::Ignore));
    // Escape gives up without a change.
    h.send(Msg::RecordShortcut(Action::NewTab));
    h.advance(Duration::from_millis(1));
    h.named(settings, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().recording, None);
    h.send(Msg::RestoredShortcuts);
    assert!(h.program().settings.keybindings.is_empty());
    assert_eq!(h.program().bindings().bound(close), Some(Action::ClosePane));
    // The defaults are saved too, before the folder they're saved in goes.
    assert!(
        h.settle(Duration::from_secs(3), |_| Settings::load(&path).is_ok_and(|saved| saved.keybindings.is_empty()))
    );
    drop(h);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tabs_are_reached_by_number_and_in_turn_and_panes_zoom() {
    use aizome_hotaru::settings::Action;
    let mut h = Harness::new(Hotaru::preview());
    let window = h.window();
    let first = h.program().active;
    h.send(Msg::NewTab);
    h.send(Msg::NewTab);
    let third = h.program().active;
    h.command(window, '1');
    assert_eq!(h.program().active, first);
    h.command(window, '9');
    assert_eq!(h.program().active, third);
    h.send(Msg::Action(Action::NextTab));
    assert_eq!(h.program().active, first, "Next Tab goes round");
    h.send(Msg::Action(Action::PreviousTab));
    assert_eq!(h.program().active, third);
    // A lone pane has nothing to zoom over.
    h.send(Msg::Action(Action::ZoomPane));
    assert!(!h.program().active_tab().unwrap().zoomed);
    h.send(Msg::Split(Axis::Right));
    let right = h.program().active_pane().unwrap().id;
    h.send(Msg::Action(Action::ZoomPane));
    assert!(h.program().active_tab().unwrap().zoomed);
    h.advance(Duration::from_millis(1));
    assert_eq!(h.focused_name(window), Some(format!("terminal-{right}")));
    assert!(h.texts(window).iter().any(|text| text.contains("Zoomed")));
    // Going to the other pane brings the split back.
    h.send(Msg::MoveFocus(Direction::Previous));
    assert!(!h.program().active_tab().unwrap().zoomed);
    h.advance(Duration::from_millis(1));
    assert_ne!(h.program().active_pane().unwrap().id, right);
}

#[test]
fn splits_have_independent_snapshots_and_close_only_the_focused_pane() {
    let mut h = Harness::new(Hotaru::preview());
    let tab = h.program().active;
    let first = h.program().active_pane().unwrap().id;
    h.send(Msg::Split(Axis::Right));
    let second = h.program().active_pane().unwrap().id;
    h.send(Msg::Split(Axis::Down));
    let third = h.program().active_pane().unwrap().id;
    assert_eq!(h.program().tabs.len(), 1);
    assert_eq!(h.program().panes.len(), 3);
    assert_eq!(h.program().active, tab);
    let mut terminal = Terminal::new(30, 10, 0);
    terminal.feed(b"only the second pane");
    h.send(Msg::Frame(second, Arc::new(terminal.snapshot())));
    h.send(Msg::FocusPane(second));
    assert!(h.program().active_pane().unwrap().snapshot.plain_text().contains("only the second pane"));
    h.send(Msg::ClosePane);
    h.advance(Duration::from_millis(1));
    assert_eq!(h.program().active_pane().unwrap().id, first);
    assert_eq!(h.focused_name(h.window()), Some(format!("terminal-{first}")));
    assert_eq!(h.program().panes.len(), 2);
    h.send(Msg::MoveFocus(Direction::Next));
    assert_eq!(h.program().active_pane().unwrap().id, third);
    h.send(Msg::NewTab);
    h.send(Msg::SelectTab(tab));
    h.send(Msg::CloseTab);
    h.advance(Duration::from_millis(1));
    assert_eq!(h.program().tabs.len(), 1);
    assert_eq!(h.program().panes.len(), 1);
    assert_eq!(h.focused_name(h.window()), Some(format!("terminal-{}", h.program().active_pane().unwrap().id)));
}

#[test]
fn split_divider_grab_band_resizes_before_terminal_selection() {
    use aizome_kit::sidestep_ui::{Cursor, PointerButton};
    use aizome_kit::{Point, Vec2};

    let mut failures = Vec::new();
    for axis in [Axis::Right, Axis::Down] {
        for offset in [-3.0, -0.51, 0.0, 3.0] {
            let mut h = Harness::new(Hotaru::preview());
            let w = h.window();
            let first = h.program().active_pane().unwrap().id;
            h.send(Msg::Split(axis));
            h.advance(Duration::ZERO);
            let rect = h.find_id(w, &format!("terminal-{first}")).unwrap();
            let (at, delta, cursor) = match axis {
                Axis::Right => {
                    (Point::new(rect.x1 + 0.5 + offset, rect.center().y), Vec2::new(70.0, 0.0), Cursor::ColResize)
                }
                Axis::Down => {
                    (Point::new(rect.center().x, rect.y1 + 0.5 + offset), Vec2::new(0.0, 70.0), Cursor::RowResize)
                }
            };
            h.move_to(w, at);
            let resize_cursor = h.platform.cursors.values().any(|c| *c == cursor);
            h.press(w, at, PointerButton::Left, 1);
            h.move_to(w, at + delta);
            h.release(w, at + delta, PointerButton::Left);
            let after = h.find_id(w, &format!("terminal-{first}")).unwrap();
            let resized = match axis {
                Axis::Right => after.width() - rect.width(),
                Axis::Down => after.height() - rect.height(),
            };
            let selected = h.program().panes.iter().any(|p| p.selection.is_some());
            eprintln!("{axis:?} offset={offset}: cursor={resize_cursor} resized={resized} selected={selected}");
            if !resize_cursor || (resized - 70.0).abs() > 1.0 || selected {
                failures.push((axis, offset, resize_cursor, resized, selected));
            }
        }
    }
    assert!(failures.is_empty(), "divider gestures reached terminals: {failures:?}");
}

#[test]
fn dragging_outside_split_grab_band_still_selects_terminal_text() {
    use aizome_kit::sidestep_ui::{Cursor, PointerButton};
    use aizome_kit::{Point, Vec2};

    for axis in [Axis::Right, Axis::Down] {
        for side in [-1.0, 1.0] {
            let mut h = Harness::new(Hotaru::preview());
            let w = h.window();
            let first = h.program().active_pane().unwrap().id;
            h.send(Msg::Split(axis));
            let rect = h.find_id(w, &format!("terminal-{first}")).unwrap();
            let at = match axis {
                Axis::Right => Point::new(rect.x1 + 0.5 + side * 6.0, rect.center().y),
                Axis::Down => Point::new(rect.center().x, rect.y1 + 0.5 + side * 6.0),
            };
            h.move_to(w, at);
            assert!(h.platform.cursors.values().any(|c| *c == Cursor::Text));
            h.press(w, at, PointerButton::Left, 1);
            h.move_to(w, at + Vec2::new(30.0, 30.0));
            h.release(w, at + Vec2::new(30.0, 30.0), PointerButton::Left);
            assert_eq!(h.find_id(w, &format!("terminal-{first}")), Some(rect));
            assert!(h.program().panes.iter().any(|p| p.selection.is_some()));
        }
    }
}

#[test]
fn nested_split_dividers_keep_their_local_grab_areas() {
    use aizome_kit::sidestep_ui::PointerButton;
    use aizome_kit::{Point, Vec2};

    let mut h = Harness::new(Hotaru::preview());
    let w = h.window();
    let first = h.program().active_pane().unwrap().id;
    h.send(Msg::Split(Axis::Right));
    let second = h.program().active_pane().unwrap().id;
    h.send(Msg::Split(Axis::Down));
    let rect = h.find_id(w, &format!("terminal-{second}")).unwrap();
    let at = Point::new(rect.center().x, rect.y1 - 2.0);
    h.press(w, at, PointerButton::Left, 1);
    h.move_to(w, at + Vec2::new(0.0, 40.0));
    h.release(w, at + Vec2::new(0.0, 40.0), PointerButton::Left);
    assert_eq!(h.find_id(w, &format!("terminal-{second}")).unwrap().height(), rect.height() + 40.0);
    assert!(h.program().panes.iter().all(|p| p.selection.is_none()));
    // Collapsing the inner split must leave the outer divider draggable.
    h.send(Msg::FocusPane(second));
    h.send(Msg::ClosePane);
    let rect = h.find_id(w, &format!("terminal-{first}")).unwrap();
    let at = Point::new(rect.x1 + 3.0, rect.center().y);
    h.press(w, at, PointerButton::Left, 1);
    h.move_to(w, at + Vec2::new(50.0, 0.0));
    h.release(w, at + Vec2::new(50.0, 0.0), PointerButton::Left);
    assert_eq!(h.find_id(w, &format!("terminal-{first}")).unwrap().width(), rect.width() + 50.0);
    assert!(h.program().panes.iter().all(|p| p.selection.is_none()));
}

#[test]
fn tabs_keep_independent_terminal_snapshots() {
    let mut h = Harness::new(Hotaru::preview());
    let first = h.program().active;
    h.send(Msg::NewTab);
    let second = h.program().active;
    assert_ne!(first, second);
    let mut terminal = Terminal::new(80, 24, 100);
    terminal.feed(b"independent tab");
    h.send(Msg::Frame(second, Arc::new(terminal.snapshot())));
    h.send(Msg::SelectTab(first));
    assert!(!h.program().active_pane().unwrap().snapshot.plain_text().contains("independent tab"));
    h.send(Msg::CloseTab);
    assert_eq!(h.program().tabs.len(), 1);
    assert_eq!(h.program().active, second);
    assert!(h.program().active_pane().unwrap().snapshot.plain_text().contains("independent tab"));
}

#[test]
fn night_and_day_views_render_with_native_chrome() {
    let mut h = Harness::new(Hotaru::preview());
    let window = h.window();
    h.set_appearance(Appearance::default());
    h.advance(Duration::from_millis(1));
    let night = h.program().active_pane().unwrap().snapshot.clone();
    settle_frame(&mut h);
    let (width, height, pixels) = h.render_rgba(window, 1.0);
    assert!(width >= 320 && height >= 180);
    assert_eq!(pixels.len(), width as usize * height as usize * 4);
    if let Some(directory) = std::env::var_os("HOTARU_SCREENSHOTS") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        h.save_png(window, directory.join("hotaru-night.png"), 1.0);
        h.set_appearance(Appearance { theme: ThemeChoice::Day, ..Appearance::default() });
        h.advance(Duration::from_millis(1));
        h.save_png(window, directory.join("hotaru-day.png"), 1.0);
    }
    h.set_appearance(Appearance { theme: ThemeChoice::Day, ..Appearance::default() });
    h.advance(Duration::from_millis(1));
    let day = &h.program().active_pane().unwrap().snapshot;
    assert_ne!(day.palette[1], night.palette[1]);
    assert_ne!(day.default_foreground, night.default_foreground);
    assert_ne!(day.default_background, night.default_background);
    assert_eq!(h.render_rgba(window, 1.0).2.len(), pixels.len());
    h.send(Msg::Find);
    assert!(h.program().find);
    h.send(Msg::CloseFind);
    assert!(!h.program().find);
}

#[test]
fn explicit_terminal_background_is_rendered_even_when_it_matches_the_builtin_default() {
    let mut h = Harness::new(Hotaru::preview());
    h.advance(Duration::from_millis(1));
    let id = h.program().active;
    let terminal = Terminal::new(80, 24, 0);
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    settle_frame(&mut h);
    let (width, height, pixels) = h.render_rgba(h.window(), 1.0);
    let center = (((height / 2) * width + width / 2) * 4) as usize;
    assert_eq!(&pixels[center..center + 4], &[0, 0, 0, 255]);
}

#[test]
fn semantic_underlines_use_their_own_color_and_overline_is_visible() {
    let mut h = Harness::new(Hotaru::preview());
    h.advance(Duration::from_millis(1));
    let id = h.program().active_pane().unwrap().id;
    let mut terminal = Terminal::new(80, 24, 0);
    let colors = [[200, 0, 0], [0, 200, 0], [0, 0, 200], [200, 200, 0], [200, 0, 200]];
    for (index, [r, g, b]) in colors.iter().enumerate() {
        terminal.feed(format!("\x1b[4:{};58;2;{r};{g};{b}m         \x1b[0m\r\n", index + 1).as_bytes());
    }
    // Spaces make these pixels attributable to the overline alone.
    terminal.feed(b"\x1b[53;38;2;0;200;200m         \x1b[0m");
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    settle_frame(&mut h);
    let (_, _, pixels) = h.render_rgba(h.window(), 1.0);
    for color in colors.into_iter().chain([[0, 200, 200]]) {
        // The one-pixel curly stroke is antialiased against the black background.
        assert!(
            pixels.chunks_exact(4).any(|pixel| {
                let coverage = f64::from(*pixel[..3].iter().max().unwrap()) / 200.0;
                coverage > 0.1
                    && pixel[..3]
                        .iter()
                        .zip(color)
                        .all(|(&actual, expected)| (f64::from(actual) - f64::from(expected) * coverage).abs() < 3.0)
            }),
            "missing decoration color {color:?}"
        );
    }
}

#[test]
fn pty_worker_publishes_final_output_before_exit() {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), r"printf '\033[32mPTY ready\033[0m\n'".into()];
    let mut h = Harness::new(keeping(command));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().is_some_and(|t| t.ended)));
    assert!(h.program().active_pane().unwrap().snapshot.plain_text().contains("PTY ready"));
    h.send(Msg::CopyAll);
    let until = Instant::now() + Duration::from_secs(2);
    while h.platform.clipboard.is_none() && Instant::now() < until {
        h.send(Msg::FontSize(0));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(h.platform.clipboard.as_ref().unwrap().contains("PTY ready"));
}

#[test]
fn native_sessions_choose_unicode_grapheme_width_before_output() {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), "printf '👍🏽👩\u{200d}💻🇯🇵M'".into()];
    let mut h = Harness::new(keeping(command));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().ended));
    let snapshot = &h.program().active_pane().unwrap().snapshot;
    assert!(snapshot.modes.grapheme_cluster);
    let row = &snapshot.lines[0];
    assert_eq!(row.grapheme(0), Some("👍🏽"));
    assert_eq!(row.grapheme(2), Some("👩\u{200d}💻"));
    assert_eq!(row.grapheme(4), Some("🇯🇵"));
    assert_eq!(row.cells()[6].character(), 'M');
}

#[test]
fn pty_worker_encodes_keyboard_and_mouse_using_negotiated_modes() {
    use hotaru_core::input as vt;
    let mut command = PtyCommand::new(std::env::current_exe().unwrap());
    command.args = ["--ignored", "--exact", "input_protocol_child", "--nocapture", "--test-threads=1"]
        .into_iter()
        .map(Into::into)
        .collect();
    command.env.push(("HOTARU_INPUT_CHILD".into(), "1".into()));
    let mut h = Harness::new(keeping(command));
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane().is_some_and(|pane| pane.snapshot.modes.kitty_flags.0 == 31)
    }));
    let id = h.program().active_pane().unwrap().id;
    let mut key = vt::KeyEvent {
        key: vt::Key::Character('a'),
        modifiers: vt::Modifiers::CONTROL,
        text: "a".into(),
        unshifted: Some('a'),
        ..Default::default()
    };
    h.send(Msg::Key(id, Arc::new(key.clone())));
    key.action = vt::KeyAction::Release;
    h.send(Msg::Key(id, Arc::new(key)));
    h.send(Msg::Mouse(
        id,
        aizome_hotaru::input::MouseInput {
            event: vt::MouseEvent {
                action: vt::MouseAction::Press,
                button: Some(vt::MouseButton::Left),
                x: 12.0,
                y: 20.0,
                ..Default::default()
            },
            width: 800.0,
            height: 480.0,
            cell_width: 8.0,
            cell_height: 16.0,
            padding: 0.0,
            scale: 1.0,
            any_button: true,
        },
    ));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().is_some_and(|pane| pane.ended)));
    let text = h.program().active_pane().unwrap().snapshot.plain_text();
    assert!(text.contains("NATIVE-INPUT-OK"), "{text}");
}

#[test]
#[ignore]
fn input_protocol_child() {
    if std::env::var_os("HOTARU_INPUT_CHILD").is_none() {
        return;
    }
    use std::io::{Read, Write};
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: this isolated fixture owns fd0's PTY and initializes termios first.
    assert_eq!(unsafe { libc::tcgetattr(0, attributes.as_mut_ptr()) }, 0);
    let mut attributes = unsafe { attributes.assume_init() };
    unsafe {
        libc::cfmakeraw(&mut attributes);
    }
    assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &attributes) }, 0);
    std::io::stdout().write_all(b"\x1b[>31u\x1b[?1000h\x1b[?1006hINPUT-READY\r\n").unwrap();
    let expected = b"\x1b[97;5u\x1b[97;5:3u\x1b[<0;2;2M";
    let mut actual = vec![0; expected.len()];
    std::io::stdin().read_exact(&mut actual).unwrap();
    assert_eq!(actual, expected);
    println!("NATIVE-INPUT-OK");
}

#[test]
fn reply_flood_resumes_without_losing_responses() {
    let mut command = PtyCommand::new(std::env::current_exe().unwrap());
    command.args = ["--ignored", "--exact", "reply_flood_child", "--nocapture", "--test-threads=1"]
        .into_iter()
        .map(Into::into)
        .collect();
    command.env.push(("HOTARU_REPLY_CHILD".into(), "1".into()));
    let mut h = Harness::new(keeping(command));
    assert!(h.settle(Duration::from_secs(10), |app| app.active_pane().is_some_and(|t| t.ended)));
    let text = h.program().active_pane().unwrap().snapshot.plain_text();
    assert!(text.contains("REPLY-FLOOD-OK"), "{text}");
}

/// Executed only as the child of the real PTY backpressure regression above.
#[test]
#[ignore]
fn reply_flood_child() {
    if std::env::var_os("HOTARU_REPLY_CHILD").is_none() {
        return;
    }
    use std::io::{Read, Write};
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: tcgetattr initializes the valid termios output on success. fd0 is
    // the fixture's PTY; this child is an independent test process.
    assert_eq!(unsafe { libc::tcgetattr(0, attributes.as_mut_ptr()) }, 0);
    let mut attributes = unsafe { attributes.assume_init() };
    unsafe {
        libc::cfmakeraw(&mut attributes);
    }
    assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &attributes) }, 0);
    let writer = std::thread::spawn(|| {
        std::io::stdout().write_all(&b"\x1b[5n".repeat(50_000)).unwrap();
    });
    // Deliberately stop draining replies until the reply reservation is full.
    std::thread::sleep(Duration::from_millis(200));
    let mut replies = vec![0u8; 4 * 50_000];
    std::io::stdin().read_exact(&mut replies).unwrap();
    writer.join().unwrap();
    assert_eq!(replies, b"\x1b[0n".repeat(50_000));
    println!("REPLY-FLOOD-OK");
}

#[test]
fn large_copy_finishes_off_thread_and_stale_reply_is_ignored() {
    let mut h = Harness::new(Hotaru::preview());
    let id = h.program().active;
    let mut terminal = Terminal::new(4096, 320, 0);
    let bytes = vec![b'x'; 4096 * 300];
    terminal.feed(&bytes);
    h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
    h.send(Msg::CopyScreen);
    h.send(Msg::FontSize(1));
    assert_eq!(h.program().font_size, aizome_hotaru::settings::DEFAULT_FONT_SIZE + 1.0);
    let until = Instant::now() + Duration::from_secs(3);
    while h.platform.clipboard.is_none() && Instant::now() < until {
        h.send(Msg::FontSize(0));
        std::thread::sleep(Duration::from_millis(2));
    }
    let text = h.platform.clipboard.as_ref().expect("worker published clipboard");
    assert!(text.len() > 1024 * 1024);
    assert!(h.platform.clipboard_data.as_ref().unwrap().data("text/html").unwrap().len() > 1024 * 1024);
    let copied = text.clone();
    h.send(Msg::CopyReady(0, id, aizome_kit::PreparedClipboardText::new("stale".into()).into()));
    assert_eq!(h.platform.clipboard.as_deref(), Some(copied.as_str()));
}

#[test]
fn newer_copies_win_and_closed_panes_cannot_publish_results() {
    let mut h = Harness::new(Hotaru::preview());
    let first = h.program().active_pane().unwrap().id;
    let mut large = Terminal::new(4096, 300, 0);
    large.feed(&vec![b'x'; 4096 * 290]);
    h.send(Msg::Frame(first, Arc::new(large.snapshot())));
    for _ in 0..16 {
        h.send(Msg::CopyScreen);
    }
    let mut latest = Terminal::new(80, 24, 0);
    latest.feed(b"newest copy survives");
    h.send(Msg::Frame(first, Arc::new(latest.snapshot())));
    h.send(Msg::CopyScreen);
    let until = Instant::now() + Duration::from_secs(3);
    while h.platform.clipboard.as_deref().is_none_or(|text| !text.contains("newest copy survives"))
        && Instant::now() < until
    {
        h.advance(Duration::from_millis(1));
        h.settle(Duration::from_millis(1), |_| false);
    }
    assert!(h.platform.clipboard.as_deref().unwrap().contains("newest copy survives"));
    assert!(!h.platform.clipboard.as_deref().unwrap().contains("xxx"));
    let copied = h.platform.clipboard.clone();
    h.send(Msg::NewTab);
    h.send(Msg::SelectTab(first));
    h.send(Msg::CopyScreen); // token18
    h.send(Msg::CloseTab); // invalidates token18 and removes its pane
    h.send(Msg::CopyReady(18, first, aizome_kit::PreparedClipboardText::new("closed pane".into()).into()));
    for _ in 0..20 {
        h.advance(Duration::from_millis(1));
        h.settle(Duration::from_millis(1), |_| false);
    }
    assert_eq!(h.platform.clipboard, copied);
}

#[test]
fn html_only_copy_has_no_stale_plain_text_fallback() {
    use aizome_hotaru::settings::CopyFormat;
    let mut h = Harness::new(Hotaru::preview());
    h.platform.clipboard = Some("old foreign text".into());
    h.send(Msg::SetCopyFormat(CopyFormat::Html));
    h.send(Msg::CopyScreen);
    let until = Instant::now() + Duration::from_secs(3);
    while h.platform.clipboard_data.is_none() && Instant::now() < until {
        h.advance(Duration::from_millis(1));
        // Advancing timers alone does not pump worker completions once the
        // initial layout ticks are exhausted.
        h.settle(Duration::from_millis(1), |_| false);
    }
    let data = h.platform.clipboard_data.as_ref().unwrap();
    assert!(data.data("text/html").is_some());
    assert!(data.data("text/plain").is_none());
    assert!(h.platform.clipboard.is_none());
}

/// Pump workers until `done` holds of the harness, for up to five seconds.
fn pump(h: &mut Harness<Hotaru>, done: impl Fn(&Harness<Hotaru>) -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    while !done(h) {
        if Instant::now() > until {
            return false;
        }
        h.advance(Duration::from_millis(5));
        h.settle(Duration::from_millis(5), |_| false);
    }
    true
}

fn shell(script: &str) -> PtyCommand {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), script.into()];
    command
}

#[test]
fn an_ended_shell_closes_its_pane_then_its_tab_then_the_window() {
    use aizome_hotaru::settings::Action;
    let mut h = Harness::new(Hotaru::new(shell("read line")));
    let first = h.program().active_pane().unwrap().id;
    h.send(Msg::Split(Axis::Right));
    let second = h.program().active_pane().unwrap().id;
    h.send(Msg::Action(Action::NewTab));
    let third = h.program().active_pane().unwrap().id;
    // A pane in the tab behind ends: the tab in front keeps the keyboard.
    h.send(Msg::Input(first, b"\n".to_vec()));
    assert!(h.settle(Duration::from_secs(5), |app| app.panes.len() == 2));
    assert_eq!(h.program().active_pane().unwrap().id, third);
    assert_eq!(h.program().tabs[0].tree.leaves(), [second]);
    // The tab in front loses its last pane: the other comes forward.
    h.send(Msg::Input(third, b"\n".to_vec()));
    assert!(h.settle(Duration::from_secs(5), |app| app.tabs.len() == 1));
    assert_eq!(h.program().active_pane().unwrap().id, second);
    // The last pane ends, and the window goes with it.
    h.send(Msg::Input(second, b"\n".to_vec()));
    assert!(h.settle(Duration::from_secs(5), |app| app.tabs.is_empty()));
    assert!(h.windows().is_empty());
}

#[test]
fn a_program_failing_as_it_starts_keeps_its_pane_until_a_key() {
    // The PTY worker calls a non-zero exit within 250 ms abnormal. A loaded
    // machine can't promise a real one that soon, so this is its report.
    let mut h = Harness::new(Hotaru::preview());
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::Exited(id, "exit status: 3".into(), true));
    let pane = h.program().active_pane().unwrap();
    assert!(pane.ended && pane.status.contains("failed as it started"), "{}", pane.status);
    // Modifiers alone leave it be.
    let shift = hotaru_core::input::KeyEvent::new(hotaru_core::input::Key::Modifier(
        hotaru_core::input::ModifierKey::ShiftLeft,
    ));
    h.send(Msg::Key(id, Arc::new(shift)));
    assert_eq!(h.program().panes.len(), 1);
    h.send(Msg::Key(id, Arc::new(hotaru_core::input::KeyEvent::new(hotaru_core::input::Key::Character('q')))));
    assert!(h.program().tabs.is_empty());
    assert!(h.windows().is_empty());
}

#[test]
fn programs_write_the_clipboard_and_primary_selection_and_asking_waits_for_an_answer() {
    use aizome_hotaru::settings::Access;
    let script = "printf '\\033]52;c;aGVsbG8=\\007\\033]52;p;d29ybGQ\\007'; read line";
    let mut h = Harness::new(Hotaru::new(shell(script)));
    assert!(pump(&mut h, |h| h.platform.clipboard.is_some() && h.platform.primary.is_some()));
    assert_eq!(h.platform.clipboard.as_deref(), Some("hello"));
    assert_eq!(h.platform.primary.as_deref(), Some("world"));
    assert!(!h.program().asking_about_clipboard());

    let mut app = Hotaru::new(shell("printf '\\033]52;c;cmVwbGFjZWQ=\\007'; read line"));
    app.settings.clipboard_write = Access::Ask;
    let mut h = Harness::new(app);
    h.platform.clipboard = Some("mine".into());
    assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
    let win = h.window();
    let texts = h.texts(win).join("\n");
    assert!(texts.contains("replace the clipboard?") && texts.contains("“replaced”"), "{texts}");
    assert_eq!(h.platform.clipboard.as_deref(), Some("mine"), "nothing changes before the answer");
    assert!(h.click_text(win, "Replace"));
    assert!(!h.program().asking_about_clipboard());
    assert_eq!(h.platform.clipboard.as_deref(), Some("replaced"));

    let mut app = Hotaru::new(shell("printf '\\033]52;c;bm8=\\007'; read line"));
    app.settings.clipboard_write = Access::Deny;
    let mut h = Harness::new(app);
    h.platform.clipboard = Some("mine".into());
    for _ in 0..40 {
        h.advance(Duration::from_millis(5));
        h.settle(Duration::from_millis(5), |_| false);
    }
    assert!(!h.program().asking_about_clipboard());
    assert_eq!(h.platform.clipboard.as_deref(), Some("mine"));
}

fn osc52_reader(selection: &str) -> Hotaru {
    let mut command = PtyCommand::new(std::env::current_exe().unwrap());
    command.args = ["--ignored", "--exact", "osc52_read_child", "--nocapture", "--test-threads=1"]
        .into_iter()
        .map(Into::into)
        .collect();
    command.env.push(("HOTARU_OSC52_CHILD".into(), selection.into()));
    keeping(command)
}

/// Tell the reader to ask, once the clipboard is set up.
fn go(h: &mut Harness<Hotaru>) {
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::Input(id, b"g".to_vec()));
}

fn reply_shown(app: &Hotaru) -> Option<String> {
    let text = app.active_pane()?.snapshot.plain_text();
    let line = text.lines().find(|line| line.starts_with("REPLY "))?;
    Some(line["REPLY ".len()..].to_owned())
}

#[test]
fn reads_are_asked_about_showing_what_they_would_get_and_answered_exactly() {
    // Asked, by default, and allowed: the program gets the clipboard's bytes.
    let mut h = Harness::new(osc52_reader("c"));
    h.platform.clipboard = Some("copied".into());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
    let win = h.window();
    let texts = h.texts(win).join("\n");
    assert!(
        texts.contains("read the clipboard?") && texts.contains("6 bytes") && texts.contains("“copied”"),
        "{texts}"
    );
    // Return doesn't answer a question that may come while you type.
    h.named(win, aizome_kit::sidestep_ui::NamedKey::Enter, aizome_kit::sidestep_ui::Modifiers::NONE);
    assert!(h.program().asking_about_clipboard());
    assert!(h.click_text(win, "Allow"));
    assert!(h.settle(Duration::from_secs(5), |app| reply_shown(app).is_some()));
    assert_eq!(reply_shown(h.program()).unwrap(), r"\x1b]52;c;Y29waWVk\x1b\\");

    // The answer is what the clipboard held when the program asked, even if
    // it changes while the question is up.
    let mut h = Harness::new(osc52_reader("c"));
    h.platform.clipboard = Some("asked".into());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
    h.platform.clipboard = Some("changed since".into());
    let win = h.window();
    assert!(h.click_text(win, "Allow"));
    assert!(h.settle(Duration::from_secs(5), |app| reply_shown(app).is_some()));
    assert_eq!(reply_shown(h.program()).unwrap(), r"\x1b]52;c;YXNrZWQ=\x1b\\");

    // Nothing to read: answered empty, without a question about nothing.
    let mut h = Harness::new(osc52_reader("c"));
    h.platform.clipboard = None;
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| reply_shown(app).is_some()));
    assert_eq!(reply_shown(h.program()).unwrap(), r"\x1b]52;c;\x1b\\");

    // Refused: an empty answer, so the program doesn't wait forever.
    let mut h = Harness::new(osc52_reader("c"));
    h.platform.clipboard = Some("secret".into());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
    let win = h.window();
    assert!(h.click_text(win, "Don’t Allow"));
    assert!(h.settle(Duration::from_secs(5), |app| reply_shown(app).is_some()));
    assert_eq!(reply_shown(h.program()).unwrap(), r"\x1b]52;c;\x1b\\");

    // Allowed outright, from the primary selection, answering under `p`.
    let mut app = osc52_reader("p");
    app.settings.clipboard_read = aizome_hotaru::settings::Access::Allow;
    let mut h = Harness::new(app);
    h.platform.primary = Some("picked".into());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| reply_shown(app).is_some()));
    assert!(!h.program().asking_about_clipboard());
    assert_eq!(reply_shown(h.program()).unwrap(), r"\x1b]52;p;cGlja2Vk\x1b\\");
}

#[test]
#[ignore]
fn osc52_read_child() {
    let Some(selection) = std::env::var_os("HOTARU_OSC52_CHILD") else { return };
    use std::io::{Read, Write};
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: this isolated subprocess owns its PTY; termios is initialized
    // before access and each libc call receives a live, correctly sized value.
    unsafe {
        assert_eq!(libc::tcgetattr(0, attributes.as_mut_ptr()), 0);
        let mut attributes = attributes.assume_init();
        libc::cfmakeraw(&mut attributes);
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &attributes), 0);
    }
    let mut out = std::io::stdout();
    // Ask once the test has set up the clipboard and says so.
    std::io::stdin().read_exact(&mut [0]).unwrap();
    write!(out, "\x1b]52;{};?\x07", selection.to_str().unwrap()).unwrap();
    out.flush().unwrap();
    let mut reply = Vec::new();
    while !reply.ends_with(b"\x1b\\") && reply.len() < 1 << 20 {
        let mut byte = [0];
        std::io::stdin().read_exact(&mut byte).unwrap();
        reply.push(byte[0]);
    }
    write!(out, "\r\nREPLY {}\r\n", reply.escape_ascii()).unwrap();
    out.flush().unwrap();
}

/// A program speaking Kitty's clipboard protocol: each request goes once
/// the test says (`go`), and its answer is shown alone as `REPLY n …`.
fn kitty_child(requests: &[&str]) -> Hotaru {
    let mut command = PtyCommand::new(std::env::current_exe().unwrap());
    command.args = ["--ignored", "--exact", "kitty_clipboard_child", "--nocapture", "--test-threads=1"]
        .into_iter()
        .map(Into::into)
        .collect();
    command.env.push(("HOTARU_KITTY_CHILD".into(), requests.join("\n").into()));
    keeping(command)
}

/// The program's `n`th answer (from 1), escaped.
fn kitty_reply(app: &Hotaru, n: usize) -> Option<String> {
    let text = app.active_pane()?.snapshot.plain_text();
    let prefix = format!("REPLY {n} ");
    let line = text.lines().find(|line| line.starts_with(&prefix))?;
    Some(line[prefix.len()..].to_owned())
}

fn kitty_answered(h: &mut Harness<Hotaru>, n: usize) -> String {
    let found = h.settle(Duration::from_secs(5), |app| kitty_reply(app, n).is_some());
    let pane = h.program().active_pane().unwrap();
    assert!(found, "no answer {n}: status {:?}, screen {:?}", pane.status, pane.snapshot.plain_text());
    kitty_reply(h.program(), n).unwrap()
}

#[test]
fn kitty_writes_offer_every_type_and_asking_can_be_remembered_for_the_session() {
    use aizome_hotaru::settings::Access;
    // Allowed by default: each type as sent, answered DONE.
    let write = concat!(
        "\x1b]5522;type=write:id=w\x1b\\",
        "\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGk=\x1b\\",
        "\x1b]5522;type=wdata:mime=dGV4dC9odG1s;PGI+aGk8L2I+\x1b\\",
        "\x1b]5522;type=wdata\x1b\\",
    );
    let mut h = Harness::new(kitty_child(&[write]));
    go(&mut h);
    assert_eq!(kitty_answered(&mut h, 1), r"\x1b]5522;type=write:status=DONE:id=w\x1b\\");
    assert_eq!(h.platform.clipboard.as_deref(), Some("hi"));
    assert_eq!(h.platform.clipboard_data.as_ref().unwrap().data("text/html"), Some(&b"<b>hi</b>"[..]));
    assert!(!h.program().asking_about_clipboard());

    // Asked: the question names the program and what it writes. Allowed
    // for the session, the same password writes again without a question.
    let named = concat!(
        "\x1b]5522;type=write:id=n:pw=c2VjcmV0:name=dmlt\x1b\\",
        "\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGk=\x1b\\",
        "\x1b]5522;type=wdata\x1b\\",
    );
    let mut app = kitty_child(&[named, named]);
    app.settings.clipboard_write = Access::Ask;
    let mut h = Harness::new(app);
    h.platform.clipboard = Some("mine".into());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
    let win = h.window();
    let texts = h.texts(win).join("\n");
    assert!(texts.contains("Let “vim” in “") && texts.contains("replace the clipboard?"), "{texts}");
    assert!(texts.contains("It would hold text/plain: 2 bytes") && texts.contains("“hi”"), "{texts}");
    assert_eq!(h.platform.clipboard.as_deref(), Some("mine"), "nothing changes before the answer");
    assert!(h.click_text(win, "Allow for This Session"));
    assert_eq!(kitty_answered(&mut h, 1), r"\x1b]5522;type=write:status=DONE:id=n\x1b\\");
    assert_eq!(h.platform.clipboard.as_deref(), Some("hi"));
    h.platform.clipboard = Some("mine again".into());
    go(&mut h);
    assert_eq!(kitty_answered(&mut h, 2), r"\x1b]5522;type=write:status=DONE:id=n\x1b\\");
    assert!(!h.program().asking_about_clipboard());
    assert_eq!(h.platform.clipboard.as_deref(), Some("hi"));

    // Refused when asked, and never allowed: EPERM, the clipboard as it was.
    for access in [Access::Ask, Access::Deny] {
        let mut app = kitty_child(&[write]);
        app.settings.clipboard_write = access;
        let mut h = Harness::new(app);
        h.platform.clipboard = Some("mine".into());
        go(&mut h);
        if access == Access::Ask {
            assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
            let win = h.window();
            assert!(!h.texts(win).iter().any(|text| text == "Allow for This Session"), "no password, no session");
            assert!(h.click_text(win, "Don’t Allow"));
        }
        assert_eq!(kitty_answered(&mut h, 1), r"\x1b]5522;type=write:status=EPERM:id=w\x1b\\");
        assert_eq!(h.platform.clipboard.as_deref(), Some("mine"));
    }
}

#[test]
fn kitty_reads_name_their_types_and_neither_listings_nor_session_grants_are_asked_about() {
    use aizome_hotaru::settings::Access;
    let clipboard = || {
        let html: Arc<[u8]> = Arc::from(&b"<b>hi</b>"[..]);
        aizome_kit::PreparedClipboard::new(Some("hi".into()), vec![("text/html".into(), html)]).unwrap()
    };
    // "text/plain text/html ." from "vim", with a password.
    let read = "\x1b]5522;type=read:id=r:pw=c2VjcmV0:name=dmlt;dGV4dC9wbGFpbiB0ZXh0L2h0bWwgLg==\x1b\\";
    let listing = "\x1b]5522;type=read:id=l;Lg==\x1b\\";
    let mut h = Harness::new(kitty_child(&[read, read, listing]));
    h.platform.clipboard = Some("hi".into());
    h.platform.clipboard_data = Some(clipboard());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
    let win = h.window();
    let texts = h.texts(win).join("\n");
    assert!(texts.contains("Let “vim” in “") && texts.contains("read the clipboard?"), "{texts}");
    assert!(texts.contains("It asks for text/plain, text/html: 11 bytes") && texts.contains("“hi”"), "{texts}");
    assert!(h.click_text(win, "Allow for This Session"));
    let types = "dGV4dC9wbGFpbjtjaGFyc2V0PXV0Zi04IFVURjhfU1RSSU5HIHRleHQvcGxhaW4gU1RSSU5HIFRFWFQgdGV4dC9odG1sCg==";
    let whole = format!(
        r"\x1b]5522;type=read:status=OK:id=r\x1b\\\x1b]5522;type=read:status=DATA:id=r:mime=Lg==;{types}\x1b\\\x1b]5522;type=read:status=DATA:id=r:mime=dGV4dC9wbGFpbg==;aGk=\x1b\\\x1b]5522;type=read:status=DATA:id=r:mime=dGV4dC9odG1s;PGI+aGk8L2I+\x1b\\\x1b]5522;type=read:status=DONE:id=r\x1b\\"
    );
    assert_eq!(kitty_answered(&mut h, 1), whole);
    go(&mut h);
    assert_eq!(kitty_answered(&mut h, 2), whole);
    assert!(!h.program().asking_about_clipboard(), "granted for the session");
    go(&mut h);
    assert_eq!(
        kitty_answered(&mut h, 3),
        format!(
            r"\x1b]5522;type=read:status=OK:id=l\x1b\\\x1b]5522;type=read:status=DATA:id=l:mime=Lg==;{types}\x1b\\\x1b]5522;type=read:status=DONE:id=l\x1b\\"
        )
    );
    assert!(!h.program().asking_about_clipboard(), "a listing reads no data");

    // Refused when asked, and never allowed: EPERM. Nothing there to read
    // needs no question.
    for access in [Access::Ask, Access::Deny] {
        let mut app = kitty_child(&[read]);
        app.settings.clipboard_read = access;
        let mut h = Harness::new(app);
        h.platform.clipboard = Some("hi".into());
        h.platform.clipboard_data = Some(clipboard());
        go(&mut h);
        if access == Access::Ask {
            assert!(h.settle(Duration::from_secs(5), |app| app.asking_about_clipboard()));
            let win = h.window();
            assert!(h.click_text(win, "Don’t Allow"));
        }
        assert_eq!(kitty_answered(&mut h, 1), r"\x1b]5522;type=read:status=EPERM:id=r\x1b\\");
    }
    let mut h = Harness::new(kitty_child(&[read]));
    go(&mut h);
    assert_eq!(
        kitty_answered(&mut h, 1),
        r"\x1b]5522;type=read:status=OK:id=r\x1b\\\x1b]5522;type=read:status=DATA:id=r:mime=Lg==\x1b\\\x1b]5522;type=read:status=DONE:id=r\x1b\\"
    );
    assert!(!h.program().asking_about_clipboard());
}

#[test]
fn paste_events_list_the_clipboards_types_and_their_password_reads_once_without_asking() {
    // The program turns paste events on (mode 5522); a paste then sends it
    // the types, never the text, and its read with the password goes
    // without a question though reads are asked about.
    let mut h = Harness::new(kitty_child(&["paste"]));
    h.platform.clipboard = Some("pasted".into());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| kitty_reply(app, 0).is_some()));
    h.send(Msg::Paste);
    let event = kitty_answered(&mut h, 1);
    let types = "dGV4dC9wbGFpbjtjaGFyc2V0PXV0Zi04IFVURjhfU1RSSU5HIHRleHQvcGxhaW4gU1RSSU5HIFRFWFQK";
    let pw = event.strip_prefix(r"\x1b]5522;type=read:status=OK:pw=").unwrap();
    let pw = &pw[..pw.find('\\').unwrap()];
    assert_eq!(pw.len(), 32);
    go(&mut h);
    assert_eq!(
        event,
        format!(
            r"\x1b]5522;type=read:status=OK:pw={pw}\x1b\\\x1b]5522;type=read:status=DATA:mime=Lg==:pw={pw};{types}\x1b\\\x1b]5522;type=read:status=DONE:pw={pw}\x1b\\"
        )
    );
    assert_eq!(
        kitty_answered(&mut h, 2),
        r"\x1b]5522;type=read:status=OK\x1b\\\x1b]5522;type=read:status=DATA:mime=dGV4dC9wbGFpbg==;cGFzdGVk\x1b\\\x1b]5522;type=read:status=DONE\x1b\\"
    );
    assert!(!h.program().asking_about_clipboard());
    assert!(!h.program().active_pane().unwrap().snapshot.plain_text().contains("pasted"));

    // A middle click's paste event is of the primary selection.
    let mut h = Harness::new(kitty_child(&["paste"]));
    h.platform.primary = Some("picked".into());
    go(&mut h);
    assert!(h.settle(Duration::from_secs(5), |app| kitty_reply(app, 0).is_some()));
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::PastePrimary(id));
    assert!(kitty_answered(&mut h, 1).starts_with(r"\x1b]5522;type=read:status=OK:loc=primary:pw="));
    go(&mut h);
    assert_eq!(
        kitty_answered(&mut h, 2),
        r"\x1b]5522;type=read:status=OK:loc=primary\x1b\\\x1b]5522;type=read:status=DATA:mime=dGV4dC9wbGFpbg==;cGlja2Vk\x1b\\\x1b]5522;type=read:status=DONE\x1b\\"
    );
}

#[test]
#[ignore]
fn kitty_clipboard_child() {
    let Some(requests) = std::env::var_os("HOTARU_KITTY_CHILD") else { return };
    use std::io::{Read, Write};
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: this isolated subprocess owns its PTY; termios is initialized
    // before access and each libc call receives a live, correctly sized value.
    unsafe {
        assert_eq!(libc::tcgetattr(0, attributes.as_mut_ptr()), 0);
        let mut attributes = attributes.assume_init();
        libc::cfmakeraw(&mut attributes);
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &attributes), 0);
    }
    let mut out = std::io::stdout();
    // An answer ends with DONE or an error.
    let answer = || {
        let mut reply = Vec::new();
        loop {
            let mut byte = [0];
            std::io::stdin().read_exact(&mut byte).unwrap();
            reply.push(byte[0]);
            if reply.ends_with(b"\x1b\\") || reply.ends_with(b"\x07") {
                let packet = &reply[reply[..reply.len() - 1].iter().rposition(|&b| b == b']').unwrap_or(0)..];
                let text = String::from_utf8_lossy(packet);
                if text.contains("status=DONE") || text.contains("status=E") {
                    return reply;
                }
            }
        }
    };
    let mut show = |n: usize, reply: &[u8]| {
        write!(out, "\x1b[H\x1b[2JREPLY {n} {}\r\n", reply.escape_ascii()).unwrap();
        out.flush().unwrap();
    };
    let go = || std::io::stdin().read_exact(&mut [0]).unwrap();
    let requests = requests.into_string().unwrap();
    if requests == "paste" {
        // Paste events on, then the event and a read with its password.
        go();
        write!(std::io::stdout(), "\x1b[?5522h").unwrap();
        show(0, b"ready");
        let event = answer();
        show(1, &event);
        let text = String::from_utf8(event).unwrap();
        let pw = &text[text.find(":pw=").unwrap() + 4..];
        let pw = &pw[..pw.find('\x1b').unwrap()];
        let loc = if text.contains("loc=primary") { ":loc=primary" } else { "" };
        // As "Paste event", for text/plain, once the test has seen it.
        go();
        write!(std::io::stdout(), "\x1b]5522;type=read{loc}:pw={pw}:name=UGFzdGUgZXZlbnQ=;dGV4dC9wbGFpbg==\x1b\\")
            .unwrap();
        std::io::stdout().flush().unwrap();
        show(2, &answer());
        return;
    }
    for (n, request) in requests.split('\n').enumerate() {
        go();
        write!(std::io::stdout(), "{request}").unwrap();
        std::io::stdout().flush().unwrap();
        show(n + 1, &answer());
    }
}

#[test]
fn selecting_words_and_lines_fills_the_primary_selection_and_a_middle_click_pastes_it() {
    use aizome_kit::Vec2;
    use aizome_kit::sidestep_ui::PointerButton;
    let mut h = Harness::new(Hotaru::new(PtyCommand::new("cat")));
    let id = h.program().active_pane().unwrap().id;
    h.send(Msg::Input(id, b"hello world\n".to_vec()));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().snapshot.plain_text().contains("hello")));
    settle_frame(&mut h);
    let win = h.window();
    let pane = h.program().active_pane().unwrap();
    let metrics = pane.font_metrics().unwrap();
    let origin = h.find_id(win, &format!("terminal-{id}")).unwrap().origin() + Vec2::new(10., 10.);
    let at = |col: f64| origin + Vec2::new(col * metrics.cell_width(), metrics.cell_height() * 0.4);
    // A double click takes the word.
    h.press(win, at(8.5), PointerButton::Left, 2);
    h.release(win, at(8.5), PointerButton::Left);
    assert!(pump(&mut h, |h| h.platform.primary.is_some()));
    assert_eq!(h.platform.primary.as_deref(), Some("world"));
    assert!(h.platform.clipboard.is_none(), "selecting leaves the clipboard alone");
    // A triple click, the line.
    h.press(win, at(2.5), PointerButton::Left, 3);
    h.release(win, at(2.5), PointerButton::Left);
    assert!(pump(&mut h, |h| h.platform.primary.as_deref() == Some("hello world")));
    // A middle click pastes it.
    h.press(win, at(2.5), PointerButton::Middle, 1);
    h.release(win, at(2.5), PointerButton::Middle);
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane().unwrap().snapshot.plain_text().matches("hello world").count() == 3
    }));
}

#[test]
fn a_right_click_opens_the_pane_menu_and_dropped_files_paste_as_quoted_paths() {
    use aizome_kit::sidestep_ui::PointerButton;
    let mut h = Harness::new(Hotaru::new(PtyCommand::new("cat")));
    assert!(h.settle(Duration::from_secs(5), |app| app.active_pane().unwrap().font_metrics().is_some()));
    settle_frame(&mut h);
    let win = h.window();
    let id = h.program().active_pane().unwrap().id;
    let centre = h.find_id(win, &format!("terminal-{id}")).unwrap().center();
    assert!(h.popups(win).is_empty());
    h.press(win, centre, PointerButton::Right, 1);
    h.release(win, centre, PointerButton::Right);
    assert_eq!(h.popups(win).len(), 1);
    h.named(win, aizome_kit::sidestep_ui::NamedKey::Escape, aizome_kit::sidestep_ui::Modifiers::NONE);
    assert!(h.drop(win, centre, vec!["file:///tmp/two%20words.txt".into(), "file:///etc/hosts".into()], None));
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane().unwrap().snapshot.plain_text().contains("'/tmp/two words.txt' /etc/hosts")
    }));
}
