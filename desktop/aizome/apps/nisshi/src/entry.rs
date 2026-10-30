//! A message as Nisshi keeps it: the fields the table, the filters and the
//! suggestions use, read from one line of `journalctl -o json`, or one line
//! of a log file. The rest of a journal entry's fields are read when it's
//! selected, by its cursor.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

/// syslog's levels, most severe first, as journald keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Priority {
    Emergency = 0,
    Alert = 1,
    Critical = 2,
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl Priority {
    pub const ALL: [Priority; 8] = [
        Priority::Emergency,
        Priority::Alert,
        Priority::Critical,
        Priority::Error,
        Priority::Warning,
        Priority::Notice,
        Priority::Info,
        Priority::Debug,
    ];

    pub fn from_level(n: u8) -> Priority {
        Priority::ALL[usize::from(n.min(7))]
    }

    pub fn level(self) -> u8 {
        self as u8
    }

    pub fn word(self) -> &'static str {
        match self {
            Priority::Emergency => "Emergency",
            Priority::Alert => "Alert",
            Priority::Critical => "Critical",
            Priority::Error => "Error",
            Priority::Warning => "Warning",
            Priority::Notice => "Notice",
            Priority::Info => "Info",
            Priority::Debug => "Debug",
        }
    }

    /// journalctl's name for it (`-p err`).
    pub fn name(self) -> &'static str {
        match self {
            Priority::Emergency => "emerg",
            Priority::Alert => "alert",
            Priority::Critical => "crit",
            Priority::Error => "err",
            Priority::Warning => "warning",
            Priority::Notice => "notice",
            Priority::Info => "info",
            Priority::Debug => "debug",
        }
    }

    /// A level by its number, journalctl's name or a common synonym.
    pub fn parse(s: &str) -> Option<Priority> {
        let s = s.trim().to_ascii_lowercase();
        if let Ok(n) = s.parse::<u8>() {
            return (n <= 7).then(|| Priority::from_level(n));
        }
        Some(match s.as_str() {
            "emerg" | "emergency" | "panic" => Priority::Emergency,
            "alert" => Priority::Alert,
            "crit" | "critical" => Priority::Critical,
            "err" | "error" | "errors" => Priority::Error,
            "warning" | "warn" | "warnings" => Priority::Warning,
            "notice" => Priority::Notice,
            "info" => Priority::Info,
            "debug" => Priority::Debug,
            _ => return None,
        })
    }

    /// An error or worse.
    pub fn is_error(self) -> bool {
        self <= Priority::Error
    }

    /// A warning or worse: what Problems lists.
    pub fn is_problem(self) -> bool {
        self <= Priority::Warning
    }
}

/// How the message reached the journal (`_TRANSPORT`), or that it's a line
/// of a log file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Transport {
    Journal,
    Syslog,
    Stdout,
    Kernel,
    Audit,
    Driver,
    File,
    Other,
}

impl Transport {
    fn parse(s: &str) -> Transport {
        match s {
            "journal" => Transport::Journal,
            "syslog" => Transport::Syslog,
            "stdout" => Transport::Stdout,
            "kernel" => Transport::Kernel,
            "audit" => Transport::Audit,
            "driver" => Transport::Driver,
            _ => Transport::Other,
        }
    }
}

/// Where an entry is in the journal: what `journalctl --cursor` takes,
/// kept in parts (its sequence and boot are shared, so this is a third of
/// the text's size), or as the text where it doesn't take that form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cursor {
    Parts {
        seqnum_id: Arc<str>,
        seqnum: u64,
        boot: Arc<str>,
        mono: u64,
        realtime: u64,
        xor: u64,
    },
    Text(Box<str>),
    /// A log file's line: no cursor.
    None,
}

