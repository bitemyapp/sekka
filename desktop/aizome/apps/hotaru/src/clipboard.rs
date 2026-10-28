//! One clipboard worker, one replaceable pending job, no interface-thread I/O.

use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use aizome_kit::{ClipboardRead, MsgSender, PreparedClipboardText};
use hotaru_core::Snapshot;

use crate::app::Msg;
use crate::terminal_view::Selection;

pub enum Job {
    Copy { token: u64, snapshot: Arc<Snapshot>, selection: Option<Selection> },
    Paste { token: u64, tab: u64, read: ClipboardRead },
}

#[derive(Default)]
struct Pending {
    job: Option<Job>,
    stop: bool,
}

pub struct ClipboardWorker {
    pending: Arc<(Mutex<Pending>, Condvar)>,
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
                    pending.job.take().expect("pending clipboard job")
                };
                let message = match job {
                    Job::Copy { token, snapshot, selection } => {
                        let text = selection.map_or_else(|| snapshot.plain_text(), |s| s.text(&snapshot));
                        Msg::CopyReady(token, PreparedClipboardText::new(text))
                    }
                    Job::Paste { token, tab, read } => {
                        let text = read.read(Duration::from_secs(2), 32 * 1024 * 1024 - 12);
                        Msg::PasteReady(token, tab, read, text)
                    }
                };
                if !tx.send(message) {
                    return;
                }
            }
        })?;
        Ok(Self { pending })
    }

    pub fn submit(&self, job: Job) {
        let (lock, wake) = &*self.pending;
        let old = lock.lock().unwrap_or_else(|e| e.into_inner()).job.replace(job);
        if let Some(Job::Paste { read, .. }) = old {
            read.cancel();
        }
        wake.notify_one();
    }
}

impl Drop for ClipboardWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.pending;
        let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
        pending.stop = true;
        if let Some(Job::Paste { read, .. }) = pending.job.take() {
            read.cancel();
        }
        wake.notify_one();
    }
}
