// SPDX-License-Identifier: GPL-3.0-or-later
//! The window: the steps down a sidebar, the step's page under its
//! heading, and a bar at the bottom with Back, what's going on, and Next.
//!
//! Views are drawn from the program's state whenever it changes, so each is
//! a plain function of it. The frame is the kit's (`sidebar_layout`,
//! `sidebar_item`, `form_section`, `scroll`); what Genkan draws of its own
//! (the partitions, the world map) is in [`partitions`] and [`map`].
use crate::app::{Genkan, Msg, Step};
use crate::format::size;
use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{column, row};
use calamares_nixos::Firmware;
use std::sync::Arc;

mod account;
mod applications;
mod desktop;
mod disk;
mod github;
mod install;
mod location;
pub mod map;
pub mod partitions;
mod review;
mod welcome;

/// The sidebar's width.
pub const SIDEBAR: f64 = 220.0;
/// The widest a page's content gets.
pub const PAGE_WIDTH: f64 = 820.0;

pub fn view(app: &Genkan) -> Element<Msg> {
    let page: Element<Msg> = match app.step {
        Step::Welcome => welcome::page(),
        Step::Disk => disk::page(app),
        Step::Account => account::page(app),
        Step::GitHub => github::page(app),
        Step::Desktop => desktop::page(app),
        Step::Applications => applications::page(app),
        Step::Location => location::page(app),
        Step::Review => review::page(app),
        Step::Install if app.completed => install::done(app),
        Step::Install => install::page(app),
    };
    let content = container(page.max_width(PAGE_WIDTH).width(Fill))
        .align_x(Align::Center)
        .width(Fill)
        .padding([20.0, 24.0, 28.0, 24.0]);
    let mut main = column![
        header(app),
        scroll(content).key(app.step).width(Fill).height(Fill)
    ];
    if !app.completed {
        main = main.push(footer(app));
    }
    sidebar_layout(sidebar(app), main.width(Fill).height(Fill))
        .first_size(SIDEBAR)
        .min_sizes(190.0, 480.0)
        .into()
}

