//! Power services against fake UPower, power-profiles-daemon and logind on
//! a private bus (nothing here suspends or powers off anything).

mod common;

use aizome_sys::Wake;
use aizome_sys::power::{Backlight, BatteryState, Can, Logind, PowerKind, PowerProfiles, SessionEvent, Upower};
use common::{eventually, path};
use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

// --- UPower ---------------------------------------------------------------

struct UpowerManager;

#[zbus::interface(name = "org.freedesktop.UPower")]
impl UpowerManager {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn lid_is_present(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn lid_is_closed(&self) -> bool {
        false
    }
    fn enumerate_devices(&self) -> Vec<OwnedObjectPath> {
        vec![
            path("/org/freedesktop/UPower/devices/battery_BAT0"),
            path("/org/freedesktop/UPower/devices/line_power_AC"),
        ]
    }
    fn get_display_device(&self) -> OwnedObjectPath {
        path("/org/freedesktop/UPower/devices/DisplayDevice")
    }
    fn enumerate_kbd_backlights(&self) -> Vec<OwnedObjectPath> {
        vec![path("/org/freedesktop/UPower/KbdBacklight/kbd")]
    }
    #[zbus(signal)]
    async fn device_added(emitter: &SignalEmitter<'_>, device: OwnedObjectPath) -> zbus::Result<()>;
}

struct PowerDevice {
    kind: u32,
    percentage: f64,
    state: u32,
    online: bool,
    model: &'static str,
    power_supply: bool,
}

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl PowerDevice {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> u32 {
        self.kind
    }
    #[zbus(property)]
    fn percentage(&self) -> f64 {
        self.percentage
    }
    #[zbus(property)]
    fn state(&self) -> u32 {
        self.state
    }
    #[zbus(property)]
    fn online(&self) -> bool {
        self.online
    }
    #[zbus(property)]
    fn model(&self) -> String {
        self.model.into()
    }
    #[zbus(property)]
    fn power_supply(&self) -> bool {
        self.power_supply
    }
    #[zbus(property)]
    fn is_present(&self) -> bool {
        self.kind != 1
    }
    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        5400
    }
    #[zbus(property)]
    fn energy_rate(&self) -> f64 {
        7.25
    }
}

fn device(kind: u32, percentage: f64, state: u32, model: &'static str, power_supply: bool) -> PowerDevice {
    PowerDevice { kind, percentage, state, online: false, model, power_supply }
}

struct Kbd {
    level: i32,
}

#[zbus::interface(name = "org.freedesktop.UPower.KbdBacklight")]
impl Kbd {
    fn get_max_brightness(&self) -> i32 {
        2
    }
    fn get_brightness(&self) -> i32 {
        self.level
    }
    async fn set_brightness(
        &mut self,
        value: i32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.level = value;
        Kbd::brightness_changed(&emitter, value).await?;
        Ok(())
    }
    #[zbus(signal)]
    async fn brightness_changed(emitter: &SignalEmitter<'_>, value: i32) -> zbus::Result<()>;
}

#[test]
fn upower_batteries_peripherals_and_keyboard_light() {
    let bus = test_bus!();
    let fake = bus
        .builder()
        .name("org.freedesktop.UPower")
        .unwrap()
        .serve_at("/org/freedesktop/UPower", UpowerManager)
        .unwrap()
        .serve_at("/org/freedesktop/UPower/devices/battery_BAT0", device(2, 57.0, 2, "5B10W51867", true))
        .unwrap()
        .serve_at("/org/freedesktop/UPower/devices/line_power_AC", device(1, 0.0, 0, "", true))
        .unwrap()
        .serve_at("/org/freedesktop/UPower/devices/DisplayDevice", device(2, 57.0, 2, "", true))
        .unwrap()
        .serve_at("/org/freedesktop/UPower/KbdBacklight/kbd", Kbd { level: 0 })
        .unwrap()
        .build()
        .unwrap();
    let upower = Upower::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| upower.available() && upower.snapshot().display.is_some()));
    let state = upower.snapshot();
    assert!(state.on_battery && state.lid_present && !state.lid_closed);
    let battery = state.batteries().next().unwrap();
    assert_eq!((battery.percentage, battery.state), (57.0, BatteryState::Discharging));
    assert_eq!(battery.time_to_empty, Some(std::time::Duration::from_secs(5400)));
    assert_eq!(state.devices.len(), 2, "the display device is apart");
    assert_eq!(state.keyboard_backlights[0].max_brightness, 2);

