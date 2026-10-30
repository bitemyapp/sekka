//! `aizome-shiori [FILE…]`: Aizome's PDF reader.
//!
//! `aizome-shiori --worker` is the process the reader starts for
//! each document it opens (`worker.rs`); it isn't for running by hand.

use std::path::PathBuf;

use aizome_shiori::Shiori;
use aizome_shiori::instance::{self, Claim};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--worker") {
        std::process::exit(aizome_shiori::worker::run());
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("aizome-shiori [FILE…]\n\nReads PDF documents.");
        return;
    }
    let paths: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(|a| match a.strip_prefix("file://") {
            Some(_) => aizome_shiori::ui::file_path(a).unwrap_or_else(|| PathBuf::from(a)),
            None => PathBuf::from(a),
        })
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();
    let mut app = Shiori::new();
    match instance::claim("shiori", &paths) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    app.initial = paths;
    aizome_kit::run(app);
}
