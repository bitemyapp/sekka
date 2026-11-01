//! Services, open in a browser Chanoma starts and ends.
//!
//! - **One profile a service** (`browser/<id>` in Chanoma's data), so a
//!   grown-up signs in once and each service keeps its own sign-in,
//!   separate from the grown-ups' own browsing.
//! - **Full screen, nothing else**: kiosk mode, no address bar, tabs or
//!   menus. A window the page opens (a pop-up) is closed at once.
//! - **The controller drives it** through the compositor ([`Bridge`]):
//!   arrows are arrow keys; on ordinary pages the left stick moves the
//!   pointer and the confirm button clicks, the right stick scrolls; on a
//!   television interface the confirm button is Return. Back is Escape, or
//!   held, the browser's Back.
//! - **Ending it** asks its window to close, then stops the browser if it
//!   hasn't gone in a few seconds.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aizome_ipc::{Request, Response, Synthetic, WindowId};

use crate::pad::{Axis, SLOTS, Shared};
use crate::services::Drive;

/// Browsers looked for, best first.
const BROWSERS: &[&str] = &["google-chrome-stable", "google-chrome", "chromium", "chromium-browser"];

/// The browser to use: the grown-ups' choice, else the first found.
pub fn find(choice: Option<&str>) -> Option<PathBuf> {
    // The grown-ups' choice, else the one the system was given
    // (`programs.aizome.chanoma.browser`), else the first found.
    let given = std::env::var("CHANOMA_BROWSER").ok();
    if let Some(c) = choice.or(given.as_deref()).filter(|c| !c.trim().is_empty()) {
        return which(c.trim());
    }
    BROWSERS.iter().find_map(|b| which(b))
}

/// Where programs are looked for: the PATH, then where NixOS installs
/// them, which a session's unit (started by the user's systemd, with its
/// own short PATH) wouldn't otherwise see.
fn search_path() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Ok(user) = std::env::var("USER") {
        dirs.push(PathBuf::from(format!("/etc/profiles/per-user/{user}/bin")));
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".nix-profile/bin"));
    }
    dirs.extend(["/run/current-system/sw/bin", "/usr/bin"].map(PathBuf::from));
    dirs
}

fn which(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_owned());
    }
    search_path().into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// The browser's arguments for `url` with its profile at `profile`.
pub fn arguments(profile: &Path, url: &str, agent: Option<&str>) -> Vec<String> {
    let mut args = vec![
        format!("--user-data-dir={}", profile.display()),
        "--kiosk".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--noerrdialogs".into(),
        "--hide-crash-restore-bubble".into(),
        "--disable-translate".into(),
        "--disable-features=Translate,TranslateUI,MediaRouter,DialMediaRouteProvider,GlobalMediaControls".into(),
        "--autoplay-policy=no-user-gesture-required".into(),
        // No keyring to unlock: sign-ins are kept in the profile.
        "--password-store=basic".into(),
        "--ozone-platform-hint=auto".into(),
    ];
    if let Some(a) = agent {
        args.push(format!("--user-agent={a}"));
    }
    args.push(url.into());
    args
}

/// What the browser did.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Its window is showing.
    Shown(WindowId),
    /// It has gone (closed, crashed, or never started).
    Gone,
}

/// What a press means on a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Ok,
    Back,
    Up,
    Down,
    Left,
    Right,
    PlayPause,
    FullScreen,
    NextControl,
    PreviousControl,
}

/// A connection to the compositor that sends input: the pointer's place
/// is kept here, moved by the sticks on a thread of its own.
pub struct Bridge {
    tx: mpsc::Sender<Vec<Synthetic>>,
    stop: Arc<AtomicBool>,
    /// The pointer moved since the keyboard was last used to move between
    /// a page's controls.
    pointing: Arc<AtomicBool>,
    back_down: Mutex<Option<Instant>>,
    drive: Drive,
}

/// Held this long, Back is the browser's Back.
const LONG_BACK: Duration = Duration::from_millis(700);
/// Points a second at full lean.
const POINTER_SPEED: f64 = 1100.0;
/// Points a second of scrolling at full lean.
const SCROLL_SPEED: f64 = 1600.0;

fn key(name: &str, pressed: bool) -> Synthetic {
    Synthetic::Key { key: name.into(), pressed }
}

