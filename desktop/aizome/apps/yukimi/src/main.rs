//! Yukimi 雪見 (Software): Sekka's Nix software manager, made native.
//!
//! `aizome-yukimi [--page PAGE] [--choose SETTING=VALUE[,VALUE]]... [--early |
//! --nightly] [--sample]`: open Yukimi (on that page, proposing those
//! changes). A second launch hands its arguments to the running Yukimi.
//! `--sample` shows a sample system and changes nothing.
use aizome_yukimi::app::Yukimi;
use aizome_yukimi::instance::{self, Instance};

const HELP: &str = "Usage: aizome-yukimi [--page PAGE] [--choose SETTING=VALUE[,VALUE]]...
                     [--early | --nightly] [--sample]

Yukimi (雪見), Aizome's software manager for Sekka's NixOS: see what is
installed and why, find what is available, install it for yourself or for
everyone, update, go back to an earlier version, keep the store tidy, and
choose the system's desktops and login screen.

  --page PAGE   overview, installed, discover, updates, history, storage or
                desktops
  --choose S=V  propose one of the system's choices on the Desktops page, such
                as --choose calamares.loginScreen=aizome; Apply builds it
  --early       propose updating Sekka early, from its main branch
  --nightly     propose updating Sekka from the nightly again
  --sample      show a sample system instead of this one (changes nothing)
  -h, --help    show this

Nothing proposed changes until you confirm it and enter an administrator
password.

Environment:
  AIZOME_YUKIMI_ETC   read the system's configuration from this directory
                      instead of /etc (looking only: nothing can be changed)
  YUKIMI_STALLS       say on standard error when the window is busy for
                      longer than this many milliseconds (50 when empty)
";

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return;
    }
    let launch = match aizome_yukimi::app::launch_from_args(&args) {
        Ok(launch) => launch,
        Err(e) => {
            eprintln!("aizome-yukimi: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    let sample = args.iter().any(|a| a == "--sample");
    let listener = if sample {
        None
    } else {
        match instance::claim(&args) {
            Instance::Handed => return,
            Instance::First(listener) => listener,
        }
    };
    let mut app = if sample { Yukimi::sample() } else { Yukimi::new() };
    if let Some(page) = launch.page {
        app.page = page;
    }
    app.launch = Some(launch);
    app.listener = listener;
    aizome_kit::run(app);
    if !sample {
        instance::release();
    }
}
