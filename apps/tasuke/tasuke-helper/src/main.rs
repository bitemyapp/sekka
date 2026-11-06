// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tasuke-helper run`: runs one command the person approved in Tasuke as
//! root. Tasuke starts it through pkexec, under the polkit action
//! `io.github.bitemyapp.Tasuke.run-as-root`, or directly when Tasuke is root
//! already (the evaluation VM).
//!
//! It reads one JSON request on standard input,
//! `{"command": "...", "purpose": "...", "timeout_seconds": 60}`, and prints
//! `{"exit_code": 0, "output": "...", "truncated": false, "timed_out": false}`
//! (see `tasuke_agent::root`). It:
//!
//! - runs only when its real user is root;
//! - refuses what Tasuke never runs (`tasuke_agent::policy::refused`), as a
//!   second line of defence, and says why in `refused`;
//! - runs `bash --noprofile --norc -c COMMAND` with a clean environment, no
//!   input, from `/`, in its own process group;
//! - kills the group at the timeout (at most 300 s), and anything the
//!   command left running in it once it ends;
//! - keeps the first and last 16 KiB of the output;
//! - logs who ran what, why, and how it ended, to the journal.

use std::ffi::CString;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tasuke_agent::policy;
use tasuke_agent::root::{MAX_TIMEOUT_SECONDS, OUTPUT_CAP, RootRequest, RootResponse};

/// The system's programs; pkexec starts the helper with almost no environment.
const PATH: &str = "/run/current-system/sw/bin:/run/current-system/sw/sbin:/run/wrappers/bin";
/// The shell, unless the package names its own at build time.
const BASH: &str = match option_env!("TASUKE_BASH") {
    Some(b) => b,
    None => "/run/current-system/sw/bin/bash",
};
/// Requests bigger than this aren't read.
const MAX_REQUEST: usize = 256 * 1024;
/// How long to wait for output after the command ends, when something it
/// started outside its group still holds the pipe.
const DRAIN: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args != ["run"] {
        eprintln!("usage: tasuke-helper run < request.json");
        return ExitCode::from(2);
    }
    // SAFETY: getuid has no preconditions.
    if unsafe { libc::getuid() } != 0 {
        eprintln!("tasuke-helper runs commands as root, so it must be started as root (through pkexec)");
        return ExitCode::from(1);
    }
    let mut input = Vec::new();
    if std::io::stdin().take(MAX_REQUEST as u64 + 1).read_to_end(&mut input).is_err() || input.len() > MAX_REQUEST {
        eprintln!("tasuke-helper: the request is missing or too large");
        return ExitCode::from(2);
    }
    let request = match parse(&input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tasuke-helper: {e}");
            return ExitCode::from(2);
        }
    };
    let user = invoking_user();
    let who = format!("{} (uid {})", user.name, user.uid);
    if let Some(reason) = refusal(&request.command, &user.home) {
        log(&format!("refused for {who}: {} (purpose: {}): {reason}", request.command, request.purpose));
        print(&RootResponse { refused: Some(reason), ..RootResponse::default() });
        return ExitCode::from(3);
    }
    log(&format!("{who} runs as root: {} (purpose: {})", request.command, request.purpose));
    let response = execute(&request, &Exec::system());
    log(&format!(
        "{who}'s command ended: {}",
        if response.timed_out {
            "timed out".to_string()
        } else {
            response.exit_code.map_or("killed by a signal".into(), |c| format!("exit status {c}"))
        }
    ));
    print(&response);
    ExitCode::SUCCESS
}

fn print(response: &RootResponse) {
    match serde_json::to_string(response) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("tasuke-helper: {e}"),
    }
}

fn parse(input: &[u8]) -> Result<RootRequest, String> {
    let mut request: RootRequest =
        serde_json::from_slice(input).map_err(|e| format!("the request isn't the JSON expected: {e}"))?;
    if request.command.trim().is_empty() {
        return Err("the request has no command".into());
    }
    if request.command.contains('\0') {
        return Err("the command contains a NUL byte".into());
    }
    request.purpose = request.purpose.replace('\0', "");
    request.timeout_seconds = request.timeout_seconds.clamp(1, MAX_TIMEOUT_SECONDS);
    Ok(request)
}

/// Why the command never runs, if it doesn't: for the person's home and
/// for root's, since `~` is root's here, and from `/`, where it runs.
fn refusal(command: &str, home: &Path) -> Option<String> {
    policy::refused(command, home)
        .or_else(|| policy::refused(command, Path::new("/root")))
        .or_else(|| policy::refused_from(command, Path::new("/root"), Path::new("/")))
}

struct User {
    uid: u32,
    name: String,
    home: PathBuf,
}

/// Who asked: pkexec says so in PKEXEC_UID; without it, root itself.
fn invoking_user() -> User {
    let uid = std::env::var("PKEXEC_UID").ok().and_then(|u| u.parse().ok()).unwrap_or(0);
    let (name, home) = passwd(uid).unwrap_or_else(|| (uid.to_string(), PathBuf::from("/root")));
    User { uid, name, home }
}

