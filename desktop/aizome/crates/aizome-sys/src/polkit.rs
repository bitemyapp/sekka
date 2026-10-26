//! The polkit authentication agent: the shell's "an application wants to
//! do something that needs a password" dialog.
//!
//! [`PolkitAgent::register`] serves `org.freedesktop.PolicyKit1.AuthenticationAgent`
//! on the system bus and registers it with polkitd for this login session.
//! When an action needs authentication, polkitd calls `BeginAuthentication`;
//! the agent hands the UI an [`AuthEvent::Begin`] with an [`AuthRequest`]
//! (the message, icon, action and the identities that may authenticate),
//! and the D-Bus call stays pending until the conversation ends.
//!
//! The UI picks an identity and calls [`AuthRequest::authenticate`]; the
//! agent then runs polkit's helper, which does the PAM conversation as
//! root, and relays it line by line ([`AuthEvent::Prompt`] for a password,
//! [`AuthEvent::Error`] and [`AuthEvent::Info`] for messages), the UI
//! answering prompts with [`AuthRequest::respond`]. When PAM succeeds the
//! helper itself tells polkitd (`AuthenticationAgentResponse2`), prints
//! `SUCCESS`, and the agent completes `BeginAuthentication`. On `FAILURE`
//! the dialog says so ([`AuthEvent::Failed`]) and may try again; dismissing
//! it is [`AuthRequest::cancel`]. polkitd's `CancelAuthentication` (the
//! application gave up) ends it too.
//!
//! # The helper
//!
//! polkit 126 and later run the helper as a socket-activated service at
//! `/run/polkit/agent-helper.socket` (no setuid binary); the agent writes the
//! user name and the cookie, one per line, and the helper learns who is
//! asking from the socket's credentials. Older polkit, or a system without
//! that socket, uses the setuid helper: `polkit-agent-helper-1 USER`, with
//! the cookie written to its standard input (never on its command line).
//! NixOS puts the setuid wrapper at `/run/wrappers/bin/polkit-agent-helper-1`;
//! other systems keep it in a libexec directory. Both speak the same lines:
//!
//! ```text
//! helper → agent   PAM_PROMPT_ECHO_OFF Password:     (agent answers with a line)
//! helper → agent   PAM_PROMPT_ECHO_ON Username:      (agent answers with a line)
//! helper → agent   PAM_ERROR_MSG …  /  PAM_TEXT_INFO …
//! helper → agent   SUCCESS  or  FAILURE
//! ```
//!
//! Needs polkit (`security.polkit.enable`, on by default). Only one agent
//! can serve a session: register fails while another one (a different
//! desktop's) has it.

use crate::bus::{Bus, Opts, value_u64};
use crate::error::{Error, Result};
use crate::oneshot;
use crate::service::lock;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use zbus::zvariant::{OwnedValue, Value};

const POLKIT: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const IFACE_AUTHORITY: &str = "org.freedesktop.PolicyKit1.Authority";
const AGENT_PATH: &str = "/org/aizome/PolicyKit1/AuthenticationAgent";

/// The socket of polkit's socket-activated helper.
pub const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";
/// Where the setuid helper may be, in order.
pub const HELPER_PATHS: [&str; 5] = [
    "/run/wrappers/bin/polkit-agent-helper-1",
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
    "/usr/lib/policykit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-1/polkit-agent-helper-1",
];

/// How to reach polkit's helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Helper {
    /// The socket-activated helper (polkit 126+).
    Socket(PathBuf),
    /// The setuid helper program.
    Program(PathBuf),
}

impl Helper {
    /// The socket if it is there, else the first setuid helper found.
    pub fn find() -> Option<Helper> {
        if Path::new(HELPER_SOCKET).exists() {
            return Some(Helper::Socket(HELPER_SOCKET.into()));
        }
        HELPER_PATHS.iter().map(PathBuf::from).find(|p| p.is_file()).map(Helper::Program)
    }
}

/// Who may authenticate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub uid: u32,
    /// The login name.
    pub name: String,
    /// The real name from the password database's GECOS field.
    pub real_name: String,
}

