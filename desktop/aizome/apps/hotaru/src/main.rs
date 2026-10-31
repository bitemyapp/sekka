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
    let mut app_id = None;
    let mut new_window = false;
    let mut hold = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "Hotaru 蛍 — Aizome's terminal\n\nUsage: aizome-hotaru [--config-file FILE] [--working-directory DIR] [--new-window] [--app-id ID] [--hold] [-- COMMAND [ARG…]]"
                );
                return;
            }
            Some("--version" | "-V") => {
                println!("Hotaru {}", env!("CARGO_PKG_VERSION"));
                return;
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
            Some("--new-window") => new_window = true,
            Some("--hold") => hold = true,
            Some("--app-id") => {
                let id = args.next().and_then(|id| id.into_string().ok());
                let Some(id) = id.filter(|id| {
                    !id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                }) else {
                    eprintln!(
                        "--app-id requires an application ID containing letters, digits, dots, hyphens or underscores"
                    );
                    std::process::exit(2);
                };
                app_id = Some(id);
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
    if let Some(path) = command.cwd.as_mut() {
        *path = std::path::absolute(&*path).unwrap_or_else(|_| path.clone());
    } else {
        command.cwd = std::env::current_dir().ok();
    }
    // A desktop's TUI launch must not become a tab in an unrelated shell:
    // its window ID and lifetime belong to this invocation. Keep ordinary
    // Hotaru launches using the existing per-user tab handoff.
    let instance = if new_window || app_id.is_some() || hold {
        None
    } else {
        match aizome_hotaru::instance::claim(&command) {
            Ok(aizome_hotaru::instance::Claim::Forwarded) => return,
            Ok(aizome_hotaru::instance::Claim::First(instance)) => Some(instance),
            Err(e) => {
                log::warn!("Could not join Hotaru's running instance: {e}");
                None
            }
        }
    };
    let mut app = Hotaru::new(command);
    if let Some(app_id) = app_id {
        app.app_id = app_id;
    }
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
    app.hold = hold;
    app.instance = instance;
    aizome_kit::run(app);
}
