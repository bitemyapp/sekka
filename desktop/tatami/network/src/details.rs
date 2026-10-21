// SPDX-License-Identifier: MIT OR Apache-2.0
//! A connection's own page: its status and addresses while in use; how it
//! joins (automatically, metered, its private address on Wi-Fi) and its
//! password; its IPv4 addresses, DNS and IPv6; and forgetting it. As in
//! Displays, each change applies at once: to the connection in use without
//! reconnecting where NetworkManager can (`device reapply`), or the next
//! time it connects. A change that fails is undone on the page too.
use crate::{
    join,
    model::{self, Dns, Kind, Mac, Settings},
    nm,
    window::{self, Ui, background, toast},
};
use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct Page {
    pub uuid: String,
    page: adw::NavigationPage,
    status: adw::PreferencesGroup,
    action: gtk::Button,
    address_row: adw::ActionRow,
    router_row: adw::ActionRow,
    dns_row: adw::ActionRow,
    hardware_row: adw::ActionRow,
    content: gtk::Box,
    settings: RefCell<Option<Settings>>,
    /// Set while the page's widgets are filled in: their handlers then
    /// change nothing.
    updating: Cell<bool>,
}

fn info_row(title: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .use_markup(false)
        .subtitle_selectable(true)
        .css_classes(["property"])
        .build()
}