    // The battery drains: PropertiesChanged reaches the snapshot.
    zbus::block_on(async {
        let iface = fake
            .inner()
            .object_server()
            .interface::<_, PowerDevice>("/org/freedesktop/UPower/devices/battery_BAT0")
            .await
            .unwrap();
        iface.get_mut().await.percentage = 42.0;
        iface.get().await.percentage_changed(iface.signal_emitter()).await.unwrap();
    });
    assert!(eventually(|| upower.snapshot().batteries().next().is_some_and(|b| b.percentage == 42.0)));

    // A mouse connects.
    zbus::block_on(async {
        let server = fake.inner().object_server();
        let mouse = "/org/freedesktop/UPower/devices/mouse_1";
        server.at(mouse, device(5, 80.0, 2, "MX Master 3", false)).await.unwrap();
        let manager = server.interface::<_, UpowerManager>("/org/freedesktop/UPower").await.unwrap();
        UpowerManager::device_added(manager.signal_emitter(), path(mouse)).await.unwrap();
    });
    assert!(eventually(|| upower
        .snapshot()
        .peripherals()
        .any(|d| d.kind == PowerKind::Mouse && d.model == "MX Master 3")));

    // The keyboard light key cycles 0 → 1.
    assert_eq!(upower.cycle_keyboard_brightness().wait().unwrap(), 1);
    assert!(eventually(|| upower.snapshot().keyboard_backlights[0].brightness == 1));
    upower.set_keyboard_brightness("/org/freedesktop/UPower/KbdBacklight/kbd", 9).wait().unwrap();
    assert!(eventually(|| upower.snapshot().keyboard_backlights[0].brightness == 2), "clamped to the maximum");
}

// --- power-profiles-daemon ---------------------------------------------------

struct Profiles {
    active: String,
    holds: Vec<(u32, String)>,
}

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.active.clone()
    }
    #[zbus(property)]
    fn set_active_profile(&mut self, profile: String) -> fdo::Result<()> {
        if !["power-saver", "balanced", "performance"].contains(&profile.as_str()) {
            return Err(fdo::Error::InvalidArgs(profile));
        }
        self.active = profile;
        Ok(())
    }
    #[zbus(property)]
    fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
        ["power-saver", "balanced", "performance"]
            .iter()
            .map(|p| {
                HashMap::from([
                    ("Profile".to_owned(), OwnedValue::try_from(Value::from(*p)).unwrap()),
                    ("Driver".to_owned(), OwnedValue::try_from(Value::from("platform_profile")).unwrap()),
                ])
            })
            .collect()
    }
    #[zbus(property)]
    fn performance_degraded(&self) -> String {
        String::new()
    }
    fn hold_profile(&mut self, profile: String, _reason: String, _app: String) -> u32 {
        let cookie = self.holds.len() as u32 + 1;
        self.holds.push((cookie, profile));
        cookie
    }
    fn release_profile(&mut self, cookie: u32) {
        self.holds.retain(|(c, _)| *c != cookie);
    }
}

#[test]
fn power_profiles_switch_and_hold() {
    let bus = test_bus!();
    let _fake = bus
        .builder()
        .name("org.freedesktop.UPower.PowerProfiles")
        .unwrap()
        .serve_at("/org/freedesktop/UPower/PowerProfiles", Profiles { active: "balanced".into(), holds: Vec::new() })
        .unwrap()
        .build()
        .unwrap();
    let profiles = PowerProfiles::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| profiles.snapshot().active == "balanced"));
    assert!(profiles.snapshot().has("performance"));
    assert_eq!(profiles.snapshot().profiles[0].driver, "platform_profile");
    profiles.set_active("power-saver").wait().unwrap();
    assert!(eventually(|| profiles.snapshot().active == "power-saver"));
    assert!(profiles.set_active("turbo").wait().is_err());
    let cookie = profiles.hold("performance", "Compiling", "org.example.Builder").wait().unwrap();
    profiles.release(cookie).wait().unwrap();
}

#[test]
fn power_profiles_absent() {
    let bus = test_bus!();
    let profiles = PowerProfiles::with_bus(bus.bus(), Wake::none());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!profiles.available());
    assert!(profiles.set_active("balanced").wait().is_err());
}

// --- logind -------------------------------------------------------------------

#[derive(Default)]
struct LogindLog {
    calls: Vec<String>,
    inhibits: usize,
    /// The read ends of the inhibitor pipes handed out.
    inhibitor_reads: Vec<OwnedFd>,
    brightness: Vec<(String, String, u32)>,
}

struct LoginManager {
    log: Arc<Mutex<LogindLog>>,
}

