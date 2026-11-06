// SPDX-License-Identifier: MIT OR Apache-2.0
//! The real helper worker, end to end, against stand-ins for Nix: each
//! script re-runs itself in an unprivileged user and mount namespace whose
//! /run, /nix/var/nix, /var/lib/yukimi, /etc/yukimi and /etc/nixos are
//! private, and checks that before running the helper. They need
//! unprivileged user namespaces, which build sandboxes don't have:
//!
//! ```sh
//! cargo test --locked -p yukimi-helper --test namespace -- --ignored
//! ```
// Tests run the scripts that run the helper; they are not the helper.
#![allow(clippy::disallowed_methods)]
use std::process::Command;

fn script(name: &str) {
    let helper = env!("CARGO_BIN_EXE_yukimi-helper");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(name);
    let out = Command::new("bash").arg(&script).arg(helper).output().expect("bash runs");
    let log = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{name} failed:\n{log}");
}

/// YUK-02: a later boot supersedes an activation that failed after commit.
#[test]
#[ignore = "needs unprivileged user namespaces"]
fn a_later_boot_supersedes_a_failed_activation() {
    script("repro_yuk_02.sh");
}

/// YUK-09: a failed or stopped change puts root's channels back.
#[test]
#[ignore = "needs unprivileged user namespaces"]
fn a_failed_or_stopped_change_puts_the_channels_back() {
    script("repro_yuk_09.sh");
}

/// YUK-10: a reused generation number gets its own saved configuration.
#[test]
#[ignore = "needs unprivileged user namespaces"]
fn a_reused_generation_number_gets_its_own_configuration() {
    script("repro_yuk_10.sh");
}

/// YUK-11: the boot menu is updated before garbage is collected.
#[test]
#[ignore = "needs unprivileged user namespaces"]
fn the_boot_menu_is_updated_before_collecting_garbage() {
    script("repro_yuk_11.sh");
}
