//! What the compositor says about windows, Desks and outputs, kept current
//! by its events, and the Deskbar's tree made from it.

use std::collections::BTreeMap;

use aizome_ipc::{DeskInfo, Event, OutputInfo, WindowId, WindowInfo};

/// What an event changed, so only the surfaces that show it are viewed
/// again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changed {
    pub windows: bool,
    pub focus: bool,
    pub desks: bool,
    pub outputs: bool,
}

impl Changed {
    pub fn any(self) -> bool {
        self.windows || self.focus || self.desks || self.outputs
    }
}

/// The compositor's state, as the shell knows it.
#[derive(Clone, Debug, Default)]
pub struct Wm {
    pub windows: BTreeMap<WindowId, WindowInfo>,
    /// Window ids in the order they opened.
    opened: Vec<WindowId>,
    /// App ids in the order their first window opened (kept after their
    /// last window closes, so an app that comes back keeps its place).
    app_order: Vec<String>,
    /// The windows most recently focused first, per app: a click on the app
    /// in the Deskbar focuses its last window.
    recent: Vec<WindowId>,
    pub desks: Vec<DeskInfo>,
    /// The current Desk, 1-based.
    pub current_desk: u32,
    pub focused: Option<WindowId>,
    pub outputs: Vec<OutputInfo>,
    pub locked: bool,
    /// Whether the compositor has been heard from.
    pub connected: bool,
}

/// What the compositor said when the shell connected.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub windows: Vec<WindowInfo>,
    pub desks: Vec<DeskInfo>,
    pub current_desk: u32,
    pub focused: Option<WindowId>,
    pub outputs: Vec<OutputInfo>,
}

impl Wm {
    /// Start over from what the compositor said on connecting.
    pub fn reset(&mut self, s: Snapshot) {
        self.windows.clear();
        self.opened.clear();
        // Window ids are never reused, so the order they came in is the
        // order they opened.
        let mut windows = s.windows;
        windows.sort_by_key(|w| w.id);
        for w in windows {
            self.add(w);
        }
        self.desks = s.desks;
        self.current_desk = s.current_desk;
        self.focused = s.focused.or_else(|| self.windows.values().find(|w| w.focused).map(|w| w.id));
        if let Some(f) = self.focused {
            self.touch(f);
        }
        self.outputs = s.outputs;
        self.connected = true;
    }

    fn add(&mut self, w: WindowInfo) {
        if !self.opened.contains(&w.id) {
            self.opened.push(w.id);
        }
        if !w.app_id.is_empty() && !self.app_order.contains(&w.app_id) {
            self.app_order.push(w.app_id.clone());
        }
        self.windows.insert(w.id, w);
    }

    fn touch(&mut self, id: WindowId) {
        self.recent.retain(|w| *w != id);
        self.recent.insert(0, id);
    }

    /// Take in an event: what it changed.
    pub fn apply(&mut self, event: &Event) -> Changed {
        let mut c = Changed::default();
        match event {
            Event::WindowOpened { window } => {
                self.add(window.clone());
                if window.focused {
                    self.focused = Some(window.id);
                    self.touch(window.id);
                    c.focus = true;
                }
                c.windows = true;
            }
            Event::WindowClosed { id } => {
                self.windows.remove(id);
                self.opened.retain(|w| w != id);
                self.recent.retain(|w| w != id);
                if self.focused == Some(*id) {
                    self.focused = None;
                    c.focus = true;
                }
                c.windows = true;
            }
            Event::WindowChanged { window } => {
                let app_changed = self.windows.get(&window.id).is_some_and(|w| w.app_id != window.app_id);
                self.add(window.clone());
                if app_changed && !self.app_order.contains(&window.app_id) {
                    self.app_order.push(window.app_id.clone());
                }
                c.windows = true;
            }
            Event::WindowFocused { id } => {
                if self.focused != *id {
                    self.focused = *id;
                    c.focus = true;
                }
                for w in self.windows.values_mut() {
                    w.focused = Some(w.id) == *id;
                }
                if let Some(id) = id {
                    self.touch(*id);
                }
                c.windows = true;
            }
            Event::DeskChanged { index, .. } => {
                self.current_desk = *index;
                for d in &mut self.desks {
                    d.current = d.index == *index;
                }
                c.desks = true;
                c.windows = true;
            }
            Event::DesksChanged { desks } => {
                self.desks = desks.clone();
                if let Some(d) = desks.iter().find(|d| d.current) {
                    self.current_desk = d.index;
                }
                c.desks = true;
            }
            Event::OutputsChanged { outputs } => {
                self.outputs = outputs.clone();
                c.outputs = true;
            }
            Event::LockStateChanged { locked } => self.locked = *locked,
            _ => {}
        }
        c
    }

