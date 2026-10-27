//! The notification server: `org.freedesktop.Notifications` at
//! `/org/freedesktop/Notifications`, per the Desktop Notifications
//! Specification 1.2 (plus its `ActivationToken` signal).
//!
//! [`NotificationServer::start`] serves the interface on a bus connection
//! and takes the bus name, failing with [`Error::Unavailable`] (naming the
//! process) when another server has it, unless
//! [`ServerConfig::replace`] is set and that server allows replacement.
//! Dropping the server closes what's open, releases the name and stops
//! serving.
//!
//! The server keeps the open notifications: it gives each an id (never 0,
//! never one still open), replaces in place when `replaces_id` names an
//! open one (an unknown id gets a new one), and closes each when its time
//! runs out (`expire_timeout`: -1 the configured default, 0 never; by
//! default critical and resident notifications never expire). The shell
//! pauses a notification's timer while the pointer is over it
//! ([`pause_expiry`](NotificationServer::pause_expiry)), or while it isn't
//! shown at all (do-not-disturb, a full screen of banners).
//!
//! The shell owns the presentation. Each change reaches the callback given
//! to `start` as a [`NotificationEvent`], and the open notifications are
//! in the [`snapshot`](NotificationServer::snapshot). The shell answers
//! with [`close`](NotificationServer::close) when the person dismisses one
//! and [`invoke_action`](NotificationServer::invoke_action) when they
//! click it or one of its buttons; neither blocks.
//!
//! Signals (`NotificationClosed`, `ActionInvoked`, `ActivationToken`) are
//! broadcast, as most servers do, and sent in order from one thread, which
//! also runs the timers and calls the callback; the callback must return
//! quickly (post to the UI's loop and return).

use super::{BUS_NAME, CloseReason, Hints, Notification, PATH, Timeout, Urgency};
use crate::bus::Bus;
use crate::error::{Error, Result};
use crate::service::{Model, Wake, lock, service_handle};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};
use zbus::fdo::{self, RequestNameFlags, RequestNameReply};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

/// The version of the specification served.
pub const SPEC_VERSION: &str = "1.2";

/// The capabilities advertised by default: actions, a body with the
/// specification's markup (`b`, `i`, `u`, `a`, `img`; see
/// [`strip_markup`](super::strip_markup) for a shell that shows plain
/// text), one static image, and persistence (a notification list).
pub const DEFAULT_CAPABILITIES: &[&str] = &["actions", "body", "body-markup", "icon-static", "persistence"];

/// How the server presents itself and expires notifications.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerConfig {
    /// What `GetCapabilities` answers: advertise only what the shell does
    /// (`action-icons`, `body-hyperlinks`, `body-images`, `sound` when it
    /// shows or plays them).
    pub capabilities: Vec<String>,
    /// `GetServerInformation`'s product name, vendor and version.
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// How long a notification sent with `expire_timeout` -1 stays; zero
    /// keeps them until closed.
    pub default_timeout: Duration,
    /// Let critical notifications expire (the specification advises
    /// against it).
    pub expire_critical: bool,
    /// Let resident notifications expire.
    pub expire_resident: bool,
    /// Take the bus name from a server that allows it (a previous shell),
    /// and allow the next one to take it in turn.
    pub replace: bool,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            capabilities: DEFAULT_CAPABILITIES.iter().map(|c| (*c).to_owned()).collect(),
            name: "Aizome".into(),
            vendor: "Sekka".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            default_timeout: Duration::from_secs(5),
            expire_critical: false,
            expire_resident: false,
            replace: false,
        }
    }
}

/// What the shell hears from the server.
#[derive(Clone, Debug, PartialEq)]
pub enum NotificationEvent {
    /// A notification to show; `replaced` when it replaces the open one
    /// with the same id in place (update it without fuss).
    Notified { notification: Arc<Notification>, replaced: bool },
    /// A notification closed: hide it.
    Closed { id: u32, reason: CloseReason },
}

/// The open notifications.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationsState {
    /// By id, so oldest first (a replaced one keeps its place).
    pub notifications: Vec<Arc<Notification>>,
}

impl NotificationsState {
    /// The open notification with this id.
    pub fn get(&self, id: u32) -> Option<&Arc<Notification>> {
        self.notifications.iter().find(|n| n.id == id)
    }
}

type Callback = Arc<dyn Fn(NotificationEvent) + Send + Sync>;

