//! The window: Miller columns (or a list, or icons), the sidebar, the
//! path bar, Quick Look, Get Info, the operations window, and the menus.

pub mod actions;
pub mod app;
pub mod browser;
pub mod clipboard;
pub mod dnd;
pub mod dragout;
pub mod fileview;
pub mod instance;
pub mod jobs;
pub mod keys;
pub mod look;
pub mod menus;
pub mod program;
pub mod settings;
pub mod sheets;
pub mod sidebar;
pub mod strip;
pub mod view;

pub use app::{Hako, Msg, Options, Request};

/// Run Hako with the command line's requests, or hand them to the Hako
/// already running.
pub fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: aizome-hako [PATH…]\n       aizome-hako --select FILE\n\nOpens Hako (Files) at each folder, or with each file selected in its folder."
        );
        return;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| crate::home_dir());
    let requests = app::parse_args(&args, &cwd);
    let mut hako = Hako::new(Options::from_env());
    match instance::claim(&requests) {
        instance::Claim::Handed => return,
        instance::Claim::First(l) => hako.instance = Some(l),
        instance::Claim::Alone => {}
    }
    aizome_kit::run(hako.with_requests(requests));
}
