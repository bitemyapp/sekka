// SPDX-License-Identifier: GPL-3.0-or-later
//! Page layouts. Behaviour lives in the parent module; these functions only
//! construct widgets and return handles to the ones it updates.
use super::{
    KEYBOARD_NAMES, LOCALE_NAMES, Step,
    widgets::{LayoutBar, caption, clamp, desktop_card, icon, label, property, scrolled},
    zonemap::ZoneMap,
};
use adw::prelude::*;
use calamares_nixos::{
    Desktop, Filesystem, KEYBOARDS, LOCALES,
    zonemap::{Place, Zones},
};
use gtk::{Align, CheckButton, Label, Orientation};
use std::{cell::RefCell, rc::Rc};

/// Installation steps reported as `Event::Progress { step: 1..=6 }`.
pub(super) const INSTALL_STEPS: [&str; 6] = [
    "Partition the disk",
    "Format and mount the filesystems",
    "Write configuration and account settings",
    "Copy the prepared system",
    "Install the bootloader",
    "Flush writes and unmount",
];
pub(super) struct Sidebar {
    pub(super) list: gtk::ListBox,
    pub(super) rows: Vec<(gtk::ListBoxRow, Label)>,
}

pub(super) struct DiskPage {
    pub(super) group: adw::PreferencesGroup,
    pub(super) rows: RefCell<Vec<adw::ActionRow>>,
    pub(super) rescan: gtk::Button,
    pub(super) spinner: adw::Spinner,
    pub(super) filesystem: adw::ToggleGroup,
    pub(super) filesystem_row: adw::ActionRow,
    pub(super) swap: adw::SwitchRow,
    pub(super) tuning: adw::SwitchRow,
    pub(super) layout: LayoutBar,
}

pub(super) struct AccountPage {
    pub(super) full_name: adw::EntryRow,
    pub(super) username: adw::EntryRow,
    pub(super) password: adw::PasswordEntryRow,
    pub(super) repeat: adw::PasswordEntryRow,
    pub(super) hostname: adw::EntryRow,
    pub(super) password_hint: Label,
}

pub(super) struct GithubPage {
    pub(super) username: adw::EntryRow,
    pub(super) lookup: gtk::Button,
    pub(super) spinner: adw::Spinner,
    pub(super) status: Label,
    pub(super) keys: adw::PreferencesGroup,
    pub(super) key_rows: RefCell<Vec<adw::ActionRow>>,
    pub(super) authorize: adw::SwitchRow,
    pub(super) ssh_server: adw::SwitchRow,
    pub(super) git_name: adw::EntryRow,
    pub(super) git_email: adw::EntryRow,
}

pub(super) struct DesktopPage {
    pub(super) cards: Vec<gtk::ToggleButton>,
    pub(super) default: adw::ComboRow,
    pub(super) default_model: gtk::StringList,
    pub(super) wifi: adw::SwitchRow,
    pub(super) unfree: adw::SwitchRow,
}

pub(super) struct LocationPage {
    pub(super) map: ZoneMap,
    pub(super) zone_name: Label,
    pub(super) zone_place: Label,
    pub(super) zone_time: Label,
    pub(super) zone_detail: Label,
    pub(super) zone_list: adw::ComboRow,
    pub(super) detect: gtk::Button,
    pub(super) spinner: adw::Spinner,
    pub(super) status: Label,
    pub(super) internet: adw::SwitchRow,
    pub(super) locale: adw::ComboRow,
    pub(super) keyboard: adw::ComboRow,
}

pub(super) struct ReviewPage {
    pub(super) page: adw::PreferencesPage,
    pub(super) erase_title: Label,
    pub(super) erase_detail: Label,
    pub(super) prep_row: adw::ActionRow,
    pub(super) prep_spinner: adw::Spinner,
    pub(super) prep_icon: gtk::Image,
    pub(super) prep_bar: gtk::ProgressBar,
    pub(super) prep_failure: adw::ExpanderRow,
    pub(super) prep_failure_text: gtk::TextView,
    pub(super) disk: adw::ActionRow,
    pub(super) partitions: adw::ActionRow,
    pub(super) filesystem: adw::ActionRow,
    pub(super) boot: adw::ActionRow,
    pub(super) desktops: adw::ActionRow,
    pub(super) session: adw::ActionRow,
    pub(super) applications: adw::ActionRow,
    pub(super) tuning: adw::ActionRow,
    pub(super) unfree: adw::ActionRow,
    pub(super) graphics: adw::ActionRow,
    pub(super) wifi: adw::ActionRow,
    pub(super) computer: adw::ActionRow,
    pub(super) user: adw::ActionRow,
    pub(super) ssh_keys: adw::ActionRow,
    pub(super) ssh_server: adw::ActionRow,
    pub(super) git: adw::ActionRow,
    pub(super) locale: adw::ActionRow,
    pub(super) zone: adw::ActionRow,
    pub(super) keyboard: adw::ActionRow,
    pub(super) erase: adw::EntryRow,
    pub(super) consent: CheckButton,
}

