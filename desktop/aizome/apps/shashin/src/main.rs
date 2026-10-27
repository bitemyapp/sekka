//! `aizome-shashin [FILE…|FOLDER]`: Aizome's image viewer.

use std::path::PathBuf;

use aizome_shashin::Shashin;
use aizome_shashin::instance::{self, Claim};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("aizome-shashin [FILE…|FOLDER]\n\nShows pictures: PNG, JPEG, GIF, WebP, BMP, TIFF, ICO and SVG.");
        return;
    }
    let paths: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(|a| match a.strip_prefix("file://") {
            Some(_) => aizome_shashin::ui::file_path(a).unwrap_or_else(|| PathBuf::from(a)),
            None => PathBuf::from(a),
        })
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();
    let mut app = Shashin::new();
    match instance::claim("shashin", &paths) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    app.initial = paths;
    aizome_kit::run(app);
}
