// SPDX-License-Identifier: GPL-3.0-or-later
//! Everything chosen, the disk that will be erased, the confirmation it
//! takes, and the preparation going on meanwhile.
use super::{group, property, sections};
use crate::app::{Genkan, Msg, PrepState, Review};
use crate::format::size;
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use calamares_nixos::{Filesystem, Firmware, disk::Layout, memory};

fn partitions(app: &Genkan, review: &Review) -> String {
    let disk = &review.disk;
    let swap = review
        .swap
        .then(|| app.memory.map(|m| memory::swap_bytes(m.total)))
        .flatten();
    match Layout::for_install(disk.bytes, review.firmware, swap, review.encrypt) {
        Ok(layout) => {
            let mut parts = vec![
                if review.firmware == Firmware::Uefi {
                    "EFI 1.0 GiB".to_string()
                } else {
                    "BIOS boot 2 MiB".to_string()
                },
                format!("NixOS {}", size(layout.root_bytes())),
            ];
            if layout.bios_boot.is_some() {
                parts.push("Unencrypted /boot 1.0 GiB".into());
            }
            if let Some(bytes) = layout.swap_bytes() {
                parts.push(format!("swap {} with zswap", size(bytes)));
            }
            parts.join(" · ")
        }
        Err(error) => format!("{error:#}"),
    }
}

fn zone(app: &Genkan, review: &Review) -> String {
    app.zones
        .as_ref()
        .and_then(|z| z.places.get(z.find(&review.timezone)?))
        .filter(|p| p.position.is_some())
        .and_then(|p| Some(format!("{} — {} ({})", p.name(false)?, p.label(), p.zone)))
        .unwrap_or_else(|| review.timezone.clone())
}

/// What went wrong, to read and copy.
pub fn failure(message: &str) -> Element<Msg> {
    column![
        container(
            text(message.to_string())
                .mono()
                .size(11.5)
                .wrap()
                .selectable()
                .width(Fill)
                .id("failure-details"),
        )
        .padding(10.0)
        .fill(Tone::Sunk)
        .border(Edge::Line)
        .width(Fill),
        row![
            spacer(),
            button("Copy Details")
                .icon(Icon::Copy)
                .compact()
                .on_press(Msg::CopyFailure),
        ]
        .width(Fill),
    ]
    .spacing(8.0)
    .width(Fill)
    .into()
}

fn preparation(app: &Genkan) -> Element<Msg> {
    let prep = &app.preparation;
    let status: Element<Msg> = match prep.state {
        PrepState::Working => progress_indeterminate().width(40.0).into(),
        PrepState::Done => icon(Icon::CircleCheck)
            .size(18.0)
            .role(Role::Success)
            .into(),
        PrepState::Failed => icon(Icon::CircleX).size(18.0).role(Role::Danger).into(),
    };
    let mut lines = column![
        text(prep.title.clone())
            .weight(650.0)
            .wrap()
            .id("preparation")
    ]
    .spacing(3.0)
    .width(Fill);
    if !prep.subtitle.is_empty() {
        lines = lines.push(
            text(prep.subtitle.clone())
                .size(12.0)
                .role(Role::Text3)
                .wrap(),
        );
    }
    let mut rows: Vec<Element<Msg>> = vec![
        row![status, lines]
            .spacing(12.0)
            .padding([10.0, 12.0])
            .width(Fill)
            .into(),
    ];
    if let Some(fraction) = prep.progress {
        rows.push(
            container(progress(fraction).width(Fill))
                .padding([0.0, 12.0, 12.0, 12.0])
                .width(Fill)
                .into(),
        );
    }
    if let Some(message) = &prep.failure {
        rows.push(container(failure(message)).padding(12.0).width(Fill).into());
    }
    group(
        "Preparing in the Background",
        "The installer checks its files and your configuration, then builds in memory when space allows. Otherwise, downloading and building wait until after formatting. Nothing is written to the target disk until you confirm installation.",
        rows,
    )
}

