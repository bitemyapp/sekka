// SPDX-License-Identifier: GPL-3.0-or-later
//! The installed system's keyboard layout, and the key just left of the
//! space bar: the page asks for it as it opens (Settei's keyboard setup,
//! `aizome_ipc::setup`). Alt there, as on most PC keyboards, and Genkan
//! recommends swapping left Alt and Super to put ❖ there; ❖ there already needs
//! nothing; any other key stays as it is. Either way the person decides.
use super::{group, sections};
use crate::app::{Genkan, KEYBOARD_NAMES, KeyCheck, Msg, beside_alt, beside_super};
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use calamares_nixos::{KEYBOARDS, SuperKey};

pub fn page(app: &Genkan) -> Element<Msg> {
    let keyboards: Vec<String> = KEYBOARD_NAMES
        .iter()
        .zip(KEYBOARDS)
        .map(|(name, code)| format!("{name} — {code}"))
        .collect();
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
                        "Swap left Alt and Super",
                        toggle(app.super_key == SuperKey::SwapAlt)
                            .on_toggle(Msg::SwapAltSuper)
                            .id("swap-alt-super"),
                    )
                    .description("Puts ❖ just left of the space bar on a PC keyboard, where ⌘ is on a Mac. For every keyboard on the installed system; Aizome offers to set up others you plug in, each with keys of its own.")
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
    let swapped = app.super_key == SuperKey::SwapAlt;
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
            // Alt: the swap, recommended. ❖: nothing to do. Anything else:
            // Genkan leaves it, and says so.
            let (why, answer, role) = if beside_alt(beside) {
                if swapped {
                    (
                        beside.why,
                        "✓ Recommended: Genkan swaps left Alt and Super on the installed system.",
                        Role::Success,
                    )
                } else {
                    (
                        beside.why,
                        "Kept as they are: ❖ stays where it is now, not beside the space bar.",
                        Role::Warning,
                    )
                }
            } else if beside_super(beside) {
                if swapped {
                    (
                        beside.why,
                        "Left Alt and Super are swapped anyway: ❖ moves away from the space bar.",
                        Role::Warning,
                    )
                } else {
                    (beside.why, "Nothing needs changing.", Role::Success)
                }
            } else if swapped {
                (
                    "Genkan swaps only left Alt and Super, which isn't this key.",
                    "Left Alt and Super are swapped anyway, wherever they are on this keyboard.",
                    Role::Warning,
                )
            } else {
                (
                    "Genkan swaps only left Alt and Super, so your keys stay as they are. Settings → Keyboard can change them after installing.",
                    "Kept as they are.",
                    Role::Text2,
                )
            };
            column![
                heading,
                text(format!("Beside the space bar: {}", beside.what)).id("beside"),
                text(why).size(11.5).role(Role::Text2).wrap(),
                text(answer).size(11.5).role(role).wrap().id("key-answer"),
            ]
            .spacing(6.0)
            .width(Fill)
            .into()
        }
        KeyCheck::Unavailable(why) => row![
            icon(Icon::TriangleAlert).size(18.0).role(Role::Warning),
            column![
                text("Genkan can't hear the key here.").weight(650.0),
                text(format!(
                    "If Alt is just left of your space bar, swap left Alt and Super below. ({why})"
                ))
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
