//! `aizome-fusen [--new TEXT | --new-from-stdin | --stickies] [FILE…]`: the
//! notes. A second launch hands its request to the running one.

use std::io::Read;

use aizome_fusen::app::{Fusen, Places, Request};
use aizome_sumi::instance::{self, Claim};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: aizome-fusen [--new TEXT | --new-from-stdin | --stickies] [FILE…]\n\n\
             Fusen, Aizome's notes, kept as Markdown in ~/Notes.\n  \
             --new TEXT        a new note holding TEXT\n  \
             --new-from-stdin  a new note holding what's read from standard input\n  \
             --stickies        only the stickies (at login)\n  \
             FILE              a note to show, or a file whose text becomes a new note"
        );
        return;
    }
    // Standard input is read here, so the running Fusen gets the text.
    if let Some(i) = args.iter().position(|a| a == "--new-from-stdin") {
        let mut text = String::new();
        let _ = std::io::stdin().read_to_string(&mut text);
        args.splice(i..i + 1, ["--new".to_owned(), text]);
    }
    let handed: Vec<String> = args
        .iter()
        .scan(false, |after_new, a| {
            let out = if *after_new || a.starts_with('-') {
                a.clone()
            } else {
                std::path::absolute(a).map(|p| p.display().to_string()).unwrap_or_else(|_| a.clone())
            };
            *after_new = a == "--new";
            Some(out)
        })
        .collect();
    let request = Request::parse(&handed);
    let mut app = Fusen::new(Places::user(), request);
    match instance::claim("fusen", &handed) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    aizome_kit::run(app);
}
