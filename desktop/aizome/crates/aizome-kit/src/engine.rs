//! The engine: windows and popups with their trees, rebuilding and
//! diffing them, routing input to widgets, the focus, menus' shortcuts,
//! timers and tooltips. It knows nothing of the program's types (messages
//! travel as [`AnyMsg`]) and nothing of Sidestep (it talks to a
//! [`Platform`]), so it runs the same in an app and in tests.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use sidestep_ui::{Canvas, Cursor, Ime, Key, KeyEvent, Modifiers, NamedKey, PointerButton, ScrollPhase};

use crate::element::{AnyMsg, Common, Mapper, MsgFn, RawElement};
use crate::layout::{Length, Point, Rect, Size, Vec2};
use crate::menu::{ActionSpec, ItemSpec, MenuRole, MenuSpec};
use crate::platform::{OpenKind, OpenSpec, Placement, Platform, WinId};
use crate::program::{AppInfo, WindowKey, WindowKind, WindowSpec};
use crate::stats::Stats;
use crate::text::TextCache;
use crate::theme::{Appearance, Theme};
use crate::tree::{NodeId, PaintEnv, Snapshot, Tree};
use crate::widget::{EditCommand, Env, Event, EventCx, FocusRequest, Focusable, Out, StateMap, Status, Vacant};
use crate::widgets::chrome::{FloatingPanel, Root, SheetLayer};
use crate::widgets::menus::{MenuBarStrip, MenuLook, MenuView, TooltipView};

/// The kit's own messages, handled before the program's.
#[derive(Clone, Debug)]
pub(crate) enum KitMsg {
    /// An item of the menu at `node` in `window` was chosen (its path:
    /// its index, and its submenus' when the menu was filtered).
    MenuChoose { window: WinId, node: NodeId, path: Vec<usize> },
    /// Open the submenu at `path` of the menu at `node`, beside `anchor`.
    MenuSub { window: WinId, node: NodeId, path: Vec<usize>, anchor: Rect },
    /// Close the submenus open from the menu in `window`.
    MenuCloseSubs { window: WinId },
    /// Give the keyboard to the submenu just opened from `window`.
    MenuFocusSub { window: WinId },
    /// ← in a menu: close it if it's a submenu.
    MenuBack { window: WinId },
    /// Close every menu over `window` (an item was chosen).
    CloseMenus { window: WinId },
    /// Open the menu bar's menu `index` under its title (in-window bar).
    OpenBarMenu { window: WinId, index: usize, anchor: Rect },

    /// An alert's button was chosen.
    AlertChoice { alert: u64, index: usize },
    /// A file panel's own message.
    Panel { panel: u64, msg: crate::dialogs::PanelMsg },
    /// The user asked to quit.
    Quit,
    /// Close a window as if the user asked (the program may refuse).
    CloseRequest(WindowKey),
    /// A native drag accepted by the platform, from a window or its popup.
    DragStarted { window: WindowKey, drag: crate::program::DragId },
    /// Ask the shell to do something (Help ▸ Search); nothing without one.
    AskShell(aizome_shell_proto::ShellRequest),
}

/// What a timer is for.
pub(crate) enum TimerUse {
    /// The program's `after` or `every`.
    App {
        msg: MsgFn,
        repeat: bool,
    },
    /// A widget's tick, asked for `after` (held while its window is
    /// hidden, and asked for again when it shows).
    Tick {
        win: WinId,
        node: NodeId,
        after: Duration,
    },
    /// The next band of a hidden window drawn ahead.
    Ahead {
        win: WinId,
    },
    Tooltip {
        win: WinId,
        node: NodeId,
    },
    /// Publish the menus to the shell.
    Menus,
}

/// Where a window key points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyTarget {
    Window(WinId),
    Sheet { parent: WinId },
}

pub(crate) enum WinKind {
    App { key: WindowKey, spec: WindowSpec },
    Popup { owner: WinId, kind: PopupKind },
}

// Few at a time: a menu's size doesn't matter.
#[allow(clippy::large_enum_variant)]
pub(crate) enum PopupKind {
    /// A menu (a pop-up button's, a context menu, a bar menu, a submenu).
    Menu {
        spec: MenuSpec,
        look: MenuLook,
        bar: Option<usize>,
    },
    /// A popover element's content, from the owner's tree, shown against
    /// `anchor` (the node's rectangle when last placed).
    Popover {
        node: NodeId,
        on_dismiss: Option<MsgFn>,
        anchor: Rect,
    },
    Tooltip,
}

pub(crate) struct Win {
    pub id: WinId,
    pub kind: WinKind,
    pub tree: Tree,
    pub states: StateMap,
    pub snapshot: Snapshot,
    pub size: Size,
    pub hot: Option<NodeId>,
    pub active: Option<NodeId>,
    pub focused: Option<NodeId>,
    /// Focus suspended by a modal, restored when that layer is uncovered.
    modal_focus: HashMap<Option<NodeId>, (Option<NodeId>, bool)>,
    focus_modal: Option<NodeId>,
    pub focus_visible: bool,
    pub has_keyboard: bool,
    pub pointer: Option<Point>,
    pub dirty: bool,
    pub cursor: Cursor,
    /// Messages from a popup go through these (the opener's mappers).
    pub mapper: Option<Mapper>,
    pub sheets: Vec<WindowKey>,
    pub alerts: Vec<u64>,
    pub panels: Vec<u64>,
    pub tooltip: Option<(NodeId, u64)>,
    pub tooltip_shown: Option<WinId>,
    /// The nearest tooltip under the pointer, including one dismissed by a click.
    tooltip_target: Option<(NodeId, Arc<str>)>,
    pub ime: (bool, Option<Rect>),
    pub title: String,
    /// Popups open over it, in order.
    pub popups: Vec<WinId>,
    pub built: bool,
    /// Widgets with a tick on its way.
    pub ticking: crate::hash::FxHashSet<NodeId>,
    /// Widgets whose state moved their children: lay out again.
    pub relayout: Vec<NodeId>,
    /// Off the screen: viewed and drawn only while it's drawn ahead, when
    /// `refresh` asks.
    pub hidden: bool,
    /// Drawn ahead of its next showing (at a scale, in bands: see
    /// [`crate::ahead`]).
    pub ahead: Option<crate::ahead::Drawing>,
    /// Its widgets' ticks that came while it was hidden: asked for again
    /// when it shows.
    pub held: Vec<(NodeId, Duration)>,
    /// What had the focus when it was hidden, focused again when it shows
    /// (unless the program focused something meanwhile).
    pub refocus: Option<NodeId>,
    /// To be viewed (and drawn) while hidden: it's drawn ahead.
    pub refresh: bool,
}

impl Win {
    fn new(id: WinId, kind: WinKind, size: Size) -> Win {
        Win {
            id,
            kind,
            tree: Tree::default(),
            states: StateMap::default(),
            snapshot: Snapshot::default(),
            size,
            hot: None,
            active: None,
            focused: None,
            modal_focus: HashMap::new(),
            focus_modal: None,
            focus_visible: false,
            has_keyboard: false,
            pointer: None,
            dirty: true,
            cursor: Cursor::Default,
            mapper: None,
            sheets: Vec::new(),
            alerts: Vec::new(),
            panels: Vec::new(),
            tooltip: None,
            tooltip_shown: None,
            tooltip_target: None,
            ime: (false, None),
            title: String::new(),
            popups: Vec::new(),
            built: false,
            ticking: Default::default(),
            relayout: Vec::new(),
            hidden: false,
            ahead: None,
            held: Vec::new(),
            refocus: None,
            refresh: false,
        }
    }

    pub fn key(&self) -> Option<WindowKey> {
        match &self.kind {
            WinKind::App { key, .. } => Some(*key),
            WinKind::Popup { .. } => None,
        }
    }

    pub fn is_popup(&self) -> bool {
        matches!(self.kind, WinKind::Popup { .. })
    }

    /// The focused widget, while this window has the keyboard.
    pub fn active_focus(&self) -> Option<NodeId> {
        if self.has_keyboard || self.is_popup() {
            self.focused.filter(|id| self.input_node(*id).is_some())
        } else {
            None
        }
    }

    /// Where input stays while a sheet is up.
    fn modal(&self) -> Option<usize> {
        self.tree.root.and(self.tree.modal())
    }

    /// Also sees a dialog opened or dismissed before its next layout.
    fn has_modal(&self) -> bool {
        !self.sheets.is_empty() || !self.alerts.is_empty() || !self.panels.is_empty()
    }

    /// A live node that may receive input in the current modal scope.
    fn input_node(&self, id: NodeId) -> Option<usize> {
        self.tree.get(id).filter(|&i| self.modal().is_none_or(|m| m == i || self.tree.is_ancestor(m, i)))
    }
}

/// An alert's buttons and what they send.
pub(crate) struct AlertState {
    pub window: WinId,
    pub title: String,
    pub message: String,
    pub buttons: Vec<crate::dialogs::AlertButton>,
}

pub(crate) struct Core {
    pub theme: Theme,
    pub appearance: Appearance,
    pub text: TextCache,
    pub wins: BTreeMap<WinId, Win>,
    next_win: WinId,
    next_key: u64,
    pub keys: HashMap<WindowKey, KeyTarget>,
    pub queue: VecDeque<AnyMsg>,
    pub timers: HashMap<u64, TimerUse>,
    pub menubar: Vec<MenuSpec>,
    pub info: AppInfo,
    /// No Aizome shell: draw the menu bar along each window's top.
    pub in_window_menus: bool,
    pub mods: Modifiers,
    /// A name to focus once it's in a tree: in the window given (one
    /// just opened, not built yet), or in the next window built that has it.
    pub pending_focus: Vec<(Option<WinId>, Arc<str>)>,
    pub bg_tx: Sender<AnyMsg>,
    pub bg_rx: Receiver<AnyMsg>,
    pub stats: Stats,
    pub alerts: HashMap<u64, AlertState>,
    pub panels: HashMap<u64, crate::dialogs::PanelState>,
    pub next_id: u64,
    /// The app window that last had the keyboard.
    pub key_window: Option<WinId>,
    pub quit: bool,
    /// Hooks for what only the compositor can do (Window menu).
    pub wm: Option<Box<dyn crate::cx::WindowManager>>,
}

impl Core {
    pub fn new(info: AppInfo, appearance: Appearance) -> Core {
        let (bg_tx, bg_rx) = std::sync::mpsc::channel();
        Core {
            theme: Theme::new(appearance),
            appearance,
            text: TextCache::default(),
            wins: BTreeMap::new(),
            next_win: 1,
            next_key: 1,
            keys: HashMap::new(),
            queue: VecDeque::new(),
            timers: HashMap::new(),
            menubar: Vec::new(),
            info,
            in_window_menus: std::env::var_os("AIZOME_SHELL_SOCKET").is_none(),
            mods: Modifiers::NONE,
            pending_focus: Vec::new(),
            bg_tx,
            bg_rx,
            stats: Stats::default(),
            alerts: HashMap::new(),
            panels: HashMap::new(),
            next_id: 1,
            key_window: None,
            quit: false,
            wm: None,
        }
    }

    pub fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    // Windows.

