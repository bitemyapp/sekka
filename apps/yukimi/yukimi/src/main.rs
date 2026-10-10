// SPDX-License-Identifier: MIT OR Apache-2.0
//! Yukimi (雪見, "snow-viewing"): a window onto a NixOS system. See what is
//! installed and why, find what is available, try it, install it for
//! yourself or for everyone, update, return to an earlier version, and keep
//! the store tidy, without editing configuration by hand.
use adw::prelude::*;

mod model;
mod ops;
mod stalls;
mod ui;

const APP_ID: &str = "io.github.bitemyapp.Yukimi";

/// Yukimi's cache directory, `~/.cache/yukimi`.
pub fn cache_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("yukimi"))
}

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(include_str!("../data/style.css"));
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    });
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        stalls::watch();
        ui::build_window(app);
    });
    app.run()
}