pub fn open(ui: &Rc<Ui>, uuid: &str) {
    let name = ui
        .state
        .borrow()
        .snapshot
        .connection(uuid)
        .map(|saved| saved.name.clone())
        .unwrap_or_else(|| "Connection".into());
    let status = adw::PreferencesGroup::new();
    // The status is the group's heading, with connecting or disconnecting
    // beside it.
    let action = gtk::Button::builder().valign(gtk::Align::Center).build();
    status.set_header_suffix(Some(&action));
    let address_row = info_row("IP address");
    let router_row = info_row("Router");
    let dns_row = info_row("DNS servers");
    let hardware_row = info_row("Hardware address");
    for row in [&address_row, &router_row, &dns_row, &hardware_row] {
        status.add(row);
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    let column = window::column();
    column.append(&status);
    column.append(&content);
    let tag = format!("details-{uuid}");
    let nav_page = window::page(&name, Some(tag.as_str()), &column);
    let page = Rc::new(Page {
        uuid: uuid.to_owned(),
        page: nav_page.clone(),
        status,
        action: action.clone(),
        address_row,
        router_row,
        dns_row,
        hardware_row,
        content,
        settings: RefCell::new(None),
        updating: Cell::new(false),
    });
    let clicked = (ui.clone(), page.clone());
    action.connect_clicked(move |_| connect_or_disconnect(&clicked.0, &clicked.1));
    *ui.details.borrow_mut() = Some(page.clone());
    page.update(ui);
    page.load(ui, true);
    ui.nav.push(&nav_page);
}

/// Bring the connection up or down, from the status header's button.
fn connect_or_disconnect(ui: &Rc<Ui>, page: &Rc<Page>) {
    let state = ui.state.borrow();
    let Some(saved) = state.snapshot.connection(&page.uuid).cloned() else {
        return;
    };
    let network = saved.ssid.as_ref().and_then(|ssid| {
        state
            .snapshot
            .networks
            .iter()
            .find(|network| network.ssid == *ssid)
            .cloned()
    });
    drop(state);
    if saved.kind.is_vpn() {
        return window::set_vpn(ui, &saved.uuid, &saved.name, !saved.active);
    }
    if saved.active || saved.activating {
        let (ui, uuid, name) = (ui.clone(), saved.uuid.clone(), saved.name.clone());
        let work = uuid.clone();
        return background(
            move || nm::disconnect(&work),
            move |result| {
                if let Err(error) = result {
                    toast(
                        &ui,
                        &format!("Could not disconnect {name}: {}", model::reason(&error)),
                    );
                }
                window::read(&ui);
            },
        );
    }
    match network {
        // Joining asks for a new password if the saved one is refused.
        Some(network) => join::network(ui, network),
        None => {
            let (ui, uuid, name) = (ui.clone(), saved.uuid.clone(), saved.name.clone());
            background(
                move || nm::connect(&uuid, None, None),
                move |result| {
                    if let Err(error) = result {
                        toast(
                            &ui,
                            &format!("Could not connect {name}: {}", model::reason(&error)),
                        );
                    }
                    window::read(&ui);
                },
            );
        }
    }
}

impl Page {
    /// The status rows, from the window's latest reading.
    pub fn update(&self, ui: &Ui) {
        let state = ui.state.borrow();
        let snapshot = &state.snapshot;
        let Some(saved) = snapshot.connection(&self.uuid) else {
            // Forgotten, here or elsewhere.
            if ui.nav.visible_page().as_ref() == Some(&self.page) {
                ui.nav.pop();
            }
            return;
        };
        self.page.set_title(&saved.name);
        let addresses = saved
            .device
            .as_ref()
            .filter(|_| saved.active)
            .and_then(|device| snapshot.addresses.get(device));
        let status = if saved.activating {
            "Connecting…"
        } else if saved.active {
            "Connected"
        } else {
            "Not connected"
        };
        self.status.set_title(status);
        let show = |row: &adw::ActionRow, value: String| {
            row.set_visible(!value.is_empty());
            row.set_subtitle(&value);
        };
        let addresses = addresses.cloned().unwrap_or_default();
        let mut ip = addresses.ipv4.clone();
        ip.extend(
            addresses
                .ipv6
                .iter()
                .filter(|address| !address.starts_with("fe80"))
                .cloned(),
        );
        show(&self.address_row, ip.join("\n"));
        show(&self.router_row, addresses.router.clone());
        show(&self.dns_row, addresses.dns.join("\n"));
        show(
            &self.hardware_row,
            if saved.kind.is_vpn() {
                String::new()
            } else {
                addresses.hardware.clone()
            },
        );
        // Wi-Fi out of range, or off, cannot be joined from here.
        let in_range = saved.kind != Kind::Wifi
            || (snapshot.wifi_enabled
                && saved.ssid.as_ref().is_some_and(|ssid| {
                    snapshot
                        .networks
                        .iter()
                        .any(|network| network.ssid == *ssid)
                }));
        let (label, sensitive) = if saved.active || saved.activating {
            ("Disconnect", true)
        } else if saved.kind == Kind::Wifi {
            ("Join", in_range)
        } else {
            ("Connect", true)
        };
        self.action.set_label(label);
        self.action
            .set_sensitive(sensitive && !state.busy.contains(&self.uuid));
        self.action
            .set_tooltip_text((!sensitive).then_some("Not in range"));
    }

    /// Read the connection's settings, and rebuild the settings shown when
    /// asked to, or when they are not what the page shows.
    fn load(self: &Rc<Self>, ui: &Rc<Ui>, rebuild: bool) {
        let (page, ui) = (self.clone(), ui.clone());
        let uuid = self.uuid.clone();
        background(
            move || nm::settings(&uuid),
            move |settings| match settings {
                Ok(settings) => {
                    let changed = page.settings.borrow().as_ref() != Some(&settings);
                    *page.settings.borrow_mut() = Some(settings);
                    if rebuild || changed {
                        page.build(&ui);
                    }
                }
                Err(error) => toast(&ui, &model::reason(&error)),
            },
        );
    }

    /// Change settings, then read them back: the page is rebuilt from what
    /// NetworkManager kept, so a refused change does not stay on screen.
    fn apply(self: &Rc<Self>, ui: &Rc<Ui>, what: &str, changes: Vec<(&'static str, String)>) {
        let version = self.settings.borrow().as_ref().map_or(0, |s| s.version);
        let device = {
            let state = ui.state.borrow();
            state
                .snapshot
                .connection(&self.uuid)
                .filter(|saved| saved.active)
                .and_then(|saved| saved.device.clone())
        };
        let (page, ui, what) = (self.clone(), ui.clone(), what.to_owned());
        let uuid = self.uuid.clone();
        background(
            move || nm::modify(&uuid, version, &changes, device.as_deref(), false),
            move |result| {
                if let Err(error) = &result {
                    toast(
                        &ui,
                        &format!("Could not change {what}: {}", model::reason(error)),
                    );
                    // Forget what the page shows, so it is rebuilt.
                    page.settings.borrow_mut().take();
                }
                page.load(&ui, false);
                window::read(&ui);
            },
        );
    }

    fn build(self: &Rc<Self>, ui: &Rc<Ui>) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        let Some(settings) = self.settings.borrow().clone() else {
            return;
        };
        self.updating.set(true);
        let kind = settings.kind.unwrap_or(Kind::Other);
        self.content
            .append(&self.joining_group(ui, &settings, kind));
        if matches!(kind, Kind::Wifi | Kind::Ethernet) {
            self.content.append(&self.ipv4_group(ui, &settings));
        }
        if kind != Kind::Other {
            self.content.append(&self.dns_group(ui, &settings));
        }
        if matches!(kind, Kind::Wifi | Kind::Ethernet) {
            self.content.append(&self.ipv6_group(ui, &settings));
        }
        if kind == Kind::Wifi || kind.is_vpn() {
            self.content.append(&self.forget_group(ui, &settings, kind));
        }
        self.updating.set(false);
    }

    fn joining_group(
        self: &Rc<Self>,
        ui: &Rc<Ui>,
        settings: &Settings,
        kind: Kind,
    ) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        let auto = adw::SwitchRow::builder()
            .title(if kind == Kind::Wifi {
                "Join automatically"
            } else {
                "Connect automatically"
            })
            .active(settings.autoconnect)
            .build();
        let (page, changed) = (self.clone(), ui.clone());
        auto.connect_active_notify(move |row| {
            if !page.updating.get() {
                let value = if row.is_active() { "yes" } else { "no" };
                page.apply(
                    &changed,
                    "joining",
                    vec![("connection.autoconnect", value.into())],
                );
            }
        });
        group.add(&auto);
        if matches!(kind, Kind::Wifi | Kind::Ethernet) {
            let metered = adw::SwitchRow::builder()
                .title("Metered connection")
                .subtitle("Applications may wait to download large updates on it")
                .active(settings.metered == "yes")
                .build();
            let (page, changed) = (self.clone(), ui.clone());
            metered.connect_active_notify(move |row| {
                if !page.updating.get() {
                    let value = if row.is_active() { "yes" } else { "no" };
                    page.apply(
                        &changed,
                        "metering",
                        vec![("connection.metered", value.into())],
                    );
                }
            });
            group.add(&metered);
        }
        if kind == Kind::Wifi {
            let current = Mac::of(&settings.mac);
            let mut choices: Vec<Mac> = Mac::CHOICES.to_vec();
            if matches!(current, Mac::Other(_)) {
                choices.push(current.clone());
            }
            let labels: Vec<String> = choices.iter().map(Mac::label).collect();
            let row = combo(
                "Private address",
                &labels,
                choices.iter().position(|mac| *mac == current),
            );
            row.set_subtitle("Applies the next time it joins");
            let (page, changed) = (self.clone(), ui.clone());
            row.connect_selected_notify(move |row| {
                if page.updating.get() {
                    return;
                }
                if let Some(mac) = choices.get(row.selected() as usize) {
                    page.apply(
                        &changed,
                        "the private address",
                        vec![("802-11-wireless.cloned-mac-address", mac.value().to_owned())],
                    );
                }
            });
            group.add(&row);
            let setting = match settings.key_mgmt.as_str() {
                "wpa-psk" | "sae" => Some(nm::PSK),
                "wpa-eap" => Some(nm::EAP_PASSWORD),
                _ => None,
            };
            if let Some(setting) = setting {
                let row = adw::ActionRow::builder()
                    .title("Password")
                    .subtitle("Kept by this computer")
                    .build();
                let change = gtk::Button::builder()
                    .label("Change…")
                    .valign(gtk::Align::Center)
                    .build();
                row.add_suffix(&change);
                let (page, opened) = (self.clone(), ui.clone());
                change.connect_clicked(move |_| {
                    join::change_password(&opened, &page.uuid, setting);
                });
                group.add(&row);
            }
        }
        group
    }

    fn ipv4_group(self: &Rc<Self>, ui: &Rc<Ui>, settings: &Settings) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("IPv4");
        // Methods other than these two are shown, and left as they are.
        let mut methods = vec![("auto", "Automatically (DHCP)"), ("manual", "Manually")];
        let other;
        if !["auto", "manual"].contains(&settings.ipv4_method.as_str()) {
            other = format!("Other ({})", settings.ipv4_method);
            methods.push((settings.ipv4_method.as_str(), other.as_str()));
        }
        let labels: Vec<String> = methods
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect();
        let method = combo(
            "Configure IPv4",
            &labels,
            methods
                .iter()
                .position(|(value, _)| *value == settings.ipv4_method),
        );
        let address = adw::EntryRow::builder()
            .title("IP address and prefix, as 192.168.1.20/24")
            .text(
                settings
                    .ipv4_addresses
                    .first()
                    .map(String::as_str)
                    .unwrap_or(""),
            )
            .show_apply_button(true)
            .build();
        let router = adw::EntryRow::builder()
            .title("Router")
            .text(settings.ipv4_gateway.as_str())
            .show_apply_button(true)
            .build();
        let manual = settings.ipv4_method == "manual";
        address.set_visible(manual);
        router.set_visible(manual);
        group.add(&method);
        group.add(&address);
        group.add(&router);
        let values: Vec<String> = methods
            .iter()
            .map(|(value, _)| (*value).to_owned())
            .collect();
        let (page, changed) = (self.clone(), ui.clone());
        let (shown_address, shown_router) = (address.clone(), router.clone());
        method.connect_selected_notify(move |row| {
            if page.updating.get() {
                return;
            }
            let value = values
                .get(row.selected() as usize)
                .cloned()
                .unwrap_or_default();
            let manual = value == "manual";
            shown_address.set_visible(manual);
            shown_router.set_visible(manual);
            if value == "auto" {
                page.apply(
                    &changed,
                    "IPv4",
                    vec![
                        ("ipv4.method", "auto".into()),
                        ("ipv4.addresses", String::new()),
                        ("ipv4.gateway", String::new()),
                    ],
                );
            } else if manual {
                // Manual takes effect with an address.
                shown_address.grab_focus();
            }
        });
        let (page, changed, other_router) = (self.clone(), ui.clone(), router.clone());
        address.connect_apply(move |row| {
            let Some(value) = model::address_with_prefix(&row.text()) else {
                row.add_css_class("error");
                toast(
                    &changed,
                    "Write the address with its prefix length, as 192.168.1.20/24",
                );
                return;
            };
            row.remove_css_class("error");
            let mut changes = vec![
                ("ipv4.method", "manual".to_owned()),
                ("ipv4.addresses", value),
            ];
            match model::ipv4(&other_router.text()) {
                Some(gateway) => changes.push(("ipv4.gateway", gateway)),
                // Without a router, nothing beyond this network is reached.
                None => {
                    other_router.grab_focus();
                    toast(&changed, "Now the router, to reach beyond this network");
                }
            }
            page.apply(&changed, "the IP address", changes);
        });
        let (page, changed, other_address) = (self.clone(), ui.clone(), address.clone());
        router.connect_apply(move |row| {
            let text = row.text();
            let gateway = if text.trim().is_empty() {
                Some(String::new())
            } else {
                model::ipv4(&text)
            };
            let Some(gateway) = gateway else {
                row.add_css_class("error");
                toast(&changed, "The router is an IPv4 address, as 192.168.1.1");
                return;
            };
            row.remove_css_class("error");
            if model::address_with_prefix(&other_address.text()).is_none() {
                toast(&changed, "Enter the IP address first");
                return;
            }
            page.apply(&changed, "the router", vec![("ipv4.gateway", gateway)]);
        });
        group
    }

    fn dns_group(self: &Rc<Self>, ui: &Rc<Ui>, settings: &Settings) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("DNS");
        let current = settings.dns.unwrap_or(Dns::Automatic);
        let labels: Vec<String> = Dns::ALL.iter().map(|dns| dns.name().to_owned()).collect();
        let provider = combo(
            "DNS servers",
            &labels,
            Dns::ALL.iter().position(|dns| *dns == current),
        );
        // Automatic: the servers the network gives, shown above while in use.
        provider.set_subtitle(if current == Dns::Automatic {
            "The network's own"
        } else {
            ""
        });
        let servers = adw::EntryRow::builder()
            .title("Servers, separated by spaces")
            .text(if current == Dns::Custom {
                settings.dns_servers.join(" ")
            } else {
                String::new()
            })
            .show_apply_button(true)
            .visible(current == Dns::Custom)
            .build();
        group.add(&provider);
        group.add(&servers);
        let selected_settings = settings.clone();
        let (page, changed, shown) = (self.clone(), ui.clone(), servers.clone());
        provider.connect_selected_notify(move |row| {
            if page.updating.get() {
                return;
            }
            let Some(dns) = Dns::ALL.get(row.selected() as usize).copied() else {
                return;
            };
            row.set_subtitle(if dns == Dns::Automatic {
                "The network's own"
            } else {
                ""
            });
            shown.set_visible(dns == Dns::Custom);
            if dns == Dns::Custom {
                // Custom takes effect with its servers.
                shown.grab_focus();
                return;
            }
            let (v4, v6): (Vec<&str>, Vec<&str>) = dns
                .servers()
                .iter()
                .partition(|server| !server.contains(':'));
            page.apply(
                &changed,
                "DNS",
                selected_settings.dns_changes(
                    v4.into_iter().map(str::to_owned).collect(),
                    v6.into_iter().map(str::to_owned).collect(),
                ),
            );
        });
        let (page, changed) = (self.clone(), ui.clone());
        let selected_settings = settings.clone();
        servers.connect_apply(move |row| match model::custom_servers(&row.text()) {
            Some((v4, v6)) => {
                row.remove_css_class("error");
                page.apply(&changed, "DNS", selected_settings.dns_changes(v4, v6));
            }
            None => {
                row.add_css_class("error");
                toast(
                    &changed,
                    "DNS servers are addresses, as 9.9.9.9 or 2620:fe::fe",
                );
            }
        });
        group
    }

    fn ipv6_group(self: &Rc<Self>, ui: &Rc<Ui>, settings: &Settings) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.set_title("IPv6");
        let mut methods = vec![("auto", "Automatically"), ("disabled", "Off")];
        let other;
        if !["auto", "disabled"].contains(&settings.ipv6_method.as_str()) {
            other = format!("Other ({})", settings.ipv6_method);
            methods.push((settings.ipv6_method.as_str(), other.as_str()));
        }
        let labels: Vec<String> = methods
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect();
        let method = combo(
            "Configure IPv6",
            &labels,
            methods
                .iter()
                .position(|(value, _)| *value == settings.ipv6_method),
        );
        let values: Vec<String> = methods
            .iter()
            .map(|(value, _)| (*value).to_owned())
            .collect();
        let (page, changed) = (self.clone(), ui.clone());
        let selected_settings = settings.clone();
        method.connect_selected_notify(move |row| {
            if page.updating.get() {
                return;
            }
            if let Some(value) = values.get(row.selected() as usize) {
                match selected_settings.ipv6_changes(value) {
                    Ok(changes) => page.apply(&changed, "IPv6", changes),
                    Err(error) => {
                        toast(&changed, &error);
                        page.load(&changed, true);
                    }
                }
            }
        });
        group.add(&method);
        group
    }

    fn forget_group(
        self: &Rc<Self>,
        ui: &Rc<Ui>,
        settings: &Settings,
        kind: Kind,
    ) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        let (title, heading, body) = if kind == Kind::Wifi {
            (
                "Forget this network",
                format!("Forget “{}”?", settings.name),
                "This computer stops joining it, and its password is removed. You can join it again later.",
            )
        } else {
            (
                "Remove this VPN",
                format!("Remove “{}”?", settings.name),
                "Its configuration is removed from this computer.",
            )
        };
        let row = adw::ButtonRow::new();
        row.set_title(title);
        row.add_css_class("destructive-action");
        let (page, ui) = (self.clone(), ui.clone());
        let name = settings.name.clone();
        let (verb, done) = if kind == Kind::Wifi {
            ("Forget", "Forgot")
        } else {
            ("Remove", "Removed")
        };
        row.connect_activated(move |_| {
            let parent = page.page.clone();
            let dialog = adw::AlertDialog::new(Some(&heading), Some(body));
            dialog.add_responses(&[("cancel", "Cancel"), ("forget", verb)]);
            dialog.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
            dialog.set_close_response("cancel");
            dialog.set_default_response(Some("cancel"));
            let (page, ui, name) = (page.clone(), ui.clone(), name.clone());
            dialog.connect_response(None, move |_, response| {
                if response != "forget" {
                    return;
                }
                let (ui, name) = (ui.clone(), name.clone());
                let uuid = page.uuid.clone();
                background(
                    move || nm::forget(&uuid),
                    move |result| {
                        match result {
                            Ok(()) => {
                                ui.nav.pop();
                                toast(&ui, &format!("{done} {name}"));
                            }
                            Err(error) => toast(
                                &ui,
                                &format!(
                                    "Could not {} {name}: {}",
                                    verb.to_lowercase(),
                                    model::reason(&error)
                                ),
                            ),
                        }
                        window::read(&ui);
                    },
                );
            });
            dialog.present(Some(&parent));
        });
        group.add(&row);
        group
    }
}

fn combo(title: &str, labels: &[String], selected: Option<usize>) -> adw::ComboRow {
    let strings: Vec<&str> = labels.iter().map(String::as_str).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&strings))
        .build();
    if let Some(selected) = selected {
        row.set_selected(selected as u32);
    }
    row
}