/// An open notification and its timer.
struct Entry {
    notification: Arc<Notification>,
    /// How long it stays shown; `None` until it is closed.
    lifetime: Option<Duration>,
    /// When the timer last started, and how long it ran before that.
    since: Instant,
    shown: Duration,
    paused: bool,
}

impl Entry {
    fn deadline(&self) -> Option<Instant> {
        if self.paused {
            return None;
        }
        // Too far to represent is never.
        self.since.checked_add(self.lifetime?.saturating_sub(self.shown))
    }
}

/// Work for the server's thread, in order.
enum Job {
    Notified(Arc<Notification>, bool),
    Closed(u32, CloseReason),
    Action { id: u32, key: String, token: Option<String> },
}

#[derive(Default)]
struct State {
    entries: BTreeMap<u32, Entry>,
    last_id: u32,
    jobs: VecDeque<Job>,
    stopped: bool,
}

impl State {
    /// The next id: counting up, past `u32::MAX` back to 1, skipping open
    /// ones (of `n` open, one of the next `n + 1` is free).
    fn new_id(&mut self) -> u32 {
        for _ in 0..=self.entries.len() {
            self.last_id = self.last_id.checked_add(1).unwrap_or(1);
            if !self.entries.contains_key(&self.last_id) {
                break;
            }
        }
        self.last_id
    }
}

struct Shared {
    config: ServerConfig,
    state: Mutex<State>,
    wakeup: Condvar,
    model: Arc<Model<NotificationsState>>,
}

impl Shared {
    fn lifetime(&self, notification: &Notification) -> Option<Duration> {
        let config = &self.config;
        let hints = &notification.hints;
        if (hints.urgency == Urgency::Critical && !config.expire_critical)
            || (hints.resident && !config.expire_resident)
        {
            return None;
        }
        match notification.timeout {
            Timeout::Never => None,
            Timeout::After(lifetime) => Some(lifetime),
            Timeout::Default => Some(config.default_timeout).filter(|d| !d.is_zero()),
        }
    }

    fn publish(&self, state: &State) {
        let notifications = state.entries.values().map(|e| e.notification.clone()).collect();
        self.model.set(NotificationsState { notifications });
    }

    /// Queue jobs (under the lock, so they stay in order) and wake the
    /// thread.
    fn queue(&self, mut state: std::sync::MutexGuard<'_, State>, jobs: impl IntoIterator<Item = Job>) {
        state.jobs.extend(jobs);
        drop(state);
        self.wakeup.notify_all();
    }

    fn notify(&self, mut notification: Notification) -> Option<u32> {
        let lifetime = self.lifetime(&notification);
        let mut state = lock(&self.state);
        if state.stopped {
            return None;
        }
        let wanted = notification.replaces_id;
        let replaced = wanted != 0 && state.entries.contains_key(&wanted);
        let id = if replaced { wanted } else { state.new_id() };
        notification.id = id;
        let notification = Arc::new(notification);
        // A replacement starts its time afresh, still paused if it was.
        let paused = state.entries.get(&id).is_some_and(|e| e.paused);
        state.entries.insert(
            id,
            Entry {
                notification: notification.clone(),
                lifetime,
                since: Instant::now(),
                shown: Duration::ZERO,
                paused,
            },
        );
        self.publish(&state);
        self.queue(state, [Job::Notified(notification, replaced)]);
        Some(id)
    }

    fn close(&self, id: u32, reason: CloseReason) -> bool {
        let mut state = lock(&self.state);
        if state.entries.remove(&id).is_none() {
            return false;
        }
        self.publish(&state);
        self.queue(state, [Job::Closed(id, reason)]);
        true
    }

    fn invoke(&self, id: u32, key: &str, token: Option<String>) -> bool {
        let mut state = lock(&self.state);
        let Some(entry) = state.entries.get(&id) else { return false };
        if entry.notification.action_for(key).is_none() {
            return false;
        }
        let action = Job::Action { id, key: key.to_owned(), token: token.filter(|t| !t.is_empty()) };
        if entry.notification.hints.resident {
            self.queue(state, [action]);
        } else {
            // Invoking an action is done with the notification, unless it
            // asked to stay.
            state.entries.remove(&id);
            self.publish(&state);
            self.queue(state, [action, Job::Closed(id, CloseReason::Dismissed)]);
        }
        true
    }

