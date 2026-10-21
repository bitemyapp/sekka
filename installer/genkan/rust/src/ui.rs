// SPDX-License-Identifier: GPL-3.0-or-later
//! GTK owns widgets only. Disk discovery, validation, Wi-Fi snapshots, page
//! cache warming, authorization and installation all run on workers that
//! report through one bounded channel drained on the main loop.
mod applications;
mod pages;
#[cfg(debug_assertions)]
mod preview;
mod widgets;
mod zonemap;

use adw::prelude::*;
use calamares_nixos::{
    Desktop, Filesystem, Firmware, Hostname, InstallPlan, KEYBOARDS, LOCALES, RawRequest, Settings,
    Username,
    disk::{self, Disk, Layout},
    github::{self, GitIdentity, GithubUser},
    install::{Event, Summary},
    memory::{self, MemInfo},
    precache,
    session::{Session, Update},
    timezone::{self, Source},
    zonemap::Zones,
};
use gtk::{Align, CheckButton, Label, Orientation, glib};
use pages::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use widgets::{badge, clock, disk_icon, duration, icon, property, size};

/// Load the stylesheet and the icons shipped with the installer package.
pub fn load_style() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../data/style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    if let Some(paths) = option_env!("CALAMARES_ICON_PATH") {
        let theme = gtk::IconTheme::for_display(&display);
        for path in paths.split(':').filter(|p| !p.is_empty()) {
            theme.add_search_path(path);
        }
        // Consistent symbolic icons regardless of the live desktop's theme.
        gtk::Settings::for_display(&display).set_gtk_icon_theme_name(Some("Adwaita"));
    }
}

enum Message {
    Scanned(Result<(Vec<Disk>, Firmware), String>),
    Memory(Result<MemInfo, String>),
    Reviewed(u64, Result<(Box<Review>, Session), String>),
    Session(u64, Update),
    Zone(u64, Result<timezone::Detection, String>),
    Github(u64, Result<github::Profile, String>),
    Warm {
        generation: u64,
        read: u64,
        total: u64,
        done: bool,
    },
    Rebooted(Result<(), String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Welcome,
    Disk,
    Account,
    GitHub,
    Desktop,
    Applications,
    Location,
    Review,
    Install,
}
impl Step {
    const ALL: [Self; 9] = [
        Self::Welcome,
        Self::Disk,
        Self::Account,
        Self::GitHub,
        Self::Desktop,
        Self::Applications,
        Self::Location,
        Self::Review,
        Self::Install,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Disk => "Disk",
            Self::Account => "Account",
            Self::GitHub => "GitHub",
            Self::Desktop => "Desktop",
            Self::Applications => "Applications",
            Self::Location => "Location",
            Self::Review => "Review",
            Self::Install => "Install",
        }
    }
    fn heading(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Disk => "Choose a Disk",
            Self::Account => "Create Your Account",
            Self::GitHub => "GitHub, SSH and Git",
            Self::Desktop => "Choose Your Desktops",
            Self::Applications => "Choose Applications",
            Self::Location => "Time Zone and Language",
            Self::Review => "Review and Confirm",
            Self::Install => "Installing",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Welcome => "go-home-symbolic",
            Self::Disk => "drive-harddisk-symbolic",
            Self::Account => "avatar-default-symbolic",
            Self::GitHub => "network-server-symbolic",
            Self::Desktop => "video-display-symbolic",
            Self::Applications => "view-app-grid-symbolic",
            Self::Location => "mark-location-symbolic",
            Self::Review => "document-properties-symbolic",
            Self::Install => "folder-download-symbolic",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Disk => "disk",
            Self::Account => "account",
            Self::GitHub => "github",
            Self::Desktop => "desktop",
            Self::Applications => "applications",
            Self::Location => "location",
            Self::Review => "review",
            Self::Install => "install",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// Share of the overall progress bar for each step: copying dominates.
const STEP_WEIGHTS: [f64; 6] = [0.04, 0.08, 0.02, 0.72, 0.10, 0.04];

fn overall_fraction(step: u8, copy_fraction: f64) -> f64 {
    let step = usize::from(step.clamp(1, 6));
    let done: f64 = STEP_WEIGHTS[..step - 1].iter().sum();
    let current = if step == 4 {
        STEP_WEIGHTS[3] * copy_fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (done + current).clamp(0.0, 1.0)
}

/// Suggest a valid username from a full name: "Ada Lovelace" → "ada".
fn suggest_username(full_name: &str) -> String {
    let first = full_name.split_whitespace().next().unwrap_or("");
    let name: String = first
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .take(31)
        .collect();
    if name.starts_with(|c: char| c.is_ascii_lowercase()) && Username::parse(&name).is_ok() {
        name
    } else {
        String::new()
    }
}

const LOCALE_NAMES: [&str; 8] = [
    "English (United States)",
    "English (United Kingdom)",
    "Deutsch (Deutschland)",
    "Français (France)",
    "Español (España)",
    "Italiano (Italia)",
    "日本語 (日本)",
    "Português (Brasil)",
];
const KEYBOARD_NAMES: [&str; 8] = [
    "English (US)",
    "English (UK)",
    "German",
    "French",
    "Spanish",
    "Italian",
    "Japanese",
    "Portuguese (Brazil)",
];

/// Display copy of a reviewed plan. The plan itself moves to the helper.
pub(crate) struct Review {
    disk: disk::Identity,
    firmware: Firmware,
    filesystem: Filesystem,
    encrypt: bool,
    hostname: String,
    username: String,
    full_name: String,
    locale: String,
    timezone: String,
    keyboard: String,
    desktops: Vec<Desktop>,
    default_desktop: Desktop,
    applications: String,
    wifi: bool,
    wifi_profiles: usize,
    unfree: bool,
    swap: bool,
    tuning: bool,
    graphics: String,
    ssh_keys: String,
    ssh_server: String,
    git: String,
}
impl Review {
    fn of(plan: &InstallPlan) -> Self {
        let onboarding = plan.onboarding();
        let ssh_keys = match (onboarding.keys().len(), onboarding.github()) {
            (0, _) | (_, None) => "None".to_string(),
            (1, Some(user)) => format!("1 key from github.com/{}", user.as_str()),
            (n, Some(user)) => format!("{n} keys from github.com/{}", user.as_str()),
        };
        let ssh_server = match (onboarding.ssh_server(), onboarding.password_login()) {
            (false, _) => "Not installed",
            (true, false) => "OpenSSH, key-only login",
            (true, true) => "OpenSSH, password login",
        };
        Self {
            disk: plan.disk().clone(),
            firmware: plan.firmware(),
            filesystem: plan.filesystem(),
            encrypt: plan.encrypt(),
            hostname: plan.hostname().as_str().into(),
            username: plan.username().as_str().into(),
            full_name: plan.full_name().into(),
            locale: plan.locale().into(),
            timezone: plan.timezone().as_str().into(),
            keyboard: plan.keyboard().into(),
            desktops: plan.desktops().selected().to_vec(),
            default_desktop: plan.desktops().default(),
            applications: plan.applications().names(),
            wifi: plan.wifi().enabled(),
            wifi_profiles: plan.wifi().profile_count(),
            unfree: plan.allow_unfree(),
            swap: plan.swap(),
            tuning: plan.tuning(),
            graphics: plan.graphics().describe(plan.allow_unfree()),
            ssh_keys,
            ssh_server: ssh_server.into(),
            git: onboarding.git().map_or_else(
                || "Not set".into(),
                |id| format!("{} <{}>", id.name(), id.email()),
            ),
        }
    }
}

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    content: adw::ToolbarView,
    split: adw::NavigationSplitView,
    sidebar: Sidebar,
    stack: gtk::Stack,
    title: adw::WindowTitle,
    back: gtk::Button,
    next: gtk::Button,
    activity: Label,
    activity_spinner: adw::Spinner,
    disk: DiskPage,
    account: AccountPage,
    github: GithubPage,
    desktop: DesktopPage,
    apps: applications::ApplicationsPage,
    location: LocationPage,
    review: ReviewPage,
    install: InstallPage,
    send: SyncSender<Message>,
    step: Cell<Step>,
    reached: Cell<Step>,
    disks: RefCell<Vec<Disk>>,
    firmware: Cell<Firmware>,
    selected_disk: Cell<Option<usize>>,
    memory: Cell<Option<MemInfo>>,
    scanning: Cell<bool>,
    username_edited: Cell<bool>,
    filling_username: Cell<bool>,
    /// The last GitHub lookup that answered, for the username it was for.
    github_profile: RefCell<Option<github::Profile>>,
    github_epoch: Cell<u64>,
    github_pending: Cell<Option<u64>>,
    /// Next was pressed before the lookup: continue when it succeeds.
    github_advance: Cell<bool>,
    /// The Git fields follow the account and GitHub until edited.
    git_name_edited: Cell<bool>,
    git_email_edited: Cell<bool>,
    filling_git: Cell<bool>,
    default_desktops: RefCell<Vec<Desktop>>,
    default_choice: Cell<Option<Desktop>>,
    updating_default: Cell<bool>,
    zones: Option<Rc<Zones>>,
    zoneinfo: String,
    /// Index into `zones.places` of the chosen time zone.
    zone: Cell<Option<usize>>,
    /// Detections apply only while the user has not chosen a zone.
    zone_manual: Cell<bool>,
    /// The last detection found nothing and fell back to New York.
    zone_fallback: Cell<bool>,
    zone_updating: Cell<bool>,
    zone_epoch: Cell<u64>,
    zone_pending: Cell<Option<u64>>,
    reviewing: Cell<bool>,
    generation: Cell<u64>,
    session: RefCell<Option<Session>>,
    phrase: RefCell<String>,
    prepared: Cell<bool>,
    prep_failed: Cell<bool>,
    installing: Cell<bool>,
    started: Cell<Option<Instant>>,
    install_step: Cell<u8>,
    copy_fraction: Cell<f64>,
    completed: Cell<bool>,
    failed: Cell<bool>,
    total_seconds: Cell<Option<f64>>,
    log_lines: Cell<usize>,
    warm_generation: Cell<u64>,
    warm_cancel: RefCell<Option<Arc<AtomicBool>>>,
    warm_timer: RefCell<Option<glib::SourceId>>,
    closed: Cell<bool>,
}

pub fn build(app: &adw::Application) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }
    let (sidebar_page, sidebar) = build_sidebar();
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(180);
    stack.set_vexpand(true);
    let welcome = build_welcome();
    let (disk_widget, disk) = build_disk_page();
    let (account_widget, account) = build_account_page();
    let (github_widget, github) = build_github_page();
    let (desktop_widget, desktop) = build_desktop_page();
    // The application page is built before the Ui that owns its callback.
    let changed_selection: Rc<RefCell<Option<Callback>>> = Rc::new(RefCell::new(None));
    let apps = applications::build(
        &desktop.unfree,
        Rc::new({
            let changed = changed_selection.clone();
            move || {
                if let Some(changed) = changed.borrow().as_ref() {
                    changed();
                }
            }
        }),
    );
    let zoneinfo = zoneinfo_dir();
    let zones = match Zones::load(std::path::Path::new(&zoneinfo)) {
        Ok(zones) => Some(Rc::new(zones)),
        Err(error) => {
            eprintln!("time zone map unavailable: {error:#}");
            None
        }
    };
    let (location_widget, location) = build_location_page(zones.clone());
    let review = build_review_page();
    let install = build_install_page();
    stack.add_named(&welcome, Some(Step::Welcome.name()));
    stack.add_named(&disk_widget, Some(Step::Disk.name()));
    stack.add_named(&account_widget, Some(Step::Account.name()));
    stack.add_named(&github_widget, Some(Step::GitHub.name()));
    stack.add_named(&desktop_widget, Some(Step::Desktop.name()));
    stack.add_named(&apps.widget, Some(Step::Applications.name()));
    stack.add_named(&location_widget, Some(Step::Location.name()));
    stack.add_named(&review.page, Some(Step::Review.name()));
    stack.add_named(&install.stack, Some(Step::Install.name()));

