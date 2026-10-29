//! What's shown: the sidebar's scope (all messages, problems, the kernel's,
//! crashes…), the chips under the toolbar (one source, one kind of message
//! hidden, a stretch of time), the level, and what's typed in search.

use std::sync::Arc;

use aizome_kit::Icon;
use regex::Regex;

use crate::entry::{Entry, Priority, Transport};
use crate::time::Clock;

/// The catalog's id of systemd-coredump's "dumped core".
pub const COREDUMP: &str = "fc2e22bc6ee647b6b90729ab34a250b1";
/// systemd's "unit failed" (`UNIT_RESULT`).
pub const UNIT_FAILED: &str = "be02cf6855d2428ba40df7e9d022f03d";

/// The sidebar's places.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    All,
    /// Warnings and worse.
    Problems,
    /// The user's own: their session's units and processes.
    Mine,
    Kernel,
    /// Core dumps, segfaults, panics.
    Crashes,
    /// Logins, sudo, polkit, ssh: who did what as whom.
    Security,
    /// A log file under /var/log (its path).
    File(String),
}

impl Scope {
    pub const JOURNAL: [Scope; 6] =
        [Scope::All, Scope::Problems, Scope::Mine, Scope::Kernel, Scope::Crashes, Scope::Security];

    pub fn title(&self) -> String {
        match self {
            Scope::All => "All Messages".into(),
            Scope::Problems => "Errors & Warnings".into(),
            Scope::Mine => "My Session".into(),
            Scope::Kernel => "Kernel".into(),
            Scope::Crashes => "Crashes".into(),
            Scope::Security => "Logins & Security".into(),
            Scope::File(p) => {
                std::path::Path::new(p).file_name().map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned())
            }
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            Scope::All => Icon::NotepadText,
            Scope::Problems => Icon::TriangleAlert,
            Scope::Mine => Icon::User,
            Scope::Kernel => Icon::Cpu,
            Scope::Crashes => Icon::Bug,
            Scope::Security => Icon::Shield,
            Scope::File(_) => Icon::FileText,
        }
    }

    /// What it shows, for its tooltip and the empty table.
    pub fn about(&self) -> &'static str {
        match self {
            Scope::All => "Every message of the boot",
            Scope::Problems => "Warnings, errors and worse",
            Scope::Mine => "Your session's apps and services",
            Scope::Kernel => "The kernel's messages: hardware, drivers, file systems",
            Scope::Crashes => "Programs that crashed: core dumps, segfaults, panics",
            Scope::Security => "Logins, administrator commands and authentication",
            Scope::File(_) => "A log file",
        }
    }

    pub fn matches(&self, e: &Entry, uid: u32) -> bool {
        match self {
            Scope::All | Scope::File(_) => true,
            Scope::Problems => e.priority.is_problem(),
            Scope::Mine => e.is_mine(uid),
            Scope::Kernel => e.transport == Transport::Kernel,
            Scope::Crashes => e.flags & crate::entry::CRASH != 0,
            Scope::Security => e.flags & crate::entry::SECURITY != 0,
        }
    }
}

/// A program's crash: a core dump, the kernel's report of a segfault or
/// trap, a Rust panic, an abort.
pub fn is_crash(e: &Entry) -> bool {
    if e.message_id.as_deref() == Some(COREDUMP) {
        return true;
    }
    let m = &e.message;
    if e.transport == Transport::Kernel {
        return m.contains(" segfault at ") || m.starts_with("traps: ") || m.contains("general protection fault");
    }
    (m.contains("panicked at ") && m.contains("thread"))
        || m.starts_with("Process ") && m.contains(" dumped core")
        || m.contains("terminate called after throwing")
}

/// What security and logins are made of.
pub fn is_security(e: &Entry) -> bool {
    const SOURCES: [&str; 21] = [
        "sudo",
        "su",
        "sshd",
        "sshd-session",
        "polkitd",
        "polkit-agent-helper-1",
        "pkexec",
        "login",
        "systemd-logind",
        "unix_chkpwd",
        "gdm-password",
        "greetd",
        "doas",
        "passwd",
        "chpasswd",
        "useradd",
        "usermod",
        "groupadd",
        "aizome-greeter",
        "audit",
        "kauditd",
    ];
    matches!(e.facility, Some(4 | 10)) || e.transport == Transport::Audit || SOURCES.contains(&&*e.source)
}

