// SPDX-License-Identifier: MIT OR Apache-2.0
//! What is installed, grouped by why: added with Yukimi, listed in the
//! configuration by hand, chosen from a catalog the system brings,
//! installed by a user for themselves, or part of the system because a
//! desktop or setting brings it.
use std::collections::BTreeSet;
use std::rc::Rc;

use adw::prelude::*;
use yukimi_config::edit::ListedPackage;
use yukimi_system::human_size;

use super::Ctx;
use super::widgets::{badge, clear, dim, fill_later, monogram, page, pending_row};
use crate::model::{Model, SystemPackage};
use crate::ops::{Operation, SystemChange};

fn remove_button(tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name("user-trash-symbolic");
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(tooltip));
    button
}

fn empty_row(text: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(text);
    row.add_css_class("dim-row");
    row
}

/// A row for a package: its name, what it is (from the index, once there
/// is one) and its version.
fn package_row(ctx: &Ctx, attr: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(&gtk::glib::markup_escape_text(attr));
    if let Some(p) = ctx.index().as_ref().and_then(|i| i.get(attr)) {
        row.set_subtitle(&gtk::glib::markup_escape_text(&p.description));
        row.add_suffix(&dim(&p.version));
    }
    row.add_prefix(&monogram(attr, 36));
    row
}

/// Ask, then remove something for everyone.
fn remove(ctx: &Ctx, what: &str, change: SystemChange) {
    ctx.confirm(
        &format!("Remove {what}?"),
        "It is removed for everyone on this computer. Yukimi changes the system configuration and builds the new \
         system, which asks for an administrator password. The current version stays in History.",
        "Remove",
        Operation::ChangeSystem { change, what: what.to_owned(), adding: false },
    );
}

/// What Yukimi added for everyone: packages and programs in `yukimi.nix`.
fn added(ctx: &Ctx, model: &Model) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Added with Yukimi");
    let file = model.setup.packages_file().map(|f| f.display().to_string()).unwrap_or_else(|| "yukimi.nix".into());
    group.set_description(Some(&format!("For everyone on this computer, kept in {file}.")));
    let browse = gtk::Button::with_label("Find more");
    browse.add_css_class("flat");
    {
        let ctx = ctx.clone();
        browse.connect_clicked(move |_| ctx.show("discover"));
    }
    group.set_header_suffix(Some(&browse));
    let mine = &model.configured.yukimi;
    if mine.is_empty() {
        group.add(&empty_row("None yet: find applications and packages in Discover"));
    }
    for program in &mine.programs {
        let app = model.catalog.iter().find(|a| a.program.as_ref() == Some(program));
        let name = app.map_or(program.as_str(), |a| a.name.as_str());
        let row = adw::ActionRow::new();
        row.set_title(name);
        row.set_subtitle(&app.map_or_else(|| format!("programs.{program}"), |a| a.description.clone()));
        row.add_prefix(&monogram(name, 36));
        let button = remove_button("Remove for everyone");
        let (ctx2, program, name) = (ctx.clone(), program.clone(), name.to_owned());
        button.connect_clicked(move |_| {
            let programs = ctx2.model().configured.yukimi.programs.iter().filter(|p| **p != program).cloned().collect();
            remove(&ctx2, &name, SystemChange { programs: Some(programs), ..SystemChange::default() });
        });
        row.add_suffix(&button);
        group.add(&row);
    }
    for attr in &mine.packages {
        let row = package_row(ctx, attr);
        let button = remove_button("Remove for everyone");
        let (ctx2, attr) = (ctx.clone(), attr.clone());
        button.connect_clicked(move |_| {
            let packages = ctx2.model().configured.yukimi.packages.iter().filter(|p| **p != attr).cloned().collect();
            remove(&ctx2, &attr, SystemChange { packages: Some(packages), ..SystemChange::default() });
        });
        row.add_suffix(&button);
        group.add(&row);
    }
    group
}

