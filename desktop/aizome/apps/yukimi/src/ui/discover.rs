//! Finding software: a catalog of well-known applications when nothing is
//! typed, and a search of all of Nixpkgs when something is, under the
//! catalog's applications by that name. Every package can be tried without
//! installing it, installed just for you, or for everyone, except one the
//! catalog installs by switching on its NixOS program (Steam): that one is
//! installed for everyone, as the catalog installs it, however it is found.
//!
//! Searching happens in a thread of its own (see `app.rs`): typing never
//! waits for it, and only the answer to the newest query shows.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use yukimi_system::catalog::{self, Application};
use yukimi_system::index::{Package, PackageIndex};

use super::widgets::{grid, monogram, tap};
use super::{caption, heading, mono, panel, tag};
use crate::app::{Msg, Yukimi, blocked, index_id};
use crate::model::Model;

/// What is installed, worked out once for all the badges on a page.
pub struct Installed<'a> {
    model: &'a Model,
    index: Option<Arc<PackageIndex>>,
    user: BTreeSet<&'a str>,
    /// The running system's package names.
    pub system: BTreeSet<&'a str>,
    versions: BTreeMap<&'a str, &'a str>,
}

impl<'a> Installed<'a> {
    pub fn of(app: &'a Yukimi) -> Installed<'a> {
        let model = &*app.model;
        let versions = model
            .store
            .as_ref()
            .map(|s| s.system_packages.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect())
            .unwrap_or_default();
        Installed { model, index: app.index.clone(), user: model.user_attrs(), system: model.system_names(), versions }
    }

    /// Whether the running system has this package. The index knows names,
    /// not store paths, and variants share a name (`btop`, `btop-cuda` and
    /// `btop-rocm` are all btop), so a name and version match counts only
    /// for the attribute named after the package, or for the only one there
    /// is.
    fn on_system(&self, package: &Package) -> bool {
        let matches = self.versions.get(package.pname.as_str()) == Some(&package.version.as_str());
        let canonical = package.attr == package.pname
            || self.index.as_ref().is_none_or(|i| i.get(&package.pname).is_none_or(|p| p.version != package.version));
        matches && canonical
    }

    pub fn for_everyone(&self, attr: &str) -> bool {
        let model = self.model;
        let configured = &model.configured;
        configured.yukimi.packages.iter().any(|p| p == attr)
            || configured.system.iter().any(|p| p.attr.as_deref() == Some(attr))
            || catalog::needing_system(&model.catalog, attr).is_some_and(|app| model.app_installed(app, &self.system))
    }

    pub fn for_you(&self, attr: &str) -> bool {
        self.user.contains(attr) || self.model.configured.user.iter().any(|p| p.attr.as_deref() == Some(attr))
    }

    /// Chips for what a package is and whether it is installed.
    pub fn badges(&self, package: &Package) -> Vec<Element<Msg>> {
        let mut out = Vec::new();
        let everyone = self.for_everyone(&package.attr);
        if everyone {
            out.push(tag("for everyone", Role::Success));
        }
        if self.for_you(&package.attr) {
            out.push(tag("for you", Role::Success));
        }
        if !everyone && self.on_system(package) {
            out.push(tag("on your system", Role::Accent));
        }
        if package.unfree {
            out.push(tag("unfree", Role::Warning));
        }
        if package.broken {
            out.push(tag("broken", Role::Danger));
        }
        if package.insecure {
            out.push(tag("insecure", Role::Danger));
        }
        if !package.available {
            out.push(tag("not for this computer", Role::Danger));
        }
        out
    }
}

fn app_card(model: &Model, system: &BTreeSet<&str>, app: &Application) -> Element<Msg> {
    let installed = model.app_installed(app, system);
    let why_not = app.unavailable.clone().or_else(|| blocked(model, app.unfree));
    let mut install = if installed {
        button("Installed").icon(Icon::Check).enabled(false)
    } else {
        button(if app.unavailable.is_some() { "Unavailable" } else { "Install" })
            .primary()
            .on_press_maybe(why_not.is_none().then(|| Msg::InstallApp(app.id.clone())))
    };
    if let (Some(why), false) = (&why_not, installed) {
        install = install.tooltip(why.clone());
    }
    let mut foot = row![install].spacing(8.0).width(Fill);
    if app.unfree {
        foot = foot.push(spacer()).push(tag("unfree", Role::Warning));
    } else if app.program.is_some() {
        foot = foot.push(spacer()).push(tag("program", Role::Violet));
    }
    let mut body = column![
        row![
            monogram(&app.name, 36.0),
            column![text(app.name.clone()).weight(650.0), text(app.category.clone()).size(11.5).role(Role::Text3)]
                .spacing(1.0),
        ]
        .spacing(10.0),
        text(app.description.clone()).wrap().size(12.0).role(Role::Text2).line_height(1.3),
    ]
    .spacing(10.0);
    if let Some(reason) = &app.unavailable {
        body = body.push(text(reason.clone()).wrap().size(11.5).role(Role::Text3));
    }
    let body = body.push(spacer()).push(foot).width(Fill).height(Fill);
    panel(body).padding(12.0).width(Fill).height(Fill).into()
}

/// Cards for some of the catalog's applications.
fn cards(model: &Model, system: &BTreeSet<&str>, apps: &[&Application]) -> Element<Msg> {
    grid(apps.iter().map(|a| app_card(model, system, a)).collect(), 200.0, 12.0)
}

/// The catalog, by category, what isn't installed first in each.
fn catalog_view(model: &Model) -> Vec<Element<Msg>> {
    let system = model.system_names();
    let mut categories: Vec<&str> = Vec::new();
    for app in &model.catalog {
        if !categories.contains(&app.category.as_str()) {
            categories.push(&app.category);
        }
    }
    let mut out = Vec::new();
    for category in categories {
        let mut apps: Vec<&Application> = model.catalog.iter().filter(|a| a.category == category).collect();
        apps.sort_by_key(|a| model.app_installed(a, &system));
        out.push(column![caption(category), cards(model, &system, &apps)].spacing(8.0).width(Fill).into());
    }
    out
}

fn result_row(installed: &Installed, package: &Package) -> Element<Msg> {
    let mut r = row![
        monogram(&package.attr, 32.0),
        column![
            text(package.attr.clone()).weight(600.0),
            text(package.description.clone()).size(12.0).role(Role::Text3).wrap(),
        ]
        .spacing(2.0)
        .width(Fill),
    ]
    .spacing(12.0)
    .width(Fill);
    for b in installed.badges(package) {
        r = r.push(b);
    }
    r = r
        .push(mono(package.version.clone()).role(Role::Text3))
        .push(icon(Icon::ChevronRight).size(14.0).role(Role::Text3));
    tap(r).padding([8.0, 12.0]).on_press(Msg::Open(package.clone())).named(package.attr.clone()).into()
}

pub fn page(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let mut c = column(Vec::new()).spacing(20.0).width(Fill);
    if app.index_stale && app.indexing {
        c = c.push(super::banner(
            Icon::Hourglass,
            Role::Accent,
            "Searching the list from an earlier Nixpkgs while this version's is made.".to_owned(),
        ));
    }
    let query = app.query.trim();
    if query.is_empty() {
        c = c.push(heading(
            "Find software",
            "Well-known applications to start from. Search to look through every package in Nixpkgs: try one \
             without installing it, install it just for you, or for everyone on this computer.",
        ));
        if app.index.is_none() && !app.indexing && model.nixpkgs.is_some() {
            c = c.push(
                panel(
                    row![
                        icon(Icon::ListTree).size(18.0).role(Role::Accent),
                        text(
                            "To search all of Nixpkgs, Yukimi first makes a list of its packages: about a minute, once \
                              for each version."
                        )
                        .wrap()
                        .width(Fill),
                        button("Make the List").on_press(Msg::MakeIndex),
                    ]
                    .spacing(12.0)
                    .width(Fill),
                )
                .width(Fill),
            );
        }
        for section in catalog_view(model) {
            c = c.push(section);
        }
        return c.into();
    }
    // The catalog's applications by that name come first: Steam's card
    // installs what its package alone can't.
    let apps: Vec<&Application> = model.catalog.iter().filter(|a| a.matches(query)).collect();
    if !apps.is_empty() {
        let system = model.system_names();
        c = c.push(column![caption("Applications"), cards(model, &system, &apps)].spacing(8.0).width(Fill));
    }
    let Some(index) = &app.index else {
        let waiting =
            if app.indexing { "The package list is still being made." } else { "There is no package list yet." };
        return c
            .push(column![caption("Packages"), super::list(vec![super::empty(waiting)])].spacing(8.0).width(Fill))
            .into();
    };
    let current = app.found.as_ref().filter(|f| f.query == app.query && f.index == index_id(index));
    let packages: Element<Msg> = match current {
        None => super::list(vec![super::pending("Searching…")]),
        Some(found) if found.results.is_empty() && !apps.is_empty() => {
            super::list(vec![super::empty("No other packages by that name.")])
        }
        Some(found) if found.results.is_empty() => panel(
            column![
                icon(Icon::Search).size(28.0).role(Role::Text3),
                text("Nothing by that name").size(15.0).weight(650.0),
                text("Try a shorter word, or what the program does: “image editor”, “pdf”.").role(Role::Text3),
            ]
            .spacing(8.0)
            .align(Align::Center),
        )
        .align_x(Align::Center)
        .padding(28.0)
        .width(Fill)
        .into(),
        Some(found) => {
            let installed = Installed::of(app);
            let mut rows: Vec<Element<Msg>> = Vec::new();
            for package in found.results.iter() {
                rows.push(result_row(&installed, package));
            }
            super::list(rows)
        }
    };
    let count = current.map(|f| f.results.len()).unwrap_or(0);
    let label =
        if count >= crate::app::RESULTS { format!("Packages · the first {count}") } else { "Packages".to_owned() };
    c.push(column![caption(&label), packages].spacing(8.0).width(Fill)).into()
}

/// The full story of one package, with what can be done with it, in a
/// sheet.
pub fn details(app: &Yukimi, package: &Package, sheet: WindowKey) -> Element<Msg> {
    let model = &app.model;
    let installed = Installed::of(app);
    let mut c = column![
        row![
            monogram(&package.attr, 52.0),
            column![
                text(package.attr.clone()).size(20.0).weight(700.0),
                mono(if package.pname == package.attr {
                    package.version.clone()
                } else {
                    format!("{} {}", package.pname, package.version)
                }),
            ]
            .spacing(3.0),
        ]
        .spacing(14.0),
    ]
    .spacing(12.0)
    .width(Fill);
    let badges = installed.badges(package);
    if !badges.is_empty() {
        c = c.push(row(badges).spacing(6.0));
    }
    if !package.description.is_empty() {
        c = c.push(text(package.description.clone()).wrap().size(15.0).weight(500.0));
    }
    if !package.long_description.is_empty() {
        let words = package.long_description.split_whitespace().collect::<Vec<_>>().join(" ");
        c = c.push(text(words).wrap().role(Role::Text2).line_height(1.35));
    }
    let mut facts: Vec<Element<Msg>> = Vec::new();
    let fact = |title: &str, value: &str, trailing: Option<Element<Msg>>| -> Element<Msg> {
        let mut r = row![
            text(title.to_owned()).size(12.0).role(Role::Text3).width(90.0),
            text(value.to_owned()).mono().size(12.0).width(Fill).selectable(),
        ]
        .spacing(10.0)
        .padding([7.0, 12.0])
        .width(Fill);
        if let Some(t) = trailing {
            r = r.push(t);
        }
        r.into()
    };
    if !package.main_program.is_empty() {
        facts.push(fact("Command", &package.main_program, None));
    }
    if !package.license.is_empty() {
        facts.push(fact("License", &package.license, None));
    }
    if !package.homepage.is_empty() {
        let open = icon_button(Icon::ExternalLink)
            .plain()
            .on_press(Msg::OpenUrl(package.homepage.clone()))
            .tooltip("Open the homepage");
        facts.push(fact("Homepage", &package.homepage, Some(open.into())));
    }
    if !facts.is_empty() {
        c = c.push(super::list(facts));
    }

    // A package the catalog installs by switching on its NixOS program is
    // installed that way here too: without the 32-bit graphics drivers its
    // program switches on, Steam's package stops before its window opens.
    let needing = catalog::needing_system(&model.catalog, &package.attr);
    if let Some(app_entry) = needing {
        let needs = app_entry.needs.as_deref().unwrap_or("more than its package");
        let mut words = format!(
            "{} needs {needs}. Only the system can set that up, so it can only be installed for everyone.",
            app_entry.name
        );
        if installed.for_you(&package.attr) {
            words.push_str(" The copy installed just for you can be removed in Installed.");
        }
        c = c.push(text(words).wrap().role(Role::Text3));
    }

    let installable = package.available && !package.broken;
    let why_not = needing
        .and_then(|a| a.unavailable.clone())
        .or_else(|| blocked(model, package.unfree || needing.is_some_and(|a| a.unfree)));
    let done = match needing {
        Some(a) => model.app_installed(a, &installed.system),
        None => installed.for_everyone(&package.attr),
    };
    let mut try_it = button("Try It")
        .icon(Icon::Play)
        .on_press_maybe((installable && needing.is_none()).then(|| Msg::TryIt(package.clone())));
    try_it = try_it.tooltip(match needing {
        Some(a) => {
            format!("Only the system can set up what {} needs, so it can't be tried without installing it", a.name)
        }
        None => "Run it in a terminal without installing it. Nothing is changed.".to_owned(),
    });
    let mut for_me = button("Install for Me").icon(Icon::User).on_press_maybe(
        (installable && needing.is_none() && !installed.for_you(&package.attr))
            .then(|| Msg::InstallForMe(package.clone())),
    );
    if let Some(a) = needing {
        for_me = for_me.tooltip(format!(
            "Only the system can set up what {} needs, so it can't be installed just for you",
            a.name
        ));
    } else {
        for_me = for_me.tooltip("Into your own profile: no password, and nobody else sees it.");
    }
    let mut for_all = button("Install for Everyone").primary().icon(Icon::Users).on_press_maybe(
        (installable && why_not.is_none() && !done).then(|| Msg::InstallForEveryone(package.attr.clone())),
    );
    if let Some(why) = &why_not {
        for_all = for_all.tooltip(why.clone());
    }
    c = c.push(divider().edge(Edge::Line));
    c = c.push(
        row![button("Close").cancel().on_press(Msg::CloseSheet(sheet)), spacer(), try_it, for_me, for_all]
            .spacing(8.0)
            .width(Fill),
    );
    container(c).padding(20.0).width(600.0).into()
}
