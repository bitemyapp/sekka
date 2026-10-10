// SPDX-License-Identifier: MIT OR Apache-2.0
//! The window: a sidebar of places and the page for each.
//!
//! Nothing slow happens on the interface thread. Reading the system, the
//! store, the package index and the sources' newest versions all happen in
//! other threads, each filling in its part of the window when it is done.
//! A page is built again only when something it shows has changed, and
//! only once it is the page being looked at.
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gio, glib};
use serde::{Deserialize, Serialize};
use yukimi_system::index::PackageIndex;
use yukimi_system::setup::Kind;
use yukimi_system::updates::Release;

use crate::model::{Model, Store};
use crate::ops::Operation;

mod discover;
mod history;
mod installed;
mod job;
mod overview;
mod storage;
mod updates;
pub mod widgets;

/// The places in the sidebar: (name, title, icon).
const PLACES: [(&str, &str, &str); 6] = [
    ("overview", "Overview", "weather-snow-symbolic"),
    ("installed", "Installed", "view-grid-symbolic"),
    ("discover", "Discover", "system-search-symbolic"),
    ("updates", "Updates", "software-update-available-symbolic"),
    ("history", "History", "document-open-recent-symbolic"),
    ("storage", "Storage", "drive-harddisk-symbolic"),
];

/// What pages share: the window, the loaded model and package index, and
/// a way to run operations and refresh afterwards.
#[derive(Clone)]
pub struct Ctx(Rc<Inner>);

/// A page's way of building itself again from the current model.
struct Page {
    name: &'static str,
    refresh: Box<dyn Fn(&Ctx)>,
    /// Something it shows has changed since it was last built.
    stale: Cell<bool>,
}

pub struct Inner {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: adw::ViewStack,
    sidebar: gtk::ListBox,
    model: RefCell<Arc<Model>>,
    index: RefCell<Option<Arc<PackageIndex>>>,
    /// The index loaded is for another version of Nixpkgs than the
    /// system's, while the right one is made.
    index_stale: Cell<bool>,
    indexing: Cell<bool>,
    updates: RefCell<Option<Rc<UpdateCheck>>>,
    checking: Cell<bool>,
    /// Reading the configuration and generations.
    loading: Cell<bool>,
    /// Reading the store.
    reading_store: Cell<bool>,
    /// Which reading is the latest, so an older one finishing late is
    /// ignored.
    epoch: Cell<u64>,
    pages: RefCell<Vec<Page>>,
    refresh_queued: Cell<bool>,
}

/// What checking for updates found: the newest version of each source (or
/// why it couldn't be asked), and when it looked.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub when: i64,
    /// The configuration it was for: a flake's directory, or `channels`.
    pub of: String,
    pub newest: BTreeMap<String, Release>,
    pub failures: BTreeMap<String, String>,
    /// Where each source came from when it was checked, so that an answer
    /// about a source since pointed elsewhere isn't taken for the new one.
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
}

/// Where each of the system's sources comes from: what a flake's input asks
/// for, or a channel's name.
pub fn sources(model: &Model) -> BTreeMap<String, String> {
    match model.setup.kind {
        Kind::Flake => model
            .inputs
            .iter()
            .filter(|i| i.follows.is_none())
            .filter_map(|i| Some((i.name.clone(), i.original.as_ref().or(i.locked.as_ref())?.describe())))
            .collect(),
        Kind::Channels => model.channels.iter().map(|c| (c.name.clone(), c.release.clone())).collect(),
    }
}

/// Where the last check for updates is kept.
fn check_file() -> Option<PathBuf> {
    Some(crate::cache_dir()?.join("updates.json"))
}

fn load_check(of: &str) -> Option<UpdateCheck> {
    let check: UpdateCheck = serde_json::from_str(&std::fs::read_to_string(check_file()?).ok()?).ok()?;
    (check.of == of).then_some(check)
}

/// What the last check is about: the flake's directory, or `channels`.
fn check_subject(model: &Model) -> String {
    match model.setup.kind {
        Kind::Flake => model.setup.flake(),
        Kind::Channels => "channels".to_owned(),
    }
}

