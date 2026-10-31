// SPDX-License-Identifier: GPL-3.0-or-later
//! The application catalog, searchable. Selection rules mirror the helper's
//! parser: Rustup requires the build tools, and proprietary choices need the
//! unfree-software permission from the Desktop page.
use super::sections;
use crate::app::{Genkan, Msg};
use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::{column as column_of, row as row_of};
use calamares_nixos::applications;

pub fn page(app: &Genkan) -> Element<Msg> {
    let query = app.app_query.trim().to_lowercase();
    let required = app.required_apps();
    let selected = app.apps.iter().filter(|on| **on).count();
    let count = match selected {
        0 => "No applications selected · Rustup also selects Development build tools".into(),
        1 => "1 application selected · Rustup also selects Development build tools".into(),
        n => format!("{n} applications selected · Rustup also selects Development build tools"),
    };
    let intro = column![
        text("Choose applications to have ready after installation. The installer reuses software on the media and downloads anything missing. Applications marked “Internet required” are not included on the media.")
            .role(Role::Text2)
            .wrap(),
        search_field(&app.app_query)
            .placeholder("Search applications")
            .on_change(Msg::AppSearch)
            .id("app-search"),
        text(count).size(12.0).role(Role::Text3).id("app-count"),
    ]
    .spacing(8.0)
    .width(Fill);

    let mut groups: Vec<(String, Vec<Element<Msg>>)> = Vec::new();
    for (index, item) in applications::catalog().iter().enumerate() {
        let haystack =
            format!("{} {} {}", item.name, item.description, item.category).to_lowercase();
        if !haystack.contains(&query) {
            continue;
        }
        if groups
            .last()
            .is_none_or(|(category, _)| *category != item.category)
        {
            groups.push((item.category.clone(), Vec::new()));
        }
        let needed = required.contains(&item.id);
        let blocked = item.unfree && !app.unfree;
        let mut control: Vec<Element<Msg>> = Vec::new();
        if !item.preload {
            control.push(badge("Internet required").role(Role::Cyan).into());
        }
        if item.terminal.is_some() {
            control.push(badge("Terminal").role(Role::Cyan).into());
        }
        if item.unfree {
            control.push(badge("Proprietary").role(Role::Warning).into());
        }
        if !item.requires.is_empty() {
            control.push(badge("Adds build tools").role(Role::Violet).into());
        }
        let mut check = checkbox("", app.apps[index])
            .on_toggle(move |on| Msg::ToggleApp(index, on))
            .enabled(!needed && !blocked)
            .label(item.name.clone())
            .id(format!("app-{}", item.id));
        if needed {
            check = check.tooltip("Required by Rustup. Deselect Rustup to make this optional.");
        } else if blocked {
            check = check
                .tooltip("Allow unfree software on the Desktop page to select this application.");
        }
        control.push(check.into());
        if let Some((_, rows)) = groups.last_mut() {
            rows.push(
                form_row(item.name.clone(), row_of(control).spacing(8.0))
                    .description(item.description.clone())
                    .key(index)
                    .into(),
            );
        }
    }
    let mut items = vec![intro.into()];
    if groups.is_empty() {
        items.push(
            column![
                icon(Icon::Search).size(28.0).role(Role::Text3),
                text("No Matching Applications").size(15.0).weight(650.0),
                text("Try a different name or category.").role(Role::Text3),
            ]
            .spacing(8.0)
            .align(Align::Center)
            .padding(24.0)
            .width(Fill)
            .into(),
        );
    }
    for (category, rows) in groups {
        items.push(form_section(category, rows));
    }
    sections(vec![column_of(items).spacing(18.0).width(Fill).into()])
}
