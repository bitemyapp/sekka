// SPDX-License-Identifier: MIT OR Apache-2.0
//! Joining Wi-Fi networks: a saved one, an open one, one with a password
//! (asked for in a dialog), an enterprise one (an organization's username
//! and password, and how to check its server), or a hidden one by name.
//! Each runs on a worker thread with a spinner on the network's row, and
//! says how it went in a toast. When a saved password is refused, the
//! password is asked for again rather than reported as a failure.
use crate::{
    details,
    model::{self, Network, Security},
    nm,
    window::{self, Ui, background, toast},
};
use adw::prelude::*;
use std::rc::Rc;

/// What to do when a saved password is refused.
type Retry = Box<dyn FnOnce(&Rc<Ui>)>;

fn wifi_device(ui: &Ui) -> Option<String> {
    ui.state
        .borrow()
        .snapshot
        .wifi_device()
        .map(|device| device.name.clone())
}

/// The shortest password a network's key management accepts.
fn minimum(setting: &str, key_mgmt: &str) -> usize {
    if setting == nm::PSK && key_mgmt != "sae" {
        8
    } else {
        1
    }
}

/// Whether another network is being joined, said in a toast. One join runs
/// at a time: nmcli answers a network's request for its password from one
/// joining process only, so a second would be refused its password.
fn busy(ui: &Ui) -> bool {
    let joining = ui.state.borrow().joining.clone();
    if let Some(joining) = &joining {
        toast(ui, &format!("Still joining {joining}"));
    }
    joining.is_some()
}

/// Join a network in range, from its row.
pub fn network(ui: &Rc<Ui>, network: Network) {
    let saved = ui.state.borrow().snapshot.saved_for(&network.ssid).cloned();
    if network.in_use {
        if let Some(saved) = saved {
            details::open(ui, &saved.uuid);
        }
        return;
    }
    if busy(ui) {
        return;
    }
    let device = wifi_device(ui);
    let ssid = network.ssid.clone();
    match (saved, network.security()) {
        (Some(saved), security) => {
            let uuid = saved.uuid.clone();
            // A refused password: ask for it, and join with the new one.
            let setting = match security {
                Security::Personal(_) => Some(nm::PSK),
                Security::Enterprise => Some(nm::EAP_PASSWORD),
                Security::Open | Security::Wep => None,
            };
            let retry = setting.map(|setting| {
                let key_mgmt = saved.security.clone().unwrap_or_default();
                let (uuid, ssid, device) = (uuid.clone(), ssid.clone(), device.clone());
                Box::new(move |ui: &Rc<Ui>| {
                    let (uuid, ssid, device, ui2) =
                        (uuid.clone(), ssid.clone(), device.clone(), ui.clone());
                    ask_password(
                        ui,
                        format!("Password for “{ssid}”"),
                        format!(
                            "“{ssid}” did not accept the saved password. Enter it again to join."
                        ),
                        minimum(setting, &key_mgmt),
                        move |password| {
                            let (uuid, device) = (uuid.clone(), device.clone());
                            run(
                                &ui2,
                                ssid.clone(),
                                move || nm::rejoin(&uuid, setting, &password, device.as_deref()),
                                None,
                            );
                        },
                    );
                }) as Retry
            });
            run(
                ui,
                ssid,
                move || nm::connect(&uuid, device.as_deref(), None),
                retry,
            );
        }
        (None, Security::Open) => run(
            ui,
            ssid.clone(),
            move || nm::join(&ssid, false, &nm::Join::Open, None, device.as_deref()),
            None,
        ),
        (None, Security::Personal(key_mgmt)) => {
            let joined = ui.clone();
            ask_password(
                ui,
                format!("Join “{ssid}”"),
                format!("Enter the password for “{ssid}”."),
                minimum(nm::PSK, key_mgmt),
                move |password| {
                    let (ssid, device) = (ssid.clone(), device.clone());
                    run(
                        &joined,
                        ssid.clone(),
                        move || {
                            nm::join(
                                &ssid,
                                false,
                                &nm::Join::Personal { key_mgmt },
                                Some(&password),
                                device.as_deref(),
                            )
                        },
                        None,
                    );
                },
            );
        }
        (None, Security::Enterprise) => form_for(ui, Some(ssid), false),
        (None, Security::Wep) => toast(
            ui,
            &format!("{ssid} uses WEP, which anyone nearby can break, so it is not offered"),
        ),
    }
}

/// Join a network by its name: a hidden one, or one not in range yet.
pub fn by_name(ui: &Rc<Ui>, name: Option<String>) {
    if !busy(ui) {
        form_for(ui, name, true);
    }
}

