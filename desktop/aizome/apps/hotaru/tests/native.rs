use std::sync::Arc;

use aizome_hotaru::app::{Hotaru, Msg};
use aizome_hotaru::panes::{Axis, Direction};
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use hotaru_core::Terminal;
use hotaru_io::PtyCommand;
use std::time::{Duration, Instant};

fn settle_frame(h: &mut Harness<Hotaru>) {
    assert!(
        h.settle(Duration::from_secs(5), |app| app
            .active_pane()
            .is_some_and(|pane| pane.displayed_snapshot().is_some_and(|shown| Arc::ptr_eq(shown, &pane.snapshot)))),
        "font worker did not present current snapshot"
    );
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
fn find_navigates_owned_history_and_wraps_in_both_directions() {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec![
        "-c".into(),
        "i=0; while [ $i -lt 120 ]; do printf 'Needle %03d\\n' \"$i\"; i=$((i + 1)); done; printf 'history-ready\\n'"
            .into(),
    ];
    let mut h = Harness::new(Hotaru::new(command));
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
    let mut app = Hotaru::new(command);
    app.settings.scrollback = 60;
    let mut h = Harness::new(app);
    assert!(h.settle(Duration::from_secs(5), |app| {
        app.active_pane().unwrap().snapshot.plain_text().contains("ready-initial")
    }));
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
    assert!(h.settle(Duration::from_secs(3), |app| {
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
    let pane = h.program().active_pane().unwrap();
    assert_eq!(pane.snapshot.position.origin + h.program().search_selected().unwrap().start.row as u64, absolute);
    h.send(Msg::Search("Mark".into()));
    for _ in 0..10 {
        h.advance(Duration::from_millis(150));
        h.settle(Duration::from_millis(20), |_| false);
    }
    h.send(Msg::SearchStep(true));
    assert!(h.settle(Duration::from_secs(3), |app| app.search_selected().is_some_and(|found| found.start.row == 0)));
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
    assert!(h.settle(Duration::from_secs(3), |app| app.search_selected().is_some()));
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
    assert!(h.settle(Duration::from_secs(3), |app| app.search_selected().is_some()));
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
    h.send(Msg::FontSize(3));
    assert!(
        h.settle(Duration::from_secs(3), |_| Settings::load(&path).is_ok_and(|settings| settings.font_size == 17.0))
    );
    std::fs::write(&path, "font_size = 999999").unwrap();
    h.send(Msg::Action(Action::ReloadConfig));
    assert!(h.settle(Duration::from_secs(3), |app| app.active_pane().unwrap().status.contains("not loaded")));
    assert_eq!(h.program().font_size, 17.0);
    drop(h);
    std::fs::remove_dir_all(directory).unwrap();
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
    let mut h = Harness::new(Hotaru::new(command));
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
    let mut h = Harness::new(Hotaru::new(command));
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
    let mut h = Harness::new(Hotaru::new(command));
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
    let mut h = Harness::new(Hotaru::new(command));
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
    assert_eq!(h.program().font_size, 15.0);
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
        std::thread::sleep(Duration::from_millis(1));
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
        std::thread::sleep(Duration::from_millis(1));
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
        std::thread::sleep(Duration::from_millis(1));
    }
    let data = h.platform.clipboard_data.as_ref().unwrap();
    assert!(data.data("text/html").is_some());
    assert!(data.data("text/plain").is_none());
    assert!(h.platform.clipboard.is_none());
}
