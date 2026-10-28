use hotaru_core::{
    ContentSnapshot, Terminal,
    search::{self, Match, Options, Position, Results},
};
use std::sync::atomic::AtomicBool;

fn find(terminal: &Terminal, needle: &[u8]) -> Results {
    search::find(
        terminal.content_snapshot(),
        needle,
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
}

#[test]
fn immutable_history_rebases_after_append_prune_and_compression() {
    let mut terminal = Terminal::new(4, 2, 3);
    terminal.feed(b"0\r\n1\r\n2\r\n3\r\n4");
    let oldest = find(&terminal, b"0");
    let retained = find(&terminal, b"2");
    let active = find(&terminal, b"4");
    let before = terminal.content_position(0);
    assert_eq!(
        (
            before.origin,
            before.first_row,
            before.history_rows,
            before.active_rows
        ),
        (0, 3, 3, 2)
    );
    terminal.feed(b"\r\n5");
    while terminal.compress_history_step() {}
    let now = terminal.content_position(0);
    assert_eq!(before.epoch, now.epoch);
    assert_eq!((now.origin, now.first_row, now.history_rows), (1, 3, 3));
    assert!(
        oldest
            .resolve_match(0, &now, terminal.generation())
            .is_none()
    );
    assert!(
        active
            .resolve_match(0, &now, terminal.generation())
            .is_none()
    );
    assert_eq!(
        retained.resolve_match(0, &now, terminal.generation()),
        Some(Match {
            start: Position { row: 1, col: 0 },
            end: Position { row: 1, col: 1 },
        })
    );
    let viewport = terminal.snapshot_scrolled(2);
    assert_eq!(viewport.position.first_row, 1);
    assert_eq!(
        retained.project(&viewport),
        vec![Match {
            start: Position { row: 0, col: 0 },
            end: Position { row: 0, col: 1 },
        }]
    );
    assert_eq!(
        retained.project_match(0, &viewport),
        retained.project(&viewport).first().copied()
    );
}

#[test]
fn active_matches_expire_on_mutation_and_never_alias_reflow_clear_or_reset() {
    let mut terminal = Terminal::new(8, 3, 100);
    terminal.feed(b"history\r\n1\r\n2\r\nactive");
    let history = find(&terminal, b"history");
    let active = find(&terminal, b"active");
    assert!(
        active
            .resolve_match(0, &terminal.content_position(0), terminal.generation())
            .is_some()
    );
    terminal.feed(b"\rnew");
    assert!(
        active
            .resolve_match(0, &terminal.content_position(0), terminal.generation())
            .is_none()
    );
    assert!(
        history
            .resolve_match(0, &terminal.content_position(0), terminal.generation())
            .is_some()
    );
    let epoch = terminal.content_position(0).epoch;
    terminal.resize(9, 4);
    assert_ne!(epoch, terminal.content_position(0).epoch);
    assert!(
        history
            .resolve_match(0, &terminal.content_position(0), terminal.generation())
            .is_none()
    );
    for command in [b"\x1b[3J".as_slice(), b"\x1bc"] {
        let old = terminal.content_position(0);
        terminal.feed(command);
        let now = terminal.content_position(0);
        assert_ne!(old.epoch, now.epoch);
        assert_eq!(now.origin, 0);
    }
}

#[test]
fn alternate_replacement_does_not_destroy_primary_identity() {
    let mut terminal = Terminal::new(8, 2, 100);
    terminal.feed(b"primary\r\n1\r\n2");
    let primary = terminal.content_position(0);
    let result = find(&terminal, b"primary");
    terminal.feed(b"\x1b[?1049halt");
    let alternate = terminal.content_position(0);
    assert_ne!(primary.epoch, alternate.epoch);
    assert!(
        result
            .resolve_match(0, &alternate, terminal.generation())
            .is_none()
    );
    terminal.feed(b"\x1b[?1049l");
    assert_eq!(primary.epoch, terminal.content_position(0).epoch);
    assert!(
        result
            .resolve_match(0, &terminal.content_position(0), terminal.generation())
            .is_some()
    );
    terminal.feed(b"\x1b[?1049h");
    assert_ne!(alternate.epoch, terminal.content_position(0).epoch);
}

#[test]
fn scrolled_snapshot_capture_preserves_origin_and_multiline_wide_clipping() {
    let mut terminal = Terminal::new(4, 2, 100);
    terminal.feed("a雪bcd雪efghij".as_bytes());
    let all = find(&terminal, "bcd雪efg".as_bytes());
    let viewport = terminal.snapshot_scrolled(1);
    let projected = all.project(&viewport);
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].start, Position { row: 0, col: 0 });
    assert_eq!(projected[0].end, Position { row: 1, col: 3 });
    let visible = ContentSnapshot::from(viewport.clone());
    assert_eq!(visible.position, viewport.position);
    let result = search::find(
        visible,
        "雪".as_bytes(),
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        result.project(&viewport),
        vec![Match {
            start: Position { row: 0, col: 2 },
            end: Position { row: 0, col: 4 },
        }]
    );
    assert_eq!(terminal.snapshot_all().position.first_row, 0);
    assert_eq!(terminal.content_snapshot().position.first_row, 0);
}