impl Bridge {
    pub fn new(drive: Drive, pads: Arc<Shared>) -> Bridge {
        let (tx, rx) = mpsc::channel::<Vec<Synthetic>>();
        let stop = Arc::new(AtomicBool::new(false));
        // On an ordinary page the confirm button clicks from the start.
        let pointing = Arc::new(AtomicBool::new(drive == Drive::Pointer));
        let (s, p) = (stop.clone(), pointing.clone());
        let _ = std::thread::Builder::new().name("chanoma-bridge".into()).spawn(move || bridge(rx, s, p, pads, drive));
        Bridge { tx, stop, pointing, back_down: Mutex::new(None), drive }
    }

    fn send(&self, events: Vec<Synthetic>) {
        let _ = self.tx.send(events);
    }

    /// A press (or release) of what `action` is on a page.
    pub fn act(&self, action: Action, pressed: bool) {
        let arrow = |name: &str| self.send(vec![key(name, pressed)]);
        match action {
            Action::Up => arrow("up"),
            Action::Down => arrow("down"),
            Action::Left => arrow("left"),
            Action::Right => arrow("right"),
            Action::Ok => {
                let click = self.drive == Drive::Pointer && self.pointing.load(Ordering::Acquire);
                if click {
                    self.send(vec![Synthetic::PointerButton { button: "left".into(), pressed }]);
                } else {
                    arrow("return");
                }
            }
            Action::Back => {
                let mut down = self.back_down.lock().unwrap_or_else(|e| e.into_inner());
                if pressed {
                    *down = Some(Instant::now());
                } else if let Some(at) = down.take() {
                    if at.elapsed() >= LONG_BACK {
                        self.send(vec![Synthetic::Chord { keys: "alt+left".into() }]);
                    } else {
                        self.send(vec![key("escape", true), key("escape", false)]);
                    }
                }
            }
            Action::PlayPause if pressed => self.send(vec![key("space", true), key("space", false)]),
            Action::FullScreen if pressed => self.send(vec![key("f", true), key("f", false)]),
            Action::NextControl if pressed => {
                self.pointing.store(false, Ordering::Release);
                self.send(vec![key("tab", true), key("tab", false)]);
            }
            Action::PreviousControl if pressed => {
                self.pointing.store(false, Ordering::Release);
                self.send(vec![Synthetic::Chord { keys: "shift+tab".into() }]);
            }
            _ => {}
        }
    }

