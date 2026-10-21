// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tatami-network`: the Network window of the Tatami session, after macOS's
//! Network settings. It turns Wi-Fi on and off, joins networks (with a
//! password, an organization's username and password, or by name when
//! hidden), shows wired connections, connects VPNs and adds WireGuard ones,
//! and sets each connection's joining, metering, private address, IPv4
//! addresses, DNS and IPv6. NetworkManager does the work, through nmcli
//! (nm.rs). `tatami-network --join NAME` opens it to join that network, as
//! the network panel does for enterprise networks.
mod details;
mod dns;
mod join;
mod model;
mod nm;
mod profile;
mod theme;
mod window;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

const CSS: &str = "
.network-status {
  padding: 6px 12px 0 12px;
}
";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("org.tatami.Network")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
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
    app.connect_shutdown(|_| nm::stop_monitor());
    // A second launch brings the open window forward, and joins the network
    // it names.
    app.connect_command_line(|app, line| {
        let args: Vec<String> = line
            .arguments()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let join = args
            .windows(2)
            .find(|pair| pair[0] == "--join")
            .map(|pair| pair[1].clone());
        let ui = window::open(app);
        if let Some(ssid) = join {
            window::join_by_name(&ui, ssid);
        }
        glib::ExitCode::SUCCESS
    });
    app.run()
}
