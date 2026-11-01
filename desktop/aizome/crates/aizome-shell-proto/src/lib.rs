//! The protocol on Aizome's shell socket (`$AIZOME_SHELL_SOCKET`), by
//! which apps publish their menus to the global menu bar: newline-delimited
//! JSON, one message per line, each an object with a `"type"`.
//!
//! An app connects, says [`AppMessage::Hello`], then sends its menus
//! ([`AppMessage::Menus`]) and sends them again whenever they change. The
//! shell answers with [`ShellMessage::Activate`] when someone picks an
//! item (from the bar, a menu at the pointer, a torn-off palette or the
//! command palette), [`ShellMessage::AboutToShow`] before it shows a menu
//! (so the app can bring its items' states up to date), and
//! [`ShellMessage::ServicesText`] to ask for the selected text a service
//! would act on. See `docs/shell-protocol.md`.
//!
//! Items have numeric ids that stay the same while the item does (the kit
//! derives them from the item's role, else its place and label), so the
//! shell can keep a torn-off palette's items across updates.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What an app sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AppMessage {
    /// First, once: who the app is. The shell keys its menus by `pid`,
    /// which is how it knows them for the focused window.
    Hello { app_id: String, pid: u32, name: String },
    /// The app's menus, whole: the first time and whenever they change.
    Menus { menus: Vec<Menu> },
    /// The answer to [`ShellMessage::ServicesText`]: the focused widget's
    /// selected text, if there is any.
    ServicesText {
        serial: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    /// Not from an app: the compositor or `aizomectl` asks the shell to do
    /// something. Needs no hello first; answered with
    /// [`ShellMessage::Reply`]. `{"type":"request","request":"palette"}`.
    Request {
        #[serde(flatten)]
        request: ShellRequest,
    },
}

/// What the compositor and `aizomectl` ask of the shell itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum ShellRequest {
    /// Open the command palette (or close it if open), in a scope
    /// (`commands`, `windows`, `files`, `apps`, `settings`), with text.
    Palette {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        query: String,
    },
    /// Open the Toolchest.
    Toolchest,
    /// Open the focused app's menu at a point of the global layout.
    AppMenu { x: f64, y: f64 },
    /// Lock the screen.
    Lock,
    /// Show the on-screen display: `kind` is `volume`, `mute`,
    /// `brightness` or `keyboard`, `value` 0.0–1.0.
    Osd { kind: String, value: f64 },
    /// Post a notification as an app would.
    Notify {
        #[serde(default)]
        app: String,
        summary: String,
        #[serde(default)]
        body: String,
    },
    /// Fold the Deskbar into the bar, or unfold it (toggle when left out).
    Deskbar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        collapsed: Option<bool>,
    },
    /// Open a systray widget's panel (`network`, `sound`, `bluetooth`,
    /// `display`, `battery`, `clock`), or a menu of the bar by its title.
    Open { what: String },
    /// Close whatever transient surface is open (menus, panels, palette).
    Dismiss,
    /// What the shell shows now, as JSON in the reply's message.
    State,
    /// The menu bar takes the keyboard (F10, ❖⌥M): the focused app's menu
    /// opens with its first row lit, and ← → go across the titles.
    MenuBar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// The system bell rang (xdg-system-bell): the screen's edge flashes
    /// where `[sound] visual_alert` asks for it.
    Bell {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// Start a screen recording, as the palette's Record Screen and Record
    /// Area do: `target` is `screen` (the focused display; the default),
    /// `area` (the person picks) or a display's name. Audio left out
    /// follows `[recording]`. Refused while one is going.
    Record {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        desktop_audio: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        microphone: Option<bool>,
    },
    /// Stop the recording going (its file is finished and announced).
    StopRecording,
}

/// What the shell sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ShellMessage {
    /// Item `id` was chosen.
    Activate { id: u32 },
    /// Menu `menu` (a top-level menu's or a submenu's id) is about to show:
    /// send menus again if any state changed.
    AboutToShow { menu: u32 },
    /// The selected text, please, for a service: answered with
    /// [`AppMessage::ServicesText`] and the same serial.
    ServicesText { serial: u64 },
    /// The answer to an [`AppMessage::Request`]: whether it was done, and
    /// what to say (JSON for [`ShellRequest::State`]).
    Reply {
        ok: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        message: String,
    },
}

