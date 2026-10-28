// SPDX-License-Identifier: GPL-3.0-or-later
//! The disk to erase, and how it's laid out and set up.
use super::{FIELD, group, note_row, partitions, sections};
use crate::app::{Field, Genkan, Msg};
use crate::format::size;
use aizome_kit::prelude::*;
use aizome_kit::row;
use calamares_nixos::{Filesystem, memory};

/// A filesystem as the switch between them names it.
fn label(filesystem: Filesystem) -> &'static str {
    match filesystem {
        Filesystem::Ext4 => "ext4",
        Filesystem::Btrfs => "Btrfs",
        Filesystem::Xfs => "XFS",
    }
}

/// The disk icon most likely to match the device type.
fn disk_icon(path: &str) -> Icon {
    if path.starts_with("/dev/mmcblk") {
        Icon::MemoryStick
    } else {
        Icon::HardDrive
    }
}

pub fn page(app: &Genkan) -> Element<Msg> {
    let mut disks: Vec<Element<Msg>> = Vec::new();
    for (index, disk) in app.disks.iter().enumerate() {
        let identity = &disk.identity;
        let model = if identity.model.is_empty() {
            "Unknown disk".to_string()
        } else {
            identity.model.clone()
        };
        let serial = if identity.serial.is_empty() {
            String::new()
        } else {
            format!(" · serial {}", identity.serial)
        };
        match &disk.blocked {
            Some(reason) => disks.push(
                form_row(model, badge("Unavailable").role(Role::Text3))
                    .description(format!(
                        "{} · {} — {reason}",
                        identity.path,
                        size(identity.bytes)
                    ))
                    .icon(disk_icon(&identity.path))
                    .key(index)
                    .into(),
            ),
            None => {
                let choice = radio("", index, app.selected_disk)
                    .on_select(Msg::SelectDisk)
                    .label(format!("Erase {model}"));
                disks.push(
                    mouse_area(
                        form_row(model, choice)
                            .description(format!(
                                "{} · {}{serial}",
                                identity.path,
                                size(identity.bytes)
                            ))
                            .icon(disk_icon(&identity.path)),
                    )
                    .on_press(Msg::SelectDisk(index))
                    .key(index)
                    .into(),
                );
            }
        }
    }
    if app.disks.is_empty() && !app.scanning {
        disks.push(
            form_row("No disks found", space(0.0))
                .description("Connect a disk, then rescan.")
                .into(),
        );
    }
    let mut rescan = row![].spacing(10.0).padding([8.0, 12.0]).width(Fill);
    if app.scanning {
        rescan = rescan
            .push(progress_indeterminate().width(64.0))
            .push(text("Looking for disks…").size(12.0).role(Role::Text3));
    }
    rescan = rescan.push(spacer()).push(
        button("Rescan")
            .icon(Icon::RefreshCw)
            .compact()
            .on_press(Msg::Rescan)
            .enabled(!app.scanning)
            .tooltip("Rescan disks"),
    );
    disks.push(rescan.into());

    let filesystem = Filesystem::ALL
        .iter()
        .position(|f| *f == app.filesystem)
        .unwrap_or(0);
    let mut storage: Vec<Element<Msg>> = vec![
        partitions::view(app),
        form_row(
            "Root filesystem",
            segmented(Filesystem::ALL.map(label), filesystem).on_select(Msg::Filesystem),
        )
        .description(match app.filesystem {
            Filesystem::Ext4 => "General purpose and the most widely used Linux filesystem (default).",
            Filesystem::Btrfs => "Checksums and zstd compression on a single root volume. Snapshots are not configured.",
            Filesystem::Xfs => "High-performance journaling for large files and parallel I/O.",
        })
        .into(),
        form_row("Swap sized to memory, with zswap", toggle(app.swap).on_toggle(Msg::Swap))
            .description(match app.memory {
                Some(info) => format!(
                    "A {} swap partition matching installed memory. zswap compresses pages in RAM before they reach the disk, and hibernation can resume from it.",
                    size(memory::swap_bytes(info.total))
                ),
                None => "Matches installed memory. zswap compresses pages in RAM before they reach the disk, and hibernation can resume from it.".into(),
            })
            .into(),
        form_row(
            "Encrypt the installation with LUKS2",
            toggle(app.encrypt).on_toggle(Msg::Encrypt),
        )
        .description("Protects the system, personal files and swap. Unlock at startup with a separate passphrase. Boot files remain unencrypted. TRIM reveals which blocks are unused.")
        .into(),
    ];
    if app.encrypt {
        let passphrase_error = (app.flagged == Some(Field::Passphrase))
            .then(|| app.passphrase_error().unwrap_or_default())
            .filter(|e| !e.is_empty());
        let repeat_error = (app.flagged == Some(Field::PassphraseRepeat))
            .then(|| "Disk passphrases do not match.".to_string());
        storage.push(
            form_row(
                "Disk passphrase",
                text_field(&app.passphrase)
                    .password()
                    .placeholder("At least 12 characters")
                    .on_change(Msg::Passphrase)
                    .error(passphrase_error)
                    .id(Field::Passphrase.id())
                    .width(FIELD),
            )
            .into(),
        );
        storage.push(
            form_row(
                "Confirm disk passphrase",
                text_field(&app.passphrase_repeat)
                    .password()
                    .on_change(Msg::PassphraseRepeat)
                    .error(repeat_error)
                    .id(Field::PassphraseRepeat.id())
                    .width(FIELD),
            )
            .into(),
        );
        storage.push(note_row(
            "Remember your disk passphrase: it cannot be recovered or reset through your login account. Use the live session’s keyboard layout when typing, and select the same layout later for startup.",
        ));
    }
    storage.push(
        form_row("Performance tuning", toggle(app.tuning).on_toggle(Msg::Tuning))
            .description("CachyOS-inspired defaults: memory and writeback sysctls, I/O schedulers, ananicy-cpp process priorities and systemd-oomd. Uses the stock kernel and packages.")
            .into(),
    );
    sections(vec![
        group(
            "Disk to Erase",
            "Everything on the selected disk will be erased. Mounted disks, the live USB and disks smaller than 24 GiB are unavailable. No disk is selected for you.",
            disks,
        ),
        group("Storage", "", storage),
    ])
}