impl Cursor {
    pub fn parse(s: &str, intern: &mut Interner) -> Cursor {
        let mut parts = [None; 6];
        for part in s.split(';') {
            let Some((k, v)) = part.split_once('=') else { return Cursor::Text(s.into()) };
            let at = match k {
                "s" => 0,
                "i" => 1,
                "b" => 2,
                "m" => 3,
                "t" => 4,
                "x" => 5,
                _ => return Cursor::Text(s.into()),
            };
            parts[at] = Some(v);
        }
        let hex = |v: Option<&str>| v.and_then(|v| u64::from_str_radix(v, 16).ok());
        match (parts[0], hex(parts[1]), parts[2], hex(parts[3]), hex(parts[4]), hex(parts[5])) {
            (Some(s_), Some(i), Some(b), Some(m), Some(t), Some(x))
                if is_hex_id(s_) && is_hex_id(b) && format!("s={s_};i={i:x};b={b};m={m:x};t={t:x};x={x:x}") == s =>
            {
                Cursor::Parts {
                    seqnum_id: intern.get(s_),
                    seqnum: i,
                    boot: intern.get(b),
                    mono: m,
                    realtime: t,
                    xor: x,
                }
            }
            _ => Cursor::Text(s.into()),
        }
    }

    pub fn text(&self) -> Option<String> {
        match self {
            Cursor::Parts { seqnum_id, seqnum, boot, mono, realtime, xor } => {
                Some(format!("s={seqnum_id};i={seqnum:x};b={boot};m={mono:x};t={realtime:x};x={xor:x}"))
            }
            Cursor::Text(t) => Some(t.to_string()),
            Cursor::None => None,
        }
    }
}

fn is_hex_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Shared copies of the strings many entries repeat (sources, units,
/// boots), so each is kept once.
#[derive(Default)]
pub struct Interner(HashSet<Arc<str>>);

impl Interner {
    pub fn get(&mut self, s: &str) -> Arc<str> {
        if let Some(a) = self.0.get(s) {
            return a.clone();
        }
        let a: Arc<str> = s.into();
        self.0.insert(a.clone());
        a
    }
}

/// One message.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// When it was written: µs since the epoch (`__REALTIME_TIMESTAMP`).
    /// Zero for a file's line that doesn't say.
    pub time: i64,
    /// µs since its boot (`__MONOTONIC_TIMESTAMP`).
    pub mono: u64,
    pub priority: Priority,
    pub transport: Transport,
    /// `SYSLOG_FACILITY`.
    pub facility: Option<u8>,
    /// What wrote it, as it names itself: `SYSLOG_IDENTIFIER`, else the
    /// process's name, else its unit; "kernel" for the kernel, a file's name
    /// for its lines.
    pub source: Arc<str>,
    /// The process's unit: its user unit when it runs in a user's manager.
    pub unit: Option<Arc<str>>,
    /// The unit a message is about (systemd's `UNIT=`/`USER_UNIT=`: "Started
    /// …", "… failed").
    pub about: Option<Arc<str>>,
    pub pid: u32,
    pub uid: Option<u32>,
    pub message: Box<str>,
    /// The catalog's id for this kind of message (`MESSAGE_ID`).
    pub message_id: Option<Arc<str>>,
    /// The message's shape ([`crate::shape`]): repeats share it.
    pub shape: u64,
    pub boot: Option<Arc<str>>,
    pub cursor: Cursor,
    /// Its place among the entries read (stable while they're kept: what
    /// a selection follows).
    pub seq: u64,
    /// What it is, decided once as it's read ([`CRASH`], [`SECURITY`],
    /// [`WRITTEN_LEVEL`]).
    pub flags: u8,
    /// The message's terminal styles, where it had any.
    pub styles: Option<Box<[crate::ansi::Run]>>,
    /// The launch it came from, where the desktop gave that launch an
    /// identifier of its own (`aizome-launch-…`: [`crate::launch`]).
    pub launch: Option<Arc<str>>,
}

/// A crash report: a core dump, a segfault, a panic ([`crate::filter::is_crash`]).
pub const CRASH: u8 = 1;
/// Logins and authentication ([`crate::filter::is_security`]).
pub const SECURITY: u8 = 2;
/// Its level was read from its text ([`crate::ansi::written_level`]):
/// journald had the one it files a service's output at.
pub const WRITTEN_LEVEL: u8 = 4;

