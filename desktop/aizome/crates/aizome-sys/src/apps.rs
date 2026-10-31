//! Applications: what's installed, how to start it, its icons, and which
//! one opens what. Everything but launching comes from `aizome-apps`, which
//! the compositor uses too (without D-Bus); launching ([`launch()`]) needs
//! the session bus for `DBusActivatable` apps, so it lives here.

pub use aizome_apps::{
    Action, App, AppIndex, DESKTOP_NAME, DefaultRole, DesktopEntry, EntryKind, Exec, ExecContext, IconLookup,
    IconTheme, Locale, TerminalMove, Watcher, XdgDirs, file_uri_to_path, find_program, parse_desktops,
    path_to_file_uri,
};

mod launch;

pub use launch::{
    LaunchEvent, LaunchObserver, LaunchOptions, Launched, activate_dbus, command_lines, dbus_object_path, launch,
    launch_command_observed, launch_in_background, launch_observed, systemd_escape, unit_name,
};
