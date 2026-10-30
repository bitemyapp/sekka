//! What picking something does: a menu item, a palette row, a Deskbar row,
//! a key binding. One vocabulary for every surface, carried out by the
//! shell's `update`.

use std::path::PathBuf;

use aizome_ipc::{DeskSel, Request, WindowId};
use aizome_search::Scope;

/// Something the shell does.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Action {
    /// Nothing (a header, a disabled row).
    #[default]
    None,
    /// Ask the app with `pid` to carry out its menu item `id`.
    AppItem {
        pid: u32,
        id: u32,
    },
    /// Ask the compositor.
    Wm(Request),
    /// Close every window of an app (Quit for apps without menus).
    QuitApp {
        app_id: String,
    },
    /// Hide an app's windows, resolving desktop-entry and window IDs.
    HideApp {
        app_id: String,
    },
    /// Reveal and raise an app's last window on this Desk, or another Desk
    /// if it has none here. Launch it when it has no windows.
    FocusApp {
        app_id: String,
    },
    /// Open an app by its desktop entry's id (`org.aizome.Hako`), or its
    /// app id: activate an existing window, or launch it if none exists.
    Launch {
        app_id: String,
    },
    /// Ask a running app for a new window (launch it again).
    NewWindow {
        app_id: String,
    },
    /// Run a command line (foot, a helper).
    Spawn {
        argv: Vec<String>,
    },
    /// Outcomes and output of applications opened by the shell.
    LaunchDetails {
        id: Option<u64>,
    },
    /// Open a file or folder: folders in Hako, files with their default app.
    Open {
        path: PathBuf,
    },
    /// Show a file in Hako.
    Reveal {
        path: PathBuf,
    },
    /// Open a link.
    OpenUrl {
        url: String,
    },
    /// Open Settei at a pane (and a place in it).
    Settings {
        pane: String,
        anchor: Option<String>,
    },
    /// The palette, in a scope (or All), with text typed.
    Palette {
        scope: Option<Scope>,
        query: String,
    },
    /// The Toolchest at the Toolchest button.
    Toolchest,
    /// Copy text to the clipboard.
    Copy {
        text: String,
    },
    /// Pick row `id` of the menu of the status item listed as `key`.
    StatusMenuItem {
        key: String,
        id: i32,
    },
    Session(SessionVerb),
    /// Night and Day.
    SetTheme {
        day: bool,
    },
    ToggleDoNotDisturb,
    /// Keep Awake on or off (`awake.rs`).
    ToggleKeepAwake,
    /// Reload Aizome without touching the apps: the compositor's logic,
    /// the shell and the settings, whatever changed (as ⌃❖R).
    ReloadAizome,
    Screenshot,
    /// Start a screen recording (one at a time).
    Record(crate::recording::Ask),
    /// Stop the recording going (the file is finished, then announced).
    StopRecording,
    NewDesk,
    /// Ask for a new Desk name in place (double-click in the strip).
    RenameDesk {
        index: u32,
    },
    EmptyTrash,
    MoveToTrash {
        path: PathBuf,
    },
    ShowDesktop,
    /// About an app (or Aizome when `None`).
    About {
        app_id: Option<String>,
    },
    /// Keep an app in the Deskbar, or stop.
    Pin {
        app_id: String,
        on: bool,
    },
    /// Run a service over the selected text.
    Service(ServiceVerb),
    /// The Deskbar shows, or folds into the bar.
    SetDeskbarCollapsed(bool),
    /// Move an app one place in the Deskbar without changing its pinned state.
    MoveDeskbarApp {
        app_id: String,
        down: bool,
    },
    /// Mount, unmount or eject a disk by its device path.
    Disk {
        device: String,
        verb: DiskVerb,
    },
    /// Take a parked Shelf item away.
    ShelfRemove {
        index: usize,
    },
    /// Make a folder on the desktop.
    NewFolderOnDesktop,
    /// Open the focused app's menu holding a command (its trail).
    ShowInMenu {
        trail: String,
    },
}

/// Leaving or pausing the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionVerb {
    Lock,
    Sleep,
    Restart,
    ShutDown,
    LogOut,
}

impl SessionVerb {
    pub fn label(self) -> &'static str {
        match self {
            SessionVerb::Lock => "Lock",
            SessionVerb::Sleep => "Sleep",
            SessionVerb::Restart => "Restart",
            SessionVerb::ShutDown => "Shut Down",
            SessionVerb::LogOut => "Log Out",
        }
    }

    /// Whether a dialog asks first (counting down 60 s).
    pub fn confirms(self) -> bool {
        matches!(self, SessionVerb::Restart | SessionVerb::ShutDown | SessionVerb::LogOut)
    }
}