    pub fn focused_window(&self) -> Option<&WindowInfo> {
        self.focused.and_then(|id| self.windows.get(&id))
    }

    /// Windows in the order they opened.
    pub fn in_open_order(&self) -> impl Iterator<Item = &WindowInfo> {
        self.opened.iter().filter_map(|id| self.windows.get(id))
    }

    /// The app's windows, most recently focused first.
    pub fn app_windows_by_recency(&self, app_id: &str) -> Vec<&WindowInfo> {
        let mut out: Vec<&WindowInfo> =
            self.recent.iter().filter_map(|id| self.windows.get(id)).filter(|w| w.app_id == app_id).collect();
        for w in self.in_open_order() {
            if w.app_id == app_id && !out.iter().any(|o| o.id == w.id) {
                out.push(w);
            }
        }
        out
    }

    /// Whether an app has a window open.
    pub fn is_running(&self, app_id: &str) -> bool {
        self.windows.values().any(|w| w.app_id == app_id)
    }

    pub fn desk(&self, index: u32) -> Option<&DeskInfo> {
        self.desks.iter().find(|d| d.index == index)
    }

    pub fn current_desk_info(&self) -> Option<&DeskInfo> {
        self.desk(self.current_desk)
    }

    /// The output a point (global, in points) is on.
    pub fn output_at(&self, x: f64, y: f64) -> Option<&OutputInfo> {
        self.outputs.iter().find(|o| {
            let r = o.logical;
            x >= r.x as f64 && y >= r.y as f64 && x < (r.x + r.w) as f64 && y < (r.y + r.h) as f64
        })
    }

    /// The Deskbar's apps, pinned first, then in launch order, each with
    /// its windows (in the order they opened).
    pub fn deskbar(&self, pinned: &[String]) -> Vec<DeskbarApp> {
        let mut ids: Vec<String> = pinned.to_vec();
        for a in &self.app_order {
            if !ids.contains(a) && self.is_running(a) {
                ids.push(a.clone());
            }
        }
        ids.into_iter()
            .map(|app_id| {
                let windows: Vec<DeskbarWindow> = self
                    .in_open_order()
                    .filter(|w| w.app_id == app_id)
                    .filter(|w| w.parent.is_none())
                    .map(|w| DeskbarWindow {
                        id: w.id,
                        title: if w.title.is_empty() { app_id.clone() } else { w.title.clone() },
                        focused: self.focused == Some(w.id),
                        note: self.note(w),
                        urgent: w.urgent,
                    })
                    .collect();
                DeskbarApp {
                    pinned: pinned.contains(&app_id),
                    focused: windows.iter().any(|w| w.focused),
                    app_id,
                    windows,
                }
            })
            .collect()
    }

    /// What a window's Deskbar row says at its right.
    fn note(&self, w: &WindowInfo) -> WindowNote {
        if w.minimised || w.hidden {
            WindowNote::Hidden
        } else if let Some(d) = w.desk.filter(|d| *d != self.current_desk && !w.sticky) {
            WindowNote::Desk(d)
        } else if !w.group_members.is_empty() {
            WindowNote::Stack
        } else {
            WindowNote::None
        }
    }
}

/// An app in the Deskbar.
#[derive(Clone, Debug, PartialEq)]
pub struct DeskbarApp {
    pub app_id: String,
    pub pinned: bool,
    /// One of its windows has the focus.
    pub focused: bool,
    pub windows: Vec<DeskbarWindow>,
}

/// A window under its app in the Deskbar.
#[derive(Clone, Debug, PartialEq)]
pub struct DeskbarWindow {
    pub id: WindowId,
    pub title: String,
    pub focused: bool,
    pub note: WindowNote,
    pub urgent: bool,
}

/// The word at a Deskbar window row's right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowNote {
    None,
    /// Minimised, or its app hidden: italic, "hidden".
    Hidden,
    /// In a tab group: "stack".
    Stack,
    /// On another Desk: "desk N".
    Desk(u32),
}

