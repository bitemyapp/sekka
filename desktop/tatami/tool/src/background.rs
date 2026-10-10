// SPDX-License-Identifier: MIT OR Apache-2.0
//! The desktop background, chosen from Tatami's wallpapers as Omarchy 4's
//! background switcher does (Style › Background, Super+Ctrl+Space). The
//! choice is a symlink in the state directory, which the background unit and
//! the lock screen read; without one, both use the shipped default.
use crate::{CONFIG, util};
use std::{fs, os::unix::fs::symlink, path::PathBuf};

/// The shipped default, a symlink the NixOS module writes.
fn default() -> PathBuf {
    PathBuf::from(format!("{CONFIG}/background"))
}

fn chosen() -> PathBuf {
    util::state_dir().join("background")
}

/// Point the choice at `target`, replacing any previous one in one step.
fn link(target: &std::path::Path) -> std::io::Result<()> {
    let link = chosen();
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir)?;
    }
    let staged = link.with_extension("new");
    let _ = fs::remove_file(&staged);
    symlink(target, &staged)?;
    fs::rename(&staged, &link)
}

/// Before the background unit starts: a missing choice, or one whose image
/// is gone after an update, becomes the default.
pub fn ensure() {
    if fs::metadata(chosen()).is_err() {
        let _ = link(&default());
    }
}

/// The image in use, for the lock screen.
pub fn path() -> PathBuf {
    if fs::metadata(chosen()).is_ok() {
        chosen()
    } else {
        default()
    }
}

/// Use `image` and show it at once. Anything but a readable file is refused.
pub fn set(image: &str) -> bool {
    let image = PathBuf::from(image);
    if !image.is_absolute() || !fs::metadata(&image).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    if link(&image).is_err() {
        return false;
    }
    util::run(
        "systemctl",
        &["--user", "restart", "--no-block", "tatami-swaybg.service"],
    );
    true
}

/// Style › Background and Super+Ctrl+Space: the wallpapers by name, with the
/// selected one previewed large (the tatami-backgrounds menu the module
/// generates).
pub fn pick() {
    crate::menu::after_menu();
    crate::menu::walker(&[
        "-m",
        "menus:tatami-backgrounds",
        "-p",
        "Background\u{2026}",
        "--minwidth",
        "330",
        "--maxwidth",
        "330",
        "--maxheight",
        "390",
    ]);
}
