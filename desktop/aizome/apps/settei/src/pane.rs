//! The pane contract: how a pane plugs into Settei's window.
//!
//! Settei is a window of panes, as macOS's System Settings is: the
//! sidebar lists them in groups, the chosen one fills the right, and search
//! finds panes and the options inside them. Each pane is a type that
//! implements [`Pane`]; `panes/mod.rs` lists them, one line each.
//!
//! # What a pane is
//!
//! - **Identity**: an [`id`](Pane::id) (`"keyboard"`: what `--pane
//!   keyboard` opens and what the palette's entries carry), a
//!   [`title`](Pane::title), a Lucide [`icon`](Pane::icon) on a
//!   [`tile`](Pane::tile) colour, and the sidebar [`group`](Pane::group).
//! - **Entries**: [`entries`](Pane::entries) lists the options people
//!   search for, each a [`SettingEntry`] with an id unique in the pane
//!   (`"caps-lock"`), the title its row shows, and keywords.
//!   `--list-settings --json` prints them without opening a window, so
//!   `entries` must not touch the system, and the pane's constructor must
//!   be cheap (no D-Bus, no files: load in [`shown`](Pane::shown)).
//! - **Messages**: the pane's own [`Msg`](Pane::Msg) type. The window
//!   carries them as an `Arc<dyn Any>` and hands them back typed to
//!   [`update`](Pane::update).
//! - **View**: [`view`](Pane::view) returns the pane's form typed with its
//!   own `Msg` (the window maps it into its own with `Element::map`). Build
//!   it with [`crate::form`]: `vcx.section("Key Repeat", rows)`, each row
//!   marked `.entry("repeat-delay")` so search highlights and reveals it.
//! - **Life**: [`shown`](Pane::shown) when the pane comes into view
//!   (refresh, start timers), [`hidden`](Pane::hidden) when another
//!   replaces it (stop them), [`focus_entry`](Pane::focus_entry) when search
//!   or `--pane keyboard.caps-lock` asks for an option (switch to the
//!   sub-page that holds it; the window then reveals its row).
//!
//! # What a pane acts with
//!
//! [`PaneCx`], in `update`, `shown` and the rest:
//!
//! - [`settings`](PaneCx::settings) and [`change`](PaneCx::change):
//!   `settings.toml` through `aizome-config`. A change is saved at once
//!   (read again first, so nobody else's edit is lost) and the compositor
//!   is asked to `reload-config`. Changes apply immediately: there are no
//!   Apply buttons.
//! - [`confirm`](PaneCx::confirm): a risky change asks first, in a sheet.
//! - [`wake`](PaneCx::wake): a [`Wake`] for the `aizome-sys` services a
//!   pane keeps (created in `shown`): when one changes, the window views
//!   again and the pane reads the new snapshot. [`sample`](PaneCx::sample)
//!   says to use a pane's fixtures instead of the live system (`--sample`,
//!   and every test): nothing a pane does in the sample changes the
//!   machine.
//! - [`spawn`](PaneCx::spawn), [`wait`](PaneCx::wait),
//!   [`stream`](PaneCx::stream), [`after`](PaneCx::after),
//!   [`every`](PaneCx::every): work off the main thread and timers, their
//!   results coming back as the pane's messages.
//! - [`say`](PaneCx::say) and [`failed`](PaneCx::failed): a line in the
//!   window's status strip.
//! - [`launch`](PaneCx::launch), [`compositor`](PaneCx::compositor),
//!   [`show_pane`](PaneCx::show_pane).
//!
//! Views read the same through [`ViewCx`]: `vcx.settings()`, `vcx.sample()`,
//! and `vcx.matches(entry)` for what search highlights.
//!
//! A setting NixOS pins (an `aizome-sys` snapshot's `*_managed`, or an
//! [`aizome_sys::Error::ManagedBySystem`]) shows
//! [`form::managed`](crate::form::managed), "Set by your system
//! configuration" with a line on how to change it, rather than failing.
//!
//! # A pane, whole
//!
//! ```ignore
//! #[derive(Default)]
//! pub struct Mouse;
//!
//! #[derive(Clone, Debug)]
//! pub enum Msg { Speed(f64), Natural(bool) }
//!
//! impl Pane for Mouse {
//!     type Msg = Msg;
//!     fn id(&self) -> &'static str { "mouse" }
//!     fn title(&self) -> &'static str { "Mouse" }
//!     fn icon(&self) -> Icon { Icon::Mouse }
//!     fn tile(&self) -> Tile { Tile::Slate }
//!     fn group(&self) -> Group { Group::Devices }
//!     fn entries(&self) -> Vec<SettingEntry> {
//!         vec![SettingEntry::new("speed", "Pointer speed").keywords(&["tracking", "acceleration"])]
//!     }
//!     fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
//!         match msg {
//!             Msg::Speed(v) => cx.change(|s| s.input.pointer.speed = v),
//!             Msg::Natural(on) => cx.change(|s| s.input.pointer.natural_scroll = on),
//!         }
//!     }
//!     fn view(&self, vcx: &ViewCx) -> Element<Msg> {
//!         let p = &vcx.settings().input.pointer;
//!         vcx.section("Pointer", vec![
//!             form::row("Pointer speed", slider(p.speed, -1.0..=1.0).on_change(Msg::Speed)).entry("speed"),
//!         ])
//!     }
//! }
//! ```