/// Disks on the desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiskVerb {
    Mount,
    Unmount,
    Eject,
    OpenInTansu,
}

/// The verbs of the app menu's Services ▸, over the selected text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ServiceVerb {
    NewNote,
    OpenInSumi,
    SearchWeb,
    OpenUrl,
    LookUpInFiles,
    Calculate,
    CopyAsPlainText,
}

impl ServiceVerb {
    pub fn label(&self) -> &'static str {
        match self {
            ServiceVerb::NewNote => "New Note with Selection",
            ServiceVerb::OpenInSumi => "Open in Sumi",
            ServiceVerb::SearchWeb => "Search the Web",
            ServiceVerb::OpenUrl => "Open URL",
            ServiceVerb::LookUpInFiles => "Look Up in Files",
            ServiceVerb::Calculate => "Calculate",
            ServiceVerb::CopyAsPlainText => "Copy as Plain Text",
        }
    }

    /// The verbs that apply to `text`, in menu order.
    pub fn for_text(text: &str) -> Vec<ServiceVerb> {
        let t = text.trim();
        if t.is_empty() {
            return Vec::new();
        }
        let mut v = vec![ServiceVerb::NewNote, ServiceVerb::OpenInSumi, ServiceVerb::SearchWeb];
        if looks_like_url(t) {
            v.push(ServiceVerb::OpenUrl);
        }
        v.push(ServiceVerb::LookUpInFiles);
        if aizome_search::calc::looks_like_math(t) && aizome_search::calc::eval(t).is_ok() {
            v.push(ServiceVerb::Calculate);
        }
        v.push(ServiceVerb::CopyAsPlainText);
        v
    }
}

/// Whether `s` is one link (a scheme and no spaces, or a bare domain).
pub fn looks_like_url(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.contains(char::is_whitespace) {
        return false;
    }
    if let Some((scheme, rest)) = s.split_once("://") {
        return !rest.is_empty()
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    }
    if s.starts_with("mailto:") {
        return true;
    }
    // www.example.org, example.com/path; not main.rs.
    const COMMON: [&str; 16] =
        ["com", "org", "net", "edu", "gov", "io", "dev", "app", "jp", "uk", "de", "fr", "eu", "info", "me", "ai"];
    let host = s.split('/').next().unwrap_or("");
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && parts.last().is_some_and(|tld| {
            tld.len() >= 2
                && tld.chars().all(|c| c.is_ascii_alphabetic())
                && (host.starts_with("www.") || COMMON.contains(&tld.to_ascii_lowercase().as_str()))
        })
}

/// Shorthands for compositor requests.
pub mod wm {
    use super::*;

    pub fn focus(id: WindowId) -> Action {
        Action::Wm(Request::Focus { id })
    }

    pub fn close(id: Option<WindowId>) -> Action {
        Action::Wm(Request::Close { id })
    }

    pub fn set_desk(index: u32) -> Action {
        Action::Wm(Request::SetDesk { desk: DeskSel::Index(index), output: None })
    }

    /// Shows a Desk on a display (from that display's bar).
    pub fn set_desk_on(index: u32, output: String) -> Action {
        let output = (!output.is_empty()).then_some(output);
        Action::Wm(Request::SetDesk { desk: DeskSel::Index(index), output })
    }

    pub fn move_to_desk(id: Option<WindowId>, index: u32) -> Action {
        Action::Wm(Request::MoveToDesk { id, desk: DeskSel::Index(index), follow: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_told_from_words() {
        assert!(looks_like_url("https://aizome.dev/docs"));
        assert!(looks_like_url("example.org"));
        assert!(looks_like_url("www.example.co.jp/a?b=c"));
        assert!(looks_like_url("mailto:a@b.c"));
        assert!(!looks_like_url("damage is per layer"));
        assert!(!looks_like_url("main.rs"));
        assert!(!looks_like_url("3.14"));
    }

    #[test]
    fn services_follow_the_selection() {
        let words = ServiceVerb::for_text("tiles at fractional scale");
        assert!(words.contains(&ServiceVerb::NewNote));
        assert!(!words.contains(&ServiceVerb::OpenUrl));
        assert!(!words.contains(&ServiceVerb::Calculate));
        assert!(ServiceVerb::for_text("https://example.org").contains(&ServiceVerb::OpenUrl));
        assert!(ServiceVerb::for_text("2^10 / 4").contains(&ServiceVerb::Calculate));
        assert!(ServiceVerb::for_text("   ").is_empty());
    }
}
