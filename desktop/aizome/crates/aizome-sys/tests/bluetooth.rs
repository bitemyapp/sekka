//! Bluetooth against a fake BlueZ on a private bus, pairing agent included.

mod common;

use aizome_sys::bluetooth::{Bluetooth, DeviceKind, PairingRequest, PairingResponse};
use aizome_sys::{Error, Wake};
use common::{eventually, path};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zbus::fdo;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::OwnedObjectPath;

const ADAPTER: &str = "/org/bluez/hci0";
const SPEAKER: &str = "/org/bluez/hci0/dev_11_22_33_44_55_66";
const FOUND: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF";

#[derive(Default)]
struct Shared {
    /// The registered agent: its bus name and path.
    agent: Option<(String, OwnedObjectPath)>,
    default_agent: bool,
    /// A busy bluetoothd: how long registering an agent takes.
    slow: std::time::Duration,
}

struct AgentManager {
    shared: Arc<Mutex<Shared>>,
}

#[zbus::interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(
        &self,
        agent: OwnedObjectPath,
        capability: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<()> {
        assert_eq!(capability, "KeyboardDisplay");
        let slow = self.shared.lock().unwrap().slow;
        std::thread::sleep(slow);
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        self.shared.lock().unwrap().agent = Some((sender, agent));
        Ok(())
    }
    fn request_default_agent(&self, _agent: OwnedObjectPath) {
        self.shared.lock().unwrap().default_agent = true;
    }
    fn unregister_agent(&self, _agent: OwnedObjectPath) {
        self.shared.lock().unwrap().agent = None;
    }
}

#[derive(Default)]
struct Adapter {
    powered: bool,
    alias: String,
    discovering: bool,
    /// The test's `/dev/rfkill`, and what it held each time `Powered` was
    /// set: what had been written to the radio switches by then.
    rfkill: PathBuf,
    powered_with: PoweredWith,
}

/// Each value `Powered` was set to, with the device's contents then.
type PoweredWith = Arc<Mutex<Vec<(bool, Vec<u8>)>>>;

#[zbus::interface(name = "org.bluez.Adapter1")]
impl Adapter {
    #[zbus(property)]
    fn address(&self) -> String {
        "00:00:00:00:00:01".into()
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        self.powered
    }
    #[zbus(property)]
    fn set_powered(&mut self, on: bool) {
        self.powered_with.lock().unwrap().push((on, std::fs::read(&self.rfkill).unwrap_or_default()));
        self.powered = on;
    }
    #[zbus(property)]
    fn power_state(&self) -> String {
        if self.powered {
            "on".into()
        } else if self.alias == "blocked" {
            "off-blocked".into()
        } else {
            "off".into()
        }
    }
    #[zbus(property)]
    fn alias(&self) -> String {
        self.alias.clone()
    }
    #[zbus(property)]
    fn set_alias(&mut self, alias: String) {
        self.alias = alias;
    }
    #[zbus(property)]
    fn discovering(&self) -> bool {
        self.discovering
    }

    async fn start_discovery(
        &mut self,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.discovering = true;
        self.discovering_changed(&emitter).await?;
        server.at(FOUND, Device::new("AA:BB:CC:DD:EE:FF", "Phone", "phone", false)).await?;
        Ok(())
    }

    async fn stop_discovery(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.discovering = false;
        self.discovering_changed(&emitter).await?;
        Ok(())
    }

