//! What is installed, grouped by why: added with Yukimi, installed by a
//! user for themselves, listed in the configuration by hand, chosen from a
//! catalog the system brings, or part of the system because a desktop or
//! setting brings it.
use std::collections::BTreeSet;

use aizome_kit::prelude::*;
use aizome_kit::row;
use yukimi_config::edit::ListedPackage;
use yukimi_system::human_size;

use super::widgets::{monogram, tap};
use super::{banner, empty, group, item_row, link, mono, pending, tag};
use crate::app::{Msg, Page, Removal, Yukimi};
use crate::model::SystemPackage;

/// How many of the system's other packages show before "Show all".
const FIRST_ROWS: usize = 25;
/// A row of the system's packages, and how tall the list of all of them
/// gets before it scrolls.
const SYSTEM_ROW: f64 = 32.0;
const ALL_HEIGHT: f64 = 448.0;

fn remove_button(tooltip: &str, msg: Msg) -> Element<Msg> {
    icon_button(Icon::Trash).plain().on_press(msg).tooltip(tooltip.to_owned()).into()
}

/// A row for a package: its name, what it is (from the index, once there
/// is one) and its version.
fn package_row(
    app: &Yukimi,
    attr: &str,
    fallback: Option<(String, String)>,
    trailing: Vec<Element<Msg>>,
) -> Element<Msg> {
    let found = app.index.as_ref().and_then(|i| i.get(attr));
    let (description, version) = match (found, fallback) {
        (Some(p), _) => (p.description.clone(), p.version.clone()),
        (None, Some((d, v))) => (d, v),
        (None, None) => (String::new(), String::new()),
    };
    let mut right = Vec::new();
    if !version.is_empty() {
        right.push(mono(version).role(Role::Text3).into());
    }
    right.extend(trailing);
    item_row(
        Some(monogram(attr, 28.0)),
        text(attr.to_owned()).weight(600.0).into(),
        (!description.is_empty()).then(|| text(description).size(12.0).role(Role::Text3).into()),
        right,
    )
}

/// What Yukimi added for everyone: packages and programs in `yukimi.nix`.
fn added(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let file = model.setup.packages_file().map(|f| f.display().to_string()).unwrap_or_else(|| "yukimi.nix".into());
    let mine = &model.configured.yukimi;
    let mut rows = Vec::new();
    if mine.is_empty() {
        rows.push(empty("None yet: find applications and packages in Discover."));
    }
    for program in &mine.programs {
        let app_entry = model.catalog.iter().find(|a| a.program.as_ref() == Some(program));
        let name = app_entry.map_or(program.as_str(), |a| a.name.as_str());
        let subtitle = app_entry.map_or_else(|| format!("programs.{program}"), |a| a.description.clone());
        rows.push(item_row(
            Some(monogram(name, 28.0)),
            text(name.to_owned()).weight(600.0).into(),
            Some(text(subtitle).size(12.0).role(Role::Text3).into()),
            vec![
                tag("program", Role::Violet),
                remove_button("Remove for everyone", Msg::Remove(Removal::Program(program.clone()))),
            ],
        ));
    }
    for attr in &mine.packages {
        rows.push(package_row(
            app,
            attr,
            None,
            vec![remove_button("Remove for everyone", Msg::Remove(Removal::Package(attr.clone())))],
        ));
    }
    group(
        "Added with Yukimi",
        Some(format!("For everyone on this computer, kept in {file}.")),
        Some(link("Find more", Msg::Show(Page::Discover))),
        rows,
    )
}

/// What the current user installed for themselves.
fn just_for_you(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let mut rows = Vec::new();
    if model.user_packages.is_empty() {
        rows.push(empty("None yet."));
    }
    for element in &model.user_packages {
        let title = element.package_attr().unwrap_or(&element.name).to_owned();
        let fallback = (
            element.original_url.clone().unwrap_or_default(),
            element.store_paths.first().map(|p| p.version().to_owned()).unwrap_or_default(),
        );
        rows.push(package_row(
            app,
            &title,
            Some(fallback),
            vec![remove_button("Remove from your profile", Msg::Remove(Removal::ForMe(element.name.clone())))],
        ));
    }
    group(
        "Just for you",
        Some("Installed into your own profile with nix profile. No password needed.".to_owned()),
        None,
        rows,
    )
}

/// A package list written in the main configuration file: for everyone
/// (`user` is `None`) or for one user.
fn listed(app: &Yukimi, entries: &[ListedPackage], user: Option<&str>) -> Option<Element<Msg>> {
    if entries.is_empty() {
        return None;
    }
    let file = app.model.setup.main_name().unwrap_or_else(|| "configuration.nix".into());
    let (title, description) = match user {
        None => ("In your configuration", format!("environment.systemPackages in {file}, for everyone.")),
        Some(user) => ("In your configuration, for you", format!("users.users.{user}.packages in {file}.")),
    };
    let rows = entries
        .iter()
        .map(|entry| match &entry.attr {
            // Something more than a name: shown as written, changed by hand.
            None => item_row(
                Some(monogram(&entry.text, 28.0)),
                text(entry.text.clone()).mono().size(12.0).into(),
                Some(
                    text("Written as an expression, so change it in the file itself.")
                        .size(12.0)
                        .role(Role::Text3)
                        .into(),
                ),
                Vec::new(),
            ),
            Some(attr) => package_row(
                app,
                attr,
                None,
                vec![remove_button(
                    &format!("Take it out of {file}"),
                    Msg::Remove(Removal::Listed { attr: attr.clone(), user: user.map(str::to_owned) }),
                )],
            ),
        })
        .collect();
    Some(group(title, Some(description), None, rows))
}

