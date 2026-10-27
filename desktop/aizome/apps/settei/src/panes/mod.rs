//! The panes. To add one: write it (the contract is in [`crate::pane`]),
//! add its `mod` line below and one line to [`all`]. The sidebar groups
//! them by [`Group`](crate::pane::Group), keeping this list's order within
//! each group.
//!
//! Pane ids are stable (the palette and the shell's systray panels open
//! them): `network`, `bluetooth`, `notifications`, `sound`, `general`,
//! `appearance`, `accessibility`, `desktop`, `menubar`, `displays`,
//! `battery`, `performance`, `search`, `lock`, `users`, `keyboard`,
//! `mouse`, `trackpad`, `printers`.

use crate::pane::{AnyPane, pane};

mod accessibility;
mod appearance;
mod battery;
mod bluetooth;
mod desktop;
mod displays;
mod general;
pub(crate) mod hw;
mod keyboard;
mod lock;
mod menubar;
mod mouse;
mod network;
mod notifications;
mod performance;
mod printers;
mod search;
mod sound;
mod trackpad;
mod users;

/// Every pane, in order.
pub fn all() -> Vec<Box<dyn AnyPane>> {
    vec![
        // Connections.
        pane(network::Network::default()),
        pane(bluetooth::Bluetooth::default()),
        // Alerts.
        pane(notifications::Notifications::default()),
        pane(sound::Sound::default()),
        // System.
        pane(general::General::default()),
        pane(appearance::Appearance::default()),
        pane(accessibility::Accessibility),
        pane(desktop::Desktop::default()),
        pane(menubar::MenuBar),
        pane(displays::Displays::default()),
        pane(battery::Battery::default()),
        pane(performance::Performance::default()),
        pane(search::Search::default()),
        // Security.
        pane(lock::Lock),
        pane(users::Users::default()),
        // Devices.
        pane(keyboard::Keyboard::default()),
        pane(mouse::Mouse),
        pane(trackpad::Trackpad::default()),
        pane(printers::Printers::default()),
    ]
}

/// Other names a pane (or an option in one) is opened by: `--pane wifi`
/// opens Network, `--pane datetime` General's Date & Time.
pub fn resolve(id: &str) -> (String, Option<String>) {
    let (pane, entry): (&str, Option<&str>) = match id {
        "wifi" | "wi-fi" | "ethernet" | "vpn" | "internet" => ("network", None),
        "audio" | "volume" => ("sound", None),
        "display" | "monitor" | "monitors" | "screen" | "screens" => ("displays", None),
        "power" | "energy" => ("battery", None),
        "datetime" | "date-time" | "date" | "time" | "clock" | "timezone" => ("general", Some("datetime")),
        "about" => ("general", Some("about")),
        "updates" | "software-update" | "update" => ("general", Some("software-update")),
        "storage" => ("general", Some("storage")),
        "language" | "region" | "locale" | "language-region" => ("general", Some("language")),
        "default-apps" | "defaults" => ("general", Some("default-apps")),
        "login-items" | "autostart" => ("general", Some("login-items")),
        "startup" | "start-up" | "boot" => ("general", Some("startup")),
        "boot-splash" | "splash" | "plymouth" => ("general", Some("boot-splash")),
        "printer" | "scanners" | "printers-scanners" => ("printers", None),
        "dock" | "deskbar" | "desks" => ("desktop", None),
        "menu-bar" | "systray" => ("menubar", None),
        "lock-screen" | "lockscreen" => ("lock", None),
        "accounts" | "user" | "users-groups" => ("users", None),
        "theme" | "dark-mode" | "wallpaper" => ("appearance", None),
        "a11y" => ("accessibility", None),
        "shortcuts" | "keys" => ("keyboard", Some("shortcuts")),
        "touchpad" => ("trackpad", None),
        "pointer" => ("mouse", None),
        "frame-stats" | "stats" | "renderer" => ("performance", None),
        "palette" => ("search", None),
        other => (other, None),
    };
    (pane.to_owned(), entry.map(str::to_owned))
}