    let title = adw::WindowTitle::new(Step::Welcome.heading(), "");
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    let back = gtk::Button::with_label("Back");
    back.add_css_class("pill");
    let next = gtk::Button::with_label("Get Started");
    next.add_css_class("pill");
    next.add_css_class("suggested-action");
    let activity_spinner = adw::Spinner::new();
    activity_spinner.set_visible(false);
    let activity = Label::new(None);
    activity.add_css_class("activity");
    activity.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let activity_box = gtk::Box::new(Orientation::Horizontal, 8);
    activity_box.set_hexpand(true);
    activity_box.set_halign(Align::Center);
    activity_box.append(&activity_spinner);
    activity_box.append(&activity);
    let bar = gtk::Box::new(Orientation::Horizontal, 12);
    bar.set_margin_top(10);
    bar.set_margin_bottom(10);
    bar.set_margin_start(16);
    bar.set_margin_end(16);
    bar.append(&back);
    bar.append(&activity_box);
    bar.append(&next);
    let content = adw::ToolbarView::new();
    content.add_top_bar(&header);
    content.set_content(Some(&stack));
    content.add_bottom_bar(&bar);
    content.set_bottom_bar_style(adw::ToolbarStyle::Raised);
    let split = adw::NavigationSplitView::new();
    split.set_sidebar(Some(&sidebar_page));
    split.set_content(Some(&adw::NavigationPage::new(&content, "Install Sekka")));
    split.set_min_sidebar_width(230.0);
    split.set_max_sidebar_width(280.0);
    split.set_show_content(true);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&split));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Install Sekka")
        .default_width(1100)
        .default_height(760)
        .content(&toasts)
        .build();
    window.set_size_request(360, 520);
    let breakpoint = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 760sp").expect("valid breakpoint"),
    );
    breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(breakpoint);

    let (send, receive) = mpsc::sync_channel::<Message>(128);
    let ui = Rc::new(Ui {
        window,
        toasts,
        content,
        split,
        sidebar,
        stack,
        title,
        back,
        next,
        activity,
        activity_spinner,
        disk,
        account,
        github,
        desktop,
        apps,
        location,
        review,
        install,
        send,
        step: Cell::new(Step::Welcome),
        reached: Cell::new(Step::Welcome),
        disks: RefCell::new(Vec::new()),
        firmware: Cell::new(Firmware::Bios),
        selected_disk: Cell::new(None),
        memory: Cell::new(None),
        scanning: Cell::new(false),
        username_edited: Cell::new(false),
        filling_username: Cell::new(false),
        github_profile: RefCell::new(None),
        github_epoch: Cell::new(0),
        github_pending: Cell::new(None),
        github_advance: Cell::new(false),
        git_name_edited: Cell::new(false),
        git_email_edited: Cell::new(false),
        filling_git: Cell::new(false),
        default_desktops: RefCell::new(Vec::new()),
        default_choice: Cell::new(Some(Desktop::Aizome)),
        updating_default: Cell::new(false),
        zones,
        zoneinfo,
        zone: Cell::new(None),
        zone_manual: Cell::new(false),
        zone_fallback: Cell::new(false),
        zone_updating: Cell::new(false),
        zone_epoch: Cell::new(0),
        zone_pending: Cell::new(None),
        reviewing: Cell::new(false),
        generation: Cell::new(0),
        session: RefCell::new(None),
        phrase: RefCell::new(String::new()),
        prepared: Cell::new(false),
        prep_failed: Cell::new(false),
        installing: Cell::new(false),
        started: Cell::new(None),
        install_step: Cell::new(0),
        copy_fraction: Cell::new(0.0),
        completed: Cell::new(false),
        failed: Cell::new(false),
        total_seconds: Cell::new(None),
        log_lines: Cell::new(0),
        warm_generation: Cell::new(0),
        warm_cancel: RefCell::new(None),
        warm_timer: RefCell::new(None),
        closed: Cell::new(false),
    });
    *changed_selection.borrow_mut() = Some(Rc::new({
        let ui = Rc::downgrade(&ui);
        move || {
            if let Some(ui) = ui.upgrade() {
                ui.schedule_warm();
            }
        }
    }));
    ui.connect();
    ui.sync_desktops();
    ui.update_layout();
    ui.update_ssh_server();
    ui.go(Step::Welcome);
    let preview = preview_requested();
    if !preview {
        ui.rescan();
        ui.read_memory();
        ui.detect_zone();
        ui.schedule_warm();
    }
    glib::timeout_add_seconds_local(15, {
        let ui = Rc::downgrade(&ui);
        move || match ui.upgrade() {
            Some(ui) if !ui.closed.get() => {
                if ui.step.get() == Step::Location {
                    ui.refresh_zone_clock();
                }
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        }
    });
    glib::timeout_add_local(Duration::from_millis(60), {
        let ui = ui.clone();
        move || {
            if ui.closed.get() {
                return glib::ControlFlow::Break;
            }
            for message in receive.try_iter().take(64) {
                ui.handle(message);
            }
            glib::ControlFlow::Continue
        }
    });
    glib::timeout_add_local(Duration::from_millis(500), {
        let ui = Rc::downgrade(&ui);
        move || {
            let Some(ui) = ui.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if let Some(started) = ui.started.get()
                && ui.installing.get()
            {
                ui.install
                    .elapsed
                    .set_text(&clock(started.elapsed().as_secs()));
            }
            glib::ControlFlow::Continue
        }
    });
    #[cfg(debug_assertions)]
    if preview {
        preview::resize(&ui.window);
    }
    ui.window.present();
    #[cfg(debug_assertions)]
    if preview {
        preview::apply(&ui);
    }
}

/// tzdata used for the installed system; outside the live ISO (previews),
/// the host's.
fn zoneinfo_dir() -> String {
    Settings::load()
        .map(|s| s.zoneinfo)
        .ok()
        .or_else(|| std::env::var("TZDIR").ok().filter(|d| !d.is_empty()))
        .unwrap_or_else(|| "/usr/share/zoneinfo".into())
}

fn zone_now(zoneinfo: &str, zone: &str) -> Option<glib::DateTime> {
    // An absolute path makes GLib read exactly this tzdata.
    let tz = glib::TimeZone::from_identifier(Some(&format!("{zoneinfo}/{zone}")))?;
    glib::DateTime::now(&tz).ok()
}

fn zone_offset_minutes(zoneinfo: &str, zone: &str) -> Option<i32> {
    i32::try_from(zone_now(zoneinfo, zone)?.utc_offset().as_seconds() / 60).ok()
}