    async fn remove_device(
        &self,
        device: OwnedObjectPath,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<()> {
        server.remove::<Battery, _>(device.as_str()).await.ok();
        server.remove::<Device, _>(device.as_str()).await?;
        Ok(())
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum BluezError {
    AuthenticationRejected(String),
    AuthenticationCanceled(String),
}

struct Device {
    address: String,
    alias: String,
    icon: String,
    paired: std::sync::atomic::AtomicBool,
    trusted: bool,
    connected: bool,
    shared: Option<Arc<Mutex<Shared>>>,
}

impl Device {
    fn new(address: &str, alias: &str, icon: &str, paired: bool) -> Device {
        Device {
            address: address.into(),
            alias: alias.into(),
            icon: icon.into(),
            paired: paired.into(),
            trusted: false,
            connected: false,
            shared: None,
        }
    }
}

#[zbus::interface(name = "org.bluez.Device1")]
impl Device {
    #[zbus(property)]
    fn address(&self) -> String {
        self.address.clone()
    }
    #[zbus(property)]
    fn alias(&self) -> String {
        self.alias.clone()
    }
    #[zbus(property)]
    fn name(&self) -> String {
        self.alias.clone()
    }
    #[zbus(property)]
    fn icon(&self) -> String {
        self.icon.clone()
    }
    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        path(ADAPTER)
    }
    #[zbus(property)]
    fn paired(&self) -> bool {
        self.paired.load(std::sync::atomic::Ordering::SeqCst)
    }
    #[zbus(property)]
    fn trusted(&self) -> bool {
        self.trusted
    }
    #[zbus(property)]
    fn set_trusted(&mut self, on: bool) {
        self.trusted = on;
    }
    #[zbus(property)]
    fn connected(&self) -> bool {
        self.connected
    }

    /// Ask the registered agent to confirm 123456.
    async fn pair(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), BluezError> {
        let agent = self.shared.as_ref().and_then(|s| s.lock().unwrap().agent.clone());
        let Some((sender, agent_path)) = agent else {
            return Err(BluezError::AuthenticationRejected("no agent".into()));
        };
        let me = header.path().map(|p| p.to_owned()).unwrap();
        let reply = conn
            .call_method(
                Some(sender.as_str()),
                agent_path.as_str(),
                Some("org.bluez.Agent1"),
                "RequestConfirmation",
                &(me, 123456u32),
            )
            .await;
        match reply {
            Ok(_) => {
                self.paired.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = self.paired_changed(&emitter).await;
                Ok(())
            }
            Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.bluez.Error.Rejected" => {
                Err(BluezError::AuthenticationRejected("Rejected".into()))
            }
            Err(_) => Err(BluezError::AuthenticationCanceled("Canceled".into())),
        }
    }

    async fn connect(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.connected = true;
        self.connected_changed(&emitter).await?;
        Ok(())
    }

    async fn disconnect(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.connected = false;
        self.connected_changed(&emitter).await?;
        Ok(())
    }
}

struct Battery;

#[zbus::interface(name = "org.bluez.Battery1")]
impl Battery {
    #[zbus(property)]
    fn percentage(&self) -> u8 {
        64
    }
}

fn serve(bus: &common::TestBus) -> (zbus::blocking::Connection, Arc<Mutex<Shared>>) {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut speaker = Device::new("11:22:33:44:55:66", "Speaker", "audio-card", false);
    speaker.shared = Some(shared.clone());
    let conn = bus
        .builder()
        .name("org.bluez")
        .unwrap()
        .serve_at("/", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at("/org/bluez", AgentManager { shared: shared.clone() })
        .unwrap()
        .serve_at(ADAPTER, Adapter { powered: true, alias: "ganymede".into(), ..Adapter::default() })
        .unwrap()
        .serve_at(SPEAKER, speaker)
        .unwrap()
        .serve_at(SPEAKER, Battery)
        .unwrap()
        .build()
        .unwrap();
    (conn, shared)
}

/// Radio switches of the test's own, in place of `/dev/rfkill` and
/// `/sys/class/rfkill` (switching an adapter writes to them): `hci0`'s
/// switch, as the kernel shows it, with index 7.
struct Switches {
    dir: PathBuf,
}

impl Switches {
    fn new(test: &str) -> Switches {
        let dir = std::env::temp_dir().join(format!("aizome-sys-bt-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let switch = dir.join("sys/rfkill7");
        std::fs::create_dir_all(&switch).unwrap();
        for (file, value) in [("index", "7"), ("name", "hci0"), ("type", "bluetooth"), ("soft", "0"), ("hard", "0")] {
            std::fs::write(switch.join(file), format!("{value}\n")).unwrap();
        }
        std::fs::write(dir.join("rfkill"), b"").unwrap();
        Switches { dir }
    }

    fn device(&self) -> PathBuf {
        self.dir.join("rfkill")
    }

    fn sysfs(&self) -> PathBuf {
        self.dir.join("sys")
    }

    /// The event last written to the device.
    fn written(&self) -> Vec<u8> {
        std::fs::read(self.device()).unwrap()
    }

    /// As the kernel does once a block (or an unblock) is written.
    fn show_blocked(&self, blocked: bool) {
        std::fs::write(self.dir.join("sys/rfkill7/soft"), if blocked { "1\n" } else { "0\n" }).unwrap();
    }
}

impl Drop for Switches {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// What `log::warn!` said, for the test that asks what was reported.
struct Warnings(Mutex<Vec<String>>);

static WARNINGS: Warnings = Warnings(Mutex::new(Vec::new()));

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}

#[test]
fn follows_adapters_and_devices() {
    let bus = test_bus!();
    let (_fake, _shared) = serve(&bus);
    let switches = Switches::new("follows");
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none()).with_rfkill(switches.device(), switches.sysfs());
    assert!(eventually(|| bt.snapshot().devices.len() == 1));
    let state = bt.snapshot();
    assert!(state.available && state.powered());
    let speaker = &state.devices[0];
    assert_eq!((speaker.name.as_str(), speaker.kind), ("Speaker", DeviceKind::Speaker));
    assert_eq!(speaker.battery, Some(64));

    bt.set_powered(ADAPTER, false).wait().unwrap();
    assert!(eventually(|| !bt.snapshot().powered()));
    bt.set_adapter_name(ADAPTER, "callisto").wait().unwrap();
    assert!(eventually(|| bt.snapshot().default_adapter().unwrap().alias == "callisto"));

    bt.start_discovery(ADAPTER).wait().unwrap();
    assert!(eventually(|| bt.snapshot().device(FOUND).is_some()));
    assert!(bt.snapshot().default_adapter().unwrap().discovering);
    assert_eq!(bt.snapshot().device(FOUND).unwrap().kind, DeviceKind::Phone);

    bt.remove(FOUND).wait().unwrap();
    assert!(eventually(|| bt.snapshot().device(FOUND).is_none()));
}

#[test]
fn pairing_asks_the_agent() {
    let bus = test_bus!();
    let (_fake, shared) = serve(&bus);
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| bt.available()));

    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = prompts.clone();
    // The UI answers on its own thread, later.
    bt.register_agent(move |prompt| {
        seen.lock().unwrap().push((prompt.name.clone(), prompt.request.clone()));
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            match prompt.request {
                PairingRequest::Confirm(123456) => prompt.responder.respond(PairingResponse::Accept),
                _ => prompt.responder.reject(),
            }
        });
    })
    .wait()
    .unwrap();
    assert!(shared.lock().unwrap().default_agent);

    bt.pair(SPEAKER).wait().unwrap();
    assert_eq!(*prompts.lock().unwrap(), [("Speaker".to_owned(), PairingRequest::Confirm(123456))]);
    assert!(eventually(|| bt.snapshot().device(SPEAKER).is_some_and(|d| d.paired && d.trusted && d.connected)));

    bt.disconnect(SPEAKER).wait().unwrap();
    assert!(eventually(|| bt.snapshot().device(SPEAKER).is_some_and(|d| !d.connected)));

    bt.unregister_agent().wait().unwrap();
    assert!(shared.lock().unwrap().agent.is_none());
}

