//! D-Bus plumbing shared by the services: connections, calls with timeouts,
//! property access that never panics on odd data, an `ObjectManager` cache,
//! and the thread that follows one daemon's signals.
//!
//! Every call races the reply against a timer: 2 seconds for queries
//! ([`TIMEOUT`]), two minutes for actions that may wait on a polkit dialog, or
//! as long as the hardware takes for long operations (formatting a disk).
//! The services use only these calls, so none can hang on a stuck daemon.
//! For actions they set D-Bus's `ALLOW_INTERACTIVE_AUTHORIZATION` flag,
//! which systemd's daemons (timedated, localed, hostnamed, logind) require
//! before they ask polkit to prompt.

use crate::error::{Error, Result};
use crate::service::{Stop, lock};
use futures_lite::StreamExt;
use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;
use zbus::message::Message;
use zbus::proxy::MethodFlags;
use zbus::zvariant::{self, OwnedObjectPath, OwnedValue, Value};

/// How long a query may take.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// A connection to a message bus: the system bus, the session bus, or one
/// at an address (tests serve fake daemons on a private bus).
#[derive(Clone, Debug)]
pub struct Bus {
    conn: zbus::Connection,
}

static SYSTEM: Mutex<Option<zbus::Connection>> = Mutex::new(None);
static SESSION: Mutex<Option<zbus::Connection>> = Mutex::new(None);

impl Bus {
    /// The system bus, connected once per process and shared.
    pub fn system() -> Result<Bus> {
        Bus::shared(&SYSTEM, zbus::connection::Builder::system)
    }

    /// The session bus, connected once per process and shared.
    pub fn session() -> Result<Bus> {
        Bus::shared(&SESSION, zbus::connection::Builder::session)
    }

    /// A new connection to the bus at `address` (`unix:path=…`).
    pub fn at(address: &str) -> Result<Bus> {
        let builder = zbus::connection::Builder::address(address)?;
        Ok(Bus { conn: connect(builder)? })
    }

    /// A bus from a zbus connection made elsewhere.
    pub fn from_connection(conn: zbus::Connection) -> Bus {
        Bus { conn }
    }

    fn shared(
        cache: &Mutex<Option<zbus::Connection>>,
        builder: impl FnOnce() -> zbus::Result<zbus::connection::Builder<'static>>,
    ) -> Result<Bus> {
        let mut cached = lock(cache);
        if let Some(conn) = cached.as_ref() {
            return Ok(Bus { conn: conn.clone() });
        }
        let conn = connect(builder().map_err(|e| Error::Unavailable(e.to_string()))?)?;
        *cached = Some(conn.clone());
        Ok(Bus { conn })
    }

    /// The zbus connection, for serving objects.
    pub fn connection(&self) -> &zbus::Connection {
        &self.conn
    }

    /// This connection's unique name on the bus.
    pub fn unique_name(&self) -> Option<String> {
        self.conn.unique_name().map(|name| name.to_string())
    }

    /// Call `method` with the query options: 2 seconds, no polkit prompt.
    pub(crate) fn call<B, R>(&self, dest: &str, path: &str, iface: &str, method: &str, body: &B) -> Result<R>
    where
        B: serde::Serialize + zvariant::DynamicType,
        R: for<'d> zvariant::DynamicDeserialize<'d>,
    {
        self.call_with(Opts::QUERY, dest, path, iface, method, body)
    }

    /// Call `method` with the given timeout and authorization flag.
    pub(crate) fn call_with<B, R>(
        &self,
        opts: Opts,
        dest: &str,
        path: &str,
        iface: &str,
        method: &str,
        body: &B,
    ) -> Result<R>
    where
        B: serde::Serialize + zvariant::DynamicType,
        R: for<'d> zvariant::DynamicDeserialize<'d>,
    {
        zbus::block_on(self.call_async(opts, dest, path, iface, method, body))
    }