pub fn page(app: &Genkan) -> Element<Msg> {
    let Some(review) = &app.review else {
        return text("Checking your settings…").role(Role::Text3).into();
    };
    let disk = &review.disk;
    let model = if disk.model.is_empty() {
        "the selected disk"
    } else {
        disk.model.as_str()
    };
    let serial = if disk.serial.is_empty() {
        String::new()
    } else {
        format!(" · serial {}", disk.serial)
    };
    let erase = container(
        row![
            icon(Icon::TriangleAlert).size(28.0).role(Role::Danger),
            column![
                text(format!("Everything on {model} will be erased"))
                    .size(15.0)
                    .weight(700.0)
                    .role(Role::Danger)
                    .wrap(),
                text(format!(
                    "{} · {}{serial} — all partitions and data on this disk will be destroyed.",
                    disk.path,
                    size(disk.bytes)
                ))
                .wrap(),
            ]
            .spacing(4.0)
            .width(Fill),
        ]
        .spacing(16.0)
        .width(Fill),
    )
    .padding(16.0)
    .fill(Tone::Sunk)
    .border(Edge::Line)
    .width(Fill);
    let confirm = group(
        "Confirm",
        "Installation uses the media's pinned Determinate Nix flake. This cannot be undone.",
        vec![
            form_row(
                format!("Type {} to confirm", app.phrase),
                text_field(&app.erase)
                    .placeholder(app.phrase.clone())
                    .mono()
                    .on_change(Msg::Erase)
                    .id("erase")
                    .width(260.0),
            )
            .into(),
            container(
                checkbox(
                    "I understand that all partitions and data on this disk will be destroyed",
                    app.consent,
                )
                .on_toggle(Msg::Consent)
                .id("consent"),
            )
            .padding([10.0, 12.0])
            .width(Fill)
            .into(),
        ],
    );
    let filesystem = format!(
        "{}{}{}",
        review.filesystem.name(),
        if review.filesystem == Filesystem::Btrfs {
            " with zstd compression"
        } else {
            ""
        },
        if review.encrypt && review.swap {
            " · LUKS2: root and swap encrypted"
        } else if review.encrypt {
            " · LUKS2: root encrypted"
        } else {
            " · Unencrypted"
        }
    );
    let user = if review.full_name.is_empty() {
        review.username.clone()
    } else {
        format!("{} ({})", review.full_name, review.username)
    };
    let wifi = if review.wifi {
        match review.wifi_profiles {
            1 => "1 saved network will be copied".to_string(),
            n => format!("{n} saved networks will be copied"),
        }
    } else {
        "Not copied".to_string()
    };
    sections(vec![
        erase.into(),
        confirm,
        preparation(app),
        group(
            "Disk",
            "",
            vec![
                property(
                    "Disk to erase",
                    format!("{model} · {} · {}", disk.path, size(disk.bytes)),
                ),
                property("New partitions", partitions(app, review)),
                property("Root filesystem", filesystem),
                property(
                    "Boot",
                    match review.firmware {
                        Firmware::Uefi => "UEFI with systemd-boot",
                        Firmware::Bios => "Legacy BIOS with GRUB",
                    },
                ),
            ],
        ),
        group(
            "System",
            "",
            vec![
                property(
                    "Desktops",
                    review
                        .desktops
                        .iter()
                        .map(|d| d.label())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                property("Default session", review.default_desktop.label()),
                property(
                    "Applications",
                    if review.applications.is_empty() {
                        "None"
                    } else {
                        &review.applications
                    },
                ),
                property(
                    "Performance tuning",
                    if review.tuning {
                        "CachyOS-inspired defaults"
                    } else {
                        "NixOS defaults"
                    },
                ),
                property(
                    "Unfree software",
                    if review.unfree {
                        "Allowed"
                    } else {
                        "Not allowed (redistributable firmware is still included)"
                    },
                ),
                property("Graphics driver", review.graphics.clone()),
                property("Wi-Fi networks", wifi),
            ],
        ),
        group(
            "Account and Location",
            "",
            vec![
                property("Computer name", review.hostname.clone()),
                property("User", user),
                property("Language", review.locale.clone()),
                property("Time zone", zone(app, review)),
                property("Keyboard layout", review.keyboard.clone()),
                property("Modifier keys", Genkan::super_key_name(review.super_key)),
            ],
        ),
        group(
            "GitHub, SSH and Git",
            "",
            vec![
                property("SSH keys", review.ssh_keys.clone()),
                property("SSH server", review.ssh_server.clone()),
                property("Git identity", review.git.clone()),
            ],
        ),
    ])
}
