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

use std::collections::HashMap;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
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
}

/// Keep the browser on `pipe` to `sites`, opening `start` in it, while
/// `kept` (kids' mode) and until `stop`. Ends when the browser does.
pub fn keep(pipe: Pipe, sites: Sites, start: String, kept: &AtomicBool, stop: &AtomicBool, told: impl Fn(Told)) {
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
        if !k.answered && began.elapsed() > ANSWER_IN {
            log::warn!("chanoma: the browser doesn't answer on its DevTools pipe; its page can't be kept");
            told(Told::Unanswered);
            return;
        }
        match readable(&k.pipe.from, Duration::from_millis(250)) {
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

/// Whether `pipe` has something to read within `wait`.
fn readable(pipe: &PipeReader, wait: Duration) -> std::io::Result<bool> {
    let mut p = libc::pollfd { fd: pipe.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    let ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: one pollfd, ours, for the length of the call.
    let r = unsafe { libc::poll(&mut p, 1, ms) };
    if r < 0 {
        let e = std::io::Error::last_os_error();
        return if e.kind() == std::io::ErrorKind::Interrupted { Ok(false) } else { Err(e) };
    }
    Ok(r > 0)
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

    fn close(&mut self, id: &str) {
        log::info!("chanoma: closing another page the browser opened");
        self.send(None, "Target.closeTarget", json!({ "targetId": id }));
    }
}