/// Ask every source for its newest version, keeping the answers.
fn run_check(model: &Model) -> Result<UpdateCheck, String> {
    let answers = match model.setup.kind {
        Kind::Flake => {
            let scratch = crate::cache_dir().ok_or("There is no cache directory")?.join("checks");
            let _ = std::fs::remove_dir_all(&scratch);
            std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
            let answers = yukimi_system::updates::check_flake(&model.setup.flake(), &model.inputs, &scratch);
            let _ = std::fs::remove_dir_all(&scratch);
            answers
        }
        Kind::Channels => yukimi_system::updates::check_channels(&model.channels),
    };
    let mut check = UpdateCheck {
        when: yukimi_system::now(),
        of: check_subject(model),
        sources: sources(model),
        ..UpdateCheck::default()
    };
    for (name, answer) in answers {
        match answer {
            Ok(release) => {
                check.newest.insert(name, release);
            }
            Err(e) => {
                check.failures.insert(name, e);
            }
        }
    }
    // Nothing answered (offline, say): keep the last check.
    if check.newest.is_empty() && !check.failures.is_empty() {
        return Err(check.failures.into_values().next().unwrap_or_default());
    }
    if let Some(file) = check_file() {
        let _ = std::fs::create_dir_all(file.parent().unwrap_or(Path::new("/")));
        let _ = std::fs::write(&file, serde_json::to_string_pretty(&check).unwrap_or_default());
    }
    Ok(check)
}

impl Ctx {
    pub fn model(&self) -> Arc<Model> {
        self.0.model.borrow().clone()
    }

    pub fn window(&self) -> &adw::ApplicationWindow {
        &self.0.window
    }

    pub fn index(&self) -> Option<Arc<PackageIndex>> {
        self.0.index.borrow().clone()
    }

    /// Whether the index loaded is for an older Nixpkgs than the system's.
    pub fn index_stale(&self) -> bool {
        self.0.index_stale.get()
    }

    pub fn indexing(&self) -> bool {
        self.0.indexing.get()
    }

    /// The last check for updates, if there has been one.
    pub fn update_check(&self) -> Option<Rc<UpdateCheck>> {
        self.0.updates.borrow().clone()
    }

    pub fn checking(&self) -> bool {
        self.0.checking.get()
    }

    pub fn loading(&self) -> bool {
        self.0.loading.get()
    }

    /// Check for updates when the last check is older than a few minutes (or
    /// there hasn't been one), as when the Updates page is opened: asking
    /// takes a second, and a commit pushed a moment ago should show.
    pub fn check_if_stale(&self) {
        let model = self.model();
        let stale = self.update_check().is_none_or(|c| yukimi_system::now() - c.when > 10 * 60);
        if stale && !self.loading() && (!model.inputs.is_empty() || !model.channels.is_empty()) {
            self.check_updates();
        }
    }

    /// Look for newer versions of the system's sources, in the background.
    pub fn check_updates(&self) {
        if self.0.checking.replace(true) {
            return;
        }
        self.changed();
        let (ctx, model) = (self.clone(), self.model());
        glib::spawn_future_local(async move {
            let checked = gio::spawn_blocking(move || run_check(&model))
                .await
                .unwrap_or_else(|_| Err("Checking for updates stopped unexpectedly".to_owned()));
            ctx.0.checking.set(false);
            match checked {
                Ok(check) => *ctx.0.updates.borrow_mut() = Some(Rc::new(check)),
                Err(e) => ctx.toast(&format!("Couldn't check for updates: {e}")),
            }
            ctx.changed();
        });
    }

    /// Build `refresh` into the page named `name` now, and again whenever
    /// something changes while it is shown (or once it is shown again).
    pub fn on_refresh(&self, name: &'static str, refresh: impl Fn(&Ctx) + 'static) {
        self.0.pages.borrow_mut().push(Page { name, refresh: Box::new(refresh), stale: Cell::new(true) });
        self.queue_refresh();
    }

    /// Something pages show has changed: the page in view is built again
    /// soon (once, however many changes come together), the others when
    /// they are next shown.
    pub fn changed(&self) {
        for page in self.0.pages.borrow().iter() {
            page.stale.set(true);
        }
        self.queue_refresh();
    }

    fn queue_refresh(&self) {
        if self.0.refresh_queued.replace(true) {
            return;
        }
        let ctx = self.clone();
        glib::idle_add_local_once(move || {
            ctx.0.refresh_queued.set(false);
            ctx.refresh_visible();
        });
    }

    fn refresh_visible(&self) {
        let visible = self.0.stack.visible_child_name();
        let pages = self.0.pages.borrow();
        for page in pages.iter().filter(|p| Some(p.name) == visible.as_deref() && p.stale.get()) {
            page.stale.set(false);
            let _busy = crate::stalls::doing(page.name);
            (page.refresh)(self);
        }
    }

    pub fn toast(&self, text: &str) {
        self.0.toasts.add_toast(adw::Toast::new(text));
    }

    /// Show a place.
    pub fn show(&self, place: &str) {
        self.0.stack.set_visible_child_name(place);
        if let Some(i) = PLACES.iter().position(|(name, _, _)| *name == place)
            && let Some(row) = self.0.sidebar.row_at_index(i as i32)
        {
            self.0.sidebar.select_row(Some(&row));
        }
    }

