// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tatami-sound`: the Sound window of the Tatami session, after Aizome's
//! Sound settings. It chooses the output and input, sets their volume,
//! balance and port, shows the input's level, sets each application's
//! volume and device, and chooses sound cards' profiles. The session's
//! pipewire-pulse does the work, through pactl (audio.rs), and the level
//! meter's own connection (meter.rs).
mod audio;
mod meter;
mod model;
mod theme;
mod window;
use adw::prelude::*;
use gtk::{gdk, glib};

/// The window's own look; Tatami's stylesheet (theme.rs) goes over it.
const CSS: &str = "
.tile {
  min-width: 26px;
  min-height: 26px;
  border-radius: 6px;
  background-color: alpha(currentColor, 0.1);
}
.tile.current {
  background-color: var(--accent-bg-color);
  color: var(--accent-fg-color);
}
.tile.playing {
  background-color: alpha(#9141ac, 0.25);
}
row.current box.title > label.title {
  font-weight: bold;
}
levelbar.input-level block {
  min-width: 8px;
  min-height: 10px;
}
levelbar.input-level block.filled {
  background-color: var(--success-color);
}
levelbar.input-level block.filled:nth-child(n+19) {
  background-color: var(--warning-color);
}
levelbar.input-level block.filled:nth-child(n+23) {
  background-color: var(--error-color);
}
";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("org.tatami.Sound")
        .build();
    app.connect_startup(|_| {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.load_from_string(CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        // The desktop's look (Tatami's), over the window's own style.
        if let Some(theme) = theme::from_environment() {
            let provider = gtk::CssProvider::new();
            provider.load_from_path(&theme.path);
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
            if theme.dark {
                adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
            }
        }
    });
    app.connect_shutdown(|_| audio::stop_monitor());
    // A second launch brings the open window forward.
    app.connect_activate(window::open);
    app.run()
}
