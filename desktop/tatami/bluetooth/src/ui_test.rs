// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exercises the real GTK controls, GIO marshalling and Agent1 over a private
//! dbus-daemon. No host bus, Bluetooth adapter or saved pairing is accessed.
use super::*;
use crate::model::{Objects, Properties};
use glib::variant::ObjectPath;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Instant,
};

const A: &str = "/org/bluez/hci0";
const B: &str = "/org/bluez/hci1";
const D: &str = "/org/bluez/hci0/dev_01";
const E: &str = "/org/bluez/hci0/dev_02";

struct Bus(Child);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn connection(address: &str) -> gio::DBusConnection {
    gio::DBusConnection::for_address_future(
        address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
    )
    .await
    .unwrap()
}

fn path(s: &str) -> ObjectPath {
    ObjectPath::try_from(s).unwrap()
}
fn props(items: &[(&str, glib::Variant)]) -> Properties {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn objects() -> Objects {
    let adapter = |name: &str| {
        HashMap::from([(
            ADAPTER.into(),
            props(&[
                ("Alias", name.to_variant()),
                ("Address", "11:22:33:44:55:66".to_variant()),
                ("Powered", true.to_variant()),
                ("Discovering", false.to_variant()),
            ]),
        )])
    };
    let device = |name: &str, paired: bool| {
        HashMap::from([
            (
                DEVICE.into(),
                props(&[
                    ("Alias", name.to_variant()),
                    ("Address", "AA:BB:CC:DD:EE:FF".to_variant()),
                    ("Adapter", path(A).to_variant()),
                    ("Paired", paired.to_variant()),
                    ("Connected", paired.to_variant()),
                    ("Trusted", false.to_variant()),
                    ("Blocked", false.to_variant()),
                    (
                        "Icon",
                        if paired {
                            "audio-headphones"
                        } else {
                            "input-keyboard"
                        }
                        .to_variant(),
                    ),
                ]),
            ),
            (
                "org.bluez.Battery1".into(),
                props(&[("Percentage", 84u8.to_variant())]),
            ),
        ])
    };
    HashMap::from([
        (path(A), adapter("ThinkPad")),
        (path(B), adapter("USB adapter")),
        (path(D), device("Studio headphones", true)),
        (path(E), device("Keyboard <&>", false)),
    ])
}

struct Fake {
    conn: gio::DBusConnection,
    objects: RefCell<Objects>,
    calls: RefCell<Vec<String>>,
    mode: RefCell<String>,
    reject_set: Cell<bool>,
    reject_connect: Cell<bool>,
    agent: RefCell<Option<(String, String)>>,
    pending: RefCell<Option<gio::DBusMethodInvocation>>,
    registered: Cell<bool>,
}

impl Fake {
    fn set(&self, object: &str, interface: &str, key: &str, value: glib::Variant) {
        self.objects
            .borrow_mut()
            .get_mut(&path(object))
            .unwrap()
            .get_mut(interface)
            .unwrap()
            .insert(key.into(), value);
    }
    fn value<T: glib::variant::FromVariant>(&self, object: &str, interface: &str, key: &str) -> T {
        self.objects.borrow()[&path(object)][interface][key]
            .get()
            .unwrap()
    }
    fn export(self: &Rc<Self>, path: &str, xml: &str) {
        let info = gio::DBusNodeInfo::for_xml(xml).unwrap();
        for interface in info.interfaces() {
            let fake = self.clone();
            self.conn
                .register_object(path, interface)
                .method_call(move |_, sender, object, interface, method, args, call| {
                    fake.calls.borrow_mut().push(format!("{object} {method}"));
                    let interface = interface.unwrap();
                    match (interface, method) {
                        ("org.freedesktop.DBus.ObjectManager", "GetManagedObjects") => {
                            call.return_value(Some(&(fake.objects.borrow().clone(),).to_variant()))
                        }
                        ("org.bluez.AgentManager1", "RegisterAgent") => {
                            let (p, capability) = args.get::<(ObjectPath, String)>().unwrap();
                            assert_eq!(capability, "KeyboardDisplay");
                            *fake.agent.borrow_mut() =
                                Some((sender.unwrap().into(), p.to_string()));
                            fake.registered.set(true);
                            call.return_value(Some(&().to_variant()));
                        }
                        ("org.bluez.AgentManager1", "UnregisterAgent") => {
                            fake.registered.set(false);
                            call.return_value(Some(&().to_variant()));
                        }
                        ("org.freedesktop.DBus.Properties", "Set") => {
                            let (iface, property, value) =
                                args.get::<(String, String, glib::Variant)>().unwrap();
                            if fake.reject_set.get() {
                                call.return_dbus_error(
                                    "org.bluez.Error.NotAuthorized",
                                    "Not allowed",
                                );
                            } else {
                                fake.set(object, &iface, &property, value.clone());
                                if property == "Powered" && value.get::<bool>() == Some(false) {
                                    fake.set(object, ADAPTER, "Discovering", false.to_variant());
                                }
                                call.return_value(Some(&().to_variant()));
                            }
                        }
                        (ADAPTER, "SetDiscoveryFilter") => {
                            let (filter,) =
                                args.get::<(HashMap<String, glib::Variant>,)>().unwrap();
                            assert_eq!(filter["Transport"].get::<String>().unwrap(), "auto");
                            assert!(!filter.contains_key("Discoverable"));
                            call.return_value(Some(&().to_variant()));
                        }
                        (ADAPTER, "StartDiscovery" | "StopDiscovery") => {
                            fake.set(
                                object,
                                ADAPTER,
                                "Discovering",
                                (method == "StartDiscovery").to_variant(),
                            );
                            call.return_value(Some(&().to_variant()));
                        }
                        (ADAPTER, "RemoveDevice") => {
                            let (p,) = args.get::<(ObjectPath,)>().unwrap();
                            assert_eq!(
                                fake.objects.borrow()[&p][DEVICE]["Adapter"]
                                    .get::<ObjectPath>()
                                    .unwrap()
                                    .as_str(),
                                object
                            );
                            fake.objects.borrow_mut().remove(&p);
                            call.return_value(Some(&().to_variant()));
                        }
                        (DEVICE, "Connect" | "Disconnect") => {
                            if method == "Connect"
                                && fake.value::<bool>(object, DEVICE, "Connected")
                            {
                                call.return_dbus_error(
                                    "org.bluez.Error.AlreadyConnected",
                                    "Already connected",
                                );
                            } else if method == "Connect" && fake.reject_connect.get() {
                                call.return_dbus_error(
                                    "org.bluez.Error.Failed",
                                    "br-connection-create-socket",
                                );
                            } else {
                                fake.set(
                                    object,
                                    DEVICE,
                                    "Connected",
                                    (method == "Connect").to_variant(),
                                );
                                call.return_value(Some(&().to_variant()));
                            }
                        }
                        (DEVICE, "Pair") => {
                            assert!(fake.registered.get());
                            *fake.pending.borrow_mut() = Some(call);
                            let mode = fake.mode.borrow().clone();
                            let fake = fake.clone();
                            let object = object.to_owned();
                            glib::spawn_future_local(async move {
                                let result = fake.ask(&object, &mode).await;
                                if let Some(call) = fake.pending.borrow_mut().take() {
                                    match result {
                                        Ok(reply) => {
                                            if mode == "RequestPasskey" {
                                                assert_eq!(reply.get::<(u32,)>().unwrap().0, 42);
                                            }
                                            if mode == "RequestPinCode" {
                                                assert_eq!(
                                                    reply.get::<(String,)>().unwrap().0,
                                                    "a1b2"
                                                );
                                            }
                                            fake.set(&object, DEVICE, "Paired", true.to_variant());
                                            call.return_value(Some(&().to_variant()));
                                        }
                                        Err(_) => call.return_dbus_error(
                                            "org.bluez.Error.AuthenticationRejected",
                                            "Declined",
                                        ),
                                    }
                                }
                            });
                        }
                        (DEVICE, "CancelPairing") => {
                            if let Some(pending) = fake.pending.borrow_mut().take() {
                                pending.return_dbus_error(
                                    "org.bluez.Error.AuthenticationCanceled",
                                    "Cancelled",
                                );
                            }
                            call.return_value(Some(&().to_variant()));
                        }
                        _ => panic!("Unexpected call {interface}.{method}"),
                    }
                })
                .build()
                .unwrap();
        }
    }
    async fn ask(&self, object: &str, method: &str) -> Result<glib::Variant, glib::Error> {
        let (destination, agent) = self.agent.borrow().clone().unwrap();
        let args = match method {
            "RequestConfirmation" => (path(object), 123456u32).to_variant(),
            "DisplayPasskey" => (path(object), 42u32, 3u16).to_variant(),
            "DisplayPinCode" => (path(object), "000042").to_variant(),
            "AuthorizeService" => {
                (path(object), "0000110b-0000-1000-8000-00805f9b34fb").to_variant()
            }
            "Cancel" | "Release" => ().to_variant(),
            _ => (path(object),).to_variant(),
        };
        self.conn
            .call_future(
                Some(&destination),
                &agent,
                "org.bluez.Agent1",
                method,
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                3000,
            )
            .await
    }
}

async fn wait_until(ready: impl Fn() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "Timed out waiting for UI state"
        );
        glib::timeout_future(Duration::from_millis(10)).await;
    }
}
async fn loaded(ui: &Rc<Ui>) {
    wait_until(|| !ui.reading.get() && !ui.busy.get()).await;
}
async fn read(ui: &Rc<Ui>) {
    refresh(ui);
    loaded(ui).await;
}
fn click_device(ui: &Ui, path: &str) {
    let button = ui.rows.borrow()[path].action.clone();
    button.emit_clicked();
}