use std::any::Any;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aizome_config::Settings;
use aizome_kit::prelude::*;
use aizome_kit::{Cx, FileDialog, MsgSender, TimerHandle};
use aizome_sys::{Task, Wake};

use crate::app::AppMsg;
use crate::shared::Shared;

/// A pane's message on its way through the window.
pub type PaneMsg = Arc<dyn Any + Send + Sync>;

/// Where a pane sits in the sidebar. Groups show in this order, each pane
/// in the order `panes::all` lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    /// Network, Bluetooth.
    Connections,
    /// Notifications, Sound.
    Alerts,
    /// General, Appearance, Accessibility, Desktop & Deskbar, Menu Bar,
    /// Displays, Battery & Power, Performance, Search.
    System,
    /// Lock Screen, Users & Groups.
    Security,
    /// Keyboard, Mouse, Trackpad, Printers & Scanners.
    Devices,
}

impl Group {
    pub const ALL: [Group; 5] = [Group::Connections, Group::Alerts, Group::System, Group::Security, Group::Devices];

    /// The caption over the group in the sidebar.
    pub fn title(self) -> &'static str {
        match self {
            Group::Connections => "Connections",
            Group::Alerts => "Alerts",
            Group::System => "System",
            Group::Security => "Security",
            Group::Devices => "Devices",
        }
    }
}

/// The colour of a pane's glyph tile: a dye of the indigo ramp, one of the
/// palette's colours, or the focus colour (which follows the user's
/// choice). Resolved against the theme when drawn, so Night and Day each
/// get theirs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tile {
    /// 藍 ai, the selection indigo.
    Ai,
    /// 縹 hanada, the accent blue.
    Hanada,
    /// 甕覗 kamenozoki, the palest dye.
    Kamenozoki,
    /// 紺 kon, the deep navy.
    Kon,
    /// 琥珀 kohaku, amber (fixed: it doesn't follow focus).
    Kohaku,
    /// The user's focus colour.
    Focus,
    Green,
    Red,
    Violet,
    Cyan,
    /// A quiet grey, for hardware.
    Slate,
}

