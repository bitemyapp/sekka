//! Aizome's system services: what the shell (menu bar widgets, Deskbar, OSD,
//! notifications, lock screen, polkit dialog) and the apps (Settei,
//! Tansu, Kura, Myaku and the rest) ask of the system.
//!
//! # Services
//!
//! Each D-Bus service is a handle built with a [`Wake`] callback:
//!
//! - **`snapshot()`** returns the current state as an `Arc`, cheaply, from a
//!   cache that the daemon's signals keep current (one background thread
//!   per service follows them);
//! - **`generation()`** counts changes, and the wake is called on each one,
//!   so the UI redraws when the counter moves; **`on_change`** adds more
//!   wakes;
//! - **action methods** return a [`Task`] at once and do the work on a
//!   thread; the wake is called again when the task is done, and its
//!   [`Result`] can then be read;
//! - **`available()`** says whether the daemon is there. Every service
//!   degrades: no BlueZ means `Bluetooth::available() == false` and an
//!   empty snapshot, never an error at start or a panic.
//!
//! The crate doesn't depend on any toolkit: the wake is any
//! `Fn() + Send + Sync`, such as a closure around a `sidestep_ui::Proxy`.
//! Every D-Bus call has a timeout (2 seconds for queries; see [`bus`]).
//!
//! Services that only read the kernel or files ([`sysinfo`], [`apps`],
//! [`keyboard`]) are plain functions and samplers the UI calls when it
//! wants to.
//!
//! # Modules
//!
//! | Module | What | Through | NixOS needs |
//! |---|---|---|---|
//! | [`network`] | devices, Wi-Fi, VPNs, airplane mode, hotspot | NetworkManager; `/dev/rfkill` | `networking.networkmanager.enable` |
//! | [`bluetooth`] | adapters, devices, pairing agent | BlueZ | `hardware.bluetooth.enable` |
//! | [`audio`] | outputs, inputs, streams, volume keys | PipeWire through pipewire-pulse | `services.pipewire.pulse.enable` |
//! | [`power`] | batteries, profiles, suspend and power off, lock before sleep, brightness | UPower, power-profiles-daemon, logind, sysfs | `services.upower.enable`, `services.power-profiles-daemon.enable` |
//! | [`datetime`] | time zone, network time, locale, hostname; the time zone list | timedated, localed, hostnamed; tzdata | `time.timeZone = null` to let Settings change it |
//! | [`users`] | accounts, names, pictures, passwords | AccountsService | `services.accounts-daemon.enable`, `users.mutableUsers = true` |
//! | [`printers`] | printers, jobs, discovery | CUPS's tools | `services.printing.enable` |
//! | [`disks`] | drives, partitions, filesystems, LUKS, images, jobs | UDisks2 | `services.udisks2.enable` |
//! | [`mpris`] | media players, media keys | MPRIS on the session bus | — |
//! | [`notifications`] | the notification server (the shell) and a client (apps) | `org.freedesktop.Notifications` | — |
//! | [`statusnotifier`] | apps' status icons and their menus (the watcher, for the shell) | `org.kde.StatusNotifierWatcher`, `com.canonical.dbusmenu` | — |
//! | [`polkit`] | the authentication agent | polkit's helper | `security.polkit.enable` |
//! | [`sysinfo`] | CPU, memory, processes, I/O, sensors, hardware | `/proc`, `/sys` | `pkgs.hwdata` for GPU names |
//! | [`apps`] | desktop entries, launching, icons, default apps | XDG files, `systemd-run` | — |
//! | [`keyboard`] | layouts, variants, XKB options | xkeyboard-config's `evdev.xml` | — |
//! | [`location`] | where the computer is, for sunrise and sunset | GeoClue 2 | `services.geoclue2.enable` |
//!
//! Each module's documentation says more about what it needs and how it
//! degrades without it.

pub mod apps;
pub mod audio;
pub mod bluetooth;
pub mod bus;
pub mod datetime;
pub mod disks;
pub mod error;
pub mod keyboard;
pub mod location;
pub mod meter;
pub mod mpris;
pub mod network;
pub mod notifications;
mod oneshot;
pub mod polkit;
pub mod power;
pub mod printers;
mod service;
pub mod statusnotifier;
pub mod sysinfo;
pub mod users;

pub use bus::Bus;
pub use error::{Error, Result};
pub use service::{Task, Wake};
