// SPDX-License-Identifier: GPL-3.0-or-later
//! `sekka-installer-helper`: started by launchd as root, never by hand.

#[cfg(target_os = "macos")]
fn main() {
    sekka_mac_helper::macos::main()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("sekka-installer-helper runs on macOS, started by launchd for Sekka Installer.");
    std::process::exit(2);
}
