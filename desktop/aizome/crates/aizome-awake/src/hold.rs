//! Keep Awake's hold on the compositor's idleness.
//!
//! Aizome is asked over its IPC (`keep-awake`, on a connection kept open).
//! That request is how the compositor tells the user's own switch from an
//! app's idle inhibitor: an app's stops counting once the session is
//! locked, so a locked screen goes dark on its timer whatever plays behind
//! it, and Keep Awake's goes on holding. Only the session's own programs
//! reach that socket; nothing is guessed from a process's name.
//!
//! A compositor that doesn't answer the request gets the Wayland idle
//! inhibitor Keep Awake held before ([`IdleInhibitor`]): an Aizome from
//! before the request, still running after an update, or another desktop.
//! Idleness is held off there all the same.

use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

use aizome_ipc::{Client, Request, Response};

use crate::IdleInhibitor;

/// How long the compositor has to answer `keep-awake`.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// Idleness held off as Keep Awake, until it's dropped. Either way it has
/// a connection of its own to the compositor, so it goes with its process
/// however that ends.
pub enum KeepAwakeHold {
    /// Asked for over Aizome's IPC: the connection is the hold.
    Asked(Client),
    /// A Wayland idle inhibitor, where `keep-awake` isn't known.
    Inhibitor(IdleInhibitor),
}

impl KeepAwakeHold {
    /// Takes the hold from the session's compositor (`AIZOME_SOCKET`, or
    /// the one for `WAYLAND_DISPLAY`); it's held once this returns.
    pub fn take() -> Result<KeepAwakeHold, String> {
        match aizome_ipc::socket_path().and_then(|path| KeepAwakeHold::ask(&path)) {
            Some(hold) => Ok(hold),
            None => IdleInhibitor::new().map(KeepAwakeHold::Inhibitor),
        }
    }

    /// Asks the compositor listening at `socket`; `None` when nothing
    /// there takes the request.
    pub fn ask(socket: &Path) -> Option<KeepAwakeHold> {
        let mut client = Client::connect_to(socket).ok()?;
        client.stream().set_read_timeout(Some(ANSWER_WITHIN)).ok()?;
        let answer = client.request(&Request::KeepAwake).ok()?;
        client.stream().set_read_timeout(None).ok()?;
        matches!(answer, Response::Ok).then_some(KeepAwakeHold::Asked(client))
    }

    /// Whether the compositor was told this is Keep Awake's (so it holds
    /// while the session is locked too).
    pub fn asked(&self) -> bool {
        matches!(self, KeepAwakeHold::Asked(_))
    }

    /// Waits up to `timeout` for the compositor and reads what it sent;
    /// an error once it's gone.
    pub fn wait(&mut self, timeout: Duration) -> Result<(), String> {
        let client = match self {
            KeepAwakeHold::Inhibitor(inhibitor) => return inhibitor.wait(timeout),
            KeepAwakeHold::Asked(client) => client,
        };
        let mut stream = client.stream();
        let mut pfd = libc::pollfd { fd: stream.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // Safety: one valid pollfd, for the duration of the call.
        let ready = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int) };
        if ready <= 0 {
            return Ok(());
        }
        // Nothing follows the request's answer on this connection, so
        // something to read is its end.
        match stream.read(&mut [0u8; 64]) {
            Ok(0) => Err("the compositor went away".into()),
            Ok(_) => Ok(()),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => Ok(()),
            Err(e) => Err(format!("the compositor went away: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    /// A compositor's IPC socket that answers one request with `answer`,
    /// and hands back what it was asked and the connection.
    fn compositor(answer: &'static str) -> (PathBuf, mpsc::Receiver<(String, UnixStream)>) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aizome-awake-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ipc.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut asked).unwrap();
            stream.write_all(answer.as_bytes()).unwrap();
            let _ = tx.send((asked, stream));
        });
        (path, rx)
    }

    #[test]
    fn the_hold_is_asked_for_by_name_and_lasts_as_long_as_its_connection() {
        let (socket, asked) = compositor("{\"response\":\"ok\"}\n");
        let mut hold = KeepAwakeHold::ask(&socket).expect("the compositor's hold");
        assert!(hold.asked());
        let (request, connection) = asked.recv().unwrap();
        assert_eq!(request.trim_end(), r#"{"request":"keep-awake"}"#);
        // The compositor is still there: nothing to say.
        assert_eq!(hold.wait(Duration::from_millis(50)), Ok(()));
        // It goes: the service hears, and ends so systemd starts it again.
        drop(connection);
        assert!(hold.wait(Duration::from_secs(5)).is_err(), "a hold outlived its compositor");
        std::fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_compositor_that_does_not_know_the_request_is_not_asked_twice() {
        // As an Aizome from before `keep-awake` answers anything it can't
        // read: Keep Awake holds a Wayland idle inhibitor there instead.
        let (socket, asked) =
            compositor("{\"response\":\"error\",\"message\":\"bad request: unknown variant `keep-awake`\"}\n");
        assert!(KeepAwakeHold::ask(&socket).is_none());
        assert_eq!(asked.recv().unwrap().0.trim_end(), r#"{"request":"keep-awake"}"#);
        std::fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    }

    #[test]
    fn no_compositor_at_the_socket_is_no_hold() {
        assert!(KeepAwakeHold::ask(Path::new("/nonexistent/aizome.sock")).is_none());
    }
}