/// A package list written in the main configuration file: for everyone
/// (`user` is `None`) or for one user.
fn listed(ctx: &Ctx, model: &Model, entries: &[ListedPackage], user: Option<&str>) -> Option<adw::PreferencesGroup> {
    if entries.is_empty() {
        return None;
    }
    let group = adw::PreferencesGroup::new();
    let file = model.setup.main_name().unwrap_or_else(|| "configuration.nix".into());
    match user {
        None => {
            group.set_title("In your configuration");
            group.set_description(Some(&format!("environment.systemPackages in {file}, for everyone.")));
        }
        Some(user) => {
            group.set_title("In your configuration, for you");
            group.set_description(Some(&format!("users.users.{user}.packages in {file}.")));
        }
    }
    for entry in entries {
        let Some(attr) = &entry.attr else {
            // Something more than a name: shown as written, changed by hand.
            let row = adw::ActionRow::new();
            row.set_title(&gtk::glib::markup_escape_text(&entry.text));
            row.set_subtitle("Written as an expression, so change it in the file itself");
            row.add_prefix(&monogram(&entry.text, 36));
            group.add(&row);
            continue;
        };
        let row = package_row(ctx, attr);
        let button = remove_button(&format!("Take it out of {file}"));
        let (ctx2, attr, user) = (ctx.clone(), attr.clone(), user.map(str::to_owned));
        button.connect_clicked(move |_| {
            let change = match &user {
                None => SystemChange { unlist_system: vec![attr.clone()], ..SystemChange::default() },
                Some(user) => {
                    SystemChange { unlist_user: Some((user.clone(), vec![attr.clone()])), ..SystemChange::default() }
                }
            };
            remove(&ctx2, &attr, change);
        });
        row.add_suffix(&button);
        group.add(&row);
    }
    Some(group)
}

/// Applications chosen from each catalog the system brings.
fn chosen(ctx: &Ctx, model: &Model) -> Vec<adw::PreferencesGroup> {
    let mut groups = Vec::new();
    for source in &model.setup.facts.catalogs {
        let Some(setting) = &source.setting else { continue };
        let Some(ids) = model.configured.settings.get(setting).filter(|ids| !ids.is_empty()) else { continue };
        let group = adw::PreferencesGroup::new();
        group.set_title(source.title.as_deref().unwrap_or("Applications chosen for this system"));
        group.set_description(Some(&format!("Listed in {setting}, for everyone on this computer.")));
        for id in ids {
            let app = model.catalog.iter().find(|a| a.id == *id && a.setting.as_ref() == Some(setting));
            let name = app.map_or(id.as_str(), |a| a.name.as_str());
            let row = adw::ActionRow::new();
            row.set_title(name);
            if let Some(app) = app {
                row.set_subtitle(&app.description);
                row.add_suffix(&badge(&app.category, "category"));
            }
            row.add_prefix(&monogram(name, 36));
            let button = remove_button("Remove for everyone");
            let (ctx2, id, name, setting) = (ctx.clone(), id.clone(), name.to_owned(), setting.clone());
            button.connect_clicked(move |_| {
                let model = ctx2.model();
                let ids = model.configured.settings.get(&setting).cloned().unwrap_or_default();
                let ids = ids.into_iter().filter(|i| *i != id).collect();
                remove(
                    &ctx2,
                    &name,
                    SystemChange { applications: Some((setting.clone(), ids)), ..SystemChange::default() },
                );
            });
            row.add_suffix(&button);
            group.add(&row);
        }
        groups.push(group);
    }
    groups
}

/// What the current user installed for themselves.
fn just_for_you(ctx: &Ctx, model: &Model) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Just for you");
    group.set_description(Some("Installed into your own profile with nix profile. No password needed."));
    if model.user_packages.is_empty() {
        group.add(&empty_row("None yet"));
    }
    for element in &model.user_packages {
        let title = element.package_attr().unwrap_or(&element.name).to_owned();
        let row = package_row(ctx, &title);
        if ctx.index().and_then(|i| i.get(&title).map(|_| ())).is_none() {
            if let Some(path) = element.store_paths.first() {
                row.add_suffix(&dim(path.version()));
            }
            if let Some(url) = &element.original_url {
                row.set_subtitle(url);
            }
        }
        let button = remove_button("Remove from your profile");
        let (ctx2, name) = (ctx.clone(), element.name.clone());
        button.connect_clicked(move |_| ctx2.run(Operation::RemoveForMe { name: name.clone() }));
        row.add_suffix(&button);
        group.add(&row);
    }
    group
}

