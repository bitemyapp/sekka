//! A browser kept to a service's sites, over its DevTools pipe.
//!
//! - **The pipe** (`--remote-debugging-pipe`): the browser reads commands
//!   on its descriptor 3 and answers on its 4, each JSON ended by a NUL.
//!   Only Chanoma holds the other ends, so nothing else on the machine can
//!   steer the browser (there's no port), and the browser closes itself
//!   if Chanoma goes.
//! - **Every page waits as it starts** (auto-attached, waiting for the
//!   debugger) until its documents are asked about here (`Fetch`, for
//!   documents only). The first page is sent to the service from here, so
//!   nothing loads before that.
//! - **A page's own document from elsewhere is refused**, as aborted: the
//!   browser stays on the page it was on, nothing added to its history,
//!   and Chanoma says why. Links, redirects, forms and scripts alike.
//!   Frames in a page load from anywhere (a player, a sign-in box), but a
//!   frame's link out of the page is the page's.
//! - **Where a page is, after**: a page somewhere else anyway (one a
//!   service worker or the back-forward cache answered, a file opened from
//!   the keyboard) goes back in its history to where it was on its sites.
//! - **Other pages** (a tab, a pop-up) are closed, and downloads refused.
//! - **Grown-up mode lets it go anywhere** (signing in can pass through
//!   other sites); kids' mode keeps it again, the page taken back if it's
//!   elsewhere by then.
//! - **Chanoma's keyboard** asks here what has the page's keyboard, in a
//!   world of Chanoma's own in the page that its scripts can't see (and
//!   without enabling the page's runtime for DevTools, which a page can
//!   notice), and types through the browser's own input, so the text
//!   reaches a box in a frame from elsewhere too, whatever the keyboard
//!   layout.

use std::collections::HashMap;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::services::{Sites, host};

/// Chanoma's ends of a browser's DevTools pipe.
pub struct Pipe {
    to: PipeWriter,
    from: PipeReader,
}

/// Start `cmd` with the DevTools pipe on its descriptors 3 and 4.
pub fn spawn(cmd: &mut Command) -> std::io::Result<(Child, Pipe)> {
    // The browser reads `theirs_in`, and writes `theirs_out`.
    let (theirs_in, to) = std::io::pipe()?;
    let (from, theirs_out) = std::io::pipe()?;
    let (r, w) = (theirs_in.as_raw_fd(), theirs_out.as_raw_fd());
    // SAFETY: only descriptor calls between fork and exec, which are
    // async-signal-safe; both descriptors stay open here until the child
    // has started.
    unsafe {
        cmd.pre_exec(move || {
            // Copied above 3 and 4 first, so placing one can't close the
            // other; dup2 leaves the placed ones open across exec.
            let r = libc::fcntl(r, libc::F_DUPFD_CLOEXEC, 10);
            let w = libc::fcntl(w, libc::F_DUPFD_CLOEXEC, 10);
            if r < 0 || w < 0 || libc::dup2(r, 3) < 0 || libc::dup2(w, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    drop((theirs_in, theirs_out));
    Ok((child, Pipe { to, from }))
}

/// What the keeping tells Chanoma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Told {
    /// The page tried to go to this host, outside its sites: it stays.
    Outside(String),
    /// The browser doesn't answer on its pipe, so it can't be kept: it's
    /// left on its blank page.
    Unanswered,
    /// What has the page's keyboard, as [`Ask::Focused`] asked.
    Focused(Focused),
}

/// What Chanoma asks of a page over the pipe, for its keyboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// What has the page's keyboard: told as [`Told::Focused`].
    Focused,
    /// Type this where the page's keyboard is.
    Type(String),
    /// Press a key there.
    Key(Key),
}

/// The keys Chanoma's keyboard presses on a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Backspace,
    Enter,
    Left,
    Right,
}

/// A text box with the page's keyboard.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Field {
    /// What it's for: its label, its placeholder or its name.
    pub label: String,
    /// What's in it; nothing for a password, whose length alone is told.
    pub text: String,
    pub length: usize,
    /// Where typing goes in it, in characters from the start.
    pub caret: usize,
    pub secret: bool,
}

/// What has a page's keyboard.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Focused {
    Field(Field),
    /// Something inside a frame from another site, which isn't asked
    /// (typing still reaches it).
    Frame,
    /// Nothing that takes typing.
    Elsewhere,
}

