// SPDX-License-Identifier: MIT OR Apache-2.0
//! Finding software: a catalog of well-known applications when nothing is
//! typed, and a search of all of Nixpkgs when something is. Every package
//! can be tried without installing it, installed just for you, or for
//! everyone.
//!
//! Searching happens in a thread of its own: typing never waits for it,
//! and only the answer to the newest query is shown.
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;
use yukimi_system::catalog::Application;
use yukimi_system::index::{Package, PackageIndex};

use super::Ctx;
use super::widgets::{badge, clear, dim, monogram, page, wrapping};
use crate::model::Model;
use crate::ops::{self, Operation, SystemChange};

const RESULTS: usize = 60;

/// What is installed, worked out once for all the badges on a page.
struct Installed<'a> {
    model: &'a Model,
    index: Option<Arc<PackageIndex>>,
    user: BTreeSet<&'a str>,
    /// Versions of the running system's packages, by name.
    versions: std::collections::BTreeMap<&'a str, &'a str>,
}

impl<'a> Installed<'a> {
    fn of(ctx: &Ctx, model: &'a Model) -> Installed<'a> {
        let versions = model
            .store
            .as_ref()
            .map(|s| s.system_packages.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect())
            .unwrap_or_default();
        Installed { model, index: ctx.index(), user: model.user_attrs(), versions }
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

    fn for_everyone(&self, attr: &str) -> bool {
        let configured = &self.model.configured;
        configured.yukimi.packages.iter().any(|p| p == attr)
            || configured.system.iter().any(|p| p.attr.as_deref() == Some(attr))
    }

    fn for_you(&self, attr: &str) -> bool {
        self.user.contains(attr) || self.model.configured.user.iter().any(|p| p.attr.as_deref() == Some(attr))
    }

    /// Badges for what a package is and whether it is installed.
    fn badges(&self, package: &Package) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let everyone = self.for_everyone(&package.attr);
        if everyone {
            row.append(&badge("for everyone", "installed"));
        }
        if self.for_you(&package.attr) {
            row.append(&badge("for you", "installed"));
        }
        if !everyone && self.on_system(package) {
            row.append(&badge("on your system", "system"));
        }
        if package.unfree {
            row.append(&badge("unfree", "unfree"));
        }
        if package.broken {
            row.append(&badge("broken", "broken"));
        }
        if package.insecure {
            row.append(&badge("insecure", "broken"));
        }
        if !package.available {
            row.append(&badge("not for this computer", "broken"));
        }
        row
    }
}

/// Why something can't be installed for everyone, if it can't.
fn blocked(model: &Model, unfree: bool) -> Option<String> {
    model
        .cannot_change_system()
        .or_else(|| (unfree && !model.allow_unfree).then(|| "This system doesn't allow unfree software".to_owned()))
}

fn install_for_everyone(ctx: &Ctx, attr: &str) {
    let model = ctx.model();
    let mut packages = model.configured.yukimi.packages.clone();
    packages.push(attr.to_owned());
    ctx.confirm(
        &format!("Install {attr} for everyone?"),
        "Yukimi adds it to the system configuration and builds the new system, which asks for an administrator \
         password and can take a few minutes. The current version stays in History.",
        "Install",
        Operation::ChangeSystem {
            change: SystemChange { packages: Some(packages), ..SystemChange::default() },
            what: attr.to_owned(),
            adding: true,
        },
    );
}

/// What installing an application changes: its setting's list for one from
/// a system catalog, otherwise `yukimi.nix`.
fn app_change(model: &Model, app: &Application) -> SystemChange {
    if let Some(setting) = &app.setting {
        let mut ids = model.configured.settings.get(setting).cloned().unwrap_or_default();
        for id in std::iter::once(&app.id).chain(&app.requires) {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        return SystemChange { applications: Some((setting.clone(), ids)), ..SystemChange::default() };
    }
    let mine = &model.configured.yukimi;
    let mut change = SystemChange::default();
    if !app.packages.is_empty() {
        let mut packages = mine.packages.clone();
        packages.extend(app.packages.iter().filter(|p| !mine.packages.contains(p)).cloned());
        change.packages = Some(packages);
    }
    if let Some(program) = &app.program {
        let mut programs = mine.programs.clone();
        if !programs.contains(program) {
            programs.push(program.clone());
        }
        change.programs = Some(programs);
    }
    change
}

fn install_app(ctx: &Ctx, app: &Application) {
    let model = ctx.model();
    ctx.confirm(
        &format!("Install {}?", app.name),
        "It is installed for everyone on this computer. Yukimi changes the system configuration and builds the new \
         system, which asks for an administrator password. The current version stays in History.",
        "Install",
        Operation::ChangeSystem { change: app_change(&model, app), what: app.name.clone(), adding: true },
    );
}

/// The full story of one package, with what can be done with it.
fn details(ctx: &Ctx, package: &Package) {
    let model = ctx.model();
    let installed = Installed::of(ctx, &model);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_top(6);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    top.append(&monogram(&package.attr, 64));
    let names = gtk::Box::new(gtk::Orientation::Vertical, 2);
    names.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(&package.attr));
    title.add_css_class("title-1");
    title.set_xalign(0.0);
    let attr = dim(&if package.pname == package.attr {
        package.version.clone()
    } else {
        format!("{} {}", package.pname, package.version)
    });
    attr.set_xalign(0.0);
    names.append(&title);
    names.append(&attr);
    top.append(&names);
    content.append(&top);
    content.append(&installed.badges(package));
    if !package.description.is_empty() {
        content.append(&wrapping(&package.description, &["title-4"]));
    }
    if !package.long_description.is_empty() {
        let text = package.long_description.split_whitespace().collect::<Vec<_>>().join(" ");
        content.append(&wrapping(&text, &["body"]));
    }

    let facts = adw::PreferencesGroup::new();
    let fact = |title: &str, value: &str| {
        let row = adw::ActionRow::new();
        row.set_title(title);
        row.set_subtitle(&glib::markup_escape_text(value));
        row.set_subtitle_selectable(true);
        row.add_css_class("property");
        row
    };
    if !package.main_program.is_empty() {
        facts.add(&fact("Command", &package.main_program));
    }
    if !package.license.is_empty() {
        facts.add(&fact("License", &package.license));
    }
    if !package.homepage.is_empty() {
        let row = fact("Homepage", &package.homepage);
        let open = gtk::Button::from_icon_name("adw-external-link-symbolic");
        open.add_css_class("flat");
        open.set_valign(gtk::Align::Center);
        let url = package.homepage.clone();
        let window = ctx.window().clone();
        open.connect_clicked(move |_| {
            gtk::UriLauncher::new(&url).launch(Some(&window), gtk::gio::Cancellable::NONE, |_| {});
        });
        row.add_suffix(&open);
        facts.add(&row);
    }
    content.append(&facts);

    let installable = package.available && !package.broken;
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.set_halign(gtk::Align::Center);
    actions.set_margin_top(6);
    let try_it = gtk::Button::with_label("Try it");
    try_it.add_css_class("pill");
    try_it.set_tooltip_text(Some("Run it in a terminal without installing it. Nothing is changed."));
    try_it.set_sensitive(installable);
    let for_me = gtk::Button::with_label("Install for me");
    for_me.add_css_class("pill");
    for_me.set_sensitive(installable && !installed.for_you(&package.attr));
    let for_all = gtk::Button::with_label("Install for everyone");
    for_all.add_css_class("pill");
    for_all.add_css_class("suggested-action");
    let why_not = blocked(&model, package.unfree);
    for_all.set_sensitive(installable && why_not.is_none() && !installed.for_everyone(&package.attr));
    for_all.set_tooltip_text(why_not.as_deref());
    actions.append(&try_it);
    actions.append(&for_me);
    actions.append(&for_all);
    content.append(&actions);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(640)
        .child(&content)
        .build();
    view.set_content(Some(&scroll));
    let dialog = adw::Dialog::builder().title(&package.attr).content_width(560).child(&view).build();

    {
        let (ctx, package) = (ctx.clone(), package.clone());
        try_it.connect_clicked(move |_| {
            let command =
                ops::try_command(&ctx.model().nixpkgs_ref(), &package.attr, &package.main_program, package.unfree);
            match ops::open_terminal(&command) {
                Ok(()) => ctx.toast(&format!("Trying {} in a terminal", package.attr)),
                Err(e) => ctx.toast(&e),
            }
        });
    }
    {
        let (ctx, package, dialog) = (ctx.clone(), package.clone(), dialog.clone());
        for_me.connect_clicked(move |_| {
            dialog.close();
            ctx.run(Operation::InstallForMe { attr: package.attr.clone(), unfree: package.unfree });
        });
    }
    {
        let (ctx, attr, dialog) = (ctx.clone(), package.attr.clone(), dialog.clone());
        for_all.connect_clicked(move |_| {
            dialog.close();
            install_for_everyone(&ctx, &attr);
        });
    }
    dialog.present(Some(ctx.window()));
    // Start on the buttons, not inside the facts.
    if for_all.is_sensitive() {
        for_all.grab_focus();
    } else {
        try_it.grab_focus();
    }
}

fn result_row(ctx: &Ctx, installed: &Installed, package: &Package) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(&glib::markup_escape_text(&package.attr));
    row.set_subtitle(&glib::markup_escape_text(&package.description));
    row.set_subtitle_lines(2);
    row.add_prefix(&monogram(&package.attr, 40));
    row.add_suffix(&installed.badges(package));
    row.add_suffix(&dim(&package.version));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row.set_activatable(true);
    let (ctx, package) = (ctx.clone(), package.clone());
    row.connect_activated(move |_| details(&ctx, &package));
    row
}