/// A filter under the toolbar.
#[derive(Clone, Debug)]
pub enum Chip {
    Source {
        name: Arc<str>,
        not: bool,
    },
    Unit {
        name: Arc<str>,
        not: bool,
    },
    Pid(u32),
    /// Messages of one shape ([`crate::shape`]), or (`not`) all but those.
    Shape {
        shape: u64,
        text: Arc<str>,
        not: bool,
    },
    /// What a suggestion found: a topic of an app's messages.
    Topic {
        label: Arc<str>,
        re: Arc<Regex>,
        not: bool,
    },
    Range {
        from: i64,
        to: i64,
    },
    MessageId {
        id: Arc<str>,
        label: Arc<str>,
    },
}

impl PartialEq for Chip {
    fn eq(&self, other: &Chip) -> bool {
        self.key() == other.key()
    }
}

impl Chip {
    /// What tells chips apart (two of one key are the same filter).
    pub fn key(&self) -> String {
        match self {
            Chip::Source { name, not } => format!("source {not} {name}"),
            Chip::Unit { name, not } => format!("unit {not} {name}"),
            Chip::Pid(p) => format!("pid {p}"),
            Chip::Shape { shape, not, .. } => format!("shape {not} {shape}"),
            Chip::Topic { re, not, .. } => format!("topic {not} {}", re.as_str()),
            Chip::Range { .. } => "range".into(),
            Chip::MessageId { id, .. } => format!("id {id}"),
        }
    }

    /// Whether it narrows to something (rather than hiding something).
    pub fn includes(&self) -> bool {
        !matches!(
            self,
            Chip::Source { not: true, .. }
                | Chip::Unit { not: true, .. }
                | Chip::Shape { not: true, .. }
                | Chip::Topic { not: true, .. }
        )
    }

    pub fn matches(&self, e: &Entry) -> bool {
        match self {
            Chip::Source { name, not } => (e.source == *name) != *not,
            Chip::Unit { name, not } => [&e.unit, &e.about].into_iter().flatten().any(|u| u == name) != *not,
            Chip::Pid(p) => e.pid == *p,
            Chip::Shape { shape, not, .. } => (e.shape == *shape) != *not,
            Chip::Topic { re, not, .. } => re.is_match(&e.message) != *not,
            Chip::Range { from, to } => e.time >= *from && e.time <= *to,
            Chip::MessageId { id, .. } => e.message_id.as_ref() == Some(id),
        }
    }

    pub fn label(&self, clock: &Clock) -> String {
        match self {
            Chip::Source { name, not: false } => format!("From {name}"),
            Chip::Source { name, not: true } => format!("Not from {name}"),
            Chip::Unit { name, not: false } => format!("Unit {name}"),
            Chip::Unit { name, not: true } => format!("Not unit {name}"),
            Chip::Pid(p) => format!("Process {p}"),
            Chip::Shape { text, not: false, .. } => format!("Like “{}”", short(text)),
            Chip::Shape { text, not: true, .. } => format!("Hiding “{}”", short(text)),
            Chip::Topic { label, not: false, .. } => label.to_string(),
            Chip::Topic { label, not: true, .. } => format!("Hiding {label}"),
            Chip::Range { from, to } => format!("{} – {}", clock.stamp(*from), clock.time(*to)),
            Chip::MessageId { label, .. } => label.to_string(),
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            Chip::Source { .. } => Icon::AppWindow,
            Chip::Unit { .. } => Icon::Settings2,
            Chip::Pid(_) => Icon::Hash,
            Chip::Shape { not: true, .. } | Chip::Topic { not: true, .. } => Icon::EyeOff,
            Chip::Shape { .. } => Icon::Layers,
            Chip::Topic { .. } => Icon::Tag,
            Chip::Range { .. } => Icon::Clock,
            Chip::MessageId { .. } => Icon::BookOpen,
        }
    }
}

