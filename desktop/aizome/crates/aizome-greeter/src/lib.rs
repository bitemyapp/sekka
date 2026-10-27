//! Aizome's login screen (`docs/greeter.md`): a greetd greeter that runs on
//! Aizome's own compositor and looks like Aizome's lock screen, offering
//! every session installed.

pub mod app;
pub mod clock;
pub mod compositor;
pub mod config;
pub mod displays;
pub mod greetd;
pub mod login;
pub mod sessions;
pub mod users;
pub mod view;

pub const USAGE: &str = "\
usage: aizome-greeter [--config PATH] [--nested] [--demo] | --session ID

Aizome's login screen, run by greetd as its greeter. It starts Aizome's
compositor for itself (without the shell) and stops it on the way out.

  --config PATH   the configuration (default /etc/aizome/greeter.toml)
  --nested        show in the current Wayland session instead of starting
                  a compositor (to look at it)
  --demo          pretend to be greetd: anyone, with the password aizome
  --session ID    start session ID at once (what logging in by itself runs)
";
