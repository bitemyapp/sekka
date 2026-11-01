//! `aizome-chanoma`: Aizome's living-room app.

use aizome_chanoma::{Chanoma, Config};

const HELP: &str = "aizome-chanoma [OPTIONS]

The family television: streaming services, films and shows, and games,
all from a controller, with a grown-up's PIN on what kids may do.

  --windowed     open in a window rather than full screen
  --no-grab      leave controllers to other programs too
  --silent       play without sound
  --no-controllers
                 read no controllers (a preview beside a desktop that has them)
  --reset-pin    forget the grown-ups' PIN (the next start asks for a new
                 one), for a grown-up who has forgotten it";

fn main() {
    // Before any thread starts: a signal to stop then puts things away.
    aizome_chanoma::signals::block();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return;
    }
    let mut config = Config::standard();
    if args.iter().any(|a| a == "--reset-pin") {
        let path = config.dirs.settings();
        let mut settings: aizome_chanoma::rules::Settings = aizome_chanoma::store::read(&path);
        settings.pin = None;
        settings.pin_length = None;
        settings.setup_done = false;
        match aizome_chanoma::store::write(&path, &settings) {
            Ok(()) => println!("The PIN is forgotten: Chanoma asks for a new one when it starts."),
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                std::process::exit(1);
            }
        }
        let _ = std::fs::remove_file(config.dirs.state.join("lockout.json"));
        return;
    }
    config.fullscreen = !args.iter().any(|a| a == "--windowed");
    config.grab = !args.iter().any(|a| a == "--no-grab");
    config.silent = args.iter().any(|a| a == "--silent");
    config.pads = !args.iter().any(|a| a == "--no-controllers");
    config.signals = true;
    let mut app = Chanoma::new(config);
    match aizome_chanoma::instance::claim() {
        aizome_chanoma::instance::Claim::Handed => return,
        aizome_chanoma::instance::Claim::First(l) => app.instance = Some(l),
        aizome_chanoma::instance::Claim::Alone => {}
    }
    aizome_kit::run(app);
}