/// Applications chosen from each catalog the system brings.
fn chosen(app: &Yukimi) -> Vec<Element<Msg>> {
    let model = &app.model;
    let mut groups = Vec::new();
    for source in &model.setup.facts.catalogs {
        let Some(setting) = &source.setting else { continue };
        let Some(ids) = model.configured.settings.get(setting).filter(|ids| !ids.is_empty()) else { continue };
        let rows = ids
            .iter()
            .map(|id| {
                let entry = model.catalog.iter().find(|a| a.id == *id && a.setting.as_ref() == Some(setting));
                let name = entry.map_or(id.as_str(), |a| a.name.as_str()).to_owned();
                let mut right = Vec::new();
                if let Some(a) = entry {
                    right.push(tag(&a.category, Role::Text3));
                }
                right.push(remove_button(
                    "Remove for everyone",
                    Msg::Remove(Removal::App { setting: setting.clone(), id: id.clone(), name: name.clone() }),
                ));
                item_row(
                    Some(monogram(&name, 28.0)),
                    text(name).weight(600.0).into(),
                    entry.map(|a| text(a.description.clone()).size(12.0).role(Role::Text3).into()),
                    right,
                )
            })
            .collect();
        groups.push(group(
            source.title.as_deref().unwrap_or("Applications chosen for this system"),
            Some(format!("Listed in {setting}, for everyone on this computer.")),
            None,
            rows,
        ));
    }
    groups
}

fn system_row(package: &SystemPackage) -> Element<Msg> {
    row![
        text(package.name.clone()).weight(500.0).width(Length::FillPortion(3)),
        text(package.version.clone()).mono().size(12.0).role(Role::Text3).width(Length::FillPortion(2)),
        mono(human_size(package.size)).width(72.0),
    ]
    .spacing(12.0)
    .padding([6.0, 12.0])
    .width(Fill)
    .into()
}

/// Everything else the running system has.
fn the_rest(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let filter: Element<Msg> =
        search_field(&app.filter).placeholder("Filter").on_change(Msg::Filter).id("filter").width(220.0).into();
    let Some(store) = &model.store else {
        return group("Part of the system", None, Some(filter), vec![pending("Reading what the system has…")]);
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
    let rest: Vec<&SystemPackage> = store.system_packages.iter().filter(|p| !named.contains(p.name.as_str())).collect();
    let needle = app.filter.trim().to_lowercase();
    let matching: Vec<&SystemPackage> =
        rest.iter().copied().filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle)).collect();
    if app.show_all && matching.len() > FIRST_ROWS {
        // All of them, in a list that builds only the rows in sight.
        let shown: std::rc::Rc<Vec<SystemPackage>> = std::rc::Rc::new(matching.iter().map(|p| (*p).clone()).collect());
        let height = (shown.len() as f64 * SYSTEM_ROW).min(ALL_HEIGHT);
        let all = virtual_list(shown.len(), SYSTEM_ROW, move |i| system_row(&shown[i])).height(height);
        let description =
            format!("{} packages your desktops and settings bring. They come and go with those settings.", rest.len());
        return group("Part of the system", Some(description), Some(filter), vec![all.into()]);
    }
    let limit = FIRST_ROWS;
    let mut rows: Vec<Element<Msg>> = matching.iter().take(limit).map(|p| system_row(p)).collect();
    if matching.is_empty() {
        rows.push(empty(&format!("Nothing called “{}” is part of the system.", app.filter.trim())));
    } else if matching.len() > limit {
        rows.push(
            tap(row![
                text(format!("Show all {}", matching.len())).role(Role::Accent),
                spacer(),
                icon(Icon::ChevronDown).size(14.0).role(Role::Text3)
            ]
            .width(Fill)
            .height(22.0))
            .padding([6.0, 12.0])
            .on_press(Msg::ShowAll)
            .named("Show all")
            .into(),
        );
    }
    group(
        "Part of the system",
        Some(format!(
            "{} packages your desktops and settings bring. They come and go with those settings.",
            rest.len()
        )),
        Some(filter),
        rows,
    )
}

pub fn page(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let mut c = column(Vec::new()).spacing(22.0).width(Fill);
    if let Some(why) = model.cannot_change_system() {
        let words = if model.paths.elsewhere {
            why
        } else {
            format!("{why}. Add ./yukimi.nix to its imports to install for everyone.")
        };
        c = c.push(banner(Icon::TriangleAlert, Role::Warning, words));
    }
    c = c.push(added(app));
    c = c.push(just_for_you(app));
    if let Some(g) = listed(app, &model.configured.system, None) {
        c = c.push(g);
    }
    if let Some(g) = listed(app, &model.configured.user, Some(&model.user)) {
        c = c.push(g);
    }
    for g in chosen(app) {
        c = c.push(g);
    }
    c.push(the_rest(app)).into()
}
