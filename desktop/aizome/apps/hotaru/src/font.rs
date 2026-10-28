//! Bounded font worker per pane: one active and one replaceable pending frame.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Once};

use aizome_kit::MsgSender;
use aizome_kit::sidestep_ui::{Font, grid};
use hotaru_core::{Snapshot, Style, attrs};

use crate::app::Msg;
use crate::terminal_view::Selection;

#[derive(Clone)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: Style,
    pub selected: bool,
    pub matched: bool,
    pub current_match: bool,
    pub layout: Arc<grid::Layout>,
}

pub struct Frame {
    /// Conservative logical ownership charge for optional previous-frame
    /// retention. Computed on the font worker; this is not an RSS estimate.
    pub retention_bytes: usize,
    pub epoch: u64,
    pub snapshot: Arc<Snapshot>,
    pub selection: Option<Selection>,
    pub metrics: grid::Metrics,
    pub rows: Vec<Arc<[Span]>>,
    pub primary_family: String,
    pub missing_families: Vec<String>,
    pub links: Vec<LinkSpan>,
}

pub struct LinkSpan {
    pub row: usize,
    pub start: usize,
    pub end: usize,
}

pub struct Job {
    pub epoch: u64,
    pub snapshot: Arc<Snapshot>,
    pub config: Arc<grid::FontConfig>,
    pub scale: f64,
    pub selection: Option<Selection>,
    pub search: Option<crate::search::Cache>,
}

#[derive(Default)]
struct Pending {
    job: Option<Job>,
    active: Option<(u64, Arc<AtomicBool>)>,
    stop: bool,
}

pub struct Worker {
    pending: Arc<(Mutex<Pending>, Condvar)>,
}

fn install_fonts() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let Some(directory) = std::env::var_os("HOTARU_FONT_DIR").map(std::path::PathBuf::from) else { return };
        // Explicit filenames and process-local registration preserve user font
        // discovery. Opening/parsing these large files never happens on the UI.
        for name in ["NotoColorEmoji.ttf", "NotoSansCJK-Regular.ttc"] {
            let path = directory.join(name);
            match Font::install_file(&path) {
                Ok(families) => log::debug!("terminal fallback families {:?} from {}", families, path.display()),
                Err(error) => log::warn!("terminal fallback {}: {error}", path.display()),
            }
        }
    });
}

impl Worker {
    pub fn new(id: u64, tx: MsgSender<Msg>) -> std::io::Result<Self> {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let shared = pending.clone();
        std::thread::Builder::new().name(format!("hotaru-font-{id}")).spawn(move || {
            install_fonts();
            let mut fonts: Option<grid::FontSet> = None;
            let mut cache = RowCache::default();
            loop {
                let (job, cancel) = {
                    let (lock, wake) = &*shared;
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while !state.stop && state.job.is_none() {
                        state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stop {
                        return;
                    }
                    let job = state.job.take().expect("pending font job");
                    let cancel = Arc::new(AtomicBool::new(false));
                    state.active = Some((job.epoch, cancel.clone()));
                    (job, cancel)
                };
                let result = (|| {
                    if fonts.as_ref().is_none_or(|f| f.config() != job.config.as_ref()) {
                        fonts = Some(grid::FontSet::new((*job.config).clone())?);
                    }
                    shape(&job, fonts.as_mut().expect("font set"), &cancel, &mut cache)
                })();
                shared.0.lock().unwrap_or_else(|e| e.into_inner()).active = None;
                if cancel.load(Ordering::Acquire) {
                    continue;
                }
                let message = match result {
                    Ok(frame) => Msg::Shaped(id, Arc::new(frame)),
                    Err(grid::Error::Cancelled) => continue,
                    Err(error) => Msg::FontFailed(id, job.epoch, error.to_string()),
                };
                if !tx.send(message) {
                    return;
                }
            }
        })?;
        Ok(Self { pending })
    }

    pub fn submit(&self, job: Job) {
        let (old, cancel) = {
            let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
            let cancel = state.active.as_ref().filter(|(epoch, _)| *epoch != job.epoch).map(|(_, c)| c.clone());
            (state.job.replace(job), cancel)
        };
        if let Some(cancel) = cancel {
            cancel.store(true, Ordering::Release);
        }
        drop(old);
        self.pending.1.notify_one();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let cancel = {
            let mut state = self.pending.0.lock().unwrap_or_else(|e| e.into_inner());
            state.stop = true;
            state.active.as_ref().map(|(_, c)| c.clone())
        };
        if let Some(cancel) = cancel {
            cancel.store(true, Ordering::Release);
        }
        self.pending.1.notify_one();
    }
}

// Admit at most one preceding frame and 8 MiB of conservatively estimated
// row/layout storage per pane. During a replacement two generations may live
// briefly (16 MiB); displayed/queued frames and FontSet caches are separate.
const ROW_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default, Clone, Copy, Debug)]
struct Work {
    prepared: usize,
    prepared_cells: usize,
    reused: usize,
    retained_before: usize,
    retained_after: usize,
}