/// Ask the shell at `path` to do something and wait for its answer: what
/// `aizomectl` and tests use.
pub fn request(path: impl AsRef<Path>, request: ShellRequest) -> io::Result<(bool, String)> {
    let mut c = Client::connect(path)?;
    c.send(&AppMessage::Request { request })?;
    loop {
        match c.recv()? {
            Some(ShellMessage::Reply { ok, message }) => return Ok((ok, message)),
            Some(_) => continue,
            None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the shell hung up")),
        }
    }
}

/// A menu of the bar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Menu {
    pub id: u32,
    pub title: String,
    pub items: Vec<Item>,
}

/// What an item is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    #[default]
    Action,
    Separator,
    Submenu,
    /// A section's name: text that does nothing.
    Header,
}

/// An item of a menu.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    #[serde(default)]
    pub kind: ItemKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<Shortcut>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub checked: bool,
    /// What the item is to the system (`"copy"`, `"quit"`, `"minimise"`):
    /// the shell may act on some itself, and the palette shows them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// A Lucide icon's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A submenu's items.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Item>,
}

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A key and its modifiers, as data: ❖ is `command` (Super).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shortcut {
    /// The key: a character (`"N"`, `","`) or a name (`"Enter"`, `"Tab"`,
    /// `"Space"`, `"Escape"`, `"Backspace"`, `"Delete"`, `"Insert"`, `"Up"`,
    /// `"Down"`, `"Left"`, `"Right"`, `"Home"`, `"End"`, `"PageUp"`,
    /// `"PageDown"`, `"F1"`…).
    pub key: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub command: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shift: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub alt: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ctrl: bool,
}

impl Shortcut {
    /// As menus print it: ⌃ ⌥ ⇧ ❖ then the key.
    pub fn label(&self) -> String {
        let mut s = String::new();
        for (on, glyph) in [(self.ctrl, "⌃"), (self.alt, "⌥"), (self.shift, "⇧"), (self.command, "❖")] {
            if on {
                s.push_str(glyph);
            }
        }
        s.push_str(&self.key);
        s
    }
}

impl Menu {
    /// The item with `id`, here or in a submenu.
    pub fn find(&self, id: u32) -> Option<&Item> {
        fn walk(items: &[Item], id: u32) -> Option<&Item> {
            items.iter().find_map(|i| if i.id == id { Some(i) } else { walk(&i.items, id) })
        }
        walk(&self.items, id)
    }
}

/// One message as a line of JSON, newline included.
pub fn encode<T: Serialize>(msg: &T) -> String {
    let mut s = serde_json::to_string(msg).expect("protocol messages serialize");
    s.push('\n');
    s
}

/// A line's message.
pub fn decode<'a, T: Deserialize<'a>>(line: &'a str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim_end())
}

/// Where the shell listens: `$AIZOME_SHELL_SOCKET`, if set.
pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("AIZOME_SHELL_SOCKET").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// An app's end of the connection, blocking.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    pub fn connect(path: impl AsRef<Path>) -> io::Result<Client> {
        let stream = UnixStream::connect(path)?;
        let writer = stream.try_clone()?;
        Ok(Client { reader: BufReader::new(stream), writer })
    }

    /// Connect and say hello.
    pub fn hello(path: impl AsRef<Path>, app_id: &str, name: &str) -> io::Result<Client> {
        let mut c = Client::connect(path)?;
        c.send(&AppMessage::Hello { app_id: app_id.into(), pid: std::process::id(), name: name.into() })?;
        Ok(c)
    }

    pub fn send(&mut self, msg: &AppMessage) -> io::Result<()> {
        self.writer.write_all(encode(msg).as_bytes())?;
        self.writer.flush()
    }

    /// The next message, waiting for it; `None` once the shell hung up.
    /// Lines that aren't messages this side knows are skipped.
    pub fn recv(&mut self) -> io::Result<Option<ShellMessage>> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(m) = decode(&line) {
                return Ok(Some(m));
            }
        }
    }

    /// A second handle for sending from another thread than the one
    /// receiving.
    pub fn sender(&self) -> io::Result<Sender> {
        Ok(Sender(self.writer.try_clone()?))
    }
}

/// Sends to the shell (from any thread).
pub struct Sender(UnixStream);