    /// Read everything again, away from the interface: the configuration
    /// and generations first, then the store. What was shown stays until
    /// its new version is read.
    pub fn reload(&self) {
        let epoch = self.0.epoch.get() + 1;
        self.0.epoch.set(epoch);
        self.0.loading.set(true);
        self.changed();
        let ctx = self.clone();
        glib::spawn_future_local(async move {
            let mut model = gio::spawn_blocking(Model::quick).await.unwrap_or_default();
            if ctx.0.epoch.get() != epoch {
                return;
            }
            model.store = ctx.model().store.clone();
            let flake_nixpkgs =
                (model.nixpkgs.is_none() && model.setup.kind == Kind::Flake).then(|| model.setup.flake());
            let generations = model.generations.clone();
            if ctx.0.updates.borrow().is_none() {
                *ctx.0.updates.borrow_mut() = load_check(&check_subject(&model)).map(Rc::new);
            }
            *ctx.0.model.borrow_mut() = Arc::new(model);
            ctx.0.loading.set(false);
            ctx.0.reading_store.set(true);
            ctx.changed();
            ctx.ensure_index(false);

            // A flake whose facts don't say which Nixpkgs it uses: ask Nix.
            if let Some(flake) = flake_nixpkgs {
                let ctx = ctx.clone();
                glib::spawn_future_local(async move {
                    let found = gio::spawn_blocking(move || yukimi_system::nix::flake_nixpkgs(&flake)).await;
                    if ctx.0.epoch.get() != epoch {
                        return;
                    }
                    if let Ok(Ok(path)) = found {
                        let model = Model { nixpkgs: Some(path), ..(*ctx.model()).clone() };
                        *ctx.0.model.borrow_mut() = Arc::new(model);
                        ctx.changed();
                        ctx.ensure_index(false);
                    }
                });
            }

            let (store, problems) = gio::spawn_blocking(move || Store::load(&generations))
                .await
                .unwrap_or_else(|_| (Store::default(), vec!["The store could not be read".to_owned()]));
            if ctx.0.epoch.get() != epoch {
                return;
            }
            let model = ctx.model().with_store(store, problems);
            *ctx.0.model.borrow_mut() = Arc::new(model);
            ctx.0.reading_store.set(false);
            ctx.changed();
        });
    }

    /// Run an operation in a progress dialog, then read everything again.
    pub fn run(&self, operation: Operation) {
        job::run(self, operation);
    }

    /// Ask before an operation that changes the whole system.
    pub fn confirm(&self, heading: &str, body: &str, action: &str, operation: Operation) {
        let dialog = adw::AlertDialog::new(Some(heading), Some(body));
        dialog.add_responses(&[("cancel", "Cancel"), ("go", action)]);
        dialog.set_response_appearance("go", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("go"));
        dialog.set_close_response("cancel");
        let ctx = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "go" {
                ctx.run(operation.clone());
            }
        });
        dialog.present(Some(self.window()));
    }

    /// Load the package index for the system's Nixpkgs, or make it (`build`
    /// makes it again even when there is one). While it is made, the index
    /// of an earlier Nixpkgs serves, if there is one. Made only once the
    /// Discover page has been shown, since it takes a minute and a few
    /// gigabytes of memory.
    pub fn ensure_index(&self, build: bool) {
        let wanted = self.0.stack.visible_child_name().as_deref() == Some("discover") || self.index().is_some();
        if self.0.indexing.get() || !wanted || (self.index().is_some() && !self.index_stale() && !build) {
            return;
        }
        let Some(nixpkgs) = self.model().nixpkgs.clone() else {
            return;
        };
        let Some(cache) = crate::cache_dir() else {
            return;
        };
        self.0.indexing.set(true);
        self.changed();
        let ctx = self.clone();
        glib::spawn_future_local(async move {
            let (load_cache, load_nixpkgs) = (cache.clone(), nixpkgs.clone());
            // What is cached comes first: this Nixpkgs's index, or an older one
            // to use while this one is made.
            let cached = if build {
                None
            } else {
                gio::spawn_blocking(move || yukimi_system::index::load_cached(&load_cache, &load_nixpkgs))
                    .await
                    .ok()
                    .flatten()
            };
            let current = match cached {
                Some((index, current)) => {
                    *ctx.0.index.borrow_mut() = Some(Arc::new(index));
                    ctx.0.index_stale.set(!current);
                    ctx.changed();
                    current
                }
                None => false,
            };
            if !current {
                let made = gio::spawn_blocking(move || yukimi_system::index::make(&cache, &nixpkgs))
                    .await
                    .unwrap_or_else(|_| Err("The package list could not be made".to_owned()));
                match made {
                    Ok(index) => {
                        *ctx.0.index.borrow_mut() = Some(Arc::new(index));
                        ctx.0.index_stale.set(false);
                    }
                    Err(e) => ctx.toast(&e),
                }
            }
            ctx.0.indexing.set(false);
            ctx.changed();
        });
    }
}

