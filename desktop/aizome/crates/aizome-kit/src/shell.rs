//! Publishing the app's menus to the Aizome shell over
//! `$AIZOME_SHELL_SOCKET` (`aizome-shell-proto`), and acting on what the
//! shell sends back. Without the socket (another desktop), the kit draws
//! the menu bar along each window's top instead.
//!
//! The shell may come later than the app (apps a session starts before
//! the shell is up) and may go away (it crashed, or restarts): while
//! there's no shell the kit draws the menu bars in the windows and looks
//! for its socket once a second; once the shell is there the menus go to
//! it again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aizome_shell_proto as proto;

use crate::app::Runtime;
use crate::engine::Core;
use crate::hash::hash_of;
use crate::menu::{ActionSpec, ItemSpec, MenuSpec, Shortcut, ShortcutKey};
use crate::platform::{Platform, Waker};
use crate::program::{AppInfo, Program};

/// The connection to the shell.
pub(crate) struct ShellLink {
    sender: proto::Sender,
    rx: Receiver<proto::ShellMessage>,
    /// Cleared when the shell hangs up.
    alive: Arc<AtomicBool>,
}

impl ShellLink {
    /// Connect to the shell at `path` and say hello; `None` (and a line in
    /// the log) if there's no shell there.
    pub fn connect_to(path: &Path, info: &AppInfo, wake: Waker) -> Option<ShellLink> {
        match ShellLink::try_connect(path, info, wake) {
            Ok(link) => Some(link),
            Err(e) => {
                log::info!("no shell at {} ({e}); menus go in the windows until there is", path.display());
                None
            }
        }
    }

    /// Connect again to a shell that took over from the one this app was
    /// talking to; `None`, quietly, if there's none.
    pub fn reconnect(path: &Path, info: &AppInfo, wake: Waker) -> Option<ShellLink> {
        ShellLink::try_connect(path, info, wake).ok()
    }

    fn try_connect(path: &Path, info: &AppInfo, wake: Waker) -> std::io::Result<ShellLink> {
        let mut client = proto::Client::hello(path, &info.app_id, &info.name)?;
        let sender = client.sender()?;
        let (tx, rx) = channel();
        let alive = Arc::new(AtomicBool::new(true));
        let flag = alive.clone();
        std::thread::Builder::new().name("aizome-shell".into()).spawn(move || {
            let mut profile = None;
            while let Ok(Some(m)) = client.recv() {
                match &m {
                    proto::ShellMessage::Profile { config } => {
                        if aizome_trace::start(config.clone()).is_ok() {
                            profile = Some(config.session);
                        }
                        continue;
                    }
                    proto::ShellMessage::ProfileStop { session } => {
                        aizome_trace::stop(*session);
                        continue;
                    }
                    _ => {}
                }
                if tx.send(m).is_err() {
                    break;
                }
                wake();
            }
            // The shell hung up.
            if let Some(id) = profile {
                aizome_trace::stop(id);
            }
            flag.store(false, Ordering::Release);
            wake();
        })?;
        Ok(ShellLink { sender, rx, alive })
    }

    /// The shell is still there.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    pub fn incoming(&mut self) -> Vec<proto::ShellMessage> {
        self.rx.try_iter().collect()
    }

    pub fn send_menus(&mut self, menus: Vec<proto::Menu>) {
        if let Err(e) = self.sender.send(&proto::AppMessage::Menus { menus }) {
            log::warn!("the shell: {e}");
        }
    }

    pub fn send(&mut self, msg: &proto::AppMessage) {
        if let Err(e) = self.sender.send(msg) {
            log::warn!("the shell: {e}");
        }
    }
}

/// Where the shell is to be found, and how to wake the loop when it
/// comes or goes.
#[derive(Clone)]
pub(crate) struct ShellPlace {
    pub path: PathBuf,
    pub wake: Waker,
    /// How often to look for the socket while there's no shell.
    pub period: Duration,
}

/// Looking for the shell on a thread of its own: its socket, once a
/// period (a `stat`, and a connection once the file is there).
pub(crate) struct ShellWatch {
    found: Arc<Mutex<Option<ShellLink>>>,
    stop: Arc<AtomicBool>,
}

impl ShellWatch {
    /// Once a second.
    pub const PERIOD: Duration = Duration::from_secs(1);

    pub fn start(place: &ShellPlace, info: &AppInfo) -> ShellWatch {
        let found = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (slot, quit, place, info) = (found.clone(), stop.clone(), place.clone(), info.clone());
        let started = std::thread::Builder::new().name("aizome-shell-watch".into()).spawn(move || {
            loop {
                std::thread::sleep(place.period);
                if quit.load(Ordering::Acquire) {
                    return;
                }
                if !place.path.exists() {
                    continue;
                }
                // There, but maybe not listening yet (or left behind).
                if let Ok(link) = ShellLink::try_connect(&place.path, &info, place.wake.clone()) {
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(link);
                    (place.wake)();
                    return;
                }
            }
        });
        if let Err(e) = started {
            log::warn!("looking for the shell: {e}");
        }
        ShellWatch { found, stop }
    }