/// How many of the system's other packages show before "Show all".
const FIRST_ROWS: usize = 25;

fn system_row(package: &SystemPackage) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title_lines(1);
    row.set_subtitle_lines(1);
    row.set_title(&package.name);
    if !package.version.is_empty() {
        row.set_subtitle(&package.version);
    }
    row.add_suffix(&dim(&human_size(package.size)));
    row
}

/// Everything else the running system has.
fn the_rest(model: &Model) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Part of the system");
    let Some(store) = &model.store else {
        group.add(&pending_row("Reading what the system has…"));
        return group;
    };
    // Packages shown in the groups above don't show again here.
    let configured = &model.configured;
    let named: BTreeSet<&str> = configured
        .yukimi
        .packages
        .iter()
        .chain(configured.system.iter().chain(&configured.user).filter_map(|p| p.attr.as_ref()))
        .map(|attr| attr.rsplit('.').next().unwrap_or(attr))
        .collect();
    let rest: Vec<SystemPackage> =
        store.system_packages.iter().filter(|p| !named.contains(p.name.as_str())).cloned().collect();
    group.set_description(Some(&format!(
        "{} packages your desktops and settings bring. They come and go with those settings.",
        rest.len()
    )));
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some("Filter"));
    filter.set_valign(gtk::Align::Center);
    group.set_header_suffix(Some(&filter));
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    // A few rows at first, and those matching the filter: a list of
    // hundreds costs the window more than anyone reads.
    let rest = Rc::new(rest);
    let show = {
        let (list, rest) = (list.clone(), rest.clone());
        Rc::new(move |text: &str, all: bool| {
            while let Some(row) = list.first_child() {
                list.remove(&row);
            }
            let matching: Vec<SystemPackage> =
                rest.iter().filter(|p| text.is_empty() || p.name.to_lowercase().contains(text)).cloned().collect();
            if all {
                let target = list.clone();
                fill_later(&list, matching, system_row, move |row| target.append(row));
                return;
            }
            for package in matching.iter().take(FIRST_ROWS) {
                list.append(&system_row(package));
            }
            if matching.len() > FIRST_ROWS {
                let row = adw::ActionRow::new();
                row.set_title(&format!("Show all {}", matching.len()));
                row.set_widget_name("show-all");
                row.set_activatable(true);
                row.add_css_class("dim-row");
                row.add_suffix(&gtk::Image::from_icon_name("go-down-symbolic"));
                list.append(&row);
            }
        })
    };
    show("", false);
    {
        let (show, filter) = (show.clone(), filter.clone());
        list.connect_row_activated(move |_, row| {
            if row.widget_name() == "show-all" {
                show(&filter.text().to_lowercase(), true);
            }
        });
    }
    filter.connect_search_changed(move |entry| show(&entry.text().to_lowercase(), false));
    group.add(&list);
    group
}

pub fn build(ctx: &Ctx) -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(36);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let scroll = page(&content);
    ctx.on_refresh("installed", move |ctx| {
        clear(&content);
        let model = ctx.model();
        if let Some(why) = model.cannot_change_system() {
            let banner = adw::Banner::new(&format!("{why}. Add ./yukimi.nix to its imports to install for everyone."));
            banner.set_revealed(true);
            content.append(&banner);
        }
        content.append(&added(ctx, &model));
        if let Some(group) = listed(ctx, &model, &model.configured.system, None) {
            content.append(&group);
        }
        if let Some(group) = listed(ctx, &model, &model.configured.user, Some(&model.user)) {
            content.append(&group);
        }
        for group in chosen(ctx, &model) {
            content.append(&group);
        }
        content.append(&just_for_you(ctx, &model));
        content.append(&the_rest(&model));
    });
    scroll
}
