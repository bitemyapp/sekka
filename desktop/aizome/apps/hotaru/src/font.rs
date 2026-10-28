//! Bounded font worker per pane: one active and one replaceable pending frame.

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
    pub epoch: u64,
    pub snapshot: Arc<Snapshot>,
    pub selection: Option<Selection>,
    pub metrics: grid::Metrics,
    pub rows: Vec<Vec<Span>>,
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
                    shape(&job, fonts.as_mut().expect("font set"), &cancel)
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

fn shape(job: &Job, fonts: &mut grid::FontSet, cancel: &AtomicBool) -> Result<Frame, grid::Error> {
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
    Ok(Frame {
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
        let frame = shape(&job, &mut fonts, &cancel).unwrap();
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
}
