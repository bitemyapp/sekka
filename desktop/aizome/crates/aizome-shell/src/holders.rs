//! Apps holding the screen on (docs/keep-awake.md): who they are, as one
//! list from the two ways an app asks, and when the menu bar's cup shows
//! for them.
//!
//! - A Wayland idle inhibitor (Utsushi, mpv, a browser's window with the
//!   keys): the compositor says which windows and processes have one (IPC
//!   `idle-inhibitors`, and `idle-inhibitors-changed`).
//! - `org.freedesktop.ScreenSaver` on the session bus (VLC, Firefox, a
//!   browser otherwise): the shell serves it, and has each hold's app,
//!   reason and process (`aizome_sys::screensaver`).
//!
//! The shell's own inhibitor, which stands at the compositor for the
//! second kind (`idle.rs`), is no app, and Keep Awake's hold is the user's
//! own switch: neither is listed. An app that asks both ways, as Chrome
//! does, is one holder.
//!
//! These holds keep the screen on and unlocked. They don't stop sleep or
//! the lid (Keep Awake does), and once the session is locked the
//! compositor stops counting them. Nothing here lets go of one: the app
//! would only take it again, and a Wayland inhibitor isn't the shell's to
//! release.
//!
//! The cup waits [`SHOWS_AFTER`] before it shows for apps and stays
//! [`LINGERS`] after the last lets go ([`Showing`]), so a hold dropped and
//! taken again at once doesn't blink in the bar.

use std::time::Duration;

use aizome_ipc::{IdleHolder, IdleInhibitorInfo, WindowId, WindowInfo};
use aizome_kit::{Cx, Icon};
use aizome_sys::apps::{App, AppIndex, program_name};
use aizome_sys::screensaver::Hold;

use crate::apps::catalog;
use crate::look::Hue;
use crate::menubar::{Cup, TrayWidget};
use crate::shell::{Msg, Shell};
use crate::wm::Wm;

/// How long apps have held before the cup shows for them: a sound that
/// plays for a moment, or a video started and stopped at once, shows
/// nothing.
pub const SHOWS_AFTER: Duration = Duration::from_secs(3);

/// How long a holder's row, and the cup after the last of them, stay once
/// its hold has gone. VLC lets go and holds again at each loop of a clip,
/// about 60 ms apart, and a browser at each pause and play: within this
/// they never left.
pub const LINGERS: Duration = Duration::from_secs(2);

/// A process, by its ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(u32);

impl Pid {
    /// Process `raw`; there is no process 0.
    pub fn new(raw: u32) -> Option<Pid> {
        (raw != 0).then_some(Pid(raw))
    }

    /// As the compositor's IPC counts them, signed.
    pub fn from_ipc(raw: i32) -> Option<Pid> {
        u32::try_from(raw).ok().and_then(Pid::new)
    }

    /// This process: the shell.
    pub fn of_shell() -> Pid {
        Pid(std::process::id())
    }

    pub fn get(self) -> u32 {
        self.0
    }

    fn to_ipc(self) -> Option<i32> {
        i32::try_from(self.0).ok()
    }
}

/// What makes two holds one app's.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HolderKey {
    /// Its desktop entry, by ID (`vlc.desktop`).
    Entry(String),
    /// What it goes by where no desktop entry is found, lowercased: a
    /// window's app ID, the name it gave the bus, its program's file name.
    Named(String),
    /// A process nothing names.
    Process(Pid),
    /// An inhibitor on a surface that's no window's, whose client's
    /// process the compositor couldn't say either.
    Surface,
    /// A connection on the session bus that said nothing of itself, whose
    /// process the bus didn't say either.
    Peer(String),
}

/// One app holding the screen on: a row of the cup's panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub key: HolderKey,
    /// Its desktop entry's name, where it has one; else what it goes by.
    pub name: String,
    pub glyph: (Icon, Hue),
    /// Why, in the words it gave the session bus ("Playing video"). A
    /// Wayland idle inhibitor says nothing.
    pub reasons: Vec<String>,
    /// A window of its to bring forward: the one that holds, else the one
    /// the app would be switched to.
    pub window: Option<WindowId>,
    /// That window's title, when it has one.
    pub title: Option<String>,
    /// A process of its, for Myaku.
    pub pid: Option<Pid>,
}

impl Holder {
    /// What goes under its name: why it holds, else which window does.
    pub fn detail(&self) -> String {
        if self.reasons.is_empty() { self.title.clone().unwrap_or_default() } else { self.reasons.join(", ") }
    }

    /// Another hold of the same app's.
    fn also(&mut self, more: Holder) {
        for reason in more.reasons {
            if !self.reasons.contains(&reason) {
                self.reasons.push(reason);
            }
        }
        if self.window.is_none() {
            self.window = more.window;
            self.title = more.title;
        }
        self.pid = self.pid.or(more.pid);
    }
}

