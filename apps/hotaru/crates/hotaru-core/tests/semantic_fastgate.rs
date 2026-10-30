use hotaru_core::Terminal;

#[test]
fn prompt_erasure_remains_enabled_after_soft_reset_alternate_use_and_reflow() {
    for transition in [b"\x1b[!p".as_slice(), b"\x1b[?1049hALT\x1b[?1049l", b""] {
        // OSC B alone can create a continuation marker through an explicit
        // linefeed, without ever receiving OSC A or P.
        let mut terminal = Terminal::new(8, 3, 20);
        terminal.feed(b"\x1b]133;B\x07abc\r\nxyz");
        terminal.feed(transition);
        terminal.resize(16, 3);
        terminal.feed(b"\x1b[2J");
        assert_eq!(terminal.scrollback_len(), 2, "{transition:?}");
        let history = terminal.snapshot_all();
        assert_eq!(history.lines[0].cells()[0].character(), 'a');
        assert_eq!(history.lines[1].cells()[0].character(), 'x');
        assert!(
            terminal
                .snapshot()
                .lines
                .iter()
                .all(|row| row.cells().iter().all(|cell| cell.character() == ' '))
        );
    }
}

#[test]
fn ended_input_keeps_existing_prompt_row_but_ris_clears_it() {
    let mut terminal = Terminal::new(12, 3, 20);
    terminal.feed(b"\x1b]133;P\x07prompt>\x1b]133;I\x07input\r\n\x1b[!p\x1b[2J");
    assert_eq!(terminal.scrollback_len(), 1);
    terminal.feed(b"\x1bcpure output\x1b[2J");
    assert_eq!(terminal.scrollback_len(), 0);
    assert!(
        terminal
            .snapshot()
            .lines
            .iter()
            .all(|row| row.cells().iter().all(|cell| cell.character() == ' '))
    );
}
