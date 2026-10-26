//! The notification server on a private bus, driven by the client helper
//! and by raw D-Bus calls.

mod common;

use aizome_sys::Error;
use aizome_sys::notifications::server::{DEFAULT_CAPABILITIES, SPEC_VERSION};
use aizome_sys::notifications::{
    self, BUS_NAME, ClientEvent, CloseReason, ImageData, Notification, NotificationClient, NotificationEvent,
    NotificationServer, ServerConfig, Timeout, Urgency,
};
use common::{TestBus, eventually};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::zvariant::Value;

type Events<T> = Arc<Mutex<Vec<T>>>;

fn start(bus: &TestBus, config: ServerConfig) -> (NotificationServer, Events<(Instant, NotificationEvent)>) {
    let events: Events<(Instant, NotificationEvent)> = Arc::default();
    let seen = events.clone();
    let server = NotificationServer::start(&bus.bus(), config, move |event| {
        seen.lock().unwrap().push((Instant::now(), event));
    })
    .expect("start the server");
    (server, events)
}

fn client(bus: &TestBus) -> (NotificationClient, Events<ClientEvent>, String) {
    let events: Events<ClientEvent> = Arc::default();
    let seen = events.clone();
    let conn = bus.bus();
    let me = conn.unique_name().unwrap();
    let client = NotificationClient::with_bus(conn, move |event| seen.lock().unwrap().push(event));
    (client, events, me)
}

fn has<T: PartialEq>(events: &Events<T>, event: &T) -> bool {
    events.lock().unwrap().contains(event)
}

fn server_events(events: &Events<(Instant, NotificationEvent)>) -> Vec<NotificationEvent> {
    events.lock().unwrap().iter().map(|(_, e)| e.clone()).collect()
}

fn closed(events: &Events<(Instant, NotificationEvent)>, id: u32) -> Option<(Instant, CloseReason)> {
    events.lock().unwrap().iter().find_map(|(at, e)| match e {
        NotificationEvent::Closed { id: closed, reason } if *closed == id => Some((*at, *reason)),
        _ => None,
    })
}

/// The server's signals as another connection hears them, in order:
/// (member, id, the other argument as text).
fn record_signals(bus: &TestBus) -> Events<(String, u32, String)> {
    let conn = bus.builder().build().unwrap();
    let rule = zbus::MatchRule::try_from("type='signal',interface='org.freedesktop.Notifications'").unwrap();
    let messages = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(64)).unwrap();
    let signals: Events<(String, u32, String)> = Arc::default();
    let seen = signals.clone();
    std::thread::spawn(move || {
        let _conn = conn;
        for msg in messages.flatten() {
            let member = msg.header().member().map(|m| m.to_string()).unwrap_or_default();
            let body = msg.body();
            let entry = match member.as_str() {
                "NotificationClosed" => body.deserialize::<(u32, u32)>().map(|(id, r)| (id, r.to_string())),
                _ => body.deserialize::<(u32, String)>(),
            };
            if let Ok((id, arg)) = entry {
                seen.lock().unwrap().push((member, id, arg));
            }
        }
    });
    signals
}

fn signal(member: &str, id: u32, arg: &str) -> (String, u32, String) {
    (member.to_owned(), id, arg.to_owned())
}

/// A raw `Notify` from a plain zbus connection.
fn raw_notify(
    conn: &zbus::blocking::Connection,
    replaces: u32,
    actions: &[&str],
    hints: HashMap<&str, Value<'_>>,
    timeout: i32,
) -> zbus::Result<u32> {
    conn.call_method(
        Some(BUS_NAME),
        notifications::PATH,
        Some(notifications::INTERFACE),
        "Notify",
        &("raw app", replaces, "dialog-information", "Raw summary", "Raw body", actions, hints, timeout),
    )?
    .body()
    .deserialize()
}