fn sidebar(app: &Genkan) -> Element<Msg> {
    let brand = row![
        container(icon(Icon::HardDriveDownload).size(18.0).role(Role::Accent))
            .padding(5.0)
            .fill(Tone::Chrome)
            .border(Edge::Line),
        column![
            text("Genkan").size(15.0).weight(700.0),
            text("玄関 · Install Sekka").size(11.0).role(Role::Text3),
        ]
        .spacing(1.0),
    ]
    .spacing(10.0)
    .padding([14.0, 12.0, 12.0, 12.0]);
    let steps = column_of(Step::ALL.iter().map(|step| step_item(app, *step)).collect())
        .spacing(1.0)
        .padding([0.0, 6.0])
        .width(Fill);
    column![
        brand,
        divider().edge(Edge::Line),
        caption("Steps"),
        steps,
        spacer(),
        computer(app)
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

/// A step in the sidebar: done ones ticked, the others numbered. Only the
/// steps already reached before the review can be gone back to.
fn step_item(app: &Genkan, step: Step) -> Element<Msg> {
    let current = step == app.step && !app.completed;
    let done = step < app.step || app.completed;
    let detail = if done && !current {
        "✓".to_string()
    } else {
        (step.index() + 1).to_string()
    };
    let item = sidebar_item(step.icon(), step.title(), current).detail(detail);
    if app.reachable(step) && !current {
        item.on_press(Msg::Jump(step)).into()
    } else {
        item.into()
    }
}

/// The sidebar's foot: this computer, as the installer sees it.
fn computer(app: &Genkan) -> Element<Msg> {
    let firmware = match app.firmware {
        Firmware::Uefi => "UEFI",
        Firmware::Bios => "Legacy BIOS",
    };
    let mut lines = column![
        row![
            icon(Icon::Laptop).size(14.0).role(Role::Text3),
            text("This computer").weight(650.0)
        ]
        .spacing(7.0)
    ]
    .spacing(4.0);
    if app.disks.is_empty() && app.scanning {
        lines = lines.push(text("Looking for disks…").size(11.0).role(Role::Text3));
    } else {
        lines = lines.push(text(firmware).mono().size(11.0).role(Role::Text3));
    }
    if let Some(memory) = app.memory {
        lines = lines.push(
            text(format!("{} of memory", size(memory.total)))
                .mono()
                .size(11.0)
                .role(Role::Text3),
        );
    }
    container(lines.width(Fill))
        .padding(12.0)
        .edges(Edge::Line, Sides::TOP)
        .width(Fill)
        .into()
}

fn header(app: &Genkan) -> Element<Msg> {
    let (title, subtitle) = if app.completed {
        ("Installation Complete".to_string(), String::new())
    } else if app.step == Step::Welcome {
        (app.step.heading().to_string(), String::new())
    } else {
        (
            app.step.heading().to_string(),
            format!("Step {} of {}", app.step.index(), Step::ALL.len() - 1),
        )
    };
    let mut titles = column![text(title).size(17.0).weight(700.0)].spacing(2.0);
    if !subtitle.is_empty() {
        titles = titles.push(text(subtitle).size(11.5).mono().role(Role::Text3));
    }
    container(titles)
        .padding([12.0, 24.0, 11.0, 24.0])
        .fill(Tone::Chrome)
        .edges(Edge::Line, Sides::BOTTOM)
        .width(Fill)
        .into()
}

/// Back, what's going on (caching, checking, a note), and Next.
fn footer(app: &Genkan) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = Vec::new();
    if app.step != Step::Welcome && app.step != Step::Install {
        items.push(
            button("Back")
                .icon(Icon::ChevronLeft)
                .on_press(Msg::Back)
                .enabled(!app.reviewing)
                .into(),
        );
    }
    items.push(spacer().into());
    let installing = app.step == Step::Install && app.installing && !app.failed;
    // A note (a check that failed, a refusal to close) comes first; then
    // the installation's reminder, or the work going on before it.
    let activity = app
        .activity
        .as_ref()
        .filter(|a| a.warning || app.step != Step::Install);
    if installing && activity.is_none() {
        items.push(
            text("Keep this computer on and connected to power until the installation finishes.")
                .role(Role::Text2)
                .into(),
        );
    } else if let Some(activity) = activity {
        if activity.busy {
            items.push(progress_indeterminate().width(64.0).into());
        }
        if activity.warning {
            items.push(
                icon(Icon::TriangleAlert)
                    .size(14.0)
                    .role(Role::Warning)
                    .into(),
            );
        }
        items.push(
            text(activity.text.clone())
                .role(if activity.warning {
                    Role::Warning
                } else {
                    Role::Text2
                })
                .id("activity")
                .into(),
        );
    }
    items.push(spacer().into());
    let next_shown = app.step != Step::Install || (app.finished() && !app.completed);
    if next_shown {
        let next = button(app.next_label())
            .on_press(Msg::Next)
            .enabled(app.next_enabled())
            .id("next");
        items.push(if app.step == Step::Review {
            next.destructive().into()
        } else {
            next.primary().into()
        });
    }
    container(row_of(items).spacing(10.0).width(Fill))
        .padding([10.0, 16.0])
        .fill(Tone::Chrome)
        .edges(Edge::Line, Sides::TOP)
        .width(Fill)
        .into()
}

fn row_of(items: Vec<Element<Msg>>) -> aizome_kit::widgets::layout::Flex<Msg> {
    aizome_kit::widgets::layout::row(items)
}

// Pieces the pages share.

/// A group of rows under a title, with a line or two about them.
pub fn group(title: &str, description: &str, rows: Vec<Element<Msg>>) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = Vec::new();
    if !title.is_empty() {
        items.push(
            container(
                text(title.to_string())
                    .mono()
                    .size(10.5)
                    .letter_spacing(1.2)
                    .caps()
                    .role(Role::Text3),
            )
            .padding([0.0, 2.0, 0.0, 2.0])
            .into(),
        );
    }
    if !description.is_empty() {
        items.push(
            container(text(description.to_string()).role(Role::Text2).wrap())
                .padding([0.0, 2.0, 2.0, 2.0])
                .width(Fill)
                .into(),
        );
    }
    if !rows.is_empty() {
        items.push(form_section("", rows));
    }
    column_of(items).spacing(6.0).width(Fill).into()
}

/// A row of a group that is text alone: a hint, a status.
pub fn note_row(text_: impl Into<Arc<str>>) -> Element<Msg> {
    let t: Arc<str> = text_.into();
    container(text(t).size(12.0).role(Role::Text3).wrap())
        .padding([9.0, 12.0])
        .width(Fill)
        .into()
}

/// A value for a label, as the Review page lists them.
pub fn property(label: &str, value: impl Into<Arc<str>>) -> Element<Msg> {
    let value: Arc<str> = value.into();
    form_row(label.to_string(), space(0.0))
        .description(value)
        .into()
}

/// A page's sections, one under another.
pub fn sections(items: Vec<Element<Msg>>) -> Element<Msg> {
    column_of(items).spacing(22.0).width(Fill).into()
}

/// The width of text fields in forms.
pub const FIELD: f64 = 300.0;