#[derive(Hash, Eq, PartialEq)]
struct RowKey {
    identity: usize,
    cols: usize,
    cursor: Option<u32>,
    selection: Option<(usize, usize)>,
    highlights: Vec<(usize, usize, u8)>,
}

#[derive(Clone)]
struct CachedRow {
    // Keeps identity stable until the lookup table is discarded. A raw address
    // without this owned reference would permit allocator-address ABA.
    source: hotaru_core::RowRef,
    spans: Arc<[Span]>,
    links: Arc<[(usize, usize)]>,
    bytes: usize,
}

struct RowCache {
    config: Option<Arc<grid::FontConfig>>,
    scale: u64,
    rows: HashMap<RowKey, CachedRow>,
    bytes: usize,
    limit: usize,
    work: Work,
}
impl Default for RowCache {
    fn default() -> Self {
        Self { config: None, scale: 0, rows: HashMap::new(), bytes: 0, limit: ROW_CACHE_BYTES, work: Work::default() }
    }
}

fn selection_range(selection: Option<Selection>, row: usize, cols: usize) -> Option<(usize, usize)> {
    let selection = selection?;
    let (start, end) = if (selection.anchor.row, selection.anchor.col) <= (selection.end.row, selection.end.col) {
        (selection.anchor, selection.end)
    } else {
        (selection.end, selection.anchor)
    };
    if row < start.row || row > end.row {
        return None;
    }
    let start_col = if row == start.row { start.col.min(cols) } else { 0 };
    let end_col = if row == end.row { end.col.min(cols) } else { cols };
    (start_col < end_col).then_some((start_col, end_col))
}

fn retention_total(parts: impl IntoIterator<Item = usize>) -> usize {
    parts.into_iter().fold(0, usize::saturating_add)
}
fn layout_bytes(layout: &grid::Layout) -> usize {
    // Deliberately count a shared layout once per span: an admission estimate,
    // not unique process allocations. Toolkit font data has separate ownership.
    retention_total([
        std::mem::size_of::<grid::Layout>() + 32,
        layout.runs.len().saturating_mul(std::mem::size_of::<grid::Run>()).saturating_add(32),
        layout.clusters.len().saturating_mul(std::mem::size_of::<grid::Cluster>()).saturating_add(32),
        retention_total(layout.runs.iter().map(|run| std::mem::size_of_val(run.glyphs.as_ref()).saturating_add(32))),
    ])
}
fn frame_auxiliary_bytes(frame: &Frame) -> usize {
    retention_total([
        std::mem::size_of::<Frame>() + std::mem::size_of::<Snapshot>() + 64,
        frame.snapshot.lines.capacity().saturating_mul(std::mem::size_of::<hotaru_core::RowRef>()),
        frame.rows.capacity().saturating_mul(std::mem::size_of::<Arc<[Span]>>()),
        frame.links.capacity().saturating_mul(std::mem::size_of::<LinkSpan>()),
        frame.primary_family.capacity(),
        frame.missing_families.capacity().saturating_mul(std::mem::size_of::<String>()),
        retention_total(frame.missing_families.iter().map(String::capacity)),
    ])
}