fn passwd(uid: u32) -> Option<(String, PathBuf)> {
    let mut buf = vec![0u8; 16 * 1024];
    // SAFETY: getpwuid_r writes only into `pwd` and `buf`, whose sizes it is given.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr().cast(), buf.len(), &mut result);
        if rc != 0 || result.is_null() {
            return None;
        }
        let name = std::ffi::CStr::from_ptr(pwd.pw_name).to_string_lossy().into_owned();
        let home = std::ffi::CStr::from_ptr(pwd.pw_dir).to_string_lossy().into_owned();
        Some((name, PathBuf::from(home)))
    }
}

/// To the journal, through syslog (authpriv, as sudo and pkexec log).
fn log(message: &str) {
    let mut text: String = message.chars().filter(|&c| c != '\0').collect();
    if text.len() > 8192 {
        let cut = (0..=8192).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        text.truncate(cut);
        text.push('…');
    }
    let Ok(text) = CString::new(text) else { return };
    // SAFETY: the identifier and format are static C strings, and the
    // message is a valid C string passed as the format's one argument.
    unsafe {
        libc::openlog(c"tasuke-helper".as_ptr(), libc::LOG_PID, libc::LOG_AUTHPRIV);
        libc::syslog(libc::LOG_NOTICE, c"%s".as_ptr(), text.as_ptr());
        libc::closelog();
    }
}

/// How commands run: the shell, the search path, the output cap.
struct Exec {
    bash: PathBuf,
    path: String,
    cap: usize,
}

impl Exec {
    fn system() -> Exec {
        Exec { bash: PathBuf::from(BASH), path: PATH.into(), cap: OUTPUT_CAP }
    }
}

/// The first and last bytes of the output, and how many there were.
#[derive(Default)]
struct Capture {
    head: Vec<u8>,
    tail: std::collections::VecDeque<u8>,
    total: usize,
}

impl Capture {
    fn push(&mut self, bytes: &[u8], cap: usize) {
        self.total += bytes.len();
        let half = cap / 2;
        let room = half.saturating_sub(self.head.len());
        let (to_head, rest) = bytes.split_at(room.min(bytes.len()));
        self.head.extend_from_slice(to_head);
        self.tail.extend(rest);
        while self.tail.len() > cap - half {
            self.tail.pop_front();
        }
    }

    fn finish(&self) -> (String, bool) {
        let kept = self.head.len() + self.tail.len();
        let mut out = self.head.clone();
        let truncated = self.total > kept;
        if truncated {
            out.extend_from_slice(format!("\n… {} bytes left out …\n", self.total - kept).as_bytes());
        }
        out.extend(self.tail.iter());
        (String::from_utf8_lossy(&out).into_owned(), truncated)
    }
}

fn execute(request: &RootRequest, exec: &Exec) -> RootResponse {
    match run(request, exec) {
        Ok(r) => r,
        Err(e) => {
            RootResponse { output: format!("tasuke-helper couldn't run the command: {e}"), ..RootResponse::default() }
        }
    }
}