fn utc_offset(seconds: i64) -> String {
    if seconds == 0 {
        return "UTC".into();
    }
    let sign = if seconds < 0 { '\u{2212}' } else { '+' };
    let minutes = seconds.abs() / 60;
    format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

fn detection_note(source: Source) -> String {
    match source {
        Source::Live => "From this live system's settings.".into(),
        Source::Internet(service) => {
            format!("Located through your internet connection by {service}; a VPN can mislead it.")
        }
        Source::Clock { offset_minutes } => format!(
            "Estimated from the computer's clock, which keeps local time ({}).",
            utc_offset(i64::from(offset_minutes) * 60)
        ),
        Source::Fallback => {
            "Your location couldn't be detected, so New York is selected. Click where you are."
                .into()
        }
    }
}

#[cfg(debug_assertions)]
fn preview_requested() -> bool {
    std::env::var_os("CALAMARES_UI_PREVIEW").is_some()
}
#[cfg(not(debug_assertions))]
fn preview_requested() -> bool {
    false
}

impl Ui {
    fn toast(&self, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_timeout(6);
        self.toasts.add_toast(toast);
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let with = move |f: fn(&Rc<Ui>)| {
            let weak = weak.clone();
            move || {
                if let Some(ui) = weak.upgrade() {
                    f(&ui);
                }
            }
        };
        self.next.connect_clicked({
            let f = with(Ui::forward);
            move |_| f()
        });
        self.back.connect_clicked({
            let f = with(Ui::backward);
            move |_| f()
        });
        self.sidebar.list.connect_row_activated({
            let weak = Rc::downgrade(self);
            move |_, row| {
                if let Some(ui) = weak.upgrade() {
                    ui.jump(Step::ALL[row.index().max(0) as usize]);
                }
            }
        });
        self.disk.rescan.connect_clicked({
            let f = with(Ui::rescan);
            move |_| f()
        });
        self.disk.filesystem.connect_active_notify({
            let f = with(Ui::update_layout);
            move |_| f()
        });
        self.disk.encrypt.connect_active_notify({
            let weak = Rc::downgrade(self);
            move |row| {
                if let Some(ui) = weak.upgrade() {
                    ui.disk.encryption_password.set_visible(row.is_active());
                    ui.disk.encryption_repeat.set_visible(row.is_active());
                    ui.disk.encryption_hint.set_visible(row.is_active());
                    if !row.is_active() {
                        ui.disk.encryption_password.set_text("");
                        ui.disk.encryption_repeat.set_text("");
                    }
                    ui.update_layout();
                }
            }
        });
        self.disk.swap.connect_active_notify({
            let f = with(Ui::update_layout);
            move |_| f()
        });
        for entry in [
            self.account.full_name.upcast_ref::<gtk::Editable>(),
            self.account.password.upcast_ref(),
            self.account.repeat.upcast_ref(),
            self.account.hostname.upcast_ref(),
        ] {
            entry.connect_changed({
                let f = with(Ui::validate_account_live);
                move |_| f()
            });
        }
        self.account.full_name.connect_changed({
            let weak = Rc::downgrade(self);
            move |entry| {
                if let Some(ui) = weak.upgrade()
                    && !ui.username_edited.get()
                {
                    ui.filling_username.set(true);
                    ui.account
                        .username
                        .set_text(&suggest_username(&entry.text()));
                    ui.filling_username.set(false);
                }
            }
        });
        self.account.username.connect_changed({
            let weak = Rc::downgrade(self);
            move |entry| {
                if let Some(ui) = weak.upgrade() {
                    if !ui.filling_username.get() {
                        ui.username_edited.set(!entry.text().is_empty());
                    }
                    ui.validate_account_live();
                }
            }
        });
        self.github.lookup.connect_clicked({
            let f = with(Ui::lookup_github);
            move |_| f()
        });
        self.github.username.connect_entry_activated({
            let f = with(Ui::lookup_github);
            move |_| f()
        });
        self.github.username.connect_changed({
            let f = with(Ui::github_username_changed);
            move |_| f()
        });
        for switch in [&self.github.authorize, &self.github.ssh_server] {
            switch.connect_active_notify({
                let f = with(Ui::update_ssh_server);
                move |_| f()
            });
        }
        // The Git name follows the full name until it is edited.
        self.account.full_name.connect_changed({
            let weak = Rc::downgrade(self);
            move |_| {
                if let Some(ui) = weak.upgrade()
                    && !ui.git_name_edited.get()
                {
                    ui.fill_git(Some(&ui.git_name_suggestion()), None);
                }
            }
        });
        for (entry, email) in [
            (&self.github.git_name, false),
            (&self.github.git_email, true),
        ] {
            entry.connect_changed({
                let weak = Rc::downgrade(self);
                move |entry| {
                    if let Some(ui) = weak.upgrade() {
                        if !ui.filling_git.get() {
                            ui.git_edited(email).set(!entry.text().is_empty());
                        }
                        ui.validate_git_live();
                    }
                }
            });
        }
        for card in &self.desktop.cards {
            card.connect_toggled({
                let weak = Rc::downgrade(self);
                move |_| {
                    if let Some(ui) = weak.upgrade() {
                        ui.sync_desktops();
                        ui.schedule_warm();
                    }
                }
            });
        }
        self.desktop.default.connect_selected_notify({
            let weak = Rc::downgrade(self);
            move |combo| {
                if let Some(ui) = weak.upgrade()
                    && !ui.updating_default.get()
                {
                    ui.default_choice.set(
                        ui.default_desktops
                            .borrow()
                            .get(combo.selected() as usize)
                            .copied(),
                    );
                }
            }
        });
        self.location.map.connect_picked({
            let weak = Rc::downgrade(self);
            move |index| {
                if let Some(ui) = weak.upgrade() {
                    ui.choose_zone(index, "Chosen on the map.");
                }
            }
        });
        self.location.zone_list.connect_selected_notify({
            let weak = Rc::downgrade(self);
            move |row| {
                if let Some(ui) = weak.upgrade()
                    && !ui.zone_updating.get()
                    && row.selected() != gtk::INVALID_LIST_POSITION
                {
                    ui.choose_zone(row.selected() as usize, "Chosen from the list.");
                }
            }
        });
        self.location.detect.connect_clicked({
            let weak = Rc::downgrade(self);
            move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.zone_manual.set(false);
                    ui.detect_zone();
                }
            }
        });
        self.location.internet.connect_active_notify({
            let weak = Rc::downgrade(self);
            move |_| {
                if let Some(ui) = weak.upgrade()
                    && !ui.zone_manual.get()
                {
                    ui.detect_zone();
                }
            }
        });
        self.review.erase.connect_changed({
            let f = with(Ui::update_nav);
            move |_| f()
        });
        self.review.consent.connect_toggled({
            let f = with(Ui::update_nav);
            move |_| f()
        });
        self.window.connect_close_request({
            let weak = Rc::downgrade(self);
            move |_| {
                let Some(ui) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if ui.installing.get() {
                    ui.toast("The installer cannot be closed while it is writing to the disk.");
                    return glib::Propagation::Stop;
                }
                ui.stop_warm();
                // Dropping an unconfirmed session withdraws its request.
                ui.session.borrow_mut().take();
                ui.closed.set(true);
                glib::Propagation::Proceed
            }
        });
    }

    // ---- Navigation -------------------------------------------------------

    fn go(self: &Rc<Self>, step: Step) {
        // The network may have come up since detection found nothing.
        if step == Step::Location
            && self.step.get() != step
            && self.zone_fallback.get()
            && !self.zone_manual.get()
            && self.zone_pending.get().is_none()
        {
            self.detect_zone();
        }
        self.step.set(step);
        if step > self.reached.get() {
            self.reached.set(step);
        }
        self.stack.set_visible_child_name(step.name());
        self.title.set_title(step.heading());
        self.title.set_subtitle(&if step == Step::Welcome {
            String::new()
        } else {
            format!("Step {} of {}", step.index(), Step::ALL.len() - 1)
        });
        self.split.set_show_content(true);
        self.update_nav();
    }

    fn locked(&self) -> bool {
        self.reviewing.get() || self.installing.get() || self.step.get() >= Step::Review
    }

    fn jump(self: &Rc<Self>, step: Step) {
        if !self.locked() && step <= self.reached.get() && step <= Step::Location {
            self.go(step);
        } else {
            self.update_nav();
        }
    }

    fn update_nav(self: &Rc<Self>) {
        let step = self.step.get();
        let locked = self.locked();
        let completed = self.completed.get();
        for (index, (row, badge)) in self.sidebar.rows.iter().enumerate() {
            let current = index == step.index() && !completed;
            let done = index < step.index() || completed;
            for (class, on) in [
                ("step-current", current),
                ("step-done", done && !current),
                ("step-future", !done && !current),
            ] {
                if on {
                    row.add_css_class(class);
                } else {
                    row.remove_css_class(class);
                }
            }
            badge.set_text(&if done && !current {
                "✓".to_string()
            } else {
                (index + 1).to_string()
            });
            row.set_sensitive(
                current
                    || (!locked
                        && index <= self.reached.get().index()
                        && index <= Step::Location.index()),
            );
        }
        self.sidebar
            .list
            .select_row(Some(&self.sidebar.rows[step.index()].0));
        let finished = self.completed.get() || (self.failed.get() && !self.installing.get());
        self.back
            .set_visible(step != Step::Welcome && step != Step::Install);
        self.back.set_sensitive(!self.reviewing.get());
        self.next.remove_css_class("suggested-action");
        self.next.remove_css_class("destructive-action");
        match step {
            Step::Welcome => {
                self.next.set_label("Get Started");
                self.next.add_css_class("suggested-action");
            }
            Step::Location => {
                self.next.set_label(if self.reviewing.get() {
                    "Checking…"
                } else {
                    "Review"
                });
                self.next.add_css_class("suggested-action");
            }
            Step::Review => {
                self.next.set_label("Erase Disk and Install");
                self.next.add_css_class("destructive-action");
            }
            Step::Install => self.next.set_label("Close Installer"),
            _ => {
                self.next.set_label("Next");
                self.next.add_css_class("suggested-action");
            }
        }
        self.next
            .set_visible(step != Step::Install || (finished && !self.completed.get()));
        self.next.set_sensitive(match step {
            Step::Review => self.confirmation_ready(),
            Step::Install => finished,
            _ => !self.reviewing.get(),
        });
        self.content.set_reveal_bottom_bars(!self.completed.get());
        if step == Step::Install && self.installing.get() && !self.failed.get() {
            self.activity_spinner.set_visible(false);
            self.activity.set_text(
                "Keep this computer on and connected to power until the installation finishes.",
            );
        } else if step == Step::Install {
            self.activity.set_text("");
        }
    }

    fn forward(self: &Rc<Self>) {
        let result = match self.step.get() {
            Step::Welcome => Ok(Step::Disk),
            Step::Disk => self.check_disk().map(|()| Step::Account),
            Step::Account => self.check_account().map(|()| Step::GitHub),
            Step::GitHub => {
                // Look the account up first, then continue on success.
                if self.github_pending.get().is_some() || self.github_needs_lookup() {
                    self.github_advance.set(true);
                    if self.github_pending.get().is_none() {
                        self.lookup_github();
                    }
                    return;
                }
                self.check_github().map(|()| Step::Desktop)
            }
            Step::Desktop => self.check_desktops().map(|()| Step::Applications),
            Step::Applications => Ok(Step::Location),
            Step::Location => {
                self.start_review();
                return;
            }
            Step::Review => {
                self.confirm_install();
                return;
            }
            Step::Install => {
                self.window.close();
                return;
            }
        };
        match result {
            Ok(step) => self.go(step),
            Err(message) => self.toast(&message),
        }
    }

    fn backward(self: &Rc<Self>) {
        match self.step.get() {
            Step::Welcome | Step::Install => {}
            Step::Review => {
                // Withdraw the unconfirmed request; nothing was written.
                self.session.borrow_mut().take();
                self.generation.set(self.generation.get() + 1);
                self.go(Step::Location);
                self.schedule_warm();
            }
            step => self.go(Step::ALL[step.index() - 1]),
        }
    }

    // ---- Disk -------------------------------------------------------------

    fn rescan(self: &Rc<Self>) {
        if self.scanning.replace(true) {
            return;
        }
        self.disk.spinner.set_visible(true);
        self.disk.rescan.set_sensitive(false);
        let send = self.send.clone();
        thread::spawn(move || {
            let result = disk::discover()
                .map(|d| (d, Firmware::current()))
                .map_err(|e| format!("{e:#}"));
            let _ = send.send(Message::Scanned(result));
        });
    }

    fn read_memory(&self) {
        let send = self.send.clone();
        thread::spawn(move || {
            let _ = send.send(Message::Memory(
                memory::read().map_err(|e| format!("{e:#}")),
            ));
        });
    }

    fn show_disks(self: &Rc<Self>, found: Vec<Disk>, firmware: Firmware) {
        for row in self.disk.rows.borrow_mut().drain(..) {
            self.disk.group.remove(&row);
        }
        self.selected_disk.set(None);
        self.firmware.set(firmware);
        let mut first: Option<CheckButton> = None;
        let mut rows = Vec::new();
        for (index, disk) in found.iter().enumerate() {
            let identity = &disk.identity;
            let model = if identity.model.is_empty() {
                "Unknown disk"
            } else {
                identity.model.as_str()
            };
            let row = adw::ActionRow::builder().title(model).build();
            row.set_use_markup(false);
            let image = icon(disk_icon(&identity.path));
            image.set_pixel_size(24);
            let serial = if identity.serial.is_empty() {
                String::new()
            } else {
                format!(" · serial {}", identity.serial)
            };
            match &disk.blocked {
                Some(reason) => {
                    row.set_subtitle(&format!(
                        "{} · {} — {reason}",
                        identity.path,
                        size(identity.bytes)
                    ));
                    row.add_prefix(&image);
                    row.add_suffix(&badge("Unavailable", "unavailable"));
                    row.set_sensitive(false);
                }
                None => {
                    row.set_subtitle(&format!(
                        "{} · {}{serial}",
                        identity.path,
                        size(identity.bytes)
                    ));
                    let check = CheckButton::new();
                    check.set_valign(Align::Center);
                    if let Some(first) = &first {
                        check.set_group(Some(first));
                    } else {
                        first = Some(check.clone());
                    }
                    check.connect_toggled({
                        let weak = Rc::downgrade(self);
                        move |check| {
                            if let Some(ui) = weak.upgrade()
                                && check.is_active()
                            {
                                ui.selected_disk.set(Some(index));
                                ui.update_layout();
                            }
                        }
                    });
                    // Prefixes are prepended: the radio ends up first.
                    row.add_prefix(&image);
                    row.add_prefix(&check);
                    row.set_activatable_widget(Some(&check));
                }
            }
            self.disk.group.add(&row);
            rows.push(row);
        }
        if found.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No disks found")
                .subtitle("Connect a disk, then rescan.")
                .build();
            self.disk.group.add(&row);
            rows.push(row);
        }
        *self.disk.rows.borrow_mut() = rows;
        *self.disks.borrow_mut() = found;
        self.update_layout();
    }

    fn selected_filesystem(&self) -> Filesystem {
        Filesystem::ALL
            .get(self.disk.filesystem.active() as usize)
            .copied()
            .unwrap_or_default()
    }

    fn swap_bytes(&self) -> Option<u64> {
        self.disk
            .swap
            .is_active()
            .then(|| self.memory.get().map(|m| memory::swap_bytes(m.total)))
            .flatten()
    }

    fn update_layout(self: &Rc<Self>) {
        let filesystem = self.selected_filesystem();
        self.disk
            .filesystem_row
            .set_subtitle(match filesystem {
                Filesystem::Ext4 => "General purpose and the most widely used Linux filesystem (default).",
                Filesystem::Btrfs => "Checksums and zstd compression on a single root volume. Snapshots are not configured.",
                Filesystem::Xfs => "High-performance journaling for large files and parallel I/O.",
            });
        self.disk.swap.set_subtitle(&match self.memory.get() {
            Some(info) => format!(
                "A {} swap partition matching installed memory. zswap compresses pages in RAM before they reach the disk, and hibernation can resume from it.",
                size(memory::swap_bytes(info.total))
            ),
            None => "Matches installed memory. zswap compresses pages in RAM before they reach the disk, and hibernation can resume from it.".into(),
        });
        let disks = self.disks.borrow();
        let Some(disk) = self.selected_disk.get().and_then(|i| disks.get(i)) else {
            self.disk
                .layout
                .clear("Select a disk to preview its new partitions.");
            return;
        };
        if self.disk.swap.is_active() && self.memory.get().is_none() {
            self.disk.layout.clear("Reading installed memory…");
            return;
        }
        match Layout::for_install(
            disk.identity.bytes,
            self.firmware.get(),
            self.swap_bytes(),
            self.disk.encrypt.is_active(),
        ) {
            Ok(layout) => self
                .disk
                .layout
                .show(&layout, self.firmware.get(), filesystem),
            Err(error) => self.disk.layout.clear(&format!("⚠ {error:#}")),
        }
    }

    fn check_disk(&self) -> Result<(), String> {
        if self.disk.encrypt.is_active() {
            calamares_nixos::encryption::validate_passphrase(&self.disk.encryption_password.text())
                .map_err(|e| e.to_string())?;
            if self.disk.encryption_password.text() != self.disk.encryption_repeat.text() {
                return Err("Disk passphrases do not match.".into());
            }
        }
        let disks = self.disks.borrow();
        let disk = self
            .selected_disk
            .get()
            .and_then(|i| disks.get(i))
            .ok_or("Select the disk to erase. No disk is selected automatically.")?;
        if let Some(reason) = &disk.blocked {
            return Err(reason.clone());
        }
        if self.disk.swap.is_active() && self.memory.get().is_none() {
            return Err("Still reading installed memory; try again in a moment.".into());
        }
        Layout::for_install(
            disk.identity.bytes,
            self.firmware.get(),
            self.swap_bytes(),
            self.disk.encrypt.is_active(),
        )
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
    }

    // ---- Account ----------------------------------------------------------

    fn account_errors(&self) -> Vec<(&adw::EntryRow, String)> {
        let a = &self.account;
        let mut errors = Vec::new();
        let full_name = a.full_name.text();
        if full_name.len() > 128
            || full_name.contains(':')
            || full_name.chars().any(char::is_control)
        {
            errors.push((&a.full_name, "The full name cannot contain a colon or control characters, and is limited to 128 bytes.".to_string()));
        }
        if let Err(e) = Username::parse(&a.username.text()) {
            errors.push((&a.username, e.to_string()));
        }
        let password = a.password.text();
        if password.chars().count() < 12
            || password.len() > 1024
            || password.chars().any(char::is_control)
        {
            errors.push((
                a.password.upcast_ref(),
                "The password needs at least 12 characters and no control characters.".into(),
            ));
        } else if password != a.repeat.text() {
            errors.push((a.repeat.upcast_ref(), "The passwords do not match.".into()));
        }
        if let Err(e) = Hostname::parse(&a.hostname.text()) {
            errors.push((&a.hostname, e.to_string()));
        }
        errors
    }

    /// Mark fields that already contain invalid text, without nagging about
    /// fields the user has not reached yet.
    fn validate_account_live(self: &Rc<Self>) {
        let a = &self.account;
        let errors = self.account_errors();
        for row in [
            &a.full_name,
            &a.username,
            a.password.upcast_ref::<adw::EntryRow>(),
            a.repeat.upcast_ref(),
            &a.hostname,
        ] {
            let bad = !row.text().is_empty() && errors.iter().any(|(r, _)| *r == row);
            if bad {
                row.add_css_class("error");
            } else {
                row.remove_css_class("error");
            }
        }
        let length = a.password.text().chars().count();
        a.password_hint.set_text(&if length == 0 {
            "Use at least 12 characters. Passwords are typed with this live session's keyboard layout.".to_string()
        } else if length < 12 {
            format!("{} more characters needed. Passwords are typed with this live session's keyboard layout.", 12 - length)
        } else if a.password.text() != a.repeat.text() {
            "Long enough. Now confirm it in the second field.".to_string()
        } else {
            "✓ Password set. Passwords are typed with this live session's keyboard layout.".to_string()
        });
    }

    fn check_account(&self) -> Result<(), String> {
        match self.account_errors().into_iter().next() {
            Some((row, message)) => {
                row.add_css_class("error");
                row.grab_focus();
                Err(message)
            }
            None => Ok(()),
        }
    }

    // ---- GitHub, SSH and Git ----------------------------------------------

    fn git_edited(&self, email: bool) -> &Cell<bool> {
        if email {
            &self.git_email_edited
        } else {
            &self.git_name_edited
        }
    }

    /// The GitHub username as typed; empty skips GitHub.
    fn github_text(&self) -> String {
        self.github.username.text().trim().to_string()
    }

    /// The last lookup answered for the username now in the field.
    fn github_profile_current(&self) -> bool {
        let text = self.github_text();
        !text.is_empty()
            && self
                .github_profile
                .borrow()
                .as_ref()
                .is_some_and(|p| p.user.as_str().eq_ignore_ascii_case(&text))
    }

    fn github_needs_lookup(&self) -> bool {
        let text = self.github_text();
        !text.is_empty() && GithubUser::parse(&text).is_ok() && !self.github_profile_current()
    }

    fn lookup_github(self: &Rc<Self>) {
        let text = self.github_text();
        if text.is_empty() || self.github_profile_current() {
            return;
        }
        let user = match GithubUser::parse(&text) {
            Ok(user) => user,
            Err(error) => {
                self.github_advance.set(false);
                self.github.username.add_css_class("error");
                self.github.status.set_text(&error.to_string());
                return;
            }
        };
        self.github_epoch
            .set(self.github_epoch.get().wrapping_add(1));
        let epoch = self.github_epoch.get();
        self.github_pending.set(Some(epoch));
        self.github.spinner.set_visible(true);
        self.github.lookup.set_sensitive(false);
        self.github
            .status
            .set_text(&format!("Looking up {} on GitHub…", user.as_str()));
        let send = self.send.clone();
        thread::spawn(move || {
            let result = github::fetch(&user).map_err(|e| format!("{e:#}"));
            let _ = send.send(Message::Github(epoch, result));
        });
    }

    /// Keys found for one username never apply to another.
    fn github_username_changed(self: &Rc<Self>) {
        let text = self.github_text();
        let error = (!text.is_empty())
            .then(|| GithubUser::parse(&text).err())
            .flatten();
        if error.is_some() {
            self.github.username.add_css_class("error");
        } else {
            self.github.username.remove_css_class("error");
        }
        if self.github_pending.take().is_some() {
            self.github_epoch
                .set(self.github_epoch.get().wrapping_add(1));
            self.github.spinner.set_visible(false);
        }
        self.github_advance.set(false);
        let current = self
            .github_profile_current()
            .then(|| self.github_profile.borrow().clone())
            .flatten();
        match &current {
            Some(profile) => self.show_github_profile(profile),
            None => {
                self.show_github_keys(&[]);
                self.github.status.set_text(&match &error {
                    Some(error) => error.to_string(),
                    None if text.is_empty() => GITHUB_HINT.into(),
                    None => "Press Enter or Look Up to fetch this account's public keys.".into(),
                });
            }
        }
        self.github
            .lookup
            .set_sensitive(!text.is_empty() && error.is_none() && current.is_none());
        self.update_ssh_server();
    }

    fn show_github_keys(&self, keys: &[github::AuthorizedKey]) {
        let page = &self.github;
        for row in page.key_rows.borrow_mut().drain(..) {
            page.keys.remove(&row);
        }
        for key in keys {
            let row = property(key.kind());
            row.set_subtitle(&key.fingerprint());
            row.add_prefix(&icon("dialog-password-symbolic"));
            page.keys.add(&row);
            page.key_rows.borrow_mut().push(row);
        }
        page.keys.set_visible(!keys.is_empty());
    }

    fn show_github_profile(&self, profile: &github::Profile) {
        self.show_github_keys(&profile.keys);
        let login = profile.user.as_str();
        let mut status = match profile.keys.len() {
            0 => format!("github.com/{login} has no public SSH keys."),
            1 => format!("Found 1 public SSH key on github.com/{login}."),
            n => format!("Found {n} public SSH keys on github.com/{login}."),
        };
        if profile.skipped > 0 {
            status.push_str(&format!(
                " {} of a type OpenSSH does not accept left out.",
                profile.skipped
            ));
        }
        self.github.status.set_text(&status);
    }

    /// The account's full name, or the GitHub profile's when it has none: a
    /// name typed into this installer wins over a profile's display name.
    fn git_name_suggestion(&self) -> String {
        let typed = self.account.full_name.text().trim().to_string();
        if !typed.is_empty() || !self.github_profile_current() {
            return typed;
        }
        self.github_profile
            .borrow()
            .as_ref()
            .and_then(|p| p.name.clone())
            .unwrap_or_default()
    }

    /// Fill the Git fields that the user has not edited.
    fn fill_git(&self, name: Option<&str>, email: Option<&str>) {
        self.filling_git.set(true);
        if let Some(name) = name.filter(|_| !self.git_name_edited.get()) {
            self.github.git_name.set_text(name);
        }
        if let Some(email) = email.filter(|_| !self.git_email_edited.get()) {
            self.github.git_email.set_text(email);
        }
        self.filling_git.set(false);
    }

    /// Git's name and address for the request. A name filled in from the
    /// account alone, with no address, means no identity.
    fn git_fields(&self) -> (String, String) {
        let name = self.github.git_name.text().trim().to_string();
        let email = self.github.git_email.text().trim().to_string();
        if email.is_empty() && !self.git_name_edited.get() {
            (String::new(), String::new())
        } else {
            (name, email)
        }
    }

    fn validate_git_live(&self) {
        let page = &self.github;
        for (row, bad) in [
            (
                &page.git_name,
                GitIdentity::parse(&page.git_name.text(), "git@example.com").is_err(),
            ),
            (
                &page.git_email,
                GitIdentity::parse("Git", &page.git_email.text()).is_err(),
            ),
        ] {
            if bad && !row.text().trim().is_empty() {
                row.add_css_class("error");
            } else {
                row.remove_css_class("error");
            }
        }
    }

    /// The GitHub account and the keys to authorize, as the request lists them.
    fn github_request(&self) -> (String, Vec<String>) {
        if !self.github_profile_current() {
            return (String::new(), vec![]);
        }
        let profile = self.github_profile.borrow();
        let Some(profile) = profile.as_ref() else {
            return (String::new(), vec![]);
        };
        let keys = if self.github.authorize.is_active() {
            profile.keys.iter().map(|k| k.text()).collect()
        } else {
            vec![]
        };
        (profile.user.as_str().into(), keys)
    }

    fn update_ssh_server(self: &Rc<Self>) {
        self.github
            .ssh_server
            .set_subtitle(if self.github_request().1.is_empty() {
                "OpenSSH, open in the firewall. With no authorized keys it accepts your account's password."
            } else {
                "OpenSSH, open in the firewall. Only your GitHub keys can log in: passwords are refused."
            });
    }

    fn check_github(&self) -> Result<(), String> {
        let page = &self.github;
        let text = self.github_text();
        if !text.is_empty() {
            if let Err(error) = GithubUser::parse(&text) {
                page.username.add_css_class("error");
                page.username.grab_focus();
                return Err(error.to_string());
            }
            if !self.github_profile_current() {
                page.username.grab_focus();
                return Err(
                    "Look up the GitHub account, or clear its username to skip GitHub.".into(),
                );
            }
        }
        let (name, email) = self.git_fields();
        if let Err(error) = GitIdentity::parse(&name, &email) {
            let row = if name.is_empty() || GitIdentity::parse(&name, "git@example.com").is_err() {
                &page.git_name
            } else {
                &page.git_email
            };
            row.add_css_class("error");
            row.grab_focus();
            return Err(error.to_string());
        }
        Ok(())
    }

    // ---- Desktops ---------------------------------------------------------

    fn selected_desktops(&self) -> Vec<Desktop> {
        Desktop::ALL
            .iter()
            .zip(&self.desktop.cards)
            .filter_map(|(desktop, card)| card.is_active().then_some(*desktop))
            .collect()
    }

    fn sync_desktops(&self) {
        let selected = self.selected_desktops();
        self.updating_default.set(true);
        let labels: Vec<&str> = selected.iter().map(|d| d.label()).collect();
        self.desktop
            .default_model
            .splice(0, self.desktop.default_model.n_items(), &labels);
        let index = self
            .default_choice
            .get()
            .and_then(|choice| selected.iter().position(|d| *d == choice))
            .unwrap_or(0);
        self.desktop.default.set_selected(if selected.is_empty() {
            gtk::INVALID_LIST_POSITION
        } else {
            index as u32
        });
        self.desktop.default.set_sensitive(selected.len() > 1);
        if self
            .default_choice
            .get()
            .is_none_or(|c| !selected.contains(&c))
        {
            self.default_choice.set(selected.first().copied());
        }
        *self.default_desktops.borrow_mut() = selected.clone();
        self.updating_default.set(false);
    }

    fn check_desktops(&self) -> Result<(), String> {
        let selected = self.selected_desktops();
        if selected.is_empty() {
            return Err("Select at least one desktop environment.".into());
        }
        Ok(())
    }

    // ---- Time zone --------------------------------------------------------

    fn detect_zone(self: &Rc<Self>) {
        self.zone_epoch.set(self.zone_epoch.get().wrapping_add(1));
        let generation = self.zone_epoch.get();
        self.zone_pending.set(Some(generation));
        let internet = self.location.internet.is_active();
        self.location.spinner.set_visible(true);
        self.location.detect.set_sensitive(false);
        self.location.status.set_text("Finding your location…");
        let send = self.send.clone();
        let zoneinfo = self.zoneinfo.clone();
        thread::spawn(move || {
            let offset_now = |zone: &str| zone_offset_minutes(&zoneinfo, zone);
            let result = timezone::detect(std::path::Path::new(&zoneinfo), internet, &offset_now)
                .map_err(|e| format!("{e:#}"));
            let _ = send.send(Message::Zone(generation, result));
        });
    }

    /// A zone the user picked; later detections no longer replace it.
    fn choose_zone(self: &Rc<Self>, index: usize, note: &str) {
        self.zone_manual.set(true);
        self.zone_fallback.set(false);
        self.zone_epoch.set(self.zone_epoch.get().wrapping_add(1));
        if self.zone_pending.take().is_some() {
            self.location.spinner.set_visible(false);
            self.location.detect.set_sensitive(true);
        }
        self.show_zone(index, note);
    }

    fn show_zone(&self, index: usize, note: &str) {
        if self.zones.as_ref().is_none_or(|z| index >= z.places.len()) {
            return;
        }
        self.zone.set(Some(index));
        self.location.map.select(Some(index));
        self.zone_updating.set(true);
        self.location.zone_list.set_selected(index as u32);
        self.zone_updating.set(false);
        self.refresh_zone_clock();
        self.location.status.set_text(note);
    }

    fn selected_place(&self) -> Option<&calamares_nixos::zonemap::Place> {
        self.zones.as_ref()?.places.get(self.zone.get()?)
    }

    /// The chosen zone's name, local time and UTC offset, which change with
    /// the clock and daylight saving time.
    fn refresh_zone_clock(&self) {
        let Some(place) = self.selected_place() else {
            return;
        };
        let location = &self.location;
        let now = zone_now(&self.zoneinfo, &place.zone);
        let daylight = now
            .as_ref()
            .is_some_and(glib::DateTime::is_daylight_savings);
        location
            .zone_name
            .set_text(place.name(daylight).unwrap_or(&place.zone));
        location.zone_place.set_text(&if place.position.is_some() {
            place.label()
        } else {
            "The same everywhere, with no daylight saving time".into()
        });
        let mut detail = Vec::new();
        if let Some(now) = &now {
            location.zone_time.set_text(
                &now.format("%-l:%M %p")
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            );
            let abbreviation = now.timezone_abbreviation();
            if abbreviation.starts_with(|c: char| c.is_ascii_alphabetic()) && abbreviation != "UTC"
            {
                detail.push(abbreviation.to_string());
            }
            detail.push(utc_offset(now.utc_offset().as_seconds()));
        } else {
            location.zone_time.set_text("");
        }
        detail.push(place.zone.clone());
        location.zone_detail.set_text(&detail.join(" · "));
    }

    // ---- Speculative caching ----------------------------------------------

    fn stop_warm(&self) {
        if let Some(timer) = self.warm_timer.borrow_mut().take() {
            timer.remove();
        }
        if let Some(cancel) = self.warm_cancel.borrow_mut().take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.warm_generation.set(self.warm_generation.get() + 1);
        self.activity_spinner.set_visible(false);
        self.activity.set_text("");
    }

    /// Warm the page cache with the media's reference closures for the
    /// current selection, a couple of seconds after it stops changing.
    fn schedule_warm(self: &Rc<Self>) {
        if self.step.get() >= Step::Review || self.reviewing.get() {
            return;
        }
        self.stop_warm();
        let weak = Rc::downgrade(self);
        let timer = glib::timeout_add_local_once(Duration::from_secs(2), move || {
            if let Some(ui) = weak.upgrade() {
                ui.warm_timer.borrow_mut().take();
                ui.start_warm();
            }
        });
        *self.warm_timer.borrow_mut() = Some(timer);
    }

    fn start_warm(self: &Rc<Self>) {
        let desktops = self.selected_desktops();
        let applications = self.apps.selected_ids();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.warm_cancel.borrow_mut() = Some(cancel.clone());
        let generation = self.warm_generation.get();
        let send = self.send.clone();
        thread::spawn(move || {
            let lists = precache::reference_lists(&desktops, &applications);
            let outcome = if lists.is_empty() {
                Ok(precache::Outcome::default())
            } else {
                precache::read_lists(&lists).and_then(|paths| {
                    precache::warm(&paths, &cancel, |read, total| {
                        // Progress is advisory: never block the warmer on a full channel.
                        let _ = send.try_send(Message::Warm {
                            generation,
                            read,
                            total,
                            done: false,
                        });
                    })
                })
            };
            let (read, total) = outcome.map(|o| (o.bytes, o.total)).unwrap_or((0, 0));
            let _ = send.send(Message::Warm {
                generation,
                read,
                total,
                done: true,
            });
        });
    }

    // ---- Review -----------------------------------------------------------

    fn start_review(self: &Rc<Self>) {
        if self.reviewing.get() {
            return;
        }
        let checks = [
            self.check_disk(),
            self.check_account(),
            self.check_github(),
            self.check_desktops(),
        ];
        let targets = [Step::Disk, Step::Account, Step::GitHub, Step::Desktop];
        for (check, step) in checks.into_iter().zip(targets) {
            if let Err(message) = check {
                self.go(step);
                self.toast(&message);
                return;
            }
        }
        let Some(timezone) = self.selected_place().map(|p| p.zone.clone()) else {
            self.go(Step::Location);
            self.toast("Choose your time zone on the map.");
            return;
        };
        let Some(disk) = self
            .selected_disk
            .get()
            .and_then(|i| self.disks.borrow().get(i).cloned())
        else {
            self.go(Step::Disk);
            return;
        };
        let (github_user, ssh_keys) = self.github_request();
        let (git_name, git_email) = self.git_fields();
        let request = RawRequest {
            confirmation: String::new(),
            disk: disk.identity,
            firmware: self.firmware.get(),
            filesystem: self.selected_filesystem(),
            encrypt: self.disk.encrypt.is_active(),
            encryption_password: if self.disk.encrypt.is_active() {
                self.disk.encryption_password.text().into()
            } else {
                String::new()
            },
            hostname: self.account.hostname.text().into(),
            username: self.account.username.text().into(),
            full_name: self.account.full_name.text().into(),
            password: self.account.password.text().into(),
            locale: LOCALES
                .get(self.location.locale.selected() as usize)
                .unwrap_or(&LOCALES[0])
                .to_string(),
            timezone,
            keyboard: KEYBOARDS
                .get(self.location.keyboard.selected() as usize)
                .unwrap_or(&KEYBOARDS[0])
                .to_string(),
            desktops: self.selected_desktops(),
            default_desktop: self.default_choice.get().unwrap_or(Desktop::Aizome),
            applications: self.apps.selected_ids(),
            copy_wifi: self.desktop.wifi.is_active(),
            wifi_profiles: vec![],
            allow_unfree: self.desktop.unfree.is_active(),
            swap: self.disk.swap.is_active(),
            tuning: self.disk.tuning.is_active(),
            graphics: calamares_nixos::graphics::Graphics::detect(),
            github_user,
            ssh_keys,
            ssh_server: self.github.ssh_server.is_active(),
            git_name,
            git_email,
        };
        self.stop_warm();
        self.reviewing.set(true);
        self.generation.set(self.generation.get() + 1);
        let generation = self.generation.get();
        self.activity_spinner.set_visible(true);
        self.activity.set_text("Checking your settings…");
        self.update_nav();
        let send = self.send.clone();
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<(Box<Review>, Session)> {
                let plan = request.parse(&Settings::load()?)?;
                disk::revalidate(plan.disk())?;
                let plan = plan.snapshot_wifi()?;
                let review = Box::new(Review::of(&plan));
                let events = send.clone();
                let session = Session::start(plan, move |update| {
                    let _ = events.send(Message::Session(generation, update));
                })?;
                Ok((review, session))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = send.send(Message::Reviewed(generation, result));
        });
    }

    fn show_review(self: &Rc<Self>, review: &Review) {
        let r = &self.review;
        let disk = &review.disk;
        let model = if disk.model.is_empty() {
            "the selected disk"
        } else {
            disk.model.as_str()
        };
        r.erase_title
            .set_text(&format!("Everything on {model} will be erased"));
        r.erase_detail.set_text(&format!(
            "{} · {}{} — all partitions and data on this disk will be destroyed.",
            disk.path,
            size(disk.bytes),
            if disk.serial.is_empty() {
                String::new()
            } else {
                format!(" · serial {}", disk.serial)
            }
        ));
        r.disk
            .set_subtitle(&format!("{model} · {} · {}", disk.path, size(disk.bytes)));
        let swap = review
            .swap
            .then(|| self.memory.get().map(|m| memory::swap_bytes(m.total)))
            .flatten();
        r.partitions.set_subtitle(&match Layout::for_install(
            disk.bytes,
            review.firmware,
            swap,
            review.encrypt,
        ) {
            Ok(layout) => {
                let mut parts = vec![
                    if review.firmware == Firmware::Uefi {
                        "EFI 1.0 GiB".to_string()
                    } else {
                        "BIOS boot 2 MiB".to_string()
                    },
                    format!("NixOS {}", size(layout.root_bytes())),
                ];
                if layout.bios_boot.is_some() {
                    parts.push("Unencrypted /boot 1.0 GiB".into());
                }
                if let Some(bytes) = layout.swap_bytes() {
                    parts.push(format!("swap {} with zswap", size(bytes)));
                }
                parts.join(" · ")
            }
            Err(error) => format!("{error:#}"),
        });
        r.filesystem.set_subtitle(&format!(
            "{}{}{}",
            review.filesystem.name(),
            if review.filesystem == Filesystem::Btrfs {
                " with zstd compression"
            } else {
                ""
            },
            if review.encrypt && review.swap {
                " · LUKS2: root and swap encrypted"
            } else if review.encrypt {
                " · LUKS2: root encrypted"
            } else {
                " · Unencrypted"
            }
        ));
        r.boot.set_subtitle(match review.firmware {
            Firmware::Uefi => "UEFI with systemd-boot",
            Firmware::Bios => "Legacy BIOS with GRUB",
        });
        r.desktops.set_subtitle(
            &review
                .desktops
                .iter()
                .map(|d| d.label())
                .collect::<Vec<_>>()
                .join(", "),
        );
        r.session.set_subtitle(review.default_desktop.label());
        r.applications
            .set_subtitle(if review.applications.is_empty() {
                "None"
            } else {
                &review.applications
            });
        r.tuning.set_subtitle(if review.tuning {
            "CachyOS-inspired defaults"
        } else {
            "NixOS defaults"
        });
        r.unfree.set_subtitle(if review.unfree {
            "Allowed"
        } else {
            "Not allowed (redistributable firmware is still included)"
        });
        r.graphics.set_subtitle(&review.graphics);
        r.wifi.set_subtitle(&if review.wifi {
            match review.wifi_profiles {
                1 => "1 saved network will be copied".to_string(),
                n => format!("{n} saved networks will be copied"),
            }
        } else {
            "Not copied".to_string()
        });
        r.computer.set_subtitle(&review.hostname);
        r.user.set_subtitle(&if review.full_name.is_empty() {
            review.username.clone()
        } else {
            format!("{} ({})", review.full_name, review.username)
        });
        r.ssh_keys.set_subtitle(&review.ssh_keys);
        r.ssh_server.set_subtitle(&review.ssh_server);
        r.git.set_subtitle(&review.git);
        r.locale.set_subtitle(&review.locale);
        r.zone.set_subtitle(
            &self
                .zones
                .as_ref()
                .and_then(|z| z.places.get(z.find(&review.timezone)?))
                .filter(|p| p.position.is_some())
                .and_then(|p| Some(format!("{} — {} ({})", p.name(false)?, p.label(), p.zone)))
                .unwrap_or_else(|| review.timezone.clone()),
        );
        r.keyboard.set_subtitle(&review.keyboard);
        let phrase = format!("ERASE {}", disk.path);
        r.erase.set_title(&format!("Type {phrase} to confirm"));
        *self.phrase.borrow_mut() = phrase;
        r.erase.set_text("");
        r.consent.set_active(false);
        self.prepared.set(false);
        self.prep_failed.set(false);
        self.set_preparation(
            PrepState::Working,
            "Waiting for authorization…",
            "The installation helper repeats every safety check before any disk write.",
        );
        r.prep_bar.set_visible(false);
        r.prep_failure.set_visible(false);
    }

    fn confirmation_ready(&self) -> bool {
        self.review.consent.is_active()
            && !self.prep_failed.get()
            && self.session.borrow().is_some()
            && !self.phrase.borrow().is_empty()
            && self.review.erase.text() == *self.phrase.borrow()
    }

    fn set_preparation(&self, state: PrepState, title: &str, subtitle: &str) {
        let r = &self.review;
        r.prep_row.set_title(title);
        r.prep_row.set_subtitle(subtitle);
        r.prep_spinner.set_visible(state == PrepState::Working);
        r.prep_icon.set_visible(state != PrepState::Working);
        r.prep_icon.set_icon_name(Some(match state {
            PrepState::Failed => "dialog-error-symbolic",
            _ => "object-select-symbolic",
        }));
        for class in ["success-icon", "error-icon"] {
            r.prep_icon.remove_css_class(class);
        }
        r.prep_icon.add_css_class(if state == PrepState::Failed {
            "error-icon"
        } else {
            "success-icon"
        });
    }

    fn prepared_text(summary: &Summary) -> (String, String) {
        if summary.deferred {
            return (
                "Ready to install".into(),
                format!(
                    "The selection needs more downloads than fit in memory, so the system will be built on the new disk after formatting. This takes longer. Prepared in {}.",
                    duration(summary.seconds)
                ),
            );
        }
        let cached = if summary.cache_limited {
            format!(
                "{} of it cached in memory (limited to keep memory free)",
                size(summary.cached_bytes)
            )
        } else {
            "fully cached in memory".to_string()
        };
        (
            "Ready to install".into(),
            format!(
                "The {} system is built and {cached}. Prepared in {}.",
                size(summary.closure_bytes),
                duration(summary.seconds)
            ),
        )
    }

    // ---- Installation -----------------------------------------------------

    fn confirm_install(self: &Rc<Self>) {
        if !self.confirmation_ready() {
            self.toast("Type the exact phrase and confirm that the disk will be erased.");
            return;
        }
        let phrase = self.phrase.borrow().clone();
        match self.session.borrow().as_ref() {
            Some(session) => session.confirm(&phrase),
            None => {
                self.toast(
                    "The installation helper is no longer running. Go back and review again.",
                );
                return;
            }
        }
        self.account.password.set_text("");
        self.disk.encryption_password.set_text("");
        self.disk.encryption_repeat.set_text("");
        self.account.repeat.set_text("");
        self.installing.set(true);
        self.started.set(Some(Instant::now()));
        self.install_step.set(0);
        self.copy_fraction.set(0.0);
        self.install.elapsed.set_text("0:00");
        self.install.bar.set_fraction(0.0);
        self.install.message.set_text(if self.prepared.get() {
            "Starting the installation…"
        } else {
            "Finishing preparation; the installation starts as soon as it is ready."
        });
        self.install.stack.set_visible_child_name("progress");
        self.go(Step::Install);
    }

    fn set_install_step(&self, step: u8) {
        for (index, (_, state, _)) in self.install.steps.iter().enumerate() {
            let number = index as u8 + 1;
            state.set_visible_child_name(if number < step {
                "done"
            } else if number == step {
                "current"
            } else {
                "pending"
            });
        }
        self.install_step.set(step);
        self.install
            .bar
            .set_fraction(overall_fraction(step, self.copy_fraction.get()));
    }

    fn append_log(&self, line: &str) {
        let buffer = self.install.log.buffer();
        let mut end = buffer.end_iter();
        buffer.insert(&mut end, line);
        buffer.insert(&mut end, "\n");
        let lines = self.log_lines.get() + 1;
        // Bounded display work, even when Nix reports thousands of paths.
        if lines > 400 {
            let mut start = buffer.start_iter();
            if let Some(mut cut) = buffer.iter_at_line(100) {
                buffer.delete(&mut start, &mut cut);
            }
            self.log_lines.set(lines - 100);
        } else {
            self.log_lines.set(lines);
        }
        buffer.place_cursor(&buffer.end_iter());
        self.install.log.scroll_mark_onscreen(&buffer.get_insert());
    }

    fn show_failure(self: &Rc<Self>, message: &str) {
        let message: String = message.chars().take(8000).collect();
        if self.installing.get() || self.step.get() == Step::Install {
            self.failed.set(true);
            if let Some(started) = self.started.take() {
                self.install
                    .elapsed
                    .set_text(&clock(started.elapsed().as_secs()));
            }
            self.install.heading.set_text("Installation Failed");
            self.install.failure_text.buffer().set_text(&message);
            self.install.failure.set_visible(true);
            self.install
                .message
                .set_text("Installation failed. See the details below.");
            if let Some((_, state, _)) = self
                .install
                .steps
                .get(usize::from(self.install_step.get().max(1)) - 1)
            {
                state.set_visible_child_name("failed");
            }
        } else {
            self.prep_failed.set(true);
            self.set_preparation(
                PrepState::Failed,
                "Preparation failed",
                "No disk was written. Go back to change your choices, or copy the details below.",
            );
            self.review.prep_bar.set_visible(false);
            self.review.prep_failure_text.buffer().set_text(&message);
            self.review.prep_failure.set_visible(true);
            self.review.prep_failure.set_expanded(true);
        }
        self.update_nav();
    }

    fn show_done(self: &Rc<Self>) {
        self.installing.set(false);
        self.completed.set(true);
        self.set_install_step(7);
        self.install.bar.set_fraction(1.0);
        let seconds = self.total_seconds.get().or_else(|| {
            self.started
                .get()
                .map(|started| started.elapsed().as_secs_f64())
        });
        let time = seconds
            .map(|s| format!("Installed in <b>{}</b>. ", duration(s)))
            .unwrap_or_default();
        self.title.set_title("Installation Complete");
        self.title.set_subtitle("");
        self.install.done.set_description(Some(&format!(
            "{time}Remove the installation media, then restart into your new system."
        )));
        let buttons = gtk::Box::new(Orientation::Horizontal, 12);
        buttons.set_halign(Align::Center);
        let restart = gtk::Button::with_label("Restart Now");
        restart.add_css_class("pill");
        restart.add_css_class("suggested-action");
        let close = gtk::Button::with_label("Close");
        close.add_css_class("pill");
        buttons.append(&close);
        buttons.append(&restart);
        restart.connect_clicked({
            let send = self.send.clone();
            move |button| {
                button.set_sensitive(false);
                let send = send.clone();
                thread::spawn(move || {
                    let result = std::process::Command::new("systemctl")
                        .arg("reboot")
                        .status()
                        .map_err(|e| e.to_string())
                        .and_then(|status| {
                            if status.success() {
                                Ok(())
                            } else {
                                Err(format!("systemctl reboot failed ({status})"))
                            }
                        });
                    let _ = send.send(Message::Rebooted(result));
                });
            }
        });
        close.connect_clicked({
            let window = self.window.clone();
            move |_| window.close()
        });
        self.install.done.set_child(Some(&buttons));
        self.install.stack.set_visible_child_name("done");
        self.update_nav();
    }

    // ---- Messages ---------------------------------------------------------

    fn handle(self: &Rc<Self>, message: Message) {
        match message {
            Message::Scanned(result) => {
                self.scanning.set(false);
                self.disk.spinner.set_visible(false);
                self.disk.rescan.set_sensitive(true);
                match result {
                    Ok((found, firmware)) => self.show_disks(found, firmware),
                    Err(error) => self.toast(&format!("Could not list disks: {error}")),
                }
            }
            Message::Memory(result) => match result {
                Ok(info) => {
                    self.memory.set(Some(info));
                    self.update_layout();
                }
                Err(error) => {
                    self.disk.swap.set_active(false);
                    self.toast(&format!("Could not read installed memory: {error}"));
                }
            },
            Message::Reviewed(generation, result) => {
                if generation != self.generation.get() {
                    return; // Dropping a stale session cancels it.
                }
                self.reviewing.set(false);
                self.activity_spinner.set_visible(false);
                self.activity.set_text("");
                match result {
                    Ok((review, session)) => {
                        *self.session.borrow_mut() = Some(session);
                        self.show_review(&review);
                        self.go(Step::Review);
                    }
                    Err(error) => {
                        self.update_nav();
                        self.toast(&error);
                        self.schedule_warm();
                    }
                }
            }
            Message::Session(generation, update) => {
                if generation == self.generation.get() {
                    self.session_update(update);
                }
            }
            Message::Zone(generation, result) => {
                // Superseded by a manual choice or a newer detection.
                if self.zone_pending.get() != Some(generation) {
                    return;
                }
                self.zone_pending.set(None);
                self.location.spinner.set_visible(false);
                self.location.detect.set_sensitive(true);
                match result {
                    Ok(found) => {
                        self.zone_fallback.set(found.source == Source::Fallback);
                        match self.zones.as_ref().and_then(|z| {
                            z.find_equivalent(
                                found.zone.as_str(),
                                std::path::Path::new(&self.zoneinfo),
                            )
                        }) {
                            Some(index) => self.show_zone(index, &detection_note(found.source)),
                            None => self.location.status.set_text(&format!(
                                "Detected {}, which is not on the map. Choose your time zone.",
                                found.zone.as_str()
                            )),
                        }
                    }
                    Err(error) => self.location.status.set_text(&error),
                }
            }
            Message::Github(epoch, result) => {
                // Superseded by an edit or a newer lookup.
                if self.github_pending.get() != Some(epoch) {
                    return;
                }
                self.github_pending.set(None);
                self.github.spinner.set_visible(false);
                let advance = self.github_advance.replace(false);
                match result {
                    Ok(profile) => {
                        let email = profile.email.clone();
                        self.show_github_profile(&profile);
                        *self.github_profile.borrow_mut() = Some(profile);
                        self.fill_git(Some(&self.git_name_suggestion()), email.as_deref());
                        self.update_ssh_server();
                        if advance && self.step.get() == Step::GitHub {
                            self.forward();
                        }
                    }
                    Err(error) => {
                        self.github.lookup.set_sensitive(true);
                        self.github.status.set_text(&error);
                        if advance {
                            self.toast(&error);
                        }
                    }
                }
            }
            Message::Warm {
                generation,
                read,
                total,
                done,
            } => {
                if generation != self.warm_generation.get() {
                    return;
                }
                self.activity_spinner.set_visible(!done && total > 0);
                self.activity.set_text(&if total == 0 {
                    String::new()
                } else if done {
                    format!("{} of selected software cached in memory", size(read))
                } else {
                    format!(
                        "Caching selected software in memory · {} of {}",
                        size(read),
                        size(total)
                    )
                });
            }
            Message::Rebooted(result) => {
                if let Err(error) = result {
                    self.toast(&format!("Could not restart: {error}"));
                }
            }
        }
    }

    fn session_update(self: &Rc<Self>, update: Update) {
        let installing = self.installing.get();
        match update {
            Update::Event(Event::Preparing { message }) => {
                if installing && self.install_step.get() == 0 {
                    self.install
                        .message
                        .set_text(&format!("Finishing preparation: {message}"));
                } else if !self.prep_failed.get() {
                    self.set_preparation(PrepState::Working, &message, "");
                }
                self.append_log(&message);
            }
            Update::Event(Event::Log { line }) => {
                if !installing && !self.prepared.get() && !self.prep_failed.get() {
                    self.review.prep_row.set_subtitle(&line);
                }
                self.append_log(&line);
            }
            Update::Event(Event::Timing { stage, seconds }) => {
                if stage == "install" {
                    self.total_seconds.set(Some(seconds));
                }
                self.append_log(&format!("{stage}: {seconds:.1} s"));
            }
            Update::Event(Event::Caching { read, total }) => {
                if !installing {
                    self.review.prep_bar.set_visible(total > 0);
                    if total > 0 {
                        self.review
                            .prep_bar
                            .set_fraction(read as f64 / total as f64);
                    }
                    self.review.prep_row.set_subtitle(&format!(
                        "{} of {} read into memory",
                        size(read),
                        size(total)
                    ));
                }
            }
            Update::Event(Event::Prepared { summary }) => {
                self.prepared.set(true);
                let (title, subtitle) = Self::prepared_text(&summary);
                self.set_preparation(PrepState::Done, &title, &subtitle);
                self.review.prep_bar.set_visible(false);
                if installing && self.install_step.get() == 0 {
                    self.install
                        .message
                        .set_text("Preparation complete. Starting the installation…");
                }
                self.append_log(&subtitle);
                self.update_nav();
            }
            Update::Event(Event::Progress { step, message }) => {
                self.install.message.set_text(&message);
                self.set_install_step(step);
                self.append_log(&message);
            }
            Update::Event(Event::Copying { bytes, total }) => {
                if total > 0 {
                    self.copy_fraction.set(bytes as f64 / total as f64);
                    self.install.bar.set_fraction(overall_fraction(
                        self.install_step.get().max(4),
                        self.copy_fraction.get(),
                    ));
                    if let Some((row, _, _)) = self.install.steps.get(3) {
                        row.set_subtitle(&format!("{} of {}", size(bytes.min(total)), size(total)));
                    }
                }
            }
            Update::Event(Event::Complete) => self.show_done(),
            Update::Event(Event::Cancelled) => {
                if !installing {
                    self.set_preparation(
                        PrepState::Failed,
                        "Preparation cancelled",
                        "Nothing was written. Go back and review again to continue.",
                    );
                }
            }
            Update::Event(Event::Failed { message }) => self.show_failure(&message),
            Update::Finished(result) => {
                self.session.borrow_mut().take();
                let was_installing = self.installing.replace(false);
                if self.completed.get() {
                    return;
                }
                if was_installing || self.step.get() == Step::Install {
                    if !self.failed.get() {
                        self.show_failure(&result.err().unwrap_or_else(|| {
                            "The installation helper exited without confirming completion.".into()
                        }));
                    }
                } else if !self.prep_failed.get() {
                    self.show_failure(&result.err().unwrap_or_else(|| {
                        "The installation helper stopped before the installation was confirmed."
                            .into()
                    }));
                }
                self.update_nav();
            }
        }
    }
}