impl Entry {
    /// The entry as it's kept: its message's plain text and styles
    /// ([`crate::ansi`]), the level written in it where journald filed a
    /// service's output at Info, its shape and its [`flags`](Entry::flags).
    pub fn finish(mut self) -> Entry {
        if let Some((plain, mut runs)) = crate::ansi::parse(&self.message) {
            let plain = plain.trim_end();
            let len = plain.len() as u32;
            runs.retain_mut(|r| {
                r.end = r.end.min(len);
                r.start < r.end
            });
            self.message = plain.into();
            self.styles = (!runs.is_empty()).then(|| runs.into_boxed_slice());
        }
        self.flags = 0;
        if self.priority == Priority::Info
            && self.transport == Transport::Stdout
            && let Some(p) = crate::ansi::written_level(&self.message)
            && p != Priority::Info
        {
            self.priority = p;
            self.flags |= WRITTEN_LEVEL;
        }
        self.shape = crate::shape::fingerprint(&self.message);
        if crate::filter::is_crash(&self) {
            self.flags |= CRASH;
        }
        if crate::filter::is_security(&self) {
            self.flags |= SECURITY;
        }
        self
    }

    /// The message's first line, and how many more it has.
    pub fn first_line(&self) -> (&str, usize) {
        let mut lines = self.message.lines();
        let first = lines.next().unwrap_or("");
        (first, lines.filter(|l| !l.trim().is_empty()).count())
    }

    /// From the user's own journal or processes.
    pub fn is_mine(&self, uid: u32) -> bool {
        self.uid == Some(uid)
    }

    /// The unit to show for it: the one it's about, else its own.
    pub fn unit_shown(&self) -> Option<&Arc<str>> {
        self.about.as_ref().or(self.unit.as_ref())
    }
}

/// What journalctl's JSON gives a field: a string, bytes (a message that
/// isn't valid UTF-8 or has control characters), several values (the
/// first is kept), or null (too large without `--all`). Read by hand
/// rather than as an untagged enum, which would copy every value first.
enum Field<'a> {
    Text(Cow<'a, str>),
    Null,
}

impl<'de: 'a, 'a> Deserialize<'de> for Field<'a> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Field<'de>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a journal field")
            }
            fn visit_borrowed_str<E>(self, s: &'de str) -> Result<Field<'de>, E> {
                Ok(Field::Text(Cow::Borrowed(s)))
            }
            fn visit_str<E>(self, s: &str) -> Result<Field<'de>, E> {
                Ok(Field::Text(Cow::Owned(s.to_owned())))
            }
            fn visit_string<E>(self, s: String) -> Result<Field<'de>, E> {
                Ok(Field::Text(Cow::Owned(s)))
            }
            fn visit_unit<E>(self) -> Result<Field<'de>, E> {
                Ok(Field::Null)
            }
            fn visit_none<E>(self) -> Result<Field<'de>, E> {
                Ok(Field::Null)
            }
            fn visit_u64<E>(self, n: u64) -> Result<Field<'de>, E> {
                Ok(Field::Text(Cow::Owned(n.to_string())))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Field<'de>, A::Error> {
                let values: Vec<Value> =
                    std::iter::from_fn(|| seq.next_element::<Value>().transpose()).collect::<Result<_, _>>()?;
                let bytes = |v: &[Value]| -> Option<String> {
                    let b: Option<Vec<u8>> = v.iter().map(|n| n.as_u64().and_then(|n| u8::try_from(n).ok())).collect();
                    b.map(|b| String::from_utf8_lossy(&b).into_owned())
                };
                // Numbers are a value's bytes; strings or arrays, several
                // values.
                if values.iter().all(Value::is_number) {
                    return Ok(bytes(&values).map_or(Field::Null, |s| Field::Text(Cow::Owned(s))));
                }
                Ok(values
                    .iter()
                    .find_map(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        Value::Array(b) => bytes(b),
                        _ => None,
                    })
                    .map_or(Field::Null, |s| Field::Text(Cow::Owned(s))))
            }
        }
        d.deserialize_any(V)
    }
}

impl Field<'_> {
    fn text(&self) -> Option<Cow<'_, str>> {
        match self {
            Field::Text(t) => Some(Cow::Borrowed(t.as_ref())),
            Field::Null => None,
        }
    }
}