/// A shape, short enough for a chip.
pub fn short(text: &str) -> String {
    let t: String = text.chars().take(42).collect();
    if t.len() < text.len() { format!("{}…", t.trim_end()) } else { t }
}

/// Everything that decides what shows.
#[derive(Clone, Debug)]
pub struct Filter {
    pub scope: Scope,
    pub level: Option<Priority>,
    pub chips: Vec<Chip>,
    pub search: crate::search::Search,
}

impl Default for Filter {
    fn default() -> Filter {
        Filter { scope: Scope::All, level: None, chips: Vec::new(), search: Default::default() }
    }
}

impl Filter {
    /// Whether it shows `e`, but for the chips naming sources and the
    /// stretch of time (so the sidebar can count each source, and the
    /// timeline chart the whole boot, with the rest of the filter).
    pub fn matches_but_sources(&self, e: &Entry, uid: u32) -> bool {
        self.matches_but_sources_in(e, uid, &mut crate::search::Names::default())
    }

    /// [`matches_but_sources`](Filter::matches_but_sources), remembering
    /// what each name decided (a view of many messages).
    pub fn matches_but_sources_in(&self, e: &Entry, uid: u32, names: &mut crate::search::Names) -> bool {
        self.scope.matches(e, uid)
            && self.level.is_none_or(|l| e.priority <= l)
            && self
                .chips
                .iter()
                .filter(|c| !matches!(c, Chip::Source { not: false, .. } | Chip::Range { .. }))
                .all(|c| c.matches(e))
            && self.search.matches_in(e, names)
    }

    pub fn matches(&self, e: &Entry, uid: u32) -> bool {
        self.matches_but_sources(e, uid)
            && self.sources_match(e)
            && self.range().is_none_or(|(a, b)| e.time >= a && e.time <= b)
    }

    /// The sources chosen: any of them (several are "this or that").
    pub fn sources_match(&self, e: &Entry) -> bool {
        let mut any = false;
        for c in &self.chips {
            if let Chip::Source { name, not: false } = c {
                if e.source == *name {
                    return true;
                }
                any = true;
            }
        }
        !any
    }

    /// The one source chosen, if one is.
    pub fn source(&self) -> Option<&Arc<str>> {
        let mut found = None;
        for c in &self.chips {
            if let Chip::Source { name, not: false } = c {
                if found.is_some() {
                    return None;
                }
                found = Some(name);
            }
        }
        found
    }

    pub fn range(&self) -> Option<(i64, i64)> {
        self.chips.iter().find_map(|c| match c {
            Chip::Range { from, to } => Some((*from, *to)),
            _ => None,
        })
    }

    /// Add a chip, replacing one of the same key (and a range, any range).
    pub fn add(&mut self, chip: Chip) {
        self.chips.retain(|c| c.key() != chip.key());
        self.chips.push(chip);
    }

    pub fn is_narrowed(&self) -> bool {
        self.scope != Scope::All || self.level.is_some() || !self.chips.is_empty() || !self.search.is_empty()
    }
}

/// The program a crash report names: "Process 298773 (gamescope-wl) of
/// user 1000 dumped core.", "traps: gamescope[1234] …", "foo[12]: segfault".
pub fn crashed(e: &Entry) -> Option<String> {
    let m = e.message.lines().next()?;
    if let Some(rest) = m.strip_prefix("Process ")
        && let Some((_, r)) = rest.split_once('(')
    {
        return r.split_once(')').map(|(n, _)| n.to_owned());
    }
    if let Some(rest) = m.strip_prefix("traps: ") {
        return rest.split_once('[').map(|(n, _)| n.to_owned());
    }
    if m.contains(" segfault at ") {
        return m.split_once('[').map(|(n, _)| n.to_owned());
    }
    if m.contains("panicked at") {
        return Some(e.source.to_string());
    }
    None
}
