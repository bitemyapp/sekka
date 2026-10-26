// SPDX-License-Identifier: MIT OR Apache-2.0
//! A native settings window, styled and sized like Tatami's Network window.
//! GIO futures keep all bus waits off the GTK event loop. Reads are serialized;
//! stale sessions and reads started before a change cannot overwrite its result.
use crate::{
    agent,
    bluez::{self, Bluez},
    model::{self, ADAPTER, DEVICE, Device, Snapshot},
};
use adw::prelude::*;
use glib::variant::ToVariant;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

pub struct Ui {
    pub window: adw::ApplicationWindow,
    pub backend: RefCell<Option<Rc<Bluez>>>,
    pub snapshot: RefCell<Snapshot>,
    pub pairing: RefCell<Option<String>>,
    pub prompt: RefCell<Option<Rc<agent::Prompt>>>,
    power: adw::SwitchRow,
    adapters: adw::ComboRow,
    known: adw::PreferencesGroup,
    nearby: adw::PreferencesGroup,
    empty_known: adw::ActionRow,
    empty_nearby: adw::ActionRow,
    status: gtk::Label,
    spinner: gtk::Spinner,
    cancel: gtk::Button,
    retry: gtk::Button,
    toast: adw::ToastOverlay,
    rows: RefCell<HashMap<String, DeviceRow>>,
    selected: RefCell<String>,
    detail: RefCell<Option<Rc<Detail>>>,
    busy: Cell<bool>,
    reading: Cell<bool>,
    updating: Cell<bool>,
    closing: Cell<bool>,
    revision: Cell<u64>,
    pairing_cancelled: Cell<bool>,
}

struct DeviceRow {
    row: adw::ActionRow,
    action: gtk::Button,
    details: gtk::Button,
    paired: bool,
}

struct Detail {
    path: String,
    dialog: adw::Dialog,
    name: adw::EntryRow,
    status: adw::ActionRow,
    trust: adw::SwitchRow,
    blocked: adw::SwitchRow,
    save: gtk::Button,
    forget: gtk::Button,
}

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

pub fn open(app: &adw::Application) {
    if let Some(ui) = UI.with_borrow(Clone::clone) {
        ui.window.present();
        return;
    }
    let ui = build(app);
    UI.with_borrow_mut(|slot| *slot = Some(ui.clone()));
    ui.window.present();
    let weak = Rc::downgrade(&ui);
    glib::timeout_add_seconds_local(3, move || {
        let Some(ui) = weak.upgrade().filter(|ui| !ui.closing.get()) else {
            return glib::ControlFlow::Break;
        };
        refresh(&ui);
        glib::ControlFlow::Continue
    });
    glib::spawn_future_local(async move {
        let connection = match gio::bus_get_future(gio::BusType::System).await {
            Ok(c) => c,
            Err(e) => {
                ui.status
                    .set_text(&format!("Bluetooth is unavailable: {e}"));
                return;
            }
        };
        watch(&ui, &connection);
    });
}

fn watch(ui: &Rc<Ui>, connection: &gio::DBusConnection) {
    if let Err(e) = agent::export(ui, connection) {
        ui.status
            .set_text(&format!("Could not prepare pairing: {e}"));
        return;
    }
    let appeared = Rc::downgrade(ui);
    let vanished = Rc::downgrade(ui);
    gio::bus_watch_name_on_connection(
        connection,
        "org.bluez",
        gio::BusNameWatcherFlags::NONE,
        move |connection, _, owner| {
            let Some(ui) = appeared.upgrade().filter(|ui| !ui.closing.get()) else {
                return;
            };
            *ui.backend.borrow_mut() = Some(Rc::new(Bluez::new(connection, owner)));
            ui.status.set_text("Loading Bluetooth devices…");
            refresh(&ui);
        },
        move |_, _| {
            let Some(ui) = vanished.upgrade() else {
                return;
            };
            ui.backend.borrow_mut().take();
            ui.pairing.borrow_mut().take();
            agent::close(&ui, None);
            *ui.snapshot.borrow_mut() = Snapshot::default();
            ui.busy.set(false);
            ui.status
                .set_text("Bluetooth is unavailable. Waiting for the Bluetooth service…");
            render(&ui);
        },
    );
}

