// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper's socket. systemd listens (`sekka-setup.socket`, root's,
//! group `sekka-setup`, mode 0660: only the login screen's account is in
//! the group) and starts the helper with it; the helper also asks the
//! kernel who connected (`SO_PEERCRED`) and answers only the login
//! screen's account. It exits once no connection has been open for a
//! while; systemd starts it again for the next.

use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::choices;
use crate::config::Config;
use crate::handoff::{self, Hints};
use crate::protocol::{self, Draft, Field, Failure, Interrupted, Progress, Request, Response, Status};
use crate::system::System;
use crate::transaction::{self, Run};

/// How long a connection may sit without a request: a person filling in
/// a page takes a while, and the login screen keeps its connection.
pub const IDLE_CONNECTION: Duration = Duration::from_secs(30 * 60);
/// How long the helper waits for another connection before it exits.
pub const IDLE_SERVER: Duration = Duration::from_secs(60);

/// The listening socket systemd passed (`LISTEN_FDS`, descriptor 3).
pub fn activated() -> Result<UnixListener> {
    let pid: u32 = std::env::var("LISTEN_PID").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let fds: u32 = std::env::var("LISTEN_FDS").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    if pid != std::process::id() || fds != 1 {
        bail!("the helper is started by sekka-setup.socket, with its socket");
    }
    // Safety: systemd hands over descriptor 3, a listening Unix socket,
    // for this process alone.
    Ok(unsafe { UnixListener::from_raw_fd(3) })
}

/// Who is at the other end of `stream`, as the kernel says.
pub fn peer_uid(stream: &UnixStream) -> Result<u32> {
    let mut cred = libc::ucred { pid: 0, uid: u32::MAX, gid: u32::MAX };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // Safety: getsockopt writes at most `len` bytes into `cred`.
    let r = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error()).context("asking who connected");
    }
    Ok(cred.uid)
}

/// Wait up to `wait` for a connection; false if none came.
fn readable(listener: &UnixListener, wait: Duration) -> Result<bool> {
    let mut pfd = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    loop {
        // Safety: one pollfd, owned here.
        let r = unsafe { libc::poll(&mut pfd, 1, wait.as_millis().min(i32::MAX as u128) as i32) };
        if r < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e.into());
        }
        return Ok(r > 0);
    }
}

/// Serve connections from the login screen's account (`allowed`), each on
/// a thread of its own with a system from `make` (the login screen keeps
/// its connection while the person fills in the pages, and anyone else is
/// told no at once), until none is open and none comes for `idle`.
/// Completions take turns on the state directory's lock.
pub fn serve<S: System + Send + 'static>(
    listener: &UnixListener,
    config: &Config,
    make: impl Fn() -> S,
    allowed: u32,
    idle: Duration,
) -> Result<()> {
    let open = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    loop {
        if !readable(listener, idle)? {
            if open.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                return Ok(());
            }
            continue;
        }
        let (stream, _) = listener.accept()?;
        let (config, mut sys, open) = (config.clone(), make(), open.clone());
        open.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::Builder::new().name("setup-connection".into()).spawn(move || {
            if let Err(e) = connection(stream, &config, &mut sys, allowed) {
                eprintln!("setup: a connection ended: {e:#}");
            }
            open.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        })?;
    }
}

/// One connection: its peer checked, then its requests in turn.
pub fn connection<S: System>(mut stream: UnixStream, config: &Config, sys: &mut S, allowed: u32) -> Result<()> {
    let uid = peer_uid(&stream)?;
    if uid != allowed {
        protocol::write(&mut stream, &Response::Error("Setup answers only the login screen.".into()))?;
        close_gently(&mut stream);
        bail!("refused a connection from user id {uid}");
    }
    stream.set_read_timeout(Some(IDLE_CONNECTION))?;
    let mut out = stream.try_clone()?;
    loop {
        let line = match protocol::read_line(&mut stream, protocol::MAX_REQUEST) {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(()),
            Err(e) => {
                let _ = protocol::write(&mut out, &Response::Error(format!("Setup couldn't read the request: {e}.")));
                close_gently(&mut stream);
                return Err(e.into());
            }
        };
        let request = protocol::parse(&line);
        drop(line);
        match request {
            Ok(r) => answer(r, config, sys, &mut out)?,
            Err(e) => {
                protocol::write(&mut out, &Response::Error(e.clone()))?;
                close_gently(&mut stream);
                bail!("{e}");
            }
        }
    }
}

