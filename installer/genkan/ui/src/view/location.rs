// SPDX-License-Identifier: GPL-3.0-or-later
//! The time zone, on the map or from the list, and the installed system's
//! language and keyboard layout.
use super::{group, map, sections};
use crate::app::{Genkan, KEYBOARD_NAMES, LOCALE_NAMES, Msg};
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use calamares_nixos::{KEYBOARDS, LOCALES, zonemap::Place};

/// The map's height: the world at the page's width, give or take.
const MAP_HEIGHT: f64 = 380.0;

pub fn page(app: &Genkan) -> Element<Msg> {
    let (name, place, time, detail) = match &app.zone_clock {
        Some(clock) => (
            clock.name.clone(),
            clock.place.clone(),
            clock.time.clone(),
            clock.detail.clone(),
        ),
        None => (
            "Finding your time zone".to_string(),
            "You can click your location on the map at any time".to_string(),
            String::new(),
            String::new(),
        ),
    };
    let info = row![
        column![
            text(name).size(15.0).weight(650.0).id("zone-name"),
            text(place).role(Role::Text2),
        ]
        .spacing(2.0)
        .width(Fill),
        column![
            text(time).size(18.0).weight(650.0).tabular(),
            text(detail).size(11.5).mono().role(Role::Text3),
        ]
        .spacing(2.0)
        .align(Align::End),
    ]
    .spacing(12.0)
    .padding([12.0, 14.0])
    .width(Fill);
    let mut status = row![].spacing(8.0).padding([8.0, 14.0]).width(Fill);
    if app.zone_pending.is_some() {
        status = status.push(progress_indeterminate().width(48.0));
    }
    status = status
        .push(
            text(app.zone_status.clone())
                .size(12.0)
                .role(Role::Text3)
                .wrap()
                .width(Fill)
                .id("zone-status"),
        )
        .push(
            button("Detect")
                .icon(Icon::LocateFixed)
                .compact()
                .on_press(Msg::Detect)
                .enabled(app.zone_pending.is_none())
                .tooltip("Detect the time zone again"),
        );
    let card = container(
        column![
            map::view(app, MAP_HEIGHT),
            divider(),
            info,
            divider(),
            status
        ]
        .width(Fill),
    )
    .fill(Tone::Chrome)
    .border(Edge::Line)
    .clip()
    .width(Fill);

    let labels: Vec<String> = app
        .zones
        .iter()
        .flat_map(|z| z.places.iter().map(Place::label))
        .collect();
    let locales: Vec<String> = LOCALE_NAMES
        .iter()
        .zip(LOCALES)
        .map(|(name, code)| format!("{name} — {code}"))
        .collect();
    let keyboards: Vec<String> = KEYBOARD_NAMES
        .iter()
        .zip(KEYBOARDS)
        .map(|(name, code)| format!("{name} — {code}"))
        .collect();
    sections(vec![
        column![
            group(
                "Time Zone",
                "Click where you are on the map, or search for a nearby city.",
                vec![],
            ),
            card,
            form_section(
                "",
                vec![
                    form_row(
                        "Time zone",
                        dropdown(labels, app.zone)
                            .placeholder("Choose a city")
                            .on_select(Msg::ZoneFromList),
                    )
                    .into(),
                    form_row("Find my location online", toggle(app.internet).on_toggle(Msg::Internet))
                        .description("Asks geoip.kde.org, or ipinfo.io if it is unavailable. They see your public IP address, and VPNs or mobile networks can place you elsewhere.")
                        .into(),
                ],
            ),
        ]
        .spacing(10.0)
        .width(Fill)
        .into(),
        group(
            "Language and Keyboard",
            "The live keyboard layout is unchanged; these apply to the installed system.",
            vec![
                form_row("System language", dropdown(locales, app.locale).on_select(Msg::Locale)).into(),
                form_row("Keyboard layout", dropdown(keyboards, app.keyboard).on_select(Msg::Keyboard)).into(),
            ],
        ),
    ])
}
