//! Cancellable search of an owned content capture, including cold history.
//!
//! Matching folds ASCII letters only and includes overlapping byte matches.
//! Soft wraps join; hard newlines and interior blanks remain searchable. The
//! worker streams rows rather than allocating the complete text or a byte map
//! for the history. Results retain their capture and never borrow a live PTY.
use crate::ContentSnapshot;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_NEEDLE_BYTES: usize = 65_536;
pub const MAX_RETAINED_MATCHES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    /// Zero is the oldest physical row in the captured content.
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub start: Position,
    /// Exclusive cell endpoint. Partial UTF-8/grapheme matches select the
    /// complete cell, including the trailing column of a wide character.
    pub end: Position,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Retain this many newest matches, while counting all matches. Zero is a
    /// count-only search. Exceeding MAX_RETAINED_MATCHES is an explicit error.
    pub max_matches: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            max_matches: 10_000,
        }
    }
}

#[derive(Debug)]
pub struct Results {
    pub snapshot: ContentSnapshot,
    pub needle: Vec<u8>,
    /// Newest to oldest, matching the direction of Find Next from the prompt.
    pub matches: Vec<Match>,
    pub total_matches: usize,
    pub bytes_scanned: usize,
    // Deferred spaces/newlines may map to historical cells but only exist
    // because later active text flushes them. Emission dependencies advance
    // monotonically through rows; generation-sensitive matches therefore form
    // a prefix after reversing results into newest-first order.
    generation_sensitive_prefix: usize,
    // Deferred newline coordinates depend on preceding content, which can be
    // pruned even when the reported match endpoints remain. Cache this once
    // so resolving many matches never rescans a potentially long needle.
    multiline: bool,
}
impl Results {
    pub fn truncated(&self) -> bool {
        self.total_matches > self.matches.len()
    }
    /// Resolve a retained match into the current whole screen. Historical rows
    /// are immutable and survive appends/pruning by absolute row identity.
    /// Matches touching the capture's active screen require the same content
    /// generation. All validation is metadata-only, suitable for the PTY actor.
    pub fn resolve_match(
        &self,
        index: usize,
        position: &crate::ContentPosition,
        generation: u64,
    ) -> Option<Match> {
        let found = *self.matches.get(index)?;
        let captured = &self.snapshot.position;
        if captured.epoch != position.epoch || found.start >= found.end {
            return None;
        }
        if self.multiline && captured.origin != position.origin {
            return None;
        }
        let last = found.end.row.checked_sub(usize::from(found.end.col == 0))?;
        if last >= self.snapshot.rows {
            return None;
        }
        if generation != self.snapshot.generation
            && (index < self.generation_sensitive_prefix
                || captured.first_row.checked_add(last)? >= captured.history_rows)
        {
            return None;
        }
        let rebase = |point: Position| -> Option<Position> {
            let absolute = captured.absolute_row(point.row)?;
            Some(Position {
                row: usize::try_from(absolute.checked_sub(position.origin)?).ok()?,
                col: point.col,
            })
        };
        let resolved = Match {
            start: rebase(found.start)?,
            end: rebase(found.end)?,
        };
        let total = position.history_rows.checked_add(position.active_rows)?;
        let end_row = resolved
            .end
            .row
            .checked_sub(usize::from(resolved.end.col == 0))?;
        (resolved.start.row < total && end_row < total).then_some(resolved)
    }
    /// Clip one safe match to this exact viewport. This never decodes history
    /// or reads live terminal state. End remains exclusive, within the viewport.
    pub fn project_match(&self, index: usize, target: &crate::Snapshot) -> Option<Match> {
        if target.rows == 0 || target.cols == 0 {
            return None;
        }
        let found = self.resolve_match(index, &target.position, target.generation)?;
        let first = target.position.first_row;
        let last = first.checked_add(target.rows - 1)?;
        let start = found.start.max(Position { row: first, col: 0 });
        let end = found.end.min(Position {
            row: last,
            col: target.cols,
        });
        if start >= end {
            return None;
        }
        Some(Match {
            start: Position {
                row: start.row.checked_sub(first)?,
                col: start.col,
            },
            end: Position {
                row: end.row.checked_sub(first)?,
                col: end.col,
            },
        })
    }
    /// O(retained matches), bounded by Options::max_matches. Call on a render
    /// worker to prepare highlights for its paired immutable screen snapshot.
    pub fn project(&self, target: &crate::Snapshot) -> Vec<Match> {
        (0..self.matches.len())
            .filter_map(|index| self.project_match(index, target))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    NeedleTooLong,
    TooManyRequestedMatches,
    AllocationFailed,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "search cancelled",
            Self::NeedleTooLong => "search needle exceeds 65536 bytes",
            Self::TooManyRequestedMatches => "search retention exceeds one million matches",
            Self::AllocationFailed => "search allocation failed",
        })
    }
}
impl std::error::Error for Error {}

