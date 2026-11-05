//! `aizome-tasuke [--fixture NAME]`: the Assistant window. `--fixture`
//! shows made-up conversations, machines and notes instead of this
//! computer's, and changes nothing.

use tasuke_window::app::{FIXTURES, Tasuke};

fn help() -> String {
    format!(
        "Usage: aizome-tasuke [--fixture NAME]\n\n\
         Assistant (助 Tasuke): Sekka's private assistant, running on this computer.\n\n\
         \x20 --fixture NAME   show made-up data instead of this computer's; NAME is one of\n\
         \x20                  {}\n\
         \x20 -h, --help       show this\n",
        FIXTURES.join(", ")
    )
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{}", help());
        return;
    }
    let fixture = args.windows(2).find(|w| w[0] == "--fixture").map(|w| w[1].clone());
    let app = match fixture {
        Some(name) => match Tasuke::fixture(&name) {
            Some(app) => app,
            None => {
                eprintln!("aizome-tasuke: no fixture {name:?}\n\n{}", help());
                std::process::exit(2);
            }
        },
        None => live(),
    };
    aizome_kit::run(app);
}

#[cfg(feature = "live")]
fn live() -> Tasuke {
    Tasuke::live()
}

#[cfg(not(feature = "live"))]
fn live() -> Tasuke {
    eprintln!("aizome-tasuke was built without the agent; showing the home fixture");
    Tasuke::fixture("home").expect("the home fixture")
}
