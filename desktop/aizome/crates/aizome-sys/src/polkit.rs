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
//! root, and relays it line by line as [`AuthEvent::Heard`]: a prompt
//! ([`Said::Prompt`]), PAM's messages, and how each attempt ended. Each
//! event names the conversation it's about, and the UI reads one only
//! after [`AuthRequest::check`] proves it's about the latest
//! ([`conversation::said`]); it answers a prompt with [`Asked::answer`],
//! which goes to the request and the prompt the `Asked` came from, as one
//! line the helper reads whole. When PAM succeeds the helper itself tells
//! polkitd (`AuthenticationAgentResponse2`), prints `SUCCESS`, and the agent
//! completes `BeginAuthentication` ([`AuthEvent::Done`]). Dismissing the
//! dialog is [`AuthRequest::cancel`]; polkitd's `CancelAuthentication` (the
//! application gave up) ends it too.
//!
//! The agent starts the next attempt itself, when its floor allows. Every
//! attempt that ends without success, however it ended (a refused answer,
//! the helper unreachable or stopping before anyone answered, a change of
//! identity), delays the next one: half a second, then 1, 2 and 4 seconds.
//! The floor is the agent's, shared by all its requests, and starts afresh
//! only after a success or a minute without a failure. A refused answer
//! starts the next attempt as soon as the floor allows ([`Said::Refused`]);
//! after three attempts in a row with no answer written it stops
//! ([`Said::Unavailable`]) until the UI asks again. So no UI, whether it
//! answers prompts, asks again or switches identities on every event,
//! makes the agent start helpers faster than one every four seconds once
//! a few have failed. The states and their proofs are in [`conversation`].
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
//! can serve a session. The handle follows polkit's bus owner, registers
//! again after a restart, and retries while another shell still owns the
//! session. Losing the owner cancels its pending conversations.

pub mod conversation;

use crate::bus::{Bus, Opts, value_u64, with_timeout};
use crate::error::{Error, Result};
use crate::oneshot;
use crate::service::lock;

pub use conversation::{Asked, Checked, Heard, MAX_ANSWER, RequestId, Said, Talk, Unsendable, Unsent, said};
use conversation::{ConvAny, Conversation, Converse, Ctx, Dialed, Due, Line, Missed, Reply, SharedFloor, Then, Wake};
use futures_lite::StreamExt;
use std::collections::HashMap;
use std::io::BufRead;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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

/// A person from the password database.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Person {
    uid: u32,
    name: String,
    real_name: String,
}

/// Someone polkitd offered to authenticate one request. Only the request
/// hands these out ([`AuthRequest::identities`]), and
/// [`AuthRequest::authenticate`] takes only its own, so the helper runs PAM
/// only for a person polkitd named for that request.
///
/// ```compile_fail,E0451
/// use aizome_sys::polkit::{AuthRequest, Identity};
/// // Authenticate as root, whom polkitd didn't offer.
/// fn root(request: &AuthRequest, other: &Identity) {
///     let root = Identity { uid: 0, name: "root".into(), real_name: String::new(), request: other.request() };
///     request.authenticate(&root);
/// }
/// ```
///
/// ```
/// use aizome_sys::polkit::AuthRequest;
/// fn first(request: &AuthRequest) {
///     if let Some(identity) = request.identities().first() {
///         request.authenticate(identity);
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    uid: u32,
    name: String,
    real_name: String,
    request: RequestId,
}

impl Identity {
    fn offered(person: Person, request: RequestId) -> Identity {
        Identity { uid: person.uid, name: person.name, real_name: person.real_name, request }
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// The login name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The real name from the password database's GECOS field.
    pub fn real_name(&self) -> &str {
        &self.real_name
    }

    pub fn display_name(&self) -> &str {
        if self.real_name.is_empty() { &self.name } else { &self.real_name }
    }

    /// The request that offered it.
    pub fn request(&self) -> RequestId {
        self.request
    }
}

/// What the dialog shows and does.
#[derive(Clone, Debug)]
pub enum AuthEvent {
    /// Show the dialog for this request.
    Begin(AuthRequest),
    /// The request's conversation said something: read it after
    /// [`AuthRequest::check`].
    Heard(Heard),
    /// It is over (authenticated, cancelled or abandoned): close the
    /// dialog for `request`.
    Done { cookie: String, authorized: bool, request: RequestId },
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
    identities: Vec<Identity>,
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
    /// Who may authenticate, as polkitd named them for this request;
    /// [`default_identity`](AuthRequest::default_identity) suggests one.
    pub fn identities(&self) -> &[Identity] {
        &self.identities
    }