/// Settings registers the agent when its Bluetooth pane shows, from its UI
/// thread: a busy bluetoothd mustn't stop the window drawing or moving.
#[test]
fn registering_the_agent_never_waits_for_bluez() {
    let bus = test_bus!();
    let (_fake, shared) = serve(&bus);
    shared.lock().unwrap().slow = std::time::Duration::from_millis(800);
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| bt.available()));
    let asked = std::time::Instant::now();
    let first = bt.register_agent(|_| {});
    // The pane hidden and shown again at once: done in that order, so
    // the agent ends up registered.
    let _ = bt.unregister_agent();
    let again = bt.register_agent(|_| {});
    let took = asked.elapsed();
    assert!(took < std::time::Duration::from_millis(200), "the caller waited on BlueZ for {took:?}");
    first.wait().unwrap();
    again.wait().unwrap();
    assert!(shared.lock().unwrap().agent.is_some());
    assert!(shared.lock().unwrap().default_agent);
}

#[test]
fn a_rejected_code_fails_pairing() {
    let bus = test_bus!();
    let (_fake, _shared) = serve(&bus);
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| bt.available()));
    // Dropping the responder rejects.
    bt.register_agent(drop).wait().unwrap();
    let result = bt.pair(SPEAKER).wait();
    assert!(matches!(result, Err(Error::Rejected(_))), "{result:?}");
    assert!(!bt.snapshot().device(SPEAKER).unwrap().paired);
}