fn app_card(ctx: &Ctx, model: &Model, system: &BTreeSet<&str>, app: &Application) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.add_css_class("app-card");
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    top.append(&monogram(&app.name, 44));
    let names = gtk::Box::new(gtk::Orientation::Vertical, 0);
    names.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some(&app.name));
    name.add_css_class("heading");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    names.append(&name);
    let category = dim(&app.category);
    category.set_xalign(0.0);
    names.append(&category);
    top.append(&names);
    card.append(&top);
    let description = wrapping(&app.description, &["dim-label"]);
    description.set_lines(3);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    description.set_vexpand(true);
    description.set_valign(gtk::Align::Start);
    card.append(&description);
    let installed = model.app_installed(app, system);
    let why_not = blocked(model, app.unfree);
    let button = gtk::Button::with_label(if installed { "Installed" } else { "Install" });
    button.add_css_class("pill");
    button.set_halign(gtk::Align::Start);
    if !installed {
        button.add_css_class("suggested-action");
    }
    button.set_sensitive(!installed && why_not.is_none());
    button.set_tooltip_text(why_not.as_deref().filter(|_| !installed));
    let (ctx, app) = (ctx.clone(), app.clone());
    button.connect_clicked(move |_| install_app(&ctx, &app));
    card.append(&button);
    card
}

