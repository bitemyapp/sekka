//! timedated, localed and hostnamed against fakes on a private bus, with a
//! fake systemd telling which settings NixOS pins.

mod common;

use aizome_sys::datetime::{Hostname, Locale, TimeDate};
use aizome_sys::{Error, Wake};
use common::{eventually, path};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

struct Systemd {
    units: HashMap<String, String>,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl Systemd {
    fn load_unit(&self, name: String) -> fdo::Result<OwnedObjectPath> {
        self.units.get(&name).map(|p| path(p)).ok_or_else(|| fdo::Error::Failed(format!("no unit {name}")))
    }
}

struct Unit {
    environment: Vec<String>,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Service")]
impl Unit {
    #[zbus(property)]
    fn environment(&self) -> Vec<String> {
        self.environment.clone()
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "System.Error")]
enum SystemError {
    #[allow(non_camel_case_types)]
    EROFS(String),
}

struct Timedated {
    timezone: String,
    ntp: bool,
    calls: Arc<Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.freedesktop.timedate1")]
impl Timedated {
    #[zbus(property)]
    fn timezone(&self) -> String {
        self.timezone.clone()
    }
    #[zbus(property, name = "NTP")]
    fn ntp(&self) -> bool {
        self.ntp
    }
    #[zbus(property, name = "CanNTP")]
    fn can_ntp(&self) -> bool {
        true
    }
    async fn set_timezone(
        &mut self,
        timezone: String,
        interactive: bool,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let flag = header.primary().flags().contains(zbus::message::Flags::AllowInteractiveAuth);
        self.calls.lock().unwrap().push(format!("SetTimezone({timezone}, {interactive}, flag={flag})"));
        self.timezone = timezone;
        self.timezone_changed(&emitter).await?;
        Ok(())
    }
    #[zbus(name = "SetNTP")]
    fn set_ntp(&mut self, _on: bool, _interactive: bool) -> Result<(), SystemError> {
        Err(SystemError::EROFS("Failed to set NTP: Read-only file system".into()))
    }
    fn set_time(&self, usec: i64, relative: bool, _interactive: bool) {
        self.calls.lock().unwrap().push(format!("SetTime({usec}, {relative})"));
    }
}

struct Localed;

#[zbus::interface(name = "org.freedesktop.locale1")]
impl Localed {
    #[zbus(property)]
    fn locale(&self) -> Vec<String> {
        vec!["LANG=en_US.UTF-8".into(), "LC_TIME=en_GB.UTF-8".into()]
    }
    #[zbus(property, name = "X11Layout")]
    fn x11_layout(&self) -> String {
        "us".into()
    }
    #[zbus(property, name = "X11Options")]
    fn x11_options(&self) -> String {
        "ctrl:nocaps".into()
    }
}

struct Hostnamed {
    pretty: String,
}

#[zbus::interface(name = "org.freedesktop.hostname1")]
impl Hostnamed {
    #[zbus(property)]
    fn hostname(&self) -> String {
        "ganymede".into()
    }
    #[zbus(property)]
    fn static_hostname(&self) -> String {
        "ganymede".into()
    }
    #[zbus(property)]
    fn pretty_hostname(&self) -> String {
        self.pretty.clone()
    }
    #[zbus(property)]
    fn chassis(&self) -> String {
        "laptop".into()
    }
    #[zbus(property)]
    fn hardware_model(&self) -> String {
        "ThinkPad X1 Carbon Gen 14".into()
    }
    async fn set_pretty_hostname(
        &mut self,
        name: String,
        _interactive: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.pretty = name;
        self.pretty_hostname_changed(&emitter).await?;
        Ok(())
    }
}

/// Serve everything; `timezone_env` is timedated's `SYSTEMD_ETC_LOCALTIME`.
fn serve(bus: &common::TestBus, timezone_env: &str) -> (Vec<zbus::blocking::Connection>, Arc<Mutex<Vec<String>>>) {
    serve_with_policy(bus, timezone_env, None)
}

fn serve_with_policy(
    bus: &common::TestBus,
    timezone_env: &str,
    managed: Option<bool>,
) -> (Vec<zbus::blocking::Connection>, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut timezone_environment = vec![format!("SYSTEMD_ETC_LOCALTIME={timezone_env}"), "TZDIR=/etc/zoneinfo".into()];
    if let Some(managed) = managed {
        timezone_environment.push(format!("AIZOME_TIMEZONE_MANAGED={}", u8::from(managed)));
    }
    let units = HashMap::from([
        ("systemd-timedated.service".to_owned(), "/org/freedesktop/systemd1/unit/timedated".to_owned()),
        ("systemd-localed.service".to_owned(), "/org/freedesktop/systemd1/unit/localed".to_owned()),
        ("systemd-hostnamed.service".to_owned(), "/org/freedesktop/systemd1/unit/hostnamed".to_owned()),
    ]);
    let systemd = bus
        .builder()
        .name("org.freedesktop.systemd1")
        .unwrap()
        .serve_at("/org/freedesktop/systemd1", Systemd { units })
        .unwrap()
        .serve_at("/org/freedesktop/systemd1/unit/timedated", Unit { environment: timezone_environment })
        .unwrap()
        .serve_at(
            "/org/freedesktop/systemd1/unit/localed",
            Unit { environment: vec!["SYSTEMD_ETC_LOCALE_CONF=/etc/static/locale.conf".into()] },
        )
        .unwrap()
        .serve_at(
            "/org/freedesktop/systemd1/unit/hostnamed",
            Unit {
                environment: vec![
                    "SYSTEMD_ETC_HOSTNAME=/etc/static/hostname".into(),
                    "SYSTEMD_ETC_MACHINE_INFO=/var/lib/machine-info".into(),
                ],
            },
        )
        .unwrap()
        .build()
        .unwrap();
    let daemons = bus
        .builder()
        .name("org.freedesktop.timedate1")
        .unwrap()
        .name("org.freedesktop.locale1")
        .unwrap()
        .name("org.freedesktop.hostname1")
        .unwrap()
        .serve_at(
            "/org/freedesktop/timedate1",
            Timedated { timezone: "America/Chicago".into(), ntp: true, calls: calls.clone() },
        )
        .unwrap()
        .serve_at("/org/freedesktop/locale1", Localed)
        .unwrap()
        .serve_at("/org/freedesktop/hostname1", Hostnamed { pretty: String::new() })
        .unwrap()
        .build()
        .unwrap();
    (vec![systemd, daemons], calls)
}

#[test]
fn a_timezone_the_configuration_leaves_free_can_be_set() {
    let bus = test_bus!();
    let (_fakes, calls) = serve(&bus, "/var/lib/localtime");
    let time = TimeDate::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| time.snapshot().timezone == "America/Chicago"));
    let state = time.snapshot();
    assert!(state.available && state.ntp && state.can_ntp && !state.timezone_managed);
    time.set_timezone("Europe/Berlin").wait().unwrap();
    assert!(calls.lock().unwrap().contains(&"SetTimezone(Europe/Berlin, true, flag=true)".to_owned()));
    assert!(eventually(|| time.snapshot().timezone == "Europe/Berlin"));
    assert!(matches!(time.set_timezone("../../etc/passwd").wait(), Err(Error::InvalidArgument(_))));
    // A read-only file system means the configuration owns it.
    assert!(matches!(time.set_ntp(false).wait(), Err(Error::ManagedBySystem(_))));
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
    time.set_time(when).wait().unwrap();
    assert!(calls.lock().unwrap().contains(&"SetTime(1800000000000000, false)".to_owned()));
}

#[test]
fn a_pinned_timezone_is_reported_and_not_tried() {
    let bus = test_bus!();
    let (_fakes, calls) = serve(&bus, "/etc/static/localtime");
    let time = TimeDate::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| time.snapshot().available));
    assert!(time.snapshot().timezone_managed);
    assert!(matches!(time.set_timezone("Europe/Berlin").wait(), Err(Error::ManagedBySystem(_))));
    assert!(calls.lock().unwrap().iter().all(|c| !c.starts_with("SetTimezone")));
}