impl Identity {
    pub fn display_name(&self) -> &str {
        if self.real_name.is_empty() { &self.name } else { &self.real_name }
    }
}

/// What the dialog shows and does.
#[derive(Debug)]
pub enum AuthEvent {
    /// Show the dialog for this request.
    Begin(AuthRequest),
    /// Ask for a line: a password (`echo` false: hide what is typed) or
    /// something else, such as a user name (`echo` true). Answer with
    /// [`AuthRequest::respond`].
    Prompt { cookie: String, text: String, echo: bool },
    /// Show an error from PAM ("Authentication failure").
    Error { cookie: String, text: String },
    /// Show a message from PAM ("Touch the fingerprint reader").
    Info { cookie: String, text: String },
    /// That attempt failed (a wrong password): say so and let the person
    /// try again with [`AuthRequest::authenticate`], or cancel.
    Failed { cookie: String },
    /// It is over (authenticated, cancelled or abandoned): close the dialog.
    Done { cookie: String, authorized: bool },
}

/// One authentication request from polkitd.
#[derive(Clone)]
pub struct AuthRequest {
    pub cookie: String,
    /// The polkit action (`org.freedesktop.udisks2.filesystem-mount`).
    pub action_id: String,
    /// What to tell the person ("Authentication is required to mount…").
    pub message: String,
    /// The icon to show, if the action names one.
    pub icon_name: String,
    /// Details polkit passes (`polkit.subject-pid`, `program`…).
    pub details: HashMap<String, String>,
    /// Who may authenticate; [`default_identity`](AuthRequest::default_identity)
    /// suggests one.
    pub identities: Vec<Identity>,
    session: Arc<Session>,
}

impl std::fmt::Debug for AuthRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthRequest")
            .field("action_id", &self.action_id)
            .field("message", &self.message)
            .field("identities", &self.identities)
            .finish_non_exhaustive()
    }
}

impl AuthRequest {
    /// The identity to preselect: the person at the screen if they may
    /// authenticate, else the first one.
    pub fn default_identity(&self) -> Option<&Identity> {
        // SAFETY: getuid has no failure mode.
        let me = unsafe { libc::getuid() };
        self.identities.iter().find(|i| i.uid == me).or_else(|| self.identities.first())
    }

    /// Start (or restart, after a failure) the conversation as `identity`.
    pub fn authenticate(&self, identity: &Identity) {
        Session::start(&self.session, identity.name.clone());
    }

    /// Answer the current prompt. The text is written to the helper and the
    /// agent's copy is overwritten.
    pub fn respond(&self, text: &str) {
        self.session.respond(text);
    }

    /// The person dismissed the dialog.
    pub fn cancel(&self) {
        self.session.finish(false);
    }
}

type Ui = Arc<dyn Fn(AuthEvent) + Send + Sync>;

/// One `BeginAuthentication` in progress.
struct Session {
    cookie: String,
    ui: Ui,
    helper: Helper,
    inner: Mutex<SessionInner>,
}

struct SessionInner {
    /// Completes `BeginAuthentication`: authorized or not.
    reply: Option<oneshot::Sender<bool>>,
    /// The conversation running, if any.
    conversation: Option<Conversation>,
    /// Counts conversations, so a superseded one's events are dropped.
    generation: u64,
}

struct Conversation {
    responses: mpsc::Sender<Vec<u8>>,
    end: Ender,
}

/// Ends a conversation from outside its thread.
#[derive(Clone)]
enum Ender {
    Child(Arc<Mutex<Option<Child>>>),
    Stream(Arc<UnixStream>),
}