#[test]
fn without_bluez_nothing_is_there() {
    let bus = test_bus!();
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!bt.available());
    assert!(bt.snapshot().adapters.is_empty());
    assert!(bt.register_agent(|_| {}).wait().is_err());
}

#[test]
fn a_blocked_adapter_is_unblocked_before_powering_on() {
    let bus = test_bus!();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let _fake = bus
        .builder()
        .name("org.bluez")
        .unwrap()
        .serve_at("/", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at("/org/bluez", AgentManager { shared })
        .unwrap()
        .serve_at(ADAPTER, Adapter { powered: false, alias: "blocked".into(), ..Adapter::default() })
        .unwrap()
        .build()
        .unwrap();
    let rfkill = std::env::temp_dir().join(format!("aizome-sys-bt-rfkill-{}", std::process::id()));
    std::fs::write(&rfkill, b"").unwrap();
    // (No sysfs here: BlueZ alone says the adapter is blocked.)
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none()).with_rfkill(rfkill.clone(), rfkill.with_extension("sys"));
    assert!(eventually(|| bt.snapshot().default_adapter().is_some_and(|a| a.blocked())));
    bt.set_powered(ADAPTER, true).wait().unwrap();
    let written = std::fs::read(&rfkill).unwrap();
    std::fs::remove_file(&rfkill).unwrap();
    // struct rfkill_event { idx 0, type BLUETOOTH, op CHANGE_ALL, soft 0, hard 0 }
    assert_eq!(written, [0, 0, 0, 0, 2, 3, 0, 0]);
    assert!(eventually(|| bt.snapshot().powered()));
}