/// The fields Nisshi asks journalctl for (`--output-fields`): it adds the
/// cursor, the timestamps and the boot itself.
pub const FIELDS: &[&str] = &[
    "MESSAGE",
    "PRIORITY",
    "SYSLOG_IDENTIFIER",
    "SYSLOG_FACILITY",
    "_COMM",
    "_PID",
    "_UID",
    "_TRANSPORT",
    "_SYSTEMD_UNIT",
    "_SYSTEMD_USER_UNIT",
    "UNIT",
    "USER_UNIT",
    "MESSAGE_ID",
];

#[derive(Deserialize)]
struct Raw<'a> {
    #[serde(rename = "__CURSOR", borrow, default)]
    cursor: Option<Field<'a>>,
    #[serde(rename = "__REALTIME_TIMESTAMP", borrow, default)]
    realtime: Option<Field<'a>>,
    #[serde(rename = "__MONOTONIC_TIMESTAMP", borrow, default)]
    mono: Option<Field<'a>>,
    #[serde(rename = "_BOOT_ID", borrow, default)]
    boot: Option<Field<'a>>,
    #[serde(rename = "MESSAGE", borrow, default)]
    message: Option<Field<'a>>,
    #[serde(rename = "PRIORITY", borrow, default)]
    priority: Option<Field<'a>>,
    #[serde(rename = "SYSLOG_IDENTIFIER", borrow, default)]
    identifier: Option<Field<'a>>,
    #[serde(rename = "SYSLOG_FACILITY", borrow, default)]
    facility: Option<Field<'a>>,
    #[serde(rename = "_COMM", borrow, default)]
    comm: Option<Field<'a>>,
    #[serde(rename = "_PID", borrow, default)]
    pid: Option<Field<'a>>,
    #[serde(rename = "_UID", borrow, default)]
    uid: Option<Field<'a>>,
    #[serde(rename = "_TRANSPORT", borrow, default)]
    transport: Option<Field<'a>>,
    #[serde(rename = "_SYSTEMD_UNIT", borrow, default)]
    unit: Option<Field<'a>>,
    #[serde(rename = "_SYSTEMD_USER_UNIT", borrow, default)]
    user_unit: Option<Field<'a>>,
    #[serde(rename = "UNIT", borrow, default)]
    about: Option<Field<'a>>,
    #[serde(rename = "USER_UNIT", borrow, default)]
    about_user: Option<Field<'a>>,
    #[serde(rename = "MESSAGE_ID", borrow, default)]
    message_id: Option<Field<'a>>,
}

fn text<'a>(f: &'a Option<Field<'a>>) -> Option<Cow<'a, str>> {
    f.as_ref().and_then(Field::text)
}

fn number<T: std::str::FromStr>(f: &Option<Field<'_>>) -> Option<T> {
    text(f).and_then(|t| t.trim().parse().ok())
}

/// Read one line of `journalctl -o json`.
pub fn parse(line: &str, intern: &mut Interner) -> Option<Entry> {
    let raw: Raw = serde_json::from_str(line).ok()?;
    let message = text(&raw.message).map(|m| m.trim_end().to_owned()).unwrap_or_default();
    let transport = text(&raw.transport).map_or(Transport::Other, |t| Transport::parse(&t));
    let unit = text(&raw.unit);
    let user_unit = text(&raw.user_unit);
    // A user's manager runs their units in user@UID.service: theirs is the
    // unit that says something.
    let own_unit = match (&unit, &user_unit) {
        (Some(u), Some(uu)) if u.starts_with("user@") => Some(uu.clone()),
        (Some(u), _) => Some(u.clone()),
        (None, uu) => uu.clone(),
    };
    let identifier = text(&raw.identifier).filter(|s| !s.is_empty());
    // An app a desktop launched: named after the app its scope is for,
    // whether its output came as the app's ID or as that launch's own.
    let app = own_unit.as_deref().and_then(crate::launch::app_of_unit);
    let launch = identifier.as_deref().filter(|i| crate::launch::is_launch_id(i)).map(|i| intern.get(i));
    let source = if launch.is_some() {
        let comm = text(&raw.comm).filter(|c| !crate::launch::is_runner(c));
        let named = app.as_deref().map(crate::launch::app_name).or_else(|| comm.map(Cow::into_owned));
        Cow::Owned(named.unwrap_or_else(|| "Launched app".into()))
    } else if let (Some(id), Some(app)) = (&identifier, &app)
        && id.as_ref() == app.as_str()
    {
        Cow::Owned(crate::launch::app_name(app))
    } else {
        identifier
            .or_else(|| (transport == Transport::Kernel).then_some(Cow::Borrowed("kernel")))
            .or_else(|| text(&raw.comm))
            .or_else(|| own_unit.clone())
            .unwrap_or(Cow::Borrowed("journal"))
    };
    let about = text(&raw.about_user).or_else(|| text(&raw.about));
    Some(
        Entry {
            time: number(&raw.realtime).unwrap_or(0),
            mono: number(&raw.mono).unwrap_or(0),
            priority: number::<u8>(&raw.priority).map_or(Priority::Info, Priority::from_level),
            transport,
            facility: number(&raw.facility),
            source: intern.get(&source),
            unit: own_unit.map(|u| intern.get(&u)),
            about: about.map(|u| intern.get(&u)),
            pid: number(&raw.pid).unwrap_or(0),
            uid: number(&raw.uid),
            shape: crate::shape::fingerprint(&message),
            message: message.into_boxed_str(),
            message_id: text(&raw.message_id).map(|m| intern.get(&m)),
            boot: text(&raw.boot).map(|b| intern.get(&b)),
            cursor: text(&raw.cursor).map_or(Cursor::None, |c| Cursor::parse(&c, intern)),
            seq: 0,
            flags: 0,
            styles: None,
            launch,
        }
        .finish(),
    )
}

