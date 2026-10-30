use hotaru_link_regex::*;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
fn limits() -> Limits {
    Limits {
        deadline: Instant::now() + Duration::from_secs(10),
        ..Default::default()
    }
}
#[test]
fn exact_pinned_source_cases() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("upstream.json")).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 90);
    let mut matcher = Matcher::new().unwrap();
    for case in cases.as_array().unwrap() {
        let text = case["input"].as_str().unwrap();
        let ranges = matcher
            .ranges(text, &limits(), &AtomicBool::new(false))
            .unwrap();
        let got = ranges.first().map(|r| &text[r.clone()]);
        assert_eq!(got, case["expected"].as_str(), "{text:?}");
    }
}
#[test]
fn owned_mapping_preserves_wrapped_unicode_and_combining_coordinates() {
    let text: Arc<str> = Arc::from("https://例子.test/界e\u{301}");
    let mut map = Vec::new();
    let mut col = 0u16;
    let mut previous = CellPoint { row: 0, col: 0 };
    for (i, ch) in text.char_indices() {
        let point = if ch == '\u{301}' {
            previous
        } else {
            CellPoint {
                row: u64::from(col / 12),
                col: col % 12,
            }
        };
        map.push(MappingRun {
            byte_end: (i + ch.len_utf8()) as u32,
            cell: point,
        });
        if ch != '\u{301}' {
            col += if "例子界".contains(ch) { 2 } else { 1 };
            previous = point;
        }
    }
    let source = LogicalSlice::new(text.clone(), map.clone().into()).unwrap();
    let mut matcher = Matcher::new().unwrap();
    let found = matcher
        .find_at(
            &source,
            CellPoint { row: 1, col: 0 },
            &limits(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
    assert_eq!(found.start(), CellPoint { row: 0, col: 0 });
    assert_eq!(found.end(), map.last().unwrap().cell);
    drop(source);
    drop(text);
    drop(map);
    assert_eq!(found.text(), "https://例子.test/界e\u{301}");
}
#[test]
fn invalid_mapping_and_input_never_reach_foreign_search() {
    for map in [
        vec![],
        vec![MappingRun {
            byte_end: 1,
            cell: CellPoint { row: 0, col: 0 },
        }],
        vec![MappingRun {
            byte_end: 3,
            cell: CellPoint { row: 0, col: 4096 },
        }],
        vec![MappingRun {
            byte_end: 9,
            cell: CellPoint { row: 0, col: 0 },
        }],
    ] {
        assert!(LogicalSlice::new(Arc::from("界"), map.into()).is_err());
    }
    let mut matcher = Matcher::new().unwrap();
    assert_eq!(
        matcher.ranges(
            &"a".repeat(MAX_BYTES + 1),
            &limits(),
            &AtomicBool::new(false)
        ),
        Err(Error::Budget(Limit::Bytes))
    );
    assert_eq!(
        matcher.ranges("https://example.test", &limits(), &AtomicBool::new(true)),
        Err(Error::Cancelled)
    );
    assert_eq!(
        matcher.ranges(
            "https://example.test",
            &Limits {
                deadline: Instant::now(),
                ..limits()
            },
            &AtomicBool::new(false)
        ),
        Err(Error::Budget(Limit::Deadline))
    );
}
#[test]
fn total_work_limits_are_explicit_and_do_not_return_partial_results() {
    let text = "https://one.test https://two.test";
    let mut matcher = Matcher::new().unwrap();
    let cancel = AtomicBool::new(false);
    for (budget, expected) in [
        (
            Limits {
                max_matches: 1,
                ..limits()
            },
            Limit::Matches,
        ),
        (
            Limits {
                max_searches: 1,
                ..limits()
            },
            Limit::Searches,
        ),
        (
            Limits {
                retry_allowance: 100_000,
                ..limits()
            },
            Limit::RetryAllowance,
        ),
        (
            Limits {
                stack_entries: 0,
                ..limits()
            },
            Limit::Stack,
        ),
    ] {
        assert_eq!(
            matcher.ranges(text, &budget, &cancel),
            Err(Error::Budget(expected))
        );
    }
    assert_eq!(matcher.ranges(text, &limits(), &cancel).unwrap().len(), 2);
}
#[test]
fn independent_workers_compile_search_and_drop_without_global_termination() {
    let handles: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..40 {
                    let mut m = Matcher::new().unwrap();
                    assert_eq!(
                        m.ranges(
                            "x https://example.test y",
                            &limits(),
                            &AtomicBool::new(false)
                        )
                        .unwrap(),
                        vec![2..22]
                    );
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn mapping_tracks_supplied_cells_without_inventing_wide_tail_coverage() {
    let source = LogicalSlice::new(
        Arc::from("https://x/界"),
        vec![
            MappingRun {
                byte_end: 10,
                cell: CellPoint { row: 9, col: 7 },
            },
            MappingRun {
                byte_end: 13,
                cell: CellPoint { row: 10, col: 0 },
            },
        ]
        .into(),
    )
    .unwrap();
    let mut matcher = Matcher::new().unwrap();
    let cancel = AtomicBool::new(false);
    let result = matcher
        .find_at(&source, CellPoint { row: 10, col: 0 }, &limits(), &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(result.bytes(), 0..13);
    assert_eq!(result.end(), CellPoint { row: 10, col: 0 });
    // The matcher honors the supplied formatter byte map; native extraction must
    // independently establish whether/how a wide-tail click maps to its head.
    assert!(!result.contains(CellPoint { row: 10, col: 1 }));
    assert!(
        matcher
            .find_at(&source, CellPoint { row: 10, col: 1 }, &limits(), &cancel)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        matcher
            .find_at(
                &source,
                CellPoint { row: 10, col: 4096 },
                &limits(),
                &cancel
            )
            .unwrap_err(),
        Error::InvalidInput
    );
}

#[test]
fn map_validation_rejects_order_endpoints_and_obeys_empty_cancellation() {
    let point = |row, col| CellPoint { row, col };
    for map in [
        vec![
            MappingRun {
                byte_end: 1,
                cell: point(0, 0),
            },
            MappingRun {
                byte_end: 1,
                cell: point(0, 1),
            },
        ],
        vec![
            MappingRun {
                byte_end: 1,
                cell: point(1, 0),
            },
            MappingRun {
                byte_end: 2,
                cell: point(0, 1),
            },
        ],
        vec![
            MappingRun {
                byte_end: 1,
                cell: point(0, 1),
            },
            MappingRun {
                byte_end: 2,
                cell: point(0, 0),
            },
        ],
    ] {
        assert!(matches!(
            LogicalSlice::new(Arc::from("ab"), map.into()),
            Err(Error::InvalidMapping)
        ));
    }
    let source = LogicalSlice::new(Arc::from(""), Arc::from([])).unwrap();
    let mut matcher = Matcher::new().unwrap();
    assert!(
        matcher
            .find_at(&source, point(0, 0), &limits(), &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        matcher.ranges("", &limits(), &AtomicBool::new(true)),
        Err(Error::Cancelled)
    );
    assert_eq!(
        matcher.ranges(
            "",
            &Limits {
                deadline: Instant::now(),
                ..limits()
            },
            &AtomicBool::new(false)
        ),
        Err(Error::Budget(Limit::Deadline))
    );
}

#[test]
fn adversarial_inputs_and_hard_caps_have_explicit_outcomes() {
    let mut matcher = Matcher::new().unwrap();
    let cancel = AtomicBool::new(false);
    for text in [
        "/".repeat(MAX_BYTES),
        "(".repeat(MAX_BYTES),
        "$".repeat(MAX_BYTES),
        "a".repeat(MAX_BYTES),
    ] {
        let result = matcher.ranges(&text, &limits(), &cancel);
        assert!(
            matches!(result, Ok(_) | Err(Error::Budget(_))),
            "{result:?}"
        );
    }
    // Caller-provided larger limits do not disable the hard caps.
    let permissive = Limits {
        max_bytes: usize::MAX,
        max_searches: usize::MAX,
        max_matches: usize::MAX,
        retry_allowance: u32::MAX,
        ..limits()
    };
    assert_eq!(
        matcher.ranges(&"a".repeat(MAX_BYTES + 1), &permissive, &cancel),
        Err(Error::Budget(Limit::Bytes))
    );
    assert_eq!(
        matcher.ranges(&"https://x ".repeat(300), &permissive, &cancel),
        Err(Error::Budget(Limit::Searches))
    );
    assert_eq!(
        matcher.ranges(
            "https://x",
            &Limits {
                stack_entries: 16_385,
                ..limits()
            },
            &cancel
        ),
        Err(Error::Budget(Limit::Stack))
    );
    assert_eq!(
        matcher.ranges(
            "https://x",
            &Limits {
                max_matches: 0,
                ..limits()
            },
            &cancel
        ),
        Err(Error::Budget(Limit::Matches))
    );
    assert_eq!(
        matcher.ranges(
            "https://x",
            &Limits {
                max_searches: 0,
                ..limits()
            },
            &cancel
        ),
        Err(Error::Budget(Limit::Searches))
    );
}

#[test]
fn stack_entry_limits_reject_below_promotion_and_accept_documented_boundaries() {
    let mut matcher = Matcher::new().unwrap();
    let cancel = AtomicBool::new(false);
    for stack_entries in [0, 1, 319, 320, 511, 16_385, u32::MAX] {
        assert_eq!(
            matcher.ranges(
                "https://x",
                &Limits {
                    stack_entries,
                    ..limits()
                },
                &cancel
            ),
            Err(Error::Budget(Limit::Stack))
        );
    }
    for stack_entries in [512, 513, 4096, 16_384] {
        assert_eq!(
            matcher
                .ranges(
                    "https://x",
                    &Limits {
                        stack_entries,
                        ..limits()
                    },
                    &cancel
                )
                .unwrap(),
            vec![0..9]
        );
    }
}
