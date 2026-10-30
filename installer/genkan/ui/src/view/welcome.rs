// SPDX-License-Identifier: GPL-3.0-or-later
//! What the installer does, and what it erases.
use crate::app::Msg;
use aizome_kit::prelude::*;
use aizome_kit::{column, row};

const FEATURES: [(Icon, &str, &str); 3] = [
    (
        Icon::Zap,
        "Checked before erasing",
        "The installer checks its files and your configuration before erasing the disk. With limited memory, downloading and building may continue on the new disk after formatting.",
    ),
    (
        Icon::ShieldCheck,
        "Pinned and reproducible",
        "Installs exactly the tested Nixpkgs and Determinate Nix revisions on this media.",
    ),
    (
        Icon::Gauge,
        "Tuned for desktops",
        "RAM-sized swap with zswap and CachyOS-inspired kernel and I/O defaults.",
    ),
];

pub fn page() -> Element<Msg> {
    let intro = column![
        text("Install Sekka").size(26.0).weight(700.0),
        text("With Determinate Nix and your choice of desktops and applications. Review the installation plan before confirming that the disk can be erased.")
            .role(Role::Text2)
            .wrap(),
    ]
    .spacing(8.0)
    .width(Fill);
    let features = aizome_kit::widgets::layout::row(
        FEATURES
            .iter()
            .map(|(glyph, title, body)| {
                container(
                    column![
                        icon(*glyph).size(20.0).role(Role::Accent),
                        text(*title).weight(650.0).wrap(),
                        text(*body).size(12.5).role(Role::Text3).wrap(),
                    ]
                    .spacing(8.0)
                    .width(Fill),
                )
                .padding(14.0)
                .fill(Tone::Chrome)
                .border(Edge::Line)
                .bevel()
                .width(Fill)
                .height(Fill)
                .into()
            })
            .collect(),
    )
    .spacing(12.0)
    .align(Align::Start)
    .width(Fill);
    let warning = container(
        row![
            icon(Icon::TriangleAlert).size(18.0).role(Role::Warning),
            text("This installer erases one whole disk: GPT with ext4, Btrfs or XFS, UEFI or legacy BIOS. It does not support manual partitioning, keeping another operating system, or offline installation. Back up your data before continuing.")
                .wrap()
                .width(Fill),
        ]
        .spacing(12.0)
        .align(Align::Start)
        .width(Fill),
    )
    .padding(14.0)
    .fill(Tone::Sunk)
    .border(Edge::Line)
    .width(Fill);
    column![intro, features, warning]
        .spacing(24.0)
        .width(Fill)
        .into()
}