impl Tile {
    /// The tile's fill and its glyph's colour in `palette`.
    pub fn colors(self, p: &aizome_theme::Palette) -> (aizome_theme::Rgba, aizome_theme::Rgba) {
        use aizome_theme::Rgba;
        let day = p.name == aizome_theme::ThemeName::Day;
        let fill = match self {
            Tile::Ai => Rgba::hex(0x3d59a1),
            Tile::Hanada => {
                if day {
                    Rgba::hex(0x2e5fd0)
                } else {
                    Rgba::hex(0x7aa2f7)
                }
            }
            Tile::Kamenozoki => Rgba::hex(0xa9c4ff),
            Tile::Kon => {
                if day {
                    Rgba::hex(0x24283b)
                } else {
                    Rgba::hex(0x414868)
                }
            }
            Tile::Kohaku => {
                if day {
                    Rgba::hex(0xd99a2b)
                } else {
                    Rgba::hex(0xe0af68)
                }
            }
            Tile::Focus => p.focus,
            Tile::Green => {
                if day {
                    Rgba::hex(0x4f7a28)
                } else {
                    Rgba::hex(0x9ece6a)
                }
            }
            Tile::Red => {
                if day {
                    Rgba::hex(0xc4314b)
                } else {
                    Rgba::hex(0xf7768e)
                }
            }
            Tile::Violet => {
                if day {
                    Rgba::hex(0x7247c9)
                } else {
                    Rgba::hex(0xbb9af7)
                }
            }
            Tile::Cyan => {
                if day {
                    Rgba::hex(0x0b6f9c)
                } else {
                    Rgba::hex(0x7dcfff)
                }
            }
            Tile::Slate => {
                if day {
                    Rgba::hex(0x6f7598)
                } else {
                    Rgba::hex(0x565f89)
                }
            }
        };
        let light = Rgba::hex(0xffffff);
        let dark = Rgba::hex(0x16161e);
        let glyph = if fill.contrast(light) >= fill.contrast(dark) { light } else { dark };
        (fill, glyph)
    }
}

/// An option people can search for: what the palette lists and what the
/// sidebar's search finds and highlights.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingEntry {
    /// Unique within the pane: `"caps-lock"`. The window names its row
    /// `entry:<pane>.<id>` to reveal it.
    pub id: String,
    /// What its row says: `"Caps Lock key"`.
    pub title: String,
    /// Where it sits inside the pane, when that isn't the pane's first
    /// page: `["Date & Time"]` in General.
    pub under: Vec<String>,
    /// Other words it answers to: `["escape", "control", "remap"]`.
    pub keywords: Vec<String>,
}

impl SettingEntry {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> SettingEntry {
        SettingEntry { id: id.into(), title: title.into(), under: Vec::new(), keywords: Vec::new() }
    }

    pub fn keywords(mut self, words: &[&str]) -> SettingEntry {
        self.keywords.extend(words.iter().map(|w| (*w).to_owned()));
        self
    }

    /// Inside a sub-page (General's "Date & Time").
    pub fn under(mut self, page: &str) -> SettingEntry {
        self.under.push(page.to_owned());
        self
    }

    /// "Keyboard › Caps Lock key", given the pane's title.
    pub fn path(&self, pane_title: &str) -> String {
        let mut parts: Vec<&str> = vec![pane_title];
        parts.extend(self.under.iter().map(String::as_str));
        parts.push(&self.title);
        parts.join(" › ")
    }
}