impl Ender {
    fn end(&self) {
        match self {
            Ender::Child(child) => {
                if let Some(mut child) = lock(child).take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            Ender::Stream(stream) => {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

impl Session {
    fn start(this: &Arc<Session>, user: String) {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let mut inner = lock(&this.inner);
        if inner.reply.is_none() {
            return; // already over
        }
        if let Some(old) = inner.conversation.take() {
            old.end.end();
        }
        inner.generation += 1;
        let generation = inner.generation;
        let connected = connect(&this.helper, &user, &this.cookie);
        let (reader, writer, end) = match connected {
            Ok(parts) => parts,
            Err(error) => {
                log::warn!("aizome-sys: polkit helper: {error}");
                drop(inner);
                (this.ui)(AuthEvent::Error { cookie: this.cookie.clone(), text: error.describe() });
                (this.ui)(AuthEvent::Failed { cookie: this.cookie.clone() });
                return;
            }
        };
        inner.conversation = Some(Conversation { responses: tx, end: end.clone() });
        drop(inner);
        let session = this.clone();
        let spawned = std::thread::Builder::new()
            .name("aizome-sys polkit".into())
            .spawn(move || session.converse(generation, reader, writer, rx, end));
        if spawned.is_err() {
            (this.ui)(AuthEvent::Failed { cookie: this.cookie.clone() });
        }
    }

    /// Whether `generation` is still the conversation in progress.
    fn current(&self, generation: u64) -> bool {
        let inner = lock(&self.inner);
        inner.reply.is_some() && inner.generation == generation
    }

    /// Relay the helper's lines until it says SUCCESS or FAILURE.
    fn converse(
        self: Arc<Session>,
        generation: u64,
        reader: Box<dyn BufRead + Send>,
        mut writer: Box<dyn Write + Send>,
        responses: mpsc::Receiver<Vec<u8>>,
        end: Ender,
    ) {
        let cookie = self.cookie.clone();
        let mut outcome = None;
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if !self.current(generation) {
                break;
            }
            match parse_line(&line) {
                HelperLine::Prompt { text, echo } => {
                    (self.ui)(AuthEvent::Prompt { cookie: cookie.clone(), text, echo });
                    let Ok(mut answer) = responses.recv() else { break };
                    answer.push(b'\n');
                    let written = writer.write_all(&answer).and_then(|()| writer.flush());
                    answer.fill(0);
                    if written.is_err() {
                        break;
                    }
                }
                HelperLine::Error(text) => (self.ui)(AuthEvent::Error { cookie: cookie.clone(), text }),
                HelperLine::Info(text) => (self.ui)(AuthEvent::Info { cookie: cookie.clone(), text }),
                HelperLine::Success => {
                    outcome = Some(true);
                    break;
                }
                HelperLine::Failure => {
                    outcome = Some(false);
                    break;
                }
                HelperLine::Unknown(line) => {
                    // As libpolkit-agent does: an unknown line ends the attempt.
                    log::warn!("aizome-sys: unknown line from the polkit helper: {line:?}");
                    outcome = Some(false);
                    break;
                }
            }
        }
        end.end();
        if !self.current(generation) {
            return;
        }
        lock(&self.inner).conversation = None;
        match outcome {
            Some(true) => self.finish(true),
            _ => (self.ui)(AuthEvent::Failed { cookie }),
        }
    }

    fn respond(&self, text: &str) {
        let inner = lock(&self.inner);
        if let Some(conversation) = &inner.conversation {
            let _ = conversation.responses.send(text.as_bytes().to_vec());
        }
    }

    /// End it: reply to polkitd and tell the UI.
    fn finish(&self, authorized: bool) {
        let mut inner = lock(&self.inner);
        let Some(reply) = inner.reply.take() else { return };
        if let Some(conversation) = inner.conversation.take() {
            conversation.end.end();
        }
        drop(inner);
        reply.send(authorized);
        (self.ui)(AuthEvent::Done { cookie: self.cookie.clone(), authorized });
    }
}

type Connected = (Box<dyn BufRead + Send>, Box<dyn Write + Send>, Ender);

/// Start the helper for `user` and hand it the cookie.
fn connect(helper: &Helper, user: &str, cookie: &str) -> Result<Connected> {
    if user.contains('\n') || cookie.contains('\n') {
        return Err(Error::InvalidArgument("a line break in a user name or cookie".into()));
    }
    match helper {
        Helper::Socket(path) => {
            let stream = UnixStream::connect(path)?;
            let mut writer = stream.try_clone()?;
            writer.write_all(format!("{user}\n{cookie}\n").as_bytes())?;
            writer.flush()?;
            let reader = BufReader::new(stream.try_clone()?);
            Ok((Box::new(reader), Box::new(writer), Ender::Stream(Arc::new(stream))))
        }
        Helper::Program(path) => {
            let mut child = Command::new(path)
                .arg(user)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .env("LC_ALL", "C")
                .spawn()
                .map_err(|e| Error::Unavailable(format!("{}: {e}", path.display())))?;
            let mut stdin = child.stdin.take().ok_or_else(|| Error::Failed("no helper stdin".into()))?;
            let stdout = child.stdout.take().ok_or_else(|| Error::Failed("no helper stdout".into()))?;
            stdin.write_all(format!("{cookie}\n").as_bytes())?;
            stdin.flush()?;
            Ok((Box::new(BufReader::new(stdout)), Box::new(stdin), Ender::Child(Arc::new(Mutex::new(Some(child))))))
        }
    }
}

/// A line from the helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HelperLine {
    Prompt { text: String, echo: bool },
    Error(String),
    Info(String),
    Success,
    Failure,
    Unknown(String),
}

/// Undo the helper's C-style escapes, as GLib's `g_strcompress` does (which
/// libpolkit-agent applies to each line): `\n`, `\t`, `\\`, `\"`, octal
/// `\NNN`, and `\X` for anything else.
pub(crate) fn compress(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 == bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        match bytes[i] {
            b'0'..=b'7' => {
                let mut value: u32 = 0;
                let mut digits = 0;
                while digits < 3 && i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) {
                    value = value * 8 + u32::from(bytes[i] - b'0');
                    i += 1;
                    digits += 1;
                }
                out.push((value & 0xff) as u8);
                continue;
            }
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn parse_line(line: &str) -> HelperLine {
    let line = compress(line.strip_suffix('\n').unwrap_or(line));
    let line = line.as_str();
    if let Some(text) = line.strip_prefix("PAM_PROMPT_ECHO_OFF ") {
        HelperLine::Prompt { text: text.to_owned(), echo: false }
    } else if let Some(text) = line.strip_prefix("PAM_PROMPT_ECHO_ON ") {
        HelperLine::Prompt { text: text.to_owned(), echo: true }
    } else if let Some(text) = line.strip_prefix("PAM_ERROR_MSG ") {
        HelperLine::Error(text.to_owned())
    } else if let Some(text) = line.strip_prefix("PAM_TEXT_INFO ") {
        HelperLine::Info(text.to_owned())
    } else if line == "SUCCESS" {
        HelperLine::Success
    } else if line == "FAILURE" {
        HelperLine::Failure
    } else {
        HelperLine::Unknown(line.to_owned())
    }
}

/// A user from the password database.
fn user_by_uid(uid: u32) -> Option<Identity> {
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 4096];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: getpwuid_r writes into pwd and buf (of the given length) and
    // sets result to &pwd on success.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr().cast(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() {
        return None;
    }
    // SAFETY: on success the fields point into buf, NUL-terminated.
    let text = |p: *const libc::c_char| unsafe {
        if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
    };
    let gecos = text(pwd.pw_gecos);
    Some(Identity { uid, name: text(pwd.pw_name), real_name: gecos.split(',').next().unwrap_or("").to_owned() })
}

/// The members of a group (by name in the group database).
fn group_members(gid: u32) -> Vec<String> {
    let mut grp: libc::group = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 16384];
    let mut result: *mut libc::group = std::ptr::null_mut();
    // SAFETY: as getpwuid_r, for the group database.
    let rc = unsafe { libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr().cast(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() || grp.gr_mem.is_null() {
        return Vec::new();
    }
    let mut members = Vec::new();
    let mut p = grp.gr_mem;
    // SAFETY: gr_mem is a NULL-terminated array of C strings in buf.
    unsafe {
        while !(*p).is_null() {
            members.push(std::ffi::CStr::from_ptr(*p).to_string_lossy().into_owned());
            p = p.add(1);
        }
    }
    members
}

fn user_by_name(name: &str) -> Option<Identity> {
    let name = std::ffi::CString::new(name).ok()?;
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 4096];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: as in user_by_uid.
    let rc = unsafe { libc::getpwnam_r(name.as_ptr(), &mut pwd, buf.as_mut_ptr().cast(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() {
        return None;
    }
    user_by_uid(pwd.pw_uid)
}

/// The people behind polkit's identities: users as they are, groups
/// (`wheel`) as their members.
fn identities(raw: &[(String, HashMap<String, OwnedValue>)]) -> Vec<Identity> {
    let mut found: Vec<Identity> = Vec::new();
    for (kind, details) in raw {
        let id = details.get("uid").or_else(|| details.get("gid")).and_then(|v| value_u64(v));
        let Some(id) = id.and_then(|n| u32::try_from(n).ok()) else { continue };
        let people: Vec<Identity> = match kind.as_str() {
            "unix-user" => user_by_uid(id).into_iter().collect(),
            "unix-group" => group_members(id).iter().filter_map(|name| user_by_name(name)).collect(),
            _ => Vec::new(),
        };
        for person in people {
            if !found.iter().any(|f| f.uid == person.uid) {
                found.push(person);
            }
        }
    }
    found
}

/// The served `org.freedesktop.PolicyKit1.AuthenticationAgent`.
struct Agent {
    ui: Ui,
    helper: Helper,
    sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum PolkitError {
    Failed(String),
    Cancelled(String),
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities_raw: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> Result<(), PolkitError> {
        let people = identities(&identities_raw);
        if people.is_empty() {
            return Err(PolkitError::Failed("No one may authenticate for this".into()));
        }
        let (tx, rx) = oneshot::channel();
        let session = Arc::new(Session {
            cookie: cookie.clone(),
            ui: self.ui.clone(),
            helper: self.helper.clone(),
            inner: Mutex::new(SessionInner { reply: Some(tx), conversation: None, generation: 0 }),
        });
        lock(&self.sessions).insert(cookie.clone(), session.clone());
        (self.ui)(AuthEvent::Begin(AuthRequest {
            cookie: cookie.clone(),
            action_id,
            message,
            icon_name,
            details,
            identities: people,
            session,
        }));
        let authorized = rx.await;
        lock(&self.sessions).remove(&cookie);
        match authorized {
            Some(true) => Ok(()),
            _ => Err(PolkitError::Cancelled("Authentication was cancelled".into())),
        }
    }

    fn cancel_authentication(&self, cookie: String) -> Result<(), PolkitError> {
        let session = lock(&self.sessions).get(&cookie).cloned();
        match session {
            Some(session) => {
                session.finish(false);
                Ok(())
            }
            None => Err(PolkitError::Failed(format!("No authentication with cookie {cookie}"))),
        }
    }
}

/// The registered agent; unregistered when dropped.
pub struct PolkitAgent {
    bus: Bus,
    session_id: String,
}

static AGENTS: AtomicU64 = AtomicU64::new(0);

impl PolkitAgent {
    /// Serve the agent on the system bus for this login session, with the
    /// helper found on this system, handing events to `ui`.
    pub fn register(ui: impl Fn(AuthEvent) + Send + Sync + 'static) -> Result<PolkitAgent> {
        let bus = Bus::system()?;
        let helper = Helper::find().ok_or_else(|| Error::Unavailable("polkit's agent helper".into()))?;
        let session_id = crate::power::session_id(&bus).ok_or_else(|| Error::NotFound("no login session".into()))?;
        PolkitAgent::register_with(bus, session_id, helper, ui)
    }

    /// Serve the agent on `bus` for the login session `session_id`, using
    /// `helper`.
    pub fn register_with(
        bus: Bus,
        session_id: String,
        helper: Helper,
        ui: impl Fn(AuthEvent) + Send + Sync + 'static,
    ) -> Result<PolkitAgent> {
        if AGENTS.fetch_add(1, Ordering::AcqRel) > 0 && bus_has_agent(&bus) {
            AGENTS.fetch_sub(1, Ordering::AcqRel);
            return Err(Error::InvalidArgument("an agent is already registered on this connection".into()));
        }
        let agent = Agent { ui: Arc::new(ui), helper, sessions: Arc::default() };
        let conn = bus.connection().clone();
        let served = zbus::block_on(async { conn.object_server().at(AGENT_PATH, agent).await });
        if let Err(error) = served {
            AGENTS.fetch_sub(1, Ordering::AcqRel);
            return Err(error.into());
        }
        if let Err(error) = crate::bus::wait_until_serving(&bus, AGENT_PATH) {
            let _ = zbus::block_on(async { conn.object_server().remove::<Agent, _>(AGENT_PATH).await });
            AGENTS.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }
        let locale = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_else(|_| "C".into());
        let registered = bus.call_with::<_, ()>(
            Opts::QUERY,
            POLKIT,
            AUTHORITY_PATH,
            IFACE_AUTHORITY,
            "RegisterAuthenticationAgent",
            &(subject(&session_id), locale, AGENT_PATH),
        );
        if let Err(error) = registered {
            let _ = zbus::block_on(async { conn.object_server().remove::<Agent, _>(AGENT_PATH).await });
            AGENTS.fetch_sub(1, Ordering::AcqRel);
            return Err(match error {
                Error::DBus { name, message } if name.ends_with("Failed") && message.contains("already") => {
                    Error::Unavailable(format!("another authentication agent serves this session: {message}"))
                }
                other => other,
            });
        }
        Ok(PolkitAgent { bus, session_id })
    }
}

fn bus_has_agent(bus: &Bus) -> bool {
    zbus::block_on(async { bus.connection().object_server().interface::<_, Agent>(AGENT_PATH).await.is_ok() })
}

/// polkit's subject for a login session.
fn subject(session_id: &str) -> (&'static str, HashMap<&'static str, Value<'_>>) {
    ("unix-session", HashMap::from([("session-id", Value::from(session_id))]))
}

impl Drop for PolkitAgent {
    fn drop(&mut self) {
        let _ = self.bus.call::<_, ()>(
            POLKIT,
            AUTHORITY_PATH,
            IFACE_AUTHORITY,
            "UnregisterAuthenticationAgent",
            &(subject(&self.session_id), AGENT_PATH),
        );
        let conn = self.bus.connection().clone();
        let _ = zbus::block_on(async move { conn.object_server().remove::<Agent, _>(AGENT_PATH).await });
        AGENTS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_lines() {
        assert_eq!(
            parse_line("PAM_PROMPT_ECHO_OFF Password: "),
            HelperLine::Prompt { text: "Password: ".into(), echo: false }
        );
        assert_eq!(parse_line("PAM_PROMPT_ECHO_ON Login:\n"), HelperLine::Prompt { text: "Login:".into(), echo: true });
        assert_eq!(
            parse_line("PAM_ERROR_MSG Authentication failure"),
            HelperLine::Error("Authentication failure".into())
        );
        assert_eq!(parse_line("PAM_TEXT_INFO Touch the reader"), HelperLine::Info("Touch the reader".into()));
        assert_eq!(parse_line("SUCCESS"), HelperLine::Success);
        assert_eq!(parse_line("FAILURE"), HelperLine::Failure);
        assert!(matches!(parse_line("garbage"), HelperLine::Unknown(_)));
        assert_eq!(parse_line("PAM_TEXT_INFO two\\nlines \\101 \\\\"), HelperLine::Info("two\nlines A \\".into()));
        assert_eq!(compress("trailing\\"), "trailing\\");
    }

    #[test]
    fn identities_resolve_users_and_skip_odd_ones() {
        let uid = |n: u32| HashMap::from([("uid".to_owned(), OwnedValue::from(n))]);
        // SAFETY: getuid has no failure mode.
        let me = unsafe { libc::getuid() };
        let people = identities(&[
            ("unix-user".into(), uid(me)),
            ("unix-user".into(), uid(me)),
            ("unix-netgroup".into(), HashMap::new()),
            ("unix-user".into(), HashMap::new()),
        ]);
        // The sandbox may lack a passwd entry; when it has one, it's there once.
        assert!(people.len() <= 1);
        if let Some(person) = people.first() {
            assert_eq!(person.uid, me);
            assert!(!person.name.is_empty());
        }
    }
}