#[test]
fn answers_capabilities_and_server_information() {
    let bus = test_bus!();
    let (_server, _) = start(&bus, ServerConfig::default());
    let (client, _, _) = client(&bus);
    assert_eq!(client.capabilities().unwrap(), DEFAULT_CAPABILITIES);
    let info = client.server_information().unwrap();
    assert_eq!(
        (info.name.as_str(), info.vendor.as_str(), info.spec_version.as_str()),
        ("Aizome", "Sekka", SPEC_VERSION)
    );
    assert_eq!(info.spec_version, "1.2");

    // Four out arguments, not one structure; the signals are introspectable.
    let raw = bus.builder().build().unwrap();
    let reply = raw
        .call_method(Some(BUS_NAME), notifications::PATH, Some(notifications::INTERFACE), "GetServerInformation", &())
        .unwrap();
    assert_eq!(reply.body().signature().to_string_no_parens(), "ssss");
    let xml: String = raw
        .call_method(
            Some(BUS_NAME),
            notifications::PATH,
            Some("org.freedesktop.DBus.Introspectable"),
            "Introspect",
            &(),
        )
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    for name in ["Notify", "CloseNotification", "NotificationClosed", "ActionInvoked", "ActivationToken"] {
        assert!(xml.contains(&format!("name=\"{name}\"")), "{name} in {xml}");
    }

    // A server that advertises less.
    drop(_server);
    let custom = ServerConfig { capabilities: vec!["body".into()], name: "Test".into(), ..ServerConfig::default() };
    let (_server, _) = start(&bus, custom);
    assert_eq!(notifications::capabilities_on(&bus.bus()).unwrap(), ["body"]);
    assert_eq!(notifications::server_information_on(&bus.bus()).unwrap().name, "Test");
}

#[test]
fn a_notification_reaches_the_shell_with_its_actions_and_hints() {
    let bus = test_bus!();
    let (server, events) = start(&bus, ServerConfig::default());
    let (client, _, me) = client(&bus);
    let pixels = vec![255, 0, 0, 255, 0, 0, 255, 128];
    let sent = Notification::new("New message")
        .body("<b>Ada</b> says: tea &amp; cake?")
        .icon("mail-unread")
        .app_name("Mail")
        .urgency(Urgency::Critical)
        .category("email.arrived")
        .desktop_entry("org.example.Mail")
        .default_action("Open")
        .action("reply", "Reply")
        .image(ImageData::from_rgba(2, 1, pixels.clone()))
        .sound_name("message-new-email")
        .transient(true)
        .hint("x-aizome-account", "work")
        .hint("sender-pid", 4242u32)
        .timeout(Timeout::Never);
    let id = client.notify(&sent).unwrap();
    assert!(id > 0);
    assert!(eventually(|| !events.lock().unwrap().is_empty()));

    let NotificationEvent::Notified { notification, replaced } = server_events(&events).remove(0) else {
        panic!("not a notification: {:?}", server_events(&events));
    };
    assert!(!replaced);
    assert_eq!(notification.id, id);
    assert_eq!(notification.sender.as_deref(), Some(me.as_str()));
    assert!(notification.received.is_some());
    let mut expected = sent.clone();
    expected.id = id;
    expected.sender = notification.sender.clone();
    expected.received = notification.received;
    assert_eq!(*notification, expected, "everything sent arrives");
    assert_eq!(notification.plain_body(), "Ada says: tea & cake?");
    assert_eq!(notification.hints.image_data.as_ref().unwrap().to_rgba8().unwrap(), pixels);
    assert_eq!(notification.click_action().unwrap().label, "Open");
    assert_eq!(notification.hints.other.len(), 2);

    let state = server.snapshot();
    assert_eq!(state.notifications.len(), 1);
    assert_eq!(server.get(id).unwrap().summary, "New message");
}