/// End a connection whose peer may still be sending, so it reads the
/// answer rather than a reset: no more from here, and what it sent (up to
/// a request's worth, for a second at most) read and dropped.
fn close_gently(stream: &mut UnixStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let mut sink = [0u8; 4096];
    let mut left = protocol::MAX_REQUEST;
    while left > 0 {
        match std::io::Read::read(stream, &mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => left = left.saturating_sub(n),
        }
    }
    zeroize::Zeroize::zeroize(&mut sink[..]);
}

fn hints(config: &Config) -> Hints {
    handoff::read(&config.handoff, &config.zoneinfo)
}

/// Answer one request on `out`.
pub fn answer<S: System>(request: Request, config: &Config, sys: &mut S, out: &mut impl std::io::Write) -> Result<()> {
    let taken = |sys: &S, name: &str| sys.account(name).ok().flatten().is_some();
    match request {
        Request::Status(_) => protocol::write(out, &Response::Status(status(config, sys, &hints(config))?))?,
        Request::Check(protocol::CheckRequest { choices, password }) => {
            let pw = password.as_ref().map(|p| p.0.as_str());
            let h = hints(config);
            let checked = choices::check(config, &choices, pw, h.computer_name.as_deref(), &|n| taken(sys, n));
            protocol::write(out, &Response::Checked(checked))?;
        }
        Request::Complete(protocol::CompleteRequest { choices, password }) => {
            let plan = match choices::plan(config, &choices, &password.0, &|n| taken(sys, n)) {
                Ok(plan) => plan,
                Err(message) => return Ok(protocol::write(out, &Response::Failed(Failure::choices(message)))?),
            };
            drop(password);
            let result = {
                let mut progress = |s| {
                    let _ = protocol::write(out, &Response::Progress(Progress::of(s)));
                };
                let mut go = |_| Ok(());
                Run { config, sys, progress: &mut progress, stop: &mut go }.begin(plan)
            };
            finished(out, result)?;
        }
        Request::Retry(_) => {
            let result = {
                let mut progress = |s| {
                    let _ = protocol::write(out, &Response::Progress(Progress::of(s)));
                };
                let mut go = |_| Ok(());
                Run { config, sys, progress: &mut progress, stop: &mut go }.resume()
            };
            finished(out, result)?;
        }
    }
    Ok(())
}

fn finished(out: &mut impl std::io::Write, result: Result<protocol::Done, Failure>) -> Result<()> {
    protocol::write(
        out,
        &match result {
            Ok(done) => Response::Done(done),
            Err(f) => Response::Failed(f),
        },
    )?;
    Ok(())
}

/// Where setup stands, what it offers, and where it suggests starting.
pub fn status<S: System>(config: &Config, sys: &S, hints: &Hints) -> Result<Status> {
    let (state, staged) = transaction::state(config, sys)?;
    let offer = choices::offer(config);
    let taken = |n: &str| sys.account(n).ok().flatten().is_some();
    // The full name only if Genkan takes it.
    let full_name = hints
        .full_name
        .clone()
        .filter(|n| {
            let d = Draft { full_name: n.clone(), ..Draft::default() };
            !choices::check(config, &d, None, None, &taken).problems.contains_key(&Field::FullName)
        })
        .unwrap_or_default();
    let offered = |id: &str, list: &[protocol::Choice]| list.iter().any(|c| c.id == id);
    let pick = |hint: &Option<String>, current: &str, list: &[protocol::Choice]| {
        hint.clone()
            .filter(|h| offered(h, list))
            .or_else(|| offered(current, list).then(|| current.to_owned()))
            .unwrap_or_else(|| list.first().map(|c| c.id.clone()).unwrap_or_default())
    };
    let current_zone = sys.time_zone().unwrap_or_default();
    let time_zone = hints
        .time_zone
        .clone()
        .or_else(|| offer.time_zones.iter().any(|z| z.id == current_zone).then_some(current_zone))
        .unwrap_or_else(|| "UTC".into());
    let suggested = Draft {
        username: choices::suggest_username(&full_name, &taken),
        hostname: choices::suggest_hostname(hints.computer_name.as_deref(), &full_name, &config.device),
        full_name,
        locale: pick(&hints.locale, &config.locale, &offer.locales),
        keyboard: pick(&hints.keyboard, &config.keyboard, &offer.keyboards),
        time_zone,
        desktop: config.default_desktop.id().into(),
    };
    let interrupted = match &staged {
        Some(s) => Some(Interrupted {
            username: s.username.clone(),
            full_name: s.full_name.clone(),
            account_exists: transaction::account_made(sys, s)?,
            can_log_in: transaction::can_log_in(sys, s)?,
        }),
        None => None,
    };
    Ok(Status { version: protocol::VERSION, state, device: config.device.clone(), offer, suggested, interrupted })
}