/// The sidebar's brand: the name, its meaning, and a snowflake.
fn brand() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("brand");
    let mark = gtk::Image::from_icon_name("weather-snow-symbolic");
    mark.set_pixel_size(28);
    mark.add_css_class("brand-mark");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let title = gtk::Label::new(Some("Yukimi"));
    title.add_css_class("brand-title");
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some("雪見 · snow-viewing for NixOS"));
    subtitle.add_css_class("brand-subtitle");
    subtitle.set_xalign(0.0);
    text.append(&title);
    text.append(&subtitle);
    row.append(&mark);
    row.append(&text);
    row
}

pub fn build_window(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Yukimi")
        .default_width(1180)
        .default_height(800)
        .build();
    let stack = adw::ViewStack::new();
    // Sized by the page in view alone: otherwise every page, even one not
    // shown, is measured whenever any of them changes.
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));
    let sidebar = gtk::ListBox::new();
    sidebar.add_css_class("navigation-sidebar");
    let ctx = Ctx(Rc::new(Inner {
        window: window.clone(),
        toasts: toasts.clone(),
        stack: stack.clone(),
        sidebar: sidebar.clone(),
        model: RefCell::new(Arc::new(Model::default())),
        index: RefCell::new(None),
        index_stale: Cell::new(false),
        indexing: Cell::new(false),
        updates: RefCell::new(None),
        checking: Cell::new(false),
        loading: Cell::new(true),
        reading_store: Cell::new(true),
        epoch: Cell::new(0),
        pages: RefCell::new(Vec::new()),
        refresh_queued: Cell::new(false),
    }));

    let pages: [(&str, gtk::Widget); 6] = [
        ("overview", overview::build(&ctx).upcast()),
        ("installed", installed::build(&ctx).upcast()),
        ("discover", discover::build(&ctx).upcast()),
        ("updates", updates::build(&ctx).upcast()),
        ("history", history::build(&ctx).upcast()),
        ("storage", storage::build(&ctx).upcast()),
    ];
    for ((name, title, icon), (_, page)) in PLACES.iter().zip(pages) {
        stack.add_titled_with_icon(&page, Some(name), title, icon);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_margin_top(8);
        row.set_margin_bottom(8);
        row.set_margin_start(6);
        row.append(&gtk::Image::from_icon_name(icon));
        let label = gtk::Label::new(Some(title));
        label.set_xalign(0.0);
        row.append(&label);
        sidebar.append(&row);
    }

    let content_title = adw::WindowTitle::new("Overview", "");
    let content_page = adw::NavigationPage::builder().title("Overview").build();
    {
        let stack = stack.clone();
        let content_title = content_title.clone();
        let content_page = content_page.clone();
        sidebar.connect_row_selected(move |_, row| {
            if let Some(row) = row
                && let Some((name, title, _)) = PLACES.get(row.index() as usize)
            {
                stack.set_visible_child_name(name);
                content_title.set_title(title);
                content_page.set_title(title);
            }
        });
    }
    {
        // A page that changed while hidden is built when it is shown.
        let ctx = ctx.clone();
        stack.connect_visible_child_name_notify(move |stack| {
            ctx.queue_refresh();
            ctx.ensure_index(false);
            if stack.visible_child_name().as_deref() == Some("updates") {
                ctx.check_if_stale();
            }
        });
    }
    sidebar.select_row(sidebar.row_at_index(0).as_ref());

    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_title_widget(Some(&brand()));
    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&sidebar_header);
    let sidebar_scroll =
        gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&sidebar).build();
    sidebar_view.set_content(Some(&sidebar_scroll));

    let content_header = adw::HeaderBar::new();
    content_header.set_title_widget(Some(&content_title));
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Read everything again"));
    {
        let ctx = ctx.clone();
        refresh.connect_clicked(move |_| ctx.reload());
    }
    content_header.pack_end(&refresh);
    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&content_header);
    content_view.set_content(Some(&toasts));
    content_page.set_child(Some(&content_view));

    let split = adw::NavigationSplitView::new();
    split.set_sidebar(Some(&adw::NavigationPage::new(&sidebar_view, "Yukimi")));
    split.set_content(Some(&content_page));
    split.set_min_sidebar_width(230.0);
    window.set_content(Some(&split));
    window.present();
    ctx.reload();
}
