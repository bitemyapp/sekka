//! The client side, for apps: send notifications to whichever server owns
//! `org.freedesktop.Notifications` (Aizome's shell, or any other), close
//! them, and hear what happens to them.
//!
//! [`notify`] and [`close`] are one blocking call each on the session bus
//! (2 seconds at most). [`NotificationClient`] does the same on any bus and
//! also follows the server's signals for the notifications it sent:
//! actions invoked (with the `ActivationToken` sent before them, to raise
//! a window with) and closings come to its callback as [`ClientEvent`]s.
//! When the server leaves the bus, the notifications it had are reported
//! closed with [`CloseReason::Undefined`].

use super::{BUS_NAME, CloseReason, INTERFACE, Notification, PATH};
use crate::bus::{self, Bus, Event};
use crate::error::Result;
use crate::service::Stop;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use zbus::message::Message;

/// Signals about ids not yet known are kept this long, and this many, in
/// case the server answers `Notify` faster than the reply is read.
const EARLY_FOR: Duration = Duration::from_secs(10);
const EARLY_MAX: usize = 256;

/// What `GetServerInformation` says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerInformation {
    /// The product (`Aizome`, `dunst`, `gnome-shell`…).
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// The version of the specification it follows.
    pub spec_version: String,
}

/// Send a notification on the session bus; returns its id (keep it to
/// replace or close the notification later).
pub fn notify(notification: &Notification) -> Result<u32> {
    notify_on(&Bus::session()?, notification)
}

/// Send a notification on `bus`.
pub fn notify_on(bus: &Bus, notification: &Notification) -> Result<u32> {
    let n = notification;
    let body = (
        n.app_name.as_str(),
        n.replaces_id,
        n.app_icon.as_str(),
        n.summary.as_str(),
        n.body.as_str(),
        n.wire_actions(),
        n.hints.to_dbus(),
        n.timeout.as_millis(),
    );
    bus.call(BUS_NAME, PATH, INTERFACE, "Notify", &body)
}

/// Close a notification sent earlier, on the session bus. Servers answer
/// with an error when it is already closed.
pub fn close(id: u32) -> Result<()> {
    close_on(&Bus::session()?, id)
}

/// Close a notification on `bus`.
pub fn close_on(bus: &Bus, id: u32) -> Result<()> {
    bus.call(BUS_NAME, PATH, INTERFACE, "CloseNotification", &(id,))
}

/// What the server on `bus` can show (`actions`, `body-markup`…).
pub fn capabilities_on(bus: &Bus) -> Result<Vec<String>> {
    bus.call(BUS_NAME, PATH, INTERFACE, "GetCapabilities", &())
}

/// Which server is on `bus`.
pub fn server_information_on(bus: &Bus) -> Result<ServerInformation> {
    let (name, vendor, version, spec_version): (String, String, String, String) =
        bus.call(BUS_NAME, PATH, INTERFACE, "GetServerInformation", &())?;
    Ok(ServerInformation { name, vendor, version, spec_version })
}

/// What happened to a notification a [`NotificationClient`] sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientEvent {
    /// The person invoked an action (`default` for a click on the
    /// notification). `activation_token`, when the server sent one, lets
    /// the app raise its window (xdg-activation, or an X11 startup id).
    ActionInvoked { id: u32, action: String, activation_token: Option<String> },
    /// The notification closed; its id is no longer valid.
    Closed { id: u32, reason: CloseReason },
}

/// Sends notifications and reports what happens to them.
pub struct NotificationClient {
    bus: Bus,
    tx: mpsc::Sender<Msg>,
    stop: Stop,
}

enum Msg {
    /// `notify` returned this id.
    Sent(u32),
    Signal(Signal),
    /// The server left the bus.
    ServerGone,
    Stop,
}

enum Signal {
    Token(u32, String),
    Action(u32, String),
    Closed(u32, CloseReason),
}

impl Signal {
    fn id(&self) -> u32 {
        match self {
            Signal::Token(id, _) | Signal::Action(id, _) | Signal::Closed(id, _) => *id,
        }
    }

    fn parse(msg: &Message) -> Option<Signal> {
        let (iface, member) = bus::member(msg);
        if iface != INTERFACE {
            return None;
        }
        let body = msg.body();
        match member.as_str() {
            "ActionInvoked" => body.deserialize::<(u32, String)>().ok().map(|(id, key)| Signal::Action(id, key)),
            "ActivationToken" => body.deserialize::<(u32, String)>().ok().map(|(id, token)| Signal::Token(id, token)),
            "NotificationClosed" => body
                .deserialize::<(u32, u32)>()
                .ok()
                .map(|(id, reason)| Signal::Closed(id, CloseReason::from_code(reason))),
            _ => None,
        }
    }
}

impl NotificationClient {
    /// A client on the session bus; `callback` hears about the
    /// notifications this client sends, on a thread of its own.
    pub fn new(callback: impl FnMut(ClientEvent) + Send + 'static) -> Result<NotificationClient> {
        Ok(NotificationClient::with_bus(Bus::session()?, callback))
    }

