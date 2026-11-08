//! The `org.freedesktop.Notifications` server (Desktop Notifications
//! Specification 1.2) on the session bus: `Notify` hands what an app asked
//! for to the shell and answers with its id at once; the shell's closes
//! and action clicks go back as `NotificationClosed` and `ActionInvoked`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

use super::store::{CloseReason, Incoming, NoteImage, NoteSound, Urgency};

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";

/// What the server tells the shell.
#[derive(Clone, Debug, PartialEq)]
pub enum NoteEvent {
    /// A notification to show, with the id it was given.
    Notify(u32, Box<Incoming>),
    /// An app closed one of its notifications.
    Close(u32),
    /// The bus name is this shell's.
    NameAcquired,
    /// Another shell took the bus name (a restart): notifications go to it.
    NameLost,
}

struct Notifications {
    next: Arc<AtomicU32>,
    sink: Arc<dyn Fn(NoteEvent) + Send + Sync>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<String> {
        ["actions", "body", "body-markup", "body-hyperlinks", "icon-static", "persistence", "sound"]
            .iter()
            .map(|s| s.to_string())
            .collect()
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
    ) -> u32 {
        let id = if replaces_id != 0 { replaces_id } else { self.next.fetch_add(1, Ordering::Relaxed).max(1) };
        let n = incoming(app_name, replaces_id, app_icon, summary, body, actions, &hints, expire_timeout);
        (self.sink)(NoteEvent::Notify(id, Box::new(n)));
        id
    }

    fn close_notification(&self, id: u32) {
        (self.sink)(NoteEvent::Close(id));
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("Aizome".into(), "Sekka".into(), env!("CARGO_PKG_VERSION").into(), "1.2".into())
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;
}

/// What `Notify`'s arguments ask for.
#[allow(clippy::too_many_arguments)]
pub fn incoming(
    app_name: String,
    replaces_id: u32,
    app_icon: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    hints: &HashMap<String, OwnedValue>,
    expire_timeout: i32,
) -> Incoming {
    let s = |k: &str| hints.get(k).and_then(|v| String::try_from(v.try_clone().ok()?).ok());
    let b = |k: &str| hints.get(k).and_then(|v| bool::try_from(&**v).ok()).unwrap_or(false);
    let urgency = hints.get("urgency").and_then(|v| u8::try_from(&**v).ok()).map(Urgency::from_u8).unwrap_or_default();
    let image = ["image-data", "image_data", "icon_data"]
        .iter()
        .find_map(|k| hints.get(*k).and_then(image_data))
        .or_else(|| s("image-path").or_else(|| s("image_path")).map(|p| NoteImage::File(path_of(&p))))
        .or_else(|| {
            (app_icon.starts_with('/') || app_icon.starts_with("file://")).then(|| NoteImage::File(path_of(&app_icon)))
        });
    let pid = hints
        .get("sender-pid")
        .and_then(|v| u32::try_from(&**v).ok().or_else(|| i64::try_from(&**v).ok().map(|p| p as u32)));
    Incoming {
        app_name,
        replaces_id,
        app_icon,
        summary,
        body,
        actions: actions.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].clone(), c[1].clone())).collect(),
        urgency,
        desktop_entry: s("desktop-entry"),
        image,
        category: s("category"),
        expire_timeout,
        transient: b("transient"),
        resident: b("resident"),
        sound: if b("suppress-sound") {
            None
        } else {
            s("sound-file").map(|p| NoteSound::File(path_of(&p))).or_else(|| s("sound-name").map(NoteSound::Named))
        },
        pid,
    }
}

fn path_of(s: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(crate::shelf::percent_decode(s.strip_prefix("file://").unwrap_or(s)))
}

/// `image-data`'s `(iiibiiay)` as straight RGBA.
fn image_data(v: &OwnedValue) -> Option<NoteImage> {
    let Value::Structure(st) = &**v else { return None };
    let f = st.fields();
    if f.len() != 7 {
        return None;
    }
    let int = |i: usize| i32::try_from(&f[i]).ok();
    let (w, h, stride) = (int(0)?, int(1)?, int(2)?);
    let alpha = bool::try_from(&f[3]).ok()?;
    let channels = int(5)?;
    let data: Vec<u8> = match &f[6] {
        Value::Array(a) => a.iter().filter_map(|x| u8::try_from(x).ok()).collect(),
        _ => return None,
    };
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
        return None;
    }
    let (w, h, stride, ch) = (w as usize, h as usize, stride as usize, channels.max(3) as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let i = y * stride + x * ch;
            let px = data.get(i..i + ch)?;
            rgba.extend_from_slice(&[px[0], px[1], px[2], if alpha && ch >= 4 { px[3] } else { 255 }]);
        }
    }
    Some(NoteImage::Pixels { width: w as u32, height: h as u32, rgba }.fit(crate::notifications::store::IMAGE_PX))
}

/// The server, while it holds the name.
pub struct NoteServer {
    tx: std::sync::mpsc::Sender<Signal>,
    /// The next id to give, shared with the D-Bus interface: the shell's own
    /// notifications take theirs from here too, so no two share one.
    next: Arc<AtomicU32>,
}

