use hotaru_core::{
    Terminal,
    search::{self, Error, Match, Options, Position},
};
use serde_json::Value;
use std::sync::atomic::AtomicBool;

fn bytes(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect()
}

#[test]
fn selections_match_real_ghostty_search() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../upstream/search-fixtures.json")).unwrap();
    let mut failures = Vec::new();
    for case in fixtures["cases"].as_array().unwrap() {
        for chunk in [1, 7, 65_536] {
            let mut terminal = Terminal::new(
                case["cols"].as_u64().unwrap() as usize,
                case["rows"].as_u64().unwrap() as usize,
                4096,
            );
            for part in bytes(&case["input"]).chunks(chunk) {
                terminal.feed(part);
            }
            if let Some(sizes) = case["resizes"].as_array() {
                for size in sizes {
                    terminal.resize(
                        size[0].as_u64().unwrap() as usize,
                        size[1].as_u64().unwrap() as usize,
                    );
                }
            }
            let capture = terminal.content_snapshot();
            let rows: Vec<_> = capture.rows().collect();
            let actual = search::find(
                capture,
                &bytes(&case["needle"]),
                Options::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
            let expected = case
                .get("corrected_matches")
                .unwrap_or(&case["matches"])
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    let start = Position {
                        row: item[0][0].as_u64().unwrap() as usize,
                        col: item[0][1].as_u64().unwrap() as usize,
                    };
                    let mut end = Position {
                        row: item[1][0].as_u64().unwrap() as usize,
                        col: item[1][1].as_u64().unwrap() as usize,
                    };
                    end.col += rows[end.row].cells()[end.col].width().max(1);
                    Match { start, end }
                })
                .collect::<Vec<_>>();
            if actual.matches != expected {
                failures.push(format!(
                    "{} chunk{chunk}: actual {:?}, expected {:?}",
                    case["name"], actual.matches, expected
                ));
            }
            assert_eq!(actual.total_matches, actual.matches.len());
            assert!(!actual.truncated());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn owned_results_survive_compression_eviction_reflow_and_teardown() {
    let mut terminal = Terminal::new(20, 4, 4096);
    for _ in 0..800 {
        terminal.feed(b"needle on a line\r\n");
    }
    while terminal.compress_history_step() {}
    let capture = terminal.content_snapshot();
    let expected = search::find(
        capture.clone(),
        b"needle",
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    for _ in 0..6000 {
        terminal.feed(b"new\r\n");
    }
    terminal.resize(3, 2);
    drop(terminal);
    let worker = std::thread::spawn(move || {
        search::find(
            capture,
            b"needle",
            Options::default(),
            &AtomicBool::new(false),
        )
        .unwrap()
    });
    let actual = worker.join().unwrap();
    assert_eq!(actual.matches, expected.matches);
    assert_eq!(actual.total_matches, 800);
    assert_eq!(actual.snapshot.rows, expected.snapshot.rows);
}

#[test]
fn retention_keeps_newest_overlaps_and_counts_the_rest() {
    let mut terminal = Terminal::new(80, 2, 0);
    terminal.feed(b"aaaaaaaaaa");
    for limit in [0, 1, 3, 100] {
        let result = search::find(
            terminal.content_snapshot(),
            b"aa",
            Options { max_matches: limit },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.total_matches, 9);
        assert_eq!(result.matches.len(), limit.min(9));
        assert_eq!(result.truncated(), limit < 9);
        for (index, m) in result.matches.iter().enumerate() {
            assert_eq!(
                m.start,
                Position {
                    row: 0,
                    col: 8 - index
                }
            );
            assert_eq!(
                m.end,
                Position {
                    row: 0,
                    col: 10 - index
                }
            );
        }
    }
}

#[test]
fn limits_and_cancellation_fail_without_partial_results() {
    let terminal = Terminal::new(80, 24, 0);
    assert_eq!(
        search::find(
            terminal.content_snapshot(),
            b"x",
            Options::default(),
            &AtomicBool::new(true)
        )
        .unwrap_err(),
        Error::Cancelled
    );
    assert_eq!(
        search::find(
            terminal.content_snapshot(),
            &vec![b'x'; search::MAX_NEEDLE_BYTES + 1],
            Options::default(),
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        Error::NeedleTooLong
    );
    assert_eq!(
        search::find(
            terminal.content_snapshot(),
            b"x",
            Options {
                max_matches: search::MAX_RETAINED_MATCHES + 1
            },
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        Error::TooManyRequestedMatches
    );
}
