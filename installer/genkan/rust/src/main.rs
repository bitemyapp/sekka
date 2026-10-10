// SPDX-License-Identifier: GPL-3.0-or-later
mod ui;
use adw::prelude::*;
fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("org.calamares.NixOSRust")
        .build();
    app.connect_startup(|_| ui::load_style());
    app.connect_activate(ui::build);
    app.run()
}