#[test]
fn odd_raw_notifications_are_read_leniently() {
    let bus = test_bus!();
    let (server, _) = start(&bus, ServerConfig::default());
    let raw = bus.builder().build().unwrap();
    let hints = HashMap::from([
        ("urgency", Value::from(0u32)),
        ("category", Value::from(5i32)),
        ("desktop-entry", Value::from("org.example.Old.desktop")),
        ("image_data", Value::from((1i32, 1i32, 3i32, false, 8i32, 3i32, vec![1u8, 2, 3]))),
        ("icon_data", Value::from("not pixels")),
        ("image_path", Value::from("file:///tmp/old%20name.png")),
        ("resident", Value::from(1u8)),
        ("x", Value::from(10i16)),
    ]);
    let id = raw_notify(&raw, 0, &["default", "Open", "dangling"], hints, -5).unwrap();
    assert!(eventually(|| server.get(id).is_some()));
    let n = server.get(id).unwrap();
    assert_eq!(n.app_name, "raw app");
    assert_eq!(n.timeout, Timeout::Default);
    assert_eq!(n.actions.len(), 1, "a key without a label is dropped");
    assert_eq!(n.hints.urgency, Urgency::Low);
    assert_eq!(n.hints.category, None);
    assert_eq!(n.hints.desktop_entry.as_deref(), Some("org.example.Old"));
    assert_eq!(n.hints.image_data.as_ref().unwrap().to_rgba8().unwrap(), [1, 2, 3, 255]);
    assert_eq!(n.hints.icon_data, None);
    assert!(n.hints.resident);
    assert_eq!((n.hints.x, n.hints.position()), (Some(10), None));
    assert!(matches!(n.image_to_show(), Some(notifications::Image::Data(_))));
    let mut without_data = (*n).clone();
    without_data.hints.image_data = None;
    assert_eq!(without_data.image_to_show(), Some(notifications::Image::File("/tmp/old name.png".into())));

    // Wrong argument types are refused by the bus layer, not a panic.
    let refused =
        raw.call_method(Some(BUS_NAME), notifications::PATH, Some(notifications::INTERFACE), "Notify", &("only", 1u8));
    assert!(refused.is_err());
    assert_eq!(server.snapshot().notifications.len(), 1);
}

#[test]
fn replaces_in_place_and_gives_unknown_ids_new_ones() {
    let bus = test_bus!();
    let (server, events) = start(&bus, ServerConfig::default());
    let (client, _, _) = client(&bus);
    let first = client.notify(&Notification::new("Downloading 10%").timeout(Timeout::Never)).unwrap();
    let other = client.notify(&Notification::new("Other").timeout(Timeout::Never)).unwrap();
    let same = client.notify(&Notification::new("Downloading 50%").replaces(first).timeout(Timeout::Never)).unwrap();
    assert_eq!(same, first, "the id is kept");
    assert!(eventually(|| server_events(&events).len() == 3));
    let state = server.snapshot();
    let summaries: Vec<&str> = state.notifications.iter().map(|n| n.summary.as_str()).collect();
    assert_eq!(summaries, ["Downloading 50%", "Other"], "replaced in place");
    match &server_events(&events)[2] {
        NotificationEvent::Notified { notification, replaced: true } => assert_eq!(notification.id, first),
        other => panic!("{other:?}"),
    }

    let fresh = client.notify(&Notification::new("Unknown").replaces(9999)).unwrap();
    assert!(fresh != 9999 && fresh != first && fresh != other && fresh > other, "{fresh}");
    assert!(eventually(|| server_events(&events).len() == 4));
    assert!(matches!(server_events(&events)[3], NotificationEvent::Notified { replaced: false, .. }));
}