    /// The shell, once found.
    pub fn take(&self) -> Option<ShellLink> {
        self.found.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl Drop for ShellWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// An item's id: its own, else its role's, else from its menu's title,
/// its label and its place, so it stays while the item does.
fn item_id(a: &ActionSpec, path: &str, index: usize) -> u32 {
    if let Some(id) = a.id {
        return id;
    }
    let h = match a.role {
        Some(r) if r != crate::menu::MenuRole::WindowList => hash_of(&("role", r.name())),
        _ => hash_of(&(path, a.label.as_str(), index)),
    };
    // Kept off 0 and the window list's range.
    ((h as u32) & 0x3fff_ffff).max(1)
}

fn menu_id(path: &str) -> u32 {
    ((hash_of(&("menu", path)) as u32) & 0x3fff_ffff).max(1) | 0x4000_0000
}

fn shortcut(s: &Shortcut) -> proto::Shortcut {
    let key = match s.key {
        ShortcutKey::Char(c) => c.to_uppercase().collect(),
        ShortcutKey::Enter => "Enter".into(),
        ShortcutKey::Tab => "Tab".into(),
        ShortcutKey::Space => "Space".into(),
        ShortcutKey::Escape => "Escape".into(),
        ShortcutKey::Backspace => "Backspace".into(),
        ShortcutKey::Delete => "Delete".into(),
        ShortcutKey::Up => "Up".into(),
        ShortcutKey::Down => "Down".into(),
        ShortcutKey::Left => "Left".into(),
        ShortcutKey::Right => "Right".into(),
        ShortcutKey::Home => "Home".into(),
        ShortcutKey::End => "End".into(),
        ShortcutKey::PageUp => "PageUp".into(),
        ShortcutKey::PageDown => "PageDown".into(),
        ShortcutKey::F(n) => format!("F{n}"),
    };
    proto::Shortcut { key, command: s.command, shift: s.shift, alt: s.alt, ctrl: s.ctrl }
}

fn items(spec: &MenuSpec, path: &str) -> Vec<proto::Item> {
    let mut out = Vec::new();
    for (i, item) in spec.items.iter().enumerate() {
        out.push(match item {
            ItemSpec::Action(a) => proto::Item {
                id: item_id(a, path, i),
                kind: proto::ItemKind::Action,
                label: a.label.clone(),
                shortcut: a.shortcut.as_ref().map(shortcut),
                enabled: a.enabled,
                checked: a.checked,
                role: a.role.map(|r| r.name().to_owned()),
                icon: a.icon.map(|i| i.name().to_owned()),
                items: Vec::new(),
            },
            ItemSpec::Separator => {
                proto::Item { id: 0, kind: proto::ItemKind::Separator, enabled: false, ..Default::default() }
            }
            ItemSpec::Header(t) => proto::Item {
                id: 0,
                kind: proto::ItemKind::Header,
                label: t.clone(),
                enabled: false,
                ..Default::default()
            },
            ItemSpec::Submenu(m) => {
                let sub = format!("{path}/{}", m.title);
                proto::Item {
                    id: menu_id(&sub),
                    kind: proto::ItemKind::Submenu,
                    label: m.title.clone(),
                    enabled: !m.items.is_empty(),
                    items: items(m, &sub),
                    ..Default::default()
                }
            }
        });
    }
    out
}

/// The menus as the shell gets them, their states settled.
pub(crate) fn published(core: &Core) -> Vec<proto::Menu> {
    core.menubar
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut m = m.clone();
            core.settle_app_menu(&mut m, None);
            let path = if i == 0 { "app".to_owned() } else { m.title.clone() };
            proto::Menu { id: menu_id(&path), title: m.title.clone(), items: items(&m, &path) }
        })
        .collect()
}

/// The action with the shell's id `id`.
pub(crate) fn find(core: &Core, id: u32) -> Option<ActionSpec> {
    fn walk(spec: &MenuSpec, path: &str, id: u32) -> Option<ActionSpec> {
        for (i, item) in spec.items.iter().enumerate() {
            match item {
                ItemSpec::Action(a) if item_id(a, path, i) == id => return Some(a.clone()),
                ItemSpec::Submenu(m) => {
                    if let Some(a) = walk(m, &format!("{path}/{}", m.title), id) {
                        return Some(a);
                    }
                }
                _ => {}
            }
        }
        None
    }
    core.menubar.iter().enumerate().find_map(|(i, m)| {
        let mut m = m.clone();
        core.settle_app_menu(&mut m, None);
        let path = if i == 0 { "app".to_owned() } else { m.title.clone() };
        walk(&m, &path, id)
    })
}

