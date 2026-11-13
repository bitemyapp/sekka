//! A private, short-lived sender. All socket and process I/O stays on this
//! worker. No PIN is put in argv, the environment, or an application log.
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESPONSE: u64 = 256 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Device {
    pub name: String,
    pub model: String,
    pub ip: String,
    pub port: u16,
    pub device_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Status {
    pub ok: bool,
    pub state: String,
    pub device: String,
    pub device_ip: String,
    pub has_audio: bool,
    pub audio_muted: bool,
    pub credential_kind: String,
    pub error: String,
    pub devices: Vec<Device>,
}

#[derive(Serialize)]
pub struct Request {
    pub cmd: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pin: String,
}

impl Request {
    pub fn new(cmd: &'static str) -> Self {
        Self { cmd, target: String::new(), port: None, pin: String::new() }
    }

    pub fn connect(device: &Device, pin: String) -> Self {
        Self { cmd: "connect", target: device.ip.clone(), port: Some(device.port), pin }
    }
}

pub enum Action {
    Request(Request),
    Shutdown,
}

#[derive(Clone)]
pub enum Event {
    Ready,
    Snapshot(Status, Vec<Device>),
    Error(String),
    Stopped,
}

/// The handle owns its worker. Dropping it cannot leave a background share.
pub struct Backend {
    tx: Sender<Action>,
    worker: Option<JoinHandle<()>>,
}

impl Backend {
    pub fn start(send: impl Fn(Event) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            if let Err(e) = run(rx, &send) {
                send(Event::Error(e));
            }
            send(Event::Stopped);
        });
        Self { tx, worker: Some(worker) }
    }

    pub fn request(&self, request: Request) {
        let _ = self.tx.send(Action::Request(request));
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Action::Shutdown);
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(rx: Receiver<Action>, send: &dyn Fn(Event)) -> Result<(), String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .ok_or("The desktop session has no runtime directory. Sign out and sign in again.")?;
    let private = tempfile::Builder::new()
        .prefix("sekka-airplay-")
        .tempdir_in(runtime)
        .map_err(|e| format!("Could not create a private AirPlay session: {e}"))?;
    let socket = private.path().join("sender.sock");
    let sender = std::env::var_os("SEKKA_AIRPLAY_SENDER").unwrap_or_else(|| "doubletake".into());
    let mut command = Command::new(sender);
    command
        .args(["-daemonize", "-socket"])
        .arg(&socket)
        .arg("-creds")
        .arg(private.path().join("pairing.json"))
        .args(["-port-range", "60000-60002", "-fps", "30", "-video-codec", "h264"])
        .env_remove("DOUBLETAKE_CODE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .process_group(0);
    if let Ok(profile) = std::env::var("SEKKA_AIRPLAY_TEST_MEDIA_PROFILE") {
        command.args(crate::battery::media_args(&profile)?);
    }
    let parent = std::process::id();
    // SAFETY: only async-signal-safe syscalls, no allocation or locks between
    // fork and exec. Recheck the parent to cover its death before prctl.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent as libc::pid_t {
                libc::_exit(1);
            }
            Ok(())
        });
    }
    let child = command.spawn().map_err(|e| format!("Could not start the AirPlay sender: {e}"))?;
    // Declared after the private directory: stop/reap the process first, then
    // remove its socket and temporary pairing keys.
    let mut process = Process(child);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !socket.exists() {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Action::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            _ => {}
        }
        process.check()?;
        if Instant::now() >= deadline {
            return Err("The AirPlay sender did not start in time. Close this window and try again.".into());
        }
    }
    send(Event::Ready);
    let mut last = None;
    let mut poll_failures = 0;
    loop {
        match rx.recv_timeout(Duration::from_millis(700)) {
            Ok(Action::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Action::Request(request)) => {
                match exchange(&socket, &request) {
                    Ok(response) if !response.ok => send(Event::Error(response.error)),
                    Err(error) => {
                        // A lost reply does not tell us whether the command
                        // ran. Reconcile status; never replay pairing/capture.
                        send(Event::Error(error));
                    }
                    _ => {}
                }
                // Reconcile optimistic UI state even when a rejected command
                // leaves the daemon's snapshot identical to the previous one.
                last = None;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        process.check()?;
        let snapshot = exchange(&socket, &Request::new("status"))
            .and_then(|status| exchange(&socket, &Request::new("devices")).map(|devices| (status, devices.devices)));
        let snapshot = match snapshot {
            Ok(value) => {
                poll_failures = 0;
                value
            }
            Err(error) => {
                poll_failures += 1;
                if poll_failures == 1 {
                    eprintln!("AirPlay status polling: {error}");
                }
                if poll_failures >= 5 {
                    return Err(error);
                }
                continue;
            }
        };
        if last.as_ref() != Some(&snapshot) {
            send(Event::Snapshot(snapshot.0.clone(), snapshot.1.clone()));
            last = Some(snapshot);
        }
    }
    // A bounded, best-effort TEARDOWN, followed by process-group cleanup.
    let _ = exchange(&socket, &Request::new("disconnect"));
    Ok(())
}

struct Process(Child);

impl Process {
    fn check(&mut self) -> Result<(), String> {
        match self.0.try_wait().map_err(|e| e.to_string())? {
            None => Ok(()),
            Some(status) => Err(format!("The AirPlay sender exited ({status}). Close this window and try again.")),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let group = -(self.0.id() as libc::pid_t);
        // SAFETY: the child created a new group; never signal our own group.
        unsafe {
            libc::kill(group, libc::SIGTERM);
        }
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(30));
        }
        // Reap any encoder children left after an abnormal backend exit too.
        unsafe {
            libc::kill(group, libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

pub fn exchange(socket: &Path, request: &Request) -> Result<Status, String> {
    let mut stream = UnixStream::connect(socket).map_err(|e| format!("AirPlay connection lost: {e}"))?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(|e| e.to_string())?;
    // The daemon answers as soon as it decodes the JSON object, then closes
    // its socket. Writing the newline separately races that close (EPIPE),
    // even though the command succeeded. Send one complete request buffer.
    let mut message = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    message.push(b'\n');
    stream.write_all(&message).map_err(|e| e.to_string())?;
    let mut line = Vec::new();
    BufReader::new(stream.take(MAX_RESPONSE + 1))
        .read_until(b'\n', &mut line)
        .map_err(|e| format!("The AirPlay sender did not answer: {e}"))?;
    if line.len() as u64 > MAX_RESPONSE || line.last() != Some(&b'\n') {
        return Err("The AirPlay sender returned an incomplete or oversized response.".into());
    }
    serde_json::from_slice(&line).map_err(|e| format!("Invalid AirPlay sender response: {e}"))
}

/// A manual address never becomes shell syntax or an arbitrary URL. The
/// backend's IPv4 support is the interoperable direct-connect path today.
pub fn manual_device(value: &str) -> Result<Device, String> {
    let value = value.trim();
    let (host, port) = value.split_once(':').unwrap_or((value, "7000"));
    let ip: std::net::Ipv4Addr =
        host.parse().map_err(|_| "Enter the TV's IPv4 address, optionally followed by :port.")?;
    let port: u16 = port.parse().ok().filter(|p| *p != 0).ok_or("The port must be between 1 and 65535.")?;
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        return Err("Enter one TV's address.".into());
    }
    Ok(Device { name: ip.to_string(), ip: ip.to_string(), port, ..Device::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn sender_can_answer_and_close_as_soon_as_json_is_complete() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("socket");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            for _ in 0..128 {
                let (mut stream, _) = listener.accept().unwrap();
                // Like Go's JSON decoder: it does not wait for a newline.
                let request = serde_json::Deserializer::from_reader(&stream)
                    .into_iter::<serde_json::Value>()
                    .next()
                    .unwrap()
                    .unwrap();
                assert_eq!(request["cmd"], "status");
                stream.write_all(b"{\"ok\":true,\"state\":\"idle\"}\n").unwrap();
            }
        });
        for _ in 0..128 {
            assert_eq!(exchange(&path, &Request::new("status")).unwrap().state, "idle");
        }
        server.join().unwrap();
    }

    #[test]
    fn explicit_target_and_port_survive_pairing_without_argv() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("socket");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(s.try_clone().unwrap()).read_line(&mut request).unwrap();
            let v: serde_json::Value = serde_json::from_str(&request).unwrap();
            assert_eq!(v["target"], "192.0.2.4");
            assert_eq!(v["port"], 7100);
            assert_eq!(v["pin"], "0042");
            s.write_all(b"{\"ok\":true,\"state\":\"connecting\"}\n").unwrap();
        });
        let device = manual_device("192.0.2.4:7100").unwrap();
        assert_eq!(exchange(&path, &Request::connect(&device, "0042".into())).unwrap().state, "connecting");
        server.join().unwrap();
    }

    #[test]
    fn rejects_truncated_or_oversized_backend_messages() {
        for bytes in [b"{}".to_vec(), vec![b'x'; MAX_RESPONSE as usize + 1]] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("socket");
            let listener = UnixListener::bind(&path).unwrap();
            let server = thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(s.try_clone().unwrap()).read_line(&mut request).unwrap();
                let _ = s.write_all(&bytes);
            });
            assert!(exchange(&path, &Request::new("status")).is_err());
            server.join().unwrap();
        }
    }

    #[test]
    fn manual_addresses_are_single_destinations() {
        assert_eq!(manual_device(" 192.0.2.4 ").unwrap().port, 7000);
        for bad in [
            "",
            "-target",
            "http://192.0.2.4",
            "192.0.2.4;true",
            "224.0.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "192.0.2.4:0",
            "192.0.2.4:65536",
        ] {
            assert!(manual_device(bad).is_err(), "{bad}");
        }
    }
}