#[test]
fn close_notification_sends_closed_by_call() {
    let bus = test_bus!();
    let signals = record_signals(&bus);
    let (server, events) = start(&bus, ServerConfig::default());
    let (client, heard, _) = client(&bus);
    let id = client.notify(&Notification::new("Build finished").timeout(Timeout::Never)).unwrap();
    client.close(id).unwrap();
    let event = ClientEvent::Closed { id, reason: CloseReason::Closed };
    assert!(eventually(|| has(&heard, &event)));
    assert!(eventually(|| !signals.lock().unwrap().is_empty()));
    assert_eq!(*signals.lock().unwrap(), [signal("NotificationClosed", id, "3")]);
    assert!(eventually(|| closed(&events, id).map(|(_, r)| r) == Some(CloseReason::Closed)));
    assert!(server.snapshot().notifications.is_empty());
    // Closing it again is the error the specification asks for.
    assert!(matches!(client.close(id), Err(Error::InvalidArgument(_))));
    assert!(matches!(notifications::close_on(&bus.bus(), 12345), Err(Error::InvalidArgument(_))));
}

#[test]
fn dismissing_sends_dismissed() {
    let bus = test_bus!();
    let signals = record_signals(&bus);
    let (server, events) = start(&bus, ServerConfig::default());
    let (client, heard, _) = client(&bus);
    let id = client.notify(&Notification::new("Battery low")).unwrap();
    assert!(eventually(|| server.get(id).is_some()));
    assert!(server.dismiss(id));
    assert!(!server.dismiss(id), "only once");
    assert!(eventually(|| has(&heard, &ClientEvent::Closed { id, reason: CloseReason::Dismissed })));
    assert!(eventually(|| !signals.lock().unwrap().is_empty()));
    assert_eq!(*signals.lock().unwrap(), [signal("NotificationClosed", id, "2")]);
    assert!(eventually(|| closed(&events, id).map(|(_, r)| r) == Some(CloseReason::Dismissed)));
    assert!(server.snapshot().notifications.is_empty());
    assert_eq!(heard.lock().unwrap().len(), 1);
}

#[test]
fn notifications_expire_on_time() {
    let bus = test_bus!();
    let config = ServerConfig { default_timeout: Duration::from_millis(200), ..ServerConfig::default() };
    let (server, events) = start(&bus, config);
    let (client, heard, _) = client(&bus);
    let ms = |ms| Timeout::After(Duration::from_millis(ms));
    let sent_at = Instant::now();
    let short = client.notify(&Notification::new("short").timeout(ms(150))).unwrap();
    let default = client.notify(&Notification::new("default")).unwrap();
    let never = client.notify(&Notification::new("never").timeout(Timeout::Never)).unwrap();
    let critical = client.notify(&Notification::new("critical").timeout(ms(100)).urgency(Urgency::Critical)).unwrap();
    let resident = client.notify(&Notification::new("resident").timeout(ms(100)).resident(true)).unwrap();

    assert!(eventually(|| closed(&events, short).is_some() && closed(&events, default).is_some()));
    let (short_at, reason) = closed(&events, short).unwrap();
    assert_eq!(reason, CloseReason::Expired);
    assert!(short_at.duration_since(sent_at) >= Duration::from_millis(150));
    let (default_at, _) = closed(&events, default).unwrap();
    assert!(default_at.duration_since(sent_at) >= Duration::from_millis(200));
    assert!(eventually(|| has(&heard, &ClientEvent::Closed { id: short, reason: CloseReason::Expired })));
    assert!(eventually(|| has(&heard, &ClientEvent::Closed { id: default, reason: CloseReason::Expired })));

    std::thread::sleep(Duration::from_millis(300));
    let open: Vec<u32> = server.snapshot().notifications.iter().map(|n| n.id).collect();
    assert_eq!(open, [never, critical, resident], "never, critical and resident ones stay");
}

