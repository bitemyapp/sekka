pub mod app;
mod automatic_links;
mod clipboard;
mod font;
pub mod input;
pub mod instance;
mod link_resolver;
mod links;

#[doc(hidden)]
pub fn resolve_link_helper(args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), &'static str> {
    link_resolver::helper(args)
}
pub mod panes;
pub mod preferences;
mod search;
mod session;
pub mod settings;
mod terminal_view;

#[cfg(not(target_os = "linux"))]
compile_error!("Hotaru supports Linux only.");

mod sprites;