/// A pane of Settei. See the module's documentation.
pub trait Pane: 'static {
    type Msg: Clone + Send + Sync + 'static;

    /// `"keyboard"`: what `--pane` takes and the palette's entries carry.
    fn id(&self) -> &'static str;

    /// `"Keyboard"`.
    fn title(&self) -> &'static str;

    /// The glyph on its tile.
    fn icon(&self) -> Icon;

    /// Its tile's colour.
    fn tile(&self) -> Tile;

    fn group(&self) -> Group;

    /// Other words the pane itself answers to (`"wifi"`, `"ethernet"`).
    fn keywords(&self) -> &'static [&'static str] {
        &[]
    }

    /// The options people search for. Mustn't touch the system.
    fn entries(&self) -> Vec<SettingEntry>;

    fn update(&mut self, msg: Self::Msg, cx: &mut PaneCx<Self::Msg>);

    /// The pane's form. The window gives it the room right of the sidebar,
    /// scrolled, at most [`crate::form::PAGE_WIDTH`] wide.
    fn view(&self, vcx: &ViewCx) -> Element<Self::Msg>;

    /// It came into view: read the system, start timers.
    fn shown(&mut self, cx: &mut PaneCx<Self::Msg>) {
        let _ = cx;
    }

    /// Another pane replaced it: stop timers.
    fn hidden(&mut self, cx: &mut PaneCx<Self::Msg>) {
        let _ = cx;
    }

    /// Search (or `--pane id.entry`) asks for an option: show the sub-page
    /// that holds it. The window reveals and highlights its row.
    fn focus_entry(&mut self, entry: &str, cx: &mut PaneCx<Self::Msg>) {
        let _ = (entry, cx);
    }

    /// A service the window shares, or one the pane keeps with
    /// [`PaneCx::wake`], changed: the window views again anyway; this is for
    /// a pane that keeps something derived from a snapshot.
    fn system_changed(&mut self, cx: &mut PaneCx<Self::Msg>) {
        let _ = cx;
    }

    /// `settings.toml` changed on disk, by another program or a hand edit
    /// (the window has read it again): for a pane that keeps something
    /// derived from it.
    fn settings_changed(&mut self, cx: &mut PaneCx<Self::Msg>) {
        let _ = cx;
    }

    /// Whether the sidebar lists it (a pane for hardware that's absent can
    /// hide; it still opens by `--pane`).
    fn listed(&self, vcx: &ViewCx) -> bool {
        let _ = vcx;
        true
    }

    /// The pane's title strip can carry a control at its right (a search
    /// field, a + button); this is it.
    fn toolbar(&self, vcx: &ViewCx) -> Option<Element<Self::Msg>> {
        let _ = vcx;
        None
    }

    /// A sheet's content, when the pane has one open (see
    /// [`PaneCx::open_sheet`]).
    fn sheet(&self, vcx: &ViewCx) -> Option<Element<Self::Msg>> {
        let _ = vcx;
        None
    }

    /// The sub-page showing, which the title strip puts after the pane's
    /// title ("General › About"); Back then returns from it.
    fn crumb(&self) -> Option<&'static str> {
        None
    }

    /// Escape or Back pressed (with no sheet open): whether the pane went
    /// back a page by itself (General's sub-pages do).
    fn back(&mut self, cx: &mut PaneCx<Self::Msg>) -> bool {
        let _ = cx;
        false
    }
}

/// A pane with its message type erased: what the window keeps.
pub trait AnyPane {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn icon(&self) -> Icon;
    fn tile(&self) -> Tile;
    fn group(&self) -> Group;
    fn keywords(&self) -> &'static [&'static str];
    fn entries(&self) -> Vec<SettingEntry>;
    fn update_any(&mut self, msg: PaneMsg, cx: &mut Cx<AppMsg>, shared: &mut Shared);
    fn view_any(&self, vcx: &ViewCx) -> Element<AppMsg>;
    fn toolbar_any(&self, vcx: &ViewCx) -> Option<Element<AppMsg>>;
    fn sheet_any(&self, vcx: &ViewCx) -> Option<Element<AppMsg>>;
    fn listed_any(&self, vcx: &ViewCx) -> bool;
    fn shown_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared);
    fn hidden_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared);
    fn focus_entry_any(&mut self, entry: &str, cx: &mut Cx<AppMsg>, shared: &mut Shared);
    fn back_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared) -> bool;
    fn crumb_any(&self) -> Option<&'static str>;
    fn system_changed_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared);
    fn settings_changed_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared);
}

fn wrap<M: Send + Sync + 'static>(pane: &'static str, msg: M) -> AppMsg {
    AppMsg::Pane(pane, Arc::new(msg))
}