    /// A client on `bus`. Returns once it follows the server's signals.
    pub fn with_bus(bus: Bus, callback: impl FnMut(ClientEvent) + Send + 'static) -> NotificationClient {
        let (tx, rx) = mpsc::channel();
        let dispatcher = Dispatcher { sent: HashSet::new(), tokens: HashMap::new(), early: VecDeque::new(), callback };
        let spawned =
            std::thread::Builder::new().name("aizome-sys notification client".into()).spawn(move || dispatcher.run(rx));
        if let Err(error) = spawned {
            log::warn!("aizome-sys: could not start a thread for notification events: {error}");
        }

        let stop = Stop::default();
        let (ready_tx, ready) = mpsc::channel();
        let mut ready_tx = Some(ready_tx);
        let signals = tx.clone();
        let rule = format!("type='signal',sender='{BUS_NAME}',interface='{INTERFACE}'");
        bus::follow(bus.clone(), BUS_NAME, vec![rule], false, stop.clone(), move |_, event| match event {
            Event::Signal(msg) => {
                if let Some(signal) = Signal::parse(&msg) {
                    let _ = signals.send(Msg::Signal(signal));
                }
            }
            Event::Vanished => {
                let _ = signals.send(Msg::ServerGone);
            }
            Event::Idle => {
                if let Some(ready) = ready_tx.take() {
                    let _ = ready.send(());
                }
            }
            Event::Appeared => {}
        });
        // Subscribed before anything is sent, so no signal is missed.
        let _ = ready.recv_timeout(Duration::from_secs(5));
        NotificationClient { bus, tx, stop }
    }

    /// Send a notification; returns its id.
    pub fn notify(&self, notification: &Notification) -> Result<u32> {
        let id = notify_on(&self.bus, notification)?;
        let _ = self.tx.send(Msg::Sent(id));
        Ok(id)
    }

    /// Close a notification this client sent.
    pub fn close(&self, id: u32) -> Result<()> {
        close_on(&self.bus, id)
    }

    /// What the server can show.
    pub fn capabilities(&self) -> Result<Vec<String>> {
        capabilities_on(&self.bus)
    }

    /// Which server it is.
    pub fn server_information(&self) -> Result<ServerInformation> {
        server_information_on(&self.bus)
    }
}

impl Drop for NotificationClient {
    fn drop(&mut self) {
        self.stop.stop();
        let _ = self.tx.send(Msg::Stop);
    }
}

/// Matches the server's signals to the ids sent, on the client's thread.
struct Dispatcher<F> {
    sent: HashSet<u32>,
    /// Activation tokens waiting for their `ActionInvoked`.
    tokens: HashMap<u32, String>,
    /// Signals about ids not (yet) sent by this client: other apps'
    /// notifications, or ours with a reply still on its way.
    early: VecDeque<(Instant, Signal)>,
    callback: F,
}

impl<F: FnMut(ClientEvent)> Dispatcher<F> {
    fn run(mut self, rx: mpsc::Receiver<Msg>) {
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::Stop => return,
                Msg::Sent(id) => {
                    self.sent.insert(id);
                    for (at, signal) in std::mem::take(&mut self.early) {
                        if signal.id() == id {
                            self.deliver(signal);
                        } else {
                            self.early.push_back((at, signal));
                        }
                    }
                }
                Msg::Signal(signal) if self.sent.contains(&signal.id()) => self.deliver(signal),
                Msg::Signal(signal) => {
                    let now = Instant::now();
                    self.early.retain(|(at, _)| now.duration_since(*at) < EARLY_FOR);
                    if self.early.len() >= EARLY_MAX {
                        self.early.pop_front();
                    }
                    self.early.push_back((now, signal));
                }
                Msg::ServerGone => {
                    self.tokens.clear();
                    self.early.clear();
                    let mut gone: Vec<u32> = self.sent.drain().collect();
                    gone.sort_unstable();
                    for id in gone {
                        (self.callback)(ClientEvent::Closed { id, reason: CloseReason::Undefined });
                    }
                }
            }
        }
    }

    fn deliver(&mut self, signal: Signal) {
        match signal {
            Signal::Token(id, token) => {
                self.tokens.insert(id, token);
            }
            Signal::Action(id, action) => {
                let activation_token = self.tokens.remove(&id);
                (self.callback)(ClientEvent::ActionInvoked { id, action, activation_token });
            }
            Signal::Closed(id, reason) => {
                self.sent.remove(&id);
                self.tokens.remove(&id);
                (self.callback)(ClientEvent::Closed { id, reason });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn signals_before_the_reply_wait_for_their_id() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let (tx, rx) = mpsc::channel();
        let dispatcher = Dispatcher {
            sent: HashSet::new(),
            tokens: HashMap::new(),
            early: VecDeque::new(),
            callback: move |event| seen.lock().unwrap().push(event),
        };
        let thread = std::thread::spawn(move || dispatcher.run(rx));
        for msg in [
            // Another app's notification: never reported.
            Msg::Signal(Signal::Closed(3, CloseReason::Expired)),
            // Ours, before `notify` returned 7.
            Msg::Signal(Signal::Token(7, "token".into())),
            Msg::Signal(Signal::Action(7, "default".into())),
            Msg::Sent(7),
            Msg::Signal(Signal::Closed(7, CloseReason::Dismissed)),
            Msg::Sent(8),
            Msg::Signal(Signal::Closed(7, CloseReason::Dismissed)),
            Msg::ServerGone,
            Msg::Stop,
        ] {
            tx.send(msg).unwrap();
        }
        thread.join().unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            [
                ClientEvent::ActionInvoked { id: 7, action: "default".into(), activation_token: Some("token".into()) },
                ClientEvent::Closed { id: 7, reason: CloseReason::Dismissed },
                ClientEvent::Closed { id: 8, reason: CloseReason::Undefined },
            ]
        );
    }
}