    pub fn open_app_window(&mut self, spec: WindowSpec, platform: &mut dyn Platform) -> WindowKey {
        let key = WindowKey(self.next_key);
        self.next_key += 1;
        let id = self.next_win;
        self.next_win += 1;
        let (kind, background) = match &spec.kind {
            WindowKind::Dialog { parent } => {
                (OpenKind::Toplevel { parent: self.window_of(*parent), utility: false }, Some(self.theme.surface()))
            }
            WindowKind::Normal => (OpenKind::Toplevel { parent: None, utility: false }, Some(self.theme.surface())),
            WindowKind::Utility => (OpenKind::Toplevel { parent: None, utility: true }, Some(self.theme.surface())),
            // Desktop surfaces draw what they like, shadows and all.
            WindowKind::Layer(o) => (OpenKind::Layer(o.clone()), None),
            WindowKind::Lock { output } => (OpenKind::Lock { output: *output }, None),
            WindowKind::Popup { parent, anchor, placement, grab } => match self.window_of(*parent) {
                Some(p) => (
                    OpenKind::Popup { parent: p, anchor: *anchor, placement: placement.placement(), grab: *grab },
                    None,
                ),
                None => {
                    log::warn!("a popup over a window that isn't open");
                    return WindowKey::NONE;
                }
            },
        };
        // Sizes are the content's: the in-window menu bar, where the kit
        // draws one, comes on top.
        let strip = self.menu_strip(&spec);
        let grow = |s: Size| Size::new(s.width, s.height + strip);
        let open = OpenSpec {
            title: spec.title.clone(),
            size: grow(spec.size),
            min_size: spec.min_size.map(grow),
            max_size: spec.max_size.map(grow),
            resizable: spec.resizable,
            kind,
            background,
            app_id: self.info.app_id.clone(),
            visible: !spec.hidden,
        };
        let mut w = Win::new(id, WinKind::App { key, spec: spec.clone() }, grow(spec.size));
        w.title = spec.title.clone();
        w.hidden = spec.hidden;
        self.wins.insert(id, w);
        self.keys.insert(key, KeyTarget::Window(id));
        platform.open_window(id, &open);
        key
    }

    /// How tall the in-window menu bar is over a window of `spec` (none
    /// where menus go to the shell, or the window has no bar).
    pub fn menu_strip(&self, spec: &WindowSpec) -> f64 {
        if self.in_window_menus && spec.has_menu_bar() && !self.menubar.is_empty() {
            self.theme.metrics.target
        } else {
            0.0
        }
    }

    /// The menu strip's height over window `win`, if it's an app's.
    pub fn menu_strip_of(&self, win: WinId) -> f64 {
        match self.wins.get(&win).map(|w| &w.kind) {
            Some(WinKind::App { spec, .. }) => self.menu_strip(spec),
            _ => 0.0,
        }
    }

    /// The engine window showing `key` (a sheet's parent's).
    pub fn window_of(&self, key: WindowKey) -> Option<WinId> {
        match self.keys.get(&key)? {
            KeyTarget::Window(w) => Some(*w),
            KeyTarget::Sheet { parent } => Some(*parent),
        }
    }

    pub fn open_sheet(&mut self, parent: WindowKey) -> Option<WindowKey> {
        let win = self.window_of(parent)?;
        let key = WindowKey(self.next_key);
        self.next_key += 1;
        self.keys.insert(key, KeyTarget::Sheet { parent: win });
        let w = self.wins.get_mut(&win)?;
        w.sheets.push(key);
        w.dirty = true;
        Some(key)
    }

    /// Close a window (or sheet), and its popups.
    pub fn close_key(&mut self, key: WindowKey, platform: &mut dyn Platform) {
        match self.keys.remove(&key) {
            Some(KeyTarget::Window(win)) => {
                // Sheets and dialogs go with it.
                let sheets: Vec<WindowKey> = self
                    .keys
                    .iter()
                    .filter(|(_, t)| **t == KeyTarget::Sheet { parent: win })
                    .map(|(k, _)| *k)
                    .collect();
                for s in sheets {
                    self.keys.remove(&s);
                }
                self.close_win(win, platform);
                if self.key_window == Some(win) {
                    self.key_window = None;
                }
            }
            Some(KeyTarget::Sheet { parent }) => {
                if let Some(w) = self.wins.get_mut(&parent) {
                    w.sheets.retain(|k| *k != key);
                    w.dirty = true;
                }
            }
            None => {}
        }
    }

    /// Show window `key` (not a sheet), or hide it: hidden, its popups
    /// close, nothing in it has the focus (what had it gets it back when
    /// it shows), its widgets' ticks wait, and it isn't viewed again until
    /// it shows or is drawn ahead.
    pub fn show_key(&mut self, key: WindowKey, shown: bool, platform: &mut dyn Platform) {
        let Some(KeyTarget::Window(win)) = self.keys.get(&key).copied() else { return };
        let Some(w) = self.wins.get_mut(&win) else { return };
        if w.hidden != shown {
            return;
        }
        if !shown {
            let popups: Vec<WinId> = w.popups.iter().copied().chain(w.tooltip_shown).collect();
            w.tooltip_shown = None;
            w.refocus = w.focused;
            for p in popups {
                self.close_win(p, platform);
            }
            self.set_focus(win, None, false, platform);
        }
        let Some(w) = self.wins.get_mut(&win) else { return };
        w.hidden = !shown;
        w.ahead = None;
        w.refresh = false;
        // Shown, it's viewed again at once: what changed since it was drawn
        // ahead (or all of it) is drawn.
        w.dirty = shown;
        platform.show_window(win, shown);
        if !shown {
            return;
        }
        let refocus = w.refocus.take().filter(|_| w.focused.is_none());
        let held = std::mem::take(&mut w.held);
        for (node, _) in &held {
            w.ticking.remove(node);
        }
        for (node, after) in held {
            self.tick_later(win, node, after, platform);
        }
        if let Some(f) = refocus {
            self.set_focus(win, Some(f), false, platform);
        }
    }

    /// Draw hidden window `key` ahead of its next showing, at `scale`:
    /// view and lay it out now, and draw it offscreen in bands at idle (see
    /// [`crate::ahead`]), again whenever this is asked again or the theme
    /// changes. Whether it is (an app window of a size, hidden).
    pub fn prepare_key(&mut self, key: WindowKey, scale: f64, platform: &mut dyn Platform) -> bool {
        let Some(KeyTarget::Window(win)) = self.keys.get(&key).copied() else { return false };
        let Some(w) = self.wins.get_mut(&win) else { return false };
        let sized = w.size.width >= 1.0 && w.size.height >= 1.0;
        if !w.hidden || !sized || !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        w.ahead = Some(crate::ahead::Drawing::new(scale));
        w.refresh = true;
        w.dirty = true;
        platform.keep_picture(win, None);
        true
    }

    /// Draw the next band of hidden window `win` ahead, and when that was
    /// the last, give the platform the picture; else ask for the next.
    fn draw_ahead(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        let Some(d) = w.ahead.as_mut().filter(|_| w.hidden && w.built) else { return };
        d.armed = false;
        let (size, scale) = (w.size, d.scale);
        let bands = crate::ahead::bands(size, scale);
        let Some(&band) = bands.get(d.bands.len()) else { return };
        let look = sidestep_ui::Appearance { dark: self.theme.is_dark(), high_contrast: false };
        let image = sidestep_ui::offscreen::render(band.size(), scale, look, |c| {
            c.translate(-band.origin().to_vec2());
            self.paint(win, c);
        });
        let Some(d) = self.wins.get_mut(&win).and_then(|w| w.ahead.as_mut()) else { return };
        d.bands.push((band, image));
        if d.bands.len() < bands.len() {
            self.arm_ahead(win, platform);
        } else {
            let picture = crate::ahead::Picture { size, scale, bands: d.bands.clone() };
            platform.keep_picture(win, Some(picture));
        }
    }

    /// Draw hidden window `win` ahead from its first band, at the loop's
    /// next turn: it was viewed again.
    fn restart_ahead(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(d) = self.wins.get_mut(&win).and_then(|w| w.ahead.as_mut()) else { return };
        d.bands.clear();
        platform.keep_picture(win, None);
        self.arm_ahead(win, platform);
    }

    fn arm_ahead(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(d) = self.wins.get_mut(&win).and_then(|w| w.ahead.as_mut()) else { return };
        if !std::mem::replace(&mut d.armed, true) {
            let token = platform.set_timer(Duration::ZERO, false);
            self.timers.insert(token, TimerUse::Ahead { win });
        }
    }

    fn close_win(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.remove(&win) else { return };
        if crate::inspect::enabled() {
            crate::inspect::closed(&self.info.app_id, win);
        }
        for p in w.popups {
            self.close_win(p, platform);
        }
        if let Some(t) = w.tooltip_shown {
            self.close_win(t, platform);
        }
        if let WinKind::Popup { owner, kind } = &w.kind
            && let Some(o) = self.wins.get_mut(owner)
        {
            o.popups.retain(|p| *p != win);
            if o.tooltip_shown == Some(win) {
                o.tooltip_shown = None;
            }
            // The in-window bar's title goes back when its menu closes.
            if let PopupKind::Menu { bar: Some(_), .. } = kind {
                o.states.retain(|_, s| !s.is::<crate::widgets::menus::BarState>());
                o.dirty = true;
            }
        }
        // Whoever showed the menu hears that it's gone.
        if let WinKind::Popup { kind: PopupKind::Menu { look, .. }, .. } = &w.kind
            && let Some(m) = &look.closed
        {
            let msg = m.make();
            self.queue.push_back(match &w.mapper {
                Some(map) => map(msg),
                None => msg,
            });
        }
        for a in w.alerts {
            self.alerts.remove(&a);
        }
        platform.close_window(win);
    }

    /// Close the lock screen's windows (the lock ended).
    pub fn close_lock_windows(&mut self, platform: &mut dyn Platform) {
        let locks: Vec<WindowKey> = self
            .wins
            .values()
            .filter_map(|w| match &w.kind {
                WinKind::App { key, spec } if matches!(spec.kind, WindowKind::Lock { .. }) => Some(*key),
                _ => None,
            })
            .collect();
        for k in locks {
            self.close_key(k, platform);
        }
    }

    pub fn app_windows(&self) -> Vec<(WinId, WindowKey)> {
        self.wins.values().filter_map(|w| w.key().map(|k| (w.id, k))).collect()
    }

    /// Windows that need viewing again: hidden ones only while they're
    /// drawn ahead.
    pub fn dirty_windows(&self) -> Vec<WinId> {
        self.wins.values().filter(|w| w.dirty && !w.is_popup() && (!w.hidden || w.refresh)).map(|w| w.id).collect()
    }

    pub fn mark_all_dirty(&mut self) {
        for w in self.wins.values_mut() {
            if !w.is_popup() {
                w.dirty = true;
            }
        }
    }

    // Building.

