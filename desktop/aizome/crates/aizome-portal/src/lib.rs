//! Aizome's xdg-desktop-portal backend.
//!
//! Apps ask `xdg-desktop-portal` to open and save files (browsers,
//! Electron apps, Zed, sandboxed apps, GTK apps told to); in an Aizome
//! session its configuration (`aizome-portals.conf`) sends those requests
//! here, `org.freedesktop.impl.portal.FileChooser` on
//! `org.freedesktop.impl.portal.desktop.aizome`.
//!
//! The service ([`service`]) holds no window. Each request runs a chooser
//! of its own ([`chooser`], this program again with `--file-chooser`),
//! which reads the request as JSON on its standard input, shows the
//! window over the app that asked, and writes its answer to its standard
//! output; closing the request ends it. A chooser that fails takes no
//! other request with it.

pub mod chooser;
pub mod filter;
pub mod memory;
pub mod request;
pub mod service;
pub mod worker;

/// The bus name the portal frontend finds this backend by.
pub const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.aizome";
/// Where backends serve their interfaces.
pub const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
/// The chooser's app id, for the compositor and the shell.
pub const CHOOSER_APP_ID: &str = "org.aizome.FileChooser";
