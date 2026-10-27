// SPDX-License-Identifier: GPL-3.0-or-later
//! The desktops to install, the one the login screen preselects, and what
//! comes along: the live session's Wi-Fi and unfree software.
use super::{group, sections};
use crate::app::{Genkan, Msg};
use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::{column as column_of, row as row_of};
use aizome_kit::{column, row};
use calamares_nixos::Desktop;

/// Cards per line.
const ACROSS: usize = 3;

fn monogram(desktop: Desktop) -> (&'static str, Color, Color) {
    let white = Color::rgb8(0xff, 0xff, 0xff);
    match desktop {
        Desktop::Aizome => (
            "藍",
            Color::rgb8(0x2e, 0x3d, 0x72),
            Color::rgb8(0xe0, 0xaf, 0x68),
        ),
        Desktop::Plasma => ("K", Color::rgb8(0x2d, 0x8e, 0xc7), white),
        Desktop::Gnome => ("G", Color::rgb8(0x3e, 0x80, 0xd0), white),
        Desktop::Xfce => ("X", Color::rgb8(0x3e, 0x78, 0xbe), white),
        Desktop::Hyprland => ("H", Color::rgb8(0x19, 0xbf, 0xbc), white),
        Desktop::Tatami => ("T", Color::rgb8(0x52, 0x68, 0xa0), white),
    }
}

fn card(app: &Genkan, desktop: Desktop) -> Element<Msg> {
    let on = app.desktops.contains(&desktop);
    let (letter, ground, ink) = monogram(desktop);
    let mark = container(text(letter).size(17.0).weight(700.0).color(ink).center())
        .fill(Tone::Color(ground))
        .center()
        .width(34.0)
        .height(34.0);
    container(
        column![
            row![
                mark,
                checkbox(desktop.label(), on)
                    .on_toggle(move |on| Msg::ToggleDesktop(desktop, on))
                    .id(format!("desktop-{}", desktop.id())),
            ]
            .spacing(10.0),
            text(desktop.description())
                .size(12.5)
                .role(Role::Text3)
                .wrap(),
        ]
        .spacing(10.0)
        .width(Fill),
    )
    .padding(12.0)
    .fill(if on { Tone::Raised } else { Tone::Chrome })
    .border(if on { Edge::Focus } else { Edge::Line })
    .width(Fill)
    .height(Fill)
    .key(desktop.id())
    .into()
}

pub fn page(app: &Genkan) -> Element<Msg> {
    let lines = Desktop::ALL
        .chunks(ACROSS)
        .map(|line| {
            let mut cards: Vec<Element<Msg>> = line.iter().map(|d| card(app, *d)).collect();
            // The last line's cards keep the others' width.
            while cards.len() < ACROSS {
                cards.push(space(0.0).width(Fill));
            }
            row_of(cards)
                .spacing(10.0)
                .align(Align::Start)
                .width(Fill)
                .into()
        })
        .collect();
    let labels: Vec<&str> = app.desktops.iter().map(|d| d.label()).collect();
    let default = app
        .default_choice
        .and_then(|choice| app.desktops.iter().position(|d| *d == choice));
    sections(vec![
        column![
            group(
                "Desktop Environments",
                "Choose one or more. Every selected desktop appears on the login screen. Your choices apply to the installed system.",
                vec![],
            ),
            column_of(lines).spacing(10.0).width(Fill),
        ]
        .spacing(8.0)
        .width(Fill)
        .into(),
        group(
            "Login",
            "",
            vec![
                form_row(
                    "Default session",
                    dropdown(labels, default)
                        .on_select(Msg::DefaultDesktop)
                        .enabled(app.desktops.len() > 1),
                )
                .description("Preselected on the login screen")
                .into(),
            ],
        ),
        group(
            "Network and Software",
            "",
            vec![
                form_row("Bring saved Wi-Fi networks", toggle(app.wifi).on_toggle(Msg::Wifi))
                    .description("Copies this live session's saved Wi-Fi connections and passwords, stored root-only outside the Nix store. Enterprise networks using certificate files or hardware tokens must be set up after installation.")
                    .into(),
                form_row("Allow unfree software", toggle(app.unfree).on_toggle(Msg::Unfree))
                    .description("Some hardware drivers and applications require it. Redistributable firmware is included either way, and allowing unfree packages does not configure every vendor driver automatically.")
                    .into(),
            ],
        ),
    ])
}