/// The uid of `name`, from the system's accounts.
pub fn uid_of<S: System>(sys: &S, name: &str) -> Result<u32> {
    Ok(sys.account(name)?.with_context(|| format!("no account named {name}"))?.uid)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};

    use super::*;
    use crate::config::sample;
    use crate::transaction::tests::{Fake, PASSWORD, draft};

    fn talk(config: &Config, sys: &mut Fake, requests: &[String]) -> Vec<serde_json::Value> {
        let (mut client, server) = UnixStream::pair().unwrap();
        let me = unsafe { libc::getuid() };
        let mut answers = Vec::new();
        std::thread::scope(|s| {
            let h = s.spawn(|| connection(server, config, sys, me));
            for r in requests {
                client.write_all(r.as_bytes()).unwrap();
                client.write_all(b"\n").unwrap();
            }
            client.shutdown(std::net::Shutdown::Write).unwrap();
            // A server that stopped reading closes on requests it didn't
            // read, which can end the stream with a reset after its answer.
            for line in BufReader::new(&client).lines().map_while(Result::ok) {
                answers.push(serde_json::from_str(&line).unwrap());
            }
            let _ = h.join().unwrap();
        });
        answers
    }

    fn completion(password: &str) -> String {
        serde_json::json!({"op": "complete", "choices": draft(), "password": password}).to_string()
    }

    #[test]
    fn the_whole_exchange() {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        std::fs::create_dir_all(&config.nixos_dir).unwrap();
        let mut sys = Fake::image();
        let check = serde_json::json!({"op": "check", "choices": {"full_name": "Ada Lovelace", "username": "Ada"}, "password": "short"});
        let answers = talk(&config, &mut sys, &[r#"{"op":"status"}"#.into(), check.to_string(), completion("short"), completion(PASSWORD)]);
        let status = &answers[0]["status"];
        assert_eq!(status["state"], "needed");
        assert_eq!(status["version"], 1);
        assert_eq!(status["device"], "Mac");
        assert_eq!(status["suggested"]["desktop"], "aizome");
        assert_eq!(status["suggested"]["keyboard"], "us");
        assert_eq!(status["offer"]["desktops"][1]["label"], "Tatami");
        let checked = &answers[1]["checked"];
        assert!(checked["problems"]["username"].as_str().unwrap().starts_with("Username must"));
        assert!(checked["problems"]["password"].as_str().unwrap().starts_with("Password must have at least 12"));
        assert_eq!(checked["suggested_username"], "ada");
        assert_eq!(checked["suggested_hostname"], "adas-mac");
        // A short password is refused before anything is staged.
        assert!(answers[2]["failed"]["message"].as_str().unwrap().starts_with("Password must"));
        assert_eq!(answers[2]["failed"]["can_retry"], false);
        // Then progress, step by step, and done.
        let steps: Vec<&str> = answers[3..].iter().filter_map(|a| a["progress"]["step"].as_str()).collect();
        assert_eq!(steps, ["check", "account", "settings", "configuration", "verify", "finish"]);
        let done = &answers.last().unwrap()["done"];
        assert_eq!((done["username"].as_str(), done["uid"].as_u64()), (Some("ada"), Some(1000)));
        // Asked again: complete.
        let answers = talk(&config, &mut sys, &[r#"{"op":"status"}"#.into()]);
        assert_eq!(answers[0]["status"]["state"], "complete");
    }

    #[test]
    fn only_the_login_screen_is_answered() {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        let mut sys = Fake::image();
        let (mut client, server) = UnixStream::pair().unwrap();
        let me = unsafe { libc::getuid() };
        client.write_all(b"{\"op\":\"status\"}\n").unwrap();
        assert!(connection(server, &config, &mut sys, me.wrapping_add(1)).is_err());
        let mut answer = String::new();
        BufReader::new(&client).read_line(&mut answer).unwrap();
        assert_eq!(answer.trim(), r#"{"error":"Setup answers only the login screen."}"#);
        // Over a real socket too, accepted from the listener.
        let path = dir.path().join("setup.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let mut c = UnixStream::connect(&path).unwrap();
        let (s, _) = listener.accept().unwrap();
        assert_eq!(peer_uid(&s).unwrap(), me);
        c.write_all(b"{\"op\":\"status\"}\n").unwrap();
        assert!(connection(s, &config, &mut sys, me.wrapping_add(1)).is_err());
    }

    /// The login screen keeps its connection open while the person types:
    /// another is still answered meanwhile, and the server ends once both
    /// have gone and none comes.
    #[test]
    fn connections_are_served_side_by_side() {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        let path = dir.path().join("setup.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let me = unsafe { libc::getuid() };
        let server = std::thread::spawn({
            let config = config.clone();
            move || serve(&listener, &config, Fake::image, me, Duration::from_millis(300))
        });
        let held = UnixStream::connect(&path).unwrap();
        let mut asking = UnixStream::connect(&path).unwrap();
        asking.write_all(b"{\"op\":\"status\"}\n").unwrap();
        let mut answer = String::new();
        BufReader::new(&asking).read_line(&mut answer).unwrap();
        assert!(answer.starts_with("{\"status\":"), "{answer}");
        drop((held, asking));
        server.join().unwrap().unwrap();
    }

    #[test]
    fn malformed_and_oversized_requests_end_the_connection() {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        let mut sys = Fake::image();
        let answers = talk(&config, &mut sys, &[r#"{"op":"run","command":"/bin/sh"}"#.into(), r#"{"op":"status"}"#.into()]);
        assert_eq!(answers.len(), 1, "nothing after the bad request: {answers:?}");
        assert!(answers[0]["error"].as_str().unwrap().starts_with("not a request"));
        let huge = format!(r#"{{"op":"check","choices":{{"full_name":"{}"}}}}"#, "x".repeat(protocol::MAX_REQUEST));
        let answers = talk(&config, &mut sys, &[huge]);
        assert!(answers[0]["error"].as_str().unwrap().contains("longer than"), "{answers:?}");
        assert!(sys.calls.is_empty());
    }

    /// The login screen hears whether an interrupted setup's account exists,
    /// and apart from that, whether it can be logged in to yet.
    #[test]
    fn an_interrupted_setup_says_whether_its_account_can_log_in() {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        std::fs::create_dir_all(&config.nixos_dir).unwrap();
        let mut sys = Fake::image();
        let plan = choices::plan(&config, &draft(), &zeroize::Zeroizing::new(PASSWORD.into()), &|_| false).unwrap();
        let mut progress = |_| {};
        let mut stop = |p| if p == transaction::Point::Account { anyhow::bail!("stopped") } else { Ok(()) };
        assert!(Run { config: &config, sys: &mut sys, progress: &mut progress, stop: &mut stop }.begin(plan).is_err());
        let interrupted = |sys: &Fake| status(&config, sys, &Hints::default()).unwrap().interrupted.unwrap();
        let i = interrupted(&sys);
        assert_eq!((i.username.as_str(), i.account_exists, i.can_log_in), ("ada", true, false), "made, without its password");
        sys.shadow.insert("ada".into(), transaction::hash(PASSWORD).unwrap().to_string());
        assert!(interrupted(&sys).can_log_in);
    }

    #[test]
    fn hints_start_setup_where_macos_was() {
        let dir = tempfile::tempdir().unwrap();
        let config = sample(dir.path());
        let sys = Fake::image();
        let hints = Hints {
            full_name: Some("Chris Allen".into()),
            computer_name: Some("Chris's MacBook Pro".into()),
            locale: Some("de_DE.UTF-8".into()),
            keyboard: Some("de".into()),
            time_zone: Some("Europe/Berlin".into()),
        };
        let s = status(&config, &sys, &hints).unwrap().suggested;
        assert_eq!(
            s,
            Draft {
                full_name: "Chris Allen".into(),
                username: "chris".into(),
                hostname: "chriss-macbook-pro".into(),
                locale: "de_DE.UTF-8".into(),
                keyboard: "de".into(),
                time_zone: "Europe/Berlin".into(),
                desktop: "aizome".into(),
            }
        );
        // Without a handoff: the image's own settings.
        let s = status(&config, &sys, &Hints::default()).unwrap().suggested;
        assert_eq!((s.full_name.as_str(), s.username.as_str(), s.hostname.as_str()), ("", "", "sekka"));
        assert_eq!((s.locale.as_str(), s.keyboard.as_str(), s.time_zone.as_str()), ("en_US.UTF-8", "us", "UTC"));
    }
}
