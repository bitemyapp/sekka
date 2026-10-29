//! `aizome-nisshi [--search TEXT] [--source NAME] [--unit NAME] [--level LEVEL]
//! [--scope problems|mine|kernel|crashes|security] [--boot N] [--file PATH]
//! [--cursor CURSOR]`:
//! the Logs window, or in the open one, the view asked for. `--print`
//! prints the newest messages instead (`--system` reads them as root,
//! through polkit).

use std::time::Duration;

use nisshi::app::{Nisshi, Request};
use nisshi::backend::{Client, Event};
use nisshi::query::Query;
use nisshi::time::Clock;

/// Read the newest messages through the reader, as the window does, and
/// print them.
fn print(args: &[String]) -> i32 {
    let root = args.iter().any(|a| a == "--system");
    let lines = args.windows(2).find(|p| p[0] == "--lines").and_then(|p| p[1].parse().ok()).unwrap_or(20u32);
    let (tx, rx) = std::sync::mpsc::channel();
    let clock = Clock::system();
    let mut c = Client::start(root, clock.clone(), move |e| {
        let _ = tx.send(e);
    });
    let mut query_id = None;
    let mut got = Vec::new();
    while let Ok(e) = rx.recv_timeout(Duration::from_secs(120)) {
        match e {
            Event::Ready(r) => {
                eprintln!(
                    "Reading as uid {} with {}: {}",
                    r.uid,
                    r.journalctl,
                    if r.system { "the system's journal" } else { "only this user's journal" }
                );
                query_id = Some(c.query(Query { limit: Some(lines), ..Query::default() }));
            }
            Event::Entries { id, entries } if Some(id) == query_id => got.extend(entries),
            Event::Done { id } if Some(id) == query_id => break,
            Event::Failed { why, .. } if nisshi::backend::nothing_to_read(&why) => break,
            Event::Failed { why, .. } => {
                eprintln!("{why}");
                return 1;
            }
            Event::Gone(g) => {
                eprintln!("{}", g.words());
                return 1;
            }
            _ => {}
        }
    }
    got.reverse();
    for e in &got {
        let pid = if e.pid > 0 { format!("[{}]", e.pid) } else { String::new() };
        println!(
            "{} {:<7} {}{pid}: {}",
            clock.stamp(e.time),
            e.priority.name(),
            e.source,
            e.message.lines().next().unwrap_or("")
        );
    }
    c.stop();
    0
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: aizome-nisshi [--search TEXT] [--source NAME] [--unit NAME] [--level LEVEL]\n\
             \x20                    [--scope problems|mine|kernel|crashes|security] [--boot N] [--file PATH]\n\
             \x20                    [--cursor CURSOR]\n\
             \x20      aizome-nisshi --print [--lines N] [--system]\n\n\
             Logs: the system journal and /var/log, searched as you type, with filters suggested by what wrote each message."
        );
        return;
    }
    if args.iter().any(|a| a == "--print") {
        std::process::exit(print(&args));
    }
    let mut app = Nisshi::live(Request::parse(&args));
    match nisshi::instance::claim(&args) {
        nisshi::instance::Claim::Handed => return,
        nisshi::instance::Claim::First(l) => app.listener = Some(l),
        nisshi::instance::Claim::Alone => {}
    }
    aizome_kit::run(app);
}
