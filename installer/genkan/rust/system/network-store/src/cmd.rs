// SPDX-License-Identifier: GPL-3.0-or-later
//! Running the tools the store is made with (cryptsetup, systemd's TPM
//! tools, e2fsprogs, util-linux), each bounded in time: a TPM or device that
//! hangs must not hold NetworkManager back.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub struct Output {
    /// None when killed for taking too long, or by a signal.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// A tool with its arguments, input and environment.
pub struct Tool<'a> {
    program: &'a str,
    args: Vec<String>,
    input: Option<&'a [u8]>,
    env: Vec<(&'a str, &'a str)>,
    limit: Duration,
}

pub fn tool(program: &str) -> Tool<'_> {
    Tool {
        program,
        args: Vec::new(),
        input: None,
        env: Vec::new(),
        limit: Duration::from_secs(60),
    }
}

impl<'a> Tool<'a> {
    pub fn arg(mut self, arg: impl AsRef<std::ffi::OsStr>) -> Self {
        self.args.push(arg.as_ref().to_string_lossy().into_owned());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: AsRef<std::ffi::OsStr>>(mut self, args: I) -> Self {
        for arg in args {
            self = self.arg(arg);
        }
        self
    }

    /// Written to its standard input, which is then closed.
    pub fn input(mut self, input: &'a [u8]) -> Self {
        self.input = Some(input);
        self
    }

    pub fn env(mut self, key: &'a str, value: &'a str) -> Self {
        self.env.push((key, value));
        self
    }

    pub fn limit(mut self, limit: Duration) -> Self {
        self.limit = limit;
        self
    }

    pub fn run(self) -> Output {
        let mut command = Command::new(self.program);
        command
            .args(&self.args)
            .stdin(if self.input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Output {
                    code: None,
                    stdout: Vec::new(),
                    stderr: format!("{}: {error}", self.program),
                };
            }
        };
        if let (Some(input), Some(mut stdin)) = (self.input, child.stdin.take()) {
            let _ = stdin.write_all(input);
        }
        let mut out = child.stdout.take().expect("piped");
        let mut err = child.stderr.take().expect("piped");
        let stdout = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out.read_to_end(&mut buf);
            buf
        });
        let stderr = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).trim().to_owned()
        });
        let start = Instant::now();
        let code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) if start.elapsed() < self.limit => {
                    thread::sleep(Duration::from_millis(20))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let mut stderr = stderr.join().unwrap_or_default();
        if code.is_none() && start.elapsed() >= self.limit {
            stderr = format!(
                "{} took longer than {:?}: {stderr}",
                self.program, self.limit
            );
        }
        Output {
            code,
            stdout: stdout.join().unwrap_or_default(),
            stderr,
        }
    }

    /// Its standard output, or why it failed.
    pub fn ok(self) -> Result<Vec<u8>, String> {
        let what = format!("{} {}", self.program, self.args.join(" "));
        let out = self.run();
        if out.success() {
            Ok(out.stdout)
        } else {
            Err(format!("{what}: {} ({:?})", out.stderr, out.code))
        }
    }
}