    /// [`call_with`](Bus::call_with) for async contexts (served interfaces
    /// run on zbus's executor and must not block).
    pub(crate) async fn call_async<B, R>(
        &self,
        opts: Opts,
        dest: &str,
        path: &str,
        iface: &str,
        method: &str,
        body: &B,
    ) -> Result<R>
    where
        B: serde::Serialize + zvariant::DynamicType,
        R: for<'d> zvariant::DynamicDeserialize<'d>,
    {
        let call = async {
            let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(&self.conn)
                .destination(dest.to_owned())?
                .path(path.to_owned())?
                .interface(iface.to_owned())?
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await?;
            let flags = if opts.interactive { MethodFlags::AllowInteractiveAuth.into() } else { Default::default() };
            proxy.call_with_flags::<_, B, R>(method, flags, body).await
        };
        match with_timeout(opts.timeout, call).await {
            None => Err(Error::Timeout),
            Some(Err(error)) => Err(error.into()),
            Some(Ok(Some(reply))) => Ok(reply),
            Some(Ok(None)) => Err(Error::Failed(format!("{method}: no reply"))),
        }
    }

    /// A property, unwrapped from its variant.
    pub(crate) fn get<R>(&self, dest: &str, path: &str, iface: &str, name: &str) -> Result<R>
    where
        R: TryFrom<OwnedValue>,
        R::Error: Into<zvariant::Error>,
    {
        let value: OwnedValue = self.call(dest, path, "org.freedesktop.DBus.Properties", "Get", &(iface, name))?;
        R::try_from(value).map_err(|e| Error::from(e.into()))
    }

    /// All of an interface's properties.
    pub(crate) fn get_all(&self, dest: &str, path: &str, iface: &str) -> Result<Props> {
        self.call(dest, path, "org.freedesktop.DBus.Properties", "GetAll", &(iface,))
    }

    /// Set a property, allowing a polkit prompt.
    pub(crate) fn set<'v>(
        &self,
        dest: &str,
        path: &str,
        iface: &str,
        name: &str,
        value: impl Into<Value<'v>>,
    ) -> Result<()> {
        let value: Value<'v> = value.into();
        self.call_with(Opts::ACTION, dest, path, "org.freedesktop.DBus.Properties", "Set", &(iface, name, value))
    }

    /// The unique name owning `name`, if anyone does.
    pub(crate) fn owner(&self, name: &str) -> Option<String> {
        self.call::<_, String>(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetNameOwner",
            &(name,),
        )
        .ok()
    }

    /// Whether the bus can start `name` on demand (D-Bus activation).
    pub(crate) fn activatable(&self, name: &str) -> bool {
        self.call::<_, Vec<String>>(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ListActivatableNames",
            &(),
        )
        .is_ok_and(|names| names.iter().any(|n| n == name))
    }

    /// Start an activatable service, waiting up to `timeout`.
    pub(crate) fn start(&self, name: &str, timeout: Duration) -> Result<()> {
        self.call_with::<_, u32>(
            Opts { timeout, interactive: false },
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "StartServiceByName",
            &(name, 0u32),
        )
        .map(drop)
    }
}

fn connect(builder: zbus::connection::Builder<'static>) -> Result<zbus::Connection> {
    zbus::block_on(with_timeout(Duration::from_secs(5), builder.build()))
        .ok_or(Error::Timeout)?
        .map_err(|e| Error::Unavailable(e.to_string()))
}

/// `future`'s output, or `None` once `timeout` passes.
pub(crate) async fn with_timeout<T>(timeout: Duration, future: impl std::future::Future<Output = T>) -> Option<T> {
    futures_lite::future::or(async { Some(future.await) }, async {
        async_io::Timer::after(timeout).await;
        None
    })
    .await
}

/// How a call may wait.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Opts {
    pub timeout: Duration,
    /// Set `ALLOW_INTERACTIVE_AUTHORIZATION`, so the daemon may ask polkit
    /// to prompt.
    pub interactive: bool,
}

impl Opts {
    /// Reading state: 2 seconds.
    pub const QUERY: Opts = Opts { timeout: TIMEOUT, interactive: false };
    /// A change that may need a polkit prompt answered: 2 minutes.
    pub const ACTION: Opts = Opts { timeout: Duration::from_secs(120), interactive: true };

    /// A change that takes as long as the hardware does.
    pub const fn long(timeout: Duration) -> Opts {
        Opts { timeout, interactive: true }
    }
}

/// An interface's properties.
pub type Props = HashMap<String, OwnedValue>;

