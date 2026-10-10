// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tatami-displays`: the Displays window of the Tatami session, after
//! macOS's Displays settings. It arranges the displays and sets each one's
//! use (main, extended, mirrored, off), resolution and scale, refresh rate,
//! variable refresh rate, rotation, color (HDR, profile, 10-bit) and the
//! built-in display's brightness, and Night Shift. The `tatami` helper keeps
//! and applies the settings (tool/src/displays.rs).
mod arrangement;
mod model;
mod system;
mod theme;
mod window;

use adw::prelude::*;
use gtk::{gdk, glib};

const CSS: &str = "
.arrangement {
  background-color: alpha(currentColor, 0.04);
  border-radius: 12px;
}
.display-tile {
  background-image: linear-gradient(160deg, #2b3a67, #1a1b26);
  border: 2px solid alpha(white, 0.22);
  border-radius: 6px;
  color: white;
}
.display-tile.selected {
  border-color: @accent_color;
  box-shadow: 0 0 0 3px alpha(@accent_color, 0.3);
}
.display-tile .menu-bar {
  min-height: 5px;
  margin: 3px 6px 0 6px;
  border-radius: 3px;
  background-color: alpha(white, 0.8);
}
.display-tile label {
  margin: 0 6px;
}
.display-tile .title {
  font-weight: bold;
  font-size: 0.9em;
}
.display-tile .subtitle {
  font-size: 0.8em;
  opacity: 0.75;
}
.scale-card {
  padding: 12px;
}
.scale-choice {
  padding: 8px 4px;
  border-radius: 8px;
}
.scale-choice:checked {
  background-color: alpha(@accent_bg_color, 0.25);
}
.scale-screen {
  border: 2px solid alpha(currentColor, 0.5);
  border-radius: 3px;
  background-color: alpha(currentColor, 0.08);
}
.scale-choice:checked .scale-screen {
  border-color: @accent_color;
}
";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("org.tatami.Displays")
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
    // A second launch brings the open window forward.
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        window::build(app).present();
    });
    app.run()
}