impl WindowNote {
    pub fn label(self) -> Option<String> {
        match self {
            WindowNote::None => None,
            WindowNote::Hidden => Some("hidden".into()),
            WindowNote::Stack => Some("stack".into()),
            WindowNote::Desk(n) => Some(format!("desk {n}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: WindowId, app: &str, title: &str) -> WindowInfo {
        WindowInfo { id, app_id: app.into(), title: title.into(), desk: Some(1), ..Default::default() }
    }

    fn desks() -> Vec<DeskInfo> {
        (1..=3)
            .map(|i| DeskInfo { index: i, id: i as u64, name: format!("{i}"), current: i == 1, ..Default::default() })
            .collect()
    }

    fn wm() -> Wm {
        let mut wm = Wm::default();
        wm.reset(Snapshot { desks: desks(), current_desk: 1, ..Default::default() });
        wm
    }

    #[test]
    fn the_deskbar_lists_apps_in_launch_order_with_their_windows() {
        let mut wm = wm();
        wm.apply(&Event::WindowOpened { window: win(1, "org.aizome.Sumi", "architecture.md") });
        wm.apply(&Event::WindowOpened { window: win(2, "foot", "cargo test") });
        wm.apply(&Event::WindowOpened { window: win(3, "org.aizome.Sumi", "roadmap.md") });
        wm.apply(&Event::WindowFocused { id: Some(1) });
        let mut hidden = win(3, "org.aizome.Sumi", "roadmap.md");
        hidden.minimised = true;
        wm.apply(&Event::WindowChanged { window: hidden });
        let bar = wm.deskbar(&[]);
        assert_eq!(bar.iter().map(|a| a.app_id.as_str()).collect::<Vec<_>>(), ["org.aizome.Sumi", "foot"]);
        let sumi = &bar[0];
        assert!(sumi.focused);
        assert_eq!(sumi.windows.len(), 2);
        assert!(sumi.windows[0].focused);
        assert_eq!(sumi.windows[1].note, WindowNote::Hidden);
        assert_eq!(sumi.windows[1].note.label().as_deref(), Some("hidden"));
    }

    #[test]
    fn pinned_apps_come_first_even_when_not_running() {
        let mut wm = wm();
        wm.apply(&Event::WindowOpened { window: win(1, "foot", "fish") });
        let bar = wm.deskbar(&["org.aizome.Hako".into()]);
        assert_eq!(bar[0].app_id, "org.aizome.Hako");
        assert!(bar[0].pinned && bar[0].windows.is_empty());
        assert_eq!(bar[1].app_id, "foot");
    }

    #[test]
    fn notes_say_stack_and_desk() {
        let mut wm = wm();
        let mut a = win(1, "foot", "cargo test");
        a.group = Some(9);
        a.group_members = vec![1, 2];
        let mut b = win(2, "org.aizome.Hako", "Downloads");
        b.desk = Some(2);
        wm.apply(&Event::WindowOpened { window: a });
        wm.apply(&Event::WindowOpened { window: b });
        let bar = wm.deskbar(&[]);
        assert_eq!(bar[0].windows[0].note, WindowNote::Stack);
        assert_eq!(bar[1].windows[0].note.label().as_deref(), Some("desk 2"));
        // Going to Desk 2 makes it local.
        wm.apply(&Event::DeskChanged { index: 2, id: 2, name: "2".into() });
        assert_eq!(wm.deskbar(&[])[1].windows[0].note, WindowNote::None);
    }

    #[test]
    fn closing_windows_drops_apps_and_focus() {
        let mut wm = wm();
        wm.apply(&Event::WindowOpened { window: win(1, "foot", "a") });
        wm.apply(&Event::WindowFocused { id: Some(1) });
        let c = wm.apply(&Event::WindowClosed { id: 1 });
        assert!(c.windows && c.focus);
        assert!(wm.focused.is_none());
        assert!(wm.deskbar(&[]).is_empty());
        // It keeps its place when it comes back.
        wm.apply(&Event::WindowOpened { window: win(2, "org.aizome.Fusen", "n") });
        wm.apply(&Event::WindowOpened { window: win(3, "foot", "b") });
        let ids: Vec<String> = wm.deskbar(&[]).into_iter().map(|a| a.app_id).collect();
        assert_eq!(ids, ["foot", "org.aizome.Fusen"]);
    }

    #[test]
    fn the_last_focused_window_of_an_app_comes_first() {
        let mut wm = wm();
        wm.apply(&Event::WindowOpened { window: win(1, "foot", "a") });
        wm.apply(&Event::WindowOpened { window: win(2, "foot", "b") });
        wm.apply(&Event::WindowFocused { id: Some(2) });
        wm.apply(&Event::WindowFocused { id: Some(1) });
        let ids: Vec<WindowId> = wm.app_windows_by_recency("foot").iter().map(|w| w.id).collect();
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn desks_follow_their_events() {
        let mut wm = wm();
        let c = wm.apply(&Event::DeskChanged { index: 3, id: 3, name: "3".into() });
        assert!(c.desks);
        assert_eq!(wm.current_desk, 3);
        assert!(wm.current_desk_info().unwrap().current);
        let mut more = desks();
        more.push(DeskInfo { index: 4, id: 4, name: "Write".into(), ..Default::default() });
        wm.apply(&Event::DesksChanged { desks: more });
        assert_eq!(wm.desks.len(), 4);
    }
}