pub(super) struct InstallPage {
    pub(super) stack: gtk::Stack,
    pub(super) heading: Label,
    pub(super) message: Label,
    pub(super) elapsed: Label,
    pub(super) bar: gtk::ProgressBar,
    pub(super) steps: Vec<(adw::ActionRow, gtk::Stack, adw::Spinner)>,
    pub(super) log: gtk::TextView,
    pub(super) failure: gtk::Box,
    pub(super) failure_text: gtk::TextView,
    pub(super) done: adw::StatusPage,
}

fn group(title: &str, description: &str) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title(title).build();
    if !description.is_empty() {
        group.set_description(Some(description));
    }
    group
}

fn boxed_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    gtk::ListBoxRow::builder()
        .activatable(false)
        .selectable(false)
        .child(child)
        .build()
}

fn check_row(text: &str) -> (adw::ActionRow, CheckButton) {
    let check = CheckButton::new();
    check.set_valign(Align::Center);
    let row = adw::ActionRow::builder()
        .title(text)
        .activatable_widget(&check)
        .build();
    row.add_prefix(&check);
    (row, check)
}

pub(super) fn build_sidebar() -> (adw::NavigationPage, Sidebar) {
    let logo = gtk::Image::from_icon_name("nix-snowflake");
    logo.set_pixel_size(40);
    let name = Label::new(Some("Genkan"));
    name.add_css_class("brand-title");
    name.set_xalign(0.0);
    let tagline = Label::new(Some("Welcome to Sekka"));
    tagline.add_css_class("brand-subtitle");
    tagline.set_xalign(0.0);
    let text = gtk::Box::new(Orientation::Vertical, 2);
    text.set_valign(Align::Center);
    text.append(&name);
    text.append(&tagline);
    let brand = gtk::Box::new(Orientation::Horizontal, 12);
    brand.set_margin_start(18);
    brand.set_margin_end(18);
    brand.set_margin_top(6);
    brand.set_margin_bottom(18);
    brand.append(&logo);
    brand.append(&text);
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.add_css_class("step-list");
    let mut rows = Vec::new();
    for step in Step::ALL {
        let badge = Label::new(Some(&(step.index() + 1).to_string()));
        badge.add_css_class("step-badge");
        let image = icon(step.icon());
        image.add_css_class("dim-label");
        let title = Label::new(Some(step.title()));
        title.add_css_class("step-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let content = gtk::Box::new(Orientation::Horizontal, 12);
        content.append(&badge);
        content.append(&title);
        content.append(&image);
        let row = gtk::ListBoxRow::builder().child(&content).build();
        list.append(&row);
        rows.push((row, badge));
    }
    let body = gtk::Box::new(Orientation::Vertical, 0);
    body.append(&brand);
    body.append(&list);
    let header = adw::HeaderBar::builder().show_title(false).build();
    let view = adw::ToolbarView::new();
    view.add_css_class("installer-sidebar");
    view.add_top_bar(&header);
    view.set_content(Some(&body));
    (
        adw::NavigationPage::new(&view, "Genkan"),
        Sidebar { list, rows },
    )
}

pub(super) fn build_welcome() -> adw::StatusPage {
    let features = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .max_children_per_line(3)
        .min_children_per_line(1)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    for (name, title, text) in [
        (
            "software-update-available-symbolic",
            "Prepared while you review",
            "Your system is built and cached in memory before anything is written, so the final install is short.",
        ),
        (
            "security-high-symbolic",
            "Pinned and reproducible",
            "Installs exactly the tested Nixpkgs and Determinate Nix revisions on this media.",
        ),
        (
            "preferences-system-symbolic",
            "Tuned for desktops",
            "RAM-sized swap with zswap and CachyOS-inspired kernel and I/O defaults.",
        ),
    ] {
        let image = icon(name);
        image.add_css_class("feature-icon");
        image.set_halign(Align::Start);
        let heading = label(title);
        heading.add_css_class("feature-title");
        let body = label(text);
        body.add_css_class("dim-label");
        // Bounded natural widths let the cards sit side by side.
        heading.set_max_width_chars(22);
        body.set_max_width_chars(22);
        body.set_width_chars(16);
        let card = gtk::Box::new(Orientation::Vertical, 10);
        card.add_css_class("card");
        card.add_css_class("feature-card");
        card.append(&image);
        card.append(&heading);
        card.append(&body);
        features.append(&card);
    }
    let warning = gtk::Box::new(Orientation::Horizontal, 12);
    warning.add_css_class("warning-card");
    let warning_icon = icon("dialog-warning-symbolic");
    warning_icon.set_valign(Align::Start);
    warning.append(&warning_icon);
    let warning_text = label(
        "This installer erases one whole disk: GPT with ext4, Btrfs or XFS, UEFI or legacy BIOS. It does not support manual partitioning, encryption, keeping another operating system, or offline installation. Back up your data before continuing.",
    );
    warning_text.set_hexpand(true);
    warning.append(&warning_text);
    let content = gtk::Box::new(Orientation::Vertical, 24);
    content.append(&features);
    content.append(&warning);
    adw::StatusPage::builder()
        .icon_name("nix-snowflake")
        .title("Install Sekka")
        .description("With Determinate Nix, your choice of desktops and applications, and a fast installation that is prepared before your disk is touched.")
        .child(&clamp(&content, 860))
        .build()
}

pub(super) fn build_disk_page() -> (adw::PreferencesPage, DiskPage) {
    let page = adw::PreferencesPage::new();
    let disks = group(
        "Disk to Erase",
        "Everything on the selected disk will be erased. Mounted disks, the live USB and disks smaller than 24 GiB are unavailable. No disk is selected for you.",
    );
    let spinner = adw::Spinner::new();
    let rescan = gtk::Button::from_icon_name("view-refresh-symbolic");
    rescan.add_css_class("flat");
    rescan.set_tooltip_text(Some("Rescan disks"));
    let suffix = gtk::Box::new(Orientation::Horizontal, 6);
    suffix.append(&spinner);
    suffix.append(&rescan);
    disks.set_header_suffix(Some(&suffix));
    page.add(&disks);

    let storage = group("Storage", "");
    let layout = LayoutBar::new();
    storage.add(&boxed_row(&layout.widget));
    let filesystem = adw::ToggleGroup::new();
    for fs in Filesystem::ALL {
        filesystem.add(
            adw::Toggle::builder()
                .name(fs.name())
                .label(match fs {
                    Filesystem::Ext4 => "ext4",
                    Filesystem::Btrfs => "Btrfs",
                    Filesystem::Xfs => "XFS",
                })
                .build(),
        );
    }
    filesystem.set_active(0);
    filesystem.set_valign(Align::Center);
    let filesystem_row = adw::ActionRow::builder().title("Root filesystem").build();
    filesystem_row.add_suffix(&filesystem);
    storage.add(&filesystem_row);
    let swap = adw::SwitchRow::builder()
        .title("Swap sized to memory, with zswap")
        .subtitle("Reading installed memory…")
        .active(true)
        .build();
    storage.add(&swap);
    let tuning = adw::SwitchRow::builder()
        .title("Performance tuning")
        .subtitle("CachyOS-inspired defaults: memory and writeback sysctls, I/O schedulers, ananicy-cpp process priorities and systemd-oomd. Uses the stock kernel and packages.")
        .active(true)
        .build();
    storage.add(&tuning);
    page.add(&storage);
    (
        page,
        DiskPage {
            group: disks,
            rows: RefCell::new(Vec::new()),
            rescan,
            spinner,
            filesystem,
            filesystem_row,
            swap,
            tuning,
            layout,
        },
    )
}

pub(super) fn build_account_page() -> (adw::PreferencesPage, AccountPage) {
    let page = adw::PreferencesPage::new();
    let you = group(
        "About You",
        "Root login is locked. Your account administers the system with sudo.",
    );
    let full_name = adw::EntryRow::builder().title("Full name").build();
    let username = adw::EntryRow::builder().title("Username").build();
    let password = adw::PasswordEntryRow::builder().title("Password").build();
    let repeat = adw::PasswordEntryRow::builder()
        .title("Confirm password")
        .build();
    you.add(&full_name);
    you.add(&username);
    you.add(&password);
    you.add(&repeat);
    let password_hint = caption(
        "Use at least 12 characters. Passwords are typed with this live session's keyboard layout.",
    );
    password_hint.set_margin_top(10);
    you.add(&password_hint);
    page.add(&you);
    let computer = group(
        "This Computer",
        "The name shown on your network: letters, numbers and inner hyphens.",
    );
    let hostname = adw::EntryRow::builder().title("Computer name").build();
    hostname.set_text("nixos");
    computer.add(&hostname);
    page.add(&computer);
    (
        page,
        AccountPage {
            full_name,
            username,
            password,
            repeat,
            hostname,
            password_hint,
        },
    )
}

/// Status line under the GitHub username before any lookup.
pub(super) const GITHUB_HINT: &str = "Optional. Press Enter or Look Up to fetch the account's public keys. Leave it empty to skip GitHub.";

pub(super) fn build_github_page() -> (adw::PreferencesPage, GithubPage) {
    let page = adw::PreferencesPage::new();
    let account = group(
        "GitHub",
        "Bring your public SSH keys from GitHub, as Ubuntu's server installer does, so you can log in to this computer with them.",
    );
    let username = adw::EntryRow::builder().title("GitHub username").build();
    let spinner = adw::Spinner::new();
    spinner.set_visible(false);
    let lookup = gtk::Button::builder()
        .label("Look Up")
        .valign(Align::Center)
        .sensitive(false)
        .build();
    lookup.add_css_class("flat");
    username.add_suffix(&spinner);
    username.add_suffix(&lookup);
    account.add(&username);
    let status = caption(GITHUB_HINT);
    status.set_margin_top(10);
    account.add(&status);
    page.add(&account);

    let keys = group(
        "SSH Keys",
        "Fetched from github.com. Only the public halves of your keys are shared.",
    );
    let authorize = adw::SwitchRow::builder()
        .title("Allow these keys to log in")
        .subtitle("Adds them to your account's authorized keys")
        .active(true)
        .build();
    keys.add(&authorize);
    keys.set_visible(false);
    page.add(&keys);

    let remote = group("Remote Login", "");
    let ssh_server = adw::SwitchRow::builder()
        .title("Install and enable the SSH server")
        .build();
    remote.add(&ssh_server);
    page.add(&remote);

    let git = group(
        "Git",
        "The name and email address recorded in your commits. Leave both empty to set them later.",
    );
    let git_name = adw::EntryRow::builder().title("Name").build();
    let git_email = adw::EntryRow::builder().title("Email").build();
    git.add(&git_name);
    git.add(&git_email);
    let git_hint = caption(
        "Saved in /etc/gitconfig for every account on this computer; git config --global sets your own.",
    );
    git_hint.set_margin_top(10);
    git.add(&git_hint);
    page.add(&git);
    (
        page,
        GithubPage {
            username,
            lookup,
            spinner,
            status,
            keys,
            key_rows: RefCell::new(Vec::new()),
            authorize,
            ssh_server,
            git_name,
            git_email,
        },
    )
}

pub(super) fn build_desktop_page() -> (adw::PreferencesPage, DesktopPage) {
    let page = adw::PreferencesPage::new();
    let environments = group(
        "Desktop Environments",
        "Choose one or more. Every selected desktop appears on the login screen. Your choices apply to the installed system.",
    );
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .max_children_per_line(3)
        .min_children_per_line(1)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    let cards: Vec<_> = Desktop::ALL
        .iter()
        .map(|desktop| {
            let card = desktop_card(*desktop);
            card.set_active(*desktop == Desktop::Aizome);
            flow.append(&card);
            card
        })
        .collect();
    environments.add(&flow);
    page.add(&environments);
    let login = group("Login", "");
    let default_model = gtk::StringList::new(&[]);
    let default = adw::ComboRow::builder()
        .title("Default session")
        .subtitle("Preselected on the login screen")
        .model(&default_model)
        .build();
    login.add(&default);
    page.add(&login);
    let software = group("Network and Software", "");
    let wifi = adw::SwitchRow::builder()
        .title("Bring saved Wi-Fi networks")
        .subtitle("Copies this live session's saved Wi-Fi connections and passwords, stored root-only outside the Nix store. Keep the wallet unlocked. Enterprise networks using certificate files or hardware tokens must be set up after installation.")
        .active(true)
        .build();
    software.add(&wifi);
    let unfree = adw::SwitchRow::builder()
        .title("Allow unfree software")
        .subtitle("Some hardware drivers and applications require it. Redistributable firmware is included either way, and allowing unfree packages does not configure every vendor driver automatically.")
        .active(calamares_nixos::DEFAULT_ALLOW_UNFREE)
        .build();
    software.add(&unfree);
    page.add(&software);
    (
        page,
        DesktopPage {
            cards,
            default,
            default_model,
            wifi,
            unfree,
        },
    )
}

pub(super) fn build_location_page(zones: Option<Rc<Zones>>) -> (gtk::ScrolledWindow, LocationPage) {
    let labels: Vec<String> = zones
        .iter()
        .flat_map(|z| z.places.iter().map(Place::label))
        .collect();
    let map = ZoneMap::new(zones);
    let text = |class: &str, xalign: f32| {
        let label = Label::new(None);
        label.add_css_class(class);
        label.set_xalign(xalign);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label
    };
    let zone_name = text("zone-name", 0.0);
    let zone_place = text("dim-label", 0.0);
    let names = gtk::Box::new(Orientation::Vertical, 2);
    names.set_hexpand(true);
    names.append(&zone_name);
    names.append(&zone_place);
    let zone_time = text("zone-time", 1.0);
    let zone_detail = text("caption", 1.0);
    zone_detail.add_css_class("dim-label");
    let clock = gtk::Box::new(Orientation::Vertical, 2);
    clock.set_valign(Align::Center);
    clock.append(&zone_time);
    clock.append(&zone_detail);
    let info = gtk::Box::new(Orientation::Horizontal, 12);
    info.add_css_class("zone-info");
    info.append(&names);
    info.append(&clock);

    let spinner = adw::Spinner::new();
    spinner.set_visible(false);
    let status = caption("Finding your location…");
    status.set_hexpand(true);
    status.set_valign(Align::Center);
    let detect = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name("find-location-symbolic")
                .label("Detect")
                .build(),
        )
        .tooltip_text("Detect the time zone again")
        .valign(Align::Center)
        .build();
    detect.add_css_class("flat");
    let footer = gtk::Box::new(Orientation::Horizontal, 8);
    footer.add_css_class("zone-status");
    footer.append(&spinner);
    footer.append(&status);
    footer.append(&detect);

    let card = gtk::Box::new(Orientation::Vertical, 0);
    card.add_css_class("card");
    card.add_css_class("zone-card");
    card.set_overflow(gtk::Overflow::Hidden);
    card.append(&map);
    card.append(&info);
    card.append(&gtk::Separator::new(Orientation::Horizontal));
    card.append(&footer);
    let zone = group(
        "Time Zone",
        "Click where you are on the map, or search for a nearby city.",
    );
    zone.add(&card);

    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let zone_list = adw::ComboRow::builder()
        .title("Time zone")
        .model(&gtk::StringList::new(&refs))
        .enable_search(true)
        .search_match_mode(gtk::StringFilterMatchMode::Substring)
        .expression(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<&gtk::Expression>,
            "string",
        ))
        .build();
    let internet = adw::SwitchRow::builder()
        .title("Find my location online")
        .subtitle("Asks geoip.kde.org, or ipinfo.io if it is unavailable. They see your public IP address, and VPNs or mobile networks can place you elsewhere.")
        .active(true)
        .build();
    // Nothing is selected until detection or the user chooses.
    zone_list.set_selected(gtk::INVALID_LIST_POSITION);
    zone_name.set_text("Finding your time zone");
    zone_place.set_text("You can click your location on the map at any time");
    let choices = adw::PreferencesGroup::new();
    choices.add(&zone_list);
    choices.add(&internet);
    let language = group(
        "Language and Keyboard",
        "The live keyboard layout is unchanged; these apply to the installed system.",
    );
    let locales: Vec<String> = LOCALE_NAMES
        .iter()
        .zip(LOCALES)
        .map(|(name, code)| format!("{name} — {code}"))
        .collect();
    let locale = adw::ComboRow::builder()
        .title("System language")
        .use_subtitle(true)
        .model(&gtk::StringList::new(
            &locales.iter().map(String::as_str).collect::<Vec<_>>(),
        ))
        .build();
    language.add(&locale);
    let keyboards: Vec<String> = KEYBOARD_NAMES
        .iter()
        .zip(KEYBOARDS)
        .map(|(name, code)| format!("{name} — {code}"))
        .collect();
    let keyboard = adw::ComboRow::builder()
        .title("Keyboard layout")
        .use_subtitle(true)
        .model(&gtk::StringList::new(
            &keyboards.iter().map(String::as_str).collect::<Vec<_>>(),
        ))
        .build();
    language.add(&keyboard);
    let content = gtk::Box::new(Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&zone);
    content.append(&choices);
    content.append(&language);
    (
        scrolled(&clamp(&content, 760)),
        LocationPage {
            map,
            zone_name,
            zone_place,
            zone_time,
            zone_detail,
            zone_list,
            detect,
            spinner,
            status,
            internet,
            locale,
            keyboard,
        },
    )
}