#[test]
fn capture_epoch_ownership_prevents_identity_reuse_after_terminal_destruction() {
    let mut old = Terminal::new(4, 2, 10);
    old.feed(b"old");
    let result = find(&old, b"old");
    drop(old);
    for _ in 0..1000 {
        let mut fresh = Terminal::new(4, 2, 10);
        fresh.feed(b"old");
        assert!(
            result
                .resolve_match(0, &fresh.content_position(0), fresh.generation())
                .is_none()
        );
    }
}

#[test]
fn leading_newline_coordinates_expire_after_prefix_pruning() {
    multiline_pruning_regression(b"\r\n \r\n\r\n \r\n \r\nX");
}

#[test]
fn newlines_expire_when_their_preceding_text_anchor_is_pruned() {
    multiline_pruning_regression(b"a\r\n \r\n\r\n \r\n \r\nX");
}

fn multiline_pruning_regression(initial: &[u8]) {
    let mut terminal = Terminal::new(4, 1, 5);
    terminal.feed(initial);
    let results = find(&terminal, b"\n\n");
    assert!(!results.matches.is_empty());
    let before = terminal.content_position(0);
    terminal.feed(b"b\r\n");
    let after = terminal.content_position(0);
    assert_eq!(before.epoch, after.epoch);
    assert_eq!(after.origin, before.origin + 1);
    // Some reported endpoints survive even though their preceding newline
    // context does not. Refresh multiline results rather than guessing a new
    // mapping from the endpoint cells alone.
    for index in 0..results.matches.len() {
        assert!(
            results
                .resolve_match(index, &after, terminal.generation())
                .is_none()
        );
    }
}

#[test]
fn synthetic_final_newline_in_history_still_depends_on_active_continuation() {
    let mut terminal = Terminal::new(4, 1, 10);
    terminal.feed(b"abcd ");
    let result = find(&terminal, b"d\n");
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].end.row, 0);
    assert_eq!(terminal.scrollback_len(), 1);
    assert!(
        result
            .resolve_match(0, &terminal.content_position(0), terminal.generation())
            .is_some()
    );
    terminal.feed(b"X");
    assert!(find(&terminal, b"d\n").matches.is_empty());
    assert!(
        result
            .resolve_match(0, &terminal.content_position(0), terminal.generation())
            .is_none()
    );
    assert!(result.project(&terminal.snapshot_scrolled(1)).is_empty());
}

#[test]
fn deferred_whitespace_matches_depend_on_the_row_that_flushes_them() {
    for (input, needle) in [
        (b"a   X".as_slice(), b"a ".as_slice()),
        (b"a\r\n\r\nX".as_slice(), b"\n\n".as_slice()),
    ] {
        let mut terminal = Terminal::new(4, 1, 10);
        terminal.feed(input);
        let result = find(&terminal, needle);
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].end.row < terminal.scrollback_len());
        terminal.feed(b"\x1b[2K");
        assert!(find(&terminal, needle).matches.is_empty());
        assert!(
            result
                .resolve_match(0, &terminal.content_position(0), terminal.generation())
                .is_none()
        );
    }
}

#[test]
fn generation_sensitive_prefix_tracks_newest_retention_without_hiding_stable_history() {
    for limit in [0, 1, 2, 3, 4, 10] {
        let mut terminal = Terminal::new(4, 1, 10);
        terminal.feed(b"a b\r\nc   X");
        let result = search::find(
            terminal.content_snapshot(),
            b" ",
            Options { max_matches: limit },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.total_matches, 4);
        terminal.feed(b"\x1b[2K");
        assert_eq!(find(&terminal, b" ").total_matches, 1);
        for index in 0..result.matches.len() {
            assert_eq!(
                result
                    .resolve_match(index, &terminal.content_position(0), terminal.generation())
                    .is_some(),
                index >= 3,
                "limit{limit}, index{index}"
            );
        }
    }
}
