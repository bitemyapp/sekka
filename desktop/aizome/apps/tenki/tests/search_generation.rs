//! Hold real background search calls until the test chooses their reply order.
//! Input still goes through the visible search field and its debounce timer.
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_tenki::location::{LocateError, Located};
use aizome_tenki::services::{FetchError, Recorded, Services};
use aizome_tenki::{Msg, Tenki};

struct Request {
    query: String,
    reply: mpsc::Sender<Result<String, FetchError>>,
}

struct Delayed {
    requests: mpsc::Sender<Request>,
    recorded: Recorded,
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

impl Services for Delayed {
    fn search(&self, query: &str) -> Result<String, FetchError> {
        let (reply, answer) = mpsc::channel();
        self.requests.send(Request { query: query.into(), reply }).unwrap();
        answer.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| Err(FetchError::Offline("test ended".into())))
    }
    fn forecast(&self, lat: f64, lon: f64) -> Result<String, FetchError> {
        self.recorded.forecast(lat, lon)
    }
    fn locate(&self) -> Result<Located, LocateError> {
        self.recorded.locate()
    }
    fn timezone(&self) -> Option<String> {
        None
    }
    fn now(&self) -> i64 {
        self.recorded.now()
    }
}

fn launch() -> (Harness<Tenki>, mpsc::Receiver<Request>) {
    let (requests, incoming) = mpsc::channel();
    let services = Arc::new(Delayed { requests, recorded: Recorded::new(fixtures()) });
    let mut h = Harness::new(Tenki::new(services, None));
    let w = h.window();
    let field = h.find_id(w, "search").unwrap();
    h.click(w, field.center());
    (h, incoming)
}

fn request(incoming: &mpsc::Receiver<Request>, query: &str) -> Request {
    let r = incoming.recv_timeout(Duration::from_secs(2)).expect("the latest query starts while an old request waits");
    assert_eq!(r.query, query);
    r
}

fn answer(r: Request, city: &str) {
    let body = std::fs::read_to_string(fixtures().join(format!("search-{}.json", city.to_lowercase()))).unwrap();
    r.reply.send(Ok(body)).unwrap();
}

fn until(h: &mut Harness<Tenki>, check: impl Fn(&Harness<Tenki>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        h.send(Msg::Idle); // Drain completed background messages, without changing app state.
        if check(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("background reply did not produce its visible result");
}

fn drain_reply(h: &mut Harness<Tenki>) {
    // At these points only the released search can complete. An Idle update
    // plus a second program update proves the stale reply was actually handled;
    // merely sleeping could let assertions pass before that reply arrived.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        h.reset_stats();
        h.send(Msg::Idle);
        if h.stats().update.len() > 1 {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("released background reply was not drained");
}

#[test]
fn latest_query_finishes_while_older_search_waits_and_ignores_late_result() {
    let (mut h, incoming) = launch();
    let w = h.window();
    h.type_text(w, "Kyo");
    h.advance(Duration::from_millis(300));
    let old = request(&incoming, "Kyo");
    h.type_text(w, "to");
    h.advance(Duration::from_millis(300));
    let latest = request(&incoming, "Kyoto");
    answer(latest, "Kyoto");
    until(&mut h, |h| h.find_id(w, "result-0").is_some());
    assert_eq!(h.program().search.results[0].name, "Kyoto");
    answer(old, "Tokyo");
    drain_reply(&mut h);
    assert_eq!(h.program().search.results[0].name, "Kyoto", "late answers never replace the current query");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().selected_place().unwrap().name, "Kyoto");
    assert_eq!(h.program().places.len(), 1);
}

#[test]
fn stale_error_does_not_release_current_search_or_lose_early_return() {
    let (mut h, incoming) = launch();
    let w = h.window();
    h.type_text(w, "Kyo");
    h.advance(Duration::from_millis(300));
    let old = request(&incoming, "Kyo");
    h.type_text(w, "to");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    let latest = request(&incoming, "Kyoto");
    old.reply.send(Err(FetchError::Offline("old request failed".into()))).unwrap();
    drain_reply(&mut h);
    assert!(h.program().search.busy);
    assert!(h.program().search.error.is_none());
    assert!(h.program().places.is_empty());
    h.advance(Duration::from_millis(300));
    assert!(incoming.try_recv().is_err(), "the debounce does not duplicate an early Return request");
    answer(latest, "Kyoto");
    until(&mut h, |h| h.program().selected_place().is_some());
    assert_eq!(h.program().selected_place().unwrap().name, "Kyoto");
    assert_eq!(h.program().places.len(), 1);
}

#[test]
fn clearing_or_changing_query_removes_stale_rows_and_errors_and_recovers() {
    let (mut h, incoming) = launch();
    let w = h.window();
    h.type_text(w, "Tokyo");
    h.advance(Duration::from_millis(300));
    answer(request(&incoming, "Tokyo"), "Tokyo");
    until(&mut h, |h| h.find_id(w, "result-0").is_some());
    h.type_text(w, "x");
    assert!(h.find_id(w, "result-0").is_none(), "editing an answered query hides its obsolete rows");
    h.command(w, 'a');
    h.type_text(w, "Kyoto");
    assert!(h.find_id(w, "result-0").is_none(), "a previous query's row is not an actionable current result");
    h.advance(Duration::from_millis(300));
    request(&incoming, "Kyoto").reply.send(Err(FetchError::Offline("current query failed".into()))).unwrap();
    until(&mut h, |h| h.program().search.error.is_some());
    h.command(w, 'a');
    h.type_text(w, "Kyo");
    assert!(h.program().search.error.is_none(), "a fresh query can recover from an error");
    h.advance(Duration::from_millis(300));
    let old = request(&incoming, "Kyo");
    h.command(w, 'a');
    h.named(w, NamedKey::Backspace, Modifiers::NONE);
    h.type_text(w, "K");
    h.advance(Duration::from_millis(300));
    assert!(incoming.try_recv().is_err(), "short queries do not reach the service");
    answer(old, "Tokyo");
    drain_reply(&mut h);
    assert!(h.program().search.results.is_empty());
    assert!(h.program().search.error.is_none());
    assert!(h.texts(w).iter().any(|t| t == "Type a city's name."));
    h.type_text(w, "yoto");
    h.advance(Duration::from_millis(300));
    answer(request(&incoming, "Kyoto"), "Kyoto");
    until(&mut h, |h| h.find_id(w, "result-0").is_some());
    let first = h.find_id(w, "result-0").unwrap();
    h.click(w, first.center());
    assert_eq!(h.program().selected_place().unwrap().name, "Kyoto");
}