    fn set_paused(&self, id: u32, paused: bool) -> bool {
        let mut state = lock(&self.state);
        let Some(entry) = state.entries.get_mut(&id) else { return false };
        let now = Instant::now();
        if paused && !entry.paused {
            entry.shown += now.saturating_duration_since(entry.since);
        } else if !paused && entry.paused {
            entry.since = now;
        }
        entry.paused = paused;
        self.queue(state, []);
        true
    }
}

/// The server's thread: closes notifications whose time ran out, sends the
/// signals and calls the callback, in order. Ends once stopped and done.
fn work(shared: Arc<Shared>, conn: zbus::Connection, callback: Callback) {
    let mut state = lock(&shared.state);
    loop {
        let now = Instant::now();
        let expired: Vec<u32> =
            state.entries.iter().filter(|(_, e)| e.deadline().is_some_and(|d| d <= now)).map(|(&id, _)| id).collect();
        if !expired.is_empty() {
            for id in expired {
                state.entries.remove(&id);
                state.jobs.push_back(Job::Closed(id, CloseReason::Expired));
            }
            shared.publish(&state);
        }
        if !state.jobs.is_empty() {
            let jobs = std::mem::take(&mut state.jobs);
            drop(state);
            for job in jobs {
                run(&conn, &callback, job);
            }
            state = lock(&shared.state);
            continue;
        }
        if state.stopped {
            return;
        }
        let next = state.entries.values().filter_map(Entry::deadline).min();
        state = match next {
            Some(deadline) => shared
                .wakeup
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .map(|(state, _)| state)
                .unwrap_or_else(|e| e.into_inner().0),
            None => shared.wakeup.wait(state).unwrap_or_else(|e| e.into_inner()),
        };
    }
}

fn run(conn: &zbus::Connection, callback: &Callback, job: Job) {
    if let Job::Notified(notification, replaced) = job {
        return callback(NotificationEvent::Notified { notification, replaced });
    }
    let emitted = zbus::block_on(async {
        let emitter = SignalEmitter::new(conn, PATH)?;
        match &job {
            Job::Notified(..) => Ok(()),
            Job::Closed(id, reason) => Interface::notification_closed(&emitter, *id, reason.code()).await,
            Job::Action { id, key, token } => {
                if let Some(token) = token {
                    Interface::activation_token(&emitter, *id, token).await?;
                }
                Interface::action_invoked(&emitter, *id, key).await
            }
        }
    });
    if let Err(error) = emitted {
        log::warn!("aizome-sys: sending a notification signal: {error}");
    }
    if let Job::Closed(id, reason) = job {
        callback(NotificationEvent::Closed { id, reason });
    }
}

