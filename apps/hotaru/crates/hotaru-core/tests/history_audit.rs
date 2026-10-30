//! Compression must be observationally transparent through live mutations,
//! row recycling, alternate screens, resize and retained snapshot ownership.
use hotaru_core::{Snapshot, Style, Terminal};

type CellImage = (char, usize, bool, Style, Option<String>);
type RowImage = (bool, Vec<CellImage>);

fn image(snapshot: &Snapshot) -> Vec<RowImage> {
    snapshot
        .lines
        .iter()
        .map(|row| {
            (
                row.wrapped(),
                row.cells()
                    .iter()
                    .enumerate()
                    .map(|(col, cell)| {
                        (
                            cell.character(),
                            cell.width(),
                            cell.is_protected(),
                            row.style(*cell),
                            row.grapheme(col).map(str::to_owned),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

fn compare(actual: &Snapshot, expected: &Snapshot, step: usize) {
    assert_eq!(
        (actual.cols, actual.rows, actual.cursor),
        (expected.cols, expected.rows, expected.cursor),
        "geometry at step {step}"
    );
    assert_eq!(actual.palette, expected.palette, "palette at step {step}");
    assert_eq!(actual.default_foreground, expected.default_foreground);
    assert_eq!(actual.default_background, expected.default_background);
    assert_eq!(actual.lines.len(), expected.lines.len());
    for (row, (a, b)) in actual.lines.iter().zip(&expected.lines).enumerate() {
        assert_eq!(a.wrapped(), b.wrapped(), "wrap at step {step}, row {row}");
        assert_eq!(a.cells().len(), b.cells().len());
        for (col, (ac, bc)) in a.cells().iter().zip(b.cells()).enumerate() {
            assert_eq!(
                (
                    ac.character(),
                    ac.width(),
                    ac.is_protected(),
                    a.style(*ac),
                    a.grapheme(col),
                ),
                (
                    bc.character(),
                    bc.width(),
                    bc.is_protected(),
                    b.style(*bc),
                    b.grapheme(col),
                ),
                "cell at step {step}, row {row}, col {col}"
            );
        }
    }
}

#[test]
fn compression_matches_resident_mutations_and_retained_snapshots() {
    let mut compressed = Terminal::new(40, 3, 1024);
    let mut resident = Terminal::new(40, 3, 1024);
    let mut seed = 0x2931_2982u32;
    let mut held = Vec::new();
    let mut did_compress = false;
    let tokens = [
        "\r\n",
        "Hello world\r\n",
        "\x1b[38;2;12;34;56m色e\u{301}y\r\n",
        "\x1b[0m",
        "\x1b[2;1H",
        "\x1b[1;1H",
        "\x1b[2K",
        "\x1b[K",
        "\x1b[P",
        "\x1b[@",
        "\x1b[3X",
        "\x1b7",
        "\x1b8",
        "\x1b[?7l",
        "\x1b[?7h",
        "\x1b[?1049h",
        "\x1b[?1049l",
        "\x1b[42m",
        "\x1b[4:3m",
        "\x1b[5;9r",
    ];
    for step in 0..12_000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let line = format!("{step:08}:alpha e\u{301}字 omega\r\n");
        let data = if !seed.is_multiple_of(5) {
            line.as_str()
        } else {
            tokens[(seed as usize >> 8) % tokens.len()]
        };
        compressed.feed(data.as_bytes());
        resident.feed(data.as_bytes());
        if step % 79 == 0 {
            let cols = 1 + seed as usize % 80;
            let rows = 1 + (seed as usize >> 16) % 9;
            compressed.resize(cols, rows);
            resident.resize(cols, rows);
        }
        if step % 23 == 0 {
            let mut ticks = 0;
            while compressed.compress_history_step() {
                ticks += 1;
                assert!(ticks < 2000, "compression failed to settle at {step}");
            }
            did_compress |= compressed.history_memory().compressed_rows > 0;
        }
        if step % 41 == 0 || (6700..6800).contains(&step) {
            compare(&compressed.snapshot_all(), &resident.snapshot_all(), step);
        }
        if step % 503 == 0 {
            let snapshot = compressed.snapshot_all();
            let expected = image(&snapshot);
            held.push((snapshot, expected));
            if held.len() > 4 {
                held.remove(0);
            }
        }
        if step % 751 == 0 {
            for (snapshot, expected) in &held {
                // Keep failure output bounded even for a large retained image.
                assert!(
                    image(snapshot) == *expected,
                    "held snapshot changed at {step}"
                );
            }
        }
    }
    assert!(
        did_compress,
        "the regression must exercise compressed storage"
    );
    compare(&compressed.snapshot_all(), &resident.snapshot_all(), 12_000);
}
