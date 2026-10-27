//! The shell socket's server side (`$AIZOME_SHELL_SOCKET`): a thread
//! accepts connections, one thread per connection reads its lines, and one
//! writes to it, so a slow app never holds up the shell.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use aizome_shell_proto as proto;

use super::registry::ConnId;

/// What the server hears.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerEvent {
    Message(ConnId, proto::AppMessage),
    Gone(ConnId),
}

/// Sends to connections (from the shell's thread).
#[derive(Clone, Default)]
pub struct Server {
    writers: Arc<Mutex<HashMap<ConnId, Sender<String>>>>,
    path: Option<PathBuf>,
    /// Held by the handles alone: the last one removes the socket.
    handles: Arc<()>,
    /// The socket file this server made.
    inode: Option<u64>,
}

/// Where a server hands what connections say.
pub type Sink = Box<dyn Fn(ServerEvent) + Send + Sync + 'static>;

impl Server {
    /// Listen at `path` (removing a stale socket there), handing what
    /// connections say to `sink` on their threads.
    pub fn listen(path: &Path, sink: Sink) -> std::io::Result<Server> {
        if path.exists() {
            // A live shell answers; a dead one's socket is only a file.
            if UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("a shell already listens at {}", path.display()),
                ));
            }
        }
        Server::listen_replacing(path, sink)
    }

    /// Listens at `path` even while another shell does (a restart): new
    /// connections come here, and the old shell's apps reconnect here as
    /// it goes.
    pub fn listen_replacing(path: &Path, sink: Sink) -> std::io::Result<Server> {
        let _ = std::fs::remove_file(path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let listener = UnixListener::bind(path)?;
        let inode = std::fs::metadata(path).map(|m| std::os::unix::fs::MetadataExt::ino(&m)).ok();
        let server = Server { writers: Arc::default(), path: Some(path.to_owned()), handles: Arc::default(), inode };
        let writers = server.writers.clone();
        let sink = Arc::new(sink);
        std::thread::Builder::new().name("shell-socket".into()).spawn(move || {
            let mut next: ConnId = 1;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let conn = next;
                next += 1;
                serve(conn, stream, writers.clone(), sink.clone());
            }
        })?;
        Ok(server)
    }

    /// Send `msg` to connection `conn`, if it's still there.
    pub fn send(&self, conn: ConnId, msg: &proto::ShellMessage) {
        let line = proto::encode(msg);
        if let Some(tx) = self.writers.lock().unwrap_or_else(|e| e.into_inner()).get(&conn) {
            let _ = tx.send(line);
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // The last handle removes the socket, if it's still this shell's
        // (a restarted shell may listen there now).
        if Arc::strong_count(&self.handles) == 1
            && let Some(p) = &self.path
        {
            let ino = std::fs::metadata(p).map(|m| std::os::unix::fs::MetadataExt::ino(&m)).ok();
            if ino.is_some() && ino == self.inode {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

fn serve(
    conn: ConnId,
    stream: UnixStream,
    writers: Arc<Mutex<HashMap<ConnId, Sender<String>>>>,
    sink: Arc<dyn Fn(ServerEvent) + Send + Sync>,
) {
    let Ok(mut write) = stream.try_clone() else { return };
    let (tx, rx) = channel::<String>();
    writers.lock().unwrap_or_else(|e| e.into_inner()).insert(conn, tx);
    let _ = std::thread::Builder::new().name(format!("shell-out-{conn}")).spawn(move || {
        for line in rx {
            if write.write_all(line.as_bytes()).and_then(|_| write.flush()).is_err() {
                break;
            }
        }
    });
    let _ = std::thread::Builder::new().name(format!("shell-in-{conn}")).spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            match proto::decode::<proto::AppMessage>(&line) {
                Ok(m) => sink(ServerEvent::Message(conn, m)),
                Err(e) => log::debug!("connection {conn}: {e}: {line}"),
            }
        }
        writers.lock().unwrap_or_else(|e| e.into_inner()).remove(&conn);
        sink(ServerEvent::Gone(conn));
    });
}

/// Where the shell listens: `$AIZOME_SHELL_SOCKET`, else
/// `$XDG_RUNTIME_DIR/aizome-shell.$WAYLAND_DISPLAY.sock`.
pub fn socket_path() -> PathBuf {
    proto::socket_path().unwrap_or_else(|| {
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        aizome_ipc::runtime_dir().join(format!("aizome-shell.{display}.sock"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_talk_to_the_server_and_hear_back() {
        let dir = std::env::temp_dir().join(format!("aizome-shell-server-{}", std::process::id()));
        let path = dir.join("shell.sock");
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let server = Server::listen(
            &path,
            Box::new(move |e| {
                let _ = tx.lock().unwrap().send(e);
            }),
        )
        .unwrap();
        let mut app = proto::Client::hello(&path, "org.aizome.Test", "Test").unwrap();
        let hello = rx.recv().unwrap();
        let ServerEvent::Message(conn, proto::AppMessage::Hello { pid, .. }) = hello else { panic!("{hello:?}") };
        assert_eq!(pid, std::process::id());
        server.send(conn, &proto::ShellMessage::Activate { id: 4 });
        assert_eq!(app.recv().unwrap(), Some(proto::ShellMessage::Activate { id: 4 }));
        // A second shell can't take the socket while this one listens.
        assert!(Server::listen(&path, Box::new(|_| {})).is_err());
        drop(app);
        // (The probe above was a connection too, gone at once.)
        while rx.recv().unwrap() != ServerEvent::Gone(conn) {}
        drop(server);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
