//! `aizome-annai [--first-run]`: Aizome's welcome. With `--first-run` it
//! shows only the first time (or every time, with "Show at login" on) and
//! otherwise exits at once.

use aizome_annai::Annai;
use aizome_annai::instance::{self, Claim};
use aizome_annai::state;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "aizome-annai [--first-run]\n\nAizome's welcome. --first-run shows it only at the first login (or at\nevery login with \"Show at login\" on), and records that in\n{}.",
            state::path().display()
        );
        return;
    }
    let path = state::path();
    if args.iter().any(|a| a == "--first-run") && !state::load(&path).due() {
        return;
    }
    let mut app = Annai::new(Some(path));
    match instance::claim("annai") {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    aizome_kit::run(app);
}
