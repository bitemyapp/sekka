//! `aizome-greeter`: Aizome's login screen. See `docs/greeter.md`.

use std::path::PathBuf;

use aizome_greeter::app::Greeter;
use aizome_greeter::compositor::Compositor;
use aizome_greeter::config::{CONFIG, Config};

fn main() {
    let mut config = PathBuf::from(CONFIG);
    let (mut nested, mut demo) = (false, false);
    let mut session: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => match args.next() {
                Some(p) => config = p.into(),
                None => usage(2),
            },
            "--nested" => nested = true,
            "--session" => match args.next() {
                Some(s) => session = Some(s),
                None => usage(2),
            },
            "--demo" => demo = true,
            "-h" | "--help" => usage(0),
            "-V" | "--version" => {
                println!("aizome-greeter {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            _ => usage(2),
        }
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // A broken configuration mustn't keep anyone from logging in: say so,
    // and come up with the defaults.
    let cfg = Config::load(&config).unwrap_or_else(|e| {
        log::error!("{e}; using the defaults");
        Config::default()
    });
    if let Some(id) = session {
        std::process::exit(start_session(&cfg, &id));
    }
    if !nested {
        // The login screen needs no session bus: without one, nothing of the
        // greeter user's session starts (the settings portal and the rest it
        // would activate). Power goes to logind, on the system bus. It's
        // Night, whatever a portal would say.
        // Safety: no other thread is running yet.
        unsafe {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "disabled:");
            std::env::set_var("SIDESTEP_APPEARANCE", "dark");
        }
    }
    let compositor = if nested {
        None
    } else {
        match Compositor::start(&cfg.compositor, cfg.compositor_settings.as_ref()) {
            Ok(c) => {
                // Safety: no other thread is running yet.
                unsafe { std::env::set_var("WAYLAND_DISPLAY", &c.socket) };
                Some(c)
            }
            Err(e) => {
                log::error!("the compositor: {e}");
                std::process::exit(1);
            }
        }
    };
    let code = match aizome_kit::try_run(Greeter::new(cfg, true, demo)) {
        Ok(_) => 0,
        Err(e) => {
            log::error!("{e}");
            1
        }
    };
    // The display is free before greetd starts the session.
    drop(compositor);
    std::process::exit(code);
}

fn usage(code: i32) -> ! {
    eprint!("{}", aizome_greeter::USAGE);
    std::process::exit(code);
}

/// `--session ID`: become session `ID` (greetd's initial session, when the
/// computer logs in by itself): its environment, then its command.
fn start_session(cfg: &Config, id: &str) -> i32 {
    use std::os::unix::process::CommandExt;
    let sessions = aizome_greeter::sessions::find(&cfg.sessions, &[], !cfg.x11_prefix.is_empty());
    let Some(s) = sessions.iter().find(|s| s.id == id).or_else(|| sessions.first()) else {
        log::error!("no session {id}, nor any other");
        return 1;
    };
    let cmd = s.command(&cfg.x11_prefix);
    let mut c = std::process::Command::new(&cmd[0]);
    c.args(&cmd[1..]);
    for kv in s.env() {
        if let Some((k, v)) = kv.split_once('=') {
            c.env(k, v);
        }
    }
    let e = c.exec();
    log::error!("{}: {e}", cmd[0]);
    1
}