/// Reads properties leniently: a missing property or one of an unexpected
/// type reads as empty, zero or false, never a panic. Integers convert
/// between widths where the value fits.
pub(crate) trait PropsExt {
    fn value(&self, key: &str) -> Option<&Value<'static>>;

    fn str(&self, key: &str) -> String {
        self.opt_str(key).unwrap_or_default()
    }

    fn opt_str(&self, key: &str) -> Option<String> {
        self.value(key).and_then(value_str)
    }

    fn bool(&self, key: &str) -> bool {
        self.value(key).and_then(value_bool).unwrap_or(false)
    }

    fn opt_bool(&self, key: &str) -> Option<bool> {
        self.value(key).and_then(value_bool)
    }

    fn i64(&self, key: &str) -> i64 {
        self.opt_i64(key).unwrap_or(0)
    }

    fn opt_i64(&self, key: &str) -> Option<i64> {
        self.value(key).and_then(value_i64)
    }

    fn u64(&self, key: &str) -> u64 {
        self.value(key).and_then(value_u64).unwrap_or(0)
    }

    fn u32(&self, key: &str) -> u32 {
        u32::try_from(self.u64(key)).unwrap_or(u32::MAX)
    }

    fn f64(&self, key: &str) -> f64 {
        self.value(key).and_then(value_f64).unwrap_or(0.0)
    }

    /// An object path; `/` (D-Bus's "none") reads as `None`.
    fn path(&self, key: &str) -> Option<String> {
        self.opt_str(key).filter(|path| path != "/" && !path.is_empty())
    }

    /// An array of strings or object paths.
    fn strings(&self, key: &str) -> Vec<String> {
        self.value(key).map(value_strings).unwrap_or_default()
    }

    /// An array of bytes (`ay`).
    fn bytes(&self, key: &str) -> Vec<u8> {
        self.value(key).map(value_bytes).unwrap_or_default()
    }

    /// A byte string (`ay`), as UDisks and NetworkManager send file names
    /// and SSIDs: lossy UTF-8, without the trailing NUL.
    fn bytestring(&self, key: &str) -> String {
        bytes_to_string(&self.bytes(key))
    }

    /// An array of byte strings (`aay`).
    fn bytestrings(&self, key: &str) -> Vec<String> {
        match self.value(key) {
            Some(Value::Array(array)) => {
                array.iter().map(|item| bytes_to_string(&value_bytes(item))).filter(|s| !s.is_empty()).collect()
            }
            _ => Vec::new(),
        }
    }
}

impl PropsExt for Props {
    fn value(&self, key: &str) -> Option<&Value<'static>> {
        self.get(key).map(|value| unwrap_variant(value))
    }
}

impl PropsExt for HashMap<String, Value<'static>> {
    fn value(&self, key: &str) -> Option<&Value<'static>> {
        self.get(key).map(unwrap_variant)
    }
}

impl PropsExt for HashMap<String, &Value<'static>> {
    fn value(&self, key: &str) -> Option<&Value<'static>> {
        self.get(key).map(|value| unwrap_variant(value))
    }
}

/// The entries of an `a{sv}` (or `a{ss}`) value; anything else reads as
/// empty.
pub(crate) fn value_dict<'a>(value: &'a Value<'static>) -> HashMap<String, &'a Value<'static>> {
    match unwrap_variant(value) {
        Value::Dict(dict) => {
            dict.iter().filter_map(|(key, value)| Some((value_str(key)?, unwrap_variant(value)))).collect()
        }
        _ => HashMap::new(),
    }
}

/// The dictionaries of an `aa{sv}` value.
pub(crate) fn value_dicts<'a>(value: &'a Value<'static>) -> Vec<HashMap<String, &'a Value<'static>>> {
    match unwrap_variant(value) {
        Value::Array(array) => array.iter().map(value_dict).collect(),
        _ => Vec::new(),
    }
}

/// The value inside any number of variant wrappers.
pub(crate) fn unwrap_variant<'a, 'v>(value: &'a Value<'v>) -> &'a Value<'v> {
    let mut value = value;
    while let Value::Value(inner) = value {
        value = inner;
    }
    value
}

pub(crate) fn value_str(value: &Value<'_>) -> Option<String> {
    match unwrap_variant(value) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        Value::Signature(s) => Some(s.to_string()),
        _ => None,
    }
}

