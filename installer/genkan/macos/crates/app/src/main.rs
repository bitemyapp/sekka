// SPDX-License-Identifier: GPL-3.0-or-later
//! `Sekka Installer.app/Contents/MacOS/sekka-installer`.

#[cfg(target_os = "macos")]
fn main() {
    sekka_installer::mac::main()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Sekka Installer runs on macOS. Its behavior is tested here with `cargo test`.");
    std::process::exit(2);
}