pub(super) fn build_review_page() -> ReviewPage {
    let page = adw::PreferencesPage::new();
    let erase_title = label("");
    erase_title.add_css_class("erase-title");
    let erase_detail = label("");
    let erase_text = gtk::Box::new(Orientation::Vertical, 4);
    erase_text.append(&erase_title);
    erase_text.append(&erase_detail);
    let erase_icon = icon("dialog-warning-symbolic");
    erase_icon.set_pixel_size(32);
    erase_icon.add_css_class("error-icon");
    erase_icon.set_valign(Align::Center);
    let erase_card = gtk::Box::new(Orientation::Horizontal, 16);
    erase_card.add_css_class("erase-card");
    erase_card.append(&erase_icon);
    erase_card.append(&erase_text);
    let top = adw::PreferencesGroup::new();
    top.add(&erase_card);
    page.add(&top);
    let confirm = group(
        "Confirm",
        "Installation uses the media's pinned Determinate Nix flake. This cannot be undone.",
    );
    let erase = adw::EntryRow::builder().title("Type ERASE /dev/…").build();
    confirm.add(&erase);
    let (consent_row, consent) =
        check_row("I understand that all partitions and data on this disk will be destroyed");
    confirm.add(&consent_row);
    page.add(&confirm);

    let preparation = group(
        "Preparing in the Background",
        "Your system is evaluated, built and cached in memory now. Nothing has been written to any disk.",
    );
    let prep_spinner = adw::Spinner::new();
    let prep_icon = icon("object-select-symbolic");
    prep_icon.set_visible(false);
    let prep_row = adw::ActionRow::builder()
        .title("Waiting for authorization…")
        .build();
    prep_row.set_use_markup(false);
    prep_row.add_prefix(&prep_spinner);
    prep_row.add_prefix(&prep_icon);
    preparation.add(&prep_row);
    let prep_bar = gtk::ProgressBar::new();
    prep_bar.set_margin_top(12);
    prep_bar.set_visible(false);
    prep_bar.add_css_class("install-progress");
    preparation.add(&prep_bar);
    let prep_failure_text = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .css_classes(["failure-view"])
        .build();
    let prep_failure = adw::ExpanderRow::builder()
        .title("Error details")
        .visible(false)
        .build();
    prep_failure.add_row(&boxed_row(
        &gtk::ScrolledWindow::builder()
            .min_content_height(160)
            .max_content_height(320)
            .child(&prep_failure_text)
            .build(),
    ));
    preparation.add(&prep_failure);
    page.add(&preparation);

    let disk_group = group("Disk", "");
    let disk = property("Disk to erase");
    let partitions = property("New partitions");
    let filesystem = property("Root filesystem");
    let boot = property("Boot");
    for row in [&disk, &partitions, &filesystem, &boot] {
        disk_group.add(row);
    }
    page.add(&disk_group);
    let system_group = group("System", "");
    let desktops = property("Desktops");
    let session = property("Default session");
    let applications = property("Applications");
    let tuning = property("Performance tuning");
    let unfree = property("Unfree software");
    let graphics = property("Graphics driver");
    let wifi = property("Wi-Fi networks");
    for row in [
        &desktops,
        &session,
        &applications,
        &tuning,
        &unfree,
        &graphics,
        &wifi,
    ] {
        system_group.add(row);
    }
    page.add(&system_group);
    let account_group = group("Account and Location", "");
    let computer = property("Computer name");
    let user = property("User");
    let locale = property("Language");
    let zone = property("Time zone");
    let keyboard = property("Keyboard layout");
    for row in [&computer, &user, &locale, &zone, &keyboard] {
        account_group.add(row);
    }
    page.add(&account_group);
    let github_group = group("GitHub, SSH and Git", "");
    let ssh_keys = property("SSH keys");
    let ssh_server = property("SSH server");
    let git = property("Git identity");
    for row in [&ssh_keys, &ssh_server, &git] {
        github_group.add(row);
    }
    page.add(&github_group);
    ReviewPage {
        page,
        erase_title,
        erase_detail,
        prep_row,
        prep_spinner,
        prep_icon,
        prep_bar,
        prep_failure,
        prep_failure_text,
        disk,
        partitions,
        filesystem,
        boot,
        desktops,
        session,
        applications,
        tuning,
        unfree,
        graphics,
        wifi,
        computer,
        user,
        ssh_keys,
        ssh_server,
        git,
        locale,
        zone,
        keyboard,
        erase,
        consent,
    }
}

