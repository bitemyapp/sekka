//! A fake BlueZ for STB-09: `org.bluez` on the bus at ARGV[1] with one
//! powered adapter (`/org/bluez/hci0`) whose `Discoverable` can be set,
//! discovery, and an agent manager that takes any agent. Each change of
//! `Discoverable` is written to the file ARGV[2] as `discoverable=BOOL`.

use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

struct AgentManager;

#[zbus::interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(&self, _agent: OwnedObjectPath, _capability: String) {}
    fn request_default_agent(&self, _agent: OwnedObjectPath) {}
    fn unregister_agent(&self, _agent: OwnedObjectPath) {}
}

struct Adapter {
    discoverable: bool,
    discovering: bool,
    state: std::path::PathBuf,
}

impl Adapter {
    fn record(&self) {
        let _ = std::fs::write(&self.state, format!("discoverable={}\n", self.discoverable));
    }
}

#[zbus::interface(name = "org.bluez.Adapter1")]
impl Adapter {
    #[zbus(property)]
    fn address(&self) -> String {
        "00:00:00:00:00:01".into()
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn power_state(&self) -> String {
        "on".into()
    }
    #[zbus(property)]
    fn alias(&self) -> String {
        "kohaku".into()
    }
    #[zbus(property)]
    fn pairable(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn discoverable_timeout(&self) -> u32 {
        // As NixOS's bluetoothd defaults to here: stays discoverable.
        0
    }
    #[zbus(property)]
    fn discovering(&self) -> bool {
        self.discovering
    }
    #[zbus(property)]
    fn discoverable(&self) -> bool {
        self.discoverable
    }
    #[zbus(property)]
    fn set_discoverable(&mut self, on: bool) {
        self.discoverable = on;
        self.record();
    }

    async fn start_discovery(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.discovering = true;
        self.discovering_changed(&emitter).await?;
        Ok(())
    }

    async fn stop_discovery(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.discovering = false;
        self.discovering_changed(&emitter).await?;
        Ok(())
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let address = args.next().expect("a bus address");
    let state = std::path::PathBuf::from(args.next().expect("a state file"));
    let adapter = Adapter { discoverable: false, discovering: false, state };
    adapter.record();
    let _conn = zbus::blocking::connection::Builder::address(address.as_str())
        .expect("address")
        .name("org.bluez")
        .expect("name")
        .serve_at("/", zbus::fdo::ObjectManager)
        .expect("object manager")
        .serve_at("/org/bluez", AgentManager)
        .expect("agent manager")
        .serve_at("/org/bluez/hci0", adapter)
        .expect("adapter")
        .build()
        .expect("serve");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
