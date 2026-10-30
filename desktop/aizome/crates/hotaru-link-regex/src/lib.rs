//! Native-only matching of the exact pinned Ghostty URL/path rule.
//! No filesystem access, subprocesses, UI or terminal mutation occurs here.
//! Call from a worker. Deadline/cancellation are cooperative between bounded
//! foreign searches; this API makes no hard wall-clock interruption guarantee.
mod engine;
use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
/// Exact src/config/url.zig default rule; actual Zig export verifies these bytes.
pub const DEFAULT_REGEX: &str = include_str!("default.regex");
pub const MAX_BYTES: usize = 64 * 1024;
const MAX_MATCHES: usize = 256;
const SEARCH_RETRIES: u32 = 100_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Bytes,
    Mapping,
    Matches,
    Searches,
    RetryAllowance,
    Stack,
    Deadline,
    Engine,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidInput,
    InvalidMapping,
    Allocation,
    Engine(i32),
    EngineContract,
    Cancelled,
    Budget(Limit),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "link matcher: {self:?}")
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CellPoint {
    pub row: u64,
    pub col: u16,
}
/// All bytes since the previous run end belong to this source cell. Wide tails
/// and grapheme byte coverage must be supplied according to the source mapper;
/// the matcher does not guess cells from UTF8 character counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappingRun {
    pub byte_end: u32,
    pub cell: CellPoint,
}
#[derive(Clone, Debug)]
pub struct LogicalSlice {
    text: Arc<str>,
    map: Arc<[MappingRun]>,
}
impl LogicalSlice {
    /// Validate on the worker, after the producer has bounded capture ownership.
    pub fn new(text: Arc<str>, map: Arc<[MappingRun]>) -> Result<Self, Error> {
        if text.len() > MAX_BYTES {
            return Err(Error::Budget(Limit::Bytes));
        }
        if map.len() > MAX_BYTES {
            return Err(Error::Budget(Limit::Mapping));
        }
        let mut end = 0;
        let mut previous = None;
        for entry in &*map {
            let next = entry.byte_end as usize;
            if next <= end
                || next > text.len()
                || !text.is_char_boundary(next)
                || entry.cell.col >= 4096
                || previous.is_some_and(|p| p > entry.cell)
            {
                return Err(Error::InvalidMapping);
            }
            end = next;
            previous = Some(entry.cell);
        }
        if end != text.len() {
            return Err(Error::InvalidMapping);
        }
        Ok(Self { text, map })
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    fn cell(&self, byte: usize) -> Result<CellPoint, Error> {
        self.map
            .get(self.map.partition_point(|m| m.byte_end as usize <= byte))
            .map(|m| m.cell)
            .ok_or(Error::InvalidMapping)
    }
}
#[derive(Clone, Debug)]
pub struct Match {
    source: LogicalSlice,
    bytes: Range<usize>,
    start: CellPoint,
    end: CellPoint,
}
impl Match {
    pub fn bytes(&self) -> Range<usize> {
        self.bytes.clone()
    }
    pub fn start(&self) -> CellPoint {
        self.start
    }
    pub fn end(&self) -> CellPoint {
        self.end
    }
    pub fn source(&self) -> &LogicalSlice {
        &self.source
    }
    pub fn text(&self) -> &str {
        &self.source.text[self.bytes.clone()]
    }
    pub fn contains(&self, point: CellPoint) -> bool {
        self.start <= point && point <= self.end
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_matches: usize,
    pub max_searches: usize,
    /// Conservative sum of the retry limits granted to calls, not observed steps.
    pub retry_allowance: u32,
    /// Engine StackType entry count, NOT bytes or a process-memory limit.
    /// The pinned library starts at160 and first promotes to320 entries; realloc can transiently
    /// hold old and double-sized storage. Accepted range512..=16_384 exceeds that initial promotion.
    pub stack_entries: u32,
    pub deadline: Instant,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_BYTES,
            max_matches: MAX_MATCHES,
            max_searches: 64,
            retry_allowance: 6_400_000,
            stack_entries: 4096,
            deadline: Instant::now() + Duration::from_millis(25),
        }
    }
}
pub struct Matcher {
    engine: engine::Engine,
}
impl Matcher {
    /// Construct on its owning worker; compilation/global initialization never
    /// runs in a UI callback or PTY owner. This type intentionally is not Send.
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            engine: engine::Engine::new()?,
        })
    }
    fn walk(
        &mut self,
        text: &str,
        limits: &Limits,
        cancel: &AtomicBool,
        mut accept: impl FnMut(Range<usize>) -> Result<bool, Error>,
    ) -> Result<(), Error> {
        if text.len() > limits.max_bytes.min(MAX_BYTES) {
            return Err(Error::Budget(Limit::Bytes));
        }
        if !(512..=16_384).contains(&limits.stack_entries) {
            return Err(Error::Budget(Limit::Stack));
        }
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= limits.deadline {
            return Err(Error::Budget(Limit::Deadline));
        }
        let mut offset = 0;
        let mut calls = 0usize;
        let mut matches = 0usize;
        let mut allowance = limits.retry_allowance.min(25_600_000);
        while offset < text.len() {
            if cancel.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= limits.deadline {
                return Err(Error::Budget(Limit::Deadline));
            }
            if calls >= limits.max_searches.min(256) {
                return Err(Error::Budget(Limit::Searches));
            }
            if allowance < SEARCH_RETRIES {
                return Err(Error::Budget(Limit::RetryAllowance));
            }
            calls += 1;
            allowance -= SEARCH_RETRIES;
            // This is intentionally a fresh suffix, matching StringMap::next:
            // lookbehind sees the suffix start, not an offset in the original text.
            let found = self
                .engine
                .search(&text[offset..], SEARCH_RETRIES, limits.stack_entries);
            if cancel.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= limits.deadline {
                return Err(Error::Budget(Limit::Deadline));
            }
            let Some(range) = found? else { return Ok(()) };
            if matches >= limits.max_matches.min(MAX_MATCHES) {
                return Err(Error::Budget(Limit::Matches));
            }
            matches += 1;
            let range = offset + range.start..offset + range.end;
            offset = range.end;
            if accept(range)? {
                return Ok(());
            }
        }
        Ok(())
    }
    pub fn ranges(
        &mut self,
        text: &str,
        limits: &Limits,
        cancel: &AtomicBool,
    ) -> Result<Vec<Range<usize>>, Error> {
        let mut found = Vec::new();
        self.walk(text, limits, cancel, |range| {
            found.try_reserve(1).map_err(|_| Error::Allocation)?;
            found.push(range);
            Ok(false)
        })?;
        Ok(found)
    }
    pub fn find_at(
        &mut self,
        source: &LogicalSlice,
        point: CellPoint,
        limits: &Limits,
        cancel: &AtomicBool,
    ) -> Result<Option<Match>, Error> {
        if point.col >= 4096 {
            return Err(Error::InvalidInput);
        }
        let mut found = None;
        self.walk(source.text(), limits, cancel, |bytes| {
            let start = source.cell(bytes.start)?;
            let end = source.cell(bytes.end - 1)?;
            if start <= point && point <= end {
                found = Some(Match {
                    source: source.clone(),
                    bytes,
                    start,
                    end,
                });
                Ok(true)
            } else {
                Ok(false)
            }
        })?;
        Ok(found)
    }
}
