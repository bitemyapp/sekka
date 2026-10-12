// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Network window, after macOS's Network settings: Wi-Fi (the switch,
//! the network in use, known and other networks in range, and joining one
//! by name), wired connections, VPNs, and the saved Wi-Fi networks; each
//! connection's settings on a page of its own (details.rs).
//!
//! It reads NetworkManager on a worker thread when NetworkManager reports a
//! change (`nmcli monitor`) and every few seconds while open, and asks for a
//! scan now and then; it never waits for one. Rows are rebuilt only when
//! what they show changes, so reading often moves nothing about.
use crate::{
    details, join,
    model::{self, Kind, Network, Saved, Snapshot},
    nm,
};
use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

/// How often the window reads NetworkManager while open, besides each change
/// it reports, and how often it asks for a scan while focused.
const READ_SECONDS: u32 = 4;
const SCAN_SECONDS: u32 = 30;

pub struct State {
    pub snapshot: Snapshot,
    pub loaded: bool,
    /// The network being joined, shown with a spinner.
    pub joining: Option<String>,
    /// Connections being brought up or down from their switches.
    pub busy: Vec<String>,
    /// A network to join once the first reading is in (`--join`).
    pending_join: Option<String>,
}

/// A group whose rows are replaced only when what they show changes.
struct Section {
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
    key: RefCell<Option<String>>,
}

impl Section {
    fn new(title: &str) -> Section {
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        Section {
            group,
            rows: RefCell::new(Vec::new()),
            key: RefCell::new(None),
        }
    }

    /// Show these rows, rebuilt only when `key` (what they show) changed.
    fn show(&self, key: String, rows: impl FnOnce() -> Vec<gtk::Widget>) {
        if self.key.borrow().as_deref() == Some(key.as_str()) {
            return;
        }
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        let rows = rows();
        for row in &rows {
            self.group.add(row);
        }
        *self.rows.borrow_mut() = rows;
        *self.key.borrow_mut() = Some(key);
    }
}

pub struct Ui {
    pub window: adw::ApplicationWindow,
    pub nav: adw::NavigationView,
    toasts: adw::ToastOverlay,
    wifi_switch: adw::SwitchRow,
    wifi: Section,
    known: Section,
    others: Section,
    scan: gtk::Stack,
    wired: Section,
    vpn: Section,
    saved: Section,
    pub state: RefCell<State>,
    /// Set while widgets are brought in line with a reading: their handlers
    /// then change nothing.
    updating: Cell<bool>,
    reading: Cell<bool>,
    read_again: Cell<bool>,
    presented: Cell<bool>,
    pub details: RefCell<Option<Rc<details::Page>>>,
    saved_list: RefCell<Option<(adw::NavigationPage, Section)>>,
}

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// The window, made the first time; shown as soon as it has something to
/// show.
pub fn open(app: &adw::Application) -> Rc<Ui> {
    if let Some(ui) = UI.with_borrow(Clone::clone) {
        ui.window.present();
        return ui;
    }
    let ui = build(app);
    UI.with_borrow_mut(|slot| *slot = Some(ui.clone()));
    read(&ui);
    scan(&ui);
    // A reading takes a few hundredths of a second; show the window then,
    // or now if it is slower, rather than empty.
    let soon = ui.clone();
    glib::timeout_add_local_once(Duration::from_millis(250), move || present(&soon));
    ui
}

fn present(ui: &Ui) {
    if !ui.presented.replace(true) {
        ui.window.present();
    }
}

/// Run `work` on a worker thread, then `then` with its result on the
/// window's.
pub fn background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    then: impl FnOnce(T) + 'static,
) {
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(result) = receiver.recv().await {
            then(result);
        }
    });
}

pub fn toast(ui: &Ui, text: &str) {
    let toast = adw::Toast::new(text);
    toast.set_use_markup(false);
    ui.toasts.add_toast(toast);
}

/// A page of the window: a header bar over scrolled, clamped groups.
pub fn page(
    title: &str,
    tag: Option<&str>,
    content: &impl IsA<gtk::Widget>,
) -> adw::NavigationPage {
    let clamp = adw::Clamp::builder()
        .maximum_size(640)
        .child(content)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&scroller));
    let page = adw::NavigationPage::new(&view, title);
    if let Some(tag) = tag {
        page.set_tag(Some(tag));
    }
    page
}

