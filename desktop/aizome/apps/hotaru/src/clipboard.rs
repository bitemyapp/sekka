//! One clipboard worker, one replaceable pending job, no interface-thread I/O.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use aizome_kit::{ClipboardRead, MsgSender, PreparedClipboard};
use hotaru_core::formatter::{self, FormatError, FormatOptions};
use hotaru_core::{ContentSnapshot, Snapshot};

use crate::app::Msg;
use crate::settings::CopyFormat;
use crate::terminal_view::Selection;

const COPY_LIMIT: usize = 128 * 1024 * 1024;

pub enum CopySource {
    Viewport { snapshot: Arc<Snapshot>, selection: Option<Selection> },
    Content(Arc<ContentSnapshot>),
    // A selection that goes past the screen, in a capture of all content.
    Selected { snapshot: Arc<ContentSnapshot>, start: (usize, usize), end: (usize, usize) },
    Search { results: Arc<hotaru_core::search::Results>, index: usize },
}

pub enum Job {
    Copy { token: u64, pane: u64, source: CopySource, format: CopyFormat, cancel: Arc<AtomicBool> },
    Paste { token: u64, tab: u64, read: ClipboardRead },
}

#[derive(Clone)]
enum Cancellation {
    Copy(Arc<AtomicBool>),
    Paste(ClipboardRead),
}

impl Cancellation {
    fn cancel(&self) {
        match self {
            Self::Copy(cancel) => cancel.store(true, Ordering::Release),
            Self::Paste(read) => read.cancel(),
        }
    }
}

impl Job {
    fn cancellation(&self) -> Cancellation {
        match self {
            Self::Copy { cancel, .. } => Cancellation::Copy(cancel.clone()),
            Self::Paste { read, .. } => Cancellation::Paste(read.clone()),
        }
    }
}

#[derive(Default)]
struct Pending {
    job: Option<Job>,
    active: Option<Cancellation>,
    stop: bool,
}

pub struct ClipboardWorker {
    pending: Arc<(Mutex<Pending>, Condvar)>,
}

pub(crate) fn prepare_copy(
    token: u64,
    pane: u64,
    source: CopySource,
    format: CopyFormat,
    cancel: &AtomicBool,
) -> Option<Msg> {
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    let (snapshot, selection) = match source {
        CopySource::Content(snapshot) => (snapshot, None),
        CopySource::Selected { snapshot, start, end } => {
            (snapshot, Some(formatter::Selection { start, end, rectangle: false }))
        }
        CopySource::Search { results, index } => {
            let found = results.matches.get(index)?;
            (
                Arc::new(results.snapshot.clone()),
                Some(formatter::Selection {
                    start: (found.start.row, found.start.col),
                    end: (found.end.row, found.end.col),
                    rectangle: false,
                }),
            )
        }
        CopySource::Viewport { snapshot, selection } => {
            let selection = selection.map(|selected| formatter::Selection {
                start: (selected.anchor.row, selected.anchor.col),
                end: (selected.end.row, selected.end.col),
                rectangle: false,
            });
            // The bounded visible-row conversion also stays off the UI thread.
            (Arc::new(ContentSnapshot::from((*snapshot).clone())), selection)
        }
    };
    let options =
        FormatOptions { format: format.formatter(), selection, trim: true, unwrap: true, max_bytes: COPY_LIMIT };
    let formatted = match formatter::format_copy(&snapshot, &options, cancel) {
        Ok(formatted) => formatted,
        Err(FormatError::Cancelled) => return None,
        Err(FormatError::LimitExceeded) => {
            return Some(Msg::CopyFailed(token, pane, "Copy exceeds the 128 MiB combined format limit.".into()));
        }
        Err(FormatError::InvalidSelection) => {
            return Some(Msg::CopyFailed(token, pane, "The selected text is no longer available.".into()));
        }
        Err(FormatError::AllocationFailed) => {
            return Some(Msg::CopyFailed(token, pane, "Not enough memory to prepare the copied text.".into()));
        }
    };
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    let plain = match formatted.plain.map(String::from_utf8).transpose() {
        Ok(plain) => plain,
        Err(_) => return Some(Msg::CopyFailed(token, pane, "Clipboard text could not be encoded.".into())),
    };
    let extra = formatted.html.into_iter().map(|html| ("text/html".into(), Arc::from(html))).collect();
    let prepared = PreparedClipboard::new(plain, extra);
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    Some(match prepared {
        Ok(prepared) => Msg::CopyReady(token, pane, prepared),
        Err(error) => Msg::CopyFailed(token, pane, format!("Clipboard formats could not be prepared: {error}")),
    })
}

impl ClipboardWorker {
    pub fn new(tx: MsgSender<Msg>) -> io::Result<Self> {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let worker = pending.clone();
        std::thread::Builder::new().name("hotaru-clipboard".into()).spawn(move || {
            loop {
                let job = {
                    let (lock, wake) = &*worker;
                    let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while !pending.stop && pending.job.is_none() {
                        pending = wake.wait(pending).unwrap_or_else(|e| e.into_inner());
                    }
                    if pending.stop {
                        return;
                    }
                    let job = pending.job.take().expect("pending clipboard job");
                    pending.active = Some(job.cancellation());
                    job
                };
                let message = match job {
                    Job::Copy { token, pane, source, format, cancel } => {
                        prepare_copy(token, pane, source, format, &cancel)
                    }
                    Job::Paste { token, tab, read } => {
                        let text = read.read(Duration::from_secs(2), 32 * 1024 * 1024 - 12);
                        Some(Msg::PasteReady(token, tab, read, text))
                    }
                };
                worker.0.lock().unwrap_or_else(|e| e.into_inner()).active = None;
                if let Some(message) = message
                    && !tx.send(message)
                {
                    return;
                }
            }
        })?;
        Ok(Self { pending })
    }

    pub fn submit(&self, job: Job) {
        let (lock, wake) = &*self.pending;
        let (old, active) = {
            let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
            (pending.job.replace(job), pending.active.clone())
        };
        if let Some(active) = active {
            active.cancel();
        }
        if let Some(old) = old {
            old.cancellation().cancel();
        }
        wake.notify_one();
    }
}

impl Drop for ClipboardWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.pending;
        let (pending, active) = {
            let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
            pending.stop = true;
            (pending.job.as_ref().map(Job::cancellation), pending.active.clone())
        };
        for cancellation in pending.into_iter().chain(active) {
            cancellation.cancel();
        }
        // The worker releases the pending snapshot, outside any UI lock.
        wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotaru_core::Terminal;

    #[test]
    fn cancelled_copy_never_prepares_or_publishes_mime_data() {
        let mut terminal = Terminal::new(80, 24, 100);
        terminal.feed(b"cancelled contents");
        let source = CopySource::Content(Arc::new(terminal.content_snapshot()));
        assert!(prepare_copy(1, 1, source, CopyFormat::Mixed, &AtomicBool::new(true)).is_none());
    }
}
