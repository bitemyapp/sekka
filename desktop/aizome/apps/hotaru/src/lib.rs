pub mod app;
mod clipboard;
mod font;
pub mod input;
pub mod instance;
mod links;
pub mod panes;
mod search;
mod session;
pub mod settings;
mod terminal_view;

#[cfg(not(target_os = "linux"))]
compile_error!("Hotaru supports Linux only.");

mod sprites;
