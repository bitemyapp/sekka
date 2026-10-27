// SPDX-License-Identifier: GPL-3.0-or-later
//! The installation as it goes, what went wrong if it failed, and what to
//! do once it's done.
use super::{group, review::failure, sections};
use crate::app::{Genkan, INSTALL_STEPS, Msg, StepState};
use crate::format::{clock, duration};
use aizome_kit::prelude::*;
use aizome_kit::{column, row};

const LOG_LINE: f64 = 17.0;

pub fn page(app: &Genkan) -> Element<Msg> {
    let heading = if app.failed {
        "Installation Failed"
    } else {
        "Installing NixOS"
    };
    let top = row![
        column![
            text(heading).size(20.0).weight(700.0).id("install-heading"),
            text(app.install_message.clone())
                .role(Role::Text2)
                .wrap()
                .id("install-message"),
        ]
        .spacing(6.0)
        .width(Fill),
        text(clock(app.elapsed))
            .size(20.0)
            .mono()
            .tabular()
            .role(Role::Text2)
            .id("elapsed"),
    ]
    .spacing(18.0)
    .width(Fill);
    let steps = INSTALL_STEPS
        .iter()
        .zip(app.steps)
        .enumerate()
        .map(|(index, (title, state))| {
            let mark: Element<Msg> = match state {
                StepState::Pending => icon(Icon::Circle).size(16.0).role(Role::Faint).into(),
                StepState::Current => progress_indeterminate().width(40.0).into(),
                StepState::Done => icon(Icon::CircleCheck)
                    .size(16.0)
                    .role(Role::Success)
                    .into(),
                StepState::Failed => icon(Icon::CircleX).size(16.0).role(Role::Danger).into(),
            };
            let mut row = form_row(title.to_string(), mark).key(index);
            if index == 3
                && let Some(copied) = &app.copied
            {
                row = row.description(copied.clone());
            }
            row.into()
        })
        .collect();
    let mut items = vec![
        column![
            top,
            progress(app.fraction()).width(Fill).id("install-progress")
        ]
        .spacing(14.0)
        .width(Fill)
        .into(),
        form_section("", steps),
    ];
    if let Some(message) = &app.failure {
        items.push(group(
            "Installation did not complete",
            "The disk may have been modified. Copy these details before closing; storage diagnostics, if any, are saved under /run.",
            vec![container(failure(message)).padding(12.0).width(Fill).into()],
        ));
    }
    let mut details = vec![
        row![
            column![
                text("Details").weight(650.0),
                text("Activity reported by Nix and the installer")
                    .size(12.0)
                    .role(Role::Text3),
            ]
            .spacing(2.0)
            .width(Fill),
            button(if app.log_open { "Hide" } else { "Show" })
                .compact()
                .on_press(Msg::ToggleLog),
        ]
        .spacing(10.0)
        .padding([10.0, 12.0])
        .width(Fill)
        .into(),
    ];
    if app.log_open {
        let log = app.log.clone();
        let last = log.len().saturating_sub(1);
        details.push(
            container(
                virtual_list(log.len(), LOG_LINE, move |i| {
                    container(
                        text(log.get(i).cloned().unwrap_or_default())
                            .mono()
                            .size(11.0)
                            .role(Role::Text2),
                    )
                    .padding([0.0, 12.0])
                })
                .reveal(last)
                .width(Fill)
                .height(Fill),
            )
            .fill(Tone::Sunk)
            .width(Fill)
            .height(220.0)
            .into(),
        );
    }
    items.push(form_section("", details));
    sections(items)
}

pub fn done(app: &Genkan) -> Element<Msg> {
    let mut spans = Vec::new();
    if let Some(seconds) = app.installed_in() {
        spans.push(Span::new("Installed in "));
        spans.push(Span::new(duration(seconds)).bold());
        spans.push(Span::new(". "));
    }
    spans.push(Span::new("Shut down before removing the installation media, then turn on your computer. If you restart instead, remove the media when your computer's startup screen appears."));
    // The bar at the bottom is gone by now: a failed restart is said here.
    let note: Element<Msg> = match &app.activity {
        Some(activity) if activity.warning => row![
            icon(Icon::TriangleAlert).size(14.0).role(Role::Warning),
            text(activity.text.clone()).role(Role::Warning).wrap(),
        ]
        .spacing(8.0)
        .into(),
        _ => space(0.0),
    };
    column![
        space(40.0),
        icon(Icon::CircleCheck).size(56.0).role(Role::Success),
        text("NixOS Is Installed")
            .size(24.0)
            .weight(700.0)
            .id("done-heading"),
        container(rich_text(spans).wrap().width(Fill)).max_width(520.0),
        row![
            button("Close").on_press(Msg::Close).enabled(!app.powering),
            button("Restart Now")
                .on_press(Msg::Power("restart"))
                .enabled(!app.powering),
            button("Shut Down")
                .primary()
                .on_press(Msg::Power("shut down"))
                .enabled(!app.powering)
                .id("shut-down"),
        ]
        .spacing(12.0),
        note,
    ]
    .spacing(16.0)
    .align(Align::Center)
    .width(Fill)
    .into()
}
