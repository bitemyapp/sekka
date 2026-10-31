//! `aizome-utsushi [FILE…|FOLDER…]`: Aizome's media player.

use std::path::PathBuf;

use aizome_utsushi::Utsushi;
use aizome_utsushi::instance::{self, Claim};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "aizome-utsushi [FILE…|FOLDER…]\n\nPlays videos and music: a folder plays its files in order.\n\n  \
             --no-sound   play without PipeWire (the picture keeps its own time)"
        );
        return;
    }
    let paths: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|a| match a.strip_prefix("file://") {
            Some(_) => aizome_utsushi::ui::file_path(a).unwrap_or_else(|| PathBuf::from(a)),
            None => PathBuf::from(a),
        })
        // Files are made absolute for the instance they may go to; URLs
        // FFmpeg reads (http, https, rtsp) stay as they are.
        .map(|p| if p.to_string_lossy().contains("://") { p } else { std::path::absolute(&p).unwrap_or(p) })
        .collect();
    let mut app = Utsushi::new();
    app.null_audio = args.iter().any(|a| a == "--no-sound");
    match instance::claim("utsushi", &paths) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    app.initial = paths;
    aizome_kit::run(app);
}
