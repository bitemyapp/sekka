//! Running one approved command as root: `tasuke-helper run`, through
//! pkexec under the polkit action [`ACTION`], or directly when Tasuke is
//! root already (as `tasuke ask` is in the evaluation VM). See DESIGN.md,
//! "Files and processes".
//!
//! The helper reads one [`RootRequest`] as JSON on its standard input and
//! prints one [`RootResponse`]. It refuses what [`crate::policy::refused`]
//! refuses, runs the command with `bash -c` in a clean environment with no
//! input, in its own process group, kills the group at the timeout, keeps
//! the head and tail of the output, and logs the command and its purpose to
//! the journal.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// The polkit action the helper runs under (`auth_admin_keep`).
pub const ACTION: &str = "io.github.bitemyapp.Tasuke.run-as-root";
/// The policy file that declares [`ACTION`] and names the helper.
pub const POLICY: &str = "io.github.bitemyapp.Tasuke.policy";
/// Where polkit finds actions: NixOS links them into the system profile.
pub const ACTION_DIRS: [&str; 3] =
    ["/run/current-system/sw/share/polkit-1/actions", "/etc/polkit-1/actions", "/usr/share/polkit-1/actions"];
pub const DEFAULT_TIMEOUT_SECONDS: u32 = 60;
pub const MAX_TIMEOUT_SECONDS: u32 = 300;
/// The output kept, head and tail together.
pub const OUTPUT_CAP: usize = 32 * 1024;
/// pkexec's exit status when the person dismissed the password dialog.
const PKEXEC_DISMISSED: i32 = 126;
/// pkexec's exit status when authorization failed or there's no agent.
const PKEXEC_NOT_AUTHORIZED: i32 = 127;

/// What `tasuke-helper run` reads on its standard input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootRequest {
    pub command: String,
    /// The model's stated purpose, for the journal.
    #[serde(default)]
    pub purpose: String,
    /// Clamped to 1..=[`MAX_TIMEOUT_SECONDS`].
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
}

fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_SECONDS
}

/// What `tasuke-helper run` prints.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootResponse {
    /// The command's exit status; none when a signal ended it (the timeout).
    pub exit_code: Option<i32>,
    /// Standard output and error as they interleaved, capped at
    /// [`OUTPUT_CAP`] with the middle left out.
    pub output: String,
    /// Whether the output lost its middle.
    pub truncated: bool,
    /// Whether the timeout ended it.
    pub timed_out: bool,
    /// Why the helper refused to run it, when it did (nothing ran).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RootError {
    /// The helper isn't installed (the NixOS module installs it).
    NoHelper,
    /// The person dismissed the password dialog.
    Dismissed,
    /// polkit didn't authorize it (a wrong password, or no agent to ask).
    NotAuthorized,
    /// The helper or pkexec failed; the text says how.
    Failed(String),
}

impl std::fmt::Display for RootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RootError::NoHelper => f.write_str(
                "Tasuke's administrator helper isn't installed (programs.tasuke.enable = true installs it), so nothing ran.",
            ),
            RootError::Dismissed => f.write_str("The password dialog was dismissed, so nothing ran."),
            RootError::NotAuthorized => f.write_str("The system didn't allow it (no password was accepted), so nothing ran."),
            RootError::Failed(e) => write!(f, "The administrator helper failed: {e}"),
        }
    }
}

impl std::error::Error for RootError {}