impl<P: Pane> AnyPane for P {
    fn id(&self) -> &'static str {
        Pane::id(self)
    }
    fn title(&self) -> &'static str {
        Pane::title(self)
    }
    fn icon(&self) -> Icon {
        Pane::icon(self)
    }
    fn tile(&self) -> Tile {
        Pane::tile(self)
    }
    fn group(&self) -> Group {
        Pane::group(self)
    }
    fn keywords(&self) -> &'static [&'static str] {
        Pane::keywords(self)
    }
    fn entries(&self) -> Vec<SettingEntry> {
        Pane::entries(self)
    }

    fn update_any(&mut self, msg: PaneMsg, cx: &mut Cx<AppMsg>, shared: &mut Shared) {
        let Some(msg) = msg.downcast_ref::<P::Msg>().cloned() else {
            log::warn!("{}: a message of another type", Pane::id(self));
            return;
        };
        let id = Pane::id(self);
        let mut pcx = PaneCx::new(cx, shared, id);
        self.update(msg, &mut pcx);
    }

    fn view_any(&self, vcx: &ViewCx) -> Element<AppMsg> {
        let id = Pane::id(self);
        self.view(vcx).map(move |m: P::Msg| wrap(id, m))
    }

    fn toolbar_any(&self, vcx: &ViewCx) -> Option<Element<AppMsg>> {
        let id = Pane::id(self);
        self.toolbar(vcx).map(|e| e.map(move |m: P::Msg| wrap(id, m)))
    }

    fn sheet_any(&self, vcx: &ViewCx) -> Option<Element<AppMsg>> {
        let id = Pane::id(self);
        self.sheet(vcx).map(|e| e.map(move |m: P::Msg| wrap(id, m)))
    }

    fn listed_any(&self, vcx: &ViewCx) -> bool {
        self.listed(vcx)
    }

    fn shown_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared) {
        let id = Pane::id(self);
        self.shown(&mut PaneCx::new(cx, shared, id));
    }

    fn hidden_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared) {
        let id = Pane::id(self);
        self.hidden(&mut PaneCx::new(cx, shared, id));
    }

    fn focus_entry_any(&mut self, entry: &str, cx: &mut Cx<AppMsg>, shared: &mut Shared) {
        let id = Pane::id(self);
        self.focus_entry(entry, &mut PaneCx::new(cx, shared, id));
    }

    fn back_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared) -> bool {
        let id = Pane::id(self);
        self.back(&mut PaneCx::new(cx, shared, id))
    }

    fn crumb_any(&self) -> Option<&'static str> {
        self.crumb()
    }

    fn system_changed_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared) {
        let id = Pane::id(self);
        self.system_changed(&mut PaneCx::new(cx, shared, id));
    }

    fn settings_changed_any(&mut self, cx: &mut Cx<AppMsg>, shared: &mut Shared) {
        let id = Pane::id(self);
        self.settings_changed(&mut PaneCx::new(cx, shared, id));
    }
}

/// Wraps a pane for the registry: `pane(Keyboard::default())`.
pub fn pane<P: Pane>(p: P) -> Box<dyn AnyPane> {
    Box::new(p)
}

/// A risky change, asked about first: `Confirm::new("Remove “guest”?",
/// "Their files stay in /home/guest.", "Remove", Msg::Remove).destructive()`.
pub struct Confirm<Msg> {
    pub title: String,
    pub message: String,
    pub action: String,
    pub on_confirm: Msg,
    pub on_cancel: Option<Msg>,
    pub destructive: bool,
}

impl<Msg> Confirm<Msg> {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        action: impl Into<String>,
        on_confirm: Msg,
    ) -> Self {
        Confirm {
            title: title.into(),
            message: message.into(),
            action: action.into(),
            on_confirm,
            on_cancel: None,
            destructive: false,
        }
    }

    /// The action button in red: it removes or turns off something.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Also tell the pane when it's cancelled (to put a control back).
    pub fn on_cancel(mut self, msg: Msg) -> Self {
        self.on_cancel = Some(msg);
        self
    }
}

/// Sends a pane its messages from another thread: [`PaneCx::stream`].
pub struct PaneSender<Msg> {
    tx: MsgSender<AppMsg>,
    pane: &'static str,
    _m: PhantomData<fn(Msg)>,
}

impl<Msg> Clone for PaneSender<Msg> {
    fn clone(&self) -> Self {
        PaneSender { tx: self.tx.clone(), pane: self.pane, _m: PhantomData }
    }
}

impl<Msg: Send + Sync + 'static> PaneSender<Msg> {
    /// Whether the window is still there to get it.
    pub fn send(&self, msg: Msg) -> bool {
        self.tx.send(wrap(self.pane, msg))
    }
}

/// What a pane acts with. See the module's documentation.
pub struct PaneCx<'a, 'c, Msg> {
    cx: &'a mut Cx<'c, AppMsg>,
    shared: &'a mut Shared,
    pane: &'static str,
    _m: PhantomData<fn(Msg)>,
}