/// A log file's line as a message: its time where it starts with one,
/// its level guessed from its words.
pub fn from_line(line: &str, source: &Arc<str>, prev_time: i64, clock: &crate::time::Clock) -> Entry {
    // Its colours first, so that a coloured timestamp is still read.
    let (plain, runs) = crate::ansi::parse(line).unwrap_or_else(|| (line.to_owned(), Vec::new()));
    let (time, rest) = clock.leading(&plain).unwrap_or((prev_time, &plain));
    let message = rest.trim_end();
    let cut = (plain.len() - rest.len()) as u32;
    let runs: Vec<crate::ansi::Run> = runs
        .into_iter()
        .filter(|r| r.end > cut)
        .map(|r| crate::ansi::Run {
            start: r.start.saturating_sub(cut),
            end: (r.end - cut).min(message.len() as u32),
            ..r
        })
        .filter(|r| r.start < r.end)
        .collect();
    Entry {
        time,
        mono: 0,
        priority: guess_priority(message),
        transport: Transport::File,
        facility: None,
        source: source.clone(),
        unit: None,
        about: None,
        pid: 0,
        uid: None,
        shape: 0,
        message: message.into(),
        message_id: None,
        boot: None,
        cursor: Cursor::None,
        seq: 0,
        flags: 0,
        styles: (!runs.is_empty()).then(|| runs.into_boxed_slice()),
        launch: None,
    }
    .finish()
}