    /// Put a window's new view in place: compose it with its menu bar,
    /// sheets and alerts, lay it out, diff it against the last, and
    /// invalidate what changed.
    pub fn install(&mut self, win: WinId, main: RawElement, sheets: Vec<RawElement>, platform: &mut dyn Platform) {
        let started = Instant::now();
        let Some(w) = self.wins.get(&win) else { return };
        let is_normal = matches!(&w.kind, WinKind::App { spec, .. } if spec.has_menu_bar());
        let opaque = matches!(&w.kind, WinKind::App { spec, .. } if !matches!(spec.kind, WindowKind::Layer(_) | WindowKind::Lock { .. } | WindowKind::Popup { .. }));
        let mut layers = Vec::new();
        let bar = self.in_window_menus && is_normal && !self.menubar.is_empty();
        let top = if bar { self.theme.metrics.target } else { 0.0 };
        // The same tree with or without the bar (the shell can go and
        // come back): the view keeps its place, so its focus and its
        // widgets' state live through the bar coming and going.
        let mut content = Common::sized(Length::Fill, Length::Fill);
        content.key = Some(2);
        let content = RawElement::new(crate::widgets::layout::FlexWidget::column(0.0), content, vec![fill(main)]);
        let mut items = Vec::with_capacity(2);
        if bar {
            let mut bar = MenuBarStrip::element(&self.info, &self.menubar, win);
            bar.common.key = Some(3);
            items.push(bar);
        }
        items.push(content);
        let mut c = Common::sized(Length::Fill, Length::Fill);
        c.key = Some(1);
        layers.push(RawElement::new(crate::widgets::layout::FlexWidget::column(0.0), c, items));
        for (i, s) in sheets.into_iter().enumerate() {
            layers.push(SheetLayer::element(s, 100 + i as u64, top));
        }
        let alerts = w.alerts.clone();
        let panels = w.panels.clone();
        for p in panels {
            if let Some(el) = crate::dialogs::panel_element(self, p) {
                layers.push(SheetLayer::element(el, 2000 + p, top));
            }
        }
        for a in alerts {
            if let Some(el) = crate::dialogs::alert_element(self, a) {
                layers.push(SheetLayer::element(el, 1000 + a, top));
            }
        }
        let root = RawElement::new(Root { opaque }, Common::sized(Length::Fill, Length::Fill), layers);
        self.build(win, root, platform);
        self.stats.view_layout(started.elapsed());
        self.sync_popovers(win, platform);
        let mut names = std::mem::take(&mut self.pending_focus);
        names.retain(|(target, n)| match target {
            Some(t) if *t != win => self.wins.contains_key(t),
            _ => !self.focus_name(win, n, platform),
        });
        self.pending_focus = names;
    }

    /// Mount, lay out, diff and invalidate.
    fn build(&mut self, win: WinId, root: RawElement, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        let base = NodeId::ROOT.child(match w.kind {
            WinKind::App { .. } => "window",
            WinKind::Popup { .. } => "popup",
        });
        let mut tree = Tree::new(root, base);
        let mut env = Env {
            focus: w.active_focus(),
            theme: &self.theme,
            text: &mut self.text,
            states: &mut w.states,
            now: Instant::now(),
            on_selection: false,
        };
        tree.layout_window(w.size, &mut env);
        let damage = if w.built {
            settle(&mut tree, &w.snapshot, w.focused, win, platform)
        } else {
            tree.scrolls.clear();
            tree.local_damage.clear();
            vec![Rect::from_origin_size(Point::ZERO, w.size)]
        };
        w.snapshot = tree.snapshot();
        let ticks = std::mem::take(&mut tree.ticks);
        w.tree = tree;
        w.built = true;
        w.dirty = false;
        w.refresh = false;
        let live = &w.tree.by_id;
        w.states.retain(|(id, _), _| live.contains_key(id));
        for (node, after) in ticks {
            self.tick_later(win, node, after, platform);
        }
        let Some(w) = self.wins.get_mut(&win) else { return };
        let mut area = 0.0;
        for r in &damage {
            area += r.area();
            platform.invalidate(win, *r);
        }
        self.stats.damage(damage.len(), area, w.size.width * w.size.height);
        // What the pointer is over may have moved.
        let focus_gone = w.focused.is_some_and(|f| w.tree.get(f).is_none());
        if focus_gone {
            w.focused = None;
            if w.ime.0 {
                w.ime = (false, None);
                platform.set_text_input(win, false, None);
            }
        }
        self.restore_modal_focus(win, platform);
        let Some(w) = self.wins.get_mut(&win) else { return };
        let ahead = w.hidden && w.ahead.is_some();
        if let Some(p) = w.pointer {
            self.update_hot(win, p, platform);
        }
        // Hidden and drawn ahead: drawn again, from the top, at idle.
        if ahead {
            self.restart_ahead(win, platform);
        }
    }

    /// Windows with widgets to lay out again (and nothing to view again).
    pub fn relayout_windows(&self) -> Vec<WinId> {
        self.wins.values().filter(|w| !w.relayout.is_empty() && w.built && !w.hidden).map(|w| w.id).collect()
    }

    /// Lay the widgets that asked out again, and what's around them, with
    /// the tree as it is: a scroll moving its content, a list showing other
    /// rows. Then diff and invalidate as a rebuild would.
    pub fn relayout(&mut self, win: WinId, platform: &mut dyn Platform) {
        let started = Instant::now();
        let Some(w) = self.wins.get_mut(&win) else { return };
        let ids = std::mem::take(&mut w.relayout);
        if w.dirty || w.tree.root.is_none() {
            // Viewed again soon; what it keeps must still be laid out anew.
            for id in ids {
                if let Some(i) = w.tree.get(id) {
                    w.tree.uncache_up(i);
                }
            }
            return;
        }
        for id in ids {
            if let Some(i) = w.tree.get(id) {
                w.tree.uncache_up(i);
            }
        }
        let size = w.size;
        let mut env = Env {
            focus: w.active_focus(),
            theme: &self.theme,
            text: &mut self.text,
            states: &mut w.states,
            now: Instant::now(),
            on_selection: false,
        };
        w.tree.layout_window(size, &mut env);
        let dead = w.tree.nodes.iter().filter(|n| n.dead).count();
        if dead > 256 && dead > w.tree.nodes.len() / 2 {
            w.tree.compact();
        }
        let damage = settle(&mut w.tree, &w.snapshot, w.focused, win, platform);
        w.snapshot = w.tree.snapshot();
        let ticks = std::mem::take(&mut w.tree.ticks);
        let live = &w.tree.by_id;
        w.states.retain(|(id, _), _| live.contains_key(id));
        let mut area = 0.0;
        for r in &damage {
            area += r.area();
            platform.invalidate(win, *r);
        }
        let full = w.size.width * w.size.height;
        self.stats.damage(damage.len(), area, full);
        for (node, after) in ticks {
            self.tick_later(win, node, after, platform);
        }
        if let Some(p) = self.wins.get(&win).and_then(|w| w.pointer) {
            self.update_hot(win, p, platform);
        }
        self.follow_anchors(win, platform);
        self.stats.view_layout(started.elapsed());
    }

    /// Re-lay-out a popup (its content changed) and resize it.
    fn build_popup(&mut self, win: WinId, root: RawElement, platform: &mut dyn Platform) -> Size {
        let Some(w) = self.wins.get_mut(&win) else { return Size::ZERO };
        let mut tree = Tree::new(root, NodeId::ROOT.child("popup"));
        let mut env = Env {
            focus: w.active_focus(),
            theme: &self.theme,
            text: &mut self.text,
            states: &mut w.states,
            now: Instant::now(),
            on_selection: false,
        };
        let size = tree.layout_free(Size::new(1200.0, 900.0), &mut env);
        let size = Size::new(size.width.ceil(), size.height.ceil());
        if w.built && size == w.size {
            for r in settle(&mut tree, &w.snapshot, w.focused, win, platform) {
                platform.invalidate(win, r);
            }
        } else {
            tree.scrolls.clear();
            tree.local_damage.clear();
            platform.invalidate_all(win);
        }
        w.size = size;
        w.snapshot = tree.snapshot();
        w.tree = tree;
        w.built = true;
        w.dirty = false;
        let live = &w.tree.by_id;
        w.states.retain(|(id, _), _| live.contains_key(id));
        size
    }

    /// The window was resized (or shown at its size).
    pub fn resized(&mut self, win: WinId, size: Size) {
        if let Some(w) = self.wins.get_mut(&win)
            && (w.size != size || !w.built)
        {
            w.size = size;
            w.dirty = true;
            w.built = false;
        }
    }

    /// A popup's rebuild without the program: lay its content out again
    /// (its states changed its layout).
    pub fn relayout_popup(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get(&win) else { return };
        let WinKind::Popup { kind, .. } = &w.kind else { return };
        let root = match kind {
            PopupKind::Menu { spec, look, .. } => MenuView::element(spec.clone(), look.clone()),
            _ => return,
        };
        // A menu fits what's left as typing filters it: it shortens and
        // grows back, and only ever widens (it doesn't jump about).
        let Some(w) = self.wins.get_mut(&win) else { return };
        let mut tree = Tree::new(root, NodeId::ROOT.child("popup"));
        let mut env = Env {
            focus: w.active_focus(),
            theme: &self.theme,
            text: &mut self.text,
            states: &mut w.states,
            now: Instant::now(),
            on_selection: false,
        };
        let natural = tree.layout_free(Size::new(1200.0, 900.0), &mut env);
        let size = Size::new(natural.width.ceil().max(w.size.width), natural.height.ceil());
        tree.layout_window(size, &mut env);
        if size == w.size {
            for r in settle(&mut tree, &w.snapshot, w.focused, win, platform) {
                platform.invalidate(win, r);
            }
        } else {
            w.size = size;
            tree.scrolls.clear();
            tree.local_damage.clear();
            platform.request_size(win, size);
            platform.invalidate_all(win);
        }
        w.snapshot = tree.snapshot();
        w.tree = tree;
        w.dirty = false;
    }

    /// Move the popovers of `win` whose anchors moved (a list scrolled, a
    /// pane resized) to follow them.
    fn follow_anchors(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get(&win) else { return };
        let mut moved = Vec::new();
        for &p in &w.popups {
            if let Some(WinKind::Popup { kind: PopupKind::Popover { node, anchor, .. }, .. }) =
                self.wins.get(&p).map(|pw| &pw.kind)
                && let Some(i) = w.tree.get(*node)
                && w.tree.nodes[i].rect != *anchor
            {
                moved.push((p, w.tree.nodes[i].rect));
            }
        }
        for (p, rect) in moved {
            if let Some(WinKind::Popup { kind: PopupKind::Popover { anchor, .. }, .. }) =
                self.wins.get_mut(&p).map(|pw| &mut pw.kind)
            {
                *anchor = rect;
            }
            platform.set_popup_anchor(p, rect);
        }
    }

    // Popups.

    /// Open a popup over `owner` showing `root`.
    #[allow(clippy::too_many_arguments)]
    fn open_popup(
        &mut self,
        owner: WinId,
        anchor: Rect,
        placement: Placement,
        grab: bool,
        kind: PopupKind,
        root: RawElement,
        mapper: Option<Mapper>,
        platform: &mut dyn Platform,
    ) -> WinId {
        let id = self.next_win;
        self.next_win += 1;
        let mut w = Win::new(id, WinKind::Popup { owner, kind }, Size::ZERO);
        w.mapper = mapper;
        w.has_keyboard = grab;
        self.wins.insert(id, w);
        let size = self.build_popup(id, root, platform);
        if let Some(o) = self.wins.get_mut(&owner) {
            o.popups.push(id);
        }
        let spec = OpenSpec {
            title: String::new(),
            size,
            min_size: None,
            max_size: None,
            resizable: false,
            kind: OpenKind::Popup { parent: owner, anchor, placement, grab },
            background: None,
            app_id: self.info.app_id.clone(),
            visible: true,
        };
        platform.open_window(id, &spec);
        id
    }

    /// Close popup `win` and those over it.
    pub fn close_popup(&mut self, win: WinId, platform: &mut dyn Platform) {
        if self.wins.get(&win).is_some_and(|w| w.is_popup()) {
            self.close_win(win, platform);
        }
    }

