//! `aizome-sumi [FILE…]`: the text editor. A second launch hands its files
//! to the running one.

use aizome_sumi::app::{Places, Sumi};
use aizome_sumi::instance::{self, Claim};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: aizome-sumi [FILE…]\n\nSumi, Aizome's text editor. Files open in the running Sumi if there is one."
        );
        return;
    }
    // Files by their full paths: the running Sumi has another folder.
    let handed: Vec<String> = args
        .iter()
        .map(|a| {
            if a.starts_with('-') {
                a.clone()
            } else {
                std::path::absolute(a).map(|p| p.display().to_string()).unwrap_or_else(|_| a.clone())
            }
        })
        .collect();
    let mut app = Sumi::new(Places::user());
    match instance::claim("sumi", &handed) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    aizome_kit::run(app);
}
