//! A page kept to its site in a real browser: one Chromium-based browser
//! (named by `CHANOMA_TEST_BROWSER`) steered over its DevTools pipe as
//! Chanoma steers it, with no display (`--ozone-platform=headless`). The
//! site is 127.0.0.1 and elsewhere is localhost, both served here, which
//! hears every request the browser makes; and Chanoma's keyboard asking
//! what has the page's and typing there. Skips without a browser.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use aizome_chanoma::browser::arguments;
use aizome_chanoma::devtools::{self, Ask, Asker, Focused, Key, Told};
use aizome_chanoma::services::Sites;

/// What the server heard: `host path` a request.
type Heard = Arc<Mutex<Vec<String>>>;

/// The site's first page tries every way out it can by itself, one after
/// another, saying after each that it's still there; then goes to its
/// own second page.
fn page_a(port: u16) -> String {
    format!(
        r#"<!doctype html><title>A</title>
<a id="redirect" href="/redirect">Out by a redirect</a>
<form id="form" method="post" action="http://localhost:{port}/b?how=form"><input name="x" value="1"></form>
<iframe src="http://localhost:{port}/frame"></iframe>
<script>
const say = (what) => fetch("/report?" + what);
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
addEventListener("message", (e) => say("frame-" + e.data));
say("webdriver-" + navigator.webdriver);
(async () => {{
  await wait(300);
  location.href = "http://localhost:{port}/b?how=script";
  await wait(700);
  await say("after-script");
  document.getElementById("redirect").click();
  await wait(700);
  await say("after-redirect");
  document.getElementById("form").submit();
  await wait(700);
  await say("after-form");
  window.open("http://localhost:{port}/b?how=popup");
  await wait(700);
  await say("after-popup");
  location.href = "/c";
}})();
</script>"#
    )
}

/// The site's second page keeps trying to leave, once a second.
fn page_c(port: u16) -> String {
    format!(
        r#"<!doctype html><title>C</title><script>
fetch("/report?loaded-c");
setInterval(() => {{ location.href = "http://localhost:{port}/b?how=free"; }}, 1000);
</script>"#
    )
}

/// A page with boxes to type in: a search box it gives the keyboard after
/// a moment, a password box it gives it after a search, then a frame from
/// elsewhere. It says what's sent, and whether anything was added to its
/// own scripts' world meanwhile.
fn page_t(port: u16) -> String {
    format!(
        r#"<!doctype html><title>T</title>
<form id="f1"><input id="q" type="search" placeholder="Search"></form>
<form id="f2"><label for="p">Password</label><input id="p" type="password"></form>
<iframe src="http://localhost:{port}/frame"></iframe>
<script>
const say = (what) => fetch("/report?" + encodeURIComponent(what));
const names = Object.getOwnPropertyNames(window).length;
say("loaded-t");
setTimeout(() => q.focus(), 1500);
f1.addEventListener("submit", (e) => {{ e.preventDefault(); say("searched-" + q.value); p.focus(); }});
f2.addEventListener("submit", (e) => {{
  e.preventDefault();
  say("unseen-" + (Object.getOwnPropertyNames(window).length === names));
  say("password-" + p.value.length);
  document.querySelector("iframe").focus();
}});
</script>"#
    )
}

const PAGE_B: &str = r#"<!doctype html><title>B</title><script>fetch("/report?loaded-b")</script>"#;
const FRAME: &str = r#"<!doctype html><title>F</title><script>parent.postMessage("loaded", "*")</script>"#;

fn serve() -> (u16, Heard) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let heard: Heard = Arc::default();
    let h = heard.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let h = h.clone();
            std::thread::spawn(move || answer(stream, port, &h));
        }
    });
    (port, heard)
}