pub(crate) fn value_bool(value: &Value<'_>) -> Option<bool> {
    match unwrap_variant(value) {
        Value::Bool(b) => Some(*b),
        other => value_i64(other).map(|n| n != 0),
    }
}

pub(crate) fn value_i64(value: &Value<'_>) -> Option<i64> {
    match unwrap_variant(value) {
        Value::U8(n) => Some(i64::from(*n)),
        Value::I16(n) => Some(i64::from(*n)),
        Value::U16(n) => Some(i64::from(*n)),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        Value::F64(f) if f.is_finite() => Some(*f as i64),
        _ => None,
    }
}

pub(crate) fn value_u64(value: &Value<'_>) -> Option<u64> {
    match unwrap_variant(value) {
        Value::U64(n) => Some(*n),
        other => value_i64(other).and_then(|n| u64::try_from(n).ok()),
    }
}

pub(crate) fn value_f64(value: &Value<'_>) -> Option<f64> {
    match unwrap_variant(value) {
        Value::F64(f) => Some(*f),
        Value::U64(n) => Some(*n as f64),
        other => value_i64(other).map(|n| n as f64),
    }
}

pub(crate) fn value_strings(value: &Value<'_>) -> Vec<String> {
    match unwrap_variant(value) {
        Value::Array(array) => array.iter().filter_map(value_str).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn value_bytes(value: &Value<'_>) -> Vec<u8> {
    match unwrap_variant(value) {
        Value::Array(array) => array
            .iter()
            .filter_map(|item| match item {
                Value::U8(b) => Some(*b),
                _ => None,
            })
            .collect(),
        Value::Str(s) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

pub(crate) fn bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// A value for a call's `a{sv}` options.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn owned<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
    let value: Value<'a> = value.into();
    // Only fd-carrying values fail to convert, and none are built here.
    OwnedValue::try_from(value).unwrap_or_else(|_| OwnedValue::from(false))
}

/// The objects of a daemon that implements `org.freedesktop.DBus.ObjectManager`
/// (NetworkManager, BlueZ, UDisks2), kept current from its signals.
#[derive(Default, Debug)]
pub(crate) struct Objects {
    pub map: BTreeMap<String, HashMap<String, Props>>,
}

/// What [`Objects::apply`] did with a signal.
#[derive(Default, Debug)]
pub(crate) struct Applied {
    pub changed: bool,
    /// Properties the daemon invalidated without sending values:
    /// `(path, interface, property)` to fetch.
    pub invalidated: Vec<(String, String, String)>,
}

type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

impl Objects {
    /// Every object under `root`, from `GetManagedObjects`.
    pub(crate) fn load(bus: &Bus, dest: &str, root: &str) -> Result<Objects> {
        let all: ManagedObjects =
            bus.call(dest, root, "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", &())?;
        Ok(Objects { map: all.into_iter().map(|(path, ifaces)| (path.to_string(), ifaces)).collect() })
    }

    pub(crate) fn get(&self, path: &str, iface: &str) -> Option<&Props> {
        self.map.get(path)?.get(iface)
    }

    /// Objects with `iface`, by path.
    pub(crate) fn with<'a>(&'a self, iface: &'a str) -> impl Iterator<Item = (&'a str, &'a Props)> {
        self.map.iter().filter_map(move |(path, ifaces)| Some((path.as_str(), ifaces.get(iface)?)))
    }

    /// Apply `InterfacesAdded`, `InterfacesRemoved` and `PropertiesChanged`.
    pub(crate) fn apply(&mut self, msg: &Message) -> Applied {
        let mut applied = Applied::default();
        let header = msg.header();
        let (Some(iface), Some(member)) = (header.interface(), header.member()) else {
            return applied;
        };
        match (iface.as_str(), member.as_str()) {
            ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded") => {
                if let Ok((path, ifaces)) = msg.body().deserialize::<(OwnedObjectPath, HashMap<String, Props>)>() {
                    let entry = self.map.entry(path.to_string()).or_default();
                    for (iface, props) in ifaces {
                        entry.insert(iface, props);
                    }
                    applied.changed = true;
                }
            }
            ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved") => {
                if let Ok((path, ifaces)) = msg.body().deserialize::<(OwnedObjectPath, Vec<String>)>() {
                    if let Some(entry) = self.map.get_mut(path.as_str()) {
                        for iface in ifaces {
                            entry.remove(&iface);
                        }
                        if entry.is_empty() {
                            self.map.remove(path.as_str());
                        }
                    }
                    applied.changed = true;
                }
            }
            ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                let Some(path) = header.path() else { return applied };
                if let Ok((iface, changed, invalidated)) = msg.body().deserialize::<(String, Props, Vec<String>)>() {
                    // Only objects we know: a property change can race ahead
                    // of InterfacesAdded, which then brings everything.
                    if let Some(props) = self.map.get_mut(path.as_str()).and_then(|o| o.get_mut(&iface)) {
                        for (name, value) in changed {
                            props.insert(name, value);
                        }
                        for name in invalidated {
                            props.remove(&name);
                            applied.invalidated.push((path.to_string(), iface.clone(), name));
                        }
                        applied.changed = true;
                    }
                }
            }
            _ => {}
        }
        applied
    }

    /// Fetch properties a signal invalidated.
    pub(crate) fn refetch(&mut self, bus: &Bus, dest: &str, invalidated: Vec<(String, String, String)>) {
        for (path, iface, name) in invalidated {
            if let Ok(value) = bus.get::<OwnedValue>(dest, &path, &iface, &name)
                && let Some(props) = self.map.get_mut(&path).and_then(|o| o.get_mut(&iface))
            {
                props.insert(name, value);
            }
        }
    }
}

