//! Apps' status icons, the systray's other half: KDE's StatusNotifierItem
//! protocol on the session bus, as libappindicator and
//! libayatana-appindicator (Steam, Discord, Dropbox), Qt's
//! `QSystemTrayIcon`, Electron and Chromium speak it.
//!
//! [`StatusNotifier::start`] serves `org.kde.StatusNotifierWatcher` at
//! `/StatusNotifierWatcher` and takes its name. Apps register their items
//! with it (each an object serving `org.kde.StatusNotifierItem`), and since
//! the shell is the only host, the watcher follows the items itself rather
//! than through a separate host: it reads each one's properties as it
//! registers, again on its `NewIcon`, `NewTitle`, `NewStatus` (and the
//! rest) signals, and drops it when its connection leaves the bus. Apps
//! that started before the watcher (or before a shell restart) register
//! when its name appears, as libappindicator and Qt watch for it.
//!
//! Two threads follow the items, as [`mpris`](crate::mpris)'s do: one only
//! moves the signals off zbus's queues, the other reads the items and
//! applies the signals and registrations. An item that hangs delays the
//! others' updates by the query timeout (2 seconds).
//!
//! The [`snapshot`](StatusNotifier::snapshot) lists the items in the order
//! they registered. Clicks reach them as [`activate`](StatusNotifier::activate)
//! and the rest; an item's menu (`com.canonical.dbusmenu`) is read whole by
//! [`menu`](StatusNotifier::menu) as it opens, and a row picked is sent
//! back with [`menu_clicked`](StatusNotifier::menu_clicked).
//!
//! Without a session bus, or when another watcher has the name and won't
//! give it up, [`StatusNotifier::available`] is false and there are no
//! items.

mod item;
mod menu;

pub use item::{
    Category, DEFAULT_PATH, ITEM_IFACE, ItemIcon, ItemStatus, Pixmap, StatusItem, best_pixmap, registration,
};
pub use menu::{MENU_IFACE, MenuItem, Toggle, strip_mnemonic};

use crate::bus::{self, Bus, Opts, Props};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, lock, service_handle};
use futures_lite::StreamExt;
use std::collections::{BTreeMap, BTreeSet};
use std::pin::Pin;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::fdo::{self, RequestNameFlags, RequestNameReply};
use zbus::message::Message;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

/// The watcher's bus name and object.
pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const DBUS: &str = "org.freedesktop.DBus";

const OWNER_RULE: &str = "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',\
                          member='NameOwnerChanged'";
const ITEM_RULE: &str = "type='signal',interface='org.kde.StatusNotifierItem'";

/// Clicks: items answer at once or not at all, though showing a window
/// may take a moment.
const ACTION: Opts = Opts { timeout: Duration::from_secs(5), interactive: false };

/// The items registered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusNotifierState {
    /// Every item, in the order they registered.
    pub items: Vec<StatusItem>,
}

impl StatusNotifierState {
    /// The item listed as `key`.
    pub fn item(&self, key: &str) -> Option<&StatusItem> {
        self.items.iter().find(|i| i.key == key)
    }
}

/// What the following thread is told.
enum Input {
    Register { service: String, path: String },
    Signal(Message),
}

/// An item followed, and the unique name of its connection.
struct Tracked {
    owner: String,
    item: StatusItem,
}

#[derive(Default)]
struct Items {
    map: BTreeMap<String, Tracked>,
    /// Counts registrations, for their order.
    seq: u64,
}

impl Items {
    fn build(&self) -> StatusNotifierState {
        let mut items: Vec<StatusItem> = self.map.values().map(|t| t.item.clone()).collect();
        items.sort_by_key(|i| i.order);
        StatusNotifierState { items }
    }
}