impl Sender {
    pub fn send(&mut self, msg: &AppMessage) -> io::Result<()> {
        self.0.write_all(encode(msg).as_bytes())?;
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_tagged_kebab_case_lines() {
        let hello = AppMessage::Hello { app_id: "org.aizome.Soroban".into(), pid: 7, name: "Soroban".into() };
        let line = encode(&hello);
        assert_eq!(line, "{\"type\":\"hello\",\"app_id\":\"org.aizome.Soroban\",\"pid\":7,\"name\":\"Soroban\"}\n");
        assert_eq!(decode::<AppMessage>(&line).unwrap(), hello);
        assert_eq!(
            decode::<ShellMessage>("{\"type\":\"about-to-show\",\"menu\":3}").unwrap(),
            ShellMessage::AboutToShow { menu: 3 }
        );
        assert_eq!(
            decode::<ShellMessage>("{\"type\":\"activate\",\"id\":12}\n").unwrap(),
            ShellMessage::Activate { id: 12 }
        );
    }

    #[test]
    fn requests_and_replies_are_flat_lines() {
        let r = AppMessage::Request {
            request: ShellRequest::Palette { scope: Some("files".into()), query: String::new() },
        };
        let line = encode(&r);
        assert_eq!(line, "{\"type\":\"request\",\"request\":\"palette\",\"scope\":\"files\"}\n");
        assert_eq!(decode::<AppMessage>(&line).unwrap(), r);
        let lock: AppMessage = decode(r#"{"type":"request","request":"lock"}"#).unwrap();
        assert_eq!(lock, AppMessage::Request { request: ShellRequest::Lock });
        let osd: AppMessage = decode(r#"{"type":"request","request":"osd","kind":"volume","value":0.5}"#).unwrap();
        assert_eq!(osd, AppMessage::Request { request: ShellRequest::Osd { kind: "volume".into(), value: 0.5 } });
        let rec: ShellRequest =
            serde_json::from_str(r#"{"request":"record","target":"area","microphone":true}"#).unwrap();
        assert_eq!(
            rec,
            ShellRequest::Record { target: Some("area".into()), desktop_audio: None, microphone: Some(true) }
        );
        let stop: ShellRequest = serde_json::from_str(r#"{"request":"stop-recording"}"#).unwrap();
        assert_eq!(stop, ShellRequest::StopRecording);
        let reply = ShellMessage::Reply { ok: true, message: String::new() };
        assert_eq!(encode(&reply), "{\"type\":\"reply\",\"ok\":true}\n");
        // An app built before replies existed skips them.
        assert!(decode::<AppMessage>("{\"type\":\"what\"}").is_err());
    }

    #[test]
    fn items_leave_out_defaults() {
        let item = Item {
            id: 4,
            label: "Copy".into(),
            shortcut: Some(Shortcut { key: "C".into(), command: true, ..Default::default() }),
            enabled: true,
            role: Some("copy".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(
            json,
            r#"{"id":4,"kind":"action","label":"Copy","shortcut":{"key":"C","command":true},"enabled":true,"role":"copy"}"#
        );
        let back: Item = serde_json::from_str(r#"{"id":4,"label":"Copy"}"#).unwrap();
        assert!(back.enabled && !back.checked && back.kind == ItemKind::Action);
        assert_eq!(item.shortcut.unwrap().label(), "❖C");
    }

    #[test]
    fn a_client_talks_over_a_socket() {
        let dir = std::env::temp_dir().join(format!("aizome-shell-proto-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("shell.sock");
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let shell = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut lines = BufReader::new(stream.try_clone().unwrap()).lines();
            let hello: AppMessage = decode(&lines.next().unwrap().unwrap()).unwrap();
            let menus: AppMessage = decode(&lines.next().unwrap().unwrap()).unwrap();
            let mut w = stream;
            w.write_all(encode(&ShellMessage::Activate { id: 9 }).as_bytes()).unwrap();
            (hello, menus)
        });
        let mut c = Client::hello(&path, "org.aizome.Test", "Test").unwrap();
        let menu = Menu {
            id: 1,
            title: "File".into(),
            items: vec![Item { id: 9, label: "New".into(), enabled: true, ..Default::default() }],
        };
        c.send(&AppMessage::Menus { menus: vec![menu.clone()] }).unwrap();
        assert_eq!(c.recv().unwrap(), Some(ShellMessage::Activate { id: 9 }));
        let (hello, menus) = shell.join().unwrap();
        assert!(matches!(hello, AppMessage::Hello { pid, .. } if pid == std::process::id()));
        assert_eq!(menus, AppMessage::Menus { menus: vec![menu] });
        assert_eq!(c.recv().unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
