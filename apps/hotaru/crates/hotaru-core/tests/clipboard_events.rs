use hotaru_core::{Terminal, TerminalEvent};

fn request(selection: u8, data: &[u8]) -> TerminalEvent {
    TerminalEvent::Clipboard {
        selection,
        data: data.to_vec(),
    }
}

#[test]
fn raw_single_byte_kinds_and_payloads_survive_without_granting_access() {
    let input = b"\x1b]52;;?\x07\x1b]52;\xff;a\xffb\x1b\\\x1b]52;p;\x07";
    for size in [1, 7, 65536] {
        let mut terminal = Terminal::new(8, 3, 0);
        for chunk in input.chunks(size) {
            terminal.feed(chunk);
        }
        assert_eq!(
            terminal.take_events(),
            vec![
                request(b'c', b"?"),
                request(0xff, b"a\xffb"),
                request(b'p', b"")
            ]
        );
        assert!(terminal.take_responses().is_empty());
    }
    for input in [
        b"52;cp;?".as_slice(),
        b"52;pc;?",
        b"52;cc;?",
        b"52;\xc3\xa9;?",
        b"52;c",
        b"052;c;?",
        b"0052;c;?",
        b"+52;c;?",
        b" 52;c;?",
        b"52 ;c;?",
    ] {
        let mut terminal = Terminal::new(8, 3, 0);
        terminal.feed(b"\x1b]");
        terminal.feed(input);
        terminal.feed(b"\x07");
        assert!(terminal.take_events().is_empty(), "{input:?}");
    }
}

#[test]
fn terminal_stream_filters_nul_retains_del_and_cancels_can_sub() {
    // Confirmed using the unchanged GTK frontend, independently of the bare
    // OSC parser API (whose next() method does not apply terminal C0 filtering).
    for body in [
        b"52\0;c;YWJj".as_slice(),
        b"52;c\0;YWJj",
        b"52;\0;YWJj",
        b"52;c;Y\0WJj",
    ] {
        let mut terminal = Terminal::new(8, 3, 0);
        terminal.feed(b"\x1b]");
        terminal.feed(body);
        terminal.feed(b"\x07");
        assert_eq!(terminal.take_events(), vec![request(b'c', b"YWJj")]);
    }
    let mut terminal = Terminal::new(8, 3, 0);
    terminal.feed(b"\x1b]52;c;Y\x7fWJj\x07");
    assert_eq!(terminal.take_events(), vec![request(b'c', b"Y\x7fWJj")]);
    for cancel in [0x18, 0x1a] {
        terminal.feed(b"\x1b]52;c;Y");
        terminal.feed(&[cancel]);
        terminal.feed(b"WJj\x07");
        assert!(
            !terminal
                .take_events()
                .iter()
                .any(|event| matches!(event, TerminalEvent::Clipboard { .. }))
        );
    }
}

#[test]
fn non_streaming_event_flood_keeps_bounded_ordered_prefix_and_ui_notifications() {
    let mut terminal = Terminal::new(8, 3, 0);
    for index in 0..100 {
        terminal.feed(format!("\x1b]52;c;{index}\x07").as_bytes());
    }
    terminal.feed(b"\x07\x1b]2;title\x07");
    let events = terminal.take_events();
    assert_eq!(events.len(), 66);
    for (index, actual) in events[..64].iter().enumerate() {
        assert_eq!(actual, &request(b'c', index.to_string().as_bytes()));
    }
    assert_eq!(
        &events[64..],
        &[TerminalEvent::Bell, TerminalEvent::Title("title".into())]
    );
    terminal.feed(b"\x1b]52;c;?\x07");
    assert_eq!(terminal.take_events(), vec![request(b'c', b"?")]);
}

#[test]
fn pending_payload_cap_is_checked_before_copy_and_released_on_drain() {
    let mut terminal = Terminal::new(8, 3, 0);
    let body = vec![b'A'; 5 * 1024 * 1024];
    for _ in 0..2 {
        terminal.feed(b"\x1b]52;c;");
        terminal.feed(&body);
        terminal.feed(b"\x07");
    }
    let events = terminal.take_events();
    assert!(matches!(&events[..], [TerminalEvent::Clipboard { data, .. }] if data == &body));
    terminal.feed(b"\x1b]52;c;?\x07");
    assert_eq!(terminal.take_events(), vec![request(b'c', b"?")]);
}

#[test]
fn notifications_come_from_osc_9_and_777_but_not_conemu_commands() {
    let mut terminal = Terminal::new(8, 3, 0);
    terminal.feed(b"\x1b]9;Build finished\x07");
    assert_eq!(
        terminal.take_events(),
        vec![TerminalEvent::Notification {
            title: String::new(),
            body: "Build finished".into()
        }]
    );
    terminal.feed(b"\x1b]777;notify;Tests;All 47 passed\x1b\\\x1b]777;notify;Title only\x07");
    // The later of two pending notifications replaces the earlier.
    assert_eq!(
        terminal.take_events(),
        vec![TerminalEvent::Notification {
            title: "Title only".into(),
            body: String::new()
        }]
    );
    for conemu in [
        &b"9;4;1;50"[..],
        b"9;1;100",
        b"9;2;message",
        b"9;5",
        b"777;other;x",
    ] {
        terminal.feed(b"\x1b]");
        terminal.feed(conemu);
        terminal.feed(b"\x07");
        assert!(terminal.take_events().is_empty(), "{conemu:?}");
    }
    terminal.feed(b"\x1b]9;10 files left\x07");
    assert_eq!(
        terminal.take_events(),
        vec![TerminalEvent::Notification {
            title: String::new(),
            body: "10 files left".into()
        }]
    );
}
