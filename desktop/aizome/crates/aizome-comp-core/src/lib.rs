//! The stable core of Aizome's compositor.
//!
//! The core owns everything a client's connection depends on: the Wayland
//! display and its clients, smithay's protocol states, outputs, the seat,
//! the backends and renderer, the event loop, IPC, the session's helpers
//! and the recovery path. What people hack on — window management, tab
//! groups, Desks, bindings, focus policy, decorations, overlays, motion and
//! the frame's scene — is the logic (`aizome-comp-logic`), a library the
//! core loads at run time and swaps for a new build without disconnecting
//! anyone (docs/reload.md).
//!
//! The core is a Rust dylib: the binary and every logic library link it,
//! so there is one copy of smithay, wayland-server and std, whose statics
//! (smithay's serial counter, the font caches) and types the logic shares.
//! The logic depends on the same crates the core does and gets them from
//! here; the re-exports below are for logic code that prefers one path.

pub mod backend;
pub mod blur;
pub mod cli;
pub mod config;
pub mod cursor;
pub mod fallback;
pub mod focus;
pub mod geom;
pub mod ghosts;
pub mod handlers;
pub mod icons;
pub mod idle;
pub mod input;
pub mod ipc;
pub mod logic;
pub mod night;
pub mod outputs;
pub mod pacing;
pub mod painter;
pub mod pieces;
pub mod popups;
pub mod power;
pub mod protocols;
pub mod recovery;
pub mod reload;
pub mod render;
pub mod requests;
pub mod scene;
pub mod screenshot;
pub mod session;
pub mod shell;
pub mod state;
pub mod stats;
pub mod svg;
pub mod text;
pub mod thumbs;
pub mod timers;
pub mod view;
pub mod watchdog;

pub use aizome_config;
pub use aizome_ipc;
pub use aizome_theme;
pub use serde;
pub use serde_json;
pub use sidestep_ui;
pub use smithay;
pub use tracing;

pub use geom::{Insets, Rect};
pub use logic::{LogicHost, LogicVTable};
pub use state::{Aizome, State};

/// A window's identity: stable while it lives, never reused.
pub type WinId = u64;

/// The core's ABI fingerprint (build.rs): a logic library is loaded only
/// if it was compiled against a core with the same one.
pub const ABI: &str = env!("AIZOME_CORE_ABI");

/// The fingerprint's parts, hashed separately, for saying what differs.
pub const ABI_PARTS: &str = env!("AIZOME_CORE_ABI_PARTS");

/// How the core was built: what `aizome-dev` must build the logic with.
pub mod build {
    pub const RUSTC: &str = env!("AIZOME_CORE_RUSTC");
    pub const TARGET: &str = env!("AIZOME_CORE_TARGET");
    pub const PROFILE: &str = env!("AIZOME_CORE_PROFILE");
    pub const OPT_LEVEL: &str = env!("AIZOME_CORE_OPT_LEVEL");
    pub const RUSTFLAGS: &str = env!("AIZOME_CORE_RUSTFLAGS");
    /// "true" when it was built with Cargo's `--target` (as the Nix package
    /// is): a logic must be too.
    pub const EXPLICIT_TARGET: &str = env!("AIZOME_CORE_EXPLICIT_TARGET");
    /// Each path package's sources, hashed: `name=hash …`.
    pub const SOURCES: &str = env!("AIZOME_CORE_SOURCES");
}

use std::process::ExitCode;

use smithay::reexports::calloop::EventLoop;
use smithay::reexports::calloop::channel;
use smithay::reexports::wayland_server::Display;
use tracing::{error, info, warn};

use crate::backend::Backend;
use crate::cli::BackendKind;

