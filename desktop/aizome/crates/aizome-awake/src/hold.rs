//! Keep Awake's hold on the compositor's idleness.
//!
//! Aizome is asked over its IPC (`keep-awake`, on a connection kept open).
//! That request is how the compositor tells the user's own switch from an
//! app's idle inhibitor: an app's stops counting once the session is
//! locked, so a locked screen goes dark on its timer whatever plays behind
//! it, and Keep Awake's goes on holding. Only the session's own programs
//! reach that socket; nothing is guessed from a process's name.
//!
//! Where the request can't be asked at all, the compositor gets the
//! Wayland idle inhibitor Keep Awake held before ([`IdleInhibitor`]): an
//! Aizome from before the request, still running after an update, which
//! answers that it doesn't know it, or another desktop, which has no such
//! socket. Idleness is held off there all the same. Only there: an Aizome
//! that takes the connection and then answers late, or not at all, is an
//! error, so the service ends and systemd starts it again. Held as an
//! app's instead, the hold would stop counting behind a lock while the
//! menu bar went on saying the screen stays on.

use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

use aizome_ipc::{Client, Request, Response};

use crate::IdleInhibitor;

/// How long the compositor has to answer `keep-awake`.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// Whether an error the compositor answered with means it doesn't know
/// the request: what an Aizome from before `keep-awake` says of anything
/// it can't read (``bad request: unknown variant `keep-awake`, expected
/// one of …``).
fn says_unknown_request(message: &str) -> bool {
    message.starts_with("bad request:") && message.contains("unknown variant")
}

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
    /// the one for `WAYLAND_DISPLAY`); it's held once this returns. An
    /// error when the compositor has the request and doesn't grant it in
    /// time.
    pub fn take() -> Result<KeepAwakeHold, String> {
        let asked = match aizome_ipc::socket_path() {
            Some(socket) => KeepAwakeHold::ask(&socket, ANSWER_WITHIN)?,
            None => None,
        };
        match asked {
            Some(hold) => Ok(hold),
            None => IdleInhibitor::new().map(KeepAwakeHold::Inhibitor),
        }
    }

    /// Asks the compositor listening at `socket`, which has `within` to
    /// answer. `Ok(None)` where `keep-awake` can't be asked: nothing
    /// listens there (no Aizome), or what does answers that it doesn't
    /// know the request (an Aizome from before it). Anything else that
    /// isn't the hold is an error: no answer in time, the connection
    /// dropped, the request refused.
    pub fn ask(socket: &Path, within: Duration) -> Result<Option<KeepAwakeHold>, String> {
        let mut client = match Client::connect_to(socket) {
            Ok(client) => client,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => return Ok(None),
            Err(e) => return Err(format!("the compositor's socket {}: {e}", socket.display())),
        };
        let socket_error = |e: std::io::Error| format!("the compositor's socket: {e}");
        client.stream().set_read_timeout(Some(within)).map_err(socket_error)?;
        let answer = client.request(&Request::KeepAwake).map_err(|e| match e.kind() {
            ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                format!("the compositor didn't answer keep-awake within {} ms", within.as_millis())
            }
            _ => format!("asking the compositor for keep-awake: {e}"),
        })?;
        client.stream().set_read_timeout(None).map_err(socket_error)?;
        match answer {
            Response::Ok => Ok(Some(KeepAwakeHold::Asked(client))),
            Response::Error { message } if says_unknown_request(&message) => Ok(None),
            Response::Error { message } => Err(format!("the compositor refused keep-awake: {message}")),
            other => Err(format!("the compositor answered keep-awake with {other:?}")),
        }
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

    /// What a stand-in compositor does with the one request it's sent.
    enum Does {
        Answers(&'static str),
        /// Takes the request and says nothing, as a compositor whose main
        /// loop is stalled.
        SaysNothing,
        /// Takes the request and closes the connection.
        HangsUp,
    }

    /// Somewhere for a socket, short whatever `TMPDIR` is (a socket's
    /// path has 108 bytes), as the compositor's tests make theirs.
    fn socket_dir() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = PathBuf::from(format!(
            "/tmp/aizome-awake-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A compositor's IPC socket, and what it was asked with the
    /// connection it kept (none once it hung up).
    fn compositor(does: Does) -> (PathBuf, mpsc::Receiver<(String, Option<UnixStream>)>) {
        let path = socket_dir().join("ipc.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut asked).unwrap();
            let kept = match does {
                Does::Answers(answer) => {
                    stream.write_all(answer.as_bytes()).unwrap();
                    Some(stream)
                }
                Does::SaysNothing => Some(stream),
                Does::HangsUp => None,
            };
            let _ = tx.send((asked, kept));
        });
        (path, rx)
    }

    fn done(socket: &Path) {
        std::fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    }

    /// What asking came to, when it wasn't the hold.
    fn refused(asked: Result<Option<KeepAwakeHold>, String>) -> Result<(), String> {
        match asked {
            Ok(Some(_)) => panic!("a hold was given"),
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        }
    }

    const SOON: Duration = Duration::from_secs(5);

    #[test]
    fn the_hold_is_asked_for_by_name_and_lasts_as_long_as_its_connection() {
        let (socket, asked) = compositor(Does::Answers("{\"response\":\"ok\"}\n"));
        let mut hold = KeepAwakeHold::ask(&socket, SOON).unwrap().expect("the compositor's hold");
        assert!(hold.asked());
        let (request, connection) = asked.recv().unwrap();
        assert_eq!(request.trim_end(), r#"{"request":"keep-awake"}"#);
        // The compositor is still there: nothing to say.
        assert_eq!(hold.wait(Duration::from_millis(50)), Ok(()));
        // It goes: the service hears, and ends so systemd starts it again.
        drop(connection);
        assert!(hold.wait(Duration::from_secs(5)).is_err(), "a hold outlived its compositor");
        done(&socket);
    }

    #[test]
    fn only_a_compositor_that_cannot_be_asked_gets_the_inhibitor_instead() {
        // An Aizome from before `keep-awake` says so, as it says of
        // anything it can't read: no hold and no error, and Keep Awake
        // takes a Wayland idle inhibitor there.
        let (socket, asked) = compositor(Does::Answers(
            "{\"response\":\"error\",\"message\":\"bad request: unknown variant `keep-awake`, expected one of `version`, `outputs`\"}\n",
        ));
        assert_eq!(refused(KeepAwakeHold::ask(&socket, SOON)), Ok(()));
        assert_eq!(asked.recv().unwrap().0.trim_end(), r#"{"request":"keep-awake"}"#);
        done(&socket);
        // Another desktop: no such socket, or one nobody listens at.
        assert_eq!(refused(KeepAwakeHold::ask(Path::new("/nonexistent/aizome.sock"), SOON)), Ok(()));
        let left = socket_dir().join("ipc.sock");
        drop(UnixListener::bind(&left).unwrap());
        assert_eq!(refused(KeepAwakeHold::ask(&left, SOON)), Ok(()));
        done(&left);
    }

    #[test]
    fn a_compositor_that_answers_late_is_an_error_and_never_an_apps_hold() {
        // Its main loop is busy: the request is taken and nothing comes
        // back in time. Not "unknown": the service must fail and be
        // started again, not hold as an app would.
        let (socket, asked) = compositor(Does::SaysNothing);
        let late = refused(KeepAwakeHold::ask(&socket, Duration::from_millis(200)));
        assert_eq!(late, Err("the compositor didn't answer keep-awake within 200 ms".to_owned()));
        // The compositor did have the request, and still has the
        // connection: its end is what lets go of a hold granted late.
        let (request, connection) = asked.recv().unwrap();
        assert_eq!(request.trim_end(), r#"{"request":"keep-awake"}"#);
        assert!(connection.is_some());
        done(&socket);
    }

    #[test]
    fn a_dropped_connection_or_another_answer_is_an_error_too() {
        let (socket, asked) = compositor(Does::HangsUp);
        let dropped = refused(KeepAwakeHold::ask(&socket, SOON)).unwrap_err();
        assert!(dropped.starts_with("asking the compositor for keep-awake:"), "{dropped}");
        assert!(asked.recv().unwrap().1.is_none());
        done(&socket);
        // An error that isn't "I don't know that request".
        let (socket, _asked) =
            compositor(Does::Answers("{\"response\":\"error\",\"message\":\"the session is ending\"}\n"));
        assert_eq!(
            refused(KeepAwakeHold::ask(&socket, SOON)),
            Err("the compositor refused keep-awake: the session is ending".to_owned())
        );
        done(&socket);
        // And an answer that is neither.
        let (socket, _asked) = compositor(Does::Answers("{\"response\":\"idle-inhibitors\",\"inhibitors\":[]}\n"));
        let odd = refused(KeepAwakeHold::ask(&socket, SOON)).unwrap_err();
        assert!(odd.starts_with("the compositor answered keep-awake with"), "{odd}");
        done(&socket);
    }

    #[test]
    fn only_an_unreadable_request_means_the_request_is_unknown() {
        assert!(says_unknown_request("bad request: unknown variant `keep-awake`, expected one of `version`"));
        assert!(!says_unknown_request("the connection that holds as Keep Awake asks"));
        assert!(!says_unknown_request("bad request: missing field `id`"));
        assert!(!says_unknown_request(""));
    }
}
