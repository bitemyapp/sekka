//! Status items against fake apps on a private bus: one registering an
//! object path as libayatana-appindicator does (Steam), one a bus name as
//! Qt does, their menus, clicks, changes and departures.

mod common;

use aizome_sys::Wake;
use aizome_sys::statusnotifier::{ItemStatus, StatusNotifier, Toggle, WATCHER_NAME, WATCHER_PATH};
use common::eventually;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, StructureBuilder, Value};

const AYATANA_PATH: &str = "/org/ayatana/NotificationItem/steam";
const MENU_PATH: &str = "/org/ayatana/NotificationItem/steam/Menu";

/// A tooltip: icon name, pixmaps, title, text.
type ToolTip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);
/// `GetLayout`'s reply: the revision and the root row.
type Layout = (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>));

/// What the apps heard.
#[derive(Default)]
struct Heard {
    calls: Mutex<Vec<String>>,
}

impl Heard {
    fn has(&self, call: &str) -> bool {
        self.calls.lock().unwrap().iter().any(|c| c == call)
    }
}

struct Item {
    id: String,
    icon: Arc<Mutex<String>>,
    status: Arc<Mutex<String>>,
    menu: Option<OwnedObjectPath>,
    heard: Arc<Heard>,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, x: i32, y: i32) -> zbus::fdo::Result<()> {
        if self.menu.is_some() {
            // libayatana's items are only menus.
            return Err(zbus::fdo::Error::UnknownMethod("only a menu".into()));
        }
        self.heard.calls.lock().unwrap().push(format!("Activate {x} {y}"));
        Ok(())
    }
    fn secondary_activate(&self, x: i32, y: i32) {
        self.heard.calls.lock().unwrap().push(format!("SecondaryActivate {x} {y}"));
    }
    fn scroll(&self, delta: i32, orientation: String) {
        self.heard.calls.lock().unwrap().push(format!("Scroll {delta} {orientation}"));
    }
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }
    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        if self.id == "steam" { "Steam".into() } else { String::new() }
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        self.icon.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(1, 1, vec![255, 1, 2, 3]), (2, 2, vec![0; 16])]
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> String {
        "/nowhere".into()
    }
    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        (String::new(), vec![], format!("{} tip", self.id), "more".into())
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        self.menu.clone().unwrap_or_else(|| OwnedObjectPath::try_from("/").unwrap())
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        self.menu.is_some()
    }
    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

