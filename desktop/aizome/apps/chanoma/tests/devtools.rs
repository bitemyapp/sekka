//! A page kept to its site in a real browser: one Chromium-based browser
//! (named by `CHANOMA_TEST_BROWSER`) steered over its DevTools pipe as
//! Chanoma steers it, with no display (`--ozone-platform=headless`). The
//! site is 127.0.0.1 and elsewhere is localhost, both served here, which
//! hears every request the browser makes. Skips without a browser.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use aizome_chanoma::browser::arguments;
use aizome_chanoma::devtools::{self, Told};
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

#[test]
fn a_page_stays_on_its_site_unless_a_grown_up_lets_it_go() {
    let Some(browser) = std::env::var_os("CHANOMA_TEST_BROWSER").map(PathBuf::from) else {
        eprintln!("skipping: set CHANOMA_TEST_BROWSER to a Chromium-based browser");
        return;
    };
    let (port, heard) = serve();
    let has = |what: &str| heard.lock().unwrap().iter().any(|h| h.contains(what));
    let profile = std::env::temp_dir().join(format!("chanoma-devtools-{}", std::process::id()));
    let mut command = Command::new(&browser);
    command
        .args(arguments(&profile, None))
        .args(["--ozone-platform=headless", "--no-sandbox", "--disable-gpu", "--disable-popup-blocking"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (mut child, pipe) = devtools::spawn(&mut command).unwrap();
    let kept = Arc::new(AtomicBool::new(true));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, told) = mpsc::channel();
    let (k, s) = (kept.clone(), stop.clone());
    let start = format!("http://127.0.0.1:{port}/a");
    let keeper = std::thread::spawn(move || {
        devtools::keep(pipe, Sites::new(&start, &[]), start, &k, &s, move |t| {
            let _ = tx.send(t);
        })
    });
    let outside = || told.try_iter().filter(|t| matches!(t, Told::Outside(h) if h == "localhost")).count();

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
    kept.store(false, Ordering::Release);
    eventually("the page going elsewhere", Duration::from_secs(10), || has("localhost /report?loaded-b"));
    assert!(has("localhost /b?how=free"));

    // Kids' mode again: it's taken back to where it was on its site, and
    // tries to leave again, refused.
    let _ = outside();
    kept.store(true, Ordering::Release);
    eventually("the page back on its site, kept", Duration::from_secs(10), || outside() > 0);

    // Chanoma gone, the pipe closes, and so does the browser.
    stop.store(true, Ordering::Release);
    keeper.join().unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    let ended = loop {
        if let Ok(Some(_)) = child.try_wait() {
            break true;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = std::fs::remove_dir_all(&profile);
    assert!(ended, "the browser outlived its pipe");
}