/// What the shell said.
pub(crate) fn handle<P: Program>(rt: &mut Runtime<P>, msg: proto::ShellMessage, platform: &mut dyn Platform) {
    match msg {
        proto::ShellMessage::Activate { id } => {
            let Some(a) = find(&rt.core, id) else {
                log::debug!("the shell chose item {id}, which isn't there any more");
                return;
            };
            let win = rt.core.key_window.or_else(|| rt.core.app_windows().first().map(|(w, _)| *w));
            if let Some(win) = win {
                rt.core.perform(&a, win, platform);
            }
        }
        proto::ShellMessage::AboutToShow { .. } => {
            rt.refresh_menus(platform);
            rt.publish_menus();
        }
        proto::ShellMessage::ServicesText { serial } => {
            let text = rt.core.focused_selection();
            if let Some(s) = &mut rt.shell {
                s.send(&proto::AppMessage::ServicesText { serial, text });
            }
        }
        // Answers to requests, which apps don't make.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{Menu, MenuBar, MenuItem, MenuRole};
    use crate::theme::Appearance;

    #[test]
    fn published_menus_keep_their_ids() {
        let info = AppInfo::new("org.aizome.Test", "Test", "Tester");
        let mut core = Core::new(info.clone(), Appearance::default());
        core.menubar = MenuBar::<()>::standard(&info).into_specs();
        let a = published(&core);
        let b = published(&core);
        assert_eq!(a, b);
        assert_eq!(a[0].title, "Test");
        let edit = &a[1];
        let copy = edit.items.iter().find(|i| i.label == "Copy").expect("Copy");
        assert_eq!(copy.role.as_deref(), Some("copy"));
        assert_eq!(copy.shortcut.as_ref().map(|s| s.label()), Some("❖C".to_owned()));
        // Without a focused text widget, Copy is disabled.
        assert!(!copy.enabled);
        let found = find(&core, copy.id).expect("found by its id");
        assert_eq!(found.label, "Copy");
        let find_menu = edit.items.iter().find(|i| i.kind == proto::ItemKind::Submenu).expect("Find ▸");
        assert!(find_menu.items.iter().any(|i| i.label == "Find…"));
        // Every action's id is distinct.
        let mut ids: Vec<u32> = Vec::new();
        fn collect(items: &[proto::Item], ids: &mut Vec<u32>) {
            for i in items {
                if i.kind == proto::ItemKind::Action {
                    ids.push(i.id);
                }
                collect(&i.items, ids);
            }
        }
        for m in &a {
            collect(&m.items, &mut ids);
        }
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn unsupported_standard_roles_are_unavailable_in_native_and_published_menus() {
        let info = AppInfo::new("org.aizome.Test", "Test", "Tester");
        let mut core = Core::new(info.clone(), Appearance::default());
        core.menubar = MenuBar::<()>::standard(&info).into_specs();
        assert!(!published(&core)[0].items.iter().any(|i| i.label == "Settings…"));
        assert!(!core.menubar[0].actions().into_iter().any(|a| a.role == Some(MenuRole::Settings)));
        // An app that explicitly adds a role without implementing it must
        // still get the unavailable state in both menu implementations.
        core.menubar = MenuBar::<()>::standard(&info)
            .extend("Test", [MenuItem::role_item("Settings…", MenuRole::Settings)])
            .into_specs();
        let menus = published(&core);
        for (index, label) in [(0, "Settings…"), (3, "Test Help")] {
            let item = menus[index].items.iter().find(|i| i.label == label).unwrap();
            assert!(!item.enabled, "{label} has no handler");
            assert!(!find(&core, item.id).unwrap().enabled, "shell lookup retains the unavailable state");
            let mut native = core.menubar[index].clone();
            core.settle_menu(&mut native);
            assert!(!native.actions().into_iter().find(|a| a.label == label).unwrap().enabled);
        }
        assert!(menus[0].items.iter().find(|i| i.label == "About Test").unwrap().enabled);
        for label in ["Search", "Aizome Tour"] {
            assert!(menus[3].items.iter().find(|i| i.label == label).unwrap().enabled);
        }
    }

    #[test]
    fn app_owned_settings_and_help_keep_their_declared_states() {
        let info = AppInfo::new("org.aizome.Test", "Test", "Tester");
        let mut core = Core::new(info, Appearance::default());
        core.menubar = MenuBar::new()
            .with_menu(
                Menu::new("App")
                    .item(MenuItem::new("Settings", ()).role(MenuRole::Settings))
                    .item(MenuItem::new("Help", ()).role(MenuRole::Help))
                    .item(MenuItem::new("Unavailable Settings", ()).role(MenuRole::Settings).id(41).enabled(false))
                    .item(MenuItem::new("Unavailable Help", ()).role(MenuRole::Help).id(42).enabled(false)),
            )
            .into_specs();
        let items = &published(&core)[0].items;
        assert_eq!(items.iter().map(|i| i.enabled).collect::<Vec<_>>(), [true, true, false, false]);
        for item in items {
            let action = find(&core, item.id).unwrap();
            assert!(action.msg.is_some());
            assert_eq!(action.enabled, item.enabled);
        }
    }
}