/// Chanoma's end of the asking: an ask waits on a channel, and a byte down
/// a pipe wakes the keeping to it.
#[derive(Clone)]
pub struct Asker {
    tx: mpsc::Sender<Ask>,
    wake: Arc<PipeWriter>,
}

impl Asker {
    pub fn ask(&self, a: Ask) {
        if self.tx.send(a).is_ok() {
            // Full, the keeping has plenty to wake to already.
            let _ = (&*self.wake).write(&[1]);
        }
    }
}

/// The keeping's end of the asking.
pub struct Asked {
    rx: mpsc::Receiver<Ask>,
    wake: PipeReader,
    /// The waking end held open here too, so it never reads as closed.
    _open: Arc<PipeWriter>,
}

/// A way to ask a page things while it's kept.
pub fn asking() -> std::io::Result<(Asker, Asked)> {
    let (wake_r, wake_w) = std::io::pipe()?;
    for fd in [wake_r.as_raw_fd(), wake_w.as_raw_fd()] {
        // SAFETY: flags on descriptors this owns.
        unsafe { libc::fcntl(fd, libc::F_SETFL, libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK) };
    }
    let (tx, rx) = mpsc::channel();
    let wake_w = Arc::new(wake_w);
    Ok((Asker { tx, wake: wake_w.clone() }, Asked { rx, wake: wake_r, _open: wake_w }))
}

/// The name of the world Chanoma asks a page in.
const WORLD: &str = "chanoma";

/// What has the keyboard, asked in Chanoma's world: through open shadow
/// roots and into frames from the page's own process; a frame from
/// another process says only that it's a frame.
const PROBE: &str = r#"(() => {
  const kinds = ["text", "search", "email", "url", "tel", "password", "number"];
  let e = document.activeElement;
  for (;;) {
    while (e && e.shadowRoot && e.shadowRoot.activeElement) e = e.shadowRoot.activeElement;
    if (!e || (e.tagName !== "IFRAME" && e.tagName !== "FRAME")) break;
    let inner = null;
    try { inner = e.contentDocument; } catch (_) {}
    if (!inner) return { where: "frame" };
    e = inner.activeElement;
  }
  if (!e) return { where: "elsewhere" };
  let kind = "";
  if (e.isContentEditable || e.tagName === "TEXTAREA") kind = "text";
  else if (e.tagName === "INPUT") {
    const t = (e.getAttribute("type") || "text").toLowerCase();
    if (kinds.includes(t)) kind = t;
  }
  if (!kind || e.disabled || e.readOnly) return { where: "elsewhere" };
  const text = e.isContentEditable ? e.innerText : String(e.value);
  let caret = text.length;
  if (typeof e.selectionStart === "number") caret = e.selectionStart;
  const named = e.getAttribute("aria-label") || e.placeholder || (e.labels && e.labels[0] && e.labels[0].innerText) || e.title || e.name || "";
  const secret = kind === "password";
  return { where: "field", label: String(named).trim().slice(0, 80), text: secret ? "" : text.slice(0, 500), length: text.length, caret: Math.min(caret, 500), secret };
})()"#;

/// What a probe's answer says.
fn focused(v: &Value) -> Focused {
    let n = |k: &str| v[k].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    match v["where"].as_str() {
        Some("field") => Focused::Field(Field {
            label: v["label"].as_str().unwrap_or_default().to_owned(),
            text: v["text"].as_str().unwrap_or_default().to_owned(),
            length: n("length"),
            caret: n("caret"),
            secret: v["secret"].as_bool().unwrap_or(false),
        }),
        Some("frame") => Focused::Frame,
        _ => Focused::Elsewhere,
    }
}

/// A key's press and release, as the browser's input takes them.
fn key_events(k: Key) -> [Value; 2] {
    let (key, vk, text) = match k {
        Key::Backspace => ("Backspace", 8, None),
        Key::Enter => ("Enter", 13, Some("\r")),
        Key::Left => ("ArrowLeft", 37, None),
        Key::Right => ("ArrowRight", 39, None),
    };
    let base = json!({ "key": key, "code": key, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk });
    let mut down = base.clone();
    // A key that types says what (Return's carriage return submits a form).
    down["type"] = if text.is_some() { "keyDown" } else { "rawKeyDown" }.into();
    if let Some(t) = text {
        down["text"] = t.into();
        down["unmodifiedText"] = t.into();
    }
    let mut up = base;
    up["type"] = "keyUp".into();
    [down, up]
}

