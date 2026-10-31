use std::ffi::OsString;
use std::path::PathBuf;

use aizome_hotaru::app::Hotaru;
use hotaru_io::PtyCommand;

fn main() {
    let mut helper_args = std::env::args_os().skip(1);
    if helper_args.next().as_deref() == Some(std::ffi::OsStr::new("--hotaru-resolve-link")) {
        if aizome_hotaru::resolve_link_helper(helper_args).is_err() {
            std::process::exit(2);
        }
        return;
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut command = PtyCommand::shell();
    let mut settings_path = aizome_hotaru::settings::default_path();
    let mut window = None;
    let mut new_window = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "Hotaru 蛍 — Aizome's terminal\n\nUsage: aizome-hotaru [--new-window | --window PID] [--config-file FILE] [--working-directory DIR] [-- COMMAND [ARG…]]\n\nAn ordinary launch opens a new window. --window PID opens a tab in that\nHotaru window, or a new window if it has closed."
                );
                return;
            }
            Some("--version" | "-V") => {
                println!("Hotaru {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            Some("--new-window") => new_window = true,
            Some("--window") => {
                window = args
                    .next()
                    .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()))
                    .filter(|pid| *pid > 0 && *pid <= i32::MAX as u32);
                if window.is_none() {
                    eprintln!("--window requires a positive process ID");
                    std::process::exit(2);
                }
            }
            Some("--working-directory") => {
                let Some(dir) = args.next() else {
                    eprintln!("--working-directory requires a directory");
                    std::process::exit(2);
                };
                command.cwd = Some(PathBuf::from(dir));
            }
            Some("--config-file") => {
                let Some(path) = args.next() else {
                    eprintln!("--config-file requires a file");
                    std::process::exit(2);
                };
                settings_path = Some(PathBuf::from(path));
            }
            Some("--" | "-e") => {
                let Some(program) = args.next() else {
                    eprintln!("-- requires a command");
                    std::process::exit(2);
                };
                command.program = program;
                command.args = args.collect::<Vec<OsString>>();
                command.integrate_shell = false;
                break;
            }
            _ => {
                eprintln!("Unknown option: {}", arg.to_string_lossy());
                std::process::exit(2);
            }
        }
    }
    if new_window && window.is_some() {
        eprintln!("--new-window and --window cannot be used together");
        std::process::exit(2);
    }
    if let Some(path) = command.cwd.as_mut() {
        *path = std::path::absolute(&*path).unwrap_or_else(|_| path.clone());
    } else {
        command.cwd = std::env::current_dir().ok();
    }
    if let Some(pid) = window {
        match aizome_hotaru::instance::forward(pid, &command) {
            Ok(true) => return,
            Ok(false) => {} // The selected window closed before the launch.
            Err(error) => {
                eprintln!("Could not open a tab in Hotaru window {pid}: {error}");
                std::process::exit(1);
            }
        }
    }
    let instance = aizome_hotaru::instance::listen()
        .map_err(|error| log::warn!("Could not listen for window commands: {error}"))
        .ok();
    let mut app = Hotaru::new(command);
    if let Some(path) = &settings_path {
        match aizome_hotaru::settings::Settings::load(path) {
            Ok(settings) => app.settings = settings,
            Err(error) => {
                eprintln!("Hotaru configuration: {error}");
                std::process::exit(2);
            }
        }
        // A system's configuration (Home Manager, NixOS) links it from the store.
        app.settings_managed = path.is_symlink();
    }
    app.settings_path = settings_path;
    app.instance = instance;
    aizome_kit::run(app);
}