/// The served watcher. Registrations go to the following thread, which
/// reads the item; the call returns at once.
struct Watcher {
    inbox: mpsc::Sender<Input>,
    items: Arc<Mutex<Items>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(
        &self,
        service: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<()> {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let (service, path) = registration(&sender, &service)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("not an item: {service:?}")))?;
        self.inbox
            .send(Input::Register { service, path })
            .map_err(|_| fdo::Error::Failed("the status notifier watcher is stopping".into()))
    }

    /// The shell is the host; others may say they are, and are welcome to
    /// the items' list and signals.
    fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        lock(&self.items).map.keys().cloned().collect()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(emitter: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_unregistered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Apps' status items; dropping it stops serving the watcher.
pub struct StatusNotifier {
    bus: Option<Bus>,
    model: Arc<Model<StatusNotifierState>>,
    items: Arc<Mutex<Items>>,
    stop: Stop,
    /// The interface is ours to remove, the name ours to release.
    serving: bool,
    named: bool,
}

service_handle!(StatusNotifier, StatusNotifierState);

impl StatusNotifier {
    /// Serve the watcher on the session bus; without one, or when the
    /// watcher's name can't be had, there are no items.
    pub fn new(wake: Wake) -> StatusNotifier {
        let started = Bus::session().and_then(|bus| StatusNotifier::start(&bus, wake.clone()));
        started.unwrap_or_else(|error| {
            log::info!("aizome-sys: no status items: {error}");
            StatusNotifier::none(wake)
        })
    }

    fn none(wake: Wake) -> StatusNotifier {
        StatusNotifier {
            bus: None,
            model: Arc::new(Model::new(StatusNotifierState::default(), wake)),
            items: Arc::default(),
            stop: Stop::default(),
            serving: false,
            named: false,
        }
    }

    /// Serve `org.kde.StatusNotifierWatcher` on `bus` and take its name,
    /// from a watcher that allows it (a previous shell's), allowing the
    /// next to take it in turn. Fails with [`Error::Unavailable`] when
    /// another watcher keeps it.
    pub fn start(bus: &Bus, wake: Wake) -> Result<StatusNotifier> {
        let (inbox_tx, inbox) = mpsc::channel::<Input>();
        let mut sn = StatusNotifier::none(wake);
        sn.bus = Some(bus.clone());
        let conn = bus.connection().clone();

        // Follow first, so no item's signals or departure are missed.
        follow_items(bus.clone(), sn.items.clone(), sn.model.clone(), sn.stop.clone(), inbox_tx.clone(), inbox)?;

        let watcher = Watcher { inbox: inbox_tx, items: sn.items.clone() };
        sn.serving = zbus::block_on(conn.object_server().at(WATCHER_PATH, watcher))?;
        if !sn.serving {
            return Err(Error::Unavailable("this connection already serves a status notifier watcher".into()));
        }
        bus::wait_until_serving(bus, WATCHER_PATH)?;
        let flags =
            RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement | RequestNameFlags::ReplaceExisting;
        match zbus::block_on(conn.request_name_with_flags(WATCHER_NAME, flags)) {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                sn.named = true;
                Ok(sn)
            }
            Ok(RequestNameReply::InQueue | RequestNameReply::Exists) | Err(zbus::Error::NameTaken) => {
                Err(Error::Unavailable(format!("another status notifier watcher has {WATCHER_NAME}")))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Whether apps can register items with it.
    pub fn available(&self) -> bool {
        self.named
    }

    /// Run `work` against the item listed as `key` on a thread; an item
    /// that isn't registered fails at once with [`Error::NotFound`].
    fn item_task<T: Send + 'static>(
        &self,
        key: &str,
        work: impl FnOnce(&Bus, &StatusItem) -> Result<T> + Send + 'static,
    ) -> Task<T> {
        let Some(item) = self.snapshot().item(key).cloned() else {
            return Task::ready(Err(Error::NotFound(format!("no status item {key}"))));
        };
        bus::spawn(&self.bus, "status items", self.model.wake(), move |bus| work(bus, &item))
    }

    fn click(&self, key: &str, method: &'static str, x: i32, y: i32) -> Task<()> {
        self.item_task(key, move |bus, item| {
            bus.call_with(ACTION, &item.service, &item.path, ITEM_IFACE, method, &(x, y))
        })
    }

    /// The primary click, at `x`, `y` on the screen (where a window it
    /// opens might go). Items that are only a menu refuse it.
    pub fn activate(&self, key: &str, x: i32, y: i32) -> Task<()> {
        self.click(key, "Activate", x, y)
    }

    /// The middle click.
    pub fn secondary_activate(&self, key: &str, x: i32, y: i32) -> Task<()> {
        self.click(key, "SecondaryActivate", x, y)
    }

    /// Ask the item to show its own menu (for items without a dbusmenu).
    pub fn context_menu(&self, key: &str, x: i32, y: i32) -> Task<()> {
        self.click(key, "ContextMenu", x, y)
    }

    /// The wheel over the item: `delta` steps, `vertical` or not.
    pub fn scroll(&self, key: &str, delta: i32, vertical: bool) -> Task<()> {
        let orientation = if vertical { "vertical" } else { "horizontal" };
        self.item_task(key, move |bus, item| {
            bus.call_with(ACTION, &item.service, &item.path, ITEM_IFACE, "Scroll", &(delta, orientation))
        })
    }

    /// The item's menu, read whole (after telling it the menu is about to
    /// show, so it can fill it). Fails with [`Error::NotFound`] when it has
    /// none.
    pub fn menu(&self, key: &str) -> Task<MenuItem> {
        self.item_task(key, |bus, item| {
            let path = item.menu.clone().ok_or_else(|| Error::NotFound(format!("{} has no menu", item.label())))?;
            let _ = bus.call::<_, bool>(&item.service, &path, MENU_IFACE, "AboutToShow", &(0i32,));
            let (_revision, (id, props, children)): (u32, (i32, Props, Vec<OwnedValue>)) =
                bus.call(&item.service, &path, MENU_IFACE, "GetLayout", &(0i32, -1i32, Vec::<String>::new()))?;
            Ok(menu::from_layout(id, &props, &children))
        })
    }

    /// The row `id` of the item's menu was picked.
    pub fn menu_clicked(&self, key: &str, id: i32) -> Task<()> {
        self.item_task(key, move |bus, item| {
            let path = item.menu.clone().ok_or_else(|| Error::NotFound(format!("{} has no menu", item.label())))?;
            bus.call_with(ACTION, &item.service, &path, MENU_IFACE, "Event", &(id, "clicked", Value::from(0i32), 0u32))
        })
    }
}

impl Drop for StatusNotifier {
    fn drop(&mut self) {
        self.stop.stop();
        let Some(bus) = &self.bus else { return };
        let conn = bus.connection().clone();
        if self.named {
            let _ = zbus::block_on(conn.release_name(WATCHER_NAME));
        }
        if self.serving {
            let _ = zbus::block_on(conn.object_server().remove::<Watcher, _>(WATCHER_PATH));
        }
    }
}

type Stream = Pin<Box<dyn futures_lite::Stream<Item = zbus::Result<Message>> + Send>>;

async fn subscribe(conn: &zbus::Connection, rule: &str) -> Result<zbus::MessageStream> {
    let rule = zbus::MatchRule::try_from(rule)?;
    Ok(zbus::MessageStream::for_match_rule(rule, conn, Some(1024)).await?)
}

async fn subscribe_all(conn: &zbus::Connection) -> Result<Stream> {
    let owners = subscribe(conn, OWNER_RULE).await?;
    let items = subscribe(conn, ITEM_RULE).await?;
    Ok(Box::pin(futures_lite::stream::or(owners, items)))
}

/// Start the two threads; returns once the signals are subscribed, so
/// none sent after is missed.
fn follow_items(
    bus: Bus,
    items: Arc<Mutex<Items>>,
    model: Arc<Model<StatusNotifierState>>,
    stop: Stop,
    messages: mpsc::Sender<Input>,
    inbox: mpsc::Receiver<Input>,
) -> Result<()> {
    let (ready, subscribed) = mpsc::channel::<Result<()>>();
    let (drain_bus, drain_stop) = (bus.clone(), stop.clone());
    std::thread::Builder::new()
        .name("aizome-sys status item signals".into())
        .spawn(move || {
            zbus::block_on(async move {
                let mut stream = match subscribe_all(drain_bus.connection()).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let _ = ready.send(Ok(()));
                while let Some(msg) = stream.next().await {
                    let Ok(msg) = msg else { continue };
                    if drain_stop.stopped() || messages.send(Input::Signal(msg)).is_err() {
                        return;
                    }
                }
            })
        })
        .map_err(|error| Error::Failed(format!("could not start a thread: {error}")))?;
    subscribed.recv().map_err(|_| Error::Failed("the status items' signals stopped".into()))??;
    std::thread::Builder::new()
        .name("aizome-sys status items".into())
        .spawn(move || {
            follow(&bus, &items, &model, &stop, &inbox);
            if !stop.stopped() {
                let mut items = lock(&items);
                items.map.clear();
                model.set_if_changed(items.build());
            }
        })
        .map_err(|error| Error::Failed(format!("could not start a thread: {error}")))?;
    Ok(())
}

/// Apply registrations and signals as they come, until `stop` is set or
/// the inbox closes.
fn follow(
    bus: &Bus,
    items: &Mutex<Items>,
    model: &Model<StatusNotifierState>,
    stop: &Stop,
    inbox: &mpsc::Receiver<Input>,
) {
    loop {
        let input = match inbox.recv_timeout(Duration::from_millis(250)) {
            Ok(input) => input,
            Err(RecvTimeoutError::Timeout) if stop.stopped() => return,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if stop.stopped() {
            return;
        }
        // Take whatever else is already queued, reading each item once.
        let mut stale = BTreeSet::new();
        dispatch(bus, items, input, &mut stale);
        while let Ok(input) = inbox.try_recv() {
            dispatch(bus, items, input, &mut stale);
        }
        for key in stale {
            reread(bus, items, &key);
        }
        let items = lock(items);
        model.set_if_changed(items.build());
    }
}

fn dispatch(bus: &Bus, items: &Mutex<Items>, input: Input, stale: &mut BTreeSet<String>) {
    match input {
        Input::Register { service, path } => register(bus, items, &service, &path),
        Input::Signal(msg) => {
            let sender = msg.header().sender().map(|s| s.to_string()).unwrap_or_default();
            let (iface, member) = bus::member(&msg);
            if iface == DBUS && member == "NameOwnerChanged" {
                if sender != DBUS {
                    return;
                }
                let Ok((name, old, _new)) = msg.body().deserialize::<(String, String, String)>() else { return };
                if !old.is_empty() {
                    gone(bus, items, &name, &old);
                }
            } else if iface == ITEM_IFACE {
                let path = bus::path(&msg);
                let items = lock(items);
                stale.extend(
                    items.map.iter().filter(|(_, t)| t.owner == sender && t.item.path == path).map(|(k, _)| k.clone()),
                );
            }
        }
    }
}

/// An app registered an item: read it.
fn register(bus: &Bus, items: &Mutex<Items>, service: &str, path: &str) {
    let owner = if service.starts_with(':') { Some(service.to_owned()) } else { bus.owner(service) };
    let Some(owner) = owner else {
        log::info!("aizome-sys: status item {service} isn't on the bus");
        return;
    };
    let key = format!("{service}{path}");
    let props = match bus.get_all(&owner, path, ITEM_IFACE) {
        Ok(props) => props,
        Err(error) => {
            log::info!("aizome-sys: reading status item {key}: {error}");
            return;
        }
    };
    let new = {
        let mut items = lock(items);
        let order = match items.map.get(&key) {
            Some(t) => t.item.order,
            None => {
                items.seq += 1;
                items.seq
            }
        };
        let item = StatusItem::from_props(&key, service, path, order, &props);
        items.map.insert(key.clone(), Tracked { owner, item }).is_none()
    };
    if new {
        emit(bus, |e| async move { Watcher::status_notifier_item_registered(&e, &key).await });
    }
}

/// `name` left `old`: the items it served are gone.
fn gone(bus: &Bus, items: &Mutex<Items>, name: &str, old: &str) {
    let removed: Vec<String> = {
        let mut items = lock(items);
        let keys: Vec<String> = items
            .map
            .iter()
            .filter(|(_, t)| t.item.service == name || (name == old && t.owner == old))
            .map(|(k, _)| k.clone())
            .collect();
        for k in &keys {
            items.map.remove(k);
        }
        keys
    };
    for key in removed {
        emit(bus, |e| async move { Watcher::status_notifier_item_unregistered(&e, &key).await });
    }
}

/// An item said something changed: read it again (keeping what was read
/// before if it doesn't answer).
fn reread(bus: &Bus, items: &Mutex<Items>, key: &str) {
    let Some((owner, service, path)) =
        lock(items).map.get(key).map(|t| (t.owner.clone(), t.item.service.clone(), t.item.path.clone()))
    else {
        return;
    };
    let Ok(props) = bus.get_all(&owner, &path, ITEM_IFACE) else { return };
    let mut items = lock(items);
    if let Some(t) = items.map.get_mut(key).filter(|t| t.owner == owner) {
        t.item = StatusItem::from_props(key, &service, &path, t.item.order, &props);
    }
}

/// Send one of the watcher's signals.
fn emit<F: std::future::Future<Output = zbus::Result<()>>>(
    bus: &Bus,
    signal: impl FnOnce(SignalEmitter<'static>) -> F,
) {
    let sent = zbus::block_on(async {
        let emitter = SignalEmitter::new(bus.connection(), WATCHER_PATH)?.into_owned();
        signal(emitter).await
    });
    if let Err(error) = sent {
        log::debug!("aizome-sys: sending a status notifier watcher signal: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_go_between_threads() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<StatusNotifier>();
        send_sync::<StatusNotifierState>();
    }

    #[test]
    fn without_a_bus_there_is_nothing() {
        let sn = StatusNotifier::none(Wake::none());
        assert!(!sn.available());
        assert!(sn.snapshot().items.is_empty());
        assert!(matches!(sn.activate("x", 0, 0).wait(), Err(Error::NotFound(_))));
    }
}
