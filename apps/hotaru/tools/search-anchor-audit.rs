use hotaru_core::{
    Terminal,
    search::{self, Options},
};
use std::sync::atomic::AtomicBool;
#[test]
fn final_newline_depends_on_active_continuation() {
    let mut t = Terminal::new(4, 1, 10);
    t.feed(b"abcd ");
    let old = search::find(
        t.content_snapshot(),
        b"d\n",
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(old.matches.len(), 1);
    assert_eq!(old.snapshot.position.history_rows, 1);
    assert_eq!(old.matches[0].end.row, 0);
    t.feed(b"X");
    let now = search::find(
        t.content_snapshot(),
        b"d\n",
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(now.matches.is_empty());
    assert!(
        old.resolve_match(0, &t.content_position(0), t.generation())
            .is_none(),
        "old result still resolves: {:?}",
        old.matches
    );
}
#[test]
fn deferred_spaces_depend_on_active_nonblank() {
    let mut t = Terminal::new(4, 1, 10);
    t.feed(b"a   X");
    let old = search::find(
        t.content_snapshot(),
        b"a ",
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(old.matches.len(), 1);
    t.feed(b"\x1b[2K");
    let now = search::find(
        t.content_snapshot(),
        b"a ",
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(now.matches.is_empty());
    assert!(
        old.resolve_match(0, &t.content_position(0), t.generation())
            .is_none(),
        "old result still resolves: {:?}",
        old.matches
    );
}
#[test]
fn deferred_newlines_depend_on_active_nonblank() {
    let mut t = Terminal::new(4, 1, 10);
    t.feed(b"a\r\n\r\nX");
    let old = search::find(
        t.content_snapshot(),
        b"\n\n",
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(old.matches.len(), 1);
    t.feed(b"\x1b[2K");
    let now = search::find(
        t.content_snapshot(),
        b"\n\n",
        Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(now.matches.is_empty());
    assert!(
        old.resolve_match(0, &t.content_position(0), t.generation())
            .is_none(),
        "old result still resolves: {:?}",
        old.matches
    );
}
#[test]
fn every_resolved_match_still_exists_after_active_mutation_or_pruning() {
    let queries: &[&[u8]] = &[b"a", b"a ", b" \n", b"\n", b"\n\n", b" X", b"ab", b"b\n"];
    for seed in 0..60 {
        let mut t = Terminal::new(4 + seed % 5, 1 + seed % 3, 30);
        let mut random = 0x29312982_u64 ^ seed as u64;
        for _ in 0..100 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let b: &[u8] = match random % 7 {
                0 => b"a",
                1 => b"b",
                2 => b" ",
                3 => b"   ",
                4 => b"X",
                _ => b"\r\n",
            };
            t.feed(b);
        }
        let operations: &[&[u8]] = &[
            b"\x1b[2K",
            b"\rX",
            b"\r\n\r\n",
            b"a   X",
            b"\x1b[2J",
            b"ab\r\n",
            b"\x1b[1;1H\x1b[L",
            b"a\r\n\r\nX",
            b"\x1b[3J",
        ];
        for (step, op) in operations.iter().enumerate() {
            let mut old = Vec::new();
            for &q in queries {
                for max_matches in [0, 1, 2, 100] {
                    old.push(
                        search::find(
                            t.content_snapshot(),
                            q,
                            Options { max_matches },
                            &AtomicBool::new(false),
                        )
                        .unwrap(),
                    );
                }
            }
            t.feed(op);
            while t.compress_history_step() {}
            for result in old {
                let now = search::find(
                    t.content_snapshot(),
                    &result.needle,
                    Options::default(),
                    &AtomicBool::new(false),
                )
                .unwrap();
                for index in 0..result.matches.len() {
                    if let Some(resolved) =
                        result.resolve_match(index, &t.content_position(0), t.generation())
                    {
                        assert!(
                            now.matches.contains(&resolved),
                            "seed={seed} step={step} query={:?} stale={resolved:?} fresh={:?}",
                            result.needle,
                            now.matches
                        );
                    }
                }
            }
        }
    }
}