/// What the shell knows to name a holder by.
pub struct Known<'a> {
    /// The windows open.
    pub wm: &'a Wm,
    /// The installed apps' desktop entries.
    pub apps: Option<&'a AppIndex>,
    /// A process's program (`/proc/PID/exe`), where it can be read.
    pub program: &'a dyn Fn(Pid) -> Option<String>,
    /// The shell's own process, whose inhibitor stands for the bus's
    /// holds.
    pub shell: Pid,
    /// The shell this one replaces, in a restart. It holds its own such
    /// inhibitor until the compositor stops it, seconds later.
    pub shell_before: Option<Pid>,
}

impl Known<'_> {
    /// A window of process `pid`'s own. An X11 app has none: its windows
    /// are xwayland-satellite's on the Wayland side.
    fn window_of(&self, pid: Pid) -> Option<&WindowInfo> {
        let pid = pid.to_ipc()?;
        self.wm.most_recent_window_matching(|w| w.pid == Some(pid))
    }

    /// The desktop entry a window's app ID names.
    fn entry_of_window(&self, window: &WindowInfo) -> Option<&App> {
        self.apps?.match_window(&window.app_id)
    }

    /// The desktop entry that runs `program` (a path or a file name), when
    /// exactly one does.
    fn entry_of_program(&self, program: &str) -> Option<&App> {
        self.apps?.match_program_identity(program)
    }

    /// A holder for one hold. `window` is the holder's own, when it has
    /// one; `said` what it called itself to the bus; `pid` its process.
    fn holder(&self, window: Option<&WindowInfo>, said: &str, pid: Option<Pid>, fallback: HolderKey) -> Holder {
        let program = pid.and_then(|p| (self.program)(p));
        let entry = window
            .and_then(|w| self.entry_of_window(w))
            .or_else(|| self.entry_of_program(said))
            .or_else(|| program.as_deref().and_then(|p| self.entry_of_program(p)))
            // Not a program's name: a desktop ID, as a sandboxed app's
            // portal gives it.
            .or_else(|| self.apps?.match_window_identity(said));
        let window_app = window.map(|w| w.app_id.as_str()).filter(|a| !a.is_empty());
        let (key, name) = match (entry, window_app) {
            (Some(entry), _) => (HolderKey::Entry(entry.id.clone()), entry.name.clone()),
            (None, Some(app_id)) => {
                (HolderKey::Named(app_id.to_lowercase()), catalog::display_name(None, None, app_id))
            }
            (None, None) => {
                let goes_by = [program_name(said), program.as_deref().map(program_name).unwrap_or_default()]
                    .into_iter()
                    .find(|name| !name.is_empty());
                match goes_by {
                    Some(name) => (HolderKey::Named(name.to_lowercase()), name.to_owned()),
                    None => (fallback, "An app".to_owned()),
                }
            }
        };
        let glyph = entry
            .and_then(|e| catalog::glyph_for(e.app_id()))
            .or_else(|| window_app.and_then(catalog::glyph_for))
            .unwrap_or_else(|| {
                (entry.map_or(Icon::AppWindow, |e| catalog::glyph_by_category(&e.categories)), Hue::Text2)
            });
        // The window to bring forward: its own, else the one a switch to
        // the app would pick (an X11 app's, which satellite holds).
        let window = window.or_else(|| entry.and_then(|e| self.wm.app_window_to_activate(e.app_id(), self.apps)));
        Holder {
            key,
            name,
            glyph,
            reasons: Vec::new(),
            window: window.map(|w| w.id),
            title: window.map(|w| w.title.trim().to_owned()).filter(|t| !t.is_empty()),
            pid,
        }
    }

    /// The app behind one of the compositor's idle inhibitors; none for
    /// Keep Awake's hold and for a shell's inhibitor: this one's, or in a
    /// restart the one's it replaces.
    fn of_inhibitor(&self, inhibitor: &IdleInhibitorInfo) -> Option<Holder> {
        let pid = inhibitor.pid.and_then(Pid::from_ipc);
        let a_shells = pid.is_some_and(|p| p == self.shell || Some(p) == self.shell_before);
        if inhibitor.kind != IdleHolder::App || a_shells {
            return None;
        }
        // The window the compositor names, as the shell knows it now;
        // else one of the process's (the inhibitor is on another surface).
        let window =
            inhibitor.window.and_then(|id| self.wm.windows.get(&id)).or_else(|| pid.and_then(|p| self.window_of(p)));
        Some(self.holder(window, "", pid, pid.map_or(HolderKey::Surface, HolderKey::Process)))
    }

    /// The app behind one of the session bus's holds.
    fn of_bus_hold(&self, hold: &Hold) -> Holder {
        let pid = hold.pid.and_then(Pid::new);
        let window = pid.and_then(|p| self.window_of(p));
        let fallback = pid.map_or_else(|| HolderKey::Peer(hold.peer.as_str().to_owned()), HolderKey::Process);
        let mut holder = self.holder(window, &hold.app, pid, fallback);
        let reason = hold.reason.trim();
        if !reason.is_empty() {
            holder.reasons.push(reason.to_owned());
        }
        holder
    }
}