/// Change a saved network's password by joining with the new one.
pub fn change_password(ui: &Rc<Ui>, uuid: &str, setting: &'static str) {
    if busy(ui) {
        return;
    }
    let (saved, in_range) = {
        let state = ui.state.borrow();
        let saved = state.snapshot.connection(uuid).cloned();
        let in_range = saved
            .as_ref()
            .and_then(|saved| saved.ssid.as_ref())
            .is_some_and(|ssid| state.snapshot.networks.iter().any(|n| n.ssid == *ssid));
        (saved, in_range)
    };
    let Some(saved) = saved else {
        return;
    };
    let ssid = saved.ssid.clone().unwrap_or(saved.name.clone());
    if !in_range {
        return toast(
            ui,
            &format!("{ssid} is not in range: its password is changed by joining it"),
        );
    }
    let device = wifi_device(ui);
    let (joined, uuid) = (ui.clone(), saved.uuid.clone());
    ask_password(
        ui,
        format!("New password for “{ssid}”"),
        "Joining with it replaces the saved password.".into(),
        minimum(setting, saved.security.as_deref().unwrap_or("")),
        move |password| {
            let (uuid, device) = (uuid.clone(), device.clone());
            run(
                &joined,
                ssid.clone(),
                move || nm::rejoin(&uuid, setting, &password, device.as_deref()),
                None,
            );
        },
    );
}

/// Join on a worker thread, the network's row showing a spinner meanwhile.
fn run(
    ui: &Rc<Ui>,
    ssid: String,
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
    retry: Option<Retry>,
) {
    // A dialog answered after another join began.
    if busy(ui) {
        return;
    }
    ui.state.borrow_mut().joining = Some(ssid.clone());
    window::redraw(ui);
    let ui = ui.clone();
    background(work, move |result| {
        ui.state.borrow_mut().joining = None;
        match result {
            Ok(()) => toast(&ui, &format!("Connected to {ssid}")),
            Err(error) => match retry {
                Some(retry) if model::refused(&error) => retry(&ui),
                _ => toast(
                    &ui,
                    &format!("Could not join {ssid}: {}", model::reason(&error)),
                ),
            },
        }
        window::redraw(&ui);
        window::read(&ui);
    });
}

/// A password, asked for in a dialog; `then` gets it when Join is chosen.
fn ask_password(
    ui: &Ui,
    heading: String,
    body: String,
    minimum: usize,
    then: impl Fn(String) + 'static,
) {
    let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
    let entry = adw::PasswordEntryRow::builder()
        .title("Password")
        .activates_default(true)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&entry);
    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[("cancel", "Cancel"), ("join", "Join")]);
    dialog.set_response_appearance("join", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("join"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("join", false);
    let enabled = dialog.clone();
    entry.connect_changed(move |entry| {
        enabled.set_response_enabled("join", entry.text().chars().count() >= minimum);
    });
    let typed = entry.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "join" {
            then(typed.text().to_string());
        }
    });
    dialog.present(Some(&ui.window));
    dialog.set_focus(Some(&entry));
}

const SECURITY: [(&str, Option<&str>); 4] = [
    ("None", None),
    ("WPA/WPA2 Personal", Some("wpa-psk")),
    ("WPA3 Personal", Some("sae")),
    ("Enterprise (802.1X)", None),
];
const ENTERPRISE: usize = 3;

