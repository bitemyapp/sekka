//! What Nisshi's typical frames cost with a large boot (over 100,000
//! messages), from the message to its pixels at 2× (the update, the view,
//! layout and diff, and rasterising each damaged rectangle): new messages
//! arriving live, a click on a row, scrolling, and a character typed in
//! search. Prints percentiles of the time taken and of the processor time
//! used (for docs/performance.md); holds the median processor time of
//! what happens most (arrivals, clicks, scrolling) to the 4 ms budget in
//! an optimised build.

use std::time::{Duration, Instant};

use aizome_kit::testing::Harness;
use aizome_kit::{Point, Vec2};
use nisshi::app::{Access, Msg, Nisshi};
use nisshi::backend::Event;
use nisshi::fixture;

/// This thread's processor time: what a frame cost, whether or not the
/// scheduler let it run straight through (the harness draws on this
/// thread).
fn cpu() -> Duration {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: t is a valid timespec to write.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// One frame: the time it took, and the processor time it used.
fn frame(h: &mut Harness<Nisshi>, act: impl FnOnce(&mut Harness<Nisshi>)) -> (Duration, Duration) {
    let w = h.window();
    h.take_damage(w);
    let (start, used) = (Instant::now(), cpu());
    act(h);
    for d in h.take_damage(w) {
        let _ = h.render_rect(w, d, 2.0);
    }
    (start.elapsed(), cpu() - used)
}

fn report(what: &str, n: usize, frames: Vec<(Duration, Duration)>) -> (Duration, Duration) {
    let (mut v, mut c): (Vec<Duration>, Vec<Duration>) = frames.into_iter().unzip();
    v.sort();
    c.sort();
    let cp = |q: f64| c[((c.len() - 1) as f64 * q) as usize];
    eprintln!("nisshi {what}, processor time: p50 {:?} p95 {:?}", cp(0.5), cp(0.95));
    let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
    eprintln!(
        "nisshi {what}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x, {n} messages)",
        p(0.5),
        p(0.95),
        v.last().unwrap(),
        v.len()
    );
    (p(0.5), cp(0.5))
}

#[test]
fn frames_with_a_large_boot() {
    let mut all = fixture::large(190);
    let live: Vec<_> = all.split_off(all.len() - 600);
    let n = all.len();
    let start = Instant::now();
    let mut h = Harness::new(Nisshi::sample(
        all,
        fixture::boots(),
        fixture::files(),
        fixture::clock(),
        fixture::UID,
        Access::System,
    ));
    eprintln!("nisshi: {n} messages taken in and shown in {:?}", start.elapsed());
    let w = h.window();
    let _ = h.render(w, 2.0);
    let id = h.program_mut().sample_follow();

    // Messages arriving live, a few at a time, as the view refreshes.
    let mut arrivals = Vec::new();
    for batch in live.chunks(10) {
        let batch = batch.to_vec();
        arrivals.push(frame(&mut h, |h| {
            h.send(Msg::Backend(0, Event::Entries { id, entries: batch }));
            h.advance(Duration::from_millis(100));
        }));
    }
    let table = h.find_id(w, "messages").expect("the table");
    let mut clicks = Vec::new();
    for k in 0..30 {
        let at = Point::new(table.x0 + 400.0, table.y0 + 40.0 + f64::from(k % 12) * 22.0);
        clicks.push(frame(&mut h, |h| h.click(w, at)));
    }
    let mut scrolls = Vec::new();
    for k in 0..30 {
        let dy = if k % 10 < 5 { 44.0 } else { -44.0 };
        scrolls.push(frame(&mut h, |h| h.scroll(w, table.center(), Vec2::new(0.0, dy))));
    }
    // Typing in search: each character shows at once; the messages are
    // filtered when typing pauses (a large boot).
    let mut typing = Vec::new();
    let mut filtered = Vec::new();
    for word in ["wlp14s0", "kex", "failed"] {
        for i in 1..=word.len() {
            let q = word[..i].to_owned();
            typing.push(frame(&mut h, |h| h.send(Msg::Search(q))));
        }
        filtered.push(frame(&mut h, |h| h.advance(Duration::from_millis(100))));
        h.send(Msg::Search(String::new()));
        h.advance(Duration::from_millis(100));
    }
    // The filtering alone, without drawing.
    let mut filtering = Vec::new();
    for q in ["w", "wl", "wlp", "kex", "failed", "app:sshd", "level:err"] {
        let (start, used) = (Instant::now(), cpu());
        let _ = h.program_mut().set_search_for_test(q);
        h.program_mut().rebuild();
        filtering.push((start.elapsed(), cpu() - used));
    }
    h.send(Msg::Search(String::new()));
    let total = h.program().store.entries.len();
    report("filtering alone", total, filtering);
    let most =
        [report("arrival", total, arrivals), report("row click", total, clicks), report("scroll", total, scrolls)];
    report("search keystroke", total, typing);
    report("search, filtered once typing pauses", total, filtered);
    // The processor time: on a machine shared with other builds, the
    // scheduler's pauses swell the time taken, which is printed beside it.
    if !cfg!(debug_assertions) {
        for (_, cpu50) in most {
            assert!(cpu50 < Duration::from_millis(4), "p50 {cpu50:?} of processor time");
        }
    }
}

/// A boot of the size desktops have (about 4,000 messages): search filters
/// them all at each character typed.
#[test]
fn typing_in_a_typical_boot() {
    let all = fixture::large(7);
    let n = all.len();
    assert!(n < nisshi::app::LARGE);
    let mut h = Harness::new(Nisshi::sample(
        all,
        fixture::boots(),
        fixture::files(),
        fixture::clock(),
        fixture::UID,
        Access::System,
    ));
    let w = h.window();
    let _ = h.render(w, 2.0);
    let mut typing = Vec::new();
    for word in ["wlp14s0", "kex", "failed", "app:sshd"] {
        for i in 1..=word.len() {
            let q = word[..i].to_owned();
            typing.push(frame(&mut h, |h| h.send(Msg::Search(q))));
        }
        h.send(Msg::Search(String::new()));
    }
    let (_, cpu50) = report("search keystroke, filtering at once", n, typing);
    if !cfg!(debug_assertions) {
        assert!(cpu50 < Duration::from_millis(4), "p50 {cpu50:?} of processor time");
    }
}
