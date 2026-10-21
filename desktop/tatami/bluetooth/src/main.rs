// SPDX-License-Identifier: MIT OR Apache-2.0
mod agent;
mod bluez;
mod model;
mod theme;
mod window;

use adw::prelude::*;
use gtk::glib;

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("org.tatami.Bluetooth")
        .build();
    app.connect_startup(|_| theme::install());
    app.connect_activate(window::open);
    app.run()
}
