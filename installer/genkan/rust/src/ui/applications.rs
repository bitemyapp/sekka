// SPDX-License-Identifier: GPL-3.0-or-later
//! Searchable application catalog. Selection rules mirror the helper's
//! parser: Rustup requires the build tools, and proprietary choices need the
//! unfree-software permission from the Desktop page.
use super::widgets::{badge, caption};
use adw::prelude::*;
use calamares_nixos::applications;
use gtk::{CheckButton, glib};
use std::{cell::Cell, collections::BTreeSet, rc::Rc};

pub struct ApplicationsPage {
    pub widget: adw::PreferencesPage,
    pub checks: Vec<CheckButton>,
}

impl ApplicationsPage {
    pub fn selected_ids(&self) -> Vec<String> {
        applications::catalog()
            .iter()
            .zip(&self.checks)
            .filter(|(_, check)| check.is_active())
            .map(|(app, _)| app.id.clone())
            .collect()
    }
}

/// `changed` runs after every user-visible selection change.
pub fn build(unfree: &adw::SwitchRow, changed: Rc<dyn Fn()>) -> ApplicationsPage {
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search applications")
        .hexpand(true)
        .build();
    let count = caption("");
    count.set_margin_top(8);
    let page = adw::PreferencesPage::new();
    let intro = adw::PreferencesGroup::builder()
        .description("Choose applications to have ready after installation. Any combination works, and you can sign in to your accounts later. Everything listed is already on this installation media.")
        .build();
    intro.add(&search);
    intro.add(&count);
    page.add(&intro);
    let checks: Vec<_> = applications::catalog()
        .iter()
        .map(|app| {
            let check = CheckButton::new();
            check.set_active(app.id == "firefox");
            check.set_valign(gtk::Align::Center);
            check
        })
        .collect();
    let mut groups: Vec<(adw::PreferencesGroup, Vec<(adw::ActionRow, String)>)> = Vec::new();
    let mut category = "";
    for (app, check) in applications::catalog().iter().zip(&checks) {
        if groups.is_empty() || category != app.category {
            category = &app.category;
            // Group titles are markup: "Media & creativity" must be escaped.
            let group = adw::PreferencesGroup::builder()
                .title(glib::markup_escape_text(category).as_str())
                .build();
            page.add(&group);
            groups.push((group, Vec::new()));
        }
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&app.name).as_str())
            .subtitle(glib::markup_escape_text(&app.description).as_str())
            .activatable_widget(check)
            .build();
        row.add_prefix(check);
        if app.terminal.is_some() {
            row.add_suffix(&badge("Terminal", "terminal"));
        }
        if app.unfree {
            row.add_suffix(&badge("Proprietary", "proprietary"));
        }
        if !app.requires.is_empty() {
            row.add_suffix(&badge("Adds build tools", "required"));
        }
        let (group, rows) = groups.last_mut().unwrap();
        group.add(&row);
        rows.push((
            row,
            format!("{} {} {}", app.name, app.description, app.category).to_lowercase(),
        ));
    }
    let empty = adw::PreferencesGroup::new();
    empty.add(
        &adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title("No Matching Applications")
            .description("Try a different name or category.")
            .build(),
    );
    empty.set_visible(false);
    page.add(&empty);
    search.connect_search_changed(move |search| {
        let query = search.text().trim().to_lowercase();
        let mut any = false;
        for (group, rows) in &groups {
            let mut visible = false;
            for (row, text) in rows {
                let matches = text.contains(&query);
                row.set_visible(matches);
                visible |= matches;
            }
            group.set_visible(visible);
            any |= visible;
        }
        empty.set_visible(!any);
    });
    let updating = Rc::new(Cell::new(false));
    let refresh: Rc<dyn Fn()> = Rc::new({
        let checks: Vec<_> = checks.iter().map(CheckButton::downgrade).collect();
        let unfree = unfree.downgrade();
        let count = count.downgrade();
        move || {
            if updating.replace(true) {
                return;
            }
            if let (Some(unfree), Some(count)) = (unfree.upgrade(), count.upgrade()) {
                let checks: Vec<_> = checks.iter().filter_map(glib::WeakRef::upgrade).collect();
                if checks.len() == applications::catalog().len() {
                    for (app, check) in applications::catalog().iter().zip(&checks) {
                        if app.unfree && !unfree.is_active() {
                            check.set_active(false);
                        }
                    }
                    let required: BTreeSet<_> = applications::catalog()
                        .iter()
                        .zip(&checks)
                        .filter(|(_, check)| check.is_active())
                        .flat_map(|(app, _)| app.requires.iter())
                        .collect();
                    for (app, check) in applications::catalog().iter().zip(&checks) {
                        let needed = required.contains(&app.id);
                        if needed {
                            check.set_active(true);
                        }
                        check.set_sensitive((!app.unfree || unfree.is_active()) && !needed);
                        check.set_tooltip_text(if needed {
                            Some("Required by Rustup. Deselect Rustup to make this optional.")
                        } else if app.unfree && !unfree.is_active() {
                            Some("Allow unfree software on the Desktop page to select this application.")
                        } else {
                            None
                        });
                    }
                    let selected = checks.iter().filter(|check| check.is_active()).count();
                    count.set_text(&match selected {
                        0 => "No applications selected · Rustup also selects Development build tools".into(),
                        1 => "1 application selected · Rustup also selects Development build tools".into(),
                        n => format!("{n} applications selected · Rustup also selects Development build tools"),
                    });
                }
            }
            updating.set(false);
        }
    });
    for check in &checks {
        check.connect_toggled({
            let refresh = refresh.clone();
            let changed = changed.clone();
            move |_| {
                refresh();
                changed();
            }
        });
    }
    unfree.connect_active_notify({
        let refresh = refresh.clone();
        move |_| refresh()
    });
    refresh();
    ApplicationsPage {
        widget: page,
        checks,
    }
}