/// The apps holding the screen on, by name: the compositor's inhibitors
/// and the bus's holds as one list, an app that holds both ways once.
pub fn merge(inhibitors: &[IdleInhibitorInfo], bus: &[Hold], known: &Known<'_>) -> Vec<Holder> {
    let mut holders: Vec<Holder> = Vec::new();
    let found = inhibitors.iter().filter_map(|i| known.of_inhibitor(i)).chain(bus.iter().map(|h| known.of_bus_hold(h)));
    for holder in found {
        match holders.iter_mut().find(|h| h.key == holder.key) {
            Some(same) => same.also(holder),
            None => holders.push(holder),
        }
    }
    holders.sort_by(|a, b| (a.name.to_lowercase(), &a.key).cmp(&(b.name.to_lowercase(), &b.key)));
    holders
}

/// Which of the cup's timers one is: a later one of the same kind
/// overtakes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Serial(u64);

/// A timer of the cup's, to be told of when it comes due.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Due {
    /// Apps have held for [`SHOWS_AFTER`].
    Lasted(Serial),
    /// The holder of this key let go [`LINGERS`] ago.
    Lingered(HolderKey, Serial),
}

/// The holders as the cup and its panel show them: each for a moment
/// after its hold goes, and the cup only once they have held a while.
/// It keeps no clock: timers come back as [`Due`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Showing {
    /// Who holds now.
    holding: Vec<Holder>,
    /// Who held until a moment ago, each with its wait's timer.
    lingering: Vec<(Holder, Serial)>,
    run: Run,
    timers: u64,
}

/// Where the cup stands in the run of holding under way: from the first
/// hold after none, until the last has lingered out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Run {
    /// Nothing holds, and nothing did a moment ago.
    #[default]
    Over,
    /// Apps hold, or did a moment ago, but not yet for [`SHOWS_AFTER`]:
    /// the timer that says when.
    Young(Serial),
    /// They have for [`SHOWS_AFTER`], with nothing holding at the moment
    /// it was up (a gap): the cup comes when a hold is back.
    Ripe,
    /// The cup shows.
    Shown,
}

impl Showing {
    fn next_timer(&mut self) -> Serial {
        self.timers += 1;
        Serial(self.timers)
    }

    /// Who holds now, merged anew. Returns the timers to start.
    pub fn update(&mut self, holding: Vec<Holder>) -> Vec<(Duration, Due)> {
        let mut timers = Vec::new();
        // Whoever let go stays listed a moment, unless it's back already.
        for gone in std::mem::take(&mut self.holding) {
            if !holding.iter().any(|h| h.key == gone.key) {
                let timer = self.next_timer();
                timers.push((LINGERS, Due::Lingered(gone.key.clone(), timer)));
                self.lingering.push((gone, timer));
            }
        }
        // Back, or the same process under a better name (its window
        // opened): the old row has nothing left to wait for.
        self.lingering
            .retain(|(was, _)| !holding.iter().any(|h| h.key == was.key || (h.pid.is_some() && h.pid == was.pid)));
        self.holding = holding;
        if self.run == Run::Over && !self.holding.is_empty() {
            let timer = self.next_timer();
            self.run = Run::Young(timer);
            timers.push((SHOWS_AFTER, Due::Lasted(timer)));
        }
        self.settle();
        timers
    }

    /// A timer came due. One that was overtaken (the holder came back,
    /// the run ended) changes nothing.
    pub fn due(&mut self, due: Due) {
        match due {
            Due::Lasted(timer) => {
                if self.run == Run::Young(timer) {
                    self.run = Run::Ripe;
                }
            }
            Due::Lingered(key, timer) => self.lingering.retain(|(was, waited)| !(was.key == key && *waited == timer)),
        }
        self.settle();
    }

    /// The run ends when nothing is left at all, and the cup comes once
    /// the run has lasted and something holds (not while all that's left
    /// is lingering).
    fn settle(&mut self) {
        self.run = match self.run {
            _ if self.count() == 0 => Run::Over,
            Run::Ripe if !self.holding.is_empty() => Run::Shown,
            run => run,
        };
    }

    /// Whether the cup shows for apps.
    pub fn shown(&self) -> bool {
        self.run == Run::Shown
    }

    /// The rows: who holds now and who held until a moment ago, by name.
    pub fn listed(&self) -> Vec<&Holder> {
        let mut rows: Vec<&Holder> = self.holding.iter().chain(self.lingering.iter().map(|(h, _)| h)).collect();
        rows.sort_by(|a, b| (a.name.to_lowercase(), &a.key).cmp(&(b.name.to_lowercase(), &b.key)));
        rows
    }