fn column() -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 24);
    column.set_margin_top(18);
    column.set_margin_bottom(24);
    column.set_margin_start(18);
    column.set_margin_end(18);
    column
}

fn row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .build()
}

fn build(app: &adw::Application) -> Rc<Ui> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Bluetooth")
        .icon_name("bluetooth")
        .default_width(720)
        .default_height(760)
        .build();
    let power = adw::SwitchRow::builder()
        .title("Bluetooth")
        .use_markup(false)
        .sensitive(false)
        .build();
    power.add_prefix(&gtk::Image::from_icon_name("bluetooth-symbolic"));
    let adapters = adw::ComboRow::builder()
        .title("Adapter")
        .use_markup(false)
        .visible(false)
        .build();
    let radio = adw::PreferencesGroup::new();
    radio.add(&power);
    radio.add(&adapters);
    let known = adw::PreferencesGroup::builder().title("My devices").build();
    let nearby = adw::PreferencesGroup::builder()
        .title("Nearby devices")
        .description("Put a device in pairing mode to connect it to this computer.")
        .build();
    let empty_known = row("No paired devices", "Devices you pair will appear here.");
    let empty_nearby = row(
        "Looking for devices…",
        "Keep the device nearby and in pairing mode.",
    );
    known.add(&empty_known);
    nearby.add(&empty_nearby);
    let spinner = gtk::Spinner::new();
    let cancel = gtk::Button::with_label("Cancel Pairing");
    cancel.set_visible(false);
    let retry = gtk::Button::with_label("Try Again");
    retry.set_visible(false);
    let status = gtk::Label::builder()
        .label("Loading Bluetooth devices…")
        .wrap(true)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["dim-label"])
        .build();
    let progress = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    progress.append(&spinner);
    progress.append(&status);
    progress.append(&cancel);
    progress.append(&retry);
    let content = column();
    content.append(&radio);
    content.append(&progress);
    content.append(&known);
    content.append(&nearby);
    let clamp = adw::Clamp::builder()
        .maximum_size(640)
        .child(&content)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&scroll));
    let toast = adw::ToastOverlay::new();
    toast.set_child(Some(&view));
    window.set_content(Some(&toast));
    let ui = Rc::new(Ui {
        window,
        backend: RefCell::new(None),
        snapshot: RefCell::new(Snapshot::default()),
        pairing: RefCell::new(None),
        prompt: RefCell::new(None),
        power,
        adapters,
        known,
        nearby,
        empty_known,
        empty_nearby,
        status,
        spinner,
        cancel,
        retry,
        toast,
        rows: RefCell::new(HashMap::new()),
        selected: RefCell::new(String::new()),
        detail: RefCell::new(None),
        busy: Cell::new(false),
        reading: Cell::new(false),
        updating: Cell::new(false),
        closing: Cell::new(false),
        revision: Cell::new(0),
        pairing_cancelled: Cell::new(false),
    });
    let weak = Rc::downgrade(&ui);
    ui.power.connect_active_notify(move |switch| {
        let Some(ui) = weak.upgrade().filter(|u| !u.updating.get()) else {
            return;
        };
        let adapter = ui.snapshot.borrow().adapter(&ui.selected.borrow()).cloned();
        if let Some(a) = adapter {
            act(
                &ui,
                Action::Set(a.path, ADAPTER, "Powered", switch.is_active().to_variant()),
                "Changing Bluetooth power…",
            );
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.adapters.connect_selected_notify(move |combo| {
        let Some(ui) = weak.upgrade().filter(|u| !u.updating.get()) else {
            return;
        };
        let adapter = ui
            .snapshot
            .borrow()
            .adapters
            .get(combo.selected() as usize)
            .cloned();
        if let Some(a) = adapter {
            *ui.selected.borrow_mut() = a.path;
            render(&ui);
            refresh(&ui);
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.cancel.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            cancel_pairing(&ui);
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.retry.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.window.connect_close_request(move |_| {
        let Some(ui) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if ui.closing.replace(true) {
            return glib::Propagation::Stop;
        }
        ui.window.set_visible(false);
        agent::close(&ui, None);
        let backend = ui.backend.borrow().clone();
        if let Some(b) = &backend {
            b.stopped.set(true);
        }
        let app = ui.window.application().unwrap();
        let hold = app.hold();
        glib::spawn_future_local(async move {
            if let Some(b) = backend {
                let pairing = ui.pairing.borrow().clone();
                if let Some(path) = pairing {
                    let _ = b.device(&path, "CancelPairing").await;
                }
                // A StartDiscovery already in flight must finish before StopDiscovery.
                while ui.reading.get() {
                    glib::timeout_future(Duration::from_millis(20)).await;
                }
                b.stop().await;
            }
            ui.window.destroy();
            UI.with_borrow_mut(|slot| *slot = None);
            drop(hold);
        });
        glib::Propagation::Stop
    });
    ui
}

fn current(ui: &Ui, backend: &Rc<Bluez>) -> bool {
    !ui.closing.get()
        && ui
            .backend
            .borrow()
            .as_ref()
            .is_some_and(|b| Rc::ptr_eq(b, backend))
}

pub fn refresh(ui: &Rc<Ui>) {
    if ui.closing.get() || ui.reading.get() {
        return;
    }
    let Some(backend) = ui.backend.borrow().clone() else {
        return;
    };
    ui.reading.set(true);
    let revision = ui.revision.get();
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let result = backend.read().await;
        if current(&ui, &backend) {
            match result {
                Ok(snapshot) if revision == ui.revision.get() => {
                    backend.observe(&snapshot);
                    *ui.snapshot.borrow_mut() = snapshot;
                    let adapter = ui.snapshot.borrow().adapter(&ui.selected.borrow()).cloned();
                    if let Some(a) = &adapter {
                        *ui.selected.borrow_mut() = a.path.clone();
                    }
                    if !backend.agent_ready.get()
                        && let Err(e) = backend.register().await
                    {
                        notify(
                            &ui,
                            &format!("Pairing is unavailable: {}", model::reason(&e)),
                        );
                    }
                    let scan = adapter
                        .as_ref()
                        .filter(|a| a.powered)
                        .map(|a| a.path.as_str());
                    let scanned = backend.scan(scan).await;
                    if current(&ui, &backend) && revision == ui.revision.get() {
                        ui.retry
                            .set_visible(scanned.is_err() || !backend.agent_ready.get());
                        if !ui.busy.get() {
                            let status = match scanned {
                                Err(e) => format!("Could not look for devices: {}", model::reason(&e)),
                                Ok(()) => match adapter {
                                    None => "No Bluetooth adapter found. Connect an adapter to get started.".into(),
                                    Some(a) if a.blocked => "Bluetooth is blocked. Check airplane mode or the computer’s wireless switch.".into(),
                                    Some(a) if !a.powered => "Bluetooth is off.".into(),
                                    Some(_) if !backend.agent_ready.get() => "Devices are available, but pairing could not be prepared. Try again.".into(),
                                    Some(_) => "Looking for nearby devices…".into(),
                                },
                            };
                            ui.status.set_text(&status);
                        }
                        render(&ui);
                    }
                }
                Ok(_) => (),
                Err(e) => {
                    ui.status.set_text(&format!(
                        "Could not read Bluetooth devices: {}",
                        model::reason(&e)
                    ));
                    ui.retry.set_visible(true);
                    // Do not offer actions against a stale snapshot.
                    *ui.snapshot.borrow_mut() = Snapshot::default();
                    render(&ui);
                }
            }
        }
        ui.reading.set(false);
        if !ui.closing.get() && (revision != ui.revision.get() || !current(&ui, &backend)) {
            refresh(&ui);
        }
    });
}

fn render(ui: &Rc<Ui>) {
    ui.updating.set(true);
    let snapshot = ui.snapshot.borrow().clone();
    let adapter = snapshot.adapter(&ui.selected.borrow());
    let powered = adapter.is_some_and(|a| a.powered);
    ui.power.set_sensitive(adapter.is_some() && !ui.busy.get());
    ui.power.set_active(powered);
    ui.power.set_subtitle(
        &adapter
            .map(|a| format!("{} · {}", a.name, a.address))
            .unwrap_or_default(),
    );
    ui.adapters.set_visible(snapshot.adapters.len() > 1);
    ui.adapters.set_sensitive(!ui.busy.get());
    let labels: Vec<String> = snapshot
        .adapters
        .iter()
        .map(|a| format!("{} · {}", a.name, a.address))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    // Replacing the combo model while open loses selection; only do so on a
    // real adapter change, not every poll.
    let old: Vec<String> = ui
        .adapters
        .model()
        .map(|m| {
            (0..m.n_items())
                .filter_map(|i| {
                    m.item(i)?
                        .downcast::<gtk::StringObject>()
                        .ok()
                        .map(|s| s.string().to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    if old.iter().map(String::as_str).collect::<Vec<_>>() != labels {
        ui.adapters.set_model(Some(&gtk::StringList::new(&labels)));
    }
    if let Some(a) = adapter {
        ui.adapters.set_selected(
            snapshot
                .adapters
                .iter()
                .position(|v| v.path == a.path)
                .unwrap() as u32,
        );
    }
    let devices: Vec<_> = snapshot
        .devices
        .iter()
        .filter(|d| adapter.is_some_and(|a| a.path == d.adapter))
        .collect();
    let mut rows = ui.rows.borrow_mut();
    rows.retain(|path, row| {
        if devices
            .iter()
            .any(|d| d.path == *path && d.paired == row.paired)
        {
            return true;
        }
        if row.paired {
            ui.known.remove(&row.row);
        } else {
            ui.nearby.remove(&row.row);
        }
        false
    });
    for device in &devices {
        let entry = rows
            .entry(device.path.clone())
            .or_insert_with(|| device_row(ui, device));
        entry.row.set_title(&device.name);
        entry.row.set_subtitle(&device.status());
        entry.action.set_label(if device.paired {
            if device.connected {
                "Disconnect"
            } else {
                "Connect"
            }
        } else {
            "Pair"
        });
        let ready = ui
            .backend
            .borrow()
            .as_ref()
            .is_some_and(|b| b.agent_ready.get());
        entry.action.set_sensitive(
            powered && !device.blocked && !ui.busy.get() && (device.paired || ready),
        );
        entry.details.set_sensitive(!ui.busy.get());
    }
    ui.empty_known
        .set_visible(!devices.iter().any(|d| d.paired));
    ui.empty_nearby
        .set_visible(!devices.iter().any(|d| !d.paired) || !powered);
    ui.empty_nearby.set_title(if powered {
        "No devices found yet"
    } else {
        "Bluetooth is off"
    });
    ui.empty_nearby.set_subtitle(if powered {
        "Keep the device nearby and in pairing mode."
    } else {
        "Turn on Bluetooth to look for nearby devices."
    });
    for entry in rows.values().filter(|r| !r.paired) {
        entry.row.set_visible(powered);
    }
    ui.nearby.set_visible(adapter.is_some());
    ui.known.set_visible(adapter.is_some());
    let scanning = ui.backend.borrow().as_ref().is_some_and(|b| b.scanning());
    ui.spinner.set_spinning(ui.busy.get() || scanning);
    ui.spinner.set_visible(ui.busy.get() || scanning);
    ui.cancel.set_visible(ui.pairing.borrow().is_some());
    ui.cancel.set_sensitive(true);
    let detail = ui.detail.borrow().clone();
    if let Some(detail) = detail {
        if let Some(device) = snapshot.devices.iter().find(|d| d.path == detail.path) {
            detail.status.set_subtitle(&device.status());
            detail.trust.set_active(device.trusted);
            detail.blocked.set_active(device.blocked);
            for w in [
                detail.trust.clone().upcast::<gtk::Widget>(),
                detail.blocked.clone().upcast(),
                detail.save.clone().upcast(),
                detail.forget.clone().upcast(),
            ] {
                w.set_sensitive(!ui.busy.get());
            }
        } else {
            detail.dialog.force_close();
        }
    }
    ui.updating.set(false);
}

fn device_row(ui: &Rc<Ui>, device: &Device) -> DeviceRow {
    let row = row(&device.name, &device.status());
    row.add_prefix(&gtk::Image::from_icon_name(device.icon()));
    let action = gtk::Button::builder().valign(gtk::Align::Center).build();
    let details = gtk::Button::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text(format!("Settings for {}", device.name))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    row.add_suffix(&action);
    row.add_suffix(&details);
    let path = device.path.clone();
    let weak = Rc::downgrade(ui);
    action.connect_clicked(move |_| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let device = ui
            .snapshot
            .borrow()
            .devices
            .iter()
            .find(|d| d.path == path)
            .cloned();
        if let Some(d) = device {
            let method = if d.paired {
                if d.connected { "Disconnect" } else { "Connect" }
            } else {
                "Pair"
            };
            act(
                &ui,
                Action::Device(path.clone(), method),
                &format!(
                    "{} “{}”…",
                    match method {
                        "Pair" => "Pairing with",
                        "Connect" => "Connecting to",
                        _ => "Disconnecting",
                    },
                    d.name
                ),
            );
        }
    });
    let path = device.path.clone();
    let weak = Rc::downgrade(ui);
    details.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            detail(&ui, &path);
        }
    });
    if device.paired {
        ui.known.add(&row);
    } else {
        ui.nearby.add(&row);
    }
    DeviceRow {
        row,
        action,
        details,
        paired: device.paired,
    }
}

#[derive(Clone)]
enum Action {
    Device(String, &'static str),
    Set(String, &'static str, &'static str, glib::Variant),
    Forget(String, String),
}

fn act(ui: &Rc<Ui>, action: Action, message: &str) {
    if ui.busy.get() || ui.closing.get() {
        return;
    }
    let Some(backend) = ui.backend.borrow().clone() else {
        return;
    };
    ui.busy.set(true);
    ui.revision.set(ui.revision.get() + 1);
    if let Action::Device(path, "Pair") = &action {
        ui.pairing_cancelled.set(false);
        *ui.pairing.borrow_mut() = Some(path.clone());
    }
    ui.status.set_text(message);
    render(ui);
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let result: bluez::Result<()> = async {
            match &action {
                Action::Device(path, method) => {
                    backend.device(path, method).await?;
                    if *method == "Pair" {
                        if !current(&ui, &backend) {
                            return Ok(());
                        }
                        agent::close(&ui, Some(path));
                        ui.pairing.borrow_mut().take();
                        render(&ui);
                        // Pairing and connecting are separate stages. A paired
                        // device remains saved if its profiles fail to connect.
                        if ui.pairing_cancelled.get() {
                            return Ok(());
                        }
                        ui.status.set_text("Paired. Connecting to the device…");
                        backend.device(path, "Connect").await.map_err(|e| {
                            format!("Paired, but could not connect: {}", model::reason(&e))
                        })?;
                    }
                    Ok(())
                }
                Action::Set(path, interface, property, value) => {
                    backend.set(path, interface, property, value.clone()).await
                }
                Action::Forget(adapter, device) => backend.remove(adapter, device).await,
            }
        }
        .await;
        if !current(&ui, &backend) {
            return;
        }
        if let Action::Device(path, "Pair") = &action {
            agent::close(&ui, Some(path));
            ui.pairing.borrow_mut().take();
        }
        ui.busy.set(false);
        ui.revision.set(ui.revision.get() + 1);
        match result {
            Ok(()) => ui.status.set_text("Bluetooth settings updated."),
            Err(e) => {
                let reason = model::reason(&e);
                ui.status.set_text(&reason);
                notify(&ui, &reason);
            }
        }
        // Render from the confirmed reading, never optimistically claim success.
        refresh(&ui);
    });
}

pub fn cancel_pairing(ui: &Rc<Ui>) {
    ui.pairing_cancelled.set(true);
    agent::close(ui, None);
    let path = ui.pairing.borrow().clone();
    let backend = ui.backend.borrow().clone();
    if let (Some(path), Some(backend)) = (path, backend) {
        ui.cancel.set_sensitive(false);
        let ui = ui.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = backend.device(&path, "CancelPairing").await
                && current(&ui, &backend)
            {
                notify(&ui, &model::reason(&e));
            }
        });
    }
}

fn notify(ui: &Ui, message: &str) {
    let toast = adw::Toast::new(message);
    toast.set_use_markup(false);
    toast.set_timeout(8);
    ui.toast.add_toast(toast);
}

fn detail(ui: &Rc<Ui>, path: &str) {
    let Some(device) = ui
        .snapshot
        .borrow()
        .devices
        .iter()
        .find(|d| d.path == path)
        .cloned()
    else {
        return;
    };
    let dialog = adw::Dialog::builder()
        .title(&device.name)
        .content_width(480)
        .build();
    let content = column();
    let properties = adw::PreferencesGroup::new();
    let name = adw::EntryRow::builder()
        .title("Name on this computer")
        .text(&device.name)
        .build();
    let save = gtk::Button::builder()
        .label("Save")
        .valign(gtk::Align::Center)
        .build();
    name.add_suffix(&save);
    let status = row("Status", &device.status());
    let address = row("Bluetooth address", &device.address);
    address.set_subtitle_selectable(true);
    properties.add(&name);
    properties.add(&status);
    properties.add(&address);
    let permissions = adw::PreferencesGroup::new();
    let trust = adw::SwitchRow::builder()
        .title("Allow automatic connections")
        .subtitle("Trust this device to connect without asking again.")
        .active(device.trusted)
        .build();
    let blocked = adw::SwitchRow::builder()
        .title("Block device")
        .subtitle("Prevent this device from connecting.")
        .active(device.blocked)
        .build();
    permissions.add(&trust);
    permissions.add(&blocked);
    let forget = gtk::Button::builder()
        .label(if device.paired {
            "Forget Device…"
        } else {
            "Remove Device…"
        })
        .css_classes(["destructive-action"])
        .halign(gtk::Align::Center)
        .build();
    content.append(&properties);
    content.append(&permissions);
    content.append(&forget);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&content));
    dialog.set_child(Some(&view));
    let detail = Rc::new(Detail {
        path: path.into(),
        dialog,
        name,
        status,
        trust,
        blocked,
        save,
        forget,
    });
    *ui.detail.borrow_mut() = Some(detail.clone());
    let weak = Rc::downgrade(ui);
    detail.dialog.connect_closed(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.detail.borrow_mut().take();
        }
    });
    let weak = Rc::downgrade(ui);
    let page = Rc::downgrade(&detail);
    detail.save.connect_clicked(move |_| {
        if let (Some(ui), Some(page)) = (weak.upgrade(), page.upgrade()) {
            let text = page.name.text();
            if text.trim().is_empty() {
                notify(&ui, "Enter a name for the device.");
                return;
            }
            act(
                &ui,
                Action::Set(
                    page.path.clone(),
                    DEVICE,
                    "Alias",
                    text.as_str().to_variant(),
                ),
                "Renaming device…",
            );
        }
    });
    for (switch, property) in [(&detail.trust, "Trusted"), (&detail.blocked, "Blocked")] {
        let weak = Rc::downgrade(ui);
        let path = path.to_owned();
        switch.connect_active_notify(move |switch| {
            if let Some(ui) = weak.upgrade().filter(|ui| !ui.updating.get()) {
                act(
                    &ui,
                    Action::Set(
                        path.clone(),
                        DEVICE,
                        property,
                        switch.is_active().to_variant(),
                    ),
                    "Updating device settings…",
                );
            }
        });
    }
    let weak = Rc::downgrade(ui);
    let page = Rc::downgrade(&detail);
    detail.forget.connect_clicked(move |_| {
        let (Some(ui), Some(page)) = (weak.upgrade(), page.upgrade()) else {
            return;
        };
        let confirm = adw::AlertDialog::builder()
            .heading(format!("Forget “{}”?", device.name))
            .heading_use_markup(false)
            .body("To use it again, you will need to pair it with this computer.")
            .build();
        confirm.add_responses(&[("cancel", "Cancel"), ("forget", "Forget Device")]);
        confirm.set_close_response("cancel");
        confirm.set_default_response(Some("cancel"));
        confirm.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
        let weak = Rc::downgrade(&ui);
        let device = device.clone();
        confirm.connect_response(Some("forget"), move |_, _| {
            if let Some(ui) = weak.upgrade() {
                act(
                    &ui,
                    Action::Forget(device.adapter.clone(), device.path.clone()),
                    "Forgetting device…",
                );
            }
        });
        confirm.present(Some(&page.dialog));
    });
    detail.dialog.present(Some(&ui.window));
}

#[cfg(test)]
#[path = "ui_test.rs"]
mod ui_test;