#[test]
#[ignore = "requires Xvfb and dbus-daemon; run by the Nix package check"]
fn bluetooth_window_and_agent_over_private_dbus() {
    adw::init().unwrap();
    crate::theme::install();
    let context = glib::MainContext::default();
    context.block_on(async {
        let mut daemon = Command::new("dbus-daemon").arg(concat!("--config-file=", env!("CARGO_MANIFEST_DIR"), "/data/test-bus.conf")).args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped()).spawn().unwrap();
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
        let _bus = Bus(daemon);
        let client = connection(address.trim()).await;
        let service = connection(address.trim()).await;
        let fake = Rc::new(Fake { conn: service, objects: RefCell::new(objects()), calls: RefCell::new(vec![]),
            mode: RefCell::new("RequestConfirmation".into()), reject_set: Cell::new(false), reject_connect: Cell::new(false),
            agent: RefCell::new(None), pending: RefCell::new(None), registered: Cell::new(false) });
        fake.export("/", r#"<node><interface name="org.freedesktop.DBus.ObjectManager"><method name="GetManagedObjects"><arg type="a{oa{sa{sv}}}" direction="out"/></method></interface></node>"#);
        fake.export("/org/bluez", r#"<node><interface name="org.bluez.AgentManager1"><method name="RegisterAgent"><arg type="o" direction="in"/><arg type="s" direction="in"/></method><method name="UnregisterAgent"><arg type="o" direction="in"/></method></interface></node>"#);
        let properties = r#"<interface name="org.freedesktop.DBus.Properties"><method name="Set"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/></method></interface>"#;
        for p in [A, B] { fake.export(p, &format!(r#"<node>{properties}<interface name="org.bluez.Adapter1"><method name="SetDiscoveryFilter"><arg type="a{{sv}}" direction="in"/></method><method name="StartDiscovery"/><method name="StopDiscovery"/><method name="RemoveDevice"><arg type="o" direction="in"/></method></interface></node>"#)); }
        for p in [D, E] { fake.export(p, &format!(r#"<node>{properties}<interface name="org.bluez.Device1"><method name="Pair"/><method name="CancelPairing"/><method name="Connect"/><method name="Disconnect"/></interface></node>"#)); }
        let app = adw::Application::builder().application_id("org.tatami.Bluetooth.Test")
            .flags(gio::ApplicationFlags::NON_UNIQUE).build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let ui = build(&app);
        watch(&ui, &client);
        let mut owner = gio::bus_own_name_on_connection(&fake.conn, "org.bluez", gio::BusNameOwnerFlags::NONE, |_, _| {}, |_, _| {});
        ui.window.present();
        wait_until(|| ui.backend.borrow().is_some()).await;
        loaded(&ui).await;
        assert!(fake.registered.get());
        assert!(fake.value::<bool>(A, ADAPTER, "Discovering"));
        assert!(!fake.value::<bool>(B, ADAPTER, "Discovering"));
        assert_eq!(ui.rows.borrow()[D].row.subtitle().unwrap(), "Connected · 84% battery");

        capture(&ui, "bluetooth-devices").await;

        // Polling changes the same row in place (preserve focus and identity).
        let before = ui.rows.borrow()[D].row.clone();
        fake.set(D, "org.bluez.Battery1", "Percentage", 0u8.to_variant());
        read(&ui).await;
        assert_eq!(ui.rows.borrow()[D].row, before);
        assert_eq!(before.subtitle().unwrap(), "Connected · 0% battery");
        click_device(&ui, D);
        loaded(&ui).await;
        assert!(!fake.value::<bool>(D, DEVICE, "Connected"));
        click_device(&ui, D);
        loaded(&ui).await;
        assert!(fake.value::<bool>(D, DEVICE, "Connected"));

        let backend = ui.backend.borrow().clone().unwrap();
        backend.device(D, "Connect").await.unwrap(); // AlreadyConnected is success.

        // Failure rolls a switch back to the daemon's state.
        fake.reject_set.set(true);
        ui.power.set_active(false);
        loaded(&ui).await;
        assert!(ui.power.is_active());
        fake.reject_set.set(false);
        ui.power.set_active(false);
        loaded(&ui).await;
        assert!(!ui.power.is_active());
        assert_eq!(ui.selected.borrow().as_str(), A); // B is still on.
        ui.power.set_active(true);
        loaded(&ui).await;
        assert!(fake.value::<bool>(A, ADAPTER, "Discovering"));

        // Adapter switching releases just our previous discovery session.
        ui.adapters.set_selected(1);
        loaded(&ui).await;
        assert!(!fake.value::<bool>(A, ADAPTER, "Discovering"));
        assert!(fake.value::<bool>(B, ADAPTER, "Discovering"));
        assert!(ui.rows.borrow().is_empty());
        ui.adapters.set_selected(0);
        loaded(&ui).await;

        // A different bus peer cannot inject a prompt.
        let stranger = connection(address.trim()).await;
        let spoof = stranger.call_future(client.unique_name().as_deref(), bluez::AGENT_PATH, "org.bluez.Agent1",
            "RequestConfirmation", Some(&(path(E), 123456u32).to_variant()), None, gio::DBusCallFlags::NONE, 3000).await;
        assert!(spoof.is_err());
        assert!(ui.prompt.borrow().is_none());

        // Real button -> Pair -> Agent1 -> confirmation/entry -> Connect.
        for mode in ["RequestConfirmation", "RequestPasskey", "RequestPinCode", "RequestAuthorization", "AuthorizeService", "DisplayPasskey", "DisplayPinCode"] {
            fake.set(E, DEVICE, "Paired", false.to_variant());
            fake.set(E, DEVICE, "Connected", false.to_variant());
            *fake.mode.borrow_mut() = mode.into();
            read(&ui).await;
            click_device(&ui, E);
            if !mode.starts_with("Display") {
                wait_until(|| ui.prompt.borrow().is_some()).await;
                if mode == "RequestConfirmation" { capture(&ui, "bluetooth-pairing").await; }
                assert!(ui.busy.get());
                assert!(!ui.power.is_sensitive());
                agent::respond(&ui, "pair", match mode { "RequestPasskey" => Some("000042"), "RequestPinCode" => Some("a1b2"), _ => None });
            }
            loaded(&ui).await;
            assert!(fake.value::<bool>(E, DEVICE, "Paired"), "{mode}");
            assert!(fake.value::<bool>(E, DEVICE, "Connected"), "{mode}");
            assert!(ui.prompt.borrow().is_none(), "display-only prompt must close at completion");
        }

        // Rejecting, daemon cancellation, and the window's Cancel button.
        for how in ["reject", "daemon", "button"] {
            fake.set(E, DEVICE, "Paired", false.to_variant());
            *fake.mode.borrow_mut() = "RequestConfirmation".into();
            read(&ui).await;
            click_device(&ui, E);
            wait_until(|| ui.prompt.borrow().is_some()).await;
            match how {
                "reject" => agent::respond(&ui, "cancel", None),
                "daemon" => { fake.ask(E, "Cancel").await.unwrap(); },
                _ => ui.cancel.emit_clicked(),
            }
            loaded(&ui).await;
            assert!(!fake.value::<bool>(E, DEVICE, "Paired"));
            assert!(ui.prompt.borrow().is_none());
        }

        // Pairing can succeed even when its profiles cannot connect.
        fake.reject_connect.set(true);
        click_device(&ui, E);
        wait_until(|| ui.prompt.borrow().is_some()).await;
        agent::respond(&ui, "pair", None);
        loaded(&ui).await;
        assert!(fake.value::<bool>(E, DEVICE, "Paired"));
        assert!(ui.rows.borrow()[E].paired);
        fake.reject_connect.set(false);

        // Settings controls mutate the selected object, preserve literal names.
        detail(&ui, D);
        let page = ui.detail.borrow().clone().unwrap();
        capture(&ui, "bluetooth-device-settings").await;
        page.name.set_text("Headphones <&>");
        page.save.emit_clicked();
        loaded(&ui).await;
        assert_eq!(fake.value::<String>(D, DEVICE, "Alias"), "Headphones <&>");
        page.trust.set_active(true);
        loaded(&ui).await;
        assert!(fake.value::<bool>(D, DEVICE, "Trusted"));
        page.blocked.set_active(true);
        loaded(&ui).await;
        assert!(fake.value::<bool>(D, DEVICE, "Blocked"));
        assert!(!ui.rows.borrow()[D].action.is_sensitive());
        // Forget requires the explicit destructive response.
        page.forget.emit_clicked();
        let confirm = ui.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        confirm.emit_by_name::<()>("response", &[&"cancel"]);
        confirm.force_close();
        assert!(fake.objects.borrow().contains_key(&path(D)));
        page.forget.emit_clicked();
        let confirm = ui.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        confirm.emit_by_name_with_details::<()>("response", glib::Quark::from_str("forget"), &[&"forget"]);
        confirm.force_close();
        loaded(&ui).await;
        assert!(!fake.objects.borrow().contains_key(&path(D)));
        assert!(ui.detail.borrow().is_none());

        // Removing the selected adapter picks the surviving one.
        fake.objects.borrow_mut().remove(&path(A));
        // BlueZ removes its children too.
        fake.objects.borrow_mut().remove(&path(E));
        read(&ui).await;
        assert_eq!(ui.selected.borrow().as_str(), B);
        assert!(ui.rows.borrow().is_empty());
        fake.objects.borrow_mut().clear();
        read(&ui).await;
        assert!(!ui.power.is_sensitive());
        assert!(ui.status.text().contains("No Bluetooth adapter"));
        // A service disappearance closes pending prompts and disables stale
        // controls. Reacquiring the name registers a fresh application agent.
        *fake.objects.borrow_mut() = objects();
        read(&ui).await;
        ui.adapters.set_selected(0);
        loaded(&ui).await;
        click_device(&ui, E);
        wait_until(|| ui.prompt.borrow().is_some()).await;
        gio::bus_unown_name(owner);
        wait_until(|| ui.backend.borrow().is_none() && !ui.busy.get()).await;
        assert!(ui.prompt.borrow().is_none());
        assert!(!ui.power.is_sensitive());
        owner = gio::bus_own_name_on_connection(&fake.conn, "org.bluez", gio::BusNameOwnerFlags::NONE, |_, _| {}, |_, _| {});
        wait_until(|| ui.backend.borrow().is_some()).await;
        loaded(&ui).await;
        assert!(ui.power.is_sensitive());

        // Libadwaita closes the pairing sheet first, which cancels pairing;
        // the next close dismisses the window and releases discovery.
        click_device(&ui, E);
        wait_until(|| ui.prompt.borrow().is_some()).await;
        ui.window.close();
        wait_until(|| ui.prompt.borrow().is_none() && !ui.busy.get()).await;
        loaded(&ui).await;
        assert!(fake.pending.borrow().is_none());
        ui.window.close();
        wait_until(|| !fake.registered.get() && app.windows().is_empty()).await;
        assert!(!fake.value::<bool>(A, ADAPTER, "Discovering"));
        assert!(fake.pending.borrow().is_none());
        assert!(!fake.calls.borrow().iter().any(|c| c.contains("RequestDefaultAgent")));
        gio::bus_unown_name(owner);
        client.close_future().await.unwrap();
        fake.conn.close_future().await.unwrap();
    });
}

async fn capture(ui: &Ui, name: &str) {
    let Ok(dir) = std::env::var("TATAMI_BLUETOOTH_TEST_CAPTURES") else {
        return;
    };
    glib::timeout_future(Duration::from_millis(300)).await;
    let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(
        &snapshot,
        f64::from(ui.window.width()),
        f64::from(ui.window.height()),
    );
    let node = snapshot.to_node().unwrap();
    let texture = ui.window.renderer().unwrap().render_texture(&node, None);
    std::fs::create_dir_all(&dir).unwrap();
    texture
        .save_to_png(std::path::Path::new(&dir).join(format!("{name}.png")))
        .unwrap();
}