fn pipe() -> (OwnedFd, OwnedFd) {
    let mut fds = [0; 2];
    // SAFETY: pipe2 fills two descriptors we then own.
    assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) }, 0);
    // SAFETY: fresh descriptors.
    unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl LoginManager {
    fn can_suspend(&self) -> String {
        "yes".into()
    }
    fn can_hibernate(&self) -> String {
        "na".into()
    }
    fn can_power_off(&self) -> String {
        "challenge".into()
    }
    fn can_reboot(&self) -> String {
        "yes".into()
    }
    fn suspend(&self, interactive: bool, #[zbus(header)] header: zbus::message::Header<'_>) {
        let flag = header.primary().flags().contains(zbus::message::Flags::AllowInteractiveAuth);
        self.log.lock().unwrap().calls.push(format!("Suspend({interactive}, flag={flag})"));
    }
    fn get_session(&self, id: String) -> fdo::Result<OwnedObjectPath> {
        if id == "7" {
            Ok(path("/org/freedesktop/login1/session/_37"))
        } else {
            Err(fdo::Error::Failed("no session".into()))
        }
    }
    fn inhibit(&self, what: String, who: String, _why: String, mode: String) -> zbus::zvariant::OwnedFd {
        let mut log = self.log.lock().unwrap();
        log.calls.push(format!("Inhibit({what}, {who}, {mode})"));
        log.inhibits += 1;
        let (read, write) = pipe();
        log.inhibitor_reads.push(read);
        zbus::zvariant::OwnedFd::from(write)
    }
    #[zbus(property)]
    fn lid_closed(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn idle_hint(&self) -> bool {
        false
    }
    #[zbus(signal)]
    async fn prepare_for_sleep(emitter: &SignalEmitter<'_>, start: bool) -> zbus::Result<()>;
}

struct LoginSession {
    log: Arc<Mutex<LogindLog>>,
    sysfs: PathBuf,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl LoginSession {
    #[zbus(property)]
    fn id(&self) -> String {
        "7".into()
    }
    #[zbus(property)]
    fn active(&self) -> bool {
        true
    }
    async fn lock(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        LoginSession::lock_signal(&emitter).await?;
        Ok(())
    }
    #[zbus(signal, name = "Lock")]
    async fn lock_signal(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    fn set_idle_hint(&self, idle: bool) {
        self.log.lock().unwrap().calls.push(format!("SetIdleHint({idle})"));
    }
    /// As the kernel would: the new level shows in sysfs.
    fn set_brightness(&self, subsystem: String, name: String, level: u32) {
        let _ = std::fs::write(self.sysfs.join(&name).join("actual_brightness"), format!("{level}\n"));
        self.log.lock().unwrap().brightness.push((subsystem, name, level));
    }
}

fn serve_logind(bus: &common::TestBus, sysfs: PathBuf) -> (zbus::blocking::Connection, Arc<Mutex<LogindLog>>) {
    let log = Arc::new(Mutex::new(LogindLog::default()));
    let conn = bus
        .builder()
        .name("org.freedesktop.login1")
        .unwrap()
        .serve_at("/org/freedesktop/login1", LoginManager { log: log.clone() })
        .unwrap()
        .serve_at("/org/freedesktop/login1/session/_37", LoginSession { log: log.clone(), sysfs })
        .unwrap()
        .build()
        .unwrap();
    (conn, log)
}

/// Whether every write end of the pipe whose read end is `read` is closed.
fn released(read: &OwnedFd) -> bool {
    let mut byte = 0u8;
    // SAFETY: reading one byte into a valid buffer from a pipe we own.
    let n = unsafe { libc::read(read.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
    n == 0
}

#[test]
fn logind_actions_events_and_the_sleep_delay() {
    let bus = test_bus!();
    let (fake, log) = serve_logind(&bus, std::env::temp_dir());
    let logind = Logind::with_bus(bus.bus(), Wake::none(), Some("7".into()));
    assert!(eventually(|| logind.available()));
    let state = logind.snapshot();
    assert_eq!(state.session_path, "/org/freedesktop/login1/session/_37");
    assert_eq!(
        (state.can_suspend, state.can_hibernate, state.can_power_off),
        (Can::Yes, Can::NotApplicable, Can::Challenge)
    );
    assert!(state.session_active);
    assert!(state.can_power_off.offered() && !state.can_hibernate.offered());

    // Actions ask for interactive authorization.
    logind.suspend().wait().unwrap();
    assert!(log.lock().unwrap().calls.contains(&"Suspend(true, flag=true)".to_owned()));

    // A lock request comes back as an event.
    logind.lock_session().wait().unwrap();
    assert!(eventually(|| logind.take_events().contains(&SessionEvent::Lock)));

    // Sleep waits for the screen to lock.
    logind.take_sleep_delay().unwrap();
    assert!(logind.snapshot().sleep_delay_held);
    assert!(log.lock().unwrap().calls.contains(&"Inhibit(sleep, Aizome, delay)".to_owned()));
    let emit = |start: bool| {
        fake.emit_signal(
            None::<&str>,
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
            &(start,),
        )
        .unwrap();
    };
    emit(true);
    assert!(eventually(|| logind.snapshot().preparing_for_sleep));
    assert_eq!(logind.take_events(), [SessionEvent::PrepareForSleep]);
    assert!(!released(&log.lock().unwrap().inhibitor_reads[0]), "held until the screen is locked");
    logind.sleep_ready();
    assert!(eventually(|| released(&log.lock().unwrap().inhibitor_reads[0])));
    // After resuming the delay is taken again.
    emit(false);
    assert!(eventually(|| log.lock().unwrap().inhibits == 2));
    assert!(eventually(|| logind.snapshot().sleep_delay_held && !logind.snapshot().preparing_for_sleep));
    assert_eq!(logind.take_events(), [SessionEvent::Resumed]);

    // Other inhibitors are released when dropped.
    let keep_awake = logind.inhibit("idle:sleep", "Playing a video", "block").wait().unwrap();
    assert_eq!(log.lock().unwrap().inhibits, 3);
    drop(keep_awake);
    assert!(eventually(|| released(&log.lock().unwrap().inhibitor_reads[2])));

    logind.set_idle_hint(true).wait().unwrap();
    assert!(log.lock().unwrap().calls.contains(&"SetIdleHint(true)".to_owned()));
}

#[test]
fn backlight_steps_through_logind() {
    let bus = test_bus!();
    let root = std::env::temp_dir().join(format!("aizome-sys-bl-{}", std::process::id()));
    let dir = root.join("intel_backlight");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("max_brightness"), "496\n").unwrap();
    std::fs::write(dir.join("actual_brightness"), "248\n").unwrap();
    std::fs::write(dir.join("type"), "raw\n").unwrap();
    let (_fake, log) = serve_logind(&bus, root.clone());
    let backlight = Backlight::with(
        Some(bus.bus()),
        root.clone(),
        Some("/org/freedesktop/login1/session/_37".into()),
        Wake::none(),
    );
    assert!(backlight.available());
    let level = backlight.step(0.05).wait().unwrap();
    assert!((level - 0.55).abs() < 0.005, "{level}");
    assert_eq!(log.lock().unwrap().brightness, [("backlight".to_owned(), "intel_backlight".to_owned(), 273)]);
    // The watcher sees the level the kernel now reports.
    assert!(eventually(|| backlight.snapshot().devices[0].brightness == 273));
    backlight.set_fraction("intel_backlight", 0.0).wait().unwrap();
    assert_eq!(log.lock().unwrap().brightness.last().unwrap().2, 0);
    assert!(backlight.set_brightness("nonexistent", 3).wait().is_err());
    std::fs::remove_dir_all(&root).unwrap();
}

/// A daemon announcing thousands of devices at once, each needing a call
/// back to it: the follower keeps up, and the shared connection doesn't
/// stall on full queues while it waits for replies.
#[test]
fn a_burst_of_new_devices_doesnt_stall_the_connection() {
    let bus = test_bus!();
    let fake = bus
        .builder()
        .name("org.freedesktop.UPower")
        .unwrap()
        .serve_at("/org/freedesktop/UPower", UpowerManager)
        .unwrap()
        .serve_at("/org/freedesktop/UPower/devices/battery_BAT0", device(2, 57.0, 2, "", true))
        .unwrap()
        .serve_at("/org/freedesktop/UPower/devices/line_power_AC", device(1, 0.0, 0, "", true))
        .unwrap()
        .serve_at("/org/freedesktop/UPower/devices/DisplayDevice", device(2, 57.0, 2, "", true))
        .unwrap()
        .build()
        .unwrap();
    let upower = Upower::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| upower.snapshot().devices.len() == 2));
    const N: usize = 2500;
    let start = std::time::Instant::now();
    zbus::block_on(async {
        let server = fake.inner().object_server();
        let manager = server.interface::<_, UpowerManager>("/org/freedesktop/UPower").await.unwrap();
        for n in 0..N {
            let p = format!("/org/freedesktop/UPower/devices/mouse_{n}");
            server.at(p.as_str(), device(5, 50.0, 2, "Mouse", false)).await.unwrap();
            UpowerManager::device_added(manager.signal_emitter(), path(&p)).await.unwrap();
        }
    });
    let deadline = std::time::Duration::from_secs(20);
    while upower.snapshot().devices.len() < N + 2 && start.elapsed() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(upower.snapshot().devices.len(), N + 2, "after {:?}", start.elapsed());
    // And the connection still answers at once.
    let quick = std::time::Instant::now();
    upower.set_keyboard_brightness("/nonexistent", 1).wait().unwrap_err();
    assert!(quick.elapsed() < std::time::Duration::from_secs(1));
    eprintln!("{N} devices followed in {:?}", start.elapsed());
}