/// A file's line's level, from the words logs use: Xorg's (EE) and (WW),
/// "error", "warning", "fatal".
pub fn guess_priority(line: &str) -> Priority {
    let head: String = line.chars().take(160).collect::<String>().to_ascii_lowercase();
    let word = |w: &str| {
        head.match_indices(w).any(|(at, _)| {
            let before = head[..at].chars().next_back().is_none_or(|c| !c.is_ascii_alphanumeric());
            let after = head[at + w.len()..].chars().next().is_none_or(|c| !c.is_ascii_alphabetic());
            before && after
        })
    };
    if word("fatal") || word("panic") || word("critical") {
        Priority::Critical
    } else if head.contains("(ee)") || word("error") || word("err") || word("failed") || word("failure") {
        Priority::Error
    } else if head.contains("(ww)") || word("warning") || word("warn") {
        Priority::Warning
    } else if head.contains("(ii)") || head.contains("(--)") || head.contains("(**)") {
        Priority::Info
    } else if word("debug") || head.contains("(dd)") {
        Priority::Debug
    } else {
        Priority::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"_SYSTEMD_UNIT":"user@1000.service","_SYSTEMD_USER_UNIT":"dbus-:1.2-org.telegram.desktop@0.service","_PID":"4142","__CURSOR":"s=9f3a82e3f878453f8db379ec9f7c7a99;i=cc214;b=573e76de00ca44adbdcb30ca43863c22;m=23324c31;t=65f0107ee9487;x=809efd67e459af7e","_COMM":".Telegram-wrapp","_UID":"1000","_TRANSPORT":"journal","SYSLOG_IDENTIFIER":"Telegram","PRIORITY":"4","__MONOTONIC_TIMESTAMP":"590498865","MESSAGE":"Trying to enable 0x5c33391b8350 but 0x5c33391b8350 is already enabled","__REALTIME_TIMESTAMP":"1793307892946055","_BOOT_ID":"573e76de00ca44adbdcb30ca43863c22"}"#;

    #[test]
    fn a_journal_line() {
        let mut i = Interner::default();
        let e = parse(LINE, &mut i).unwrap();
        assert_eq!(&*e.source, "Telegram");
        assert_eq!(e.unit.as_deref(), Some("dbus-:1.2-org.telegram.desktop@0.service"));
        assert_eq!(e.priority, Priority::Warning);
        assert_eq!((e.pid, e.uid), (4142, Some(1000)));
        assert_eq!(e.time, 1793307892946055);
        assert_eq!(e.transport, Transport::Journal);
        // The cursor is kept in parts and given back as journalctl wrote it.
        assert!(matches!(e.cursor, Cursor::Parts { seqnum: 0xcc214, .. }));
        assert_eq!(
            e.cursor.text().as_deref(),
            Some(
                "s=9f3a82e3f878453f8db379ec9f7c7a99;i=cc214;b=573e76de00ca44adbdcb30ca43863c22;m=23324c31;t=65f0107ee9487;x=809efd67e459af7e"
            )
        );
    }

    #[test]
    fn bytes_several_values_and_the_kernel() {
        let mut i = Interner::default();
        // "héllo\x07" as journald gives a message with a control character.
        let e = parse(
            r#"{"__CURSOR":"weird","__REALTIME_TIMESTAMP":"5","MESSAGE":[104,195,169,108,108,111,7],"_TRANSPORT":"kernel","PRIORITY":"3"}"#,
            &mut i,
        )
        .unwrap();
        // Bytes, as UTF-8; the bell, a control character, left out.
        assert_eq!(&*e.message, "héllo");
        assert_eq!(&*e.source, "kernel");
        assert_eq!(e.cursor, Cursor::Text("weird".into()));
        assert!(e.priority.is_error());
        let e = parse(r#"{"MESSAGE":["one","two"],"SYSLOG_IDENTIFIER":"x","MESSAGE_ID":null}"#, &mut i).unwrap();
        assert_eq!(&*e.message, "one");
        assert_eq!(e.message_id, None);
        assert_eq!(e.priority, Priority::Info);
    }

    #[test]
    fn colours_and_the_level_a_service_wrote() {
        let mut i = Interner::default();
        // What Aizome's compositor writes to stdout, which journald files
        // at Info: ESC is a control character, so JSON gives its bytes.
        let raw = "\u{1b}[2m2026-10-29T21:04:01Z\u{1b}[0m \u{1b}[33m WARN\u{1b}[0m aizome: no EDID";
        let bytes: Vec<String> = raw.bytes().map(|b| b.to_string()).collect();
        let line = format!(
            r#"{{"MESSAGE":[{}],"PRIORITY":"6","_TRANSPORT":"stdout","SYSLOG_IDENTIFIER":"env"}}"#,
            bytes.join(",")
        );
        let e = parse(&line, &mut i).unwrap();
        assert_eq!(&*e.message, "2026-10-29T21:04:01Z  WARN aizome: no EDID");
        assert_eq!(e.priority, Priority::Warning);
        assert!(e.flags & WRITTEN_LEVEL != 0);
        let runs = e.styles.as_deref().unwrap();
        assert_eq!(runs[1].style.hue, Some(crate::ansi::Hue::Yellow));
        assert_eq!(e.shape, crate::shape::fingerprint(&e.message));
        // A level journald was told isn't second-guessed.
        let e = parse(r#"{"MESSAGE":"ERROR: but notice","PRIORITY":"5","_TRANSPORT":"stdout"}"#, &mut i).unwrap();
        assert_eq!(e.priority, Priority::Notice);
        // A file's line with a coloured timestamp.
        let c = crate::time::Clock::fixed("UTC", 0);
        let e = from_line("\u{1b}[2m2026-10-29T16:05:00Z\u{1b}[0m \u{1b}[31mfailed\u{1b}[0m", &"app.log".into(), 0, &c);
        assert_eq!(&*e.message, "failed");
        assert_eq!(e.time, 1_793_289_900_000_000);
        assert_eq!(
            e.styles.as_deref().unwrap()[0],
            crate::ansi::Run {
                start: 0,
                end: 6,
                style: crate::ansi::Style { hue: Some(crate::ansi::Hue::Red), ..Default::default() }
            }
        );
    }

    #[test]
    fn launched_apps_are_named_for_the_app() {
        let mut i = Interner::default();
        // A launch the shell watched (its own identifier), in Koe's scope.
        let e = parse(
            r#"{"MESSAGE":"koe: daemon did not become ready","SYSLOG_IDENTIFIER":"aizome-launch-f332d86ed3fac3deb4589d0998c3a08e","_COMM":"aizome-koe","_SYSTEMD_UNIT":"user@1000.service","_SYSTEMD_USER_UNIT":"app-aizome-org.aizome.Koe-0123abcd.scope","_TRANSPORT":"stdout"}"#,
            &mut i,
        )
        .unwrap();
        assert_eq!(&*e.source, "Koe");
        assert_eq!(e.launch.as_deref(), Some("aizome-launch-f332d86ed3fac3deb4589d0998c3a08e"));
        // Without a scope, the process's name; a shell's says nothing.
        let e = parse(r#"{"MESSAGE":"x","SYSLOG_IDENTIFIER":"aizome-launch-0123456789abcdef","_COMM":"sh"}"#, &mut i)
            .unwrap();
        assert_eq!(&*e.source, "Launched app");
        // The launcher as it is today: the app's ID, credited to `cat`.
        let e = parse(
            r#"{"MESSAGE":"x","SYSLOG_IDENTIFIER":"org.example.Viewer","_COMM":"cat","_SYSTEMD_USER_UNIT":"app-aizome-org.example.Viewer-72395e0ce4c25ac6.scope"}"#,
            &mut i,
        )
        .unwrap();
        assert_eq!(&*e.source, "Viewer");
        assert_eq!(e.launch, None);
        // Anything else keeps its own identifier.
        let e = parse(r#"{"MESSAGE":"x","SYSLOG_IDENTIFIER":"Telegram","_SYSTEMD_USER_UNIT":"app-aizome-org.telegram.desktop-ab12.scope"}"#, &mut i).unwrap();
        assert_eq!(&*e.source, "Telegram");
    }

    #[test]
    fn levels_by_name() {
        assert_eq!(Priority::parse("warn"), Some(Priority::Warning));
        assert_eq!(Priority::parse("3"), Some(Priority::Error));
        assert_eq!(Priority::parse("8"), None);
        assert_eq!(Priority::parse("loud"), None);
    }

    #[test]
    fn a_files_lines_guess_their_level() {
        assert_eq!(guess_priority("[    38.211] (EE) Failed to load module \"nv\""), Priority::Error);
        assert_eq!(guess_priority("Kernel panic - not syncing"), Priority::Critical);
        assert_eq!(guess_priority("[    38.211] (WW) Disabling Keyboard0"), Priority::Warning);
        assert_eq!(guess_priority("ERROR: could not open"), Priority::Error);
        assert_eq!(guess_priority("Terrors of the deep"), Priority::Info);
        assert_eq!(guess_priority("[  OK  ] Started Login Service."), Priority::Info);
    }
}

/// A hasher for keys that are already well spread (pointers, counts):
/// a multiply and a rotate, rather than SipHash's rounds, for the maps a
/// view fills once per message.
#[derive(Default, Clone, Copy)]
pub struct Quick(u64);

impl std::hash::Hasher for Quick {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(u64::from(*b));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }

    fn write_u8(&mut self, n: u8) {
        self.write_u64(u64::from(n));
    }
}

pub type QuickMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<Quick>>;

/// An interned name's address: what [`QuickMap`]s key names by.
pub fn address(name: &Arc<str>) -> usize {
    Arc::as_ptr(name) as *const u8 as usize
}
