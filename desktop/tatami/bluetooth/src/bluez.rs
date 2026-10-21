// SPDX-License-Identifier: MIT OR Apache-2.0
//! All calls are asynchronous, with bounded deadlines. Each session is pinned
//! to BlueZ's unique bus owner: a daemon restart cannot complete an old action
//! against a newly discovered device. Discovery belongs to our bus connection.
use crate::model::{ADAPTER, DEVICE, Objects, Snapshot};
use glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

pub const AGENT_PATH: &str = "/org/tatami/Bluetooth/agent";
pub type Result<T> = std::result::Result<T, String>;

pub struct Bluez {
    pub connection: gio::DBusConnection,
    pub owner: String,
    pub agent_ready: Cell<bool>,
    pub stopped: Cell<bool>,
    scan: RefCell<Option<String>>,
}

impl Bluez {
    pub fn new(connection: gio::DBusConnection, owner: &str) -> Self {
        Self {
            connection,
            owner: owner.into(),
            agent_ready: Cell::new(false),
            stopped: Cell::new(false),
            scan: RefCell::new(None),
        }
    }

    pub async fn call(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        parameters: glib::Variant,
        timeout: i32,
    ) -> Result<glib::Variant> {
        self.connection
            .call_future(
                Some(&self.owner),
                path,
                interface,
                method,
                Some(&parameters),
                None,
                gio::DBusCallFlags::NONE,
                timeout,
            )
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn read(&self) -> Result<Snapshot> {
        let result = self
            .call(
                "/",
                "org.freedesktop.DBus.ObjectManager",
                "GetManagedObjects",
                ().to_variant(),
                5000,
            )
            .await?;
        let (objects,) = result
            .get::<(Objects,)>()
            .ok_or("Bluetooth returned an invalid device list")?;
        Ok(Snapshot::parse(objects))
    }

    pub async fn register(&self) -> Result<()> {
        let path = ObjectPath::try_from(AGENT_PATH).unwrap();
        self.call(
            "/org/bluez",
            "org.bluez.AgentManager1",
            "RegisterAgent",
            (&path, "KeyboardDisplay").to_variant(),
            5000,
        )
        .await?;
        self.agent_ready.set(true);
        // Our agent is used for our own Pair calls without replacing the
        // desktop's default agent. Incoming pairing is deliberately not offered.
        Ok(())
    }

    pub async fn set(
        &self,
        path: &str,
        interface: &str,
        property: &str,
        value: glib::Variant,
    ) -> Result<()> {
        self.call(
            path,
            "org.freedesktop.DBus.Properties",
            "Set",
            (interface, property, value).to_variant(),
            5000,
        )
        .await
        .map(|_| ())
    }

    pub async fn device(&self, path: &str, method: &str) -> Result<()> {
        let result = self
            .call(
                path,
                DEVICE,
                method,
                ().to_variant(),
                if method == "Pair" { 120_000 } else { 30_000 },
            )
            .await
            .map(|_| ());
        match result {
            Err(error)
                if (method == "Connect" && error.contains("org.bluez.Error.AlreadyConnected"))
                    || (method == "Disconnect"
                        && error.contains("org.bluez.Error.NotConnected"))
                    || (method == "Pair" && error.contains("org.bluez.Error.AlreadyExists")) =>
            {
                Ok(())
            }
            other => other,
        }
    }

    pub async fn remove(&self, adapter: &str, device: &str) -> Result<()> {
        let path = ObjectPath::try_from(device).map_err(|e| e.to_string())?;
        self.call(adapter, ADAPTER, "RemoveDevice", (path,).to_variant(), 5000)
            .await
            .map(|_| ())
    }

    /// Reconcile only our discovery session; never stop another client's scan
    /// or toggle global Discoverable/Pairable properties just by opening a window.
    pub fn observe(&self, snapshot: &Snapshot) {
        let active = self.scan.borrow().clone();
        if let Some(path) = active
            && !snapshot
                .adapters
                .iter()
                .any(|a| a.path == path && a.powered && a.discovering)
        {
            self.scan.borrow_mut().take();
        }
    }

    pub fn scanning(&self) -> bool {
        self.scan.borrow().is_some()
    }

    pub async fn scan(&self, adapter: Option<&str>) -> Result<()> {
        if self.scan.borrow().as_deref() == adapter {
            return Ok(());
        }
        let previous = self.scan.borrow_mut().take();
        if let Some(previous) = previous {
            let result = self
                .call(&previous, ADAPTER, "StopDiscovery", ().to_variant(), 5000)
                .await;
            if let Err(error) = result {
                // An adapter that went away or powered off already lost its session.
                if !error.contains("NotReady")
                    && !error.contains("UnknownObject")
                    && !error.contains("DoesNotExist")
                {
                    *self.scan.borrow_mut() = Some(previous);
                    return Err(error);
                }
            }
        }
        if let Some(path) = adapter.filter(|_| !self.stopped.get()) {
            let filter = HashMap::from([("Transport", "auto".to_variant())]);
            self.call(
                path,
                ADAPTER,
                "SetDiscoveryFilter",
                (filter,).to_variant(),
                5000,
            )
            .await?;
            self.call(path, ADAPTER, "StartDiscovery", ().to_variant(), 5000)
                .await?;
            *self.scan.borrow_mut() = Some(path.into());
        }
        Ok(())
    }

    pub async fn stop(&self) {
        self.stopped.set(true);
        let _ = self.scan(None).await;
        if self.agent_ready.replace(false) {
            let path = ObjectPath::try_from(AGENT_PATH).unwrap();
            let _ = self
                .call(
                    "/org/bluez",
                    "org.bluez.AgentManager1",
                    "UnregisterAgent",
                    (path,).to_variant(),
                    5000,
                )
                .await;
        }
    }
}