/// What a [`follow`] handler hears.
pub(crate) enum Event {
    /// The daemon is on the bus (at start, or after it restarted). Load
    /// everything.
    Appeared,
    /// The daemon left the bus.
    Vanished,
    /// A signal from the daemon.
    Signal(Message),
    /// No more signals are queued right now: publish the model.
    Idle,
}

/// Follow the daemon that owns `name`: subscribe to `rules` (signals; the
/// sender is checked against `name`'s current owner, since a shared
/// connection hears other daemons' signals too) and to `name`'s owner
/// changes, then call `handle` with each [`Event`] on a thread of its own.
/// Bursts of signals end with one [`Event::Idle`]. It ends soon after
/// `stop` is set, or when the connection closes.
///
/// Two threads do this. One only moves messages from zbus's queues into an
/// unbounded channel, so zbus's socket reader never waits on a full queue
/// (which would hold up every reply on the shared connection, including
/// the ones `handle` itself waits for). The other runs `handle`, which may
/// make blocking calls.
///
/// When `activate` is set and the daemon is not running but activatable,
/// it is started first.
pub(crate) fn follow(
    bus: Bus,
    name: &'static str,
    rules: Vec<String>,
    activate: bool,
    stop: Stop,
    mut handle: impl FnMut(&Bus, Event) + Send + 'static,
) {
    let (messages, inbox) = std::sync::mpsc::channel::<Message>();
    let (ready, subscribed) = std::sync::mpsc::channel::<Result<()>>();
    let drain_bus = bus.clone();
    let drain_stop = stop.clone();
    let drained = std::thread::Builder::new().name(format!("aizome-sys {name} signals")).spawn(move || {
        zbus::block_on(async move {
            let mut stream = match subscribe_all(drain_bus.connection(), name, &rules).await {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let _ = ready.send(Ok(()));
            while let Some(msg) = stream.next().await {
                let Ok(msg) = msg else { continue };
                if drain_stop.stopped() || messages.send(msg).is_err() {
                    return;
                }
            }
        })
    });
    if let Err(error) = drained {
        log::warn!("aizome-sys: could not start a thread for {name}: {error}");
        return;
    }
    let thread = std::thread::Builder::new().name(format!("aizome-sys {name}")).spawn(move || {
        match subscribed.recv() {
            Ok(Ok(())) => follow_loop(&bus, name, activate, &stop, &inbox, &mut handle),
            Ok(Err(error)) => log::warn!("aizome-sys: can't follow {name}: {error}"),
            Err(_) => {}
        }
        handle(&bus, Event::Vanished);
        handle(&bus, Event::Idle);
    });
    if let Err(error) = thread {
        log::warn!("aizome-sys: could not start a thread for {name}: {error}");
    }
}

type BoxedStream = Pin<Box<dyn futures_lite::Stream<Item = zbus::Result<Message>> + Send>>;

/// One stream of `name`'s owner changes and every rule's signals.
async fn subscribe_all(conn: &zbus::Connection, name: &str, rules: &[String]) -> Result<BoxedStream> {
    let owner_rule = format!(
        "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',\
         member='NameOwnerChanged',arg0='{name}'"
    );
    let mut stream: BoxedStream = Box::pin(subscribe(conn, &owner_rule).await?);
    for rule in rules {
        let more = subscribe(conn, rule).await?;
        stream = Box::pin(futures_lite::stream::or(stream, more));
    }
    Ok(stream)
}

fn follow_loop(
    bus: &Bus,
    name: &'static str,
    activate: bool,
    stop: &Stop,
    inbox: &std::sync::mpsc::Receiver<Message>,
    handle: &mut impl FnMut(&Bus, Event),
) {
    use std::sync::mpsc::RecvTimeoutError;
    // Subscribed: now see who owns the name, so no change is missed.
    let mut owner = bus.owner(name);
    if owner.is_none() && activate && bus.activatable(name) {
        if let Err(error) = bus.start(name, Duration::from_secs(10)) {
            log::info!("aizome-sys: could not start {name}: {error}");
        }
        owner = bus.owner(name);
    }
    if owner.is_some() {
        handle(bus, Event::Appeared);
    } else {
        handle(bus, Event::Vanished);
    }
    handle(bus, Event::Idle);

    loop {
        let msg = match inbox.recv_timeout(Duration::from_millis(250)) {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) if stop.stopped() => return,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if stop.stopped() {
            return;
        }
        dispatch(bus, name, &mut owner, msg, handle);
        // Take whatever else is already queued before publishing.
        while let Ok(msg) = inbox.try_recv() {
            dispatch(bus, name, &mut owner, msg, handle);
        }
        handle(bus, Event::Idle);
    }
}

async fn subscribe(conn: &zbus::Connection, rule: &str) -> Result<zbus::MessageStream> {
    let rule = zbus::MatchRule::try_from(rule)?;
    Ok(zbus::MessageStream::for_match_rule(rule, conn, Some(1024)).await?)
}

fn dispatch(bus: &Bus, name: &str, owner: &mut Option<String>, msg: Message, handle: &mut impl FnMut(&Bus, Event)) {
    let header = msg.header();
    let sender = header.sender().map(|s| s.as_str().to_owned());
    if sender.as_deref() == Some("org.freedesktop.DBus")
        && header.member().is_some_and(|m| m.as_str() == "NameOwnerChanged")
    {
        if let Ok((changed, _old, new)) = msg.body().deserialize::<(String, String, String)>()
            && changed == name
        {
            let had = owner.take();
            *owner = Some(new).filter(|n| !n.is_empty());
            if had.is_some() {
                handle(bus, Event::Vanished);
            }
            if owner.is_some() {
                handle(bus, Event::Appeared);
            }
        }
        return;
    }
    if sender.is_some() && sender == *owner {
        handle(bus, Event::Signal(msg));
    }
}

/// Wait until the connection's object server answers calls to `path`.
///
/// zbus starts a connection's object server lazily, as a task on the
/// connection's executor, and a method call that arrives before that task
/// has subscribed is dropped without a reply (the caller waits out its
/// timeout). Connections built with `serve_at` wait for the task; objects
/// served later on a shared connection (the Bluetooth and polkit agents,
/// the notification server) don't. So ping our own object until it
/// answers before telling anyone to call it.
pub(crate) fn wait_until_serving(bus: &Bus, path: &str) -> Result<()> {
    let Some(me) = bus.unique_name() else { return Ok(()) };
    let ping = Opts { timeout: Duration::from_millis(100), interactive: false };
    let mut tries = 0;
    loop {
        match bus.call_with::<_, ()>(ping, &me, path, "org.freedesktop.DBus.Peer", "Ping", &()) {
            Err(Error::Timeout) if tries < 50 => tries += 1,
            // The system bus's policy forbids a connection calling itself
            // (dbus-broker denies the polkit agent's ping), so there is no
            // way to check there; carry on, as the object is served by now.
            Err(Error::NotAuthorized(_)) => return Ok(()),
            other => return other,
        }
    }
}

/// Run `work` with the bus on a thread, as a [`Task`](crate::Task); without
/// a bus the task fails at once with [`Error::Unavailable`].
pub(crate) fn spawn<T: Send + 'static>(
    bus: &Option<Bus>,
    service: &str,
    wake: crate::Wake,
    work: impl FnOnce(&Bus) -> Result<T> + Send + 'static,
) -> crate::Task<T> {
    match bus {
        None => crate::Task::ready(Err(Error::Unavailable(service.to_owned()))),
        Some(bus) => {
            let bus = bus.clone();
            crate::Task::spawn(service, &wake, move || work(&bus))
        }
    }
}

