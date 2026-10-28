pub mod app;
mod clipboard;
pub mod input;
pub mod instance;
pub mod panes;
mod session;
pub mod settings;
mod terminal_view;

#[cfg(not(target_os = "linux"))]
compile_error!("Hotaru supports Linux only.");
