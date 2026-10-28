use hotaru_core::{ContentSnapshot, DEFAULT_HISTORY_BYTES, HistoryLimits, Terminal};
use std::sync::atomic::AtomicBool;

fn terminal(bytes: Option<usize>) -> Terminal {
    Terminal::with_history_limits(40, 4, HistoryLimits { rows: 1000, bytes })
}
fn compress(t: &mut Terminal) {
    let mut count = 0;
    while t.compress_history_step() {
        count += 1;
        assert!(count < 10_000);
    }
}
fn same(a: &ContentSnapshot, b: &ContentSnapshot) {
    assert_eq!((a.cols, a.rows, a.cursor), (b.cols, b.rows, b.cursor));
    for (a, b) in a.rows().zip(b.rows()) {
        assert_eq!(a.cells(), b.cells());
        assert_eq!(a.wrapped(), b.wrapped());
        for (col, cell) in a.cells().iter().enumerate() {
            assert_eq!(a.style(*cell), b.style(b.cells()[col]));
            assert_eq!(a.grapheme(col), b.grapheme(col));
            assert_eq!(a.hyperlink(col), b.hyperlink(col));
        }
    }
}
fn bounded(t: &Terminal, limit: usize) {
    let memory = t.history_memory();
    assert!(
        memory.logical_bytes <= limit.saturating_sub(memory.active_bytes),
        "logical={} active={} limit={}",
        memory.logical_bytes,
        memory.active_bytes,
        limit
    );
}
#[test]
fn default_policy_and_explicit_unlimited_row_limit_are_distinct() {
    assert_eq!(
        Terminal::new(40, 4, 1000).history_memory().limit_bytes,
        Some(DEFAULT_HISTORY_BYTES)
    );
    let mut limited = terminal(Some(12_000));
    let mut unlimited = terminal(None);
    for _ in 0..1200 {
        limited.feed(b"line\r\n");
        unlimited.feed(b"line\r\n");
        bounded(&limited, 12_000);
    }
    assert!(limited.scrollback_len() < 30);
    assert_eq!(unlimited.scrollback_len(), 1000);
}
#[test]
fn compression_preserves_byte_pruning_through_metadata_churn_shrink_and_recycling() {
    const LIMIT: usize = 80_000;
    let mut resident = terminal(Some(LIMIT));
    let mut cold = terminal(Some(LIMIT));
    let mut held = Vec::new();
    let mut did_compress = false;
    for index in 0..1800 {
        let uri = format!("https://example.test/{index}/{}", "u".repeat(index % 97));
        let line = format!(
            "\x1b]8;id={index};{uri}\x1b\\\x1b[38;2;{};72;91m{index:04} 雪 e{}\x1b]8;;\x1b\\\x1b[0m\r\n",
            index % 255,
            "\u{301}".repeat(index % 29)
        );
        for t in [&mut resident, &mut cold] {
            if index % 47 == 0 {
                for style in 0..91 {
                    t.feed(format!("\r\x1b[38;5;{style}mX").as_bytes());
                }
            }
            t.feed(line.as_bytes());
            if index % 131 == 0 {
                t.feed(b"\x1b[?7l");
                t.resize(17, 4);
            }
            if index % 131 == 3 {
                t.resize(40, 4);
                t.feed(b"\x1b[?7h");
            }
            bounded(t, LIMIT);
        }
        if index % 13 == 0 {
            compress(&mut cold);
            did_compress |= cold.history_memory().compressed_rows > 0;
        }
        if index % 19 == 0 {
            assert_eq!(
                resident.history_memory().logical_bytes,
                cold.history_memory().logical_bytes,
                "step{index}"
            );
            assert_eq!(
                resident.history_memory().active_bytes,
                cold.history_memory().active_bytes,
                "active step{index}"
            );
            same(&resident.content_snapshot(), &cold.content_snapshot());
        }
        if index % 317 == 0 {
            held.push((resident.content_snapshot(), cold.content_snapshot()));
        }
    }
    assert!(did_compress);
    drop(resident);
    drop(cold);
    for (a, b) in held {
        same(&a, &b);
    }
}
#[test]
fn oversized_active_metadata_always_fits_but_discards_old_history() {
    let mut t = terminal(Some(8_000));
    for _ in 0..30 {
        t.feed(b"old\r\n");
    }
    assert!(t.scrollback_len() > 0);
    for col in 0..20 {
        let data = format!(
            "\x1b]8;id={col};https://example.test/{}\x1b\\X",
            "u".repeat(1700)
        );
        t.feed(data.as_bytes());
    }
    let memory = t.history_memory();
    assert!(memory.active_bytes > 8_000);
    assert_eq!(memory.rows, 0);
    assert_eq!(t.snapshot().lines[3].hyperlink(19).unwrap().uri.len(), 1721);
    t.feed(b"\x1b]8;;\x1b\\\x1b[2J");
    bounded(&t, 8_000);
    for _ in 0..30 {
        t.feed(b"new\r\n");
    }
    assert!(t.scrollback_len() > 0);
}
#[test]
fn held_capture_estimates_survive_pruning_and_cancelled_iteration() {
    let mut t = terminal(Some(40_000));
    for _ in 0..1000 {
        t.feed(b"captured\r\n");
    }
    let resident = t.content_snapshot();
    let before = resident.retention();
    compress(&mut t);
    let cold = t.content_snapshot();
    let cold_before = cold.retention();
    assert_eq!(resident.history_rows, cold.history_rows);
    assert!(cold_before.storage_bytes_estimate < before.storage_bytes_estimate);
    for _ in 0..1500 {
        t.feed(b"replacement\r\n");
    }
    drop(t);
    assert_eq!(resident.retention(), before);
    assert_eq!(cold.retention(), cold_before);
    same(&resident, &cold);
    assert!(
        cold.rows()
            .next_cancellable(&AtomicBool::new(true))
            .is_err()
    );
}