fn run(request: &RootRequest, exec: &Exec) -> std::io::Result<RootResponse> {
    let timeout = Duration::from_secs(u64::from(request.timeout_seconds.clamp(1, MAX_TIMEOUT_SECONDS)));
    let (mut reader, writer) = std::io::pipe()?;
    let mut command = Command::new(&exec.bash);
    command
        .args(["--noprofile", "--norc", "-c", &request.command])
        .env_clear()
        .env("PATH", &exec.path)
        .env("HOME", "/root")
        .env("USER", "root")
        .env("LOGNAME", "root")
        .env("SHELL", &exec.bash)
        .env("LANG", "C.UTF-8")
        .env("TERM", "dumb")
        .env("PAGER", "cat")
        .env("SYSTEMD_PAGER", "")
        .env("SYSTEMD_COLORS", "0")
        .env("NO_COLOR", "1")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .process_group(0);
    let mut child = command.spawn()?;
    // Our copies of the pipe's writing end must go, or reading never ends.
    drop(command);
    let pid = child.id() as libc::pid_t;

    let capture = Arc::new(Mutex::new(Capture::default()));
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    {
        let capture = Arc::clone(&capture);
        let cap = exec.cap;
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => capture.lock().unwrap_or_else(|p| p.into_inner()).push(&buf[..n], cap),
                }
            }
            let _ = done_tx.send(());
        });
    }

    // Wait for it to end without reaping it, so its process group can't be
    // reused before the group is killed below.
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    loop {
        if exited(pid) {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // The timeout, or whatever the command left running in its group.
    // SAFETY: signalling a process group has no memory-safety preconditions.
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    let status = child.wait()?;
    let _ = done_rx.recv_timeout(DRAIN);
    let (output, truncated) = capture.lock().unwrap_or_else(|p| p.into_inner()).finish();
    Ok(RootResponse {
        exit_code: if timed_out { None } else { status.code() },
        output,
        truncated,
        timed_out,
        refused: None,
    })
}

/// Whether the child has exited, leaving it unreaped.
fn exited(pid: libc::pid_t) -> bool {
    // SAFETY: waitid writes only into `info`; WNOWAIT leaves the child to reap.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let rc = libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT);
        rc == 0 && info.si_pid() != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec() -> Exec {
        let bash = std::env::var_os("PATH")
            .and_then(|p| std::env::split_paths(&p).map(|d| d.join("bash")).find(|b| b.is_file()))
            .expect("bash on PATH");
        Exec { bash, path: std::env::var("PATH").unwrap_or_default(), cap: OUTPUT_CAP }
    }

    fn run_cmd(command: &str, timeout: u32) -> RootResponse {
        execute(&RootRequest { command: command.into(), purpose: "test".into(), timeout_seconds: timeout }, &exec())
    }

    #[test]
    fn requests_are_checked() {
        let r = parse(br#"{"command": "ls", "purpose": "look", "timeout_seconds": 9999}"#).unwrap();
        assert_eq!(r.timeout_seconds, MAX_TIMEOUT_SECONDS);
        let r = parse(br#"{"command": "ls", "timeout_seconds": 0}"#).unwrap();
        assert_eq!(r.timeout_seconds, 1);
        assert_eq!(parse(br#"{"command": "ls"}"#).unwrap().timeout_seconds, 60);
        assert!(parse(br#"{"command": "  "}"#).is_err());
        assert!(parse(br#"{"command": "ls\u0000; reboot"}"#).is_err());
        assert!(parse(b"ls").is_err());
        assert!(parse(br#"{"purpose": "x"}"#).is_err());
    }

    #[test]
    fn what_tasuke_never_runs_is_refused() {
        let home = Path::new("/home/alice");
        assert!(refusal("reboot", home).is_some());
        assert!(refusal("rm -rf /home/alice", home).is_some());
        assert!(refusal("rm -rf ~", home).is_some());
        assert!(refusal("systemctl stop display-manager", home).is_some());
        // Relative to /, where root's commands run.
        assert!(refusal("rm -rf etc", home).is_some());
        assert!(refusal("rm -rf nix/store", home).is_some());
        assert!(refusal("rm -rf tmp/build", home).is_none());
        assert!(refusal("nixos-rebuild switch", home).is_none());
        assert!(refusal("systemctl restart NetworkManager", home).is_none());
    }

    #[test]
    fn output_and_exit_status_come_back() {
        let r = run_cmd("echo out; echo err >&2; exit 3", 10);
        assert_eq!(r.exit_code, Some(3));
        assert_eq!(r.output, "out\nerr\n");
        assert!(!r.truncated && !r.timed_out && r.refused.is_none());
    }

    #[test]
    fn the_environment_is_clean_and_there_is_no_input() {
        // SAFETY: no other test reads this variable.
        unsafe { std::env::set_var("TASUKE_TEST_SECRET", "leak") };
        let r = run_cmd("echo \"[$TASUKE_TEST_SECRET]\"; cat; pwd; echo $HOME", 10);
        assert_eq!(r.output, "[]\n/\n/root\n");
        assert_eq!(r.exit_code, Some(0));
    }

    #[test]
    fn the_timeout_kills_the_whole_group() {
        let start = Instant::now();
        let r = run_cmd("sleep 30 & sleep 30; echo never", 1);
        assert!(r.timed_out);
        assert_eq!(r.exit_code, None);
        assert!(!r.output.contains("never"));
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
    }

    #[test]
    fn what_it_leaves_running_is_stopped() {
        let start = Instant::now();
        let r = run_cmd("sleep 30 & echo started", 20);
        assert_eq!((r.exit_code, r.output.as_str()), (Some(0), "started\n"));
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
    }

    #[test]
    fn long_output_keeps_head_and_tail() {
        let r = run_cmd("echo FIRST; head -c 100000 /dev/zero | tr '\\0' x; echo; echo LAST", 10);
        assert!(r.truncated);
        assert!(r.output.starts_with("FIRST\n"));
        assert!(r.output.ends_with("LAST\n"));
        assert!(r.output.contains("bytes left out"));
        assert!(r.output.len() < OUTPUT_CAP + 100, "{}", r.output.len());
    }

    #[test]
    fn capture_keeps_exactly_the_cap() {
        let mut c = Capture::default();
        for chunk in (0..100u8).map(|i| vec![b'a' + i % 26; 1000]) {
            c.push(&chunk, 4096);
        }
        assert_eq!(c.total, 100_000);
        assert_eq!(c.head.len() + c.tail.len(), 4096);
        let (text, truncated) = c.finish();
        assert!(truncated);
        assert!(text.contains("95904 bytes left out"));
        let mut small = Capture::default();
        small.push(b"hello", 4096);
        assert_eq!(small.finish(), ("hello".to_string(), false));
    }

    #[test]
    fn responses_are_the_documented_json() {
        let r = RootResponse { exit_code: Some(0), output: "ok\n".into(), ..RootResponse::default() };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"exit_code":0,"output":"ok\n","truncated":false,"timed_out":false}"#
        );
    }
}
