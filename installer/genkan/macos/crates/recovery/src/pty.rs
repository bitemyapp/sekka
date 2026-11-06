// SPDX-License-Identifier: GPL-3.0-or-later AND MIT
//! Running `bputil`, `kmutil` and `bless` with the owner's credentials,
//! answering their prompts through a pseudo-terminal.
//!
//! Adapted from omacom/omarchy-mac-installer's Recovery/Native/
//! Authorization.c (c07e54c2, MIT, © David Heinemeier Hansson): the prompt
//! rules, the wait for `kmutil` to turn off echo before sending a password,
//! the single confirmation `kmutil configure-boot` asks, and the lines by
//! which `bputil` reports a refused password all come from there. Only
//! these three programs, by absolute path, may be run; the password is
//! never written to a log, and the output kept for "Show Details" has every
//! occurrence of it redacted.
use std::ffi::CString;
use std::time::{Duration, Instant};

/// The programs this module will run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Bputil,
    Kmutil,
    Bless,
}

impl Program {
    pub fn path(self) -> &'static str {
        match self {
            Program::Bputil => "/usr/bin/bputil",
            Program::Kmutil => "/usr/bin/kmutil",
            Program::Bless => "/usr/sbin/bless",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("the password wasn't accepted")]
    PasswordRejected,
    #[error("{0}")]
    Failed(String),
}

/// What a prompt asks for, from the last line of output so far.
#[derive(Debug, PartialEq, Eq)]
pub enum Prompt {
    User,
    Password,
    Confirm,
}

/// Read the last non-empty line, lowercased, and say what it asks.
pub fn prompt(output: &str) -> Option<Prompt> {
    let line = output.trim_end().rsplit(['\n', '\r']).next()?.to_lowercase();
    if line.contains("[y/n]") || line.contains("(y/n)") || line.contains("(enter y or n)") {
        Some(Prompt::Confirm)
    } else if line.contains(':') && line.contains("password") {
        Some(Prompt::Password)
    } else if line.contains(':')
        && (line.contains("username") || line.contains("user name") || line.contains("authorized user"))
    {
        Some(Prompt::User)
    } else {
        None
    }
}

/// `bputil` reports a refused password in these words.
pub fn rejected(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("password rejected")
        || lower.contains("failed to create acm context with provided username and password")
}

/// Replace every occurrence of the password.
pub fn redact(output: &str, password: &str) -> String {
    if password.is_empty() {
        output.to_string()
    } else {
        output.replace(password, "[redacted]")
    }
}

/// How long a command may take before it is stopped.
pub const TIMEOUT: Duration = Duration::from_secs(600);

/// Run `program` with `args` under a pseudo-terminal, answering its user,
/// password and (only for `kmutil configure-boot`) confirmation prompts
/// once each. `log` receives redacted output as it arrives.
pub fn authorized(
    program: Program,
    args: &[&str],
    user: &str,
    password: &str,
    log: &mut dyn FnMut(&str),
) -> Result<(), AuthError> {
    run(program.path(), args, user, password, program == Program::Kmutil && args.first() == Some(&"configure-boot"), TIMEOUT, log)
}