#[test]
fn byte_pruning_is_chunk_invariant_with_peak_allocations_and_held_cow() {
    const LIMIT: usize = 60_000;
    let mut stages = vec![b"older retained rows\r\n".repeat(300)];
    for index in 0..50 {
        let mut stage = Vec::new();
        for col in 0..40 {
            stage.extend_from_slice(
                format!(
                    "\x1b]8;id={index}-{col};https://example.test/{}\x1b\\\x1b[38;5;{}mX",
                    "u".repeat(1500),
                    (index + col) % 255
                )
                .as_bytes(),
            );
        }
        stage.extend_from_slice(b"\x1b]8;;\x1b\\\r");
        stage.extend_from_slice(
            format!(
                "e{}\x1b[2K\x1b[0m\x1b[2Jnew {index}\r\n",
                "\u{301}".repeat(63)
            )
            .as_bytes(),
        );
        stage.extend_from_slice(b"fresh retained rows\r\n".repeat(100).as_slice());
        stages.push(stage);
    }
    let mut expected = None;
    for cold in [false, true] {
        for chunk in [1, 7, 65536] {
            let mut t = terminal(Some(LIMIT));
            let mut render_holds = Vec::new();
            let mut content_holds = Vec::new();
            for stage in &stages {
                for part in stage.chunks(chunk) {
                    t.feed(part);
                }
                bounded(&t, LIMIT);
                if cold {
                    compress(&mut t);
                }
                render_holds.push(t.snapshot());
                content_holds.push(t.content_snapshot());
                if render_holds.len() > 3 {
                    render_holds.remove(0);
                    content_holds.remove(0);
                }
            }
            let before = t.history_memory().active_bytes;
            for held in &content_holds {
                let _ = held.rows().count();
            }
            drop(render_holds);
            drop(content_holds);
            assert_eq!(
                t.history_memory().active_bytes,
                before,
                "capture lifetime must not alter live accounting"
            );
            let actual = t.content_snapshot();
            if let Some(expected) = &expected {
                same(&actual, expected);
            } else {
                expected = Some(actual);
            }
        }
    }
}

#[test]
fn overwrite_allocation_order_is_independent_of_ascii_and_unicode_batching() {
    let cluster = format!("e{}", "\u{301}".repeat(63));
    let old_link = format!("\x1b]8;id=old;https://old.test/{}\x1b\\", "u".repeat(1200));
    let new_link = format!("\x1b]8;id=new;https://new.test/{}\x1b\\", "v".repeat(1200));
    let cases = [
        (cluster.clone(), "\r\x1b[31m雪".to_owned()),
        (
            format!("{cluster} other {cluster}"),
            "\r\x1b[31mreplacement text".to_owned(),
        ),
        (
            format!("{old_link}old linked row\x1b]8;;\x1b\\ {cluster}"),
            format!("\r{new_link}\x1b[31m{}", "雪".repeat(12)),
        ),
        (
            format!("{old_link}old linked row\x1b]8;;\x1b\\ {cluster}"),
            format!("\r{new_link}\x1b[31mASCII replacement text"),
        ),
    ];
    for (seed, overwrite) in cases {
        let mut initial = b"old\r\n".repeat(80);
        initial.extend_from_slice(seed.as_bytes());
        let mut reference = terminal(None);
        reference.feed(&initial);
        let memory = reference.history_memory();
        let exact_limit = memory.logical_bytes + memory.active_bytes;
        for limit in [None, Some(exact_limit)] {
            let mut expected = None;
            for cold in [false, true] {
                for chunk in [1, 7, 65536] {
                    let mut t = terminal(limit);
                    for part in initial.chunks(chunk) {
                        t.feed(part);
                    }
                    let render = t.snapshot();
                    let content = t.content_snapshot();
                    if cold {
                        compress(&mut t);
                    }
                    for part in overwrite.as_bytes().chunks(chunk) {
                        t.feed(part);
                    }
                    let memory = t.history_memory();
                    let charges = (memory.rows, memory.logical_bytes, memory.active_bytes);
                    let actual = t.content_snapshot();
                    if let Some((snapshot, prior_charges)) = &expected {
                        same(snapshot, &actual);
                        assert_eq!(*prior_charges, charges, "limit={limit:?} chunk={chunk}");
                    } else {
                        expected = Some((actual, charges));
                    }
                    drop((render, content));
                    assert_eq!(t.history_memory().active_bytes, memory.active_bytes);
                }
            }
        }
    }
}
