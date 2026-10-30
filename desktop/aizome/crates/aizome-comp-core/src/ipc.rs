//! The IPC server: newline-delimited JSON on
//! `$XDG_RUNTIME_DIR/aizome.$WAYLAND_DISPLAY.sock`, one response per
//! request, and a stream of events after `subscribe`.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use aizome_ipc::{Event, Request, Response};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, Mode, PostAction};
use tracing::{debug, info, warn};

use crate::state::State;

/// A client's output beyond this is dropped with the client.
const MAX_BACKLOG: usize = 16 << 20;

struct Conn {
    stream: UnixStream,
    inbuf: Vec<u8>,
    outbuf: Vec<u8>,
    subscribed: bool,
}

#[derive(Default)]
pub struct IpcServer {
    pub path: Option<PathBuf>,
    conns: HashMap<u64, Conn>,
    next: u64,
    events_sent: u64,
}

impl IpcServer {
    pub fn new() -> IpcServer {
        IpcServer::default()
    }

    /// Whether connection `id` is still open.
    pub fn is_open(&self, id: u64) -> bool {
        self.conns.contains_key(&id)
    }

    /// Binds the socket and starts accepting connections.
    pub fn listen(state: &mut State) -> io::Result<PathBuf> {
        let display = state.az.socket_name.to_string_lossy().into_owned();
        let path = aizome_ipc::socket_path_for(&display);
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        state
            .az
            .lh
            .insert_source(Generic::new(listener, Interest::READ, Mode::Level), |_, listener, state: &mut State| {
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => state.ipc_accept(stream),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => {
                            warn!("IPC accept: {e}");
                            break;
                        }
                    }
                }
                Ok(PostAction::Continue)
            })
            .map_err(|e| io::Error::other(e.to_string()))?;
        info!("IPC on {}", path.display());
        state.az.ipc.path = Some(path.clone());
        Ok(path)
    }

    /// Connections listening for events (the shell's, among them).
    pub fn subscribers(&self) -> usize {
        self.conns.values().filter(|c| c.subscribed).count()
    }

    /// Sends an event to every subscriber.
    pub fn emit(&mut self, event: &Event) {
        if !self.conns.values().any(|c| c.subscribed) {
            return;
        }
        let mut line = match serde_json::to_string(event) {
            Ok(l) => l,
            Err(e) => {
                warn!("IPC event: {e}");
                return;
            }
        };
        line.push('\n');
        self.events_sent += 1;
        for c in self.conns.values_mut().filter(|c| c.subscribed) {
            c.outbuf.extend_from_slice(line.as_bytes());
        }
        self.flush();
    }

    /// Writes what the sockets take now; drops clients that fall too far
    /// behind or went away.
    pub fn flush(&mut self) {
        let mut dead = Vec::new();
        for (id, c) in self.conns.iter_mut() {
            if c.outbuf.is_empty() {
                continue;
            }
            match c.stream.write(&c.outbuf) {
                Ok(n) => {
                    c.outbuf.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => dead.push(*id),
            }
            if c.outbuf.len() > MAX_BACKLOG {
                dead.push(*id);
            }
        }
        for id in dead {
            if let Some(c) = self.conns.remove(&id) {
                let _ = c.stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p);
        }
    }
}

impl State {
    fn ipc_accept(&mut self, stream: UnixStream) {
        if stream.set_nonblocking(true).is_err() {
            return;
        }
        let Ok(reader) = stream.try_clone() else { return };
        let id = self.az.ipc.next;
        self.az.ipc.next += 1;
        self.az.ipc.conns.insert(id, Conn { stream, inbuf: Vec::new(), outbuf: Vec::new(), subscribed: false });
        let res = self.az.lh.insert_source(
            Generic::new(reader, Interest::READ, Mode::Level),
            move |_, stream, state: &mut State| {
                // Safety: we only read through this handle.
                let stream = unsafe { stream.get_mut() };
                Ok(state.ipc_readable(id, stream))
            },
        );
        if let Err(e) = res {
            warn!("IPC connection: {e}");
            self.az.ipc.conns.remove(&id);
        }
    }

    fn ipc_readable(&mut self, id: u64, stream: &mut UnixStream) -> PostAction {
        let mut buf = [0u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    self.az.ipc.conns.remove(&id);
                    return PostAction::Remove;
                }
                Ok(n) => {
                    let Some(c) = self.az.ipc.conns.get_mut(&id) else { return PostAction::Remove };
                    c.inbuf.extend_from_slice(&buf[..n]);
                    if c.inbuf.len() > MAX_BACKLOG {
                        self.az.ipc.conns.remove(&id);
                        return PostAction::Remove;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.az.ipc.conns.remove(&id);
                    return PostAction::Remove;
                }
            }
        }
        loop {
            let Some(c) = self.az.ipc.conns.get_mut(&id) else { return PostAction::Remove };
            let Some(nl) = c.inbuf.iter().position(|b| *b == b'\n') else { break };
            let line: Vec<u8> = c.inbuf.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]).trim().to_string();
            if line.is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<Request>(&line) {
                Ok(Request::Subscribe) => {
                    if let Some(c) = self.az.ipc.conns.get_mut(&id) {
                        c.subscribed = true;
                    }
                    Response::Ok
                }
                // The connection's, so the wait ends with it (Settei gone).
                Ok(Request::CatchKey { tag }) => {
                    self.az.keyboards.catch = tag.map(|t| (t, id));
                    Response::Ok
                }
                Ok(req) => {
                    debug!("IPC {req:?}");
                    self.handle_request(req)
                }
                Err(e) => Response::error(format!("bad request: {e}")),
            };
            let mut out = serde_json::to_string(&response)
                .unwrap_or_else(|e| format!("{{\"response\":\"error\",\"message\":\"{e}\"}}"));
            out.push('\n');
            if let Some(c) = self.az.ipc.conns.get_mut(&id) {
                c.outbuf.extend_from_slice(out.as_bytes());
            }
        }
        self.az.ipc.flush();
        if self.az.ipc.conns.contains_key(&id) { PostAction::Continue } else { PostAction::Remove }
    }
}