    /// The identity to preselect: the person at the screen if they may
    /// authenticate, else the first one.
    pub fn default_identity(&self) -> Option<&Identity> {
        // SAFETY: getuid has no failure mode.
        let me = unsafe { libc::getuid() };
        self.identities.iter().find(|i| i.uid == me).or_else(|| self.identities.first())
    }

    /// Start the conversation as `identity`: the first, another identity's,
    /// or one more after [`Said::Unavailable`]. The agent starts it when its
    /// floor allows (at once, unless attempts have been ending with no
    /// answer); a call while an attempt waits for the floor changes only
    /// whom it's for. An identity another request offered is refused: the
    /// helper runs PAM only for someone polkitd named for this request.
    pub fn authenticate(&self, identity: &Identity) {
        if identity.request != self.session.id {
            log::warn!("aizome-sys: polkit: refused an identity another request offered");
            return;
        }
        let user = identity.name.clone();
        self.session.step(move |state| (start(state, user), ()));
    }

    /// The proof that `heard` is about this request's latest conversation,
    /// or the event back when it's another request's, or about one a newer
    /// conversation superseded. Read it with [`said`].
    pub fn check(&self, heard: Heard) -> std::result::Result<Checked, Heard> {
        lock(&self.session.inner).conversation.check_heard(heard)
    }

    /// The person dismissed the dialog.
    pub fn cancel(&self) {
        self.session.cancel();
    }

    /// Which request this is, as [`AuthEvent::Done`] names it.
    pub fn id(&self) -> RequestId {
        self.session.id
    }
}

type Ui = Arc<dyn Fn(AuthEvent) + Send + Sync>;

/// One `BeginAuthentication` in progress.
struct Session {
    cookie: String,
    id: RequestId,
    ui: Ui,
    helper: Helper,
    inner: Mutex<SessionInner>,
}

struct SessionInner {
    conversation: Conversation,
    /// A thread is handing the UI what the conversation said.
    telling: bool,
}

/// Whether a helper's line was about the latest conversation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Heeded {
    Current,
    Stale,
}

