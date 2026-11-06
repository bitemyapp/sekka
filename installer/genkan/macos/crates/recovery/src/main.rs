// SPDX-License-Identifier: GPL-3.0-or-later
//! `Sekka Recovery.app/Contents/MacOS/sekka-recovery`.

#[cfg(target_os = "macos")]
fn main() {
    sekka_recovery::mac::main()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Finish Installing Sekka runs in a Mac's Recovery. Its steps are tested here with `cargo test`.");
    std::process::exit(2);
}
