//! Worker-only extraction from the exact immutable displayed snapshot.
//! Context clipped by the viewport is an explicit failure, never a new regex boundary.
use hotaru_core::Snapshot;
use hotaru_link_regex::{CellPoint, LogicalSlice, MAX_BYTES, MappingRun};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    ClippedStart,
    ClippedEnd,
    Cells,
    Bytes,
    Cancelled,
    Deadline,
    Allocation,
    Mapping,
}
#[derive(Debug)]
pub struct Mapped {
    pub source: LogicalSlice,
    pub start: CellPoint,
    pub end: CellPoint,
}
struct Budget<'a> {
    remaining: usize,
    cancel: &'a AtomicBool,
    deadline: Instant,
}
impl Budget<'_> {
    fn step(&mut self) -> Result<(), Error> {
        if self.remaining == 0 {
            return Err(Error::Cells);
        }
        self.remaining -= 1;
        if self.cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if self.remaining.is_multiple_of(128) && Instant::now() >= self.deadline {
            return Err(Error::Deadline);
        }
        Ok(())
    }
}
fn point(row: usize, col: usize) -> CellPoint {
    CellPoint { row: row as u64, col: col as u16 }
}
fn push(text: &mut String, map: &mut Vec<MappingRun>, value: &str, cell: CellPoint) -> Result<(), Error> {
    if value.len() > MAX_BYTES - text.len() {
        return Err(Error::Bytes);
    }
    text.try_reserve(value.len()).map_err(|_| Error::Allocation)?;
    text.push_str(value);
    if let Some(last) = map.last_mut().filter(|last| last.cell == cell) {
        last.byte_end = text.len() as u32;
    } else {
        map.try_reserve(1).map_err(|_| Error::Allocation)?;
        map.push(MappingRun { byte_end: text.len() as u32, cell });
    }
    Ok(())
}
pub fn extract(snapshot: &Snapshot, at: CellPoint, cancel: &AtomicBool, deadline: Instant) -> Result<Mapped, Error> {
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    let row = usize::try_from(at.row).map_err(|_| Error::Invalid)?;
    let col = usize::from(at.col);
    let cols = snapshot.cols;
    if cols == 0
        || cols > 4096
        || row >= snapshot.lines.len()
        || col >= cols
        || snapshot.lines.len() > 4096
        || snapshot.lines.iter().any(|r| r.cells().len() != cols)
    {
        return Err(Error::Invalid);
    }
    let semantic = snapshot.lines[row].cells()[col].semantic_content();
    let mut budget = Budget { remaining: MAX_BYTES * 2, cancel, deadline };
    let (mut sr, mut sc) = (row, col);
    loop {
        budget.step()?;
        if sc > 0 {
            if snapshot.lines[sr].cells()[sc - 1].semantic_content() != semantic {
                break;
            }
            sc -= 1;
        } else if sr > 0 {
            if !snapshot.lines[sr - 1].wrapped()
                || snapshot.lines[sr - 1].cells()[cols - 1].semantic_content() != semantic
            {
                break;
            }
            sr -= 1;
            sc = cols - 1;
        } else {
            if snapshot.position.first_row > 0 {
                return Err(Error::ClippedStart);
            }
            break;
        }
    }
    let (mut er, mut ec) = (row, col);
    loop {
        budget.step()?;
        if ec + 1 < cols {
            if snapshot.lines[er].cells()[ec + 1].semantic_content() != semantic {
                break;
            }
            ec += 1;
        } else {
            if !snapshot.lines[er].wrapped() {
                break;
            }
            if er + 1 >= snapshot.lines.len() {
                return Err(Error::ClippedEnd);
            }
            if snapshot.lines[er + 1].cells()[0].semantic_content() != semantic {
                break;
            }
            er += 1;
            ec = 0;
        }
    }
    let start = point(sr, sc);
    let end = point(er, ec);
    // The pinned formatter includes a wrapped wide glyph when selection ends on
    // its spacer head, and includes the head when selection starts on a tail.
    if snapshot.lines[er].cells()[ec].is_spacer_head() {
        if er + 1 >= snapshot.lines.len() {
            return Err(Error::ClippedEnd);
        }
        er += 1;
        ec = 0;
    }
    let cells = (er - sr)
        .checked_mul(cols)
        .and_then(|n| n.checked_add(ec + 1))
        .and_then(|n| n.checked_sub(sc))
        .ok_or(Error::Cells)?;
    if cells > MAX_BYTES {
        return Err(Error::Cells);
    }
    let mut text = String::new();
    let mut map = Vec::new();
    let mut blank_cells: Vec<CellPoint> = Vec::new();
    let mut blank_rows = 0usize;
    for y in sr..=er {
        budget.step()?;
        let line = &snapshot.lines[y];
        let cells = line.cells();
        let mut from = if y == sr { sc } else { 0 };
        let to = if y == er { ec + 1 } else { cols };
        if from > 0 {
            if cells[from].width() == 0 {
                from -= 1;
            } else if cells[from].is_spacer_head() {
                continue;
            }
        }
        if !cells[from..to].iter().any(|c| c.has_text()) {
            blank_rows += 1;
            continue;
        }
        if blank_rows > 0 {
            let previous = map.last().map(|m: &MappingRun| m.cell).unwrap_or(point(0, 0));
            for offset in 0..blank_rows {
                push(
                    &mut text,
                    &mut map,
                    "\n",
                    if offset == 0 { previous } else { point(previous.row as usize + offset, 0) },
                )?;
            }
            blank_rows = 0;
        }
        if !line.wrapped() {
            blank_rows += 1;
        }
        if !line.wrap_continuation() {
            blank_cells.clear();
        }
        for (x, cell) in cells.iter().enumerate().take(to).skip(from) {
            budget.step()?;
            if cell.width() == 0 || cell.is_spacer_head() {
                continue;
            }
            let p = point(y, x);
            if !cell.has_text() {
                if blank_cells.len() >= MAX_BYTES {
                    return Err(Error::Cells);
                }
                blank_cells.try_reserve(1).map_err(|_| Error::Allocation)?;
                blank_cells.push(p);
                continue;
            }
            for blank in blank_cells.drain(..) {
                push(&mut text, &mut map, " ", blank)?;
            }
            if let Some(cluster) = line.grapheme(x) {
                push(&mut text, &mut map, cluster, p)?;
            } else {
                let mut bytes = [0; 4];
                push(&mut text, &mut map, cell.character().encode_utf8(&mut bytes), p)?;
            }
        }
    }
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    let source = LogicalSlice::new(Arc::from(text), map.into()).map_err(|_| Error::Mapping)?;
    Ok(Mapped { source, start, end })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn run(data: &str, cols: usize, rows: usize, at: CellPoint) -> Result<Mapped, Error> {
        let mut terminal = hotaru_core::Terminal::new(cols, rows, 0);
        terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
        terminal.feed(data.as_bytes());
        extract(&terminal.snapshot(), at, &AtomicBool::new(false), Instant::now() + Duration::from_secs(1))
    }
    #[test]
    fn wraps_and_semantic_cell_boundaries_are_owned() {
        let mapped =
            run("\x1b]133;A\x07$ \x1b]133;B\x07https://例子.test/e\u{301}\x1b]133;C\x07", 12, 4, point(1, 2)).unwrap();
        assert_eq!(mapped.source.text(), "https://例子.test/e\u{301}");
        assert_eq!(mapped.start, point(0, 2));
    }
    #[test]
    fn clipped_context_and_cancelled_requests_do_not_create_regex_boundaries() {
        let mut terminal = hotaru_core::Terminal::new(12, 2, 0);
        terminal.feed(b"https://x");
        let mut snap = terminal.snapshot();
        snap.position.first_row = 20;
        assert_eq!(
            extract(&snap, point(0, 2), &AtomicBool::new(false), Instant::now() + Duration::from_secs(1)).unwrap_err(),
            Error::ClippedStart
        );
        assert_eq!(
            extract(&snap, point(0, 2), &AtomicBool::new(true), Instant::now() + Duration::from_secs(1)).unwrap_err(),
            Error::Cancelled
        );
        assert_eq!(extract(&snap, point(0, 2), &AtomicBool::new(false), Instant::now()).unwrap_err(), Error::Deadline);
    }
    #[test]
    fn unprinted_blanks_differ_from_literal_trailing_spaces() {
        assert_eq!(run("https://x   ", 24, 2, point(0, 3)).unwrap().source.text(), "https://x   ");
        assert_eq!(run("https://x", 24, 2, point(0, 3)).unwrap().source.text(), "https://x");
        assert_eq!(run("/tmp/a\x1b[Cb.txt", 24, 2, point(0, 3)).unwrap().source.text(), "/tmp/a b.txt");
    }
}