    /// How many rows there are.
    pub fn count(&self) -> usize {
        self.holding.len() + self.lingering.len()
    }
}

impl Shell {
    /// The compositor says who holds idleness off.
    pub fn idle_inhibitors_changed(&mut self, inhibitors: Vec<IdleInhibitorInfo>) {
        if self.inhibitors != inhibitors {
            self.inhibitors = inhibitors;
            self.holders_stale = true;
        }
    }

    /// The session bus's holds, as the screen saver the shell serves has
    /// them now.
    pub fn screen_holds_changed(&mut self, holds: Vec<Hold>) {
        if self.screen_holds != holds {
            self.screen_holds = holds;
            self.holders_stale = true;
        }
    }

    /// What holders are named by changed (a window, the installed apps):
    /// their rows are made again, if there are any.
    pub(crate) fn holders_may_be_renamed(&mut self) {
        self.holders_stale |= !self.inhibitors.is_empty() || !self.screen_holds.is_empty();
    }

    /// Merges the holders anew when what they're merged from changed, and
    /// starts the timers that asks for.
    pub(crate) fn sync_holders(&mut self, cx: &mut Cx<Msg>) {
        if !std::mem::take(&mut self.holders_stale) {
            return;
        }
        let (fixture, live) = (self.programs_fixture.as_ref(), self.opts.live);
        let program = |pid: Pid| match fixture {
            Some(programs) => programs.get(&pid.get()).cloned(),
            None if live => {
                let exe = std::fs::read_link(format!("/proc/{}/exe", pid.get())).ok()?;
                let exe = exe.to_string_lossy();
                // A program replaced on disk while it runs.
                Some(exe.strip_suffix(" (deleted)").unwrap_or(&exe).to_owned())
            }
            None => None,
        };
        let known = Known {
            wm: &self.wm,
            apps: self.apps.as_deref(),
            program: &program,
            shell: Pid::of_shell(),
            shell_before: self.shell_before,
        };
        let holding = merge(&self.inhibitors, &self.screen_holds, &known);
        for (delay, due) in self.holders.update(holding) {
            cx.after(delay, Msg::Holders(due));
        }
        self.cup_changed(cx);
    }

    /// A timer of the cup's came due.
    pub(crate) fn holders_due(&mut self, due: Due, cx: &mut Cx<Msg>) {
        self.holders.due(due);
        self.cup_changed(cx);
    }

    /// What the cup at the front of the systray shows for now: Keep Awake
    /// while it's on, else the apps holding the screen on, once they have
    /// held a while.
    pub fn cup(&self) -> Option<Cup> {
        match self.awake {
            aizome_awake::Status::On => Some(Cup::KeepAwake),
            aizome_awake::Status::Unprotected => Some(Cup::Unprotected),
            aizome_awake::Status::Off => self.holders.shown().then(|| Cup::Apps(self.holders.count())),
        }
    }

    /// Keep Awake or the holders changed: when the cup goes, so does its
    /// panel.
    pub(crate) fn cup_changed(&mut self, cx: &mut Cx<Msg>) {
        if self.cup().is_none() && self.panel.as_ref().is_some_and(|p| p.widget == TrayWidget::Awake) {
            self.close_panel(cx);
        }
    }

