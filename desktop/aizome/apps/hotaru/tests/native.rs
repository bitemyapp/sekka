use std::sync::Arc;

use aizome_hotaru::app::{Hotaru, Msg};
use aizome_hotaru::panes::{Axis, Direction};
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use hotaru_core::Terminal;
use hotaru_io::PtyCommand;
use std::time::{Duration, Instant};

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
    let copied = text.clone();
    h.send(Msg::CopyReady(0, aizome_kit::PreparedClipboardText::new("stale".into())));
    assert_eq!(h.platform.clipboard.as_deref(), Some(copied.as_str()));
}
