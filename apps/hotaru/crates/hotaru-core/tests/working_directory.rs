use hotaru_core::{Terminal, TerminalEvent};

#[test]
fn osc7_is_owned_coalesced_and_parser_bounded() {
    let mut terminal = Terminal::new(20, 2, 0);
    for byte in
        b"\x1b]7;file://local/first\x07\x1b]2;title\x07\x1b]7;file://local/second%20directory\x1b\\"
    {
        terminal.feed(&[*byte]);
    }
    assert_eq!(
        terminal.take_events(),
        vec![
            TerminalEvent::Title("title".into()),
            TerminalEvent::WorkingDirectory("file://local/second%20directory".into()),
        ]
    );
    terminal.feed(format!("\x1b]7;file://local/{}\x07OK", "x".repeat(16 * 1024)).as_bytes());
    assert!(terminal.take_events().is_empty());
    assert!(terminal.snapshot().plain_text().starts_with("OK"));
}

#[test]
fn osc7_boundary_is_independent_of_title_and_clipboard_limits() {
    for size in [2048, 2049, 16 * 1024, 16 * 1024 + 1] {
        let body = "x".repeat(size);
        let stream = format!("\x1b]7;{body}\x1b\\tail");
        for chunk in [1, 7, stream.len()] {
            let mut terminal = Terminal::new(20, 2, 0);
            for bytes in stream.as_bytes().chunks(chunk) {
                terminal.feed(bytes);
            }
            let expected = if size <= 16 * 1024 {
                vec![TerminalEvent::WorkingDirectory(body.clone())]
            } else {
                Vec::new()
            };
            assert_eq!(
                terminal.take_events(),
                expected,
                "size={size}, chunk={chunk}"
            );
            assert!(terminal.snapshot().plain_text().starts_with("tail"));
        }
    }
    let mut terminal = Terminal::new(20, 2, 0);
    terminal.feed(format!("\x1b]2;{}\x07", "x".repeat(2049)).as_bytes());
    assert!(terminal.take_events().is_empty());
}