#[test]
fn a_mutable_zone_stays_editable_after_timedated_writes_an_absolute_link() {
    let bus = test_bus!();
    let path = std::env::temp_dir().join(format!("aizome-mutable-localtime-{}", std::process::id()));
    std::os::unix::fs::symlink("../etc/zoneinfo/America/Chicago", &path).unwrap();
    let (_fakes, calls) = serve_with_policy(&bus, path.to_str().unwrap(), Some(false));
    let time = TimeDate::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| time.snapshot().available));
    assert!(!time.snapshot().timezone_managed);
    // The real NixOS timedated writes this spelling on SetTimezone.
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink("/etc/zoneinfo/Asia/Tokyo", &path).unwrap();
    time.set_timezone("Asia/Tokyo").wait().unwrap();
    assert!(eventually(|| time.snapshot().timezone == "Asia/Tokyo"));
    assert!(!time.snapshot().timezone_managed);
    time.set_timezone("America/Chicago").wait().unwrap();
    assert!(eventually(|| time.snapshot().timezone == "America/Chicago"));
    assert!(!time.snapshot().timezone_managed);
    assert_eq!(calls.lock().unwrap().iter().filter(|c| c.starts_with("SetTimezone")).count(), 2);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn an_explicitly_managed_zone_is_not_tried_even_with_a_writable_path() {
    let bus = test_bus!();
    let (_fakes, calls) = serve_with_policy(&bus, "/var/lib/localtime", Some(true));
    let time = TimeDate::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| time.snapshot().available));
    assert!(time.snapshot().timezone_managed);
    assert!(matches!(time.set_timezone("Asia/Tokyo").wait(), Err(Error::ManagedBySystem(_))));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn locale_and_hostname() {
    let bus = test_bus!();
    let (_fakes, _calls) = serve(&bus, "/etc/static/localtime");
    let locale = Locale::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| locale.snapshot().available));
    let l = locale.snapshot();
    assert_eq!(l.lang(), Some("en_US.UTF-8"));
    assert_eq!((l.x11_layout.as_str(), l.x11_options.as_str()), ("us", "ctrl:nocaps"));
    assert!(l.locale_managed);
    assert!(matches!(locale.set_locale(&["LANG=de_DE.UTF-8".into()]).wait(), Err(Error::ManagedBySystem(_))));

    let host = Hostname::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| host.snapshot().available));
    let h = host.snapshot();
    assert!(h.static_hostname_managed && !h.pretty_hostname_managed);
    assert_eq!(h.display_name(), "ganymede");
    assert_eq!(h.hardware_model, "ThinkPad X1 Carbon Gen 14");
    host.set_pretty_hostname("Chris's ThinkPad").wait().unwrap();
    assert!(eventually(|| host.snapshot().display_name() == "Chris's ThinkPad"));
    assert!(matches!(host.set_static_hostname("io").wait(), Err(Error::ManagedBySystem(_))));
}

#[test]
fn zones_from_this_machines_tzdata() {
    let Ok(zones) = aizome_sys::datetime::zones() else {
        eprintln!("skipping: no tzdata");
        return;
    };
    assert!(zones.len() > 300);
    let chicago = zones.iter().find(|z| z.id == "America/Chicago").unwrap();
    assert_eq!(chicago.city, "Chicago");
    assert!(chicago.country_names.iter().any(|n| n.contains("United States")));
    assert!(zones.iter().any(|z| z.id == "UTC"));
    let regions = aizome_sys::datetime::by_region(&zones);
    assert!(regions.contains_key("Europe") && regions.contains_key("Pacific"));
    assert_eq!(aizome_sys::datetime::nearest(&zones, 35.68, 139.69).unwrap().id, "Asia/Tokyo");
}