impl<'a, 'c, Msg: Clone + Send + Sync + 'static> PaneCx<'a, 'c, Msg> {
    pub(crate) fn new(cx: &'a mut Cx<'c, AppMsg>, shared: &'a mut Shared, pane: &'static str) -> Self {
        PaneCx { cx, shared, pane, _m: PhantomData }
    }

    fn wrap(&self, msg: Msg) -> AppMsg {
        wrap(self.pane, msg)
    }

    // Settings.

    /// `settings.toml` as last read or written.
    pub fn settings(&self) -> &Settings {
        &self.shared.settings
    }

    /// Change `settings.toml` now: read it again, apply `f`, save it, and
    /// ask the compositor to apply it (`reload-config`), which tells the
    /// shell and every app. A file that can't be written says so in the
    /// status strip.
    pub fn change(&mut self, f: impl FnOnce(&mut Settings)) {
        self.shared.change(f, self.cx);
    }

    // The system.

    /// Whether to use fixtures instead of the live system: `--sample`, and
    /// every test. Nothing done in the sample changes the machine.
    pub fn sample(&self) -> bool {
        self.shared.sample
    }

    /// A wake for `aizome-sys` services the pane keeps: each change the
    /// service sees views the window again.
    pub fn wake(&mut self) -> Wake {
        let tx = self.cx.stream();
        Wake::new(move || {
            tx.send(AppMsg::SystemChanged);
        })
    }

    /// The services the window shares (the accounts, for the sidebar's
    /// card and Users & Groups).
    pub fn sys(&self) -> &crate::system::System {
        &self.shared.sys
    }

    // Work and time.

    /// Send the pane `msg` once this update returns.
    pub fn send(&mut self, msg: Msg) {
        let m = self.wrap(msg);
        self.cx.send(m);
    }

    /// Run `work` off the main thread; its result comes back as a message.
    pub fn spawn(&mut self, work: impl FnOnce() -> Msg + Send + 'static) {
        let pane = self.pane;
        self.cx.spawn(move || wrap(pane, work()));
    }

    /// A sender for a thread that keeps sending (a watcher, a sampler).
    pub fn stream(&mut self) -> PaneSender<Msg> {
        PaneSender { tx: self.cx.stream(), pane: self.pane, _m: PhantomData }
    }

    /// Wait for an `aizome-sys` task on a thread of its own (it may wait
    /// for a polkit prompt), then send `f(result)`.
    pub fn wait<T: Send + 'static>(
        &mut self,
        task: Task<T>,
        f: impl FnOnce(aizome_sys::Result<T>) -> Msg + Send + 'static,
    ) {
        let tx = self.stream();
        let spawned = std::thread::Builder::new().name("settei-wait".into()).spawn(move || {
            tx.send(f(task.wait()));
        });
        if let Err(e) = spawned {
            log::warn!("no thread to wait on: {e}");
        }
    }

    /// Send `msg` once, after `delay`.
    pub fn after(&mut self, delay: Duration, msg: Msg) -> TimerHandle {
        let m = self.wrap(msg);
        self.cx.after(delay, m)
    }

    /// Send `msg` every `period` until cancelled (stop it in `hidden`).
    pub fn every(&mut self, period: Duration, msg: Msg) -> TimerHandle {
        let m = self.wrap(msg);
        self.cx.every(period, m)
    }

    pub fn cancel(&mut self, timer: TimerHandle) {
        self.cx.cancel(timer);
    }

    // Asking and telling.

    /// Ask before a risky change: a sheet with the action and Cancel.
    pub fn confirm(&mut self, c: Confirm<Msg>) {
        let pane = self.pane;
        let mut alert = aizome_kit::Alert::new(c.title, c.message);
        alert = if c.destructive {
            alert.destructive_button(c.action, wrap(pane, c.on_confirm))
        } else {
            alert.default_button(c.action, wrap(pane, c.on_confirm))
        };
        alert = match c.on_cancel {
            Some(m) => alert.cancel_with("Cancel", wrap(pane, m)),
            None => alert.cancel_button("Cancel"),
        };
        let w = self.shared.window;
        self.cx.alert(w, alert);
    }

    /// A line in the status strip at the window's foot, for a few seconds.
    pub fn say(&mut self, text: impl Into<String>) {
        self.shared.say(text.into(), false, self.cx);
    }

    /// Say that `what` failed and why, in the status strip (in red).
    pub fn failed(&mut self, what: &str, error: impl std::fmt::Display) {
        self.shared.say(format!("{what}: {error}"), true, self.cx);
    }

    /// Start a program (Yukimi, Kura, a terminal), detached. In the sample
    /// it's only said.
    pub fn launch(&mut self, argv: &[&str]) {
        let argv: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
        self.shared.launch(argv, self.cx);
    }

    /// Ask the compositor (`aizome-ipc`) on a thread; `f` gets its answer
    /// (an error when there's no compositor, as in the sample).
    pub fn compositor(
        &mut self,
        request: aizome_ipc::Request,
        f: impl FnOnce(std::io::Result<aizome_ipc::Response>) -> Msg + Send + 'static,
    ) {
        let sample = self.shared.sample;
        self.spawn(move || {
            if sample {
                return f(Err(std::io::Error::other("the sample has no compositor")));
            }
            f(aizome_ipc::Client::connect().and_then(|mut c| c.request(&request)))
        });
    }

    /// Open another pane (and, with `entry`, reveal that option).
    pub fn show_pane(&mut self, pane: &'static str, entry: Option<&str>) {
        self.cx.send(AppMsg::Open(pane.to_owned(), entry.map(str::to_owned)));
    }

    /// Open the pane's sheet: the window shows [`Pane::sheet`] over itself
    /// until [`close_sheet`](PaneCx::close_sheet).
    pub fn open_sheet(&mut self) {
        self.cx.send(AppMsg::OpenSheet(self.pane));
    }

    pub fn close_sheet(&mut self) {
        self.cx.send(AppMsg::CloseSheet);
    }

    /// Ask for files: `f` gets them, or `None` if cancelled.
    pub fn open_panel(&mut self, dialog: FileDialog, f: impl Fn(Option<Vec<PathBuf>>) -> Msg + 'static) {
        let pane = self.pane;
        let w = self.shared.window;
        self.cx.open_panel(w, dialog, move |paths| wrap(pane, f(paths)));
    }

    /// Focus the element given `.id(name)`.
    pub fn focus(&mut self, name: &str) {
        self.cx.focus(name);
    }

    pub fn set_clipboard_text(&mut self, text: &str) {
        self.cx.set_clipboard_text(text);
    }

    /// The window, for what the kit asks one of.
    pub fn window(&self) -> WindowKey {
        self.shared.window
    }

    /// The kit's own context, for anything not wrapped here. Messages sent
    /// through it are the window's ([`AppMsg`]).
    pub fn kit(&mut self) -> &mut Cx<'c, AppMsg> {
        self.cx
    }
}

