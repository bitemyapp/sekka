//! The compositor, over its IPC (`$AIZOME_SOCKET`): one thread follows its
//! events (reconnecting when it restarts), another sends requests, so the
//! shell's loop never waits on a socket.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use aizome_ipc::{Client, Event, Request, Response};

use crate::wm::Snapshot;

/// What the IPC threads hear.
#[derive(Clone, Debug, PartialEq)]
pub enum IpcEvent {
    /// Connected (again): the state to start from.
    Connected(Box<SnapshotBox>),
    Event(Event),
    /// The compositor went away (its events will come again when it's back).
    Disconnected,
    /// The answer to a request sent with a tag.
    Reply {
        tag: u64,
        response: Response,
    },
}

/// [`Snapshot`] in a box: it's large beside the other events.
#[derive(Clone, Debug, Default)]
pub struct SnapshotBox(pub Snapshot);

impl PartialEq for SnapshotBox {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

/// Sends requests to the compositor.
#[derive(Clone)]
pub struct Ipc {
    tx: Option<Sender<(Request, Option<u64>)>>,
}

impl Ipc {
    /// An IPC that goes nowhere (no compositor: tests, other desktops).
    pub fn none() -> Ipc {
        Ipc { tx: None }
    }

    #[cfg(test)]
    pub(crate) fn recording() -> (Ipc, std::sync::mpsc::Receiver<(Request, Option<u64>)>) {
        let (tx, rx) = channel();
        (Ipc { tx: Some(tx) }, rx)
    }

    /// Follow the compositor at `path` (`$AIZOME_SOCKET` when `None`),
    /// handing what it says to `sink` on the IPC threads.
    pub fn spawn(path: Option<PathBuf>, sink: impl Fn(IpcEvent) + Send + Sync + 'static) -> Ipc {
        let Some(path) = path.or_else(aizome_ipc::socket_path) else {
            log::warn!("no compositor IPC socket (AIZOME_SOCKET): windows and Desks won't show");
            return Ipc::none();
        };
        let sink: Arc<dyn Fn(IpcEvent) + Send + Sync> = Arc::new(sink);
        {
            let (path, sink) = (path.clone(), sink.clone());
            let _ = std::thread::Builder::new().name("ipc-events".into()).spawn(move || follow(path, sink));
        }
        let (tx, rx) = channel::<(Request, Option<u64>)>();
        let _ = std::thread::Builder::new().name("ipc-requests".into()).spawn(move || {
            let mut client: Option<Client> = None;
            for (req, tag) in rx {
                let mut answer = None;
                for _attempt in 0..2 {
                    if client.is_none() {
                        client = Client::connect_to(&path).ok();
                    }
                    let Some(c) = client.as_mut() else { break };
                    match c.request(&req) {
                        Ok(r) => {
                            answer = Some(r);
                            break;
                        }
                        Err(e) => {
                            log::debug!("IPC request {req:?}: {e}");
                            client = None;
                        }
                    }
                }
                match (answer, tag) {
                    (Some(Response::Error { message }), None) => log::warn!("the compositor: {message} ({req:?})"),
                    (Some(response), Some(tag)) => sink(IpcEvent::Reply { tag, response }),
                    (None, Some(tag)) => sink(IpcEvent::Reply { tag, response: Response::error("no compositor") }),
                    _ => {}
                }
            }
        });
        Ipc { tx: Some(tx) }
    }

    /// Ask the compositor to do something, without waiting.
    pub fn send(&self, req: Request) {
        if let Some(tx) = &self.tx {
            let _ = tx.send((req, None));
        } else {
            log::debug!("no compositor for {req:?}");
        }
    }

    /// Ask, and hear the answer as [`IpcEvent::Reply`] with `tag`.
    pub fn ask(&self, req: Request, tag: u64) {
        if let Some(tx) = &self.tx {
            let _ = tx.send((req, Some(tag)));
        }
    }

    pub fn is_connected(&self) -> bool {
        self.tx.is_some()
    }
}

fn follow(path: PathBuf, sink: Arc<dyn Fn(IpcEvent) + Send + Sync>) {
    let mut wait = Duration::from_millis(100);
    let mut was_connected = false;
    loop {
        match connect(&path) {
            Ok((snapshot, events)) => {
                wait = Duration::from_millis(100);
                was_connected = true;
                sink(IpcEvent::Connected(Box::new(SnapshotBox(snapshot))));
                for e in events {
                    match e {
                        Ok(e) => sink(IpcEvent::Event(e)),
                        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => log::debug!("IPC event: {e}"),
                        Err(_) => break,
                    }
                }
                sink(IpcEvent::Disconnected);
            }
            Err(e) => {
                if !was_connected && wait >= Duration::from_secs(2) {
                    log::debug!("the compositor at {}: {e}", path.display());
                }
            }
        }
        std::thread::sleep(wait);
        wait = (wait * 2).min(Duration::from_secs(3));
    }
}

fn connect(path: &std::path::Path) -> std::io::Result<(Snapshot, aizome_ipc::Events)> {
    let mut c = Client::connect_to(path)?;
    let mut s = Snapshot::default();
    if let Response::Outputs { outputs } = c.call(&Request::Outputs)? {
        s.outputs = outputs;
    }
    if let Response::Desks { desks, current } = c.call(&Request::Desks)? {
        s.desks = desks;
        s.current_desk = current;
    }
    if let Response::Windows { windows } = c.call(&Request::Windows)? {
        s.windows = windows;
    }
    if let Response::Window { window } = c.call(&Request::FocusedWindow)? {
        s.focused = window.map(|w| w.id);
    }
    let events = c.events()?;
    Ok((s, events))
}