pub fn column() -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 24);
    column.set_margin_top(18);
    column.set_margin_bottom(24);
    column.set_margin_start(18);
    column.set_margin_end(18);
    column
}

/// A row that leads to another page.
pub fn next_row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row
}

fn build(app: &adw::Application) -> Rc<Ui> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Network")
        .icon_name("preferences-system-network")
        .default_width(720)
        .default_height(760)
        .build();

    let wifi_switch = adw::SwitchRow::builder()
        .title("Wi-Fi")
        .use_markup(false)
        .build();
    let wifi = Section::new("");
    wifi.group.add(&wifi_switch);
    let known = Section::new("Known networks");
    let others = Section::new("Other networks");
    // Look for networks: a button, or a spinner while a scan runs.
    let scan_button = gtk::Button::from_icon_name("view-refresh-symbolic");
    scan_button.add_css_class("flat");
    scan_button.set_tooltip_text(Some("Look for networks"));
    let scan_stack = gtk::Stack::new();
    scan_stack.add_named(&scan_button, Some("button"));
    let spinner = adw::Spinner::builder()
        .width_request(16)
        .height_request(16)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    scan_stack.add_named(&spinner, Some("spinner"));
    others.group.set_header_suffix(Some(&scan_stack));
    let wired = Section::new("Wired");
    let vpn = Section::new("VPN");
    let saved = Section::new("");

    let content = column();
    for section in [&wifi, &known, &others, &wired, &vpn, &saved] {
        section.group.set_visible(false);
        content.append(&section.group);
    }
    let root = page("Network", Some("network"), &content);
    let nav = adw::NavigationView::new();
    nav.add(&root);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&nav));
    window.set_content(Some(&toasts));

    let ui = Rc::new(Ui {
        window: window.clone(),
        nav: nav.clone(),
        toasts,
        wifi_switch: wifi_switch.clone(),
        wifi,
        known,
        others,
        scan: scan_stack,
        wired,
        vpn,
        saved,
        state: RefCell::new(State {
            snapshot: Snapshot::default(),
            loaded: false,
            joining: None,
            busy: Vec::new(),
            pending_join: None,
        }),
        updating: Cell::new(false),
        reading: Cell::new(false),
        read_again: Cell::new(false),
        presented: Cell::new(false),
        details: RefCell::new(None),
        saved_list: RefCell::new(None),
    });

    // Escape goes back a page, and closes the window from the first, as it
    // does Tatami's other floating windows.
    let shortcuts = gtk::ShortcutController::new();
    let escape = Rc::downgrade(&ui);
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(move |_, _| {
            if let Some(ui) = escape.upgrade() {
                let tag = ui.nav.visible_page().and_then(|page| page.tag());
                if tag.as_deref() == Some("network") {
                    ui.window.close();
                } else {
                    ui.nav.pop();
                }
            }
            glib::Propagation::Stop
        })),
    ));
    window.add_controller(shortcuts);

    let toggled = ui.clone();
    wifi_switch.connect_active_notify(move |row| {
        if toggled.updating.get() {
            return;
        }
        let on = row.is_active();
        let ui = toggled.clone();
        background(
            move || nm::set_wifi(on),
            move |result| {
                if let Err(error) = result {
                    toast(&ui, &model::reason(&error));
                }
                read(&ui);
                if on {
                    scan(&ui);
                }
            },
        );
    });
    let clicked = ui.clone();
    scan_button.connect_clicked(move |_| scan(&clicked));
    let popped = ui.clone();
    nav.connect_popped(move |_, page| {
        let closed = |tag: &str| page.tag().as_deref() == Some(tag);
        let open = popped
            .details
            .borrow()
            .as_ref()
            .map(|details| format!("details-{}", details.uuid));
        if open.is_some_and(|tag| closed(&tag)) {
            popped.details.borrow_mut().take();
        }
        if closed("saved") {
            popped.saved_list.borrow_mut().take();
        }
    });

    // Read again on each change NetworkManager reports, a moment after the
    // first of a burst.
    let (sender, receiver) = async_channel::unbounded::<()>();
    nm::monitor(move || {
        let _ = sender.send_blocking(());
    });
    let changed = Rc::downgrade(&ui);
    glib::spawn_future_local(async move {
        while receiver.recv().await.is_ok() {
            glib::timeout_future(Duration::from_millis(150)).await;
            while receiver.try_recv().is_ok() {}
            match changed.upgrade() {
                Some(ui) => read(&ui),
                None => break,
            }
        }
    });
    let timer = Rc::downgrade(&ui);
    glib::timeout_add_seconds_local(READ_SECONDS, move || match timer.upgrade() {
        Some(ui) => {
            if ui.window.is_visible() {
                read(&ui);
            }
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    let scanner = Rc::downgrade(&ui);
    glib::timeout_add_seconds_local(SCAN_SECONDS, move || match scanner.upgrade() {
        Some(ui) => {
            if ui.window.is_active() {
                scan(&ui);
            }
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    ui
}

/// Read NetworkManager and show what it says; a reading asked for while one
/// runs follows it.
pub fn read(ui: &Rc<Ui>) {
    if ui.reading.replace(true) {
        ui.read_again.set(true);
        return;
    }
    let ui = ui.clone();
    background(nm::snapshot, move |snapshot| {
        ui.reading.set(false);
        {
            let mut state = ui.state.borrow_mut();
            state.snapshot = snapshot;
            state.loaded = true;
        }
        render(&ui);
        present(&ui);
        let details = ui.details.borrow().clone();
        if let Some(details) = details {
            details.update(&ui);
        }
        let pending = ui.state.borrow_mut().pending_join.take();
        if let Some(ssid) = pending {
            join_by_name(&ui, ssid);
        }
        if ui.read_again.replace(false) {
            read(&ui);
        }
    });
}

/// Ask for a scan, with the spinner shown for as long as one usually takes.
pub fn scan(ui: &Rc<Ui>) {
    if ui.scan.visible_child_name().as_deref() == Some("spinner") {
        return;
    }
    ui.scan.set_visible_child_name("spinner");
    let ui = ui.clone();
    background(
        || {
            nm::rescan();
            std::thread::sleep(Duration::from_secs(6));
        },
        move |()| {
            ui.scan.set_visible_child_name("button");
            read(&ui);
        },
    );
}

/// Join the network with this name: from `--join`, once the window has
/// read what is in range. A name not in range is joined as a hidden one.
pub fn join_by_name(ui: &Rc<Ui>, ssid: String) {
    if !ui.state.borrow().loaded {
        ui.state.borrow_mut().pending_join = Some(ssid);
        return;
    }
    let network = ui
        .state
        .borrow()
        .snapshot
        .networks
        .iter()
        .find(|network| network.ssid == ssid)
        .cloned();
    match network {
        Some(network) => join::network(ui, network),
        None => join::by_name(ui, Some(ssid)),
    }
}

fn wifi_subtitle(snapshot: &Snapshot) -> String {
    if !snapshot.wifi_hardware {
        "Off, turned off by the hardware switch".into()
    } else if !snapshot.wifi_enabled {
        "Off".into()
    } else if let Some((network, _)) = snapshot.current_wifi() {
        format!("Connected to {}", network.ssid)
    } else if snapshot
        .wifi_device()
        .is_some_and(model::Device::connecting)
    {
        "Connecting…".into()
    } else {
        "Not connected".into()
    }
}

/// Show the latest reading again, after the window's own state changed.
pub fn redraw(ui: &Rc<Ui>) {
    render(ui);
}

fn render(ui: &Rc<Ui>) {
    let state = ui.state.borrow();
    let snapshot = &state.snapshot;
    let wifi = snapshot.wifi_device().cloned();

    ui.updating.set(true);
    ui.wifi_switch.set_active(snapshot.wifi_enabled);
    ui.updating.set(false);
    ui.wifi_switch.set_sensitive(snapshot.wifi_hardware);
    ui.wifi_switch.set_subtitle(&wifi_subtitle(snapshot));
    let on = wifi.is_some() && snapshot.wifi_enabled;
    ui.wifi.group.set_visible(wifi.is_some());
    ui.others.group.set_visible(on);

    // The network in use.
    let current = snapshot.current_wifi().map(|(network, saved)| {
        let address = wifi
            .as_ref()
            .and_then(|device| snapshot.addresses.get(&device.name))
            .and_then(|addresses| addresses.address().map(str::to_owned))
            .unwrap_or_default();
        (network.clone(), saved.uuid.clone(), address)
    });
    ui.wifi.show(format!("{current:?}{on}"), || {
        let Some((network, uuid, address)) = current.clone().filter(|_| on) else {
            return Vec::new();
        };
        let subtitle = if address.is_empty() {
            "Connected".to_owned()
        } else {
            format!("Connected · {address}")
        };
        let row = next_row(&network.ssid, &subtitle);
        row.add_prefix(&gtk::Image::from_icon_name(model::signal_icon(
            network.signal,
        )));
        let opened = ui.clone();
        row.connect_activated(move |_| details::open(&opened, &uuid));
        vec![row.upcast()]
    });

    // Networks in range: those saved, then the others.
    let in_use = current.as_ref().map(|(network, ..)| network.ssid.clone());
    let (known, others): (Vec<&Network>, Vec<&Network>) = snapshot
        .networks
        .iter()
        .filter(|network| Some(&network.ssid) != in_use.as_ref())
        .partition(|network| snapshot.saved_for(&network.ssid).is_some());
    let describe = |networks: &[&Network]| -> Vec<(String, u8, String, bool)> {
        networks
            .iter()
            .map(|network| {
                (
                    network.ssid.clone(),
                    model::bars(network.signal),
                    network.security.clone(),
                    state.joining.as_deref() == Some(network.ssid.as_str()),
                )
            })
            .collect()
    };
    let rows = |networks: Vec<&Network>, saved: bool| -> Vec<gtk::Widget> {
        networks
            .into_iter()
            .map(|network| {
                let joining = state.joining.as_deref() == Some(network.ssid.as_str());
                let autoconnect = snapshot
                    .saved_for(&network.ssid)
                    .is_none_or(|saved| saved.autoconnect);
                network_row(ui, network, saved, autoconnect, joining).upcast()
            })
            .collect()
    };
    ui.known.group.set_visible(on && !known.is_empty());
    ui.known.show(format!("{:?}", describe(&known)), || {
        rows(known.clone(), true)
    });
    let searching = on && snapshot.networks.is_empty();
    ui.others
        .show(format!("{:?}{searching}", describe(&others)), || {
            let mut list = rows(others.clone(), false);
            if searching {
                let row = adw::ActionRow::builder()
                    .title("Looking for networks…")
                    .build();
                row.add_prefix(&adw::Spinner::new());
                list.push(row.upcast());
            }
            let other = next_row("Other network…", "Join a hidden network by its name");
            let opened = ui.clone();
            other.connect_activated(move |_| join::by_name(&opened, None));
            list.push(other.upcast());
            list
        });

    // Wired connections.
    let ethernet: Vec<_> = snapshot.ethernet().cloned().collect();
    let many = ethernet.len() > 1;
    let described: Vec<(String, String, Option<String>)> = ethernet
        .iter()
        .map(|device| {
            let subtitle = if device.connected() {
                match snapshot
                    .addresses
                    .get(&device.name)
                    .and_then(model::Addresses::address)
                {
                    Some(address) => format!("Connected · {address}"),
                    None => "Connected".into(),
                }
            } else if device.connecting() {
                "Connecting…".into()
            } else if device.state == "unavailable" {
                "Cable unplugged".into()
            } else {
                "Not connected".into()
            };
            // Its connection, or the one it would use.
            let uuid = device.connection.clone().or_else(|| {
                snapshot
                    .saved
                    .iter()
                    .filter(|saved| saved.kind == Kind::Ethernet)
                    .filter(|saved| {
                        saved
                            .device
                            .as_ref()
                            .is_none_or(|name| *name == device.name)
                    })
                    .max_by_key(|saved| saved.timestamp)
                    .map(|saved| saved.uuid.clone())
            });
            let title = if many {
                format!("Ethernet · {}", device.name)
            } else {
                "Ethernet".into()
            };
            (title, subtitle, uuid)
        })
        .collect();
    ui.wired.group.set_visible(!described.is_empty());
    ui.wired.show(format!("{described:?}"), || {
        described
            .iter()
            .map(|(title, subtitle, uuid)| {
                let row = match uuid {
                    Some(uuid) => {
                        let row = next_row(title, subtitle);
                        let opened = ui.clone();
                        let uuid = uuid.clone();
                        row.connect_activated(move |_| details::open(&opened, &uuid));
                        row
                    }
                    None => adw::ActionRow::builder()
                        .title(title.as_str())
                        .subtitle(subtitle.as_str())
                        .use_markup(false)
                        .build(),
                };
                row.add_prefix(&gtk::Image::from_icon_name("network-wired-symbolic"));
                row.upcast()
            })
            .collect()
    });

    // VPNs, each with its switch, and adding a WireGuard one.
    let vpns: Vec<Saved> = snapshot.vpns().cloned().collect();
    let described: Vec<_> = vpns
        .iter()
        .map(|vpn| {
            (
                vpn.uuid.clone(),
                vpn.name.clone(),
                vpn.active,
                vpn.activating,
                state.busy.contains(&vpn.uuid),
            )
        })
        .collect();
    ui.vpn.group.set_visible(true);
    ui.vpn.show(format!("{described:?}"), || {
        let mut rows: Vec<gtk::Widget> = vpns
            .iter()
            .map(|vpn| vpn_row(ui, vpn, state.busy.contains(&vpn.uuid)).upcast())
            .collect();
        let add = adw::ActionRow::builder()
            .title("Add WireGuard VPN…")
            .subtitle("From the configuration file your provider gives you")
            .activatable(true)
            .build();
        add.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
        let opened = ui.clone();
        add.connect_activated(move |_| add_wireguard(&opened));
        rows.push(add.upcast());
        rows
    });

    // Saved Wi-Fi networks, in range or not.
    let count = snapshot.saved_wifi().len();
    ui.saved.group.set_visible(wifi.is_some() && count > 0);
    ui.saved.show(format!("{count}"), || {
        let row = next_row(
            "Saved networks",
            &if count == 1 {
                "1 network".to_owned()
            } else {
                format!("{count} networks")
            },
        );
        let opened = ui.clone();
        row.connect_activated(move |_| open_saved(&opened));
        vec![row.upcast()]
    });
    drop(state);
    update_saved(ui);
}

fn network_row(
    ui: &Rc<Ui>,
    network: &Network,
    saved: bool,
    autoconnect: bool,
    joining: bool,
) -> adw::ActionRow {
    let security = network.security();
    let subtitle = if saved {
        if autoconnect {
            String::new()
        } else {
            "Automatic joining off".into()
        }
    } else {
        security.describe().into()
    };
    let row = adw::ActionRow::builder()
        .title(network.ssid.as_str())
        .use_markup(false)
        .activatable(!joining)
        .build();
    if !subtitle.is_empty() {
        row.set_subtitle(&subtitle);
    }
    row.add_prefix(&gtk::Image::from_icon_name(model::signal_icon(
        network.signal,
    )));
    if joining {
        row.add_suffix(&adw::Spinner::new());
    } else if security != model::Security::Open {
        let lock = gtk::Image::from_icon_name("channel-secure-symbolic");
        lock.add_css_class("dim-label");
        lock.set_tooltip_text(Some(security.describe()));
        row.add_suffix(&lock);
    }
    let opened = ui.clone();
    let network = network.clone();
    row.connect_activated(move |_| join::network(&opened, network.clone()));
    row
}

fn vpn_row(ui: &Rc<Ui>, vpn: &Saved, busy: bool) -> adw::ActionRow {
    let kind = if vpn.kind == Kind::WireGuard {
        "WireGuard"
    } else {
        "VPN"
    };
    let state = if vpn.activating {
        "Connecting…"
    } else if vpn.active {
        "Connected"
    } else {
        "Not connected"
    };
    let row = adw::ActionRow::builder()
        .title(vpn.name.as_str())
        .subtitle(format!("{kind} · {state}"))
        .use_markup(false)
        .activatable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("network-vpn-symbolic"));
    let switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(vpn.active || vpn.activating)
        .sensitive(!busy && !vpn.activating)
        .build();
    // Before the arrow: the switch is the row's quick action.
    row.add_suffix(&switch);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let toggled = ui.clone();
    let uuid = vpn.uuid.clone();
    let name = vpn.name.clone();
    switch.connect_state_set(move |_, on| {
        set_vpn(&toggled, &uuid, &name, on);
        glib::Propagation::Proceed
    });
    let opened = ui.clone();
    let uuid = vpn.uuid.clone();
    row.connect_activated(move |_| details::open(&opened, &uuid));
    row
}

pub fn set_vpn(ui: &Rc<Ui>, uuid: &str, name: &str, on: bool) {
    ui.state.borrow_mut().busy.push(uuid.to_owned());
    let (ui, uuid, name) = (ui.clone(), uuid.to_owned(), name.to_owned());
    let work = uuid.clone();
    background(
        move || {
            if on {
                nm::connect(&work, None, None)
            } else {
                nm::disconnect(&work)
            }
        },
        move |result| {
            ui.state.borrow_mut().busy.retain(|busy| *busy != uuid);
            if let Err(error) = result {
                let what = if on { "connect" } else { "disconnect" };
                toast(
                    &ui,
                    &format!("Could not {what} {name}: {}", model::reason(&error)),
                );
            }
            read(&ui);
        },
    );
}

/// Add a WireGuard VPN from its configuration file.
fn add_wireguard(ui: &Rc<Ui>) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("WireGuard configuration"));
    filter.add_suffix("conf");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Add WireGuard VPN")
        .modal(true)
        .filters(&filters)
        .default_filter(&filter)
        .build();
    let ui = ui.clone();
    dialog.open(
        Some(&ui.window.clone()),
        gio::Cancellable::NONE,
        move |file| {
            let Some(path) = file.ok().and_then(|file| file.path()) else {
                return;
            };
            let name = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let path = path.to_string_lossy().into_owned();
            background(
                move || nm::import("wireguard", &path),
                move |result| {
                    match result {
                        Ok(()) => toast(&ui, &format!("Added {name}")),
                        Err(error) => toast(
                            &ui,
                            &format!("Could not add {name}: {}", model::reason(&error)),
                        ),
                    }
                    read(&ui);
                },
            );
        },
    );
}

/// The saved Wi-Fi networks, on a page of their own.
fn open_saved(ui: &Rc<Ui>) {
    let section = Section::new("");
    section.group.set_description(Some(
        "Networks this computer has joined. Open one to change how it joins, or to forget it.",
    ));
    let content = column();
    content.append(&section.group);
    let page = page("Saved networks", Some("saved"), &content);
    *ui.saved_list.borrow_mut() = Some((page.clone(), section));
    update_saved(ui);
    ui.nav.push(&page);
}

fn update_saved(ui: &Rc<Ui>) {
    let list = ui.saved_list.borrow();
    let Some((_, section)) = list.as_ref() else {
        return;
    };
    let state = ui.state.borrow();
    let snapshot = &state.snapshot;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    let saved: Vec<(String, String, String)> = snapshot
        .saved_wifi()
        .into_iter()
        .map(|saved| {
            let mut subtitle = if saved.active {
                "Connected".to_owned()
            } else {
                model::since(now, saved.timestamp)
            };
            if !saved.autoconnect {
                subtitle.push_str(" · automatic joining off");
            }
            (saved.uuid.clone(), saved.name.clone(), subtitle)
        })
        .collect();
    section.show(format!("{saved:?}"), || {
        saved
            .iter()
            .map(|(uuid, name, subtitle)| {
                let row = next_row(name, subtitle);
                row.add_prefix(&gtk::Image::from_icon_name("network-wireless-symbolic"));
                let opened = ui.clone();
                let uuid = uuid.clone();
                row.connect_activated(move |_| details::open(&opened, &uuid));
                row.upcast()
            })
            .collect()
    });
}
