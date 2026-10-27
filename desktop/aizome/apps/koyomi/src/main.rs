//! `aizome-koyomi`: the calendar, its alert agent (`--agent`) and its
//! answers for the shell (`--upcoming --json`).

use std::process::ExitCode;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use koyomi::settings::{self, Settings, State};
use koyomi::store::Store;

const HELP: &str = "\
Koyomi 暦, Aizome's calendar.

Usage:
  aizome-koyomi [FILE.ics…]          open the calendar (and import the files)
  aizome-koyomi --show YYYY-MM-DD    open the calendar at a day
  aizome-koyomi --upcoming [--json] [--days N] [--limit N]
                                     the next events (today and N−1 days on; 7)
  aizome-koyomi --list [--json]      the calendars
  aizome-koyomi --export ID          a calendar as iCalendar, on standard output
  aizome-koyomi --import FILE --calendar ID
                                     add a file's events to a calendar
  aizome-koyomi --agent              post alerts as notifications (a user service)

Calendars are iCalendar files in $XDG_DATA_HOME/aizome/koyomi/.
";

fn value_of(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn upcoming(args: &[String]) -> ExitCode {
    let days = value_of(args, "--days").and_then(|d| d.parse().ok()).unwrap_or(7u32);
    let limit = value_of(args, "--limit").and_then(|d| d.parse().ok()).unwrap_or(100usize);
    let store = Store::open(settings::data_dir());
    let hidden = State::load().hidden;
    let tz = koyomi::tz::system().zone.unwrap_or(TimeZone::UTC);
    let r = koyomi::upcoming::upcoming(&store.calendars, &hidden, &tz, Timestamp::now(), days, limit);
    if args.iter().any(|a| a == "--json") {
        match serde_json::to_string_pretty(&r) {
            Ok(j) => println!("{j}"),
            Err(e) => {
                eprintln!("aizome-koyomi: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        let s = Settings::load();
        print!("{}", koyomi::upcoming::text(&r, s.uses_24h(settings::desktop_24h())));
    }
    ExitCode::SUCCESS
}

fn list(args: &[String]) -> ExitCode {
    let store = Store::open(settings::data_dir());
    let hidden = State::load().hidden;
    if args.iter().any(|a| a == "--json") {
        let v: Vec<serde_json::Value> = store
            .calendars
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "dye": c.color.key(),
                    "color": format!("#{:06x}", c.color.hex()),
                    "events": c.events.len(),
                    "shown": !hidden.contains(&c.id),
                    "path": store.path_of(&c.id),
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(v));
    } else {
        for c in &store.calendars {
            println!(
                "{:<20} {:<24} {:>5} events{}",
                c.id,
                c.name,
                c.events.len(),
                if hidden.contains(&c.id) { "  (hidden)" } else { "" }
            );
        }
    }
    for p in &store.problems {
        eprintln!("aizome-koyomi: {p}");
    }
    ExitCode::SUCCESS
}

fn export(args: &[String]) -> ExitCode {
    let Some(id) = value_of(args, "--export") else {
        eprintln!("aizome-koyomi: --export needs a calendar's id (see --list)");
        return ExitCode::FAILURE;
    };
    let store = Store::open(settings::data_dir());
    match store.index_of(&id) {
        Some(i) => {
            print!("{}", store.calendars[i].to_ics());
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("aizome-koyomi: no calendar {id:?} (see --list)");
            ExitCode::FAILURE
        }
    }
}

fn import(args: &[String]) -> ExitCode {
    let (Some(file), Some(id)) = (value_of(args, "--import"), value_of(args, "--calendar")) else {
        eprintln!("aizome-koyomi: --import FILE --calendar ID");
        return ExitCode::FAILURE;
    };
    let text = match std::fs::read(&file) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => {
            eprintln!("aizome-koyomi: {file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut store = Store::open(settings::data_dir());
    let Some(i) = store.index_of(&id) else {
        eprintln!("aizome-koyomi: no calendar {id:?} (see --list)");
        return ExitCode::FAILURE;
    };
    let (from, skipped) = koyomi::model::Calendar::parse("import", &text);
    let mut r = koyomi::store::merge(&mut store.calendars[i], &from);
    r.skipped = skipped;
    if let Err(e) = store.save(i) {
        eprintln!("aizome-koyomi: {e}");
        return ExitCode::FAILURE;
    }
    println!("{} added, {} updated, {} unchanged, {} skipped", r.added, r.updated, r.unchanged, r.skipped);
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--help") || has("-h") {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if has("--agent") {
        return if koyomi::agent::run() == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    if has("--upcoming") {
        return upcoming(&args);
    }
    if has("--list") {
        return list(&args);
    }
    if has("--export") {
        return export(&args);
    }
    if has("--import") {
        return import(&args);
    }
    koyomi::app::main(args);
    ExitCode::SUCCESS
}