    /// Let go of everything held (a menu opened over the page).
    pub fn release_all(&self) {
        *self.back_down.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let mut events: Vec<Synthetic> =
            ["up", "down", "left", "right", "return"].iter().map(|k| key(k, false)).collect();
        events.push(Synthetic::PointerButton { button: "left".into(), pressed: false });
        self.send(events);
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// The main display's area, in points.
fn screen_area(c: &mut aizome_ipc::Client) -> Option<(f64, f64, f64, f64)> {
    let Ok(Response::Outputs { outputs }) = c.request(&Request::Outputs) else { return None };
    let o = outputs.iter().find(|o| o.enabled && o.primary).or_else(|| outputs.iter().find(|o| o.enabled))?;
    let r = o.logical;
    Some((f64::from(r.x), f64::from(r.y), f64::from(r.w), f64::from(r.h)))
}

/// A stick's lean, with a curve: fine near the middle, quick at the edge.
fn curve(v: i16) -> f64 {
    let x = f64::from(v) / 32767.0;
    x * x.abs()
}

fn bridge(
    rx: mpsc::Receiver<Vec<Synthetic>>,
    stop: Arc<AtomicBool>,
    pointing: Arc<AtomicBool>,
    pads: Arc<Shared>,
    drive: Drive,
) {
    let mut client: Option<aizome_ipc::Client> = None;
    let mut area = (0.0, 0.0, 1920.0, 1080.0);
    let mut at: Option<(f64, f64)> = None;
    let mut last = Instant::now();
    let mut scroll_left = (0.0f64, 0.0f64);
    while !stop.load(Ordering::Acquire) {
        if client.is_none() {
            client = aizome_ipc::Client::connect().ok();
            if let Some(c) = client.as_mut()
                && let Some(a) = screen_area(c)
            {
                area = a;
            }
        }
        let mut events = Vec::new();
        let moving = (0..SLOTS).any(|s| {
            pads.connected(s)
                && [Axis::LeftX, Axis::LeftY, Axis::RightX, Axis::RightY].iter().any(|a| pads.axis(s, *a) != 0)
        });
        let wait = if moving { Duration::from_millis(8) } else { Duration::from_millis(40) };
        match rx.recv_timeout(wait) {
            Ok(mut e) => events.append(&mut e),
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Ok(mut e) = rx.try_recv() {
            events.append(&mut e);
        }
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f64().min(0.05);
        last = now;
        if drive == Drive::Pointer {
            let (mut dx, mut dy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
            for s in (0..SLOTS).filter(|s| pads.connected(*s)) {
                dx += curve(pads.axis(s, Axis::LeftX));
                dy += curve(pads.axis(s, Axis::LeftY));
                sx += curve(pads.axis(s, Axis::RightX));
                sy += curve(pads.axis(s, Axis::RightY));
            }
            if dx != 0.0 || dy != 0.0 {
                let (x0, y0, w, h) = area;
                let (x, y) = at.unwrap_or((x0 + w / 2.0, y0 + h / 2.0));
                let x = (x + dx * POINTER_SPEED * dt).clamp(x0, x0 + w - 1.0);
                let y = (y + dy * POINTER_SPEED * dt).clamp(y0, y0 + h - 1.0);
                at = Some((x, y));
                pointing.store(true, Ordering::Release);
                events.push(Synthetic::PointerMotion { x, y });
            }
            // Whole points of scrolling, the rest carried over.
            scroll_left.0 += sx * SCROLL_SPEED * dt;
            scroll_left.1 += sy * SCROLL_SPEED * dt;
            let (wx, wy) = (scroll_left.0.trunc(), scroll_left.1.trunc());
            if wx != 0.0 || wy != 0.0 {
                scroll_left.0 -= wx;
                scroll_left.1 -= wy;
                events.push(Synthetic::PointerScroll { dx: wx, dy: wy });
            }
        }
        if events.is_empty() {
            continue;
        }
        let sent = client.as_mut().map(|c| c.request(&Request::Synthesize { events }));
        if !matches!(sent, Some(Ok(_))) {
            client = None;
        }
    }
}

/// A service open in the browser.
pub struct Browser {
    child: Arc<Mutex<Child>>,
    pub pid: i32,
    pub service: String,
    pub bridge: Bridge,
    window: Arc<Mutex<Option<WindowId>>>,
    stop: Arc<AtomicBool>,
}

impl Browser {
    /// Start `browser` on `url` with the profile at `profile`.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        browser: &Path,
        profile: &Path,
        service: &str,
        url: &str,
        agent: Option<&str>,
        drive: Drive,
        pads: Arc<Shared>,
        events: impl Fn(Event) + Send + 'static,
    ) -> std::io::Result<Browser> {
        std::fs::create_dir_all(profile)?;
        let child = Command::new(browser)
            .args(arguments(profile, url, agent))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let pid = child.id() as i32;
        let child = Arc::new(Mutex::new(child));
        let window = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (c, w, s) = (child.clone(), window.clone(), stop.clone());
        let _ = std::thread::Builder::new().name("chanoma-browser".into()).spawn(move || watch(pid, c, w, s, events));
        Ok(Browser { child, pid, service: service.into(), bridge: Bridge::new(drive, pads), window, stop })
    }

    /// Its window, once shown.
    pub fn window(&self) -> Option<WindowId> {
        *self.window.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bring its window forward.
    pub fn focus(&self) {
        if let Some(id) = self.window()
            && let Ok(mut c) = aizome_ipc::Client::connect()
        {
            let _ = c.request(&Request::Focus { id });
        }
    }

    /// End it: its window asked to close, the process stopped if it's
    /// still there in a few seconds. Doesn't wait.
    pub fn close(self) {
        let _ = std::thread::Builder::new().name("chanoma-browser-close".into()).spawn(move || self.close_and_wait());
    }

    /// End it, waiting until it has (Chanoma leaving: a browser left
    /// behind would hold the screen).
    pub fn close_and_wait(self) {
        self.stop.store(true, Ordering::Release);
        if let Some(id) = self.window()
            && let Ok(mut c) = aizome_ipc::Client::connect()
        {
            let _ = c.request(&Request::Close { id: Some(id) });
        }
        // SAFETY: a signal to the browser this process started.
        unsafe { libc::kill(self.pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let done = self.child.lock().map(|mut c| matches!(c.try_wait(), Ok(Some(_)))).unwrap_or(true);
            if done {
                return;
            }
            if Instant::now() > deadline {
                let _ = self.child.lock().map(|mut c| c.kill());
                let _ = self.child.lock().map(|mut c| c.wait());
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Whether process `p` is `ancestor` or was started by it, however far down:
/// a launcher script that doesn't `exec`, or a browser that starts its
/// window in a child, still owns the window.
fn descends(mut p: i32, ancestor: i32) -> bool {
    for _ in 0..16 {
        if p == ancestor {
            return true;
        }
        let Ok(status) = std::fs::read_to_string(format!("/proc/{p}/status")) else { return false };
        let parent = status.lines().find_map(|l| l.strip_prefix("PPid:")).and_then(|v| v.trim().parse::<i32>().ok());
        match parent {
            Some(pp) if pp > 1 => p = pp,
            _ => return false,
        }
    }
    false
}

/// Follow the browser: its window, pop-ups closed, and its end.
fn watch(
    pid: i32,
    child: Arc<Mutex<Child>>,
    window: Arc<Mutex<Option<WindowId>>>,
    stop: Arc<AtomicBool>,
    events: impl Fn(Event),
) {
    let mut client: Option<aizome_ipc::Client> = None;
    let mut shown: Option<WindowId> = None;
    let started = Instant::now();
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let exited = child.lock().map(|mut c| matches!(c.try_wait(), Ok(Some(_)))).unwrap_or(true);
        if client.is_none() {
            client = aizome_ipc::Client::connect().ok();
        }
        let mine: Vec<aizome_ipc::WindowInfo> = match client.as_mut().map(|c| c.request(&Request::Windows)) {
            Some(Ok(Response::Windows { windows })) => {
                windows.into_iter().filter(|w| w.pid.is_some_and(|p| descends(p, pid))).collect()
            }
            Some(_) => {
                client = None;
                Vec::new()
            }
            None => Vec::new(),
        };
        if let Some(c) = client.as_mut() {
            match shown {
                None => {
                    if let Some(w) = mine.first() {
                        log::info!("chanoma: the browser's window is {} ({})", w.id, w.app_id);
                        shown = Some(w.id);
                        *window.lock().unwrap_or_else(|e| e.into_inner()) = Some(w.id);
                        if !w.fullscreen {
                            let _ = c.request(&Request::Fullscreen { id: Some(w.id), state: Some(true) });
                        }
                        let _ = c.request(&Request::Focus { id: w.id });
                        events(Event::Shown(w.id));
                    }
                }
                Some(main) => {
                    for w in mine.iter().filter(|w| w.id != main) {
                        log::info!("chanoma: closing a window the page opened: {}", w.title);
                        let _ = c.request(&Request::Close { id: Some(w.id) });
                    }
                    if !mine.iter().any(|w| w.id == main) {
                        // Its window went: the browser is ending, or
                        // crashed with its process left behind.
                        events(Event::Gone);
                        return;
                    }
                }
            }
        }
        if exited || (shown.is_none() && started.elapsed() > Duration::from_secs(60)) {
            events(Event::Gone);
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_opens_the_service_full_screen_in_its_own_profile() {
        let args = arguments(Path::new("/data/browser/netflix"), "https://www.netflix.com/browse", None);
        assert_eq!(args.first().map(String::as_str), Some("--user-data-dir=/data/browser/netflix"));
        assert!(args.iter().any(|a| a == "--kiosk"));
        assert!(args.iter().any(|a| a == "--password-store=basic"));
        assert_eq!(args.last().map(String::as_str), Some("https://www.netflix.com/browse"));
        let tv = arguments(Path::new("/p"), "https://www.youtube.com/tv", Some(crate::services::TV_AGENT));
        assert!(tv.iter().any(|a| a.starts_with("--user-agent=Mozilla/5.0 (SMART-TV")));
    }

    #[test]
    fn a_process_descends_from_its_parents() {
        let me = std::process::id() as i32;
        let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
        assert!(descends(child.id() as i32, me));
        assert!(descends(me, me));
        assert!(!descends(me, child.id() as i32));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn sticks_lean_gently_at_first() {
        assert_eq!(curve(0), 0.0);
        assert!((curve(32767) - 1.0).abs() < 1e-9);
        assert!((curve(-16384) + 0.25).abs() < 1e-3);
    }
}