/// What a view reads: the settings, the system, and what search found.
pub struct ViewCx<'a> {
    pub(crate) shared: &'a Shared,
    pub(crate) pane: &'static str,
    /// Entries of this pane that the search matches.
    pub(crate) matched: &'a HashSet<String>,
    /// The entry just revealed, drawn highlighted.
    pub(crate) revealed: Option<&'a str>,
}

impl<'a> ViewCx<'a> {
    pub fn settings(&self) -> &'a Settings {
        &self.shared.settings
    }

    pub fn sys(&self) -> &'a crate::system::System {
        &self.shared.sys
    }

    pub fn sample(&self) -> bool {
        self.shared.sample
    }

    /// The pane being viewed.
    pub fn pane(&self) -> &'static str {
        self.pane
    }

    /// Whether search matches `entry` of this pane (its row is
    /// highlighted), or it was just revealed.
    pub fn matches(&self, entry: &str) -> bool {
        self.matched.contains(entry) || self.revealed == Some(entry)
    }

    /// The name the window reveals `entry`'s row by.
    pub fn entry_name(&self, entry: &str) -> String {
        entry_name(self.pane, entry)
    }
}

/// `entry:<pane>.<entry>`: the name of an entry's row.
pub fn entry_name(pane: &str, entry: &str) -> String {
    format!("entry:{pane}.{entry}")
}