/// BlueZ switches every adapter on when it starts and keeps no record of
/// `Powered`, so off is kept by the adapter's own radio switch (a blocked
/// adapter BlueZ leaves off, and systemd-rfkill puts the block back at each
/// start); on clears the block before asking BlueZ.
#[test]
fn switching_off_blocks_the_adapters_radio_switch_and_on_unblocks_it_first() {
    let bus = test_bus!();
    let switches = Switches::new("stays-off");
    let powered_with = PoweredWith::default();
    let adapter = Adapter {
        powered: true,
        alias: "ganymede".into(),
        rfkill: switches.device(),
        powered_with: powered_with.clone(),
        ..Adapter::default()
    };
    let _fake = bus
        .builder()
        .name("org.bluez")
        .unwrap()
        .serve_at("/", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at(ADAPTER, adapter)
        .unwrap()
        .build()
        .unwrap();
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none()).with_rfkill(switches.device(), switches.sysfs());
    assert!(eventually(|| bt.snapshot().powered()));

    bt.set_powered(ADAPTER, false).wait().unwrap();
    // struct rfkill_event { idx 7, type BLUETOOTH, op CHANGE, soft 1, hard 0 }:
    // hci0's switch alone (every Bluetooth switch would take another
    // adapter with it, and on some laptops the adapter itself away).
    assert_eq!(switches.written(), [&7u32.to_ne_bytes()[..], &[2, 2, 1, 0]].concat());
    // Written once BlueZ had switched the adapter off.
    assert_eq!(*powered_with.lock().unwrap(), [(false, vec![])]);
    assert!(eventually(|| !bt.snapshot().powered()));

    // The kernel shows the block at once; BlueZ's `off-blocked` comes a
    // moment later (never, from this fake).
    switches.show_blocked(true);
    assert!(!bt.snapshot().default_adapter().unwrap().blocked());
    bt.set_powered(ADAPTER, true).wait().unwrap();
    // struct rfkill_event { idx 0, type BLUETOOTH, op CHANGE_ALL, soft 0, hard 0 },
    // there by the time BlueZ was asked.
    let unblock = vec![0, 0, 0, 0, 2, 3, 0, 0];
    assert_eq!(switches.written(), unblock);
    assert_eq!(powered_with.lock().unwrap().last(), Some(&(true, unblock)));
    assert!(eventually(|| bt.snapshot().powered()));
}

/// Where the radio switch can't be written (no seat's `uaccess`, no rfkill)
/// the adapter still goes off, and the log says for how long.
#[test]
fn switching_off_without_the_radio_switch_still_switches_off_and_says_so() {
    let _ = log::set_logger(&WARNINGS);
    log::set_max_level(log::LevelFilter::Warn);
    let bus = test_bus!();
    let (_fake, _shared) = serve(&bus);
    let switches = Switches::new("no-rfkill");
    std::fs::remove_file(switches.device()).unwrap();
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none()).with_rfkill(switches.device(), switches.sysfs());
    assert!(eventually(|| bt.snapshot().powered()));

    bt.set_powered(ADAPTER, false).wait().unwrap();
    assert!(eventually(|| !bt.snapshot().powered()));
    assert!(!switches.device().exists(), "something was made in the device's place");
    let warnings = WARNINGS.0.lock().unwrap();
    assert!(
        warnings.iter().any(|w| w.contains(ADAPTER) && w.contains("only until BlueZ next starts")),
        "nothing said the switch won't last: {warnings:?}"
    );
}

/// BlueZ's `Cancel` names no device: the agent says which one it was
/// about (the one it asked last), so the UI can close that question.
#[test]
fn a_cancel_from_bluez_names_the_device_it_was_about() {
    let bus = test_bus!();
    let (fake, shared) = serve(&bus);
    let bt = Bluetooth::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| bt.available()));
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = prompts.clone();
    bt.register_agent(move |p| seen.lock().unwrap().push(p)).wait().unwrap();
    let pairing = bt.pair(SPEAKER);
    assert!(eventually(|| prompts.lock().unwrap().len() == 1));
    let (sender, agent) = shared.lock().unwrap().agent.clone().unwrap();
    fake.call_method(Some(sender.as_str()), agent.as_str(), Some("org.bluez.Agent1"), "Cancel", &()).unwrap();
    assert!(eventually(|| prompts.lock().unwrap().len() == 2));
    {
        let p = &prompts.lock().unwrap()[1];
        assert_eq!(p.request, PairingRequest::Cancelled);
        assert_eq!(p.device, SPEAKER);
    }
    // The question left unanswered is a no: pairing ends.
    let first = prompts.lock().unwrap().remove(0);
    first.responder.reject();
    assert!(pairing.wait().is_err());
}