fn shape(
    job: &Job,
    fonts: &mut grid::FontSet,
    cancel: &AtomicBool,
    cache: &mut RowCache,
) -> Result<Frame, grid::Error> {
    let metrics = fonts.metrics(job.scale)?;
    if cache.config.as_deref() != Some(job.config.as_ref()) || cache.scale != job.scale.to_bits() {
        cache.rows.clear();
        cache.bytes = 0;
    }
    cache.config = Some(job.config.clone());
    cache.scale = job.scale.to_bits();
    cache.work = Work::default();
    let previous = std::mem::take(&mut cache.rows);
    let previous_bytes = std::mem::take(&mut cache.bytes);
    let mut next = HashMap::new();
    let mut next_bytes = 0usize;
    let mut retained_rows = 0usize;
    let mut rows = Vec::with_capacity(job.snapshot.rows);
    let mut links: Vec<LinkSpan> = Vec::new();
    let projected = job.search.as_ref().map_or_else(Vec::new, |cache| cache.results.project(&job.snapshot));
    let current = job
        .search
        .as_ref()
        .and_then(|cache| cache.selected.and_then(|index| cache.results.project_match(index, &job.snapshot)));
    for (y, row) in job.snapshot.lines.iter().enumerate().take(job.snapshot.rows) {
        if cancel.load(Ordering::Relaxed) {
            return Err(grid::Error::Cancelled);
        }
        let cursor =
            (job.snapshot.cursor.visible && job.snapshot.cursor.row == y).then_some(job.snapshot.cursor.col as u32);
        let mut key = RowKey {
            identity: (&**row as *const hotaru_core::Row) as usize,
            cols: job.snapshot.cols,
            cursor,
            selection: selection_range(job.selection, y, row.cells().len()),
            highlights: Vec::new(),
        };
        for (found, mark) in projected.iter().map(|found| (found, 1)).chain(current.iter().map(|found| (found, 2))) {
            if y >= found.start.row && y <= found.end.row {
                let start = if y == found.start.row { found.start.col } else { 0 };
                let end = if y == found.end.row { found.end.col } else { row.cells().len() };
                if start.min(row.cells().len()) < end.min(row.cells().len()) {
                    key.highlights.push((start, end, mark));
                }
            }
        }
        if let Some(hit) = previous.get(&key) {
            debug_assert!(hotaru_core::RowRef::ptr_eq(&hit.source, row));
            cache.work.reused += 1;
            retained_rows = retained_rows.saturating_add(hit.bytes);
            rows.push(hit.spans.clone());
            links.extend(hit.links.iter().map(|&(start, end)| LinkSpan { row: y, start, end }));
            // A previous admitted entry can still fail admission when another
            // row consumed this generation's budget. Drawing remains complete.
            if next_bytes.saturating_add(hit.bytes) <= cache.limit {
                next_bytes += hit.bytes;
                next.insert(key, hit.clone());
            }
            continue;
        }
        cache.work.prepared += 1;
        cache.work.prepared_cells += row.cells().len().min(job.snapshot.cols);
        let links_start = links.len();
        for col in 0..row.cells().len().min(job.snapshot.cols) {
            if let Some(link) = row.hyperlink(col) {
                if let Some(last) = links.last_mut()
                    && last.row == y
                    && last.end == col
                    && row.hyperlink(last.start) == Some(link)
                {
                    last.end += 1;
                } else {
                    links.push(LinkSpan { row: y, start: col, end: col + 1 });
                }
            }
        }
        let mut matches = vec![0u8; row.cells().len()];
        for (found, mark) in projected.iter().map(|found| (found, 1)).chain(current.iter().map(|found| (found, 2))) {
            if y < found.start.row || y > found.end.row {
                continue;
            }
            let start = if y == found.start.row { found.start.col } else { 0 };
            let end = if y == found.end.row { found.end.col } else { matches.len() };
            for cell in matches.iter_mut().take(end).skip(start) {
                *cell = mark;
            }
        }
        let mut spans = Vec::new();
        let mut column = 0;
        while column < row.cells().len().min(job.snapshot.cols) {
            let first = row.cells()[column];
            if first.width() == 0 {
                column += 1;
                continue;
            }
            let style = row.style(first);
            let selected = job.selection.is_some_and(|s| s.contains(y, column));
            let matched = matches[column];
            let start = column;
            let mut owned = Vec::new();
            loop {
                let cell = row.cells()[column];
                if cell.width() > 0 {
                    owned.push((
                        row.grapheme(column).map(str::to_owned).unwrap_or_else(|| cell.character().to_string()),
                        column,
                        cell.width(),
                    ));
                }
                column += cell.width().max(1);
                if column >= row.cells().len().min(job.snapshot.cols)
                    || row.style(row.cells()[column]) != style
                    || job.selection.is_some_and(|s| s.contains(y, column)) != selected
                    || matches[column] != matched
                {
                    break;
                }
            }
            let cells: Vec<_> = owned
                .iter()
                .map(|(text, col, width)| grid::Cell { text, column: *col as u32, width: *width as u8 })
                .collect();
            let cursor =
                (job.snapshot.cursor.visible && job.snapshot.cursor.row == y).then_some(job.snapshot.cursor.col as u32);
            let layout = fonts.shape(
                &cells,
                grid::Style { bold: style.attrs & attrs::BOLD != 0, italic: style.attrs & attrs::ITALIC != 0 },
                job.scale,
                cursor,
                cancel,
            )?;
            spans.push(Span {
                start,
                end: column,
                style,
                selected,
                matched: matched > 0,
                current_match: matched == 2,
                layout,
            });
        }
        let spans: Arc<[Span]> = spans.into();
        let local_links: Arc<[(usize, usize)]> =
            links[links_start..].iter().map(|link| (link.start, link.end)).collect();
        let bytes = row
            .storage_bytes_estimate()
            .saturating_add(4 * (std::mem::size_of::<CachedRow>() + std::mem::size_of::<RowKey>()) + 128)
            .saturating_add(key.highlights.capacity().saturating_mul(std::mem::size_of::<(usize, usize, u8)>()))
            .saturating_add(local_links.len().saturating_mul(std::mem::size_of::<(usize, usize)>()).saturating_add(32))
            .saturating_add(spans.len().saturating_mul(std::mem::size_of::<Span>()).saturating_add(32))
            .saturating_add(retention_total(spans.iter().map(|span| layout_bytes(&span.layout))));
        retained_rows = retained_rows.saturating_add(bytes);
        if next_bytes.saturating_add(bytes) <= cache.limit {
            next_bytes += bytes;
            next.insert(key, CachedRow { source: row.clone(), spans: spans.clone(), links: local_links, bytes });
        }
        rows.push(spans);
    }
    cache.work.retained_before = previous_bytes;
    cache.work.retained_after = next_bytes;
    cache.rows = next;
    cache.bytes = next_bytes;
    let mut frame = Frame {
        retention_bytes: retained_rows,
        epoch: job.epoch,
        snapshot: job.snapshot.clone(),
        selection: job.selection,
        metrics,
        rows,
        primary_family: fonts.primary_family().into(),
        missing_families: fonts.missing_families.clone(),
        links,
    };
    frame.retention_bytes = frame.retention_bytes.saturating_add(frame_auxiliary_bytes(&frame));
    Ok(frame)
}