#[test]
fn paused_timers_wait_for_the_pointer_to_leave() {
    let bus = test_bus!();
    let (server, events) = start(&bus, ServerConfig::default());
    let (client, _, _) = client(&bus);
    let id = client.notify(&Notification::new("hover me").timeout(Timeout::After(Duration::from_millis(300)))).unwrap();
    assert!(server.pause_expiry(id));
    std::thread::sleep(Duration::from_millis(500));
    assert!(server.get(id).is_some(), "paused");
    // Replaced while paused: still paused.
    client
        .notify(&Notification::new("hover me more").replaces(id).timeout(Timeout::After(Duration::from_millis(200))))
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(server.get(id).unwrap().summary, "hover me more");
    let resumed = Instant::now();
    assert!(server.resume_expiry(id));
    assert!(eventually(|| closed(&events, id).is_some()));
    let (at, reason) = closed(&events, id).unwrap();
    assert_eq!(reason, CloseReason::Expired);
    assert!(at.duration_since(resumed) >= Duration::from_millis(150), "it had its time left");
    assert!(!server.pause_expiry(id) && !server.resume_expiry(id), "gone");
}

#[test]
fn invoking_an_action_sends_the_token_then_the_action() {
    let bus = test_bus!();
    let signals = record_signals(&bus);
    let (server, events) = start(&bus, ServerConfig::default());
    let (client, heard, _) = client(&bus);
    let message = Notification::new("Ada").body("tea?").default_action("Open").action("reply", "Reply");
    let id = client.notify(&message).unwrap();
    assert!(eventually(|| server.get(id).is_some()));
    assert!(!server.invoke_action(id, "delete", None), "no such action");
    assert!(!server.invoke_action(id + 100, "reply", None), "no such notification");
    assert!(server.invoke_action(id, "reply", Some("xdg-token-1".into())));

    let action =
        ClientEvent::ActionInvoked { id, action: "reply".into(), activation_token: Some("xdg-token-1".into()) };
    let close = ClientEvent::Closed { id, reason: CloseReason::Dismissed };
    assert!(eventually(|| heard.lock().unwrap().len() == 2));
    assert_eq!(*heard.lock().unwrap(), [action, close], "the action, then the closing");
    assert!(eventually(|| signals.lock().unwrap().len() == 3));
    assert_eq!(
        *signals.lock().unwrap(),
        [
            signal("ActivationToken", id, "xdg-token-1"),
            signal("ActionInvoked", id, "reply"),
            signal("NotificationClosed", id, "2"),
        ]
    );
    assert!(eventually(|| closed(&events, id).map(|(_, r)| r) == Some(CloseReason::Dismissed)));

    // A resident notification stays; no token, no ActivationToken.
    let resident = client.notify(&message.clone().resident(true)).unwrap();
    assert!(eventually(|| server.get(resident).is_some()));
    assert!(server.invoke_action(resident, "default", None));
    let clicked = ClientEvent::ActionInvoked { id: resident, action: "default".into(), activation_token: None };
    assert!(eventually(|| has(&heard, &clicked)));
    assert!(eventually(|| signals.lock().unwrap().len() == 4));
    assert_eq!(signals.lock().unwrap()[3], signal("ActionInvoked", resident, "default"));
    std::thread::sleep(Duration::from_millis(100));
    assert!(server.get(resident).is_some());
    assert_eq!(heard.lock().unwrap().len(), 3);
}

#[test]
fn the_name_taken_by_another_server_is_an_error() {
    let bus = test_bus!();
    // zbus lets names be replaced unless told otherwise.
    let holder = bus.builder().name(BUS_NAME).unwrap().allow_name_replacements(false).build().unwrap();
    let owner = holder.unique_name().unwrap().to_string();
    let result = NotificationServer::start(&bus.bus(), ServerConfig::default(), |_| {});
    match result {
        Err(Error::Unavailable(message)) => {
            assert!(message.contains("another notification server") && message.contains(&owner), "{message}");
        }
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("started next to another server"),
    }
    // Replacing needs the other's leave.
    let replace = ServerConfig { replace: true, ..ServerConfig::default() };
    assert!(matches!(NotificationServer::start(&bus.bus(), replace, |_| {}), Err(Error::Unavailable(_))));
    // The failed attempts left nothing behind on the holder's name.
    assert_eq!(common_owner(&bus).as_deref(), Some(owner.as_str()));

    drop(holder);
    assert!(eventually(|| common_owner(&bus).is_none()));
    let (server, _) = start(&bus, ServerConfig::default());
    assert!(server.owns_name());
}