fn answer(stream: TcpStream, port: u16, heard: &Heard) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let (mut host, mut length) = (String::new(), 0usize);
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or_default();
        match name.to_ascii_lowercase().as_str() {
            "host" => host = value.trim().split(':').next().unwrap_or_default().to_owned(),
            "content-length" => length = value.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    let mut body = vec![0u8; length];
    let _ = reader.read_exact(&mut body);
    heard.lock().unwrap().push(format!("{host} {path}"));
    let (status, extra, page) = match path.split('?').next().unwrap_or_default() {
        "/a" => ("200 OK", String::new(), page_a(port)),
        "/c" => ("200 OK", String::new(), page_c(port)),
        "/t" => ("200 OK", String::new(), page_t(port)),
        "/b" => ("200 OK", String::new(), PAGE_B.to_owned()),
        "/frame" => ("200 OK", String::new(), FRAME.to_owned()),
        "/redirect" => ("302 Found", format!("Location: http://localhost:{port}/b?how=redirect\r\n"), String::new()),
        _ => ("204 No Content", String::new(), String::new()),
    };
    let mut out = stream;
    let _ = write!(
        out,
        "HTTP/1.1 {status}\r\n{extra}Content-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
}

fn eventually(what: &str, within: Duration, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + within;
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A browser kept as Chanoma keeps one, on `start`.
struct Run {
    child: Child,
    asker: Asker,
    told: mpsc::Receiver<Told>,
    kept: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    keeper: Option<std::thread::JoinHandle<()>>,
    profile: PathBuf,
}

impl Run {
    fn start(browser: &Path, start: String, name: &str) -> Run {
        let profile = std::env::temp_dir().join(format!("chanoma-devtools-{name}-{}", std::process::id()));
        let mut command = Command::new(browser);
        command
            .args(arguments(&profile, None))
            .args(["--ozone-platform=headless", "--no-sandbox", "--disable-gpu", "--disable-popup-blocking"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let (child, pipe) = devtools::spawn(&mut command).unwrap();
        let (asker, asked) = devtools::asking().unwrap();
        let kept = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, told) = mpsc::channel();
        let (k, s) = (kept.clone(), stop.clone());
        let keeper = std::thread::spawn(move || {
            devtools::keep(pipe, Sites::new(&start, &[]), start, asked, &k, &s, move |t| {
                let _ = tx.send(t);
            })
        });
        Run { child, asker, told, kept, stop, keeper: Some(keeper), profile }
    }

    /// What has the page's keyboard, asked as Chanoma's keyboard asks.
    fn focused(&self) -> Focused {
        self.asker.ask(Ask::Focused);
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.told.recv_timeout(left) {
                Ok(Told::Focused(f)) => return f,
                Ok(_) => {}
                Err(_) => panic!("the page didn't say what has its keyboard"),
            }
        }
    }

    /// Chanoma gone: the pipe closes, and so does the browser (whether it
    /// did in time).
    fn finish(mut self) -> bool {
        self.stop.store(true, Ordering::Release);
        if let Some(k) = self.keeper.take() {
            k.join().unwrap();
        }
        let end = Instant::now() + Duration::from_secs(15);
        let ended = loop {
            if let Ok(Some(_)) = self.child.try_wait() {
                break true;
            }
            if Instant::now() > end {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break false;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let _ = std::fs::remove_dir_all(&self.profile);
        ended
    }
}

fn test_browser() -> Option<PathBuf> {
    let b = std::env::var_os("CHANOMA_TEST_BROWSER").map(PathBuf::from);
    if b.is_none() {
        eprintln!("skipping: set CHANOMA_TEST_BROWSER to a Chromium-based browser");
    }
    b
}

#[test]
fn a_page_stays_on_its_site_unless_a_grown_up_lets_it_go() {
    let Some(browser) = test_browser() else { return };
    let (port, heard) = serve();
    let has = |what: &str| heard.lock().unwrap().iter().any(|h| h.contains(what));
    let run = Run::start(&browser, format!("http://127.0.0.1:{port}/a"), "kept");
    let outside = || run.told.try_iter().filter(|t| matches!(t, Told::Outside(h) if h == "localhost")).count();

    // Kept: every way out leaves the page where it was, and nothing is
    // asked of elsewhere but the frame inside the page.
    eventually("the site's own second page", Duration::from_secs(30), || has("127.0.0.1 /report?loaded-c"));
    let mut refused = outside();
    for still in ["after-script", "after-redirect", "after-form", "after-popup"] {
        assert!(has(&format!("/report?{still}")), "the page left before {still}: {:?}", heard.lock().unwrap());
    }
    assert!(has("127.0.0.1 /redirect"));
    assert!(has("127.0.0.1 /report?frame-loaded"), "a frame from elsewhere loads inside the page");
    assert!(has("127.0.0.1 /report?webdriver-false"), "pages aren't told the browser is steered");
    let out: Vec<String> = heard.lock().unwrap().iter().filter(|h| h.contains("/b")).cloned().collect();
    assert!(out.is_empty(), "something reached elsewhere: {out:?}");
    // The script, the redirect, the form and the pop-up's page, at least.
    eventually("the second page's tries refused", Duration::from_secs(10), || {
        refused += outside();
        refused >= 5
    });

    // Grown-up mode: it goes.
    run.kept.store(false, Ordering::Release);
    eventually("the page going elsewhere", Duration::from_secs(10), || has("localhost /report?loaded-b"));
    assert!(has("localhost /b?how=free"));

    // Kids' mode again: it's taken back to where it was on its site, and
    // tries to leave again, refused.
    let _ = outside();
    run.kept.store(true, Ordering::Release);
    eventually("the page back on its site, kept", Duration::from_secs(10), || outside() > 0);

    assert!(run.finish(), "the browser outlived its pipe");
}

#[test]
fn chanomas_keyboard_hears_what_has_the_pages_and_types_there() {
    let Some(browser) = test_browser() else { return };
    let (port, heard) = serve();
    let has = |what: &str| heard.lock().unwrap().iter().any(|h| h.contains(what));
    let run = Run::start(&browser, format!("http://127.0.0.1:{port}/t"), "typing");
    eventually("the page", Duration::from_secs(30), || has("127.0.0.1 /report?loaded-t"));

    // Nothing yet, then the search box the page gives the keyboard.
    assert_eq!(run.focused(), Focused::Elsewhere);
    eventually("the search box", Duration::from_secs(10), || matches!(run.focused(), Focused::Field(_)));
    let Focused::Field(f) = run.focused() else { panic!("the box let go") };
    assert_eq!((f.label.as_str(), f.text.as_str(), f.secret), ("Search", "", false));

    // Typed, a letter taken back, the caret moved and a letter put in
    // there: what the page has, and where the caret is.
    run.asker.ask(Ask::Type("héllo".into()));
    run.asker.ask(Ask::Key(Key::Backspace));
    run.asker.ask(Ask::Key(Key::Left));
    run.asker.ask(Ask::Type("X".into()));
    let Focused::Field(f) = run.focused() else { panic!("the box let go") };
    assert_eq!((f.text.as_str(), f.length, f.caret), ("hélXl", 5, 4));

    // Enter sends it; the page gives the keyboard to its password box,
    // whose letters aren't told, only how many.
    run.asker.ask(Ask::Key(Key::Enter));
    eventually("the search sent", Duration::from_secs(10), || has("/report?searched-h%C3%A9lXl"));
    run.asker.ask(Ask::Type("pw".into()));
    let Focused::Field(f) = run.focused() else { panic!("the password box hasn't the keyboard") };
    assert_eq!((f.label.as_str(), f.text.as_str(), f.length, f.secret), ("Password", "", 2, true));

    // Then a frame from another site has it: only that it's a frame.
    run.asker.ask(Ask::Key(Key::Enter));
    eventually("the password sent", Duration::from_secs(10), || has("/report?password-2"));
    assert!(has("/report?unseen-true"), "the page saw something of Chanoma's asking");
    assert_eq!(run.focused(), Focused::Frame);
    assert!(run.finish(), "the browser outlived its pipe");
}