/// The catalog, by category, what isn't installed first in each.
fn catalog(ctx: &Ctx, model: &Model, content: &gtk::Box) {
    let system = model.system_names();
    let mut categories: Vec<&str> = Vec::new();
    for app in &model.catalog {
        if !categories.contains(&app.category.as_str()) {
            categories.push(&app.category);
        }
    }
    for category in categories {
        let heading = gtk::Label::new(Some(category));
        heading.add_css_class("title-4");
        heading.set_xalign(0.0);
        content.append(&heading);
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_min_children_per_line(2);
        flow.set_max_children_per_line(4);
        flow.set_column_spacing(12);
        flow.set_row_spacing(12);
        let mut apps: Vec<&Application> = model.catalog.iter().filter(|a| a.category == category).collect();
        apps.sort_by_key(|a| model.app_installed(a, &system));
        for app in apps {
            flow.append(&app_card(ctx, model, &system, app));
        }
        content.append(&flow);
    }
}

/// A query for the search thread: its number, the text, and the index.
type Query = (u64, String, Arc<PackageIndex>);

/// The search thread: it answers the newest query it has, skipping any that
/// came in while it was busy.
fn searcher() -> (async_channel::Sender<Query>, async_channel::Receiver<(u64, Vec<Package>)>) {
    let (queries, incoming) = async_channel::unbounded::<Query>();
    let (answer, answers) = async_channel::unbounded();
    std::thread::spawn(move || {
        while let Ok(mut query) = incoming.recv_blocking() {
            while let Ok(newer) = incoming.try_recv() {
                query = newer;
            }
            let (number, text, index) = query;
            let found: Vec<Package> = index.search(&text, RESULTS).into_iter().cloned().collect();
            if answer.send_blocking((number, found)).is_err() {
                break;
            }
        }
    });
    (queries, answers)
}

/// What was found: for which query, in which index, and the packages.
type Found = Rc<RefCell<Option<(String, usize, Vec<Package>)>>>;

fn index_id(index: &Arc<PackageIndex>) -> usize {
    Arc::as_ptr(index) as usize
}

