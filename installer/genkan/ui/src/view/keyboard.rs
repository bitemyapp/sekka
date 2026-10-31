// SPDX-License-Identifier: GPL-3.0-or-later
//! The installed system's keyboard layout, and the key just left of the
//! space bar: the page asks for it as it opens, and suggests the swap that
//! puts ❖ there (Settei's keyboard setup, `aizome_ipc::setup`), which the
//! person can decline.
use super::{group, sections};
use crate::app::{Genkan, KEYBOARD_NAMES, KeyCheck, Msg, SUPER_KEY_NAMES};
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use calamares_nixos::{KEYBOARDS, SuperKey};

pub fn page(app: &Genkan) -> Element<Msg> {
    let keyboards: Vec<String> = KEYBOARD_NAMES
        .iter()
        .zip(KEYBOARDS)
        .map(|(name, code)| format!("{name} — {code}"))
        .collect();
    let arrangements: Vec<String> = SUPER_KEY_NAMES.iter().map(|name| (*name).into()).collect();
    let chosen = SuperKey::ALL
        .iter()
        .position(|key| *key == app.super_key)
        .unwrap_or(0);
    sections(vec![
        group(
            "Layout",
            "The live keyboard layout is unchanged; this applies to the installed system.",
            vec![
                form_row(
                    "Keyboard layout",
                    dropdown(keyboards, app.keyboard).on_select(Msg::Keyboard),
                )
                .into(),
            ],
        ),
        column![
            group(
                "❖ Super Key",
                "❖ is the command key in Aizome, as ⌘ is on a Mac: ❖C copies, ❖V pastes and ❖Space finds anything. It's easiest to reach just left of the space bar, under your thumb.",
                vec![],
            ),
            card(app),
            form_section(
                "",
                vec![
                    form_row(
                        "Modifier keys",
                        dropdown(arrangements, chosen)
                            .on_select(Msg::SuperKey)
                            .id("super-key"),
                    )
                    .description("For every keyboard on the installed system. Aizome offers to set up others you plug in, each with keys of its own.")
                    .into(),
                ],
            ),
        ]
        .spacing(10.0)
        .width(Fill)
        .into(),
    ])
}

/// The question, the key that answered it, or why it can't be asked.
fn card(app: &Genkan) -> Element<Msg> {
    let keyboard = || icon(Icon::Keyboard).size(18.0).role(Role::Accent);
    let content: Element<Msg> = match &app.key_check {
        KeyCheck::Asking | KeyCheck::Waiting => {
            let waiting = app.key_check == KeyCheck::Waiting;
            let (detail, role) = match (&app.key_error, waiting) {
                (Some(again), _) => (again.clone(), Role::Warning),
                (None, true) => (
                    "Waiting for the key… Esc stops waiting.".into(),
                    Role::Text3,
                ),
                (None, false) => (
                    "Genkan listens for it and suggests where ❖ should go.".into(),
                    Role::Text3,
                ),
            };
            let action = if waiting {
                button("Stop").compact().on_press(Msg::StopListening)
            } else {
                button("Listen for the Key")
                    .primary()
                    .on_press(Msg::ListenForKey)
            };
            row![
                keyboard(),
                column![
                    text("Press the key just left of your space bar.")
                        .size(15.0)
                        .weight(650.0),
                    text(detail).role(role).wrap().id("key-status"),
                ]
                .spacing(3.0)
                .width(Fill),
                action.id("listen"),
            ]
            .spacing(12.0)
            .align(Align::Center)
            .width(Fill)
            .into()
        }
        KeyCheck::Caught {
            keyboard: from,
            beside,
        } => {
            let name = from
                .as_ref()
                .map_or_else(|| "Your keyboard".to_string(), |k| k.name.clone());
            let mut heading = row![keyboard(), text(name).weight(650.0)]
                .spacing(8.0)
                .align(Align::Center)
                .width(Fill);
            if let Some(k) = from {
                heading = heading.push(text(k.id.clone()).mono().size(11.0).role(Role::Text3));
            }
            heading = heading.push(spacer()).push(
                button("Press Again")
                    .compact()
                    .on_press(Msg::ListenForKey)
                    .id("listen"),
            );
            let suggested = beside.suggest.and_then(SuperKey::from_xkb_options);
            let (answer, role) = match suggested {
                Some(SuperKey::AsIs) => (None, Role::Text2),
                Some(key) if key == app.super_key => (
                    Some("✓ Genkan swaps them on the installed system."),
                    Role::Success,
                ),
                Some(_) => (
                    Some("Kept as they are: ❖ stays where it is now."),
                    Role::Warning,
                ),
                None => (None, Role::Text2),
            };
            let mut lines = column![
                heading,
                text(format!("Beside the space bar: {}", beside.what)).id("beside"),
                text(beside.why).size(11.5).role(Role::Text2).wrap(),
            ]
            .spacing(6.0)
            .width(Fill);
            if let Some(answer) = answer {
                lines = lines.push(text(answer).size(11.5).role(role).wrap());
            }
            lines.into()
        }
        KeyCheck::Unavailable(why) => row![
            icon(Icon::TriangleAlert).size(18.0).role(Role::Warning),
            column![
                text("Genkan can't hear the key here.").weight(650.0),
                text(format!("Choose the modifier keys below instead. ({why})"))
                    .size(11.5)
                    .role(Role::Text3)
                    .wrap(),
            ]
            .spacing(3.0)
            .width(Fill),
            button("Try Again")
                .compact()
                .on_press(Msg::ListenForKey)
                .id("listen"),
        ]
        .spacing(12.0)
        .align(Align::Center)
        .width(Fill)
        .into(),
    };
    container(content)
        .padding([12.0, 14.0])
        .fill(Tone::Chrome)
        .border(Edge::Line)
        .width(Fill)
        .id("key-card")
        .into()
}