/// The same, for any executable: tests run scripted programs through it.
pub fn run(
    executable: &str,
    args: &[&str],
    user: &str,
    password: &str,
    confirm: bool,
    timeout: Duration,
    log: &mut dyn FnMut(&str),
) -> Result<(), AuthError> {
    if user.is_empty() || password.is_empty() || password.len() > 4096 || user.contains(['\r', '\n']) || password.contains(['\r', '\n']) {
        return Err(AuthError::Failed("The account or password can't be used.".into()));
    }
    let c = |s: &str| CString::new(s).map_err(|_| AuthError::Failed("an argument contains NUL".into()));
    let program = c(executable)?;
    let mut argv_owned = vec![program.clone()];
    for a in args {
        argv_owned.push(c(a)?);
    }
    let mut argv: Vec<*const libc::c_char> = argv_owned.iter().map(|a| a.as_ptr()).collect();
    argv.push(std::ptr::null());
    let path = c("PATH=/usr/bin:/bin:/usr/sbin:/sbin")?;
    let lang = c("LC_ALL=C")?;
    let envp = [path.as_ptr(), lang.as_ptr(), std::ptr::null()];

    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    // SAFETY: openpty fills two descriptors; the other pointers may be null.
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } != 0 {
        return Err(AuthError::Failed("A terminal for the command couldn't be opened.".into()));
    }
    // SAFETY: fork, then only async-signal-safe calls in the child.
    let child = unsafe { libc::fork() };
    if child < 0 {
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
        return Err(AuthError::Failed("The command couldn't be started.".into()));
    }
    if child == 0 {
        unsafe {
            libc::setsid();
            libc::ioctl(slave, libc::TIOCSCTTY as _, 0);
            libc::dup2(slave, 0);
            libc::dup2(slave, 1);
            libc::dup2(slave, 2);
            if slave > 2 {
                libc::close(slave);
            }
            libc::close(master);
            libc::execve(program.as_ptr(), argv.as_ptr(), envp.as_ptr());
            libc::_exit(127);
        }
    }
    unsafe { libc::close(slave) };

    let deadline = Instant::now() + timeout;
    let mut raw: Vec<u8> = Vec::new();
    let mut pending = String::new();
    let (mut user_sent, mut password_sent, mut confirm_sent) = (false, false, false);
    let mut password_waiting = false;
    let mut exited: Option<i32> = None;
    let mut failure: Option<AuthError> = None;
    let send = |text: &str| -> bool {
        let line = format!("{text}\n");
        let mut bytes = line.as_bytes();
        while !bytes.is_empty() {
            // SAFETY: writing our own buffer to our own descriptor.
            let n = unsafe { libc::write(master, bytes.as_ptr().cast(), bytes.len()) };
            if n <= 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return false;
            }
            bytes = &bytes[n as usize..];
        }
        true
    };
    loop {
        if exited.is_none() {
            let mut status = 0;
            // SAFETY: reaping our own child.
            let done = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
            if done == child {
                exited = Some(if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) });
            }
        }
        if Instant::now() >= deadline {
            failure = Some(AuthError::Failed("The command didn't finish in time.".into()));
            break;
        }
        // Wait for the program to turn echo off before sending a password:
        // kmutil prints its prompt before getpass, which flushes input.
        if password_waiting && exited.is_none() {
            let mut t: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(master, &mut t) } == 0 && t.c_lflag & (libc::ECHO | libc::ECHONL) == 0 {
                if !send(password) {
                    failure = Some(AuthError::Failed("The password couldn't be sent.".into()));
                    break;
                }
                password_waiting = false;
            }
        }
        let mut pfd = libc::pollfd {
            fd: master,
            events: libc::POLLIN,
            revents: 0,
        };
        let wait = if exited.is_some() { 0 } else if password_waiting { 10 } else { 100 };
        // SAFETY: polling one descriptor we own.
        let ready = unsafe { libc::poll(&mut pfd, 1, wait) };
        if ready <= 0 || pfd.revents & libc::POLLIN == 0 {
            if exited.is_some() {
                break;
            }
            continue;
        }
        let mut buf = [0u8; 512];
        let n = unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            if exited.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let bytes = &buf[..n as usize];
        if raw.len() < 64 * 1024 {
            raw.extend_from_slice(bytes);
        }
        log(&redact(&String::from_utf8_lossy(bytes), password));
        if exited.is_some() {
            continue;
        }
        pending.push_str(&String::from_utf8_lossy(bytes));
        if pending.len() > 4096 {
            pending = pending[pending.len() - 2048..].to_string();
        }
        match prompt(&pending) {
            Some(Prompt::Confirm) => {
                if !confirm || confirm_sent {
                    failure = Some(AuthError::Failed(format!("{executable} asked an unexpected question.")));
                    break;
                }
                confirm_sent = true;
                if !send("y") {
                    failure = Some(AuthError::Failed("The answer couldn't be sent.".into()));
                    break;
                }
                pending.clear();
            }
            Some(Prompt::Password) => {
                if password_sent {
                    // bputil's refusal reads like a second prompt.
                    failure = Some(AuthError::PasswordRejected);
                    break;
                }
                password_sent = true;
                password_waiting = true;
                pending.clear();
            }
            Some(Prompt::User) => {
                if user_sent {
                    failure = Some(AuthError::Failed(format!("{executable} asked for the account twice.")));
                    break;
                }
                user_sent = true;
                if !send(user) {
                    failure = Some(AuthError::Failed("The account couldn't be sent.".into()));
                    break;
                }
                pending.clear();
            }
            None => {}
        }
    }
    // The session is this command's alone: stop anything still holding it.
    unsafe {
        libc::kill(-child, libc::SIGKILL);
        if exited.is_none() {
            libc::kill(child, libc::SIGKILL);
            let mut status = 0;
            libc::waitpid(child, &mut status, 0);
        }
        libc::close(master);
    }
    let output = String::from_utf8_lossy(&raw).to_string();
    raw.iter_mut().for_each(|b| *b = 0);
    if let Some(f) = failure {
        if f != AuthError::PasswordRejected && rejected(&output) {
            return Err(AuthError::PasswordRejected);
        }
        return Err(f);
    }
    match exited {
        Some(0) => Ok(()),
        Some(code) if rejected(&output) => {
            let _ = code;
            Err(AuthError::PasswordRejected)
        }
        Some(code) => Err(AuthError::Failed(format!("{executable} stopped with status {code}."))),
        None => Err(AuthError::Failed(format!("{executable} didn't finish."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn prompts_are_recognized_as_omarchy_found_them() {
        assert_eq!(prompt("Some text\nAuthorized user: "), Some(Prompt::User));
        assert_eq!(prompt("Enter password for callen:"), Some(Prompt::Password));
        assert_eq!(prompt("Password: "), Some(Prompt::Password));
        assert_eq!(prompt("Are you sure? [y/n] "), Some(Prompt::Confirm));
        assert_eq!(prompt("Working...\n"), None);
        assert!(rejected("bputil: failed to set credential: Password rejected"));
    }

    /// The test environment's own `stty`, by absolute path: the commands run
    /// with a fixed PATH that a build sandbox doesn't populate.
    fn stty() -> String {
        std::env::var_os("PATH")
            .and_then(|p| std::env::split_paths(&p).map(|d| d.join("stty")).find(|p| p.exists()))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "stty".into())
    }

    fn script(dir: &std::path::Path, body: &str) -> String {
        let path = dir.join("tool");
        let body = body.replace("stty", &stty());
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn sh() -> bool {
        std::path::Path::new("/bin/sh").exists()
    }

    #[test]
    fn a_tool_that_asks_for_a_user_and_hidden_password_gets_both_once() {
        if !sh() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // Like kmutil: the prompt comes before echo is turned off.
        let tool = script(
            dir.path(),
            "printf 'Authorized user: '; read u; printf 'Password: '; stty -echo; read p; stty echo; echo; \
             [ \"$u\" = owner ] && [ \"$p\" = 's3cret pass' ] && echo done && exit 0; exit 9\n",
        );
        let mut log = String::new();
        run(&tool, &[], "owner", "s3cret pass", false, Duration::from_secs(10), &mut |s| log.push_str(s)).unwrap();
        assert!(log.contains("done"));
        assert!(!log.contains("s3cret"));
    }

    #[test]
    fn confirmation_is_answered_only_where_expected() {
        if !sh() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tool = script(dir.path(), "printf 'Really? [y/n] '; read a; [ \"$a\" = y ] && exit 0; exit 3\n");
        run(&tool, &[], "owner", "pw", true, Duration::from_secs(10), &mut |_| {}).unwrap();
        assert!(matches!(
            run(&tool, &[], "owner", "pw", false, Duration::from_secs(10), &mut |_| {}),
            Err(AuthError::Failed(_))
        ));
    }

    #[test]
    fn a_refused_password_is_reported_as_such() {
        if !sh() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tool = script(
            dir.path(),
            "printf 'Password: '; stty -echo; read p; stty echo; echo; echo 'failed to set credential: Password rejected'; exit 1\n",
        );
        assert_eq!(
            run(&tool, &[], "owner", "wrong", false, Duration::from_secs(10), &mut |_| {}),
            Err(AuthError::PasswordRejected)
        );
        let again = script(dir.path(), "printf 'Password: '; stty -echo; read p; stty echo; echo; printf 'Password: '; stty -echo; read p; exit 1\n");
        assert_eq!(
            run(&again, &[], "owner", "wrong", false, Duration::from_secs(10), &mut |_| {}),
            Err(AuthError::PasswordRejected)
        );
    }

    #[test]
    fn a_hanging_tool_is_stopped() {
        if !sh() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tool = script(dir.path(), "sleep 30\n");
        assert!(run(&tool, &[], "owner", "pw", false, Duration::from_millis(300), &mut |_| {}).is_err());
    }
}