/// Follow one object's properties on one interface (systemd's timedated,
/// localed, hostnamed; power-profiles-daemon): `changed` gets them after
/// every change, or `None` when the daemon is gone. Daemons that exit when
/// idle and are started on demand (`keep_on_exit`) keep their last
/// properties while away, since they come back on the next call.
pub(crate) fn follow_props(
    bus: Bus,
    name: &'static str,
    path: &'static str,
    iface: &'static str,
    keep_on_exit: bool,
    stop: Stop,
    mut changed: impl FnMut(&Bus, Option<&Props>) + Send + 'static,
) {
    let mut props: Option<Props> = None;
    let mut dirty = false;
    follow(bus, name, vec![signals_from(name)], true, stop, move |bus, event| match event {
        Event::Appeared => {
            props = Some(bus.get_all(name, path, iface).unwrap_or_default());
            dirty = true;
        }
        Event::Vanished => {
            if !keep_on_exit || props.is_none() {
                props = None;
                dirty = true;
            }
        }
        Event::Signal(msg) => {
            if msg.header().path().is_some_and(|p| p.as_str() == path)
                && member(&msg).1 == "PropertiesChanged"
                && let Ok((changed_iface, changed, invalidated)) =
                    msg.body().deserialize::<(String, Props, Vec<String>)>()
                && changed_iface == iface
                && let Some(props) = props.as_mut()
            {
                props.extend(changed);
                for name_ in invalidated {
                    match bus.get::<OwnedValue>(name, path, iface, &name_) {
                        Ok(value) => {
                            props.insert(name_, value);
                        }
                        Err(_) => {
                            props.remove(&name_);
                        }
                    }
                }
                dirty = true;
            }
        }
        Event::Idle => {
            if dirty {
                changed(bus, props.as_ref());
                dirty = false;
            }
        }
    });
}