type Callback = Rc<dyn Fn()>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrepState {
    Working,
    Done,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usernames_are_suggested_only_when_valid() {
        assert_eq!(suggest_username("Ada Lovelace"), "ada");
        assert_eq!(suggest_username("  Grace  Hopper "), "grace");
        assert_eq!(suggest_username("Ünal"), "nal");
        assert_eq!(suggest_username("2pac"), "");
        assert_eq!(suggest_username("root"), "");
        assert_eq!(suggest_username(""), "");
    }
    #[test]
    fn progress_is_monotonic_and_copy_dominates() {
        let mut last = 0.0;
        for step in 1..=6 {
            for copy in [0.0, 0.5, 1.0] {
                let fraction = overall_fraction(step, if step == 4 { copy } else { 0.0 });
                assert!(fraction >= last);
                last = fraction;
            }
        }
        assert!((STEP_WEIGHTS.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(overall_fraction(4, 1.0) - overall_fraction(4, 0.0) > 0.5);
    }
    #[test]
    fn steps_have_unique_names() {
        let names: std::collections::BTreeSet<_> = Step::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names.len(), Step::ALL.len());
        assert_eq!(LOCALE_NAMES.len(), LOCALES.len());
        assert_eq!(KEYBOARD_NAMES.len(), KEYBOARDS.len());
    }
}