/// The installed `tasuke-helper`: `$TASUKE_HELPER`, else the program the
/// polkit action names (pkexec only accepts that exact path), else the one
/// the package was built with (`$TASUKE_HELPER_PATH` at build time, the
/// same path), else the one beside this program (`../libexec/tasuke-helper`).
pub fn find_helper() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("TASUKE_HELPER").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    if let Some(p) = option_env!("TASUKE_HELPER_PATH").map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let dirs = ACTION_DIRS.map(Path::new);
    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(dir.join(POLICY)) else { continue };
        if let Some(path) = exec_path(&text, ACTION).map(PathBuf::from)
            && path.is_file()
        {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../libexec/tasuke-helper"), dir.join("tasuke-helper")]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}

/// The program a polkit policy's `action` runs, from its
/// `org.freedesktop.policykit.exec.path` annotation.
pub fn exec_path(policy: &str, action: &str) -> Option<String> {
    let start = policy.find(&format!("<action id=\"{action}\""))?;
    let body = &policy[start..];
    let body = &body[..body.find("</action>")?];
    let key = "<annotate key=\"org.freedesktop.policykit.exec.path\">";
    let value = &body[body.find(key)? + key.len()..];
    let value = value[..value.find("</annotate>")?].trim();
    value.starts_with('/').then(|| value.to_string())
}

/// Run `request` as root: through pkexec (polkit asks for a password), or
/// directly when this process is root. Blocks until it ends; run it off the
/// UI thread. The approval must already have been given.
pub fn run_as_root(request: &RootRequest) -> Result<RootResponse, RootError> {
    let helper = find_helper().ok_or(RootError::NoHelper)?;
    // SAFETY: geteuid has no preconditions.
    let root = unsafe { libc::geteuid() } == 0;
    let mut command = if root {
        Command::new(&helper)
    } else {
        // On NixOS only the wrapper in /run/wrappers is setuid.
        let wrapper = Path::new("/run/wrappers/bin/pkexec");
        let mut c = Command::new(if wrapper.exists() { wrapper } else { Path::new("pkexec") });
        // Without a terminal, the desktop's polkit agent asks.
        if !std::io::stderr().is_terminal() {
            c.arg("--disable-internal-agent");
        }
        c.arg(&helper);
        c
    };
    command.arg("run").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    run_helper(command, request, !root)
}

fn run_helper(mut command: Command, request: &RootRequest, via_pkexec: bool) -> Result<RootResponse, RootError> {
    let mut child = command.spawn().map_err(|e| RootError::Failed(format!("couldn't start it: {e}")))?;
    let json = serde_json::to_vec(request).map_err(|e| RootError::Failed(e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A helper that exits early (refused, not root) closes its end.
        let _ = stdin.write_all(&json);
    }
    let mut stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    let status = child.wait().map_err(|e| RootError::Failed(e.to_string()))?;
    let errors = errors.join().unwrap_or_default();
    if let Ok(response) = serde_json::from_str::<RootResponse>(out.trim()) {
        return Ok(response);
    }
    match status.code() {
        Some(PKEXEC_DISMISSED) if via_pkexec => Err(RootError::Dismissed),
        Some(PKEXEC_NOT_AUTHORIZED) if via_pkexec => Err(RootError::NotAuthorized),
        _ => {
            let why = errors.trim();
            Err(RootError::Failed(if why.is_empty() { format!("it ended with {status}") } else { why.to_string() }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_responses_have_the_documented_json() {
        let r: RootRequest = serde_json::from_str(r#"{"command": "ls", "purpose": "look"}"#).unwrap();
        assert_eq!(r.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
        let r = RootResponse { exit_code: Some(0), output: "x".into(), ..RootResponse::default() };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"exit_code":0,"output":"x","truncated":false,"timed_out":false}"#
        );
        let r: RootResponse = serde_json::from_str(
            r#"{"exit_code":null,"output":"","truncated":false,"timed_out":false,"refused":"No."}"#,
        )
        .unwrap();
        assert_eq!(r.refused.as_deref(), Some("No."));
    }

    #[test]
    fn the_policy_names_the_helper() {
        let policy = r#"<policyconfig><action id="io.github.bitemyapp.Tasuke.run-as-root">
            <annotate key="org.freedesktop.policykit.exec.path">/nix/store/x-tasuke/libexec/tasuke-helper</annotate>
            </action></policyconfig>"#;
        assert_eq!(exec_path(policy, ACTION).as_deref(), Some("/nix/store/x-tasuke/libexec/tasuke-helper"));
        assert_eq!(exec_path(policy, "other"), None);
    }

    #[test]
    fn a_helper_that_fails_says_why() {
        let mut c = Command::new("sh");
        c.args(["-c", "cat >/dev/null; echo broken >&2; exit 4"]);
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let request = RootRequest { command: "true".into(), purpose: String::new(), timeout_seconds: 5 };
        assert_eq!(run_helper(c, &request, false), Err(RootError::Failed("broken".into())));
        let mut c = Command::new("sh");
        c.args(["-c", "exit 126"]);
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        assert_eq!(run_helper(c, &request, true), Err(RootError::Dismissed));
        let mut c = Command::new("sh");
        c.args(["-c", r#"cat >/dev/null; echo '{"exit_code":3,"output":"hi","truncated":false,"timed_out":false}'"#]);
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let r = run_helper(c, &request, true).unwrap();
        assert_eq!((r.exit_code, r.output.as_str()), (Some(3), "hi"));
    }
}
