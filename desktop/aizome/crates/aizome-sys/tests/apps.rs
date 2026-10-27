//! D-Bus activation of a `DBusActivatable=true` app, against a fake app on
//! a private bus.

mod common;

use aizome_sys::apps::{DesktopEntry, LaunchOptions, Locale, activate_dbus, dbus_object_path};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::zvariant::OwnedValue;

struct App {
    calls: Arc<Mutex<Vec<String>>>,
}

fn token(platform: &HashMap<String, OwnedValue>) -> String {
    platform.get("activation-token").and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default()
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl App {
    fn activate(&self, platform: HashMap<String, OwnedValue>) {
        self.calls.lock().unwrap().push(format!("Activate token={}", token(&platform)));
    }
    fn open(&self, uris: Vec<String>, platform: HashMap<String, OwnedValue>) {
        self.calls.lock().unwrap().push(format!("Open {uris:?} token={}", token(&platform)));
    }
    fn activate_action(&self, action: String, _parameter: Vec<OwnedValue>, platform: HashMap<String, OwnedValue>) {
        self.calls.lock().unwrap().push(format!("ActivateAction {action} token={}", token(&platform)));
    }
}

#[test]
fn dbus_activatable_apps_are_activated_over_the_bus() {
    let bus = test_bus!();
    assert_eq!(dbus_object_path("org.example.Some-App"), "/org/example/Some_App");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let _app = bus
        .builder()
        .name("org.example.Viewer")
        .unwrap()
        .serve_at("/org/example/Viewer", App { calls: calls.clone() })
        .unwrap()
        .build()
        .unwrap();
    let entry = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=Viewer\nDBusActivatable=true\nExec=viewer %U\nActions=new-window;\n\n\
         [Desktop Action new-window]\nName=New Window\nExec=viewer --new-window\n",
        "org.example.Viewer.desktop",
        &Locale::untranslated(),
    )
    .unwrap();
    let bus = bus.bus();
    let plain = LaunchOptions { activation_token: Some("tok-9".into()), ..LaunchOptions::default() };
    activate_dbus(&entry, &plain, &bus).unwrap();
    let open = LaunchOptions {
        targets: vec!["/tmp/a b.png".into(), "https://example.org/".into()],
        ..LaunchOptions::default()
    };
    activate_dbus(&entry, &open, &bus).unwrap();
    let action = LaunchOptions { action: Some("new-window".into()), ..LaunchOptions::default() };
    activate_dbus(&entry, &action, &bus).unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "Activate token=tok-9".to_owned(),
            r#"Open ["file:///tmp/a%20b.png", "https://example.org/"] token="#.to_owned(),
            "ActivateAction new-window token=".to_owned(),
        ]
    );
    // An app that isn't there fails, so launch falls back to its Exec.
    let missing = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=Gone\nDBusActivatable=true\nExec=gone\n",
        "org.example.Gone.desktop",
        &Locale::untranslated(),
    )
    .unwrap();
    assert!(activate_dbus(&missing, &plain, &bus).is_err());
}
