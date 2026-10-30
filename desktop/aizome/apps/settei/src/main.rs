//! `aizome-settei`: Aizome's Settings.
use settei::app::{Settei, Start};
use settei::instance::{self, Instance};

const HELP: &str = "Usage: aizome-settei [--pane ID] [--keyboard NAME] [--sample]
       aizome-settei --list-settings [--json]

Settei (設定), Aizome's Settings. Changes apply at once.

  --pane ID          open a pane (keyboard), or an option in one
                     (keyboard.caps-lock)
  --keyboard NAME    with --pane keyboard.setup: the keyboard to choose in the
                     setup sheet, 'Name (vendor:product)' or either alone
  --list-settings    print every pane and option; with --json, as JSON for
                     the command palette (id, pane, title, path, keywords)
  --sample           show a sample computer instead of this one: nothing it
                     does changes the system (settings still go to
                     $XDG_CONFIG_HOME/aizome/settings.toml)
  -h, --help         show this
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return;
    }
    if args.iter().any(|a| a == "--list-settings") {
        let panes = settei::panes::all();
        if args.iter().any(|a| a == "--json") {
            println!("{}", settei::listing::json(&panes));
        } else {
            print!("{}", settei::listing::table(&panes));
        }
        return;
    }
    env_logger::init();
    let start = Start::from_args(&args);
    let sample = args.iter().any(|a| a == "--sample");
    let listener = if sample {
        None
    } else {
        match instance::claim(&args) {
            Instance::Handed => return,
            Instance::First(listener) => listener,
        }
    };
    let mut app = if sample { Settei::sample(aizome_config::Settings::path(), start) } else { Settei::live(start) };
    app.listener = listener;
    match aizome_kit::try_run(app) {
        // Every change in the file before going.
        Ok(app) => app.shared.flush(),
        Err(e) => {
            eprintln!("aizome-settei: {e}");
            std::process::exit(1);
        }
    }
    if !sample {
        instance::release();
    }
}