/// Runs the compositor: the `aizome` binary's `main`.
pub fn run() -> ExitCode {
    let filter = std::env::var("AIZOME_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "info,smithay=warn".into());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .init();
    logic::install_panic_hook();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--build-info") {
        println!("{}", logic::build_info_json());
        return ExitCode::SUCCESS;
    }
    let opts = match cli::parse(args) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("aizome: {e}");
            }
            eprint!("{}", cli::USAGE);
            return if e.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(2) };
        }
    };
    let config_path = opts.config.clone().unwrap_or_else(aizome_config::Settings::path);
    let settings = aizome_config::Settings::load_from(&config_path).unwrap_or_else(|e| {
        warn!("{}: {e}; using the defaults", config_path.display());
        aizome_config::Settings::default()
    });

    let mut event_loop: EventLoop<'static, State> = match EventLoop::try_new() {
        Ok(l) => l,
        Err(e) => {
            error!("event loop: {e}");
            return ExitCode::FAILURE;
        }
    };
    let display: Display<State> = match Display::new() {
        Ok(d) => d,
        Err(e) => {
            error!("Wayland display: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The command line or AIZOME_RENDERER, else the settings' choice.
    let mut opts = opts;
    if opts.renderer.is_none() {
        opts.renderer = match settings.renderer_choice() {
            aizome_config::RendererChoice::Auto => None,
            aizome_config::RendererChoice::Gpu => Some(cli::RendererKind::Gles),
            aizome_config::RendererChoice::Software => Some(cli::RendererKind::Pixman),
        };
    }
    let backend = match opts.backend {
        BackendKind::Headless => {
            Backend::Headless(backend::headless::Headless::new(opts.renderer, opts.headless_outputs.clone()))
        }
        BackendKind::Winit => match backend::winit::WinitBackend::new(&event_loop) {
            Ok(w) => Backend::Winit(Box::new(w)),
            Err(e) => {
                error!("winit: {e}");
                return ExitCode::FAILURE;
            }
        },
        BackendKind::Udev => match backend::udev::Udev::new(&event_loop, opts.renderer) {
            Ok(u) => Backend::Udev(Box::new(u)),
            Err(e) => {
                error!("udev: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let mut state = State::new(display, event_loop.handle(), event_loop.get_signal(), backend, opts.clone(), settings);
    match opts.backend {
        BackendKind::Headless => backend::headless::Headless::init(&mut state),
        BackendKind::Winit => backend::winit::WinitBackend::init(&mut state),
        BackendKind::Udev => backend::udev::Udev::init(&mut state),
    }
    info!("{} backend, {} renderer", state.backend.name(), state.backend.renderer_name());

    if let Err(e) = ipc::IpcServer::listen(&mut state) {
        error!("IPC socket: {e}");
        return ExitCode::FAILURE;
    }

    // Settings changes apply live.
    let (tx, rx) = channel::channel::<()>();
    let _watcher = aizome_config::Watcher::spawn_for(config_path, std::time::Duration::from_secs(1), move || {
        let _ = tx.send(());
    });
    let _ = event_loop.handle().insert_source(rx, |event, _, state: &mut State| {
        if let channel::Event::Msg(()) = event
            && let Err(e) = state.reload_config()
        {
            warn!("settings: {e}");
        }
    });

    state.apply_output_settings();
    state.place_pointer_on_primary();
    state.update_night_light(true);
    idle::start(&mut state);
    watchdog::start(&mut state);
    recovery::start(&mut state);

    // The logic: the one asked for, else the installed one. Without it the
    // session still runs (the recovery path), and a reload brings it.
    let path = opts.logic.clone().or_else(|| std::env::var_os("AIZOME_LOGIC").map(Into::into));
    match logic::default_logic_path(path) {
        Some(p) => {
            if let Err(e) = state.load_logic(&p, false) {
                error!("{e}");
                state.notify_logic_problem("Aizome couldn't load its window manager", &e);
            }
        }
        None => error!("no compositor logic library found (--logic, $AIZOME_LOGIC, or beside the binary)"),
    }
    session::start(&mut state);

    if let Err(e) = event_loop.run(None, &mut state, |state| {
        watchdog::beat();
        state.on_loop_iteration()
    }) {
        error!("event loop: {e}");
    }
    session::stop_children(&mut state.az);
    info!("bye");
    ExitCode::SUCCESS
}