/// The served interface.
struct Interface {
    shared: Arc<Shared>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Interface {
    fn get_capabilities(&self) -> Vec<String> {
        self.shared.config.capabilities.clone()
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<u32> {
        let notification = Notification {
            id: 0,
            replaces_id,
            app_name,
            app_icon,
            summary,
            body,
            actions: Notification::parse_actions(actions),
            hints: Hints::from_dbus(hints),
            timeout: Timeout::from_millis(expire_timeout),
            sender: header.sender().map(|s| s.to_string()),
            received: Some(SystemTime::now()),
        };
        self.shared.notify(notification).ok_or_else(|| fdo::Error::Failed("the notification server is stopping".into()))
    }

    fn close_notification(&self, id: u32) -> fdo::Result<()> {
        if self.shared.close(id, CloseReason::Closed) {
            Ok(())
        } else {
            Err(fdo::Error::InvalidArgs(format!("no notification {id}")))
        }
    }

    #[zbus(out_args("name", "vendor", "version", "spec_version"))]
    fn get_server_information(&self) -> (String, String, String, String) {
        let config = &self.shared.config;
        (config.name.clone(), config.vendor.clone(), config.version.clone(), SPEC_VERSION.to_owned())
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn activation_token(emitter: &SignalEmitter<'_>, id: u32, activation_token: &str) -> zbus::Result<()>;
}

/// The notification server; dropping it stops serving.
pub struct NotificationServer {
    bus: Bus,
    shared: Arc<Shared>,
    model: Arc<Model<NotificationsState>>,
    worker: Option<JoinHandle<()>>,
    /// The interface is ours to remove, the name ours to release.
    serving: bool,
    named: bool,
}

service_handle!(NotificationServer, NotificationsState);

impl NotificationServer {
    /// Serve `org.freedesktop.Notifications` on `bus` (the session bus, for
    /// the shell) and take its name. Each change goes to `callback`, from
    /// the server's thread. Fails with [`Error::Unavailable`] when another
    /// server owns the name (and doesn't let this one replace it).
    ///
    /// The name is requested only once the interface answers calls, so no
    /// app's `Notify` can arrive before the server is ready.
    pub fn start(
        bus: &Bus,
        config: ServerConfig,
        callback: impl Fn(NotificationEvent) + Send + Sync + 'static,
    ) -> Result<NotificationServer> {
        let model = Arc::new(Model::new(NotificationsState::default(), Wake::none()));
        let replace = config.replace;
        let shared = Arc::new(Shared { config, state: Mutex::default(), wakeup: Condvar::new(), model: model.clone() });
        let conn = bus.connection().clone();
        let worker = {
            let (shared, conn) = (shared.clone(), conn.clone());
            let callback: Callback = Arc::new(callback);
            std::thread::Builder::new()
                .name("aizome-sys notifications".into())
                .spawn(move || work(shared, conn, callback))
                .map_err(|error| Error::Failed(format!("could not start a thread: {error}")))?
        };
        // From here, dropping `server` undoes whatever was done.
        let mut server =
            NotificationServer { bus: bus.clone(), shared, model, worker: Some(worker), serving: false, named: false };

        // Serve first, so calls arriving as soon as the name is ours find it.
        let interface = Interface { shared: server.shared.clone() };
        server.serving = zbus::block_on(conn.object_server().at(PATH, interface))?;
        if !server.serving {
            return Err(Error::Unavailable("this connection already serves notifications".into()));
        }
        crate::bus::wait_until_serving(bus, PATH)?;
        let flags = if replace {
            RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement | RequestNameFlags::ReplaceExisting
        } else {
            RequestNameFlags::DoNotQueue.into()
        };
        match zbus::block_on(conn.request_name_with_flags(BUS_NAME, flags)) {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                server.named = true;
                Ok(server)
            }
            Ok(RequestNameReply::InQueue) => {
                server.named = true;
                Err(Error::Unavailable(describe_owner(bus)))
            }
            Ok(RequestNameReply::Exists) | Err(zbus::Error::NameTaken) => Err(Error::Unavailable(describe_owner(bus))),
            Err(error) => Err(error.into()),
        }
    }

    /// Close a notification (the person dismissed it: reason
    /// [`CloseReason::Dismissed`]); false if it wasn't open.
    pub fn close(&self, id: u32, reason: CloseReason) -> bool {
        self.shared.close(id, reason)
    }

    /// The person dismissed a notification.
    pub fn dismiss(&self, id: u32) -> bool {
        self.close(id, CloseReason::Dismissed)
    }

    /// The person clicked the notification (the `default` action) or one of
    /// its buttons. Sends `ActivationToken` with `activation_token` (an
    /// xdg-activation token for the app to raise its window with) if there
    /// is one, then `ActionInvoked`; then closes the notification (reason
    /// dismissed) unless it is resident. False if it isn't open or has no
    /// such action.
    pub fn invoke_action(&self, id: u32, key: &str, activation_token: Option<String>) -> bool {
        self.shared.invoke(id, key, activation_token)
    }

    /// Stop a notification's timer (the pointer is over it, or it isn't
    /// shown); false if it isn't open.
    pub fn pause_expiry(&self, id: u32) -> bool {
        self.shared.set_paused(id, true)
    }

    /// Run its timer again, for the time it had left.
    pub fn resume_expiry(&self, id: u32) -> bool {
        self.shared.set_paused(id, false)
    }

    /// An open notification.
    pub fn get(&self, id: u32) -> Option<Arc<Notification>> {
        self.snapshot().get(id).cloned()
    }

    /// The configuration it was started with.
    pub fn config(&self) -> &ServerConfig {
        &self.shared.config
    }

    /// Whether this server still owns the bus name (another may have
    /// replaced it, when [`ServerConfig::replace`] allowed it). Asks the
    /// bus.
    pub fn owns_name(&self) -> bool {
        let owner = self.bus.owner(BUS_NAME);
        owner.is_some() && owner == self.bus.unique_name()
    }
}

impl Drop for NotificationServer {
    fn drop(&mut self) {
        // Close what's open (apps waiting on their notifications hear of
        // it), then stop the thread once it has sent everything, while the
        // name is still ours.
        {
            let mut state = lock(&self.shared.state);
            let open: Vec<u32> = state.entries.keys().copied().collect();
            state.entries.clear();
            if !open.is_empty() {
                self.shared.publish(&state);
            }
            state.stopped = true;
            self.shared.queue(state, open.into_iter().map(|id| Job::Closed(id, CloseReason::Undefined)));
        }
        if let Some(worker) = self.worker.take()
            && worker.thread().id() != std::thread::current().id()
        {
            let _ = worker.join();
        }
        let conn = self.bus.connection().clone();
        if self.named {
            let _ = zbus::block_on(conn.release_name(BUS_NAME));
        }
        if self.serving {
            let _ = zbus::block_on(conn.object_server().remove::<Interface, _>(PATH));
        }
    }
}

/// Who owns the name: its unique name and, when the bus tells, the
/// process's name.
fn describe_owner(bus: &Bus) -> String {
    let Some(owner) = bus.owner(BUS_NAME) else { return format!("{BUS_NAME} is taken") };
    let process = bus
        .call::<_, u32>(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetConnectionUnixProcessID",
            &(owner.as_str(),),
        )
        .ok()
        .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
        .map(|comm| comm.trim().to_owned())
        .filter(|comm| !comm.is_empty());
    match process {
        Some(process) => format!("another notification server owns {BUS_NAME}: {process} ({owner})"),
        None => format!("another notification server owns {BUS_NAME} ({owner})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(config: ServerConfig) -> Shared {
        Shared {
            config,
            state: Mutex::default(),
            wakeup: Condvar::new(),
            model: Arc::new(Model::new(NotificationsState::default(), Wake::none())),
        }
    }

    #[test]
    fn handles_go_between_threads() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<NotificationServer>();
        send_sync::<NotificationEvent>();
        send_sync::<super::super::NotificationClient>();
    }

    #[test]
    fn ids_are_never_zero_nor_reused_while_open() {
        let mut state = State { last_id: u32::MAX - 1, ..State::default() };
        let entry = |id| Entry {
            notification: Arc::new(Notification { id, ..Notification::default() }),
            lifetime: None,
            since: Instant::now(),
            shown: Duration::ZERO,
            paused: false,
        };
        state.entries.insert(1, entry(1));
        state.entries.insert(2, entry(2));
        assert_eq!(state.new_id(), u32::MAX);
        assert_eq!(state.new_id(), 3, "wraps past 0 and the open ones");
    }

    #[test]
    fn lifetimes_follow_the_timeout_and_the_policy() {
        let s = shared(ServerConfig { default_timeout: Duration::from_secs(7), ..ServerConfig::default() });
        let n = |timeout, urgency, resident| Notification {
            timeout,
            hints: Hints { urgency, resident, ..Hints::default() },
            ..Notification::default()
        };
        let secs = |s| Timeout::After(Duration::from_secs(s));
        assert_eq!(s.lifetime(&n(Timeout::Default, Urgency::Normal, false)), Some(Duration::from_secs(7)));
        assert_eq!(s.lifetime(&n(Timeout::Never, Urgency::Normal, false)), None);
        assert_eq!(s.lifetime(&n(secs(2), Urgency::Low, false)), Some(Duration::from_secs(2)));
        assert_eq!(s.lifetime(&n(secs(2), Urgency::Critical, false)), None);
        assert_eq!(s.lifetime(&n(secs(2), Urgency::Normal, true)), None);
        let lax = shared(ServerConfig {
            expire_critical: true,
            expire_resident: true,
            default_timeout: Duration::ZERO,
            ..ServerConfig::default()
        });
        assert_eq!(lax.lifetime(&n(secs(2), Urgency::Critical, true)), Some(Duration::from_secs(2)));
        assert_eq!(lax.lifetime(&n(Timeout::Default, Urgency::Normal, false)), None);
    }

    #[test]
    fn paused_timers_keep_what_they_had_left() {
        let start = Instant::now();
        let mut entry = Entry {
            notification: Arc::new(Notification::default()),
            lifetime: Some(Duration::from_secs(10)),
            since: start,
            shown: Duration::from_secs(4),
            paused: false,
        };
        assert_eq!(entry.deadline(), Some(start + Duration::from_secs(6)));
        entry.paused = true;
        assert_eq!(entry.deadline(), None);
        entry.paused = false;
        entry.lifetime = Some(Duration::MAX);
        assert_eq!(entry.deadline(), None, "too far is never");
    }
}
