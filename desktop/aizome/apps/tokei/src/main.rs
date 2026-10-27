//! `aizome-tokei`: the clock. `--ring <id>` rings an alarm (its systemd
//! timer runs that); `--sync` schedules the alarms' timers again (at
//! login) and exits.

use aizome_tokei::alarms::{self, Recorder, Scheduler, Systemd};
use aizome_tokei::app::{Tokei, state_path};
use aizome_tokei::instance::{self, Claim};
use aizome_tokei::{ring, sound};

/// The scheduler: systemd's, or one that only logs what it would do
/// (`AIZOME_TOKEI_DRY_RUN=1`, for trying Tokei without touching timers).
fn scheduler() -> Box<dyn Scheduler> {
    if std::env::var_os("AIZOME_TOKEI_DRY_RUN").is_some_and(|v| v != "0") {
        Box::new(Recorder::new())
    } else {
        Box::new(Systemd::default())
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--ring") => {
            let Some(id) = args.get(1) else {
                eprintln!("aizome-tokei --ring <alarm id>");
                std::process::exit(2);
            };
            let mut notifier = ring::Dbus::connect();
            let mut player = sound::System::default();
            let mut s = scheduler();
            let code =
                ring::ring_with(&alarms::alarms_path(), id, &mut notifier, &mut player, s.as_mut(), ring::RING_FOR);
            std::process::exit(code);
        }
        Some("--sync") => {
            let list = alarms::load(&alarms::alarms_path());
            let mut s = scheduler();
            let errors = alarms::sync(s.as_mut(), &list, jiff::Timestamp::now(), &aizome_tokei::clock::local_zone());
            for e in &errors {
                eprintln!("{e}");
            }
            std::process::exit(if errors.is_empty() { 0 } else { 1 });
        }
        Some("--help" | "-h") => {
            println!("aizome-tokei [--ring <alarm id> | --sync]");
            return;
        }
        _ => {}
    }
    let mut app =
        Tokei::new(Some(state_path()), Some(alarms::alarms_path()), scheduler(), Box::new(sound::System::default()));
    match instance::claim("tokei", &args) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    aizome_kit::run(app);
}