/// The dialog for an enterprise network, or for a network joined by name:
/// its name and security when hidden, then a password, or an
/// organization's username, password and how to check its server.
fn form_for(ui: &Rc<Ui>, ssid: Option<String>, hidden: bool) {
    let title = match (&ssid, hidden) {
        (Some(ssid), false) => format!("Join “{ssid}”"),
        _ => "Join a network by name".to_owned(),
    };
    let join = gtk::Button::builder()
        .label("Join")
        .css_classes(["suggested-action"])
        .sensitive(false)
        .build();
    let cancel = gtk::Button::with_label("Cancel");
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&join);
    let page = adw::PreferencesPage::new();

    // The network: its name and security, when joined by name.
    let network = adw::PreferencesGroup::new();
    let name = adw::EntryRow::builder()
        .title("Network name")
        .text(ssid.as_deref().unwrap_or(""))
        .build();
    let labels: Vec<&str> = SECURITY.iter().map(|(label, _)| *label).collect();
    let security = adw::ComboRow::builder()
        .title("Security")
        .model(&gtk::StringList::new(&labels))
        .selected(if hidden { 1 } else { ENTERPRISE as u32 })
        .build();
    network.add(&name);
    network.add(&security);
    network.set_visible(hidden);
    page.add(&network);

    // Signing in: a password, or a username and password.
    let sign_in = adw::PreferencesGroup::new();
    let methods = gtk::StringList::new(&["PEAP", "TTLS"]);
    let method = adw::ComboRow::builder()
        .title("Method")
        .model(&methods)
        .build();
    let username = adw::EntryRow::builder().title("Username").build();
    let password = adw::PasswordEntryRow::builder()
        .title("Password")
        .activates_default(true)
        .build();
    sign_in.add(&method);
    sign_in.add(&username);
    sign_in.add(&password);
    page.add(&sign_in);

    // How the server is checked.
    let server = adw::PreferencesGroup::new();
    server.set_title("Server");
    server.set_description(Some(
        "Checking the server's certificate keeps your password from a look-alike network.",
    ));
    let validate = adw::SwitchRow::builder()
        .title("Check the server's certificate")
        .subtitle("With this computer's certificate authorities")
        .active(true)
        .build();
    let domain = adw::EntryRow::builder()
        .title("Server domain, as radius.example.org")
        .build();
    server.add(&validate);
    server.add(&domain);
    page.add(&server);
    let advanced = adw::PreferencesGroup::new();
    advanced.set_title("Advanced");
    let inner = adw::ComboRow::builder()
        .title("Inner authentication")
        .model(&gtk::StringList::new(&["MSCHAPv2", "PAP"]))
        .build();
    let anonymous = adw::EntryRow::builder().title("Anonymous identity").build();
    advanced.add(&inner);
    advanced.add(&anonymous);
    page.add(&advanced);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&page));
    let dialog = adw::Dialog::builder()
        .title(title.as_str())
        .content_width(480)
        .content_height(if hidden { 420 } else { 700 })
        .child(&view)
        .default_widget(&join)
        .build();

    // Show what the chosen security asks for, and allow joining once it is
    // given.
    let refresh = {
        let (security, method, username, password, server, advanced, domain, validate, name, join) = (
            security.clone(),
            method.clone(),
            username.clone(),
            password.clone(),
            server.clone(),
            advanced.clone(),
            domain.clone(),
            validate.clone(),
            name.clone(),
            join.clone(),
        );
        move || {
            let choice = security.selected() as usize;
            let enterprise = choice == ENTERPRISE;
            let personal = SECURITY.get(choice).and_then(|(_, key)| *key);
            method.set_visible(enterprise);
            username.set_visible(enterprise);
            password.set_visible(enterprise || personal.is_some());
            server.set_visible(enterprise);
            advanced.set_visible(enterprise);
            domain.set_visible(validate.is_active());
            let length = password.text().chars().count();
            let ready = !name.text().trim().is_empty()
                && match personal {
                    Some(key) => length >= minimum(nm::PSK, key),
                    None if enterprise => !username.text().trim().is_empty() && length > 0,
                    None => true,
                };
            join.set_sensitive(ready);
        }
    };
    refresh();
    let refresh = Rc::new(refresh);
    for entry in [&name, &username] {
        let refresh = refresh.clone();
        entry.connect_changed(move |_| refresh());
    }
    let changed = refresh.clone();
    password.connect_changed(move |_| changed());
    let changed = refresh.clone();
    security.connect_selected_notify(move |_| changed());
    let changed = refresh.clone();
    validate.connect_active_notify(move |_| changed());

    // The first thing still to fill in: the name, then the username of an
    // enterprise network or the password of another.
    let focus: gtk::Widget = if name.text().is_empty() {
        name.clone().upcast()
    } else if security.selected() as usize == ENTERPRISE {
        username.clone().upcast()
    } else {
        password.clone().upcast()
    };
    let closed = dialog.clone();
    cancel.connect_clicked(move |_| {
        closed.close();
    });
    let (joined, closed) = (ui.clone(), dialog.clone());
    join.connect_clicked(move |_| {
        let ssid = name.text().trim().to_owned();
        let choice = security.selected() as usize;
        let how = if choice == ENTERPRISE {
            nm::Join::Enterprise(nm::Enterprise {
                method: ["peap", "ttls"][method.selected().min(1) as usize].into(),
                inner: ["mschapv2", "pap"][inner.selected().min(1) as usize].into(),
                identity: username.text().trim().to_owned(),
                anonymous: anonymous.text().trim().to_owned(),
                domain: domain.text().trim().to_owned(),
                validate: validate.is_active(),
            })
        } else if let Some(key_mgmt) = SECURITY.get(choice).and_then(|(_, key)| *key) {
            nm::Join::Personal { key_mgmt }
        } else {
            nm::Join::Open
        };
        let secret = (!matches!(how, nm::Join::Open)).then(|| password.text().to_string());
        let device = wifi_device(&joined);
        closed.close();
        run(
            &joined,
            ssid.clone(),
            move || nm::join(&ssid, hidden, &how, secret.as_deref(), device.as_deref()),
            None,
        );
    });
    dialog.present(Some(&ui.window));
    dialog.set_focus(Some(&focus));
}