/// The match rule for every signal `sender` emits.
pub(crate) fn signals_from(sender: &str) -> String {
    format!("type='signal',sender='{sender}'")
}

/// The interface and member of a signal.
pub(crate) fn member(msg: &Message) -> (String, String) {
    let header = msg.header();
    (
        header.interface().map(|i| i.to_string()).unwrap_or_default(),
        header.member().map(|m| m.to_string()).unwrap_or_default(),
    )
}

/// The object path a signal came from.
pub(crate) fn path(msg: &Message) -> String {
    msg.header().path().map(|p| p.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn props_read_leniently() {
        let mut props = Props::new();
        props.insert("U".into(), OwnedValue::from(7u32));
        props.insert("I".into(), OwnedValue::from(-3i32));
        props.insert("S".into(), owned("hello"));
        props.insert("B".into(), OwnedValue::from(true));
        props.insert("Bytes".into(), owned(b"/dev/sda1\0".to_vec()));
        props.insert("V".into(), owned(Value::Value(Box::new(Value::U8(5)))));
        assert_eq!(props.u32("U"), 7);
        assert_eq!(props.i64("I"), -3);
        assert_eq!(props.u64("I"), 0, "negative values don't wrap");
        assert_eq!(props.str("S"), "hello");
        assert_eq!(props.str("U"), "", "wrong types read as empty");
        assert!(props.bool("B"));
        assert!(!props.bool("Missing"));
        assert_eq!(props.bytestring("Bytes"), "/dev/sda1");
        assert_eq!(props.u32("V"), 5, "variants are unwrapped");
    }
}
