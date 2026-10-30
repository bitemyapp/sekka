use hotaru_core::{Snapshot, Style, Terminal};
use std::sync::mpsc::sync_channel;

#[derive(Debug, PartialEq, Eq)]
struct Contents {
    text: String,
    cells: Vec<(char, usize, Style, Option<String>)>,
    wraps: Vec<bool>,
}

fn contents(snapshot: &Snapshot) -> Contents {
    Contents {
        text: snapshot.plain_text(),
        cells: snapshot
            .lines
            .iter()
            .flat_map(|row| {
                row.cells().iter().enumerate().map(|(col, &cell)| {
                    (
                        cell.character(),
                        cell.width(),
                        row.style(cell),
                        row.grapheme(col).map(str::to_owned),
                    )
                })
            })
            .collect(),
        wraps: snapshot.lines.iter().map(|row| row.wrapped()).collect(),
    }
}

#[test]
fn concurrent_snapshot_readers_keep_owned_rows_during_reuse_and_reflow() {
    std::thread::scope(|scope| {
        let mut senders = Vec::new();
        for _ in 0..3 {
            let (send, recv) = sync_channel::<(Snapshot, Contents)>(3);
            senders.push(send);
            scope.spawn(move || {
                for (snapshot, expected) in recv {
                    // Clone/drop row owners while the terminal recycles rows on
                    // another thread. Keep one clone after releasing its parent.
                    let retained = snapshot.clone();
                    drop(snapshot);
                    std::thread::yield_now();
                    assert_eq!(contents(&retained), expected);
                }
            });
        }
        let mut terminal = Terminal::new(40, 8, 25);
        for round in 0..240 {
            terminal.feed(
                format!(
                    "\x1b[38;2;{};77;233m{round}:雪花 e\u{301} 👩🏽‍💻\x1b[0m\r\n",
                    round % 256
                )
                .as_bytes(),
            );
            if round % 7 == 0 {
                terminal.resize(3 + round % 47, 3 + round % 9);
            }
            let snapshot = terminal.snapshot_all();
            let expected = contents(&snapshot);
            senders[round % senders.len()]
                .send((snapshot, expected))
                .unwrap();
        }
        // The last queued snapshots remain valid after their terminal is gone.
        drop(terminal);
        drop(senders);
    });
}