    /// The compositor dismissed popup `win` (it's closed already).
    pub fn popup_dismissed(&mut self, win: WinId, platform: &mut dyn Platform) {
        // A popup of the program's own is gone: the program hears it as a
        // close request (whatever it answers).
        if let Some(WinKind::App { key, .. }) = self.wins.get(&win).map(|w| &w.kind) {
            let key = *key;
            self.close_key(key, platform);
            self.queue.push_back(Box::new(KitMsg::CloseRequest(key)));
            return;
        }
        if let Some(w) = self.wins.get(&win)
            && let WinKind::Popup { kind: PopupKind::Popover { on_dismiss: Some(m), .. }, .. } = &w.kind
        {
            let msg = m.make();
            let msg = match &w.mapper {
                Some(map) => map(msg),
                None => msg,
            };
            self.queue.push_back(msg);
        }
        self.close_popup(win, platform);
    }

    /// The menus open from `win` (and over them) close.
    fn close_menus(&mut self, win: WinId, platform: &mut dyn Platform) {
        // Up to the first window that isn't a menu.
        let mut top = win;
        while let Some(WinKind::Popup { owner, kind: PopupKind::Menu { .. } }) = self.wins.get(&top).map(|w| &w.kind) {
            top = *owner;
        }
        let menus: Vec<WinId> = self
            .wins
            .get(&top)
            .map(|w| {
                w.popups
                    .iter()
                    .copied()
                    .filter(|p| {
                        matches!(
                            self.wins.get(p).map(|w| &w.kind),
                            Some(WinKind::Popup { kind: PopupKind::Menu { .. }, .. })
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let had = !menus.is_empty();
        for m in menus {
            self.close_win(m, platform);
        }
        // The in-window bar's open title goes back.
        if had && let Some(w) = self.wins.get_mut(&top) {
            let before = w.states.len();
            w.states.retain(|_, s| !s.is::<crate::widgets::menus::BarState>());
            if w.states.len() != before {
                w.dirty = true;
            }
        }
    }

    /// The menu requested by a widget: a pop-up button's, a context menu.
    fn open_menu_request(&mut self, win: WinId, req: crate::widget::MenuRequest, platform: &mut dyn Platform) {
        if !self.wins.contains_key(&win) {
            return;
        }
        let mapper = self.mapper_chain(win, req.opener);
        let look = MenuLook { min_width: req.min_width, ..MenuLook::default() };
        let placement = if req.at_point { Placement::AtPoint } else { Placement::Below };
        self.show_menu(win, req.anchor, req.menu, look, placement, mapper, platform);
    }

    /// Open `spec` as a menu popup over `win` at `anchor`.
    #[allow(clippy::too_many_arguments)]
    pub fn show_menu(
        &mut self,
        win: WinId,
        anchor: Rect,
        mut spec: MenuSpec,
        look: MenuLook,
        placement: Placement,
        mapper: Option<Mapper>,
        platform: &mut dyn Platform,
    ) -> WinId {
        self.close_menus(win, platform);
        self.settle_menu(&mut spec);
        let root = MenuView::element(spec.clone(), look.clone());
        let keyboard = look.keyboard;
        let menu = self.open_popup(
            win,
            anchor,
            placement,
            true,
            PopupKind::Menu { spec, look, bar: None },
            root,
            mapper,
            platform,
        );
        if keyboard {
            self.highlight_first_row(menu, platform);
        }
        menu
    }

    /// A menu opened from the keyboard (or a submenu by →): its first
    /// row highlighted, as ↓ would.
    fn highlight_first_row(&mut self, menu: WinId, platform: &mut dyn Platform) {
        let Some(root) = self.wins.get(&menu).and_then(|w| w.tree.root) else { return };
        let down = KeyEvent {
            key: Key::Named(NamedKey::ArrowDown),
            text: None,
            pressed: true,
            repeat: false,
            code: 0,
            modifiers: Modifiers::NONE,
            composing: None,
        };
        self.send(menu, root, &Event::Key(down), platform);
    }

    /// Open the in-window menu bar's menu `index`.
    pub fn open_bar_menu(&mut self, win: WinId, index: usize, anchor: Rect, platform: &mut dyn Platform) {
        self.close_menus(win, platform);
        let Some(mut spec) = self.menubar.get(index).cloned() else { return };
        self.settle_app_menu(&mut spec, Some(win));
        let root = MenuView::element(spec.clone(), MenuLook::default());
        self.open_popup(
            win,
            anchor,
            Placement::Below,
            true,
            PopupKind::Menu { spec, look: MenuLook::default(), bar: Some(index) },
            root,
            None,
            platform,
        );
        // Closing the last menu put the bar's title back; this one's open.
        if let Some(w) = self.wins.get_mut(&win)
            && let Some(n) = w.tree.nodes.iter().find(|n| !n.dead && n.widget.name() == "menu-bar")
        {
            let (id, rect) = (n.id, n.rect);
            crate::widget::state_of::<crate::widgets::menus::BarState>(&mut w.states, id).open = Some(index);
            platform.invalidate(win, rect);
        }
    }

    /// The menu (and its look) of the widget `node` in `win`.
    fn menu_at(&self, win: WinId, node: NodeId) -> Option<(MenuSpec, MenuLook)> {
        let w = self.wins.get(&win)?;
        let i = w.tree.get(node)?;
        let spec = w.tree.nodes[i].widget.menu()?.clone();
        let look = match &w.kind {
            WinKind::Popup { kind: PopupKind::Menu { look, .. }, .. } => look.clone(),
            _ => MenuLook { title: w.tree.nodes[i].widget.menu_title().map(str::to_owned), ..MenuLook::default() },
        };
        Some((spec, look))
    }

    /// The menu popups opened from `win` (its submenus).
    fn menu_popups_of(&self, win: WinId) -> Vec<WinId> {
        self.wins
            .get(&win)
            .map(|w| {
                w.popups
                    .iter()
                    .copied()
                    .filter(|p| {
                        matches!(
                            self.wins.get(p).map(|w| &w.kind),
                            Some(WinKind::Popup { kind: PopupKind::Menu { .. }, .. })
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn menu_sub(&mut self, win: WinId, node: NodeId, path: Vec<usize>, anchor: Rect, platform: &mut dyn Platform) {
        let Some((spec, look)) = self.menu_at(win, node) else { return };
        let mut items = &spec.items;
        let mut sub = None;
        for &i in &path {
            match items.get(i) {
                Some(ItemSpec::Submenu(m)) => {
                    items = &m.items;
                    sub = Some(m);
                }
                _ => return,
            }
        }
        let Some(mut sub) = sub.cloned() else { return };
        let idx = self.wins.get(&win).and_then(|w| w.tree.get(node)).unwrap_or(0);
        let mapper = self.mapper_chain(win, idx);
        // One submenu open at a time.
        for o in self.menu_popups_of(win) {
            self.close_win(o, platform);
        }
        let look = look.sub(&sub.title, &path, anchor);
        self.settle_menu(&mut sub);
        let root = MenuView::element(sub.clone(), look.clone());
        self.open_popup(
            win,
            anchor,
            Placement::Beside,
            true,
            PopupKind::Menu { spec: sub, look, bar: None },
            root,
            mapper,
            platform,
        );
    }

    /// An item of the menu at `node` in `win` was chosen: close the menus,
    /// then carry it out (its message through the menu's mappers).
    fn menu_choose(&mut self, win: WinId, node: NodeId, path: Vec<usize>, platform: &mut dyn Platform) {
        let Some((spec, _)) = self.menu_at(win, node) else { return };
        let mut items = &spec.items;
        let mut action = None;
        for (n, &i) in path.iter().enumerate() {
            match items.get(i) {
                Some(ItemSpec::Submenu(m)) if n + 1 < path.len() => items = &m.items,
                Some(ItemSpec::Action(a)) if n + 1 == path.len() => action = Some(a.clone()),
                _ => return,
            }
        }
        let Some(a) = action else { return };
        let idx = self.wins.get(&win).and_then(|w| w.tree.get(node)).unwrap_or(0);
        let mapper = self.mapper_chain(win, idx);
        let app = self.app_window_for(win);
        // A bar submenu still belongs to the application's command menu.
        // Widget menus inside a dialog (dropdowns, text context menus) do not.
        let mut owner = win;
        let mut bar = false;
        while let Some(WinKind::Popup { owner: parent, kind, .. }) = self.wins.get(&owner).map(|w| &w.kind) {
            bar |= matches!(kind, PopupKind::Menu { bar: Some(_), .. });
            owner = *parent;
        }
        if self.wins.get(&win).is_some_and(|w| w.is_popup()) {
            self.close_menus(win, platform);
        } else {
            for o in self.menu_popups_of(win) {
                self.close_win(o, platform);
            }
        }
        if bar {
            self.perform(&a, app, platform);
            return;
        }
        if let Some(role) = a.role {
            if let Some(cmd) = role.edit_command()
                && self.command(app, cmd, platform)
            {
                return;
            }
            if a.msg.is_none() {
                self.role(role, app, a.id, platform);
                return;
            }
        }
        if let Some(m) = &a.msg {
            let msg = m.make();
            self.queue.push_back(match &mapper {
                Some(map) => map(msg),
                None => msg,
            });
        }
    }

    /// Enabled states the kit knows: supported roles, Edit items by the
    /// focused widget, and the Window menu's list of windows.
    pub fn settle_menu(&self, spec: &mut MenuSpec) {
        let focused = self.key_window.and_then(|w| self.wins.get(&w)).and_then(|w| {
            let idx = w.input_node(w.focused?)?;
            Some((w, idx))
        });
        spec.settle(&|a: &ActionSpec| {
            let role = a.role?;
            if a.msg.is_some() {
                return None;
            }
            // These roles need an app's own action. The kit has no generic
            // preferences window or app-help destination to open.
            if matches!(role, MenuRole::Settings | MenuRole::Help) {
                return Some(false);
            }
            let cmd = role.edit_command()?;
            Some(focused.is_some_and(|(w, idx)| {
                let n = &w.tree.nodes[idx];
                n.widget.can(crate::widget::StateView::new(&w.states, n.id), cmd)
            }))
        });
        self.expand_window_list(spec);
    }

    /// Global commands cannot act on the document covered by a dialog.
    /// Edit commands belong to its focused field; window navigation remains
    /// available. Dialog-owned menus use `settle_menu` instead.
    pub fn settle_app_menu(&self, spec: &mut MenuSpec, win: Option<WinId>) {
        self.settle_menu(spec);
        // Quit is application-wide, unlike Save/Close and editing commands.
        if self.wins.values().any(Win::has_modal) {
            spec.settle(&|a| (a.role == Some(MenuRole::Quit)).then_some(false));
        }
        let Some(win) = win.or(self.key_window) else { return };
        let win = self.app_window_for(win);
        let Some(w) = self.wins.get(&win).filter(|w| w.has_modal()) else { return };
        spec.settle(&|a| {
            if let Some(cmd) = a.role.and_then(MenuRole::edit_command) {
                return Some(w.focused.and_then(|f| w.input_node(f)).is_some_and(|i| {
                    let n = &w.tree.nodes[i];
                    n.widget.can(crate::widget::StateView::new(&w.states, n.id), cmd)
                }));
            }
            (!Self::window_command(a)).then_some(false)
        });
    }

    fn window_command(a: &ActionSpec) -> bool {
        a.msg.is_none()
            && matches!(
                a.role,
                Some(
                    MenuRole::Minimise
                        | MenuRole::Fill
                        | MenuRole::Hide
                        | MenuRole::HideOthers
                        | MenuRole::ShowAll
                        | MenuRole::StackWith
                        | MenuRole::BringAllToFront
                        | MenuRole::WindowList
                )
            )
    }

    pub fn has_modal(&self, win: WinId) -> bool {
        self.wins.get(&self.app_window_for(win)).is_some_and(Win::has_modal)
    }

    fn expand_window_list(&self, spec: &mut MenuSpec) {
        let at =
            spec.items.iter().position(|i| matches!(i, ItemSpec::Action(a) if a.role == Some(MenuRole::WindowList)));
        let Some(at) = at else { return };
        let mut items = Vec::new();
        for (win, _key) in self.app_windows() {
            let w = &self.wins[&win];
            items.push(ItemSpec::Action(ActionSpec {
                label: if w.title.is_empty() { self.info.name.clone() } else { w.title.clone() },
                msg: None,
                shortcut: None,
                enabled: true,
                checked: self.key_window == Some(win),
                role: Some(MenuRole::WindowList),
                id: Some(0x5700_0000 + win),
                icon: None,
            }));
        }
        spec.items.splice(at..at + 1, items);
    }

    /// Keep each open popover's popup showing its content.
    fn sync_popovers(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        let found = std::mem::take(&mut w.tree.popovers);
        let mut wanted: Vec<(NodeId, Rect, RawElement, Option<MsgFn>, usize)> = Vec::new();
        for p in found {
            if p.content.open && !w.tree.nodes[p.node].dead {
                let n = &w.tree.nodes[p.node];
                wanted.push((n.id, n.rect, p.content.content, p.content.on_dismiss, p.node));
            }
        }
        // Close popovers no longer wanted.
        let popups = w.popups.clone();
        let existing: Vec<(WinId, NodeId)> = popups
            .iter()
            .filter_map(|p| match self.wins.get(p).map(|w| &w.kind) {
                Some(WinKind::Popup { kind: PopupKind::Popover { node, .. }, .. }) => Some((*p, *node)),
                _ => None,
            })
            .collect();
        for (p, node) in &existing {
            if !wanted.iter().any(|(n, ..)| n == node) {
                self.close_win(*p, platform);
            }
        }
        for (node, anchor, content, on_dismiss, idx) in wanted {
            let mapper = self.mapper_chain(win, idx);
            let root = FloatingPanel::element(content);
            match existing.iter().find(|(_, n)| *n == node) {
                Some((p, _)) => {
                    let before = self.wins.get(p).map(|w| w.size);
                    let size = self.build_popup(*p, root, platform);
                    if before != Some(size) {
                        platform.request_size(*p, size);
                    }
                }
                None => {
                    self.open_popup(
                        win,
                        anchor,
                        Placement::Below,
                        true,
                        PopupKind::Popover { node, on_dismiss, anchor },
                        root,
                        mapper,
                        platform,
                    );
                }
            }
        }
        self.follow_anchors(win, platform);
    }

    /// The mappers from node `idx` up to the root, and the window's own.
    fn mapper_chain(&self, win: WinId, idx: usize) -> Option<Mapper> {
        let w = self.wins.get(&win)?;
        let mut maps: Vec<Mapper> = Vec::new();
        let mut at = Some(idx);
        while let Some(i) = at {
            let n = w.tree.nodes.get(i)?;
            if let Some(m) = &n.common.mapper {
                maps.push(m.clone());
            }
            at = n.parent;
        }
        if let Some(m) = &w.mapper {
            maps.push(m.clone());
        }
        if maps.is_empty() {
            return None;
        }
        Some(std::rc::Rc::new(move |mut msg| {
            for m in &maps {
                msg = m(msg);
            }
            msg
        }))
    }

    // Input.

    /// Hand `event` to node `idx`, then apply what it asked for.
    fn send(&mut self, win: WinId, idx: usize, event: &Event, platform: &mut dyn Platform) -> Status {
        let Some(w) = self.wins.get_mut(&win) else { return Status::Ignored };
        let Some(node) = w.tree.nodes.get_mut(idx) else { return Status::Ignored };
        if node.dead {
            return Status::Ignored;
        }
        let id = node.id;
        let origin = node.rect.origin().to_vec2();
        let mut widget = std::mem::replace(&mut node.widget, Box::new(Vacant));
        let mut out = Out::default();
        let local = localize(event, origin);
        let status = {
            let mut env = Env {
                focus: w.active_focus(),
                theme: &self.theme,
                text: &mut self.text,
                states: &mut w.states,
                now: Instant::now(),
                on_selection: false,
            };
            let mut cx = EventCx {
                tree: &w.tree,
                node: idx,
                win,
                env: &mut env,
                out: &mut out,
                platform,
                hovered: w.hot == Some(id),
                pressed: w.active == Some(id),
                focused: w.focused == Some(id),
                any_focus: w.focused.is_some(),
                mods: self.mods,
            };
            widget.event(&mut cx, &local)
        };
        w.tree.nodes[idx].widget = widget;
        if status == Status::Handled {
            out.handled_by = Some(idx);
        }
        self.apply(win, idx, out, platform);
        status
    }

    /// Hand `event` to `idx` and then its ancestors until one uses it.
    fn bubble(&mut self, win: WinId, mut idx: usize, event: &Event, platform: &mut dyn Platform) -> Option<usize> {
        loop {
            if self.send(win, idx, event, platform) == Status::Handled {
                return Some(idx);
            }
            idx = self.wins.get(&win)?.tree.nodes.get(idx)?.parent?;
        }
    }

    /// Carry out what a handler asked for.
    fn apply(&mut self, win: WinId, idx: usize, out: Out, platform: &mut dyn Platform) {
        let Out {
            msgs,
            damage,
            rebuild,
            relayout,
            focus,
            capture,
            cursor,
            ime,
            ticks,
            menus,
            close_popup,
            reveal,
            begin_move,
            drag,
            ..
        } = out;
        if let Some(source) = drag {
            // The pointer is the compositor's now: nothing here is pressed.
            if let Some(w) = self.wins.get_mut(&win) {
                w.active = None;
            }
            self.start_drag(win, source, platform);
        }
        if relayout
            && let Some(w) = self.wins.get_mut(&win)
            && let Some(n) = w.tree.nodes.get(idx)
        {
            w.relayout.push(n.id);
        }
        for (node, msg) in msgs {
            let msg = self.map_up(win, node, msg);
            self.queue.push_back(msg);
        }
        let bounds = self.wins.get(&win).map(|w| Rect::from_origin_size(Point::ZERO, w.size));
        for r in damage {
            let r = bounds.map_or(r, |b| r.intersect(b));
            if r.area() > 0.0 {
                platform.invalidate(win, r);
            }
        }
        let menu_popup =
            matches!(self.wins.get(&win).map(|w| &w.kind), Some(WinKind::Popup { kind: PopupKind::Menu { .. }, .. }));
        if rebuild && menu_popup {
            self.relayout_popup(win, platform);
        }
        // A popover's content comes from its owner's view.
        let owner = match self.wins.get(&win).map(|w| &w.kind) {
            Some(WinKind::Popup { owner, .. }) if rebuild && !menu_popup => Some(*owner),
            _ => None,
        };
        if let Some(o) = owner.and_then(|o| self.wins.get_mut(&o)) {
            o.dirty = true;
        }
        if let Some(w) = self.wins.get_mut(&win) {
            if rebuild && !menu_popup && owner.is_none() {
                w.dirty = true;
            }
            match capture {
                Some(true) => w.active = w.tree.nodes.get(idx).map(|n| n.id),
                Some(false) => w.active = None,
                None => {}
            }
            if let Some(c) = cursor
                && c != w.cursor
            {
                w.cursor = c;
                platform.set_cursor(win, c);
            }
            if let Some(i) = ime
                && i != w.ime
            {
                w.ime = i;
                platform.set_text_input(win, i.0, i.1);
            }
        }
        for (node, after) in ticks {
            self.tick_later(win, node, after, platform);
        }
        if begin_move {
            platform.begin_move(win);
        }
        if let Some((node, rect)) = reveal {
            self.reveal(win, node, rect, platform);
        }
        match focus {
            Some(FocusRequest::Node(id)) => self.set_focus(win, Some(id), false, platform),
            Some(FocusRequest::Clear) => self.set_focus(win, None, false, platform),
            Some(FocusRequest::Next) => self.move_focus(win, true, platform),
            Some(FocusRequest::Previous) => self.move_focus(win, false, platform),
            None => {}
        }
        for m in menus {
            self.open_menu_request(win, m, platform);
        }
        if close_popup {
            self.close_popup(win, platform);
        }
    }

    /// A tick for `node` after `after`, unless one is on its way.
    fn tick_later(&mut self, win: WinId, node: NodeId, after: Duration, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        if !w.ticking.insert(node) {
            return;
        }
        let token = platform.set_timer(after, false);
        self.timers.insert(token, TimerUse::Tick { win, node, after });
    }

    /// A message from node `idx` through the mappers above it.
    fn map_up(&self, win: WinId, idx: usize, mut msg: AnyMsg) -> AnyMsg {
        let Some(w) = self.wins.get(&win) else { return msg };
        let mut at = Some(idx);
        while let Some(i) = at {
            let Some(n) = w.tree.nodes.get(i) else { break };
            if let Some(m) = &n.common.mapper {
                msg = m(msg);
            }
            at = n.parent;
        }
        if let Some(m) = &w.mapper {
            msg = m(msg);
        }
        msg
    }

    /// Have the scroll views around node `idx` show `rect`.
    fn reveal(&mut self, win: WinId, idx: usize, rect: Rect, platform: &mut dyn Platform) {
        let mut at = self.wins.get(&win).and_then(|w| w.tree.nodes.get(idx)).and_then(|n| n.parent);
        while let Some(i) = at {
            let Some(w) = self.wins.get_mut(&win) else { return };
            let mut widget = std::mem::replace(&mut w.tree.nodes[i].widget, Box::new(Vacant));
            let mut out = Out::default();
            {
                let mut env = Env {
                    focus: w.active_focus(),
                    theme: &self.theme,
                    text: &mut self.text,
                    states: &mut w.states,
                    now: Instant::now(),
                    on_selection: false,
                };
                let mut cx = EventCx {
                    tree: &w.tree,
                    node: i,
                    win,
                    env: &mut env,
                    out: &mut out,
                    platform: &mut *platform,
                    hovered: false,
                    pressed: false,
                    focused: false,
                    any_focus: true,
                    mods: self.mods,
                };
                widget.reveal(&mut cx, rect);
            }
            w.tree.nodes[i].widget = widget;
            at = w.tree.nodes[i].parent;
            self.apply(win, i, out, platform);
        }
    }

    /// Give `id` the focus (or none).
    pub fn set_focus(&mut self, win: WinId, id: Option<NodeId>, visible: bool, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        let id = id.filter(|id| w.input_node(*id).is_some());
        if w.focused == id {
            w.focus_visible = visible || w.focus_visible;
            return;
        }
        let old = std::mem::replace(&mut w.focused, id);
        w.focus_visible = visible;
        // A list's selected rows change colours with its focus.
        w.relayout.extend(old.into_iter().chain(id));
        let old_idx = old.and_then(|o| w.tree.get(o));
        let new_idx = id.and_then(|n| w.tree.get(n));
        for i in [old_idx, new_idx].into_iter().flatten() {
            let r = w.tree.nodes[i].own_rect();
            platform.invalidate(win, r.inflate(3.0, 3.0));
        }
        if let Some(i) = old_idx {
            self.send(win, i, &Event::Focus(false), platform);
        }
        // A widget that doesn't take text stops input methods.
        if let Some(w) = self.wins.get_mut(&win)
            && w.ime.0
        {
            w.ime = (false, None);
            platform.set_text_input(win, false, None);
        }
        if let Some(i) = new_idx {
            self.send(win, i, &Event::Focus(true), platform);
            let r = self.wins[&win].tree.nodes[i].rect;
            self.reveal(win, i, r, platform);
        }
        // Hidden and drawn ahead: drawn again with the focus where it is.
        if self.wins.get(&win).is_some_and(|w| w.hidden && w.ahead.is_some()) {
            self.restart_ahead(win, platform);
        }
    }

    /// A new sheet/alert suspends the covered field, including its IME.
    /// Keep each layer's focus so dismissing an alert returns to the draft.
    fn restore_modal_focus(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        let modal = w.modal().map(|i| w.tree.nodes[i].id);
        if modal == w.focus_modal {
            return;
        }
        w.modal_focus.insert(w.focus_modal, (w.focused, w.focus_visible));
        w.focus_modal = modal;
        let (focus, visible) = w.modal_focus.remove(&modal).unwrap_or((None, false));
        w.modal_focus.retain(|id, _| id.is_none_or(|id| w.tree.get(id).is_some()));
        w.active = w.active.filter(|id| w.input_node(*id).is_some());
        self.set_focus(win, focus, visible, platform);
        // A menu from the covered layer must not retain an armed action.
        self.close_menus(win, platform);
    }

    /// Tab and Shift-Tab.
    pub fn move_focus(&mut self, win: WinId, forward: bool, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get(&win) else { return };
        if w.tree.root.is_none() {
            return;
        }
        let chain = w.tree.focus_chain(w.modal());
        if chain.is_empty() {
            return;
        }
        let at = w.focused.and_then(|f| w.tree.get(f)).and_then(|i| chain.iter().position(|c| *c == i));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => chain.len() - 1,
            (Some(i), true) => (i + 1) % chain.len(),
            (Some(i), false) => (i + chain.len() - 1) % chain.len(),
        };
        let id = w.tree.nodes[chain[next]].id;
        self.set_focus(win, Some(id), true, platform);
    }

    /// Focus the element named `name` in `win`, if it's there.
    pub fn focus_name(&mut self, win: WinId, name: &str, platform: &mut dyn Platform) -> bool {
        let Some(id) = self.wins.get(&win).and_then(|w| w.tree.names.get(name).copied()) else { return false };
        self.set_focus(win, Some(id), true, platform);
        true
    }

    fn update_hot(&mut self, win: WinId, p: Point, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        if w.tree.root.is_none() {
            return;
        }
        let path = w.tree.path_at(p, w.modal());
        let hot_idx = path.iter().rev().copied().find(|&i| w.tree.nodes[i].widget.hit());
        let hot = hot_idx.map(|i| w.tree.nodes[i].id);
        let tip = path
            .iter()
            .rev()
            .find_map(|&i| w.tree.nodes[i].common.tooltip.as_ref().map(|text| (w.tree.nodes[i].id, text.clone())));
        let hot_changed = hot != w.hot;
        let tip_changed = tip != w.tooltip_target;
        if !hot_changed && !tip_changed {
            return;
        }
        let old = if hot_changed { std::mem::replace(&mut w.hot, hot) } else { None };
        if hot_changed {
            for id in [old, hot].into_iter().flatten() {
                if let Some(i) = w.tree.get(id)
                    && w.tree.nodes[i].widget.hover_paints()
                {
                    platform.invalidate(win, w.tree.nodes[i].own_rect());
                }
            }
            let cursor = hot_idx.and_then(|i| w.tree.nodes[i].widget.cursor()).unwrap_or(Cursor::Default);
            if cursor != w.cursor {
                w.cursor = cursor;
                platform.set_cursor(win, cursor);
            }
        }
        // A rebuild can change a tooltip without changing the hot widget.
        // A pending tooltip reads its text when shown, so a text-only change
        // keeps its deadline. A visible tooltip changes immediately.
        let same_source = tip.as_ref().map(|t| t.0) == w.tooltip_target.as_ref().map(|t| t.0);
        let keep_pending = !hot_changed && same_source && w.tooltip.is_some();
        let refresh_shown = !hot_changed && w.tooltip_shown.is_some();
        w.tooltip_target = tip.clone();
        let shown = w.tooltip_shown.take();
        let pending = if keep_pending { None } else { w.tooltip.take() };
        if let Some((_, token)) = pending {
            platform.cancel_timer(token);
            self.timers.remove(&token);
        }
        if let Some(t) = shown {
            self.close_win(t, platform);
        }
        if let Some((node, _)) = tip {
            if refresh_shown {
                self.show_tooltip(win, node, platform);
            } else if !keep_pending {
                let token = platform.set_timer(Duration::from_millis(650), false);
                self.timers.insert(token, TimerUse::Tooltip { win, node });
                if let Some(w) = self.wins.get_mut(&win) {
                    w.tooltip = Some((node, token));
                }
            }
        }
        if let Some(old) = old
            && let Some(i) = self.wins.get(&win).and_then(|w| w.tree.get(old))
        {
            self.send(win, i, &Event::PointerLeave, platform);
        }
    }

    pub fn pointer_move(&mut self, win: WinId, p: Point, platform: &mut dyn Platform) {
        let started = Instant::now();
        let Some(w) = self.wins.get_mut(&win) else { return };
        w.pointer = Some(p);
        if crate::inspect::enabled() {
            self.inspect(win);
        }
        let Some(w) = self.wins.get(&win) else { return };
        if w.tree.root.is_none() {
            return;
        }
        let captured = w.active.and_then(|a| w.tree.get(a));
        self.update_hot(win, p, platform);
        let ev = Event::PointerMove { pos: p, mods: self.mods };
        match captured {
            Some(i) => {
                self.bubble(win, i, &ev, platform);
            }
            None => {
                let Some(w) = self.wins.get(&win) else { return };
                let path = w.tree.path_at(p, w.modal());
                if let Some(&last) = path.last() {
                    self.bubble(win, last, &ev, platform);
                }
            }
        }
        self.stats.event(started.elapsed());
    }

    pub fn pointer_leave(&mut self, win: WinId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        w.pointer = None;
        w.tooltip_target = None;
        let old = w.hot.take();
        if let Some((_, token)) = w.tooltip.take() {
            platform.cancel_timer(token);
        }
        if let Some(t) = w.tooltip_shown.take() {
            self.close_win(t, platform);
        }
        let Some(w) = self.wins.get(&win) else { return };
        if let Some(i) = old.and_then(|o| w.tree.get(o)) {
            if w.tree.nodes[i].widget.hover_paints() {
                platform.invalidate(win, w.tree.nodes[i].own_rect());
            }
            self.send(win, i, &Event::PointerLeave, platform);
        }
    }

    pub fn pointer_button(
        &mut self,
        win: WinId,
        p: Point,
        button: PointerButton,
        pressed: bool,
        clicks: u32,
        platform: &mut dyn Platform,
    ) {
        let started = Instant::now();
        let Some(w) = self.wins.get_mut(&win) else { return };
        w.pointer = Some(p);
        if w.tree.root.is_none() {
            return;
        }
        if let Some(t) = w.tooltip_shown.take() {
            self.close_win(t, platform);
        }
        let mods = self.mods;
        if pressed {
            // A press in a window closes the menus over it (outside them).
            let menus: Vec<WinId> = self.wins[&win]
                .popups
                .iter()
                .copied()
                .filter(|p| {
                    matches!(
                        self.wins.get(p).map(|w| &w.kind),
                        Some(WinKind::Popup { kind: PopupKind::Menu { .. }, .. })
                    )
                })
                .collect();
            for m in menus {
                self.close_win(m, platform);
            }
            self.update_hot(win, p, platform);
            let Some(w) = self.wins.get(&win) else { return };
            let path = w.tree.path_at(p, w.modal());
            let Some(&last) = path.last() else { return };
            // A click focuses what takes focus by clicking, else clears it.
            // What takes the focus by a click: the nearest such around the
            // pointer, unless a control (a button) is nearer, which leaves
            // the focus where it is.
            let mut target = None;
            for &i in path.iter().rev() {
                let wd = &w.tree.nodes[i].widget;
                if matches!(wd.focusable(), Focusable::Click | Focusable::Pointer) {
                    target = Some(i);
                    break;
                }
                if wd.hit() {
                    target = w.focused.and_then(|f| w.tree.get(f));
                    break;
                }
            }
            let focus = target.map(|i| w.tree.nodes[i].id);
            let ev = Event::PointerDown { pos: p, button, clicks, mods };
            let handled = self.bubble(win, last, &ev, platform);
            if button == PointerButton::Left {
                match focus {
                    Some(f) => self.set_focus(win, Some(f), false, platform),
                    None if handled.is_none_or(|h| {
                        self.wins.get(&win).is_some_and(|w| w.tree.nodes[h].widget.focusable() == Focusable::No)
                    }) =>
                    {
                        let keep = self.wins.get(&win).and_then(|w| w.focused).and_then(|f| {
                            let w = self.wins.get(&win)?;
                            let i = w.tree.get(f)?;
                            path.contains(&i).then_some(())
                        });
                        if keep.is_none() {
                            self.set_focus(win, None, false, platform);
                        }
                    }
                    None => {}
                }
            }
            if let Some(h) = handled
                && let Some(w) = self.wins.get_mut(&win)
            {
                // What took the press gets the release.
                if w.active.is_none() {
                    w.active = w.tree.nodes.get(h).map(|n| n.id);
                }
                let r = w.tree.nodes[h].own_rect();
                if w.tree.nodes[h].widget.hover_paints() {
                    platform.invalidate(win, r);
                }
            }
        } else {
            let Some(w) = self.wins.get_mut(&win) else { return };
            let active = w.active.take().and_then(|a| w.tree.get(a));
            let ev = Event::PointerUp { pos: p, button, mods };
            match active {
                Some(i) => {
                    if w.tree.nodes[i].widget.hover_paints() {
                        platform.invalidate(win, w.tree.nodes[i].own_rect());
                    }
                    self.bubble(win, i, &ev, platform);
                }
                None => {
                    let path = w.tree.path_at(p, w.modal());
                    if let Some(&last) = path.last() {
                        self.bubble(win, last, &ev, platform);
                    }
                }
            }
            self.update_hot(win, p, platform);
        }
        self.stats.event(started.elapsed());
    }

    pub fn scroll(
        &mut self,
        win: WinId,
        p: Point,
        delta: Vec2,
        phase: ScrollPhase,
        momentum: ScrollPhase,
        platform: &mut dyn Platform,
    ) {
        let started = Instant::now();
        let Some(w) = self.wins.get(&win) else { return };
        if w.tree.root.is_none() {
            return;
        }
        let path = w.tree.path_at(p, w.modal());
        if let Some(&last) = path.last() {
            let ev = Event::Scroll { pos: p, delta, phase, momentum, mods: self.mods };
            self.bubble(win, last, &ev, platform);
        }
        self.stats.event(started.elapsed());
    }

    /// A pinch at `p`, to the widget under it (and its parents).
    pub fn pinch(&mut self, win: WinId, p: Point, scale: f64, phase: ScrollPhase, platform: &mut dyn Platform) {
        let started = Instant::now();
        let Some(w) = self.wins.get(&win) else { return };
        if w.tree.root.is_none() {
            return;
        }
        let path = w.tree.path_at(p, w.modal());
        if let Some(&last) = path.last() {
            let ev = Event::Pinch { pos: p, scale, phase, mods: self.mods };
            self.bubble(win, last, &ev, platform);
        }
        self.stats.event(started.elapsed());
    }

    pub fn window_focus(&mut self, win: WinId, on: bool, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        w.has_keyboard = on;
        if let Some(f) = w.focused {
            w.relayout.push(f);
        }
        if on && !w.is_popup() {
            self.key_window = Some(win);
        }
        let Some(w) = self.wins.get(&win) else { return };
        if let Some(i) = w.focused.and_then(|f| w.tree.get(f)) {
            platform.invalidate(win, w.tree.nodes[i].own_rect().inflate(3.0, 3.0));
            self.send(win, i, &Event::WindowFocus(on), platform);
        }
    }

    pub fn ime(&mut self, win: WinId, ime: Ime, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get(&win) else { return };
        if let Some(i) = w.focused.and_then(|f| w.input_node(f)) {
            self.send(win, i, &Event::Ime(ime), platform);
        }
    }

    /// A key: menus' shortcuts, then the focused widget and its parents,
    /// then the kit's own keys (Tab, Escape, Return in sheets).
    pub fn key(&mut self, win: WinId, ev: KeyEvent, platform: &mut dyn Platform) {
        let started = Instant::now();
        self.mods = ev.modifiers;
        if !self.wins.contains_key(&win) {
            return;
        }
        let m = ev.modifiers;
        if ev.pressed && (m.super_key() || m.control()) {
            let found = self.menubar.iter().find_map(|menu| menu.find_shortcut(&ev).cloned());
            if let Some(a) = found {
                let mut a = a;
                // Settle Edit items against the focused widget.
                let mut probe = MenuSpec { title: String::new(), items: vec![ItemSpec::Action(a.clone())], icon: None };
                self.settle_app_menu(&mut probe, Some(self.app_window_for(win)));
                if let Some(ItemSpec::Action(s)) = probe.items.first() {
                    a.enabled = s.enabled;
                }
                if a.enabled || a.msg.is_some() {
                    let target = self.app_window_for(win);
                    self.perform(&a, target, platform);
                    self.stats.event(started.elapsed());
                    return;
                }
            }
        }
        let w = &self.wins[&win];
        let focused = w.focused.and_then(|f| w.input_node(f));
        // With nothing focused, keys go to what's under the pointer.
        let under = || w.pointer.and_then(|p| w.tree.path_at(p, w.modal()).last().copied());
        let target = focused.or_else(|| if w.is_popup() { w.tree.root } else { under() });
        let handled = match target {
            Some(i) => self.bubble(win, i, &Event::Key(ev.clone()), platform).is_some(),
            None => false,
        };
        if !handled && ev.pressed {
            self.default_key(win, &ev, focused, platform);
        }
        self.stats.event(started.elapsed());
    }

    /// The app window a popup (or the window itself) belongs to.
    pub fn app_window_for(&self, mut win: WinId) -> WinId {
        while let Some(WinKind::Popup { owner, .. }) = self.wins.get(&win).map(|w| &w.kind) {
            win = *owner;
        }
        win
    }

    pub fn start_drag(
        &mut self,
        win: WinId,
        source: sidestep_ui::DragSource,
        platform: &mut dyn Platform,
    ) -> Option<crate::program::DragId> {
        let window = self.wins.get(&self.app_window_for(win))?.key()?;
        let drag = crate::program::DragId(platform.start_drag(win, source)?);
        self.queue.push_back(Box::new(KitMsg::DragStarted { window, drag }));
        Some(drag)
    }

    fn default_key(&mut self, win: WinId, ev: &KeyEvent, focused: Option<usize>, platform: &mut dyn Platform) {
        let m = ev.modifiers;
        match &ev.key {
            Key::Named(NamedKey::Tab) if !m.control() && !m.super_key() => self.move_focus(win, !m.shift(), platform),
            Key::Named(NamedKey::Escape) => {
                let w = &self.wins[&win];
                if w.is_popup() {
                    self.popup_dismissed(win, platform);
                    return;
                }
                // A popover over the window, then a sheet's cancel button.
                if let Some(&p) = w.popups.last() {
                    self.popup_dismissed(p, platform);
                    return;
                }
                if let Some(i) = self.find_in_modal(win, |n| n.widget.is_cancel()) {
                    self.activate(win, i, platform);
                }
            }
            Key::Named(NamedKey::Enter) => {
                if let Some(f) = focused
                    && self.activate(win, f, platform)
                {
                    return;
                }
                if let Some(i) = self.find_in_modal(win, |n| n.widget.is_default()) {
                    self.activate(win, i, platform);
                }
            }
            Key::Character(s) if s == " " && m.is_empty() || s == " " && m == Modifiers::SHIFT => {
                if let Some(f) = focused {
                    self.activate(win, f, platform);
                }
            }
            _ => {}
        }
    }

    /// The first node in the topmost sheet (or the window) matching `f`.
    fn find_in_modal(&self, win: WinId, f: impl Fn(&crate::tree::Node) -> bool) -> Option<usize> {
        let w = self.wins.get(&win)?;
        let start = w.modal().or(w.tree.root)?;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            let n = &w.tree.nodes[i];
            if n.dead {
                continue;
            }
            if f(n) {
                return Some(i);
            }
            stack.extend(n.children.iter().rev().copied());
        }
        None
    }

    fn activate(&mut self, win: WinId, idx: usize, platform: &mut dyn Platform) -> bool {
        let Some(w) = self.wins.get_mut(&win) else { return false };
        let Some(node) = w.tree.nodes.get_mut(idx) else { return false };
        let mut widget = std::mem::replace(&mut node.widget, Box::new(Vacant));
        let id = w.tree.nodes[idx].id;
        let mut out = Out::default();
        let done = {
            let mut env = Env {
                focus: w.active_focus(),
                theme: &self.theme,
                text: &mut self.text,
                states: &mut w.states,
                now: Instant::now(),
                on_selection: false,
            };
            let mut cx = EventCx {
                tree: &w.tree,
                node: idx,
                win,
                env: &mut env,
                out: &mut out,
                platform: &mut *platform,
                hovered: w.hot == Some(id),
                pressed: false,
                focused: w.focused == Some(id),
                any_focus: w.focused.is_some(),
                mods: self.mods,
            };
            widget.activate(&mut cx)
        };
        w.tree.nodes[idx].widget = widget;
        self.apply(win, idx, out, platform);
        done
    }

    /// Carry out a menu item: its role, then its message.
    pub fn perform(&mut self, a: &ActionSpec, win: WinId, platform: &mut dyn Platform) {
        let win = self.app_window_for(win);
        if a.role == Some(MenuRole::Quit) && self.wins.values().any(Win::has_modal) {
            return;
        }
        if self.has_modal(win) {
            if let Some(cmd) = a.role.and_then(MenuRole::edit_command) {
                // Do not fall through to an app's Undo/Find/etc. if the
                // dialog's focused widget cannot handle the command.
                self.command(win, cmd, platform);
                return;
            }
            if !Self::window_command(a) {
                return;
            }
        }
        if let Some(role) = a.role {
            if let Some(cmd) = role.edit_command()
                && self.command(win, cmd, platform)
            {
                return;
            }
            if a.msg.is_none() {
                self.role(role, win, a.id, platform);
                return;
            }
        }
        if let Some(m) = &a.msg {
            self.queue.push_back(m.make());
        }
    }

    /// The focused widget's selected text, in the key window.
    pub fn focused_selection(&self) -> Option<String> {
        let w = self.wins.get(&self.key_window?)?;
        let i = w.input_node(w.focused?)?;
        let n = &w.tree.nodes[i];
        n.widget.selection(crate::widget::StateView::new(&w.states, n.id))
    }

    /// A drag over `win` at `p`: whether something there takes it.
    pub fn drag_over(
        &mut self,
        win: WinId,
        p: Point,
        urls: &[String],
        has_text: bool,
        platform: &mut dyn Platform,
    ) -> bool {
        let Some(w) = self.wins.get(&win) else { return false };
        if w.tree.root.is_none() {
            return false;
        }
        let path = w.tree.path_at(p, w.modal());
        let Some(&last) = path.last() else { return false };
        let ev = Event::DragOver { pos: p, urls: urls.to_vec(), has_text };
        self.bubble(win, last, &ev, platform).is_some()
    }

    pub fn drag_left(&mut self, win: WinId, platform: &mut dyn Platform) {
        let _ = (win, platform);
    }

    /// A drop on `win` at `p`: whether something there took it.
    pub fn drop(
        &mut self,
        win: WinId,
        p: Point,
        urls: Vec<String>,
        text: Option<String>,
        platform: &mut dyn Platform,
    ) -> bool {
        let Some(w) = self.wins.get(&win) else { return false };
        if w.tree.root.is_none() {
            return false;
        }
        let path = w.tree.path_at(p, w.modal());
        let Some(&last) = path.last() else { return false };
        let ev = Event::Drop { pos: p, urls, text };
        self.bubble(win, last, &ev, platform).is_some()
    }

    /// A context menu at `at` in `win`, from the program.
    pub fn open_context_menu(&mut self, win: WinId, at: Point, spec: MenuSpec, platform: &mut dyn Platform) {
        let anchor = Rect::from_origin_size(at, (1.0, 1.0));
        self.show_menu(win, anchor, spec, MenuLook::default(), Placement::AtPoint, None, platform);
    }

    /// An Edit command to the focused widget of `win` (or of the key
    /// window): whether one took it.
    pub fn command(&mut self, win: WinId, cmd: EditCommand, platform: &mut dyn Platform) -> bool {
        let win = if self.wins.get(&win).is_some_and(|w| w.focused.is_some() || w.has_modal()) {
            win
        } else {
            self.key_window.unwrap_or(win)
        };
        let Some(w) = self.wins.get_mut(&win) else { return false };
        let Some(idx) = w.focused.and_then(|f| w.input_node(f)) else { return false };
        let mut widget = std::mem::replace(&mut w.tree.nodes[idx].widget, Box::new(Vacant));
        let id = w.tree.nodes[idx].id;
        let mut out = Out::default();
        let done = {
            let mut env = Env {
                focus: w.active_focus(),
                theme: &self.theme,
                text: &mut self.text,
                states: &mut w.states,
                now: Instant::now(),
                on_selection: false,
            };
            let mut cx = EventCx {
                tree: &w.tree,
                node: idx,
                win,
                env: &mut env,
                out: &mut out,
                platform: &mut *platform,
                hovered: w.hot == Some(id),
                pressed: false,
                focused: true,
                any_focus: true,
                mods: self.mods,
            };
            widget.command(&mut cx, cmd)
        };
        w.tree.nodes[idx].widget = widget;
        self.apply(win, idx, out, platform);
        done
    }

    /// What a role does when the item has no message of its own.
    fn role(&mut self, role: MenuRole, win: WinId, id: Option<u32>, platform: &mut dyn Platform) {
        let app = self.app_window_for(win);
        match role {
            MenuRole::Quit => self.queue.push_back(Box::new(KitMsg::Quit)),
            MenuRole::CloseWindow => {
                if let Some(k) = self.wins.get(&app).and_then(|w| w.key()) {
                    self.queue.push_back(Box::new(KitMsg::CloseRequest(k)));
                }
            }
            MenuRole::Minimise => match &mut self.wm {
                Some(wm) => wm.minimise(app),
                None => platform.minimize(app),
            },
            MenuRole::WindowList => {
                if let Some(target) = id.and_then(|i| i.checked_sub(0x5700_0000)) {
                    platform.activate(target);
                }
            }
            MenuRole::About => crate::dialogs::show_about(self, app),
            // The shell's palette, on the app's commands.
            MenuRole::HelpSearch => {
                let request =
                    aizome_shell_proto::ShellRequest::Palette { scope: Some("commands".into()), query: String::new() };
                self.queue.push_back(Box::new(KitMsg::AskShell(request)));
            }
            MenuRole::Tour => match std::process::Command::new("aizome-annai").spawn() {
                Ok(mut child) => {
                    // Reaped when it ends.
                    std::thread::spawn(move || child.wait());
                }
                Err(e) => log::warn!("the tour (aizome-annai): {e}"),
            },
            MenuRole::Hide
            | MenuRole::HideOthers
            | MenuRole::ShowAll
            | MenuRole::Fill
            | MenuRole::StackWith
            | MenuRole::BringAllToFront => {
                if let Some(wm) = &mut self.wm {
                    wm.perform(role, app);
                } else {
                    log::info!("{} needs the compositor (aizome-ipc)", role.name());
                }
            }
            _ => log::debug!("menu role {} has nothing to do", role.name()),
        }
    }

    /// A kit message.
    pub fn kit(&mut self, msg: KitMsg, platform: &mut dyn Platform) {
        match msg {
            KitMsg::MenuChoose { window, node, path } => self.menu_choose(window, node, path, platform),
            KitMsg::MenuSub { window, node, path, anchor } => self.menu_sub(window, node, path, anchor, platform),
            KitMsg::MenuCloseSubs { window } => {
                for s in self.menu_popups_of(window) {
                    self.close_win(s, platform);
                }
            }
            KitMsg::MenuFocusSub { window } => {
                if let Some(&sub) = self.menu_popups_of(window).last() {
                    self.highlight_first_row(sub, platform);
                }
            }
            KitMsg::MenuBack { window } => {
                let owner = match self.wins.get(&window).map(|w| &w.kind) {
                    Some(WinKind::Popup { owner, kind: PopupKind::Menu { .. } }) => Some(*owner),
                    _ => None,
                };
                if owner.is_some_and(|o| {
                    matches!(
                        self.wins.get(&o).map(|w| &w.kind),
                        Some(WinKind::Popup { kind: PopupKind::Menu { .. }, .. })
                    )
                }) {
                    self.close_win(window, platform);
                }
            }
            KitMsg::CloseMenus { window } => self.close_menus(window, platform),
            KitMsg::OpenBarMenu { window, index, anchor } => self.open_bar_menu(window, index, anchor, platform),

            KitMsg::AlertChoice { alert, index } => crate::dialogs::alert_chosen(self, alert, index),
            KitMsg::Panel { panel, msg } => crate::dialogs::panel_message(self, panel, msg, platform),
            // The runtime handles these (they need the program).
            KitMsg::Quit | KitMsg::CloseRequest(_) | KitMsg::DragStarted { .. } | KitMsg::AskShell(_) => {}
        }
    }

    /// A timer came due: whether it was the kit's.
    pub fn timer(&mut self, token: u64, platform: &mut dyn Platform) {
        let Some(use_) = self.timers.get(&token) else { return };
        match use_ {
            TimerUse::App { msg, repeat } => {
                let m = msg.make();
                if !*repeat {
                    self.timers.remove(&token);
                }
                self.queue.push_back(m);
            }
            TimerUse::Tick { win, node, after } => {
                let (win, node, after) = (*win, *node, *after);
                self.timers.remove(&token);
                if let Some(w) = self.wins.get_mut(&win) {
                    // A hidden window's widgets wait (a caret doesn't blink
                    // unseen), still ticking, asked again when it shows.
                    if w.hidden {
                        w.held.push((node, after));
                        return;
                    }
                    w.ticking.remove(&node);
                }
                if let Some(i) = self.wins.get(&win).and_then(|w| w.tree.get(node)) {
                    self.send(win, i, &Event::Tick, platform);
                }
            }
            TimerUse::Tooltip { win, node } => {
                let (win, node) = (*win, *node);
                self.timers.remove(&token);
                self.show_tooltip(win, node, platform);
            }
            TimerUse::Menus => {
                self.timers.remove(&token);
            }
            TimerUse::Ahead { win } => {
                let win = *win;
                self.timers.remove(&token);
                self.draw_ahead(win, platform);
            }
        }
    }

    fn show_tooltip(&mut self, win: WinId, node: NodeId, platform: &mut dyn Platform) {
        let Some(w) = self.wins.get_mut(&win) else { return };
        w.tooltip = None;
        let Some(i) = w.tree.get(node) else { return };
        let Some(text) = w.tree.nodes[i].common.tooltip.clone() else { return };
        let anchor = w.tree.nodes[i].rect;
        let root = TooltipView::element(&text);
        let id = self.open_popup(win, anchor, Placement::Below, false, PopupKind::Tooltip, root, None, platform);
        if let Some(w) = self.wins.get_mut(&win) {
            w.popups.retain(|p| *p != id);
            w.tooltip_shown = Some(id);
        }
    }

    // Drawing.

    pub fn paint(&mut self, win: WinId, canvas: &mut Canvas) {
        let started = Instant::now();
        let Some(w) = self.wins.get(&win) else { return };
        if w.tree.root.is_none() {
            return;
        }
        let mut env = PaintEnv {
            theme: &self.theme,
            text: &mut self.text,
            states: &w.states,
            hot: w.hot,
            active: w.active,
            focused: w.focused,
            focus_visible: w.focus_visible,
            window_active: w.has_keyboard || w.is_popup(),
            now: Instant::now(),
        };
        w.tree.paint(canvas, &mut env);
        // The focus ring, over everything.
        if let Some(i) = w.focused.and_then(|f| w.tree.get(f)) {
            let n = &w.tree.nodes[i];
            let ring = n.widget.focus_ring();
            let show = match ring {
                crate::widget::FocusRing::Always => true,
                crate::widget::FocusRing::Keyboard => w.focus_visible,
                crate::widget::FocusRing::Never => false,
            };
            if show && (w.has_keyboard || w.is_popup()) {
                canvas.save();
                canvas.clip_rect(n.clip.inflate(3.0, 3.0));
                crate::draw::focus_ring(canvas, n.rect, self.theme.focus());
                canvas.restore();
            }
        }
        if crate::inspect::enabled() {
            self.inspect(win);
        }
        self.stats.paint(started.elapsed());
    }

    /// Write what window `win` shows for tests that drive the app from
    /// outside (`AIZOME_KIT_INSPECT`).
    fn inspect(&self, win: WinId) {
        let Some(w) = self.wins.get(&win) else { return };
        let (kind, owner) = match &w.kind {
            WinKind::App { spec, .. } => (
                match spec.kind {
                    WindowKind::Normal => "window",
                    WindowKind::Dialog { .. } => "dialog",
                    WindowKind::Utility => "utility",
                    _ => "surface",
                },
                None,
            ),
            WinKind::Popup { owner, kind } => (
                match kind {
                    PopupKind::Menu { .. } => "menu",
                    PopupKind::Popover { .. } => "popover",
                    PopupKind::Tooltip => "tooltip",
                },
                Some(*owner),
            ),
        };
        let header = serde_json::json!({
            "title": w.title,
            "kind": kind,
            "owner": owner,
            "size": [w.size.width, w.size.height],
            "menu_strip": self.menu_strip_of(win),
            "has_keyboard": w.has_keyboard,
            "pointer": w.pointer.map(|p| [p.x, p.y]),
        });
        crate::inspect::painted(&self.info.app_id, win, header, &w.tree, w.focused);
    }

    // Re-theming.

    pub fn retheme(&mut self, appearance: Appearance, platform: &mut dyn Platform) {
        let theme = self.theme.rethemed(appearance);
        let changed =
            theme.palette != self.theme.palette || theme.metrics != self.theme.metrics || appearance != self.appearance;
        if !changed {
            return;
        }
        self.appearance = appearance;
        self.theme = theme;
        self.text.clear();
        let ids: Vec<WinId> = self.wins.keys().copied().collect();
        for id in ids {
            if let Some(w) = self.wins.get_mut(&id) {
                w.dirty = true;
                w.built = false;
                // Drawn ahead, it's drawn again in the new look.
                w.refresh = w.hidden && w.ahead.is_some();
            }
            platform.invalidate_all(id);
        }
        // Popups (menus, tooltips) close rather than change under the
        // pointer.
        let popups: Vec<WinId> = self.wins.values().filter(|w| w.is_popup()).map(|w| w.id).collect();
        for p in popups {
            if let Some(w) = self.wins.get(&p)
                && matches!(w.kind, WinKind::Popup { kind: PopupKind::Menu { .. } | PopupKind::Tooltip, .. })
            {
                self.close_win(p, platform);
            }
        }
    }
}

/// An event in a widget's own coordinates.
/// The damage of `tree` laid out anew since `before`: content that
/// scrolled moves as pixels (the platform hears that first), and the
/// rest is drawn again.
fn settle(
    tree: &mut Tree,
    before: &Snapshot,
    focus: Option<NodeId>,
    win: WinId,
    platform: &mut dyn Platform,
) -> Vec<Rect> {
    // `AIZOME_NO_PIXEL_SCROLL=1` draws what scrolled again instead (to
    // compare, or where moving pixels misbehaves).
    static DRAW_SCROLLS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let draw = *DRAW_SCROLLS.get_or_init(|| std::env::var_os("AIZOME_NO_PIXEL_SCROLL").is_some_and(|v| v != "0"));
    let (moved, mut damage) = tree.take_scrolls(before, !draw);
    for m in &moved {
        platform.scroll(win, m.rect, m.delta);
    }
    damage.extend(tree.diff_moved(before, &moved, focus));
    damage.extend(tree.take_local_damage());
    damage
}

fn localize(ev: &Event, origin: Vec2) -> Event {
    match ev {
        Event::PointerMove { pos, mods } => Event::PointerMove { pos: *pos - origin, mods: *mods },
        Event::PointerDown { pos, button, clicks, mods } => {
            Event::PointerDown { pos: *pos - origin, button: *button, clicks: *clicks, mods: *mods }
        }
        Event::PointerUp { pos, button, mods } => Event::PointerUp { pos: *pos - origin, button: *button, mods: *mods },
        Event::Scroll { pos, delta, phase, momentum, mods } => {
            Event::Scroll { pos: *pos - origin, delta: *delta, phase: *phase, momentum: *momentum, mods: *mods }
        }
        Event::Drop { pos, urls, text } => Event::Drop { pos: *pos - origin, urls: urls.clone(), text: text.clone() },
        Event::DragOver { pos, urls, has_text } => {
            Event::DragOver { pos: *pos - origin, urls: urls.clone(), has_text: *has_text }
        }
        other => other.clone(),
    }
}

/// An element that fills what it's given.
fn fill(mut el: RawElement) -> RawElement {
    if el.common.width == Length::Shrink {
        el.common.width = Length::Fill;
    }
    if el.common.height == Length::Shrink {
        el.common.height = Length::Fill;
    }
    el
}