struct Matcher<'a> {
    needle: Vec<u8>,
    prefix: Vec<usize>,
    positions: Vec<Position>,
    matched: usize,
    next: usize,
    bytes: usize,
    matches: VecDeque<Match>,
    total: usize,
    generation_sensitive: bool,
    generation_sensitive_matches: usize,
    limit: usize,
    cancel: &'a AtomicBool,
    last: Match,
}
impl Matcher<'_> {
    fn byte(&mut self, byte: u8, position: Match) -> Result<(), Error> {
        if self.bytes & 255 == 0 && self.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let byte = byte.to_ascii_lowercase();
        self.positions[self.next] = position.start;
        self.next += 1;
        if self.next == self.needle.len() {
            self.next = 0;
        }
        while self.matched > 0 && self.needle[self.matched] != byte {
            self.matched = self.prefix[self.matched - 1];
        }
        if self.needle[self.matched] == byte {
            self.matched += 1;
        }
        if self.matched == self.needle.len() {
            self.total += 1;
            if self.generation_sensitive {
                self.generation_sensitive_matches += 1;
            }
            if self.limit > 0 {
                if self.matches.len() == self.limit {
                    self.matches.pop_front();
                }
                self.matches
                    .try_reserve(1)
                    .map_err(|_| Error::AllocationFailed)?;
                self.matches.push_back(Match {
                    start: self.positions[self.next],
                    end: position.end,
                });
            }
            self.matched = self.prefix[self.matched - 1];
        }
        self.bytes += 1;
        self.last = position;
        Ok(())
    }
    fn text(&mut self, text: &str, position: Match) -> Result<(), Error> {
        for byte in text.bytes() {
            self.byte(byte, position)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct BlankRun {
    row: usize,
    start: usize,
    end: usize,
}

/// Run on a background worker. Cancellation discards partial results. The
/// result owns the exact capture used for coordinates; callers must reject
/// stale captures when presenting results against a newer screen generation.
/// This is a static capture search, not Ghostty's live incremental reconciler.
pub fn find(
    snapshot: ContentSnapshot,
    needle: &[u8],
    options: Options,
    cancel: &AtomicBool,
) -> Result<Results, Error> {
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if needle.len() > MAX_NEEDLE_BYTES {
        return Err(Error::NeedleTooLong);
    }
    if options.max_matches > MAX_RETAINED_MATCHES {
        return Err(Error::TooManyRequestedMatches);
    }
    let mut owned_needle = Vec::new();
    owned_needle
        .try_reserve_exact(needle.len())
        .map_err(|_| Error::AllocationFailed)?;
    owned_needle.extend_from_slice(needle);
    if needle.is_empty() {
        return Ok(Results {
            snapshot,
            needle: owned_needle,
            matches: Vec::new(),
            total_matches: 0,
            bytes_scanned: 0,
            generation_sensitive_prefix: 0,
            multiline: needle.contains(&b'\n'),
        });
    }
    let mut folded = Vec::new();
    folded
        .try_reserve_exact(needle.len())
        .map_err(|_| Error::AllocationFailed)?;
    folded.extend(needle.iter().map(u8::to_ascii_lowercase));
    let mut prefix = Vec::new();
    prefix
        .try_reserve_exact(needle.len())
        .map_err(|_| Error::AllocationFailed)?;
    prefix.resize(needle.len(), 0);
    for i in 1..needle.len() {
        if i & 1023 == 0 && cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut matched = prefix[i - 1];
        while matched > 0 && folded[i] != folded[matched] {
            matched = prefix[matched - 1];
        }
        if folded[i] == folded[matched] {
            matched += 1;
        }
        prefix[i] = matched;
    }
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(needle.len())
        .map_err(|_| Error::AllocationFailed)?;
    positions.resize(needle.len(), Position::default());
    let mut matcher = Matcher {
        needle: folded,
        prefix,
        positions,
        matched: 0,
        next: 0,
        bytes: 0,
        matches: VecDeque::new(),
        total: 0,
        generation_sensitive: false,
        generation_sensitive_matches: 0,
        limit: options.max_matches,
        cancel,
        last: Match {
            start: Position::default(),
            end: Position { row: 0, col: 1 },
        },
    };
    let mut rows = snapshot.rows();
    let mut blank_rows = 0;
    // Whitespace is delayed until followed by text, preserving soft-wrap
    // spaces while trimming hard-line ends. Runs retain coordinates only,
    // never decoded rows; adjacent columns coalesce into one run per row.
    let mut blanks: Vec<BlankRun> = Vec::new();
    let mut row_index = 0;
    while let Some(row) = rows
        .next_cancellable(cancel)
        .map_err(|_| Error::Cancelled)?
    {
        let y = row_index;
        row_index += 1;
        // A delayed blank run is committed by this row's nonblank text, not
        // by the older cell coordinates reported for its materialized bytes.
        // Once iteration reaches active rows this dependency cannot go back.
        matcher.generation_sensitive |= snapshot
            .position
            .first_row
            .checked_add(y)
            .is_none_or(|row| row >= snapshot.position.history_rows);
        let cells = &row.cells()[..row.cells().len().min(snapshot.cols)];
        if !cells.iter().any(|cell| cell.has_text()) {
            blank_rows += 1;
            continue;
        }
        if blank_rows > 0 {
            let previous = matcher.last;
            for offset in 0..blank_rows {
                let position = if offset == 0 {
                    previous
                } else {
                    Match {
                        start: Position {
                            row: previous.start.row + offset,
                            col: 0,
                        },
                        end: Position {
                            row: previous.start.row + offset,
                            col: 1,
                        },
                    }
                };
                matcher.byte(b'\n', position)?;
            }
            blank_rows = 0;
        }
        if !row.wrapped() {
            blank_rows += 1;
        }
        if !row.wrap_continuation {
            blanks.clear();
        }
        for (col, cell) in cells.iter().enumerate() {
            if cell.width() == 0 || cell.is_spacer_head() {
                continue;
            }
            if !cell.has_text() || cell.character() == ' ' {
                if let Some(run) = blanks
                    .last_mut()
                    .filter(|run| run.row == y && run.end == col)
                {
                    run.end += 1;
                } else {
                    blanks.try_reserve(1).map_err(|_| Error::AllocationFailed)?;
                    blanks.push(BlankRun {
                        row: y,
                        start: col,
                        end: col + 1,
                    });
                }
                continue;
            }
            for run in blanks.drain(..) {
                for col in run.start..run.end {
                    matcher.byte(
                        b' ',
                        Match {
                            start: Position { row: run.row, col },
                            end: Position {
                                row: run.row,
                                col: col + 1,
                            },
                        },
                    )?;
                }
            }
            let position = Match {
                start: Position { row: y, col },
                end: Position {
                    row: y,
                    col: col + cell.width(),
                },
            };
            if let Some(text) = row.grapheme(col) {
                matcher.text(text, position)?;
            } else {
                let mut bytes = [0; 4];
                matcher.text(cell.character().encode_utf8(&mut bytes), position)?;
            }
        }
    }
    // Ghostty search appends a final hard newline even to an empty screen,
    // mapping it to the last emitted cell (or cell zero for an empty screen).
    matcher.generation_sensitive = true;
    matcher.byte(b'\n', matcher.last)?;
    let generation_sensitive_prefix = matcher
        .generation_sensitive_matches
        .min(matcher.matches.len());
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let mut matches = Vec::new();
    matches
        .try_reserve_exact(matcher.matches.len())
        .map_err(|_| Error::AllocationFailed)?;
    matches.extend(matcher.matches.into_iter().rev());
    Ok(Results {
        snapshot,
        needle: owned_needle,
        matches,
        total_matches: matcher.total,
        bytes_scanned: matcher.bytes,
        generation_sensitive_prefix,
        multiline: needle.contains(&b'\n'),
    })
}