/// How long the browser has to answer on its pipe.
const ANSWER_IN: Duration = Duration::from_secs(30);
/// More times than this a page is taken back in a while and it's left
/// blank instead (its sites lead elsewhere by themselves).
const BACKS: usize = 3;
const BACKS_IN: Duration = Duration::from_secs(10);

/// A page the browser shows.
struct Tab {
    session: Option<String>,
    url: String,
}

struct Keeper {
    pipe: Pipe,
    sites: Sites,
    start: String,
    next: u64,
    pages: HashMap<String, Tab>,
    /// The page the service opens in; any other is a tab or a pop-up.
    main: Option<String>,
    started: bool,
    /// Asked for a page's history (by the answer's id), to take it back.
    histories: HashMap<u64, String>,
    backs: Vec<Instant>,
    answered: bool,
    /// Kept to its sites, as last set.
    kept: bool,
    /// The world Chanoma asks the page in, apart from its own scripts.
    world: Option<i64>,
    /// What answers to Chanoma's asks are for, by id.
    asks: HashMap<u64, Pending>,
}

/// An answer awaited for an ask.
enum Pending {
    /// Chanoma's world made in the page: its id.
    World,
    /// What has the keyboard; asked again in a new world if the page's
    /// document (and the world with it) has gone.
    Probe { again: bool },
}

/// Keep the browser on `pipe` to `sites`, opening `start` in it, while
/// `kept` (kids' mode) and until `stop`, answering what's `asked`. Ends
/// when the browser does.
pub fn keep(
    pipe: Pipe,
    sites: Sites,
    start: String,
    asked: Asked,
    kept: &AtomicBool,
    stop: &AtomicBool,
    told: impl Fn(Told),
) {
    let mut k = Keeper {
        pipe,
        sites,
        start,
        next: 0,
        pages: HashMap::new(),
        main: None,
        started: false,
        histories: HashMap::new(),
        backs: Vec::new(),
        answered: false,
        kept: !kept.load(Ordering::Acquire),
        world: None,
        asks: HashMap::new(),
    };
    k.send(None, "Target.setDiscoverTargets", json!({ "discover": true }));
    let attach = json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true });
    k.send(None, "Target.setAutoAttach", attach);
    let began = Instant::now();
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 1 << 16];
    while !stop.load(Ordering::Acquire) {
        let want = kept.load(Ordering::Acquire);
        if want != k.kept {
            k.set_kept(want);
        }
        while let Ok(a) = asked.rx.try_recv() {
            k.ask(a);
        }
        if !k.answered && began.elapsed() > ANSWER_IN {
            log::warn!("chanoma: the browser doesn't answer on its DevTools pipe; its page can't be kept");
            told(Told::Unanswered);
            return;
        }
        match readable(&k.pipe.from, &asked.wake, Duration::from_millis(250)) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(e) => {
                log::warn!("chanoma: the browser's DevTools pipe: {e}");
                return;
            }
        }
        let n = match k.pipe.from.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => 0,
        };
        if n == 0 {
            // The browser has gone (or never took the pipe).
            if !k.answered {
                told(Told::Unanswered);
            }
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        while let Some(end) = buf.iter().position(|b| *b == 0) {
            let message: Vec<u8> = buf.drain(..=end).collect();
            match serde_json::from_slice::<Value>(&message[..end]) {
                Ok(m) => k.handle(&m, &told),
                Err(e) => log::debug!("chanoma: DevTools said something unreadable: {e}"),
            }
        }
    }
}