fn row(id: i32, props: &[(&str, Value<'static>)], children: Vec<Value<'static>>) -> Value<'static> {
    let dict: HashMap<String, Value<'static>> = props.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
    let children: Vec<Value<'static>> = children.into_iter().map(|c| Value::Value(Box::new(c))).collect();
    StructureBuilder::new()
        .add_field(id)
        .add_field(dict)
        .append_field(Value::Array(zbus::zvariant::Array::from(children)))
        .build()
        .unwrap()
        .into()
}

struct Menu {
    heard: Arc<Heard>,
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn about_to_show(&self, id: i32) -> bool {
        self.heard.calls.lock().unwrap().push(format!("AboutToShow {id}"));
        false
    }
    fn get_layout(&self, parent: i32, _depth: i32, _props: Vec<String>) -> Layout {
        let children = [
            row(1, &[("label", "_Library".into())], vec![]),
            row(2, &[("type", "separator".into())], vec![]),
            row(
                3,
                &[("label", "Friends".into()), ("children-display", "submenu".into())],
                vec![row(
                    4,
                    &[("label", "Online".into()), ("toggle-type", "radio".into()), ("toggle-state", 1i32.into())],
                    vec![],
                )],
            ),
            row(5, &[("label", "Exit Steam".into())], vec![]),
        ];
        let children = children.into_iter().map(|c| OwnedValue::try_from(Value::Value(Box::new(c))).unwrap()).collect();
        (7, (parent, HashMap::new(), children))
    }
    fn event(&self, id: i32, event: String, _data: OwnedValue, _timestamp: u32) {
        self.heard.calls.lock().unwrap().push(format!("Event {id} {event}"));
    }
}

#[test]
fn items_register_change_click_and_leave() {
    let bus = test_bus!();
    let sn = StatusNotifier::start(&bus.bus(), Wake::none()).expect("serve the watcher");
    assert!(sn.available());

    // Steam, as libayatana registers: an object path on its connection.
    let steam_heard = Arc::new(Heard::default());
    let steam_icon = Arc::new(Mutex::new("steam".to_owned()));
    let steam_status = Arc::new(Mutex::new("Active".to_owned()));
    let steam = bus
        .builder()
        .serve_at(
            AYATANA_PATH,
            Item {
                id: "steam".into(),
                icon: steam_icon.clone(),
                status: steam_status.clone(),
                menu: Some(OwnedObjectPath::try_from(MENU_PATH).unwrap()),
                heard: steam_heard.clone(),
            },
        )
        .unwrap()
        .serve_at(MENU_PATH, Menu { heard: steam_heard.clone() })
        .unwrap()
        .build()
        .unwrap();
    let steam_name = steam.unique_name().unwrap().to_string();
    steam
        .call_method(
            Some(WATCHER_NAME),
            WATCHER_PATH,
            Some(WATCHER_NAME),
            "RegisterStatusNotifierItem",
            &(AYATANA_PATH,),
        )
        .expect("register Steam");

    // A Qt app, by bus name at the default path.
    let qt_heard = Arc::new(Heard::default());
    let qt = bus
        .builder()
        .name("org.kde.StatusNotifierItem-42-1")
        .unwrap()
        .serve_at(
            "/StatusNotifierItem",
            Item {
                id: "qtapp".into(),
                icon: Arc::new(Mutex::new("qtapp".into())),
                status: Arc::new(Mutex::new("Passive".into())),
                menu: None,
                heard: qt_heard.clone(),
            },
        )
        .unwrap()
        .build()
        .unwrap();
    qt.call_method(
        Some(WATCHER_NAME),
        WATCHER_PATH,
        Some(WATCHER_NAME),
        "RegisterStatusNotifierItem",
        &("org.kde.StatusNotifierItem-42-1",),
    )
    .expect("register the Qt app");

    assert!(eventually(|| sn.snapshot().items.len() == 2));
    let s = sn.snapshot();
    let steam_key = format!("{steam_name}{AYATANA_PATH}");
    let qt_key = "org.kde.StatusNotifierItem-42-1/StatusNotifierItem".to_owned();
    assert_eq!(s.items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), [steam_key.as_str(), qt_key.as_str()]);
    let st = s.item(&steam_key).unwrap();
    assert_eq!((st.id.as_str(), st.label(), st.icon.name.as_str()), ("steam", "Steam", "steam"));
    assert_eq!(st.icon.pixmaps.len(), 2);
    assert_eq!(&*st.icon.pixmaps[0].rgba, &[1, 2, 3, 255]);
    assert_eq!(st.icon_theme_path, "/nowhere");
    assert_eq!(st.menu.as_deref(), Some(MENU_PATH));
    assert!(st.item_is_menu);
    let q = s.item(&qt_key).unwrap();
    assert_eq!((q.label(), q.status, q.menu.clone()), ("qtapp tip", ItemStatus::Passive, None));

    // The watcher lists them, as other hosts would read it.
    let listed: Vec<String> = zbus::blocking::Proxy::new(&steam, WATCHER_NAME, WATCHER_PATH, WATCHER_NAME)
        .unwrap()
        .get_property("RegisteredStatusNotifierItems")
        .unwrap();
    assert_eq!(listed.len(), 2);

    // A new icon, announced.
    *steam_icon.lock().unwrap() = "steam_tray_mono".into();
    *steam_status.lock().unwrap() = "NeedsAttention".into();
    zbus::block_on(Item::new_icon(&SignalEmitter::new(steam.inner(), AYATANA_PATH).unwrap())).unwrap();
    assert!(eventually(|| sn.snapshot().item(&steam_key).is_some_and(|i| i.icon.name == "steam_tray_mono")));
    assert_eq!(sn.snapshot().item(&steam_key).unwrap().status, ItemStatus::NeedsAttention);

    // Clicks.
    assert!(sn.activate(&steam_key, 10, 20).wait().is_err(), "a menu-only item refuses Activate");
    sn.activate(&qt_key, 10, 20).wait().unwrap();
    sn.secondary_activate(&qt_key, 1, 2).wait().unwrap();
    assert!(qt_heard.has("Activate 10 20") && qt_heard.has("SecondaryActivate 1 2"));
    sn.scroll(&qt_key, -2, true).wait().unwrap();
    sn.scroll(&qt_key, 1, false).wait().unwrap();
    assert!(qt_heard.has("Scroll -2 vertical") && qt_heard.has("Scroll 1 horizontal"));

    // Steam's menu, read whole, and a row picked.
    let menu = sn.menu(&steam_key).wait().unwrap();
    assert!(steam_heard.has("AboutToShow 0"));
    let labels: Vec<&str> = menu.children.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Library", "", "Friends", "Exit Steam"]);
    assert_eq!(menu.children[2].children[0].toggle, Toggle::Radio(true));
    sn.menu_clicked(&steam_key, 5).wait().unwrap();
    assert!(steam_heard.has("Event 5 clicked"));
    assert!(sn.menu(&qt_key).wait().is_err(), "no menu");

    // Leaving the bus takes the item away.
    drop(steam);
    assert!(eventually(|| sn.snapshot().items.len() == 1));
    assert_eq!(sn.snapshot().items[0].key, qt_key);
    drop(qt);
    assert!(eventually(|| sn.snapshot().items.is_empty()));
}

#[test]
fn a_second_watcher_takes_over_and_apps_find_it() {
    let bus = test_bus!();
    let first = StatusNotifier::start(&bus.bus(), Wake::none()).expect("serve the watcher");
    // A restarted shell (another connection) takes the name.
    let shell2 = aizome_sys::Bus::at(&bus.address).unwrap();
    let second = StatusNotifier::start(&shell2, Wake::none()).expect("take over");
    assert!(second.available());
    let owner = zbus::blocking::fdo::DBusProxy::new(&bus.builder().build().unwrap())
        .unwrap()
        .get_name_owner(WATCHER_NAME.try_into().unwrap())
        .unwrap();
    assert_eq!(Some(owner.to_string()), shell2.unique_name());
    drop(first);
    drop(second);
}