#[cfg(test)]
struct UncachedFrame {
    epoch: u64,
    snapshot: Arc<Snapshot>,
    selection: Option<Selection>,
    metrics: grid::Metrics,
    rows: Vec<Vec<Span>>,
    primary_family: String,
    missing_families: Vec<String>,
    links: Vec<LinkSpan>,
}
#[cfg(test)]
impl UncachedFrame {
    fn into_frame(self) -> Frame {
        Frame {
            // The preserved uncached benchmark does not participate in pixel
            // retention and must not acquire extra timed accounting work.
            retention_bytes: usize::MAX,
            epoch: self.epoch,
            snapshot: self.snapshot,
            selection: self.selection,
            metrics: self.metrics,
            rows: self.rows.into_iter().map(Arc::from).collect(),
            primary_family: self.primary_family,
            missing_families: self.missing_families,
            links: self.links,
        }
    }
}

#[cfg(test)]
fn shape_uncached(job: &Job, fonts: &mut grid::FontSet, cancel: &AtomicBool) -> Result<UncachedFrame, grid::Error> {
    let metrics = fonts.metrics(job.scale)?;
    let mut rows = Vec::with_capacity(job.snapshot.rows);
    let mut links: Vec<LinkSpan> = Vec::new();
    let projected = job.search.as_ref().map_or_else(Vec::new, |cache| cache.results.project(&job.snapshot));
    let current = job
        .search
        .as_ref()
        .and_then(|cache| cache.selected.and_then(|index| cache.results.project_match(index, &job.snapshot)));
    for (y, row) in job.snapshot.lines.iter().enumerate().take(job.snapshot.rows) {
        if cancel.load(Ordering::Relaxed) {
            return Err(grid::Error::Cancelled);
        }
        for col in 0..row.cells().len().min(job.snapshot.cols) {
            if let Some(link) = row.hyperlink(col) {
                if let Some(last) = links.last_mut()
                    && last.row == y
                    && last.end == col
                    && row.hyperlink(last.start) == Some(link)
                {
                    last.end += 1;
                } else {
                    links.push(LinkSpan { row: y, start: col, end: col + 1 });
                }
            }
        }
        let mut matches = vec![0u8; row.cells().len()];
        for (found, mark) in projected.iter().map(|found| (found, 1)).chain(current.iter().map(|found| (found, 2))) {
            if y < found.start.row || y > found.end.row {
                continue;
            }
            let start = if y == found.start.row { found.start.col } else { 0 };
            let end = if y == found.end.row { found.end.col } else { matches.len() };
            for cell in matches.iter_mut().take(end).skip(start) {
                *cell = mark;
            }
        }
        let mut spans = Vec::new();
        let mut column = 0;
        while column < row.cells().len().min(job.snapshot.cols) {
            let first = row.cells()[column];
            if first.width() == 0 {
                column += 1;
                continue;
            }
            let style = row.style(first);
            let selected = job.selection.is_some_and(|s| s.contains(y, column));
            let matched = matches[column];
            let start = column;
            let mut owned = Vec::new();
            loop {
                let cell = row.cells()[column];
                if cell.width() > 0 {
                    owned.push((
                        row.grapheme(column).map(str::to_owned).unwrap_or_else(|| cell.character().to_string()),
                        column,
                        cell.width(),
                    ));
                }
                column += cell.width().max(1);
                if column >= row.cells().len().min(job.snapshot.cols)
                    || row.style(row.cells()[column]) != style
                    || job.selection.is_some_and(|s| s.contains(y, column)) != selected
                    || matches[column] != matched
                {
                    break;
                }
            }
            let cells: Vec<_> = owned
                .iter()
                .map(|(text, col, width)| grid::Cell { text, column: *col as u32, width: *width as u8 })
                .collect();
            let cursor =
                (job.snapshot.cursor.visible && job.snapshot.cursor.row == y).then_some(job.snapshot.cursor.col as u32);
            let layout = fonts.shape(
                &cells,
                grid::Style { bold: style.attrs & attrs::BOLD != 0, italic: style.attrs & attrs::ITALIC != 0 },
                job.scale,
                cursor,
                cancel,
            )?;
            spans.push(Span {
                start,
                end: column,
                style,
                selected,
                matched: matched > 0,
                current_match: matched == 2,
                layout,
            });
        }
        rows.push(spans);
    }
    Ok(UncachedFrame {
        epoch: job.epoch,
        snapshot: job.snapshot.clone(),
        selection: job.selection,
        metrics,
        rows,
        primary_family: fonts.primary_family().into(),
        missing_families: fonts.missing_families.clone(),
        links,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_retention_charge_saturates_instead_of_admitting_wrapped_sizes() {
        assert_eq!(retention_total([usize::MAX - 8, 8, 1]), usize::MAX);
        assert_eq!(retention_total([100, 200, 300]), 600);
    }

    #[test]
    fn whole_content_search_projects_wide_and_soft_wrapped_highlights_on_worker() {
        let mut terminal = hotaru_core::Terminal::new(6, 4, 8);
        terminal.feed("ab界cDE".as_bytes());
        let cancel = AtomicBool::new(false);
        let results =
            hotaru_core::search::find(terminal.content_snapshot(), "界cDE".as_bytes(), Default::default(), &cancel)
                .unwrap();
        assert_eq!(results.total_matches, 1);
        let config = Arc::new(grid::FontConfig::default());
        let mut fonts = grid::FontSet::new((*config).clone()).unwrap();
        let job = Job {
            epoch: 0,
            snapshot: Arc::new(terminal.snapshot()),
            config,
            scale: 1.0,
            selection: None,
            search: Some(crate::search::Cache { results: Arc::new(results), selected: Some(0) }),
        };
        let frame = shape(&job, &mut fonts, &cancel, &mut RowCache::default()).unwrap();
        let spans: Vec<_> = frame
            .rows
            .iter()
            .enumerate()
            .flat_map(|(row, spans)| {
                spans.iter().filter(|span| span.current_match).map(move |span| (row, span.start, span.end))
            })
            .collect();
        assert_eq!(spans, [(0, 2, 6), (1, 0, 1)]);
    }

    fn fingerprint(frame: &Frame) -> String {
        let mut out = format!(
            "{:?}|{:?}|{:?}|{}|{:?}",
            frame.metrics, frame.selection, frame.epoch, frame.primary_family, frame.missing_families
        );
        use std::fmt::Write;
        for (y, row) in frame.rows.iter().enumerate() {
            for span in row.iter() {
                write!(
                    &mut out,
                    "{y}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}|",
                    span.start, span.end, span.style, span.selected, span.matched, span.current_match, span.layout
                )
                .unwrap();
            }
        }
        for link in &frame.links {
            write!(&mut out, "L{},{},{}|", link.row, link.start, link.end).unwrap();
        }
        out
    }

    fn compared(job: &Job, fonts: &mut grid::FontSet, cache: &mut RowCache) -> Frame {
        let cancel = AtomicBool::new(false);
        let actual = shape(job, fonts, &cancel, cache).unwrap();
        let expected = shape_uncached(job, fonts, &cancel).unwrap().into_frame();
        assert!(Arc::ptr_eq(&actual.snapshot, &job.snapshot));
        assert_eq!(fingerprint(&actual), fingerprint(&expected));
        assert!(cache.bytes <= cache.limit);
        assert!(cache.rows.len() <= job.snapshot.rows);
        actual
    }

    fn job(terminal: &hotaru_core::Terminal) -> Job {
        Job {
            epoch: 0,
            snapshot: Arc::new(terminal.snapshot()),
            config: Arc::new(grid::FontConfig::default()),
            scale: 1.0,
            selection: None,
            search: None,
        }
    }

    #[test]
    fn row_cache_reuses_moved_rows_and_preserves_frame_and_link_ownership() {
        install_fonts();
        let mut terminal = hotaru_core::Terminal::new(24, 8, 32);
        terminal.feed(b"\x1b[?25l");
        for y in 0..8 {
            terminal.feed(
                format!("\x1b[{};1H\x1b]8;id=r{y};https://example.test/{y}\x1b\\row {y} != ===\x1b]8;;\x1b\\", y + 1)
                    .as_bytes(),
            );
        }
        let mut work = job(&terminal);
        let mut fonts = grid::FontSet::new((*work.config).clone()).unwrap();
        let mut cache = RowCache::default();
        let held = compared(&work, &mut fonts, &mut cache);
        let held_text = fingerprint(&held);
        assert_eq!(cache.work.prepared, 8);
        let reused = compared(&work, &mut fonts, &mut cache);
        assert_eq!(reused.retention_bytes, held.retention_bytes);
        assert!(Arc::ptr_eq(&reused.snapshot, &held.snapshot));
        assert_eq!(reused.snapshot.cursor, held.snapshot.cursor);
        assert_eq!(cache.work.reused, 8);
        assert_eq!(cache.work.prepared_cells, 0);
        terminal.feed(b"\x1b[8;1H\r\nnew row");
        work.snapshot = Arc::new(terminal.snapshot());
        let moved = compared(&work, &mut fonts, &mut cache);
        assert_eq!(cache.work.reused, 7);
        assert_eq!(cache.work.prepared, 1);
        assert!(Arc::ptr_eq(&held.rows[1], &moved.rows[0]));
        assert_eq!(moved.links[0].row, 0);
        assert_eq!(fingerprint(&held), held_text);
        terminal.resize(1, 3);
        terminal.feed("界\u{301}".as_bytes());
        work.snapshot = Arc::new(terminal.snapshot());
        compared(&work, &mut fonts, &mut cache);
        assert_eq!(fingerprint(&held), held_text);
    }

    #[test]
    fn row_cache_context_limits_cancellation_and_metadata_are_complete() {
        install_fonts();
        let mut terminal = hotaru_core::Terminal::new(32, 6, 32);
        terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
        terminal.feed("!= === e\u{301} 界 👩\u{200d}💻\r\nneedle needle\r\nthird row".as_bytes());
        let mut work = job(&terminal);
        let mut fonts = grid::FontSet::new((*work.config).clone()).unwrap();
        let mut cache = RowCache::default();
        compared(&work, &mut fonts, &mut cache);
        work.selection = Some(Selection {
            anchor: crate::terminal_view::CellPoint { row: 2, col: 5 },
            end: crate::terminal_view::CellPoint { row: 0, col: 1 },
        });
        work.epoch += 1;
        compared(&work, &mut fonts, &mut cache);
        assert_eq!(cache.work.prepared, 3);
        assert_eq!(cache.work.reused, 3);
        let cancel = AtomicBool::new(false);
        let results =
            hotaru_core::search::find(terminal.content_snapshot(), b"needle", Default::default(), &cancel).unwrap();
        work.search = Some(crate::search::Cache { results: Arc::new(results), selected: Some(0) });
        compared(&work, &mut fonts, &mut cache);
        work.search.as_mut().unwrap().selected = Some(1);
        compared(&work, &mut fonts, &mut cache);
        assert_eq!(cache.work.prepared, 1);
        work.scale = 1.25;
        compared(&work, &mut fonts, &mut cache);
        assert_eq!(cache.work.prepared, 6);
        let mut config = (*work.config).clone();
        config.size = 18.0;
        work.config = Arc::new(config);
        fonts = grid::FontSet::new((*work.config).clone()).unwrap();
        compared(&work, &mut fonts, &mut cache);
        assert_eq!(cache.work.prepared, 6);
        cache.limit = 1;
        compared(&work, &mut fonts, &mut cache);
        assert!(cache.rows.is_empty());
        compared(&work, &mut fonts, &mut cache);
        assert_eq!(cache.work.prepared, 6);
        assert!(cache.rows.is_empty());
        cache.limit = ROW_CACHE_BYTES;
        compared(&work, &mut fonts, &mut cache);
        let cancelled = AtomicBool::new(true);
        assert!(matches!(shape(&work, &mut fonts, &cancelled, &mut cache), Err(grid::Error::Cancelled)));
        assert!(cache.rows.is_empty());
        assert_eq!(cache.bytes, 0);
        compared(&work, &mut fonts, &mut cache);
    }

    #[test]
    fn row_cache_matches_uncached_seeded_snapshot_transitions() {
        install_fonts();
        let mut terminal = hotaru_core::Terminal::new(40, 9, 128);
        terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
        let mut work = job(&terminal);
        let mut fonts = grid::FontSet::new((*work.config).clone()).unwrap();
        let mut cache = RowCache::default();
        let mut seed = 0x2931_2982u32;
        let cancel = AtomicBool::new(false);
        let mut held = Vec::new();
        for step in 0..600 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let row = (seed as usize % work.snapshot.rows) + 1;
            let col = ((seed >> 8) as usize % work.snapshot.cols) + 1;
            match seed % 13 {
                0 => terminal.feed(format!("\x1b[{row};{col}H\x1b[38;5;{}mX", seed as u8).as_bytes()),
                1 => terminal.feed(" != -> === e\u{301} 雪 👍🏽 ".as_bytes()),
                2 => terminal.feed(b"\r\nscrolling needle"),
                3 => terminal.feed(format!("\x1b[{row};{col}H").as_bytes()),
                4 => terminal.feed(b"\x1b[?25l"),
                5 => terminal.feed(b"\x1b[?25h"),
                6 => terminal
                    .feed(format!("\x1b]8;id={step};https://example.test/{step}\x1b\\link\x1b]8;;\x1b\\").as_bytes()),
                7 => terminal.feed(b"\x1b[2K"),
                8 => terminal.resize(1 + (seed as usize % 60), 1 + ((seed >> 12) as usize % 12)),
                9 => {
                    let a = crate::terminal_view::CellPoint { row: row - 1, col: col - 1 };
                    let b = crate::terminal_view::CellPoint {
                        row: (seed >> 20) as usize % work.snapshot.rows,
                        col: (seed >> 16) as usize % work.snapshot.cols,
                    };
                    work.selection = Some(Selection { anchor: a, end: b });
                    work.epoch += 1;
                }
                10 => {
                    work.selection = None;
                    work.search = None;
                    work.epoch += 1;
                }
                11 => {
                    let results =
                        hotaru_core::search::find(terminal.content_snapshot(), b"needle", Default::default(), &cancel)
                            .unwrap();
                    work.search =
                        Some(crate::search::Cache { results: Arc::new(results), selected: Some(seed as usize % 3) });
                }
                _ => {
                    work.scale = [1.0, 1.25, 2.0][seed as usize % 3];
                    work.epoch += 1;
                }
            }
            work.snapshot = Arc::new(terminal.snapshot());
            let frame = compared(&work, &mut fonts, &mut cache);
            if step % 41 == 0 {
                held.push((fingerprint(&frame), frame));
            }
            if held.len() > 3 {
                held.remove(0);
            }
            for (expected, old) in &held {
                assert_eq!(fingerprint(old), *expected);
            }
        }
    }
    include!("font_benchmark.rs");
}