    /// Keep Awake, the cup and who holds the screen on, for `aizome-shell
    /// msg state`.
    pub fn awake_json(&self) -> serde_json::Value {
        let holders: Vec<serde_json::Value> = self
            .holders
            .listed()
            .into_iter()
            .map(|h| {
                serde_json::json!({
                    "name": h.name,
                    "detail": h.detail(),
                    "reasons": h.reasons,
                    "window": h.window,
                    "pid": h.pid.map(Pid::get),
                })
            })
            .collect();
        serde_json::json!({
            "keep_awake": match self.awake {
                aizome_awake::Status::Off => "off",
                aizome_awake::Status::On => "on",
                aizome_awake::Status::Unprotected => "unprotected",
            },
            "cup": self.cup().map(|cup| match cup {
                Cup::KeepAwake => "keep-awake",
                Cup::Unprotected => "unprotected",
                Cup::Apps(_) => "apps",
            }),
            "holders": holders,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_sys::apps::{Locale, XdgDirs};
    use aizome_sys::screensaver::Holds;

    /// Desktop entries as Nixpkgs ships them for these apps, and a
    /// sandboxed one with a reverse-DNS ID.
    fn apps() -> AppIndex {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "aizome-shell-holders-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let dirs = XdgDirs::rooted(&root);
        let dir = dirs.data_home.join("applications");
        std::fs::create_dir_all(&dir).unwrap();
        for (id, name, exec, extra) in [
            ("mpv", "mpv Media Player", "mpv --player-operation-mode=pseudo-gui -- %U", ""),
            ("vlc", "VLC media player", "/nix/store/abc-vlc-3.0.21/bin/vlc --started-from-file %U", ""),
            ("firefox", "Firefox", "firefox --name firefox %U", ""),
            ("chromium-browser", "Chromium", "chromium %U", "StartupWMClass=chromium-browser\n"),
            ("google-chrome", "Google Chrome", "/nix/store/abc-google-chrome-157/bin/google-chrome-stable %U", ""),
            ("org.aizome.Utsushi", "Utsushi", "aizome-utsushi %U", ""),
            ("org.example.Talk", "Talk", "flatpak run org.example.Talk", ""),
        ] {
            std::fs::write(
                dir.join(format!("{id}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}"),
            )
            .unwrap();
        }
        let index = AppIndex::load_with(dirs, Locale::default());
        std::fs::remove_dir_all(root).unwrap();
        index
    }

    fn window(id: WindowId, app_id: &str, title: &str, pid: i32) -> WindowInfo {
        WindowInfo {
            id,
            app_id: app_id.into(),
            title: title.into(),
            pid: Some(pid),
            desk: Some(1),
            ..Default::default()
        }
    }

    fn wm(windows: Vec<WindowInfo>) -> Wm {
        let mut wm = Wm::default();
        wm.reset(crate::wm::Snapshot { windows, current_desk: 1, ..Default::default() });
        wm
    }

    /// xwayland-satellite's process, which has every X11 app's windows.
    const SATELLITE: i32 = 900;
    const SHELL: u32 = 700;

    fn inhibitor(window: Option<WindowId>, pid: i32) -> IdleInhibitorInfo {
        IdleInhibitorInfo { kind: IdleHolder::App, window, pid: Some(pid), ..Default::default() }
    }

    fn bus(holds: &[(&str, u32, &str, &str)]) -> Vec<Hold> {
        let mut table = Holds::default();
        for (peer, pid, app, reason) in holds {
            let peer = aizome_sys::screensaver::Peer::new(peer).unwrap();
            table.take(peer, Pid::new(*pid).map(Pid::get), app, reason).unwrap();
        }
        table.state().holds
    }

    fn programs(pid: Pid) -> Option<String> {
        Some(
            match pid.get() {
                2100 => "/nix/store/abc-vlc-3.0.21/bin/.vlc-wrapped",
                2200 => "/nix/store/abc-firefox-159.0.1/lib/firefox/firefox",
                2300 => "/nix/store/abc-chromium-142/libexec/chromium/chromium",
                2400 => "/nix/store/abc-google-chrome-157/share/google/chrome/chrome",
                2500 => "/nix/store/abc-xdg-desktop-portal-gtk-1.15/libexec/xdg-desktop-portal-gtk",
                2600 => "/nix/store/abc-mpv-0.40/bin/.mpv-wrapped",
                _ => return None,
            }
            .to_owned(),
        )
    }

    fn merged(wm: &Wm, inhibitors: &[IdleInhibitorInfo], holds: &[Hold]) -> Vec<Holder> {
        merged_after(None, wm, inhibitors, holds)
    }

    /// As a shell that replaced `shell_before` merges them.
    fn merged_after(
        shell_before: Option<u32>,
        wm: &Wm,
        inhibitors: &[IdleInhibitorInfo],
        holds: &[Hold],
    ) -> Vec<Holder> {
        let apps = apps();
        let known = Known {
            wm,
            apps: Some(&apps),
            program: &programs,
            shell: Pid::new(SHELL).unwrap(),
            shell_before: shell_before.and_then(Pid::new),
        };
        merge(inhibitors, holds, &known)
    }

    fn rows(holders: &[Holder]) -> Vec<(String, String)> {
        holders.iter().map(|h| (h.name.clone(), h.detail())).collect()
    }

    fn row(name: &str, detail: &str) -> (String, String) {
        (name.to_owned(), detail.to_owned())
    }

    #[test]
    fn a_wayland_player_is_named_by_its_window_and_a_bus_caller_by_its_program() {
        let wm = wm(vec![
            window(1, "mpv", "Test Pattern - mpv", 2600),
            window(2, "org.aizome.Utsushi", "Test Pattern", 2700),
            window(3, "vlc", "Test Pattern - VLC media player", SATELLITE),
            window(4, "firefox", "A Talk — Mozilla Firefox", 2200),
        ]);
        let holders = merged(
            &wm,
            &[inhibitor(Some(1), 2600), inhibitor(Some(2), 2700)],
            &bus(&[(":1.40", 2100, "vlc", "Playing some media."), (":1.52", 2200, "firefox", "Playing video")]),
        );
        assert_eq!(
            rows(&holders),
            [
                row("Firefox", "Playing video"),
                // A Wayland idle inhibitor gives no reason: which window, then.
                row("mpv Media Player", "Test Pattern - mpv"),
                row("Utsushi", "Test Pattern"),
                row("VLC media player", "Playing some media."),
            ]
        );
        let by_name = |name: &str| holders.iter().find(|h| h.name == name).unwrap();
        // Each can be reached: its own window, or for the X11 app the one
        // a switch to it would show (satellite has it on the Wayland side).
        assert_eq!(by_name("mpv Media Player").window, Some(1));
        assert_eq!(by_name("Utsushi").window, Some(2));
        assert_eq!(by_name("VLC media player").window, Some(3));
        assert_eq!(by_name("Firefox").window, Some(4));
        // And found in Myaku by its own process, never satellite's.
        assert_eq!(by_name("VLC media player").pid, Pid::new(2100));
        assert_eq!(by_name("mpv Media Player").pid, Pid::new(2600));
        assert_eq!(by_name("VLC media player").key, HolderKey::Entry("vlc.desktop".into()));
        assert_eq!(by_name("Utsushi").glyph, (Icon::Film, Hue::Red));
    }

    #[test]
    fn an_app_that_holds_both_ways_is_one_row_with_its_reason() {
        // Chrome with the keys: its window's inhibitor, and the bus under
        // its program's path.
        let wm = wm(vec![window(5, "google-chrome", "A Film - Google Chrome", 2400)]);
        let chrome = "/nix/store/abc-google-chrome-157/share/google/chrome/chrome";
        let holders = merged(&wm, &[inhibitor(Some(5), 2400)], &bus(&[(":1.61", 2400, chrome, "Video Wake Lock")]));
        assert_eq!(rows(&holders), [row("Google Chrome", "Video Wake Lock")]);
        assert_eq!(holders[0].window, Some(5));
        assert_eq!(holders[0].key, HolderKey::Entry("google-chrome.desktop".into()));
        // Two videos in it, and so two holds of the bus's: still one row,
        // its reason said once.
        let twice = bus(&[(":1.61", 2400, chrome, "Video Wake Lock"), (":1.61", 2400, chrome, "Video Wake Lock")]);
        assert_eq!(rows(&merged(&wm, &[inhibitor(Some(5), 2400)], &twice)), [row("Google Chrome", "Video Wake Lock")]);
        // Firefox playing a video with sound has one for each.
        let both = bus(&[(":1.52", 2200, "firefox", "Playing audio"), (":1.52", 2200, "firefox", "Playing video")]);
        assert_eq!(rows(&merged(&wm, &[], &both)), [row("Firefox", "Playing audio, Playing video")]);
    }

    #[test]
    fn a_program_path_is_not_shown_where_a_name_can_be_found() {
        // Chromium as an X11 client: no window of its own process, and its
        // program's path for a name. Its desktop entry runs that program.
        let wm = wm(vec![window(6, "chromium-browser", "video.html - Chromium", SATELLITE)]);
        let chromium = "/nix/store/abc-chromium-142/libexec/chromium/chromium";
        let holders = merged(&wm, &[], &bus(&[(":1.70", 2300, chromium, "Video Wake Lock")]));
        assert_eq!(rows(&holders), [row("Chromium", "Video Wake Lock")]);
        assert_eq!(holders[0].window, Some(6), "the window a switch to Chromium shows");
        // No desktop entry runs it: the program's file name, not its path.
        let unknown = merged(&wm, &[], &bus(&[(":1.71", 0, "/opt/kiosk/bin/.kiosk-player-wrapped", "Looping")]));
        assert_eq!(rows(&unknown), [row("kiosk-player", "Looping")]);
        assert_eq!(unknown[0].key, HolderKey::Named("kiosk-player".into()));
        assert_eq!((unknown[0].window, unknown[0].pid), (None, None));
    }

    #[test]
    fn the_portal_names_the_app_it_asks_for_or_itself() {
        let wm = wm(Vec::new());
        // For a sandboxed app the portal's backend gives the app's ID; for
        // one that isn't, nothing, and then the backend is who holds.
        let holds = bus(&[(":1.80", 2500, "org.example.Talk", "In a call"), (":1.80", 2500, "", "a film")]);
        assert_eq!(
            rows(&merged(&wm, &[], &holds)),
            [row("Talk", "In a call"), row("xdg-desktop-portal-gtk", "a film")]
        );
        // Nothing said and no process known: still a row, and its own.
        let nameless = merged(&wm, &[], &bus(&[(":1.81", 0, "", ""), (":1.82", 0, "", "")]));
        assert_eq!(rows(&nameless), [row("An app", ""), row("An app", "")]);
        assert_ne!(nameless[0].key, nameless[1].key);
    }

    #[test]
    fn the_shells_own_inhibitor_and_keep_awakes_hold_are_nobodys_row() {
        let wm = wm(vec![window(3, "vlc", "Test Pattern - VLC media player", SATELLITE)]);
        // While VLC holds on the bus the shell holds an inhibitor for it,
        // and Keep Awake is on besides.
        let keep_awake = IdleInhibitorInfo { kind: IdleHolder::KeepAwake, pid: Some(1500), ..Default::default() };
        let inhibitors = [inhibitor(None, SHELL as i32), keep_awake];
        let holders = merged(&wm, &inhibitors, &bus(&[(":1.40", 2100, "vlc", "Playing some media.")]));
        assert_eq!(rows(&holders), [row("VLC media player", "Playing some media.")]);
        assert!(merged(&wm, &inhibitors, &[]).is_empty());
        // Another client's bare surface is somebody's, though: named by
        // its program.
        assert_eq!(rows(&merged(&wm, &[inhibitor(None, 2600)], &[])), [row("mpv Media Player", "")]);
        // And with nothing to name it, by nothing but its process.
        let unnamed = merged(&wm, &[inhibitor(None, 4000)], &[]);
        assert_eq!(rows(&unnamed), [row("An app", "")]);
        assert_eq!(unnamed[0].key, HolderKey::Process(Pid::new(4000).unwrap()));
    }

    #[test]
    fn in_a_restart_the_inhibitor_of_the_shell_before_is_nobodys_row() {
        // The shell was restarted while VLC held on the bus. The shell
        // before still runs for a few seconds, with the inhibitor it held
        // for VLC; mpv plays too.
        const BEFORE: u32 = 650;
        let wm = wm(vec![window(1, "mpv", "Test Pattern - mpv", 2600)]);
        let inhibitors = [inhibitor(None, BEFORE as i32), inhibitor(Some(1), 2600)];
        let restarted = merged_after(Some(BEFORE), &wm, &inhibitors, &[]);
        assert_eq!(rows(&restarted), [row("mpv Media Player", "Test Pattern - mpv")]);
        // To a shell that replaced nobody, that process is some client.
        assert_eq!(merged(&wm, &inhibitors, &[]).len(), 2);
    }

    fn holder(name: &str) -> Holder {
        Holder {
            key: HolderKey::Named(name.to_lowercase()),
            name: name.into(),
            glyph: (Icon::AppWindow, Hue::Text2),
            reasons: Vec::new(),
            window: None,
            title: None,
            pid: None,
        }
    }

    /// Runs the timers `update` asked for as a clock would: each comes due
    /// its delay after it was started.
    #[derive(Default)]
    struct Clock {
        now: Duration,
        timers: Vec<(Duration, Due)>,
        showing: Showing,
    }

    impl Clock {
        fn holds(&mut self, names: &[&str]) {
            let timers = self.showing.update(names.iter().map(|n| holder(n)).collect());
            self.timers.extend(timers.into_iter().map(|(delay, due)| (self.now + delay, due)));
        }

        fn pass(&mut self, time: Duration) {
            let until = self.now + time;
            self.timers.sort_by_key(|(at, _)| *at);
            while self.timers.first().is_some_and(|(at, _)| *at <= until) {
                let (at, due) = self.timers.remove(0);
                self.now = at;
                self.showing.due(due);
            }
            self.now = until;
        }

        fn names(&self) -> Vec<&str> {
            self.showing.listed().into_iter().map(|h| h.name.as_str()).collect()
        }
    }

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn the_cup_waits_for_a_hold_to_last_and_lingers_after_the_last() {
        let mut c = Clock::default();
        c.holds(&["mpv"]);
        // Listed at once, for a panel that's open; the cup waits.
        assert_eq!(c.names(), ["mpv"]);
        c.pass(SHOWS_AFTER - MS);
        assert!(!c.showing.shown(), "the cup before the hold had lasted");
        c.pass(MS);
        assert!(c.showing.shown());
        // Let go: the cup and the row stay a moment, then both go.
        c.holds(&[]);
        c.pass(LINGERS - MS);
        assert!(c.showing.shown(), "the cup went with the hold");
        assert_eq!(c.names(), ["mpv"]);
        c.pass(MS);
        assert!(!c.showing.shown());
        assert!(c.names().is_empty() && c.showing.count() == 0);
    }

    #[test]
    fn a_hold_that_ends_at_once_never_shows_the_cup() {
        let mut c = Clock::default();
        // Played and paused within a second.
        c.holds(&["firefox"]);
        c.pass(1000 * MS);
        c.holds(&[]);
        // The hold's three seconds come while only its row lingers.
        c.pass(10_000 * MS);
        assert!(!c.showing.shown());
        assert_eq!(c.showing.count(), 0);
        // A later hold starts its own wait.
        c.holds(&["firefox"]);
        c.pass(SHOWS_AFTER - MS);
        assert!(!c.showing.shown(), "an earlier hold's time counted for this one");
        c.pass(MS);
        assert!(c.showing.shown());
    }

    #[test]
    fn a_hold_dropped_and_taken_again_at_once_never_left() {
        let mut c = Clock::default();
        // VLC looping a two-second clip: at each loop it lets go and
        // holds again 60 ms later. The gaps don't start the wait over…
        c.holds(&["vlc"]);
        for _ in 0..3 {
            c.pass(2000 * MS);
            c.holds(&[]);
            assert_eq!(c.names(), ["vlc"], "the row blinked");
            c.pass(60 * MS);
            c.holds(&["vlc"]);
        }
        assert!(c.showing.shown(), "the gaps kept the cup from ever showing");
        // …nor blink the cup once it shows.
        for _ in 0..3 {
            c.pass(2000 * MS);
            c.holds(&[]);
            assert!(c.showing.shown(), "the cup blinked");
            c.pass(60 * MS);
            c.holds(&["vlc"]);
            assert!(c.showing.shown());
        }
        assert_eq!(c.names(), ["vlc"], "a gap left a second row");
        // A gap's wait ends when the hold is back: long after, the old
        // timers have taken nothing away.
        c.pass(60_000 * MS);
        assert!(c.showing.shown());
        assert_eq!(c.names(), ["vlc"]);
    }

    #[test]
    fn the_cup_comes_when_the_hold_is_back_if_its_time_came_in_a_gap() {
        let mut c = Clock::default();
        c.holds(&["vlc"]);
        c.pass(SHOWS_AFTER - 30 * MS);
        c.holds(&[]);
        // Three seconds are up mid-gap, with nothing holding: not yet.
        c.pass(60 * MS);
        assert!(!c.showing.shown());
        c.holds(&["vlc"]);
        assert!(c.showing.shown());
    }

    #[test]
    fn each_holder_lingers_for_itself_and_the_cup_for_the_last() {
        let mut c = Clock::default();
        c.holds(&["mpv", "vlc"]);
        c.pass(SHOWS_AFTER);
        assert!(c.showing.shown());
        // mpv pauses; VLC plays on.
        c.holds(&["vlc"]);
        assert_eq!(c.names(), ["mpv", "vlc"]);
        c.pass(LINGERS);
        assert_eq!(c.names(), ["vlc"]);
        assert!(c.showing.shown());
        // A second app joining a run that has lasted shows at once.
        c.holds(&["firefox", "vlc"]);
        assert_eq!(c.names(), ["firefox", "vlc"]);
        c.holds(&[]);
        c.pass(LINGERS - MS);
        assert!(c.showing.shown());
        c.pass(MS);
        assert!(!c.showing.shown());
    }

    #[test]
    fn a_holder_under_a_better_name_takes_its_old_rows_place() {
        // mpv's inhibitor comes before its window: first a process, then
        // the app once the window is known.
        let pid = Pid::new(2600);
        let early = Holder { key: HolderKey::Process(pid.unwrap()), pid, ..holder("An app") };
        let named = Holder { pid, ..holder("mpv Media Player") };
        let mut showing = Showing::default();
        showing.update(vec![early]);
        showing.update(vec![named]);
        let names: Vec<&str> = showing.listed().into_iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["mpv Media Player"]);
    }

    #[test]
    fn the_waits_are_seconds_and_a_players_gap_fits_well_inside_the_linger() {
        // VLC's gaps between holds measured about 60 ms, and a pause and
        // play by hand takes about a second: both are well inside.
        assert!(LINGERS >= 20 * 60 * MS, "{LINGERS:?}");
        // Still "a few seconds" and "briefly": the cup is there while a
        // video is watched, and gone soon after it stops.
        assert!((Duration::from_secs(2)..=Duration::from_secs(5)).contains(&SHOWS_AFTER), "{SHOWS_AFTER:?}");
        assert!(LINGERS <= Duration::from_secs(3), "{LINGERS:?}");
    }

    #[test]
    fn a_rows_click_asks_for_its_window_and_its_button_for_myaku_and_nothing_else() {
        use crate::launch::Launch;
        use crate::systray::panels::PanelMsg;
        let mut h = aizome_kit::testing::Harness::new(Shell::new(crate::shell::Options::default()));
        let (ipc, requests) = crate::ipc::Ipc::recording();
        h.program_mut().ipc = ipc;
        // A live shell records what it launches; nothing starts here, as
        // no activation token comes back.
        h.program_mut().opts.live = true;

        h.send(Msg::Panel(PanelMsg::ShowHolder(21)));
        assert_eq!(requests.try_recv().unwrap(), (aizome_ipc::Request::Focus { id: 21 }, None));
        h.send(Msg::Panel(PanelMsg::HolderInMyaku(Pid::new(2100).unwrap())));
        let launches: Vec<Launch> = h.program().launches.values().map(|(launch, _)| launch.clone()).collect();
        assert_eq!(launches, [Launch::Argv(vec!["aizome-myaku".into(), "--pid".into(), "2100".into()])]);
        // The compositor was asked for the window and nothing more: no
        // window closed, no hold let go.
        assert!(requests.try_recv().is_err());
    }
}