pub(super) fn build_install_page() -> InstallPage {
    let heading = Label::new(Some("Installing NixOS"));
    heading.add_css_class("install-title");
    heading.set_xalign(0.0);
    let message = label("Starting…");
    message.add_css_class("dim-label");
    let elapsed = Label::new(Some("0:00"));
    elapsed.add_css_class("elapsed");
    elapsed.set_valign(Align::Center);
    let titles = gtk::Box::new(Orientation::Vertical, 6);
    titles.set_hexpand(true);
    titles.append(&heading);
    titles.append(&message);
    let top = gtk::Box::new(Orientation::Horizontal, 18);
    top.append(&titles);
    top.append(&elapsed);
    let bar = gtk::ProgressBar::new();
    bar.add_css_class("install-progress");
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let mut steps = Vec::new();
    for title in INSTALL_STEPS {
        let state = gtk::Stack::new();
        let pending = icon("radio-symbolic");
        pending.add_css_class("pending-icon");
        let spinner = adw::Spinner::new();
        let done = icon("object-select-symbolic");
        done.add_css_class("success-icon");
        let failed = icon("dialog-error-symbolic");
        failed.add_css_class("error-icon");
        state.add_named(&pending, Some("pending"));
        state.add_named(&spinner, Some("current"));
        state.add_named(&done, Some("done"));
        state.add_named(&failed, Some("failed"));
        state.set_visible_child_name("pending");
        let row = adw::ActionRow::builder().title(title).build();
        row.add_prefix(&state);
        list.append(&row);
        steps.push((row, state, spinner));
    }
    let log = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .css_classes(["log-view"])
        .build();
    let details = adw::ExpanderRow::builder()
        .title("Details")
        .subtitle("Activity reported by Nix and the installer")
        .build();
    details.add_row(&boxed_row(
        &gtk::ScrolledWindow::builder()
            .min_content_height(180)
            .max_content_height(260)
            .child(&log)
            .build(),
    ));
    let details_list = gtk::ListBox::new();
    details_list.add_css_class("boxed-list");
    details_list.set_selection_mode(gtk::SelectionMode::None);
    details_list.append(&details);
    let failure_text = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::WordChar)
        .css_classes(["failure-view", "card"])
        .build();
    let failure_title = label("Installation did not complete");
    failure_title.add_css_class("title-4");
    failure_title.add_css_class("error");
    let failure = gtk::Box::new(Orientation::Vertical, 8);
    failure.append(&failure_title);
    failure.append(&caption(
        "The disk may have been modified. Copy these details before closing; storage diagnostics, if any, are saved under /run.",
    ));
    failure.append(
        &gtk::ScrolledWindow::builder()
            .min_content_height(160)
            .max_content_height(320)
            .child(&failure_text)
            .build(),
    );
    failure.set_visible(false);
    let body = gtk::Box::new(Orientation::Vertical, 18);
    body.set_margin_top(32);
    body.set_margin_bottom(32);
    body.set_margin_start(12);
    body.set_margin_end(12);
    body.append(&top);
    body.append(&bar);
    body.append(&list);
    body.append(&failure);
    body.append(&details_list);
    let progress = scrolled(&clamp(&body, 720));
    let done = adw::StatusPage::builder()
        .icon_name("object-select-symbolic")
        .title("NixOS Is Installed")
        .build();
    done.add_css_class("done-page");
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.add_named(&progress, Some("progress"));
    stack.add_named(&done, Some("done"));
    InstallPage {
        stack,
        heading,
        message,
        elapsed,
        bar,
        steps,
        log,
        failure,
        failure_text,
        done,
    }
}
