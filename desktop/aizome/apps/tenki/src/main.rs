//! `aizome-tenki`: the weather.
//!
//! `aizome-tenki --record-fixtures DIR` fetches the forecasts and searches
//! the tests use into DIR (a handful of requests to Open-Meteo), for
//! re-recording `tests/fixtures`.

use std::sync::Arc;

use aizome_tenki::Tenki;
use aizome_tenki::instance::{self, Claim};
use aizome_tenki::services::{Live, record};
use aizome_tenki::store::Store;

/// What the tests' fixtures hold: Tokyo, Chicago and Reykjavík (warm,
/// middling and cold), and searches for a city and a time zone's city.
const FIXTURE_PLACES: [(f64, f64); 3] = [(35.6895, 139.69171), (41.85003, -87.65005), (64.13548, -21.89541)];
const FIXTURE_SEARCHES: [&str; 3] = ["Tokyo", "Chicago", "Kyoto"];

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("aizome-tenki {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--record-fixtures") => {
            let Some(dir) = args.get(1) else {
                eprintln!("usage: aizome-tenki --record-fixtures DIR");
                std::process::exit(2);
            };
            if let Err(e) = record(std::path::Path::new(dir), &FIXTURE_PLACES, &FIXTURE_SEARCHES) {
                eprintln!("{e}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }
    let mut app = Tenki::new(Arc::new(Live::new()), Some(Store::xdg()));
    match instance::claim("tenki", &args) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    aizome_kit::run(app);
    // What the window left to write.
    aizome_tenki::store::flush();
}