/// Whether `pipe` has something to read within `wait`, or an ask woke
/// the keeping first (then false, its bytes taken).
fn readable(pipe: &PipeReader, wake: &PipeReader, wait: Duration) -> std::io::Result<bool> {
    let mut p = [pipe.as_raw_fd(), wake.as_raw_fd()].map(|fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
    let ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: two pollfds, ours, for the length of the call.
    let r = unsafe { libc::poll(p.as_mut_ptr(), 2, ms) };
    if r < 0 {
        let e = std::io::Error::last_os_error();
        return if e.kind() == std::io::ErrorKind::Interrupted { Ok(false) } else { Err(e) };
    }
    if p[1].revents != 0 {
        let mut drained = [0u8; 64];
        while matches!((&*wake).read(&mut drained), Ok(n) if n > 0) {}
    }
    Ok(p[0].revents != 0)
}

impl Keeper {
    fn send(&mut self, session: Option<&str>, method: &str, params: Value) -> u64 {
        self.next += 1;
        let mut m = json!({ "id": self.next, "method": method, "params": params });
        if let Some(s) = session {
            m["sessionId"] = s.into();
        }
        let mut bytes = serde_json::to_vec(&m).unwrap_or_default();
        bytes.push(0);
        // Gone, it says so on the other end (end of file).
        let _ = self.pipe.to.write_all(&bytes);
        self.next
    }

    fn handle(&mut self, m: &Value, told: &impl Fn(Told)) {
        if let Some(id) = m["id"].as_u64() {
            if !self.answered {
                self.answered = true;
                log::info!("chanoma: the browser answers on its DevTools pipe");
            }
            if let Some(e) = m.get("error") {
                log::debug!("chanoma: DevTools refused {id}: {e}");
            }
            if let Some(target) = self.histories.remove(&id) {
                self.take_back(&target, &m["result"]);
            }
            if let Some(p) = self.asks.remove(&id) {
                self.answered_ask(p, m, told);
            }
            return;
        }
        let params = &m["params"];
        match m["method"].as_str().unwrap_or_default() {
            "Target.attachedToTarget" => self.attached(params),
            "Target.targetCreated" | "Target.targetInfoChanged" => self.changed(&params["targetInfo"]),
            "Target.targetDestroyed" => {
                if let Some(id) = params["targetId"].as_str() {
                    self.pages.remove(id);
                }
            }
            "Fetch.requestPaused" => self.paused(params, m["sessionId"].as_str(), told),
            _ => {}
        }
    }

    /// A target, waiting to start: a page asks about its documents first.
    fn attached(&mut self, params: &Value) {
        let Some(session) = params["sessionId"].as_str() else { return };
        let info = &params["targetInfo"];
        if info["type"] == "page"
            && let Some(id) = info["targetId"].as_str()
        {
            let pattern = json!({ "urlPattern": "*", "resourceType": "Document", "requestStage": "Request" });
            self.send(Some(session), "Fetch.enable", json!({ "patterns": [pattern] }));
            let url = info["url"].as_str().unwrap_or_default().to_owned();
            let tab = self.pages.entry(id.to_owned()).or_insert(Tab { session: None, url });
            tab.session = Some(session.to_owned());
            if self.main.is_none() {
                self.main = Some(id.to_owned());
            }
            if self.main.as_deref() == Some(id) && !self.started {
                self.started = true;
                let start = self.start.clone();
                self.send(Some(session), "Page.navigate", json!({ "url": start }));
                log::info!("chanoma: the page opens; its sites are {}", self.sites);
            } else if self.main.as_deref() != Some(id) && self.kept {
                self.close(id);
            }
        }
        self.send(Some(session), "Runtime.runIfWaitingForDebugger", json!({}));
    }

    /// A page's address (or title) changed.
    fn changed(&mut self, info: &Value) {
        if info["type"] != "page" {
            return;
        }
        let (Some(id), Some(url)) = (info["targetId"].as_str(), info["url"].as_str()) else { return };
        let tab = self.pages.entry(id.to_owned()).or_insert(Tab { session: None, url: String::new() });
        if tab.url == url {
            return;
        }
        tab.url = url.to_owned();
        if self.kept {
            self.check(id);
        }
    }

    /// A document asked for: a page's own from elsewhere is refused while
    /// kept, the rest go on.
    fn paused(&mut self, params: &Value, session: Option<&str>, told: &impl Fn(Told)) {
        let Some(request) = params["requestId"].as_str() else { return };
        let url = params["request"]["url"].as_str().unwrap_or_default();
        let own = params["frameId"].as_str().is_some_and(|f| self.pages.contains_key(f));
        if self.kept && own && !self.sites.allows(url) {
            self.send(session, "Fetch.failRequest", json!({ "requestId": request, "errorReason": "Aborted" }));
            let to = host(url).unwrap_or_else(|| url.chars().take(60).collect());
            log::info!("chanoma: the page stays: {to} is outside {}", self.sites);
            told(Told::Outside(to));
        } else {
            self.send(session, "Fetch.continueRequest", json!({ "requestId": request }));
        }
    }

    fn set_kept(&mut self, on: bool) {
        self.kept = on;
        let downloads = if on { "deny" } else { "default" };
        self.send(None, "Browser.setDownloadBehavior", json!({ "behavior": downloads }));
        if on {
            log::info!("chanoma: the page is kept to {}", self.sites);
            let ids: Vec<String> = self.pages.keys().cloned().collect();
            for id in ids {
                self.check(&id);
            }
        } else {
            log::info!("chanoma: the page may go anywhere (grown-up mode)");
        }
    }

    /// While kept: a page that's elsewhere anyway goes back, and any other
    /// page than the service's is closed.
    fn check(&mut self, id: &str) {
        if self.main.as_deref() != Some(id) {
            if self.pages.get(id).is_some_and(|t| t.session.is_some()) {
                self.close(id);
            }
            return;
        }
        let Some(tab) = self.pages.get(id) else { return };
        let Some(session) = tab.session.clone() else { return };
        if self.sites.allows(&tab.url) {
            return;
        }
        log::info!("chanoma: the page is outside its sites ({}): taking it back", host(&tab.url).unwrap_or_default());
        self.backs.retain(|t| t.elapsed() < BACKS_IN);
        self.backs.push(Instant::now());
        if self.backs.len() > BACKS {
            log::warn!("chanoma: the page keeps leaving its sites by itself; leaving it blank");
            self.send(Some(&session), "Page.navigate", json!({ "url": "about:blank" }));
            return;
        }
        let asked = self.send(Some(&session), "Page.getNavigationHistory", json!({}));
        self.histories.insert(asked, id.to_owned());
    }

    /// Back to the last place in a page's history on its sites, or to the
    /// service's start if there's none.
    fn take_back(&mut self, id: &str, history: &Value) {
        let Some(session) = self.pages.get(id).and_then(|t| t.session.clone()) else { return };
        let current = history["currentIndex"].as_u64().and_then(|c| usize::try_from(c).ok()).unwrap_or(0);
        let entries = history["entries"].as_array().map(Vec::as_slice).unwrap_or_default();
        let back = entries.get(..current.min(entries.len())).unwrap_or_default().iter().rev().find(|e| {
            let url = e["url"].as_str().unwrap_or_default();
            url != "about:blank" && self.sites.allows(url)
        });
        match back.and_then(|e| e["id"].as_i64()) {
            Some(entry) => self.send(Some(&session), "Page.navigateToHistoryEntry", json!({ "entryId": entry })),
            None => {
                let start = self.start.clone();
                self.send(Some(&session), "Page.navigate", json!({ "url": start }))
            }
        };
    }

    /// The service's page and its session, once it's there.
    fn main_session(&self) -> Option<(String, String)> {
        let id = self.main.clone()?;
        let session = self.pages.get(&id)?.session.clone()?;
        Some((id, session))
    }

    fn ask(&mut self, a: Ask) {
        let Some((_, session)) = self.main_session() else { return };
        match a {
            Ask::Focused => self.probe(true),
            // As a keyboard's typing is: through the browser's input, to
            // whatever has its keyboard, a frame from elsewhere too.
            Ask::Type(text) => {
                self.send(Some(&session), "Input.insertText", json!({ "text": text }));
            }
            Ask::Key(key) => {
                for e in key_events(key) {
                    self.send(Some(&session), "Input.dispatchKeyEvent", e);
                }
            }
        }
    }

    /// Ask what has the keyboard, in Chanoma's world in the page: made
    /// first if there's none. Neither shows to the page's own scripts, and
    /// the page's runtime isn't enabled for it (which a page can notice).
    fn probe(&mut self, again: bool) {
        let Some((id, session)) = self.main_session() else { return };
        let (method, params, pending) = match self.world {
            Some(ctx) => (
                "Runtime.evaluate",
                json!({ "expression": PROBE, "contextId": ctx, "returnByValue": true }),
                Pending::Probe { again },
            ),
            None => (
                "Page.createIsolatedWorld",
                json!({ "frameId": id, "worldName": WORLD, "grantUniversalAccess": true }),
                Pending::World,
            ),
        };
        let asked = self.send(Some(&session), method, params);
        self.asks.insert(asked, pending);
    }

    fn answered_ask(&mut self, p: Pending, m: &Value, told: &impl Fn(Told)) {
        let result = &m["result"];
        match p {
            Pending::World => {
                self.world = result["executionContextId"].as_i64();
                if self.world.is_some() {
                    self.probe(false);
                }
            }
            // An error, or the page's document gone with the world in it.
            Pending::Probe { again } if m.get("error").is_some() || result.get("exceptionDetails").is_some() => {
                self.world = None;
                if again {
                    self.probe(false);
                }
            }
            Pending::Probe { .. } => told(Told::Focused(focused(&result["result"]["value"]))),
        }
    }

    fn close(&mut self, id: &str) {
        log::info!("chanoma: closing another page the browser opened");
        self.send(None, "Target.closeTarget", json!({ "targetId": id }));
    }
}