fn common_owner(bus: &TestBus) -> Option<String> {
    let conn = bus.builder().build().unwrap();
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "GetNameOwner",
        &(BUS_NAME,),
    )
    .ok()?
    .body()
    .deserialize()
    .ok()
}

#[test]
fn a_new_shell_replaces_one_that_allows_it() {
    let bus = test_bus!();
    let replace = ServerConfig { replace: true, ..ServerConfig::default() };
    let (old, old_events) = start(&bus, replace.clone());
    assert!(old.owns_name());
    let (new, new_events) = start(&bus, replace);
    assert!(new.owns_name());
    assert!(!old.owns_name());
    let (client, _, _) = client(&bus);
    client.notify(&Notification::new("to the new one")).unwrap();
    assert!(eventually(|| server_events(&new_events).len() == 1));
    assert!(server_events(&old_events).is_empty());
    drop(old);
    assert!(new.owns_name(), "the old one doesn't release what it lost");
}

#[test]
fn dropping_the_server_closes_and_releases() {
    let bus = test_bus!();
    let (server, events) = start(&bus, ServerConfig::default());
    let (client, heard, _) = client(&bus);
    let id = client.notify(&Notification::new("left open").timeout(Timeout::Never)).unwrap();
    assert!(eventually(|| server.get(id).is_some()));
    drop(server);
    assert_eq!(closed(&events, id).map(|(_, r)| r), Some(CloseReason::Undefined));
    assert!(eventually(|| has(&heard, &ClientEvent::Closed { id, reason: CloseReason::Undefined })));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(heard.lock().unwrap().len(), 1, "reported once");
    assert_eq!(common_owner(&bus), None);
    // Another server can start, on a connection that served one before too.
    let conn = bus.bus();
    let again = NotificationServer::start(&conn, ServerConfig::default(), |_| {}).unwrap();
    drop(again);
    NotificationServer::start(&conn, ServerConfig::default(), |_| {}).unwrap();
}

#[test]
fn a_client_hears_only_about_its_own_notifications() {
    let bus = test_bus!();
    let (server, _) = start(&bus, ServerConfig::default());
    let (mine, heard, _) = client(&bus);
    let (theirs, _, _) = client(&bus);
    let their_id = theirs.notify(&Notification::new("theirs").action("default", "Open")).unwrap();
    let my_id = mine.notify(&Notification::new("mine")).unwrap();
    assert!(eventually(|| server.snapshot().notifications.len() == 2));
    assert!(server.invoke_action(their_id, "default", None));
    assert!(server.dismiss(my_id));
    assert!(eventually(|| has(&heard, &ClientEvent::Closed { id: my_id, reason: CloseReason::Dismissed })));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(heard.lock().unwrap().len(), 1, "{:?}", heard.lock().unwrap());
}

#[test]
fn without_a_server_sending_fails() {
    let bus = test_bus!();
    let result = notifications::notify_on(&bus.bus(), &Notification::new("nobody listens"));
    assert!(matches!(result, Err(Error::Unavailable(_))), "{result:?}");
    let (client, _, _) = client(&bus);
    assert!(client.notify(&Notification::new("nobody")).is_err());
}

#[test]
fn a_second_server_on_one_connection_is_refused() {
    let bus = test_bus!();
    let conn = bus.bus();
    let _first = NotificationServer::start(&conn, ServerConfig::default(), |_| {}).unwrap();
    let second = NotificationServer::start(&conn, ServerConfig::default(), |_| {});
    assert!(matches!(second, Err(Error::Unavailable(_))));
    drop(second);
    // The first is untouched by the second's failure.
    assert_eq!(common_owner(&bus), conn.unique_name());
    let id = notifications::notify_on(&bus.bus(), &Notification::new("still here")).unwrap();
    assert!(id > 0);
}