enum Signal {
    Closed(u32, u32),
    Action(u32, String),
}

impl NoteServer {
    /// Take the name on the session bus and serve, giving ids from `first`
    /// on; `sink` hears what apps ask (on the bus's thread). The name is
    /// held allowing replacement and taken replacing whoever has it, so a
    /// restarted shell takes it over from the old one without a gap.
    pub fn start(first: u32, sink: impl Fn(NoteEvent) + Send + Sync + 'static) -> zbus::Result<NoteServer> {
        let sink: Arc<dyn Fn(NoteEvent) + Send + Sync> = Arc::new(sink);
        let next = Arc::new(AtomicU32::new(first.max(1)));
        let iface = Notifications { next: next.clone(), sink: sink.clone() };
        let conn =
            zbus::block_on(async { zbus::connection::Builder::session()?.serve_at(PATH, iface)?.build().await })?;
        {
            let (conn, sink) = (conn.clone(), sink.clone());
            std::thread::Builder::new()
                .name("notifications-name".into())
                .spawn(move || watch_name(conn, sink))
                .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        }
        let (tx, rx) = std::sync::mpsc::channel::<Signal>();
        std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                for s in rx {
                    let r = zbus::block_on(async {
                        let emitter = SignalEmitter::new(&conn, PATH)?;
                        match s {
                            Signal::Closed(id, reason) => {
                                Notifications::notification_closed(&emitter, id, reason).await
                            }
                            Signal::Action(id, key) => Notifications::action_invoked(&emitter, id, &key).await,
                        }
                    });
                    if let Err(e) = r {
                        log::debug!("a notification signal: {e}");
                    }
                }
            })
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        Ok(NoteServer { tx, next })
    }

    /// An id for a notification of the shell's own.
    pub fn fresh_id(&self) -> u32 {
        self.next.fetch_add(1, Ordering::Relaxed).max(1)
    }

    pub fn closed(&self, id: u32, reason: CloseReason) {
        let _ = self.tx.send(Signal::Closed(id, reason as u32));
    }

    pub fn action_invoked(&self, id: u32, key: &str) {
        let _ = self.tx.send(Signal::Action(id, key.to_owned()));
    }
}

/// Asks for the name, then says when it's ours and when it's lost.
fn watch_name(conn: zbus::Connection, sink: Arc<dyn Fn(NoteEvent) + Send + Sync>) {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    let conn = zbus::blocking::Connection::from(conn);
    let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return };
    let (Ok(lost), Ok(acquired)) = (proxy.receive_name_lost(), proxy.receive_name_acquired()) else { return };
    let name = zbus::names::WellKnownName::from_static_str_unchecked(NAME);
    let flags = RequestNameFlags::AllowReplacement | RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue;
    match proxy.request_name(name.clone(), flags) {
        Ok(RequestNameReply::PrimaryOwner) | Ok(RequestNameReply::AlreadyOwner) => {}
        Ok(r) => log::warn!("{NAME}: another server keeps it ({r:?})"),
        Err(e) => log::warn!("{NAME}: {e}"),
    }
    {
        let sink = sink.clone();
        std::thread::spawn(move || {
            for s in acquired {
                if s.args().is_ok_and(|a| a.name() == NAME) {
                    sink(NoteEvent::NameAcquired);
                }
            }
        });
    }
    for s in lost {
        if s.args().is_ok_and(|a| a.name() == NAME) {
            sink(NoteEvent::NameLost);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_say_urgency_entry_and_flags() {
        let mut hints: HashMap<String, OwnedValue> = HashMap::new();
        hints.insert("urgency".into(), OwnedValue::from(2u8));
        hints.insert("desktop-entry".into(), Value::from("thunderbird").try_into().unwrap());
        hints.insert("resident".into(), OwnedValue::from(true));
        let n = incoming(
            "Mail".into(),
            0,
            "".into(),
            "Re: tiles".into(),
            "Half a pixel".into(),
            vec!["default".into(), "Open".into(), "reply".into()],
            &hints,
            -1,
        );
        assert_eq!(n.urgency, Urgency::Critical);
        assert_eq!(n.desktop_entry.as_deref(), Some("thunderbird"));
        assert!(n.resident && !n.transient);
        assert_eq!(n.actions, [("default".to_owned(), "Open".to_owned())], "an odd action out is dropped");
    }

    #[test]
    fn image_data_becomes_rgba() {
        let data: Vec<u8> = vec![10, 20, 30, 40, 50, 60, 0, 0];
        let st = zbus::zvariant::StructureBuilder::new()
            .add_field(2i32)
            .add_field(1i32)
            .add_field(8i32)
            .add_field(false)
            .add_field(8i32)
            .add_field(3i32)
            .add_field(data)
            .build()
            .unwrap();
        let v: OwnedValue = Value::from(st).try_into().unwrap();
        let Some(NoteImage::Pixels { width, height, rgba }) = image_data(&v) else { panic!("an image") };
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    }
}
