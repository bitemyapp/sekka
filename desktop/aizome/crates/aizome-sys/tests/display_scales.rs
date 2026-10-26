//! The login screen's public account preference, over the real D-Bus wire.
mod common;

use aizome_sys::users::{display_scales, publish_display_scales};
use std::sync::{Arc, Mutex};
use zbus::zvariant::OwnedObjectPath;

struct Accounts(Arc<Mutex<Vec<i64>>>);

#[zbus::interface(name = "org.freedesktop.Accounts")]
impl Accounts {
    fn find_user_by_id(&self, uid: i64) -> OwnedObjectPath {
        self.0.lock().unwrap().push(uid);
        common::path(&format!("/org/freedesktop/Accounts/User{uid}"))
    }
}

#[derive(Default)]
struct Scales(String);

#[zbus::interface(name = "org.aizome.DisplayScales")]
impl Scales {
    #[zbus(property)]
    fn scales(&self) -> &str {
        &self.0
    }

    #[zbus(property)]
    fn set_scales(&mut self, value: String) {
        self.0 = value;
    }
}

#[test]
fn publishes_own_scales_and_reads_a_named_account() {
    let bus = test_bus!();
    let uid = unsafe { libc::getuid() };
    let lookups = Arc::new(Mutex::new(Vec::new()));
    let path = format!("/org/freedesktop/Accounts/User{uid}");
    let _server = bus
        .builder()
        .name("org.freedesktop.Accounts")
        .unwrap()
        .serve_at("/org/freedesktop/Accounts", Accounts(lookups.clone()))
        .unwrap()
        .serve_at(path.as_str(), Scales::default())
        .unwrap()
        .build()
        .unwrap();
    let client = bus.bus();
    let scales = "[[outputs]]\nname = 'DP-1'\nscale = 1.5\n";
    publish_display_scales(&client, scales).unwrap();
    assert_eq!(display_scales(&client, uid).unwrap(), scales);
    publish_display_scales(&client, "outputs = []").unwrap();
    assert_eq!(display_scales(&client, uid).unwrap(), "outputs = []");
    assert_eq!(*lookups.lock().unwrap(), vec![i64::from(uid); 4]);
    // Older or unconfigured AccountsService installations are a normal error.
    assert!(display_scales(&client, uid + 1).is_err());
}