pub fn build(ctx: &Ctx) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search packages"));
    search.add_css_class("discover-search");
    search.set_hexpand(true);
    let status = dim("");
    let search_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    search_box.set_margin_top(24);
    search_box.set_margin_start(18);
    search_box.set_margin_end(18);
    search_box.append(&search);
    search_box.append(&status);
    let search_clamp = adw::Clamp::builder().maximum_size(720).child(&search_box).build();
    outer.append(&search_clamp);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(36);
    content.set_margin_start(18);
    content.set_margin_end(18);
    outer.append(&page(&content));

    let query = Rc::new(RefCell::new(String::new()));
    let asked = Rc::new(Cell::new(0u64));
    let found: Found = Rc::new(RefCell::new(None));
    let (queries, answers) = searcher();

    // Send the current query to the search thread.
    let ask = {
        let (query, asked) = (query.clone(), asked.clone());
        Rc::new(move |index: &Arc<PackageIndex>| {
            asked.set(asked.get() + 1);
            let _ = queries.send_blocking((asked.get(), query.borrow().clone(), index.clone()));
        })
    };

    let render = {
        let (content, status, query, found) = (content.clone(), status.clone(), query.clone(), found.clone());
        Rc::new(move |ctx: &Ctx| {
            clear(&content);
            let model = ctx.model();
            let index = ctx.index();
            status.set_text(&match (&index, ctx.indexing(), ctx.index_stale()) {
                (Some(_), true, true) => {
                    "Searching the list from an earlier Nixpkgs while this version's is made".to_owned()
                }
                (Some(index), _, _) => format!("{} packages in Nixpkgs", group_digits(index.len())),
                (None, true, _) => "Getting to know Nixpkgs… about a minute, and only once for each version".to_owned(),
                (None, false, _) => String::new(),
            });
            let text = query.borrow().clone();
            if text.trim().is_empty() {
                if index.is_none() && !ctx.indexing() && model.nixpkgs.is_some() {
                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                    let label = wrapping("To search all of Nixpkgs, Yukimi first makes a list of its packages.", &[]);
                    label.set_hexpand(true);
                    let build = gtk::Button::with_label("Make the list");
                    build.add_css_class("pill");
                    let ctx2 = ctx.clone();
                    build.connect_clicked(move |_| ctx2.ensure_index(true));
                    row.append(&label);
                    row.append(&build);
                    content.append(&row);
                }
                catalog(ctx, &model, &content);
                return;
            }
            let Some(index) = index else {
                let waiting = if ctx.indexing() {
                    "The package list is still being made."
                } else {
                    "There is no package list yet."
                };
                content.append(&wrapping(waiting, &["dim-label"]));
                return;
            };
            let found = found.borrow();
            let current = found.as_ref().filter(|(q, of, _)| *q == text && *of == index_id(&index));
            let Some((_, _, results)) = current else {
                let spinner = adw::Spinner::new();
                spinner.set_size_request(32, 32);
                content.append(&spinner);
                return;
            };
            if results.is_empty() {
                let empty = adw::StatusPage::builder()
                    .icon_name("system-search-symbolic")
                    .title("Nothing by that name")
                    .description("Try a shorter word, or what the program does: \"image editor\", \"pdf\".")
                    .build();
                content.append(&empty);
                return;
            }
            let installed = Installed::of(ctx, &model);
            let list = gtk::ListBox::new();
            list.add_css_class("boxed-list");
            list.set_selection_mode(gtk::SelectionMode::None);
            for package in results {
                list.append(&result_row(ctx, &installed, package));
            }
            content.append(&list);
        })
    };

    {
        // Built again when something changes; a new index searches again.
        let (render, query, found, ask) = (render.clone(), query.clone(), found.clone(), ask.clone());
        ctx.on_refresh("discover", move |ctx| {
            let text = query.borrow().clone();
            if let Some(index) = ctx.index()
                && !text.trim().is_empty()
                && found.borrow().as_ref().is_none_or(|(q, of, _)| *q != text || *of != index_id(&index))
            {
                ask(&index);
            }
            render(ctx);
        });
    }
    {
        let (ctx, render, query, ask) = (ctx.clone(), render.clone(), query.clone(), ask.clone());
        search.connect_search_changed(move |entry| {
            let _busy = crate::stalls::doing("searching");
            *query.borrow_mut() = entry.text().to_string();
            if let Some(index) = ctx.index()
                && !entry.text().trim().is_empty()
            {
                ask(&index);
            }
            ctx.ensure_index(false);
            render(&ctx);
        });
    }
    {
        let (ctx, render, query) = (ctx.clone(), render.clone(), query.clone());
        glib::spawn_future_local(async move {
            while let Ok((number, results)) = answers.recv().await {
                if number != asked.get() {
                    continue;
                }
                let Some(index) = ctx.index() else { continue };
                *found.borrow_mut() = Some((query.borrow().clone(), index_id(&index), results));
                let _busy = crate::stalls::doing("showing results");
                render(&ctx);
            }
        });
    }
    outer
}

fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn digits_are_grouped() {
        assert_eq!(super::group_digits(123), "123");
        assert_eq!(super::group_digits(1234), "1,234");
        assert_eq!(super::group_digits(1234567), "1,234,567");
    }
}