/// The UI asked for an attempt as `user`.
fn start(state: ConvAny<'_>, user: String) -> conversation::Step<'_> {
    match state {
        ConvAny::Idle(s) => s.start(user),
        ConvAny::Dialing(s) => s.start(user),
        ConvAny::Starting(s) => s.start(user),
        ConvAny::Prompted(s) => s.start(user),
        ConvAny::Checking(s) => s.start(user),
        ConvAny::Cooling(s) => (s.start(user).into(), Then::Nothing),
        ConvAny::Unavailable(s) => s.start(user),
        ConvAny::Over(s) => (s.into(), Then::Nothing),
    }
}

/// The UI answered a prompt.
fn answer(state: ConvAny<'_>, reply: Reply) -> conversation::Step<'_> {
    let Ok(now) = state.check(reply) else { return (state, Then::Nothing) };
    let next = match state {
        ConvAny::Prompted(s) => match s.answer(now) {
            Ok((checking, answering)) => return (checking.into(), Then::Answer(answering)),
            Err(prompted) => prompted.into(),
        },
        // The conversation's ticket, but its prompt is answered already.
        s @ (ConvAny::Idle(_)
        | ConvAny::Dialing(_)
        | ConvAny::Starting(_)
        | ConvAny::Checking(_)
        | ConvAny::Cooling(_)
        | ConvAny::Unavailable(_)
        | ConvAny::Over(_)) => s,
    };
    (next, Then::Nothing)
}

/// The helper wrote a line, or its output ended.
fn hear(state: ConvAny<'_>, line: Line) -> (conversation::Step<'_>, Heeded) {
    let Ok(now) = state.check(line) else { return ((state, Then::Nothing), Heeded::Stale) };
    let (next, then) = match state {
        ConvAny::Starting(s) => s.heard(now),
        ConvAny::Prompted(s) => s.heard(now),
        ConvAny::Checking(s) => s.heard(now),
        // A current line names a running helper, so these never see one.
        s @ (ConvAny::Idle(_)
        | ConvAny::Dialing(_)
        | ConvAny::Cooling(_)
        | ConvAny::Unavailable(_)
        | ConvAny::Over(_)) => (s, Then::Nothing),
    };
    ((next, then), Heeded::Current)
}

/// Starting a helper went one way or the other.
fn dialed(state: ConvAny<'_>, dialed: Dialed) -> conversation::Step<'_> {
    let now = match state.check(dialed) {
        Ok(now) => now,
        Err(stale) => return (state, Then::Abandon(stale)),
    };
    match state {
        ConvAny::Dialing(s) => s.dialed(now),
        // A current connection names the attempt being started.
        s @ (ConvAny::Idle(_)
        | ConvAny::Starting(_)
        | ConvAny::Prompted(_)
        | ConvAny::Checking(_)
        | ConvAny::Cooling(_)
        | ConvAny::Unavailable(_)
        | ConvAny::Over(_)) => (s, Then::Abandon(now.into_answer())),
    }
}

/// A retry's timer went off.
fn due(state: ConvAny<'_>, due: Due) -> conversation::Step<'_> {
    let Ok(now) = state.check(due) else { return (state, Then::Nothing) };
    match state {
        ConvAny::Cooling(s) => s.due(now),
        // A current timer names the planned attempt, which may be starting.
        s @ (ConvAny::Idle(_)
        | ConvAny::Dialing(_)
        | ConvAny::Starting(_)
        | ConvAny::Prompted(_)
        | ConvAny::Checking(_)
        | ConvAny::Unavailable(_)
        | ConvAny::Over(_)) => (s, Then::Nothing),
    }
}

/// A retry's timer couldn't be set.
fn missed(state: ConvAny<'_>, missed: Missed, why: String) -> conversation::Step<'_> {
    let Ok(now) = state.check(missed) else { return (state, Then::Nothing) };
    match state {
        ConvAny::Cooling(s) => s.missed(now, why),
        // A current timer names the planned attempt, which waits in Cooling.
        s @ (ConvAny::Idle(_)
        | ConvAny::Dialing(_)
        | ConvAny::Starting(_)
        | ConvAny::Prompted(_)
        | ConvAny::Checking(_)
        | ConvAny::Unavailable(_)
        | ConvAny::Over(_)) => (s, Then::Nothing),
    }
}

/// The request ends, unauthorized.
fn cancel(state: ConvAny<'_>) -> conversation::Step<'_> {
    let over = match state {
        ConvAny::Idle(s) => s.cancel(),
        ConvAny::Dialing(s) => s.cancel(),
        ConvAny::Starting(s) => s.cancel(),
        ConvAny::Prompted(s) => s.cancel(),
        ConvAny::Checking(s) => s.cancel(),
        ConvAny::Cooling(s) => s.cancel(),
        ConvAny::Unavailable(s) => s.cancel(),
        ConvAny::Over(s) => s,
    };
    (over.into(), Then::Nothing)
}

impl Session {
    /// Runs one step of the conversation under its lock, then, with the lock
    /// released, tells the UI what it said and does the work it left
    /// (starting a helper, writing an answer, a thread, a timer). Nothing
    /// waits on I/O under the lock, and a proof the step checked
    /// ([`conversation::Now`]) can't outlive it.
    fn step<R>(self: &Arc<Self>, f: impl for<'s> FnOnce(ConvAny<'s>) -> ((ConvAny<'s>, Then), R)) -> R {
        let (then, r) = lock(&self.inner).conversation.with_typed(|state| {
            let ((next, then), r) = f(state);
            (next, (then, r))
        });
        self.tell();
        self.carry_out(then);
        r
    }

    fn cancel(self: &Arc<Self>) {
        self.step(|state| (cancel(state), ()));
    }

    /// An answer to one of this session's prompts ([`Asked::answer`]).
    fn answer(self: &Arc<Self>, reply: Reply) {
        self.step(move |state| (answer(state, reply), ()));
    }

    /// Hands the UI what the conversation said, in order, without the lock:
    /// one thread at a time, so events reach the UI in the order the steps
    /// made them, and a UI that answers inside its callback only queues.
    fn tell(&self) {
        {
            let mut inner = lock(&self.inner);
            if inner.telling {
                return;
            }
            inner.telling = true;
        }
        // If the UI's callback panics, the next step tells again.
        struct Unstick<'a>(&'a Session);
        impl Drop for Unstick<'_> {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    lock(&self.0.inner).telling = false;
                }
            }
        }
        let _unstick = Unstick(self);
        loop {
            let next = {
                let mut inner = lock(&self.inner);
                let next = inner.conversation.next_event();
                if next.is_none() {
                    inner.telling = false;
                }
                next
            };
            match next {
                Some(event) => (self.ui)(event),
                None => return,
            }
        }
    }

    fn carry_out(self: &Arc<Self>, then: Then) {
        match then {
            Then::Nothing => {}
            Then::Dial(dial) => {
                let result = dial.connect(&self.helper, &self.cookie);
                self.step(move |state| (dialed(state, result), ()));
            }
            Then::Wake(wake) => self.wake(wake),
            Then::Converse(conversation) => self.converse(conversation),
            Then::Abandon(dialed) => dialed.abandon(),
            Then::Answer(answering) => answering.send(),
        }
    }

    /// The planned attempt starts when its timer goes off (if the request
    /// is still there and the attempt still wanted).
    fn wake(self: &Arc<Self>, wake: Wake) {
        let session = Arc::downgrade(self);
        let missed_timer = wake.missed();
        let spawned = std::thread::Builder::new().name("aizome-sys polkit retry".into()).spawn(move || {
            std::thread::sleep(wake.at().saturating_duration_since(Instant::now()));
            if let Some(session) = session.upgrade() {
                let fired = wake.fire();
                session.step(move |state| (due(state, fired), ()));
            }
        });
        if let Err(error) = spawned {
            // No thread, so no retry: say so, rather than wait for ever.
            self.step(move |state| (missed(state, missed_timer, format!("no thread for the retry: {error}")), ()));
        }
    }

    /// Reads the helper on a thread of its own.
    fn converse(self: &Arc<Self>, conversation: Converse) {
        let ended = conversation.ended();
        let session = self.clone();
        let spawned =
            std::thread::Builder::new().name("aizome-sys polkit".into()).spawn(move || session.relay(conversation));
        if let Err(error) = spawned {
            log::warn!("aizome-sys: polkit helper: {error}");
            self.step(move |state| hear(state, ended));
        }
    }

    /// Relays the helper's lines until its output ends or a newer
    /// conversation supersedes it, then ends it.
    fn relay(self: Arc<Session>, conversation: Converse) {
        let (lines, reader, ender) = conversation.into_parts();
        let mut heeded = Heeded::Current;
        for text in reader.lines() {
            let Ok(text) = text else { break };
            let line = lines.line(&text);
            heeded = self.step(move |state| hear(state, line));
            match heeded {
                Heeded::Current => {}
                Heeded::Stale => break,
            }
        }
        match heeded {
            Heeded::Current => {
                let line = lines.ended();
                self.step(move |state| hear(state, line));
            }
            Heeded::Stale => {}
        }
        ender.end();
    }
}

/// Ends a helper from outside its thread.
#[derive(Clone)]
pub(crate) enum Ender {
    Child(Arc<ChildSlot>),
    Stream(Arc<UnixStream>),
}

/// The setuid helper, until its thread reaps it.
pub(crate) struct ChildSlot(Mutex<Option<Child>>);

impl Drop for ChildSlot {
    fn drop(&mut self) {
        // Nothing reaped it (its thread never ran): don't leave a zombie.
        if let Some(mut child) = self.0.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Ender {
    /// Stops the helper, without waiting: its thread sees its output end.
    fn stop(&self) {
        match self {
            Ender::Child(slot) => {
                if let Some(child) = lock(&slot.0).as_mut() {
                    let _ = child.kill();
                }
            }
            Ender::Stream(stream) => {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    /// Stops the helper and reaps it (its own thread, outside any lock).
    fn end(&self) {
        match self {
            Ender::Child(slot) => {
                let child = lock(&slot.0).take();
                if let Some(mut child) = child {
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
fn user_by_uid(uid: u32) -> Option<Person> {
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
    Some(Person { uid, name: text(pwd.pw_name), real_name: gecos.split(',').next().unwrap_or("").to_owned() })
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

fn user_by_name(name: &str) -> Option<Person> {
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
fn identities(raw: &[(String, HashMap<String, OwnedValue>)]) -> Vec<Person> {
    let mut found: Vec<Person> = Vec::new();
    for (kind, details) in raw {
        let id = details.get("uid").or_else(|| details.get("gid")).and_then(|v| value_u64(v));
        let Some(id) = id.and_then(|n| u32::try_from(n).ok()) else { continue };
        let people: Vec<Person> = match kind.as_str() {
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
    state: Arc<Mutex<AgentState>>,
    /// When the next helper may start, for every request.
    floor: SharedFloor,
}

#[derive(Default)]
struct AgentState {
    owner: Option<String>,
    sessions: HashMap<String, Arc<Session>>,
}

impl AgentState {
    fn check_sender(&self, header: &zbus::message::Header<'_>) -> Result<(), PolkitError> {
        if self.owner.as_deref().is_some_and(|owner| header.sender().is_some_and(|sender| sender == owner)) {
            Ok(())
        } else {
            Err(PolkitError::Failed("The caller is not the current polkit authority".into()))
        }
    }
}

/// Close old dialogs before allowing requests from a new authority.
fn change_owner(state: &Mutex<AgentState>, owner: Option<String>) {
    let sessions = {
        let mut state = lock(state);
        state.owner = None;
        std::mem::take(&mut state.sessions)
    };
    for session in sessions.into_values() {
        session.cancel();
    }
    lock(state).owner = owner;
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum PolkitError {
    Failed(String),
    Cancelled(String),
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    // Polkit fixes the D-Bus arguments; the header is needed to validate its sender.
    #[allow(clippy::too_many_arguments)]
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities_raw: Vec<(String, HashMap<String, OwnedValue>)>,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> Result<(), PolkitError> {
        let people = identities(&identities_raw);
        if people.is_empty() {
            return Err(PolkitError::Failed("No one may authenticate for this".into()));
        }
        let (tx, rx) = oneshot::channel();
        let id = RequestId::next();
        let session = Arc::new_cyclic(|session| Session {
            cookie: cookie.clone(),
            id,
            ui: self.ui.clone(),
            helper: self.helper.clone(),
            inner: Mutex::new(SessionInner {
                conversation: Conversation::new(Ctx::new(id, cookie.clone(), tx, self.floor.clone(), session.clone())),
                telling: false,
            }),
        });
        {
            let mut state = lock(&self.state);
            state.check_sender(&header)?;
            if state.sessions.contains_key(&cookie) {
                return Err(PolkitError::Failed("An authentication with this cookie is already pending".into()));
            }
            state.sessions.insert(cookie.clone(), session.clone());
            // Queued first, so the UI hears of the request before its end,
            // and told once the agent's lock is released.
            lock(&session.inner).conversation.begun(AuthRequest {
                cookie: cookie.clone(),
                action_id,
                message,
                icon_name,
                details,
                identities: people.into_iter().map(|person| Identity::offered(person, id)).collect(),
                session: session.clone(),
            });
        }
        session.tell();
        let authorized = rx.await;
        let mut state = lock(&self.state);
        if state.sessions.get(&cookie).is_some_and(|current| Arc::ptr_eq(current, &session)) {
            state.sessions.remove(&cookie);
        }
        match authorized {
            Some(true) => Ok(()),
            _ => Err(PolkitError::Cancelled("Authentication was cancelled".into())),
        }
    }

    fn cancel_authentication(
        &self,
        cookie: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> Result<(), PolkitError> {
        let session = {
            let state = lock(&self.state);
            state.check_sender(&header)?;
            state.sessions.get(&cookie).cloned()
        };
        match session {
            Some(session) => {
                session.cancel();
                Ok(())
            }
            None => Err(PolkitError::Failed(format!("No authentication with cookie {cookie}"))),
        }
    }
}

/// A session's agent and its registration worker. Dropping it stops the
/// worker, cancels pending prompts, and unregisters from its authority.
pub struct PolkitAgent {
    stop: Option<oneshot::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for PolkitAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolkitAgent").finish_non_exhaustive()
    }
}

const REGISTER_RETRY: Duration = Duration::from_secs(2);

impl PolkitAgent {
    /// Serve the agent for this login session and keep it registered.
    /// Registration is asynchronous: an absent or restarting polkit, or
    /// a previous shell still registered, does not disable this agent.
    /// Setup failures retry, and a lost bus connection is replaced. The
    /// returned handle owns all recovery work; dropping it stops retries.
    pub fn register(ui: impl Fn(AuthEvent) + Send + Sync + 'static) -> Result<PolkitAgent> {
        Self::reconnecting(ui, || {
            // The process-wide bus cache can retain a disconnected connection.
            // This agent owns a fresh connection on every recovery attempt.
            let conn = zbus::block_on(with_timeout(crate::bus::TIMEOUT, zbus::connection::Builder::system()?.build()))
                .ok_or(Error::Timeout)??;
            let bus = Bus::from_connection(conn);
            let helper = Helper::find().ok_or_else(|| Error::Unavailable("polkit's agent helper".into()))?;
            let session_id =
                crate::power::session_id(&bus).ok_or_else(|| Error::NotFound("no login session".into()))?;
            Ok((bus, session_id, helper))
        })
    }

    fn reconnecting(
        ui: impl Fn(AuthEvent) + Send + Sync + 'static,
        mut connect: impl FnMut() -> Result<(Bus, String, Helper)> + Send + 'static,
    ) -> Result<PolkitAgent> {
        let ui: Ui = Arc::new(ui);
        Self::spawn(move |mut stopped| {
            loop {
                let setup = connect().and_then(|(bus, session_id, helper)| {
                    serve(&bus, helper, ui.clone()).map(|(state, changes)| (bus, session_id, state, changes))
                });
                match setup {
                    Ok((bus, session_id, state, changes)) => {
                        if maintain(bus, session_id, state, changes, &mut stopped) {
                            break;
                        }
                    }
                    Err(error) => log::warn!("aizome-sys: polkit agent setup deferred: {error}"),
                }
                // Both initial setup failures and a closed bus are recoverable.
                // Dropping the handle interrupts this wait as well as maintain.
                if zbus::block_on(with_timeout(REGISTER_RETRY, &mut stopped)).is_some() {
                    break;
                }
            }
        })
    }

    /// Serve and maintain the agent on `bus` for `session_id`, using `helper`.
    /// Unlike [`register`](Self::register), a supplied bus cannot be recreated;
    /// losing it ends this worker and makes [`is_running`](Self::is_running) false.
    pub fn register_with(
        bus: Bus,
        session_id: String,
        helper: Helper,
        ui: impl Fn(AuthEvent) + Send + Sync + 'static,
    ) -> Result<PolkitAgent> {
        let (state, changes) = serve(&bus, helper, Arc::new(ui))?;
        let worker_bus = bus.clone();
        match Self::spawn(move |mut stopped| {
            maintain(worker_bus, session_id, state, changes, &mut stopped);
        }) {
            Ok(agent) => Ok(agent),
            Err(error) => {
                let _ = zbus::block_on(bus.connection().object_server().remove::<Agent, _>(AGENT_PATH));
                Err(error)
            }
        }
    }

    fn spawn(run: impl FnOnce(oneshot::Receiver<()>) + Send + 'static) -> Result<PolkitAgent> {
        let (stop, stopped) = oneshot::channel();
        let worker =
            std::thread::Builder::new().name("aizome-sys polkit registration".into()).spawn(move || run(stopped))?;
        Ok(PolkitAgent { stop: Some(stop), worker: Some(worker) })
    }

    /// Whether the worker is still serving or recovering its connection.
    pub fn is_running(&self) -> bool {
        self.worker.as_ref().is_some_and(|worker| !worker.is_finished())
    }
}

fn serve(bus: &Bus, helper: Helper, ui: Ui) -> Result<(Arc<Mutex<AgentState>>, zbus::MessageStream)> {
    // Subscribe before querying the owner or registering. The stream
    // owns its match rule and releases it when the worker stops.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .add_arg(POLKIT)?
        .build();
    let changes = zbus::block_on(with_timeout(
        crate::bus::TIMEOUT,
        zbus::MessageStream::for_match_rule(rule, bus.connection(), Some(64)),
    ))
    .ok_or(Error::Timeout)??;
    let state = Arc::new(Mutex::new(AgentState::default()));
    let agent = Agent { ui, helper, state: state.clone(), floor: SharedFloor::default() };
    // ObjectServer::at atomically refuses an existing interface; a
    // second handle must never remove the first one's served object.
    if !zbus::block_on(bus.connection().object_server().at(AGENT_PATH, agent))? {
        return Err(Error::InvalidArgument("an agent is already registered on this connection".into()));
    }
    if let Err(error) = crate::bus::wait_until_serving(bus, AGENT_PATH) {
        let _ = zbus::block_on(bus.connection().object_server().remove::<Agent, _>(AGENT_PATH));
        return Err(error);
    }
    Ok((state, changes))
}

/// True when the handle was dropped; false when its bus disconnected.
fn maintain(
    bus: Bus,
    session_id: String,
    state: Arc<Mutex<AgentState>>,
    mut changes: zbus::MessageStream,
    stopped: &mut oneshot::Receiver<()>,
) -> bool {
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "C".into());
    let mut owner = bus.owner(POLKIT);
    change_owner(&state, owner.clone());
    let mut registered = false;
    let mut cleanup_pending = false;
    let mut warned = false;
    let stopping = loop {
        if owner.is_none() && bus.activatable(POLKIT) {
            let _ = bus.start(POLKIT, crate::bus::TIMEOUT);
            owner = bus.owner(POLKIT);
            change_owner(&state, owner.clone());
        }
        if let Some(dest) = owner.as_deref().filter(|_| !registered) {
            if cleanup_pending {
                cleanup_pending = unregister(&bus, dest, &session_id).is_err();
            }
            // Bind the call to this daemon, not the well-known name: a
            // reply or cleanup must never affect its replacement.
            match bus.call_with::<_, ()>(
                Opts::QUERY,
                dest,
                AUTHORITY_PATH,
                IFACE_AUTHORITY,
                "RegisterAuthenticationAgent",
                &(subject(&session_id), &locale, AGENT_PATH),
            ) {
                Ok(()) => {
                    registered = true;
                    cleanup_pending = false;
                    log::info!("aizome-sys: polkit agent registered");
                }
                Err(error) => {
                    // The daemon may have registered us but lost its reply.
                    // Remove only this connection's registration at that exact
                    // authority before retrying, or every retry could collide.
                    if matches!(error, Error::Timeout) {
                        cleanup_pending = unregister(&bus, dest, &session_id).is_err();
                    }
                    if !warned {
                        log::warn!("aizome-sys: polkit registration deferred: {error}");
                        warned = true;
                    }
                }
            }
        }
        enum Next {
            Stop,
            Disconnected,
            Changed,
            Retry,
        }
        let next = zbus::block_on(futures_lite::future::or(
            async {
                (&mut *stopped).await;
                Next::Stop
            },
            async {
                let next = async {
                    match changes.next().await {
                        Some(Ok(_)) => Next::Changed,
                        _ => Next::Disconnected,
                    }
                };
                if !registered { with_timeout(REGISTER_RETRY, next).await.unwrap_or(Next::Retry) } else { next.await }
            },
        ));
        match next {
            Next::Stop => break true,
            Next::Disconnected => break false,
            Next::Retry | Next::Changed => {
                // Reconcile queued signals with the current owner. An
                // initial snapshot can already include a queued change.
                let current = bus.owner(POLKIT);
                if current != owner {
                    change_owner(&state, current.clone());
                    owner = current;
                    registered = false;
                    cleanup_pending = false;
                    warned = false;
                }
            }
        }
    };
    change_owner(&state, None);
    if (registered || cleanup_pending)
        && let Some(owner) = owner
    {
        let _ = unregister(&bus, &owner, &session_id);
    }
    let _ = zbus::block_on(bus.connection().object_server().remove::<Agent, _>(AGENT_PATH));
    stopping
}

fn unregister(bus: &Bus, owner: &str, session_id: &str) -> Result<()> {
    bus.call::<_, ()>(
        owner,
        AUTHORITY_PATH,
        IFACE_AUTHORITY,
        "UnregisterAuthenticationAgent",
        &(subject(session_id), AGENT_PATH),
    )
}

/// polkit's subject for a login session.
fn subject(session_id: &str) -> (&'static str, HashMap<&'static str, Value<'_>>) {
    ("unix-session", HashMap::from([("session-id", Value::from(session_id))]))
}

impl Drop for PolkitAgent {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    #[test]
    fn setup_failure_and_bus_loss_reconnect_until_the_handle_is_dropped() {
        struct Daemon(Child, String);
        impl Daemon {
            fn start() -> Option<Self> {
                let mut child = Command::new("dbus-daemon")
                    .args(["--session", "--nofork", "--print-address"])
                    .stdout(Stdio::piped())
                    .spawn()
                    .ok()?;
                let mut address = String::new();
                BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
                // A build sandbox has the program but no /etc/dbus-1 for
                // --session to read: the daemon exits without an address.
                if address.trim().is_empty() {
                    let _ = child.wait();
                    return None;
                }
                Some(Self(child, address.trim().into()))
            }
        }
        impl Drop for Daemon {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let Some(first) = Daemon::start() else {
            eprintln!("skipping: no dbus-daemon");
            return;
        };
        let second = Daemon::start().unwrap();
        let (first_address, second_address) = (first.1.clone(), second.1.clone());
        let (tx, rx) = mpsc::channel();
        let mut attempt = 0;
        let agent = PolkitAgent::reconnecting(
            |_| {},
            move || {
                attempt += 1;
                if attempt == 1 || attempt == 3 {
                    return Err(Error::Unavailable("bus not ready".into()));
                }
                let bus = Bus::at(if attempt == 2 { &first_address } else { &second_address })?;
                tx.send(bus.clone()).unwrap();
                Ok((bus, "7".into(), Helper::Program("/unused".into())))
            },
        )
        .unwrap();
        let wait_serving = |bus: &Bus| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while zbus::block_on(bus.connection().object_server().interface::<_, Agent>(AGENT_PATH)).is_err() {
                assert!(std::time::Instant::now() < deadline, "agent never served");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let first_bus = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        wait_serving(&first_bus);
        drop(first);
        let second_bus = rx.recv_timeout(Duration::from_secs(7)).unwrap();
        wait_serving(&second_bus);
        assert!(agent.is_running());
        drop(agent);
        assert!(zbus::block_on(second_bus.connection().object_server().interface::<_, Agent>(AGENT_PATH)).is_err());
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

        // Setup retries are owned too: dropping during the retry wait ends it.
        let (tx, rx) = mpsc::channel();
        let agent = PolkitAgent::reconnecting(
            |_| {},
            move || {
                tx.send(()).unwrap();
                Err(Error::Unavailable("still unavailable".into()))
            },
        )
        .unwrap();
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let started = std::time::Instant::now();
        drop(agent);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

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
