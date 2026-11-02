//! The shell's program: its state, its messages, and which surface shows
//! what. Each surface's view and handling lives in its own module; this
//! one keeps them together and talks to the threads (the compositor's IPC,
//! the shell socket, system services, the meters).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aizome_config::{DeskbarSettings, LockSettings, MenubarSettings, NotificationSettings, PaletteSettings, Settings};
use aizome_ipc::{AppSel, Event, OutputInfo, Request};
use aizome_kit::element::Element;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::prelude::*;
use aizome_kit::{
    ActivationRequest, Anchors, KeyboardInteractivity, Layer, LayerOptions, MenuStyle, MsgSender, Output,
};
use aizome_shell_proto as proto;
use aizome_sys::apps::AppIndex;

use crate::actions::{Action, SessionVerb};
use crate::apps::registry::{Registry, RegistryChange};
use crate::apps::server::{Server, ServerEvent};
use crate::clock::Local;
use crate::ipc::{Ipc, IpcEvent};
use crate::launch::Launch;
use crate::menubar::{BarModel, BarMsg, Slot, Tray, TrayWidget};
use crate::menus::build;
use crate::menus::model::Menu as ShellMenu;
use crate::systray::meters::Reading;
use crate::wm::Wm;

/// What the shell is told.
#[derive(Clone, Debug)]
pub enum Msg {
    Ipc(IpcEvent),
    Server(ServerEvent),
    /// The installed apps were read.
    Apps(AppsLoaded),
    /// The apps' directories changed.
    AppsChanged,
    /// A system service changed.
    Sys,
    Meters(Reading),
    /// Keep Awake changed (on, off, or on without its locks).
    Awake(aizome_awake::Status),
    /// `aizome-awake on` or `off` finished: on or off, or why not.
    AwakeSet(Result<bool, String>),
    /// The clock's label turned.
    Tick,
    TimeChanged,
    /// The settings file changed.
    SettingsChanged,
    Run(Action),
    Startup(crate::startup::Event),
    ActivationExpired(ActivationRequest),
    Bar(WindowKey, BarMsg),
    Deskbar(crate::deskbar::DeskbarMsg),
    Palette(crate::palette::PaletteEvent),
    Panel(crate::systray::panels::PanelMsg),
    /// The OSD of generation `n` has shown long enough.
    OsdHide(u64),
    Desktop(u32, crate::desktop::DesktopMsg),
    /// What's in `~/Desktop`, and whether the Trash is full.
    DesktopItems(Vec<(PathBuf, bool)>, bool),
    Lock(crate::lock::LockMsg),
    Auth(crate::auth::AuthIn),
    PolkitReady(Result<Arc<aizome_sys::polkit::PolkitAgent>, String>),
    AuthUi(crate::auth::AuthMsg),
    Tear(crate::tearoff::TearMsg),
    Confirm(crate::confirm::ConfirmMsg),
    Card(crate::notifications::cards::CardMsg),
    /// A menu's strip was dragged: keep it as a palette.
    TearOff {
        app: String,
        path: Vec<String>,
        output: String,
        at: Point,
    },
    /// The selected text for Services came (from the app or the primary
    /// selection), for a menu waiting on it.
    Selection(u64, Option<String>),
    Note(crate::notifications::server::NoteEvent),
    /// What the shell this one replaces held last (a restart).
    Carried(Box<crate::carry::Carried>),
    /// Idle after starting: warm the palette.
    WarmPalette,
    /// Draw the palette ahead again, if it was asked for last (the ask's
    /// count).
    PreparePalette(u64),
    /// An output's wallpaper, scaled (or not: it didn't read).
    Wallpaper(crate::wallpaper::WallKey, Option<aizome_kit::Image>),
    /// The bell's flash of generation `n` has shown long enough.
    FlashDone(u64),
    /// logind's `HandlePowerKey`, read.
    LogindPowerKey(Option<String>),
    /// Koyomi's events and Tokei's world clocks, read.
    Agenda(crate::agenda::Agenda),
    /// The bar's menu of generation `n` closed.
    MenuClosed(u64),
    /// ← (-1) or → (1) past the bar's open menu.
    BarSide(i32),
    /// Typed in the Toolchest: the palette takes over.
    ToolchestTyped(String),
    /// The palette has had time to show: the Toolchest closes, if the
    /// compositor hasn't closed it already.
    ToolchestHandedOver,
    /// The bar's notification line has shown long enough.
    BannerDone(u32),
    /// What a recording's thread says (the recording's serial).
    Recording(u64, crate::recording::run::Event),
    /// The recording indicator's second turned (recording, tick).
    RecordTick(u64, u64),
    /// Where GeoClue says the computer is (latitude, longitude), or why
    /// it says nothing.
    Located(Result<(f64, f64), String>),
    /// Time to play the volume's feedback (it changed a moment ago).
    VolumeFeedback,
    /// A status item that turned out to be only a menu was clicked: its
    /// menu (bar, slot, where).
    ItemMenuWanted(WindowKey, u64, Rect),
    /// A status item's menu, read.
    ItemMenu(WindowKey, u64, Rect, Result<aizome_sys::statusnotifier::MenuItem, String>),
    /// Nothing: a message to wake the loop.
    Nothing,
}

/// The app index, read on a thread.
#[derive(Clone)]
pub struct AppsLoaded(pub Arc<AppIndex>);

impl std::fmt::Debug for AppsLoaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppsLoaded")
    }
}

/// What a window of the shell is.
#[derive(Clone, Debug, PartialEq)]
pub enum Surface {
    MenuBar {
        output: u32,
    },
    Desktop {
        output: u32,
    },
    Deskbar,
    /// The width an edge drag would give the Deskbar, over it.
    DeskbarPreview,
    /// Where a drag of its grip would put the Deskbar, at the other edge.
    DeskbarLanding,
    Palette,
    Panel,
    Osd,
    Lock {
        output: u32,
    },
    Auth,
    TearOff(usize),
    Confirm,
    Cards,
    Startup,
    /// The bell's flash.
    Flash,
}

/// An output and the shell's surfaces on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub output: Output,
    pub bar: Option<WindowKey>,
    pub desktop: Option<WindowKey>,
}

impl Screen {
    /// The connector's name (`eDP-1`), as the compositor's IPC names it.
    pub fn name(&self) -> String {
        self.output.connector.clone().unwrap_or_else(|| self.output.name.clone())
    }
}

/// Whether a connector is a built-in panel (a laptop's eDP, LVDS or DSI),
/// the kind whose light `/sys/class/backlight` sets.
pub fn builtin_panel(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| connector.starts_with(p))
}

/// The bytes of window memory a surface `size` points big takes at
/// `scale` (a buffer of device pixels, four bytes each).
pub(crate) fn surface_bytes(size: aizome_kit::Size, scale: f64) -> usize {
    ((size.width * scale).ceil().max(0.0) * (size.height * scale).ceil().max(0.0)) as usize * 4
}

impl Shell {
    /// Whether one of the outputs shown is a built-in panel: only then do
    /// the brightness keys and sliders reach the machine's backlight. A
    /// session on other outputs (headless, nested in another desktop, a
    /// desktop computer's monitors) leaves the machine's panel alone.
    pub fn shows_builtin_panel(&self) -> bool {
        self.screens.iter().any(|s| builtin_panel(&s.name()))
    }
}

/// How the shell starts.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Talk to the compositor, serve the shell socket, follow system
    /// services: off in tests, which send what they want heard.
    pub live: bool,
    /// The compositor's IPC socket (else `$AIZOME_SOCKET`).
    pub ipc_socket: Option<PathBuf>,
    /// The shell socket (else `$AIZOME_SHELL_SOCKET`).
    pub shell_socket: Option<PathBuf>,
}

/// The shell.
pub struct Shell {
    pub opts: Options,
    pub settings: Settings,
    pub bar_cfg: MenubarSettings,
    pub deskbar_cfg: DeskbarSettings,
    pub notes_cfg: NotificationSettings,
    pub lock_cfg: LockSettings,
    pub palette_cfg: PaletteSettings,
    pub desktop_cfg: aizome_config::DesktopSettings,
    pub power_cfg: aizome_config::PowerSettings,
    pub sound_cfg: aizome_config::SoundSettings,
    pub wm: Wm,
    pub ipc: Ipc,
    pub server: Option<Server>,
    pub registry: Registry,
    pub apps: Option<Arc<AppIndex>>,
    pub sys: crate::sys::Sys,
    pub tray: Tray,
    pub screens: Vec<Screen>,
    pub surfaces: HashMap<WindowKey, Surface>,
    pub deskbar: Option<WindowKey>,
    pub meters: Reading,
    pub now: Local,
    pub renaming: Option<(u32, String)>,
    pub shelf: crate::shelf::Shelf,
    /// Apps whose windows the person folded (true) or unfolded (false).
    pub folded: HashMap<String, bool>,
    pub notes: crate::notifications::store::Store,
    /// What was last saved for the next shell (`carry.rs`), and whether
    /// saving has begun (a restarted shell first takes in the old one's
    /// last word).
    pub carried: Option<u64>,
    pub carry_ready: bool,
    /// Another shell has taken over (a restart): this one saves nothing
    /// more and waits to be stopped.
    pub retiring: bool,
    /// Apps whose every unread notification shows.
    pub notes_open: Vec<String>,
    pub note_server: Option<crate::notifications::server::NoteServer>,
    /// The notification in the bar's line now, and those waiting for it.
    pub banner: Option<u32>,
    pub banner_queue: Vec<u32>,
    pub palette: Option<crate::palette::PaletteState>,
    /// The palette's window between openings, hidden and drawn ahead.
    pub palette_ahead: crate::palette::Ahead,
    pub frecency: aizome_search::frecency::Frecency,
    pub file_index: Option<Arc<aizome_search::files::FileIndex>>,
    pub settings_list: Vec<crate::palette::model::SettingEntry>,
    /// The theme showing is Day.
    pub theme_day: bool,
    /// What GTK was last told of the look (dconf).
    pub gtk_look: Option<crate::gtk::GtkLook>,
    /// The text selected in the focused app, for Services.
    pub selection: Option<String>,
    pub panel: Option<crate::systray::glue::OpenPanel>,
    pub panel_ui: crate::systray::glue::PanelUi,
    /// Panels to show instead of the system's (tests and pictures).
    pub panels_fixture: Option<crate::systray::glue::Fixture>,
    pub osd: Option<crate::osd::OsdState>,
    pub desktop: crate::desktop::DesktopState,
    /// Each output's wallpaper, scaled to it (see `wallpaper`).
    pub(crate) wallpapers: HashMap<crate::wallpaper::WallKey, aizome_kit::Image>,
    /// Wallpapers being scaled, and ones that didn't read.
    pub(crate) wall_pending: std::collections::HashSet<crate::wallpaper::WallKey>,
    pub(crate) wall_failed: std::collections::HashSet<PathBuf>,
    /// The wallpapers' files, Night's and Day's.
    pub(crate) wall_paths: [Option<PathBuf>; 2],
    pub trash_full: bool,
    /// The user: login and full name.
    pub user: (String, String),
    pub(crate) drag_grab: aizome_kit::Vec2,
    /// Volumes to show when the system has none (tests and pictures).
    pub volumes_fixture: Vec<aizome_sys::disks::Volume>,
    /// The system's disk read from the mounts, for when UDisks isn't
    /// there to say.
    pub system_disk_fallback: Option<aizome_sys::disks::Volume>,
    pub lock: crate::lock::LockState,
    pub authenticator: Arc<dyn crate::pam::Authenticator>,
    pub user_picture: Option<aizome_kit::Image>,
    pub auth: Option<crate::auth::AuthState>,
    polkit: Option<Arc<aizome_sys::polkit::PolkitAgent>>,
    pub tearoffs: Vec<crate::tearoff::TearOff>,
    pub(crate) tear_shown: HashMap<usize, crate::tearoff::Shown>,
    /// Recently used files, read when the Toolchest opens.
    pub recent: Vec<PathBuf>,
    /// A menu at the pointer waiting for the selected text: its serial and place.
    pub(crate) services_wait: Option<(u64, f64, f64)>,
    pub(crate) serial: u64,
    pub confirm: Option<crate::confirm::Confirm>,
    /// The session verb last carried out (tests: nothing happens).
    pub session_done: Option<SessionVerb>,
    /// The critical notifications' cards, how many, and their margin at the
    /// right.
    pub cards: Option<(WindowKey, usize, f64)>,
    /// What the shell's own notifications' actions do.
    pub note_actions: HashMap<(u32, String), Action>,
    /// Actions to carry out once the update in hand is done.
    pub(crate) pending_actions: Vec<Action>,
    pub(crate) deskbar_size: Option<aizome_kit::Size>,
    pub(crate) deskbar_drag: Option<(String, Point)>,
    pub(crate) deskbar_visibility: crate::deskbar_glue::Visibility,
    pub(crate) deskbar_transient: bool,
    /// The output the Deskbar was opened on: it opens again on the main
    /// display when that changes.
    pub(crate) deskbar_output: Option<u32>,
    /// The edge the open Deskbar was anchored to.
    pub(crate) deskbar_side: aizome_config::DeskbarSide,
    /// An edge drag under way: the width at the press, and the pointer's x
    /// then.
    pub(crate) deskbar_resize: Option<(f64, f64)>,
    /// The surface showing the width that drag would give, and the width.
    pub(crate) deskbar_preview: Option<(WindowKey, f64)>,
    /// A drag of the grip under way: the surface showing where the Deskbar
    /// would land, and whether letting go now moves it there.
    pub(crate) deskbar_landing: Option<(WindowKey, bool)>,
    /// Apps' own icons (PNG), by app id, for apps the shell has no glyph for.
    pub(crate) icons: HashMap<String, Option<aizome_kit::Image>>,
    /// Apps' status items' icons, read once per change.
    pub(crate) item_icons: crate::systray::items::IconCache,
    /// What's left of the wheel over each status item, short of a step.
    pub(crate) item_wheel: HashMap<u64, aizome_kit::layout::Vec2>,
    /// The processes playing sound, as last told to the compositor.
    pub(crate) audible_sent: Vec<u32>,
    pub(crate) icon_lookup: Option<aizome_sys::apps::IconLookup>,
    /// The primary bar's Desks, by where they are across it.
    pub(crate) desk_strip: Vec<(u32, Rect)>,
    /// The bar menu open (its title lit).
    pub bar_menu: Option<crate::barmenus::BarMenu>,
    pub(crate) menu_gen: u64,
    /// The bar menu that closed last, and when (a press on its title closed it).
    pub(crate) just_closed: Option<(WindowKey, Slot, std::time::Instant)>,
    /// Koyomi's next events and Tokei's world clocks, for the clock's panel.
    pub agenda: crate::agenda::Agenda,
    /// The last sound made (or that would have been, in tests).
    pub last_sound: Option<String>,
    /// The bell's flash showing, and its generation.
    pub flash: Option<(WindowKey, u64)>,
    /// logind's `handle-power-key`, held while `[power] power_key` is set.
    pub(crate) power_inhibit: Option<aizome_sys::Task<aizome_sys::power::Inhibition>>,
    /// logind's own `HandlePowerKey`.
    pub logind_power_key: Option<String>,
    /// Night Light, as the compositor last told.
    pub night: Option<aizome_ipc::NightLightState>,
    /// Keep Awake, as the watcher last saw it (`awake.rs`).
    pub awake: aizome_awake::Status,
    /// GeoClue is being asked where the computer is; when it last answered.
    pub(crate) locating: bool,
    pub(crate) located_at: Option<std::time::Instant>,
    /// The screen recording going, if any (one at a time).
    pub recording: Option<crate::recording::Live>,
    pub(crate) record_gen: u64,
    /// Which recorders can record here (probed once, as the shell starts).
    pub(crate) recorders: crate::recording::probe::Probes,
    /// What's shared (screen and window sharing), as the compositor says.
    pub(crate) shares: Vec<crate::sharing::Share>,
    pub text: aizome_kit::text::TextCache,
    pub(crate) launches: HashMap<ActivationRequest, (Launch, Option<PathBuf>)>,
    pub(crate) startup: crate::startup::Startup,
    stream: Option<MsgSender<Msg>>,
    _settings_watch: Option<aizome_config::Watcher>,
    _apps_watch: Option<aizome_sys::apps::Watcher>,
    _timedate: Option<aizome_sys::datetime::TimeDate>,
    greeter_scales: Option<std::sync::mpsc::Sender<aizome_config::greeter::DisplayScales>>,
}

impl Shell {
    pub fn new(opts: Options) -> Shell {
        let settings = if opts.live { Settings::load_or_default() } else { Settings::default() };
        let mut s = Shell {
            opts,
            bar_cfg: MenubarSettings::default(),
            deskbar_cfg: DeskbarSettings::default(),
            notes_cfg: NotificationSettings::default(),
            lock_cfg: LockSettings::default(),
            palette_cfg: PaletteSettings::default(),
            desktop_cfg: Default::default(),
            power_cfg: Default::default(),
            sound_cfg: Default::default(),
            settings,
            wm: Wm::default(),
            ipc: Ipc::none(),
            server: None,
            registry: Registry::default(),
            apps: None,
            sys: crate::sys::Sys::default(),
            tray: Tray { cpu: true, memory: true, ..Tray::default() },
            screens: Vec::new(),
            surfaces: HashMap::new(),
            deskbar: None,
            meters: Reading::default(),
            now: Local::now(),
            renaming: None,
            shelf: crate::shelf::Shelf::default(),
            folded: HashMap::new(),
            notes: crate::notifications::store::Store::default(),
            carried: None,
            carry_ready: false,
            retiring: false,
            notes_open: Vec::new(),
            note_server: None,
            banner: None,
            banner_queue: Vec::new(),
            palette: None,
            palette_ahead: Default::default(),
            frecency: aizome_search::frecency::Frecency::default(),
            file_index: None,
            settings_list: Vec::new(),
            theme_day: false,
            gtk_look: None,
            selection: None,
            panel: None,
            panel_ui: Default::default(),
            panels_fixture: None,
            osd: None,
            desktop: Default::default(),
            wallpapers: HashMap::new(),
            wall_pending: Default::default(),
            wall_failed: Default::default(),
            wall_paths: [None, None],
            trash_full: false,
            user: crate::pam::current_user(),
            drag_grab: aizome_kit::Vec2::ZERO,
            volumes_fixture: Vec::new(),
            system_disk_fallback: None,
            lock: Default::default(),
            authenticator: Arc::new(crate::pam::Pam::new("aizome")),
            user_picture: None,
            auth: None,
            polkit: None,
            tearoffs: Vec::new(),
            tear_shown: HashMap::new(),
            recent: Vec::new(),
            services_wait: None,
            serial: 0,
            confirm: None,
            session_done: None,
            cards: None,
            note_actions: HashMap::new(),
            pending_actions: Vec::new(),
            deskbar_size: None,
            deskbar_drag: None,
            deskbar_visibility: Default::default(),
            deskbar_transient: false,
            deskbar_output: None,
            deskbar_side: aizome_config::DeskbarSide::Right,
            deskbar_resize: None,
            deskbar_preview: None,
            deskbar_landing: None,
            icons: HashMap::new(),
            item_icons: HashMap::new(),
            item_wheel: HashMap::new(),
            audible_sent: Vec::new(),
            icon_lookup: None,
            desk_strip: Vec::new(),
            bar_menu: None,
            menu_gen: 0,
            just_closed: None,
            agenda: Default::default(),
            last_sound: None,
            flash: None,
            power_inhibit: None,
            logind_power_key: None,
            night: None,
            awake: aizome_awake::Status::Off,
            locating: false,
            located_at: None,
            recording: None,
            record_gen: 0,
            recorders: Default::default(),
            shares: Vec::new(),
            text: aizome_kit::text::TextCache::default(),
            launches: HashMap::new(),
            startup: Default::default(),
            stream: None,
            _settings_watch: None,
            _apps_watch: None,
            _timedate: None,
            greeter_scales: None,
        };
        s.read_settings();
        s.theme_day = s.settings.theme_now() == aizome_theme::ThemeName::Day;
        s
    }

    fn read_settings(&mut self) {
        self.bar_cfg = self.settings.menubar_settings();
        let deskbar = self.settings.deskbar_settings();
        if deskbar.auto_hide != self.deskbar_cfg.auto_hide || deskbar.collapsed != self.deskbar_cfg.collapsed {
            self.deskbar_visibility = crate::deskbar_glue::Visibility {
                generation: self.deskbar_visibility.generation.wrapping_add(1),
                ..Default::default()
            };
        }
        self.deskbar_cfg = deskbar;
        self.notes_cfg = self.settings.notification_settings();
        self.lock_cfg = self.settings.lock_settings();
        self.palette_cfg = self.settings.palette_settings();
        self.desktop_cfg = self.settings.desktop_settings();
        self.power_cfg = self.settings.power_settings();
        self.sound_cfg = self.settings.sound_settings();
    }

    /// Send `msg` to the program from a thread.
    pub fn sender(&self) -> Option<MsgSender<Msg>> {
        self.stream.clone()
    }

    // Outputs and surfaces.

    /// The output the Deskbar and banners show on: the main display, as the
    /// compositor says (`primary = true` in the settings, or the session's
    /// choice); before it has said, the one at the layout's origin, else the
    /// first.
    pub fn primary(&self) -> Option<&Screen> {
        let main = self.wm.outputs.iter().find(|o| o.primary && o.enabled).map(|o| o.name.as_str());
        main.and_then(|n| self.screen_named(n))
            .or_else(|| self.screens.iter().find(|s| s.output.frame.x0 == 0.0 && s.output.frame.y0 == 0.0))
            .or(self.screens.first())
    }

    /// The output with the focus: the focused window's, else the one
    /// showing the current Desk (each display shows a Desk of its own, and
    /// the focus is on one of them), else the primary.
    pub fn focused_screen(&self) -> Option<&Screen> {
        let name = self.wm.focused_window().and_then(|w| w.output.clone());
        name.and_then(|n| self.screen_named(&n))
            .or_else(|| {
                self.wm.current_desk_info().and_then(|d| d.output.as_deref()).and_then(|n| self.screen_named(n))
            })
            .or(self.primary())
    }

    pub fn screen_named(&self, name: &str) -> Option<&Screen> {
        self.screens.iter().find(|s| s.name() == name)
    }

    /// The screen a window of the shell is on.
    pub fn screen_of(&self, w: WindowKey) -> Option<&Screen> {
        match self.surfaces.get(&w)? {
            Surface::MenuBar { output } | Surface::Desktop { output } => {
                self.screens.iter().find(|s| s.output.id == *output)
            }
            _ => self.primary(),
        }
    }

    /// The compositor's description of an output (its place in the layout).
    pub fn output_info(&self, screen: &Screen) -> Option<&OutputInfo> {
        self.wm.outputs.iter().find(|o| o.name == screen.name())
    }

    /// Open or close each output's bar and desktop to match the outputs.
    pub fn sync_screens(&mut self, outputs: Vec<Output>, cx: &mut Cx<Msg>) {
        // Outputs gone: their surfaces go.
        let gone: Vec<Screen> =
            self.screens.iter().filter(|s| !outputs.iter().any(|o| o.id == s.output.id)).cloned().collect();
        for s in gone {
            for k in [s.bar, s.desktop].into_iter().flatten() {
                cx.close_window(k);
                self.surfaces.remove(&k);
            }
        }
        self.screens.retain(|s| outputs.iter().any(|o| o.id == s.output.id));
        for o in outputs {
            match self.screens.iter_mut().find(|s| s.output.id == o.id) {
                Some(s) => s.output = o,
                None => self.screens.push(Screen { output: o, bar: None, desktop: None }),
            }
        }
        self.sync_lock_surfaces(cx);
        // The Deskbar first: surfaces of one layer stack in the order they
        // open, and the bar's menus and panels show over what's under it.
        let had_deskbar = self.deskbar.is_some();
        self.sync_deskbar(cx);
        // Window memory for the surfaces opened here, faulted in while the
        // compositor configures them: their first frames (the desktop's
        // fills the output) then draw into memory that's there.
        let mut fresh = match (had_deskbar, self.deskbar_size, self.primary()) {
            (false, Some(size), Some(s)) => surface_bytes(size, s.output.scale),
            _ => 0,
        };
        for i in 0..self.screens.len() {
            let id = self.screens[i].output.id;
            let (frame, scale) = (self.screens[i].output.frame.size(), self.screens[i].output.scale);
            if self.screens[i].bar.is_none() {
                let opts = LayerOptions::new(Layer::Top, "aizome-menubar")
                    .anchors(Anchors::TOP | Anchors::LEFT | Anchors::RIGHT)
                    .exclusive_zone(crate::menubar::HEIGHT as i32)
                    .keyboard(KeyboardInteractivity::OnDemand)
                    .output(id);
                let k = cx.open_window(layer(opts, 0.0, crate::menubar::HEIGHT, "Menu bar"));
                self.surfaces.insert(k, Surface::MenuBar { output: id });
                self.screens[i].bar = Some(k);
                fresh += surface_bytes(aizome_kit::Size::new(frame.width, crate::menubar::HEIGHT), scale);
            }
            if self.screens[i].desktop.is_none() {
                let opts = LayerOptions::new(Layer::Background, "aizome-desktop")
                    .anchors(Anchors::ALL)
                    .exclusive_zone(-1)
                    .keyboard(KeyboardInteractivity::OnDemand)
                    .output(id);
                let k = cx.open_window(layer(opts, 0.0, 0.0, "Desktop"));
                self.surfaces.insert(k, Surface::Desktop { output: id });
                self.screens[i].desktop = Some(k);
                fresh += surface_bytes(frame, scale);
            }
        }
        cx.reserve_buffers(fresh);
    }

    // The bar.

    /// The focused app's name and menus (published, or the fallback
    /// ones), or Aizome's when no window has focus.
    pub fn focused_menus(&self) -> (String, Vec<ShellMenu>) {
        let Some(w) = self.wm.focused_window() else {
            return ("Aizome".into(), build::desktop(&self.wm.desks));
        };
        let conn = self.registry.for_window(w.pid, &w.app_id);
        let name = crate::apps::catalog::display_name(self.apps.as_deref(), conn.map(|c| c.name.as_str()), &w.app_id);
        match conn {
            Some(c) if !c.menus.is_empty() => (name, build::app_menus(&c.menus, c.pid)),
            _ => (name.clone(), build::fallback(&name, Some(w), &self.wm.desks)),
        }
    }

    pub fn bar_model(&self, screen: &Screen) -> BarModel {
        let (app_name, menus) = self.focused_menus();
        let primary = self.primary().is_some_and(|p| p.output.id == screen.output.id);
        let folded = (self.deskbar_cfg.collapsed || self.deskbar_cfg.auto_hide)
            .then(|| self.wm.deskbar(&self.deskbar_cfg.pinned).iter().filter(|a| !a.windows.is_empty()).count());
        BarModel {
            app_name,
            menus: menus.iter().map(|m| m.title.clone()).collect(),
            open: self
                .bar_menu
                .filter(|m| Some(m.bar) == screen.bar)
                .map(|m| m.slot)
                .or_else(|| self.panel.as_ref().filter(|p| Some(p.bar) == screen.bar).map(|p| Slot::Tray(p.widget))),
            desks: self.wm.desks.clone(),
            renaming: self.renaming.clone(),
            tray: self.tray.clone(),
            meters: self.meters.clone(),
            clock: self.now.bar_label(self.bar_cfg.clock, self.bar_cfg.clock_seconds, self.bar_cfg.clock_date),
            folded_apps: folded,
            deskbar_visible: self.deskbar_visible(),
            deskbar_auto_hide: self.deskbar_cfg.auto_hide,
            unread: self.notes.unread_count(),
            banner: if primary { self.banner_view() } else { None },
            primary,
            output: screen.name(),
            recording: if primary { self.record_badge() } else { None },
            sharing: if primary { self.share_badge() } else { None },
            header: self.header_width(),
            deskbar_left: self.deskbar_cfg.side == aizome_config::DeskbarSide::Left,
        }
    }

    fn bar(&mut self, win: WindowKey, msg: BarMsg, cx: &mut Cx<Msg>) {
        match msg {
            BarMsg::DeskbarHover(inside) => self.deskbar_hover(true, inside, cx),
            BarMsg::Run(a) => self.run(a, cx),
            BarMsg::Press(slot, rect) => {
                // Pressing the open menu's title closes it (the press closed
                // the menus before it got here).
                if !self.pressed_to_close(win, slot) {
                    self.open_slot(win, slot, rect, false, cx);
                }
            }
            BarMsg::Item(slot, click, rect) => self.item_click(win, slot, click, rect, cx),
            BarMsg::ItemScroll(slot, delta) => self.item_scroll(slot, delta),
            BarMsg::Hover(slot) => self.hover_title(win, slot, cx),
            BarMsg::RenameText(t) => {
                if let Some((_, s)) = &mut self.renaming {
                    *s = t;
                }
            }
            BarMsg::RenameDone => {
                if let Some((index, name)) = self.renaming.take()
                    && !name.trim().is_empty()
                {
                    self.ipc.send(Request::RenameDesk {
                        desk: aizome_ipc::DeskSel::Index(index),
                        name: name.trim().to_owned(),
                    });
                }
            }
            BarMsg::RenameCancel => self.renaming = None,
            BarMsg::Banner(id) => {
                self.open_note(id, cx);
                self.banner_done(id, cx);
            }
            BarMsg::DeskMenu(index, at) => {
                let Some(desk) = self.wm.desk(index).cloned() else { return };
                let menu = build::desk_context(&desk, self.wm.desks.len() > 1);
                self.show_menu(win, Rect::from_origin_size(at, (1.0, 1.0)), &menu, MenuStyle::at_point(), cx);
            }
        }
    }

    /// Open what a slot of the bar opens (from the keyboard: the menu's
    /// first row lit).
    pub(crate) fn open_slot(&mut self, win: WindowKey, slot: Slot, rect: Rect, keyboard: bool, cx: &mut Cx<Msg>) {
        match slot {
            Slot::Toolchest => {
                if self.opts.live {
                    self.recent = aizome_search::recent::load(aizome_search::recent::default_path())
                        .into_iter()
                        .take(12)
                        .map(|r| r.path)
                        .collect();
                }
                let menu = self.toolchest_menu();
                let anchor = Rect::new(rect.x0, rect.y0, rect.x1, crate::menubar::HEIGHT - 1.0);
                let output = self.screen_of(win).map(|s| s.name()).unwrap_or_default();
                let tear = Msg::TearOff {
                    app: crate::tearoff::TOOLCHEST.into(),
                    path: vec![],
                    output: output.clone(),
                    at: Point::new(rect.x0 + 12.0, crate::menubar::HEIGHT + 12.0),
                };
                let subs = crate::tearoff::tear_sub(
                    crate::tearoff::TOOLCHEST.into(),
                    menu.clone(),
                    vec![],
                    None,
                    output,
                    Point::new(rect.x0, crate::menubar::HEIGHT),
                );
                let style = MenuStyle::below()
                    .titled("Toolchest")
                    .mark()
                    .min_width(rect.width())
                    .on_tear_off(tear)
                    .on_tear_off_sub(subs)
                    .on_type_until_closed(Msg::ToolchestTyped);
                let style = self.as_bar_menu(style, win, slot, keyboard);
                self.show_menu(win, anchor, &menu, style, cx);
            }
            Slot::Menu(i) => {
                let (_, menus) = self.focused_menus();
                let Some(menu) = menus.get(i) else { return };
                if let (Some(pid), Some(id)) = (menu.pid, menu.proto_id) {
                    self.about_to_show(pid, id);
                }
                let anchor = Rect::new(rect.x0, rect.y0, rect.x1, crate::menubar::HEIGHT - 1.0);
                // Its submenus tear off into the app's palettes.
                let app =
                    self.wm.focused_window().map_or_else(|| crate::tearoff::DESKTOP.to_owned(), |w| w.app_id.clone());
                let output = self.screen_of(win).map(|s| s.name()).unwrap_or_default();
                let subs = crate::tearoff::tear_sub(
                    app,
                    menu.clone(),
                    vec![menu.title.clone()],
                    None,
                    output,
                    Point::new(rect.x0, crate::menubar::HEIGHT),
                );
                let style = self.as_bar_menu(MenuStyle::below().on_tear_off_sub(subs), win, slot, keyboard);
                self.show_menu(win, anchor, menu, style, cx);
            }
            Slot::Tray(w) => {
                self.toggle_panel(win, w, rect, cx);
            }
            Slot::Item(s) => self.item_menu(win, s, rect, cx),
            Slot::Apps => self.run(Action::SetDeskbarCollapsed(false), cx),
            Slot::Sharing => self.open_share_menu(win, rect, cx),
        }
    }

    /// Show one of the shell's menus over `win`.
    pub fn show_menu(
        &mut self,
        win: WindowKey,
        anchor: Rect,
        menu: &ShellMenu,
        style: MenuStyle<Msg>,
        cx: &mut Cx<Msg>,
    ) {
        let kit = build::to_kit(menu, &Msg::Run);
        cx.show_menu(win, anchor, kit, style);
    }

    pub fn toolchest_menu(&self) -> ShellMenu {
        let apps: Vec<&aizome_sys::apps::App> = self.apps.as_deref().map(|i| i.apps()).unwrap_or_default();
        let desk_folder = self.wm.current_desk_info().and_then(|d| d.folder.as_deref()).map(aizome_config::expand_home);
        let recent = self.recent.clone();
        let disks = self
            .sys
            .disks
            .as_ref()
            .map(|d| d.snapshot().volumes())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| Some((v.name, PathBuf::from(v.mount_point?))))
            .collect();
        build::toolchest(&build::ToolchestInput { apps: &apps, desks: &self.wm.desks, desk_folder, disks, recent })
    }

    /// Tell an app one of its menus is about to show.
    fn about_to_show(&self, pid: u32, menu: u32) {
        if let (Some(server), Some(conn)) = (&self.server, self.registry.by_pid(pid)) {
            server.send(conn.conn, &proto::ShellMessage::AboutToShow { menu });
        }
    }

    // Doing things.

    /// Carry out an action.
    pub fn run(&mut self, action: Action, cx: &mut Cx<Msg>) {
        match action {
            Action::None => {}
            Action::AppItem { pid, id } => match self.registry.by_pid(pid) {
                Some(c) => {
                    if let Some(s) = &self.server {
                        s.send(c.conn, &proto::ShellMessage::Activate { id });
                    }
                }
                None => log::debug!("item {id} of {pid}, which is gone"),
            },
            Action::Wm(req) => self.ipc.send(req),
            Action::QuitApp { app_id } => {
                for w in self.wm.app_windows_by_recency(&app_id, self.apps.as_deref()) {
                    self.ipc.send(Request::Close { id: Some(w.id) });
                }
            }
            Action::HideApp { app_id } => {
                let ids: std::collections::BTreeSet<_> = self
                    .wm
                    .app_windows_by_recency(&app_id, self.apps.as_deref())
                    .into_iter()
                    .map(|w| w.app_id.as_str())
                    .collect();
                for id in ids {
                    self.ipc.send(Request::HideApp { app: app_sel(id) });
                }
            }
            Action::Launch { app_id } | Action::FocusApp { app_id } => {
                match self.wm.app_window_to_activate(&app_id, self.apps.as_deref()) {
                    Some(w) => self.ipc.send(Request::Focus { id: w.id }),
                    None => self.launch(Launch::App { app_id, targets: vec![] }, cx),
                }
            }
            Action::NewWindow { app_id } => self.launch(Launch::App { app_id, targets: vec![] }, cx),
            Action::Spawn { argv } => self.launch(Launch::Argv(argv), cx),
            Action::LaunchDetails { id } => self.show_startup(id, cx),
            Action::Open { path } => {
                let l = crate::launch::open_path(self.apps.as_deref(), &path);
                self.launch(l, cx);
            }
            Action::Reveal { path } => {
                let l = crate::launch::reveal_path(self.apps.as_deref(), &path);
                self.launch(l, cx);
            }
            Action::OpenUrl { url } => self.launch(Launch::Argv(vec!["xdg-open".into(), url]), cx),
            Action::Settings { pane, anchor } => {
                // The shell owns the user's activation. A second launch
                // forwards the pane, but the running instance has no fresh
                // input serial with which to raise itself across Desks.
                self.focus_app_window(crate::launch::SETTEI);
                self.launch(crate::launch::settings(&pane, anchor.as_deref()), cx);
            }
            Action::SetUpKeyboard { name, id } => {
                self.focus_app_window(crate::launch::SETTEI);
                self.launch(crate::launch::keyboard_setup(&name, &id), cx);
            }
            Action::Copy { text } => cx.set_clipboard_text(&text),
            Action::StatusMenuItem { key, id } => self.status_menu_item(&key, id),
            Action::NewDesk => {
                let name = (self.wm.desks.len() + 1).to_string();
                self.ipc.send(Request::AddDesk { name, folder: None, index: None });
            }
            Action::RenameDesk { index } => {
                let name = self.wm.desk(index).map(|d| d.name.clone()).unwrap_or_default();
                self.renaming = Some((index, name));
                cx.focus("desk-rename");
            }
            Action::Session(SessionVerb::Sleep) => self.do_session(SessionVerb::Sleep),
            Action::Session(v @ (SessionVerb::LogOut | SessionVerb::Restart | SessionVerb::ShutDown)) => {
                self.confirm(v, cx)
            }
            Action::Session(SessionVerb::Lock) => self.lock(cx),
            Action::Pin { app_id, on } => {
                self.deskbar_cfg.pinned.retain(|a| *a != app_id);
                if on {
                    self.deskbar_cfg.pinned.push(app_id);
                }
                self.save_section("deskbar");
            }
            Action::SetDeskbarCollapsed(on) => {
                self.deskbar_cfg.collapsed = on;
                self.deskbar_visibility.revealed = !on;
                self.deskbar_visibility.generation = self.deskbar_visibility.generation.wrapping_add(1);
                self.save_section("deskbar");
            }
            Action::MoveDeskbarApp { app_id, down } => self.move_deskbar_app(&app_id, down),
            Action::PlaceDeskbar { side, width } => {
                if let Some(s) = side {
                    self.deskbar_cfg.side = s;
                }
                if let Some(w) = width {
                    self.deskbar_cfg.width = crate::deskbar::clamp_width(w);
                }
                self.save_section("deskbar");
            }
            Action::EmptyTrash => crate::launch::empty_trash(),
            Action::MoveToTrash { path } => {
                if let Err(e) = crate::launch::move_to_trash(&path) {
                    log::warn!("{}: {e}", path.display());
                }
            }
            Action::ShowDesktop => {
                for w in self.wm.windows.values().filter(|w| !w.minimised && w.desk == Some(self.wm.current_desk)) {
                    self.ipc.send(Request::Minimise { id: Some(w.id) });
                }
            }
            Action::Screenshot => self.ipc.send(Request::Screenshot {
                output: None,
                path: None,
                window: None,
                area: None,
                clipboard: false,
            }),
            Action::Record(ask) => self.record(ask, cx),
            Action::StopRecording => self.stop_recording(cx),
            Action::About { app_id: None } => self.run(Action::Settings { pane: "about".into(), anchor: None }, cx),
            Action::About { app_id: Some(app_id) } => self.about_app(&app_id, cx),
            Action::Palette { scope, query } => self.open_palette(scope, query, cx),
            Action::Toolchest => self.open_toolchest(None, cx),
            Action::SetTheme { day } => self.set_theme(day),
            Action::ShowInMenu { trail } => self.show_in_menu(&trail, cx),
            Action::Disk { device, verb } => self.disk(device, verb, cx),
            Action::ToggleDoNotDisturb => {
                self.notes_cfg.do_not_disturb = !self.notes_cfg.do_not_disturb;
                self.save_section("notifications");
            }
            Action::ToggleKeepAwake => self.set_keep_awake(self.awake == aizome_awake::Status::Off, cx),
            // The compositor does it (⌃❖R's way): the logic, this shell and
            // the settings, whatever changed.
            Action::ReloadAizome => self.ipc.send(Request::Action { action: "reload".into() }),
            Action::ShelfRemove { index } => {
                self.shelf.remove(index);
                self.save_shelf();
            }
            Action::Service(verb) => self.run_service(verb, cx),
            Action::NewFolderOnDesktop => {
                let dir = crate::launch::desktop_dir();
                let mut path = dir.join("New Folder");
                let mut n = 2;
                while path.exists() {
                    path = dir.join(format!("New Folder {n}"));
                    n += 1;
                }
                if let Err(e) = std::fs::create_dir_all(&path) {
                    log::warn!("{}: {e}", path.display());
                }
            }
        }
    }

    /// Use the same explicit identities and Desk preference for every
    /// activation path, including notifications and requested Settings panes.
    pub(crate) fn focus_app_window(&self, app_id: &str) -> bool {
        let window = self.wm.app_window_to_activate(app_id, self.apps.as_deref());
        if let Some(w) = window {
            self.ipc.send(Request::Focus { id: w.id });
            true
        } else {
            false
        }
    }

    /// Start something, asking first for an activation token so its window
    /// comes up with the keyboard.
    pub fn launch(&mut self, l: Launch, cx: &mut Cx<Msg>) {
        let cwd = self.wm.current_desk_info().and_then(|d| d.folder.as_deref()).map(aizome_config::expand_home);
        if !self.opts.live {
            log::info!("launch {l:?}");
            return;
        }
        let req = cx.request_activation_token(l.app_id());
        self.launches.insert(req, (l, cwd));
        // An unavailable activation-token protocol must not swallow a launch.
        cx.after(Duration::from_secs(5), Msg::ActivationExpired(req));
    }

    pub(crate) fn save_section(&mut self, section: &str) {
        match section {
            "deskbar" => self.settings.set_shell_section("deskbar", &self.deskbar_cfg),
            "menubar" => self.settings.set_shell_section("menubar", &self.bar_cfg),
            "notifications" => self.settings.set_shell_section("notifications", &self.notes_cfg),
            _ => {}
        }
        if self.opts.live
            && let Err(e) = self.settings.save()
        {
            log::warn!("saving the settings: {e}");
        }
    }

    // Threads.

    fn start_threads(&mut self, cx: &mut Cx<Msg>) {
        let tx = cx.stream();
        self.stream = Some(tx.clone());
        self.frecency = aizome_search::frecency::Frecency::load(aizome_search::frecency::Frecency::default_path());
        self.shelf = crate::shelf::Shelf::load(&crate::shelf::Shelf::default_dir());
        self.desktop.load_positions(&crate::desktop::DesktopState::path());
        self.system_disk_fallback = std::fs::read_to_string("/proc/self/mountinfo").ok().and_then(|m| {
            let home = aizome_config::home_dir();
            aizome_sys::disks::system_volume_from_mounts(
                &m,
                &home.to_string_lossy(),
                std::path::Path::new("/sys/class/block"),
            )
        });
        self.tearoffs = crate::tearoff::load(&crate::tearoff::store_path());
        self.recent = aizome_search::recent::load(aizome_search::recent::default_path())
            .into_iter()
            .take(12)
            .map(|r| r.path)
            .collect();
        self.refresh_wall_paths();
        self.start_palette_threads(cx);
        {
            let tx = tx.clone();
            self.ipc = Ipc::spawn(self.opts.ipc_socket.clone(), move |e| {
                tx.send(Msg::Ipc(e));
            });
        }
        // What the shell before this one held (a restart, or a crash).
        let previous = crate::carry::previous_shell();
        if let Some(c) = crate::carry::load() {
            log::info!("taking up {} notifications from the shell before (pid {})", c.notes.len(), c.pid);
            // In a restart, ids go on well past the last the old shell gave,
            // as it may give a few more meanwhile.
            let skip = if previous.is_some() { 1000 } else { 0 };
            let c = c.for_restore();
            self.notes.restore(c.notes, c.next_id + skip);
            self.folded = c.folded.into_iter().collect();
            self.notes_open = c.notes_open;
        }
        // Saving waits, in a restart, for the old shell's last word.
        self.carry_ready = previous.is_none();
        {
            let tx = tx.clone();
            let path = self.opts.shell_socket.clone().unwrap_or_else(crate::apps::server::socket_path);
            // A restart takes the socket over from the shell still running:
            // apps reconnect to this one as that one goes.
            let listen = if previous.is_some() { Server::listen_replacing } else { Server::listen };
            match listen(
                &path,
                Box::new(move |e| {
                    tx.send(Msg::Server(e));
                }),
            ) {
                Ok(s) => self.server = Some(s),
                Err(e) => log::warn!("the shell socket {}: {e}", path.display()),
            }
        }
        {
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("apps".into())
                .spawn(move || {
                    let index = AppIndex::load();
                    tx.send(Msg::Apps(AppsLoaded(Arc::new(index))));
                })
                .ok();
        }
        {
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("meters".into())
                .spawn(move || {
                    let mut s = crate::systray::meters::Sampler::new();
                    // A first reading soon, then every two seconds.
                    std::thread::sleep(Duration::from_millis(250));
                    // Only what changes the bars wakes the shell: an idle machine
                    // doesn't redraw.
                    let mut last = None;
                    loop {
                        let r = s.read();
                        let shown = (r.bar_heights(), r.hot(), r.memory_height());
                        if last.as_ref() != Some(&shown) {
                            last = Some(shown);
                            if !tx.send(Msg::Meters(r)) {
                                return;
                            }
                        }
                        std::thread::sleep(Duration::from_secs(2));
                    }
                })
                .ok();
        }
        // Keep Awake belongs to the real session; a headless or nested
        // shell leaves it alone.
        if real_session() {
            let tx = tx.clone();
            crate::awake::watch(move |m| tx.send(m));
            let scales = crate::greeter::start();
            let _ = scales.send(aizome_config::greeter::DisplayScales::from_outputs(&self.settings.outputs));
            self.greeter_scales = Some(scales);
        }
        {
            let tx = tx.clone();
            self._settings_watch = Some(aizome_config::Watcher::spawn(move || {
                tx.send(Msg::SettingsChanged);
            }));
        }
        {
            let tx = tx.clone();
            // Ids go on from the last one given.
            let first = self.notes.next_id() + 1;
            match crate::notifications::server::NoteServer::start(first, move |e| {
                tx.send(Msg::Note(e));
            }) {
                Ok(s) => self.note_server = Some(s),
                Err(e) => log::warn!("the notifications server: {e}"),
            }
        }
        // In a restart, take in what the old shell got after this one read
        // its state: it writes its last word as it loses the bus name to
        // this one (or, without a bus, as it stops).
        if let Some(previous) = previous {
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("carry".into())
                .spawn(move || {
                    let c = crate::carry::wait_for_last(previous).unwrap_or_default();
                    tx.send(Msg::Carried(Box::new(c)));
                })
                .ok();
        }
        let tx2 = tx.clone();
        self.sys = crate::sys::Sys::start(move || {
            tx2.send(Msg::Sys);
        });
        let time_tx = tx.clone();
        self._timedate = Some(aizome_sys::datetime::TimeDate::new(aizome_sys::Wake::new(move || {
            time_tx.send(Msg::TimeChanged);
        })));
        self.watch_desktop();
        self.icon_lookup = Some(aizome_sys::apps::IconLookup::from_env());
        self.start_lock();
        self.sync_power_key();
        self.adopt_recording();
        self.recorders.start();
        self.read_logind_power_key(cx);
        self.start_polkit();
        self.follow_gtk();
    }

    fn take_ipc(&mut self, e: IpcEvent, cx: &mut Cx<Msg>) {
        match e {
            IpcEvent::Connected(s) => {
                self.wm.reset(s.0);
                // Which display is the main one: the Deskbar goes there.
                self.sync_screens(cx.outputs(), cx);
                // Also after a shell replacement: the desktop's theme may
                // already be Day, so no theme-change event will arrive.
                self.follow_foot(self.wm.windows.values().filter_map(|w| w.pid));
                // What Night Light is doing, for the Display panel.
                self.ipc.ask(Request::NightLight { enabled: None, temperature: None }, NIGHT_LIGHT_TAG);
                // What's shared already (the shell restarted mid-share).
                self.ipc.ask(Request::Shares, SHARES_TAG);
            }
            IpcEvent::Event(e) => {
                self.wm.apply(&e);
                match e {
                    Event::WindowOpened { window } => {
                        self.startup_window(&window);
                        // Foot starts with its configured default palette.
                        // Once mapped, its signal handlers are ready and it
                        // can follow the current theme without a restart.
                        self.follow_foot(window.pid);
                    }
                    Event::ConfigChanged => self.reload_settings(),
                    Event::WindowChanged { window } => self.startup_window(&window),
                    Event::Binding { action, pointer, output } => self.binding(&action, pointer, output, cx),
                    Event::Idle { kind } if kind == "lock" => self.lock(cx),
                    // The monitors went off for idleness: lock, unless the
                    // settings say not to.
                    Event::Idle { kind } if kind == "monitors-off" && self.lock_cfg.after_monitors_off => self.lock(cx),
                    Event::ScreenshotTaken { path } => self.screenshot_taken(PathBuf::from(path), cx),
                    Event::NightLightChanged { state } => self.night = Some(state),
                    Event::AreaPicked { tag, area, output, .. } => self.area_picked(&tag, area, output.as_deref(), cx),
                    Event::SharesChanged { shares } => self.shares_changed(shares),
                    // Another main display: the Deskbar goes there.
                    Event::OutputsChanged { .. } => self.sync_screens(cx.outputs(), cx),
                    _ => {}
                }
            }
            IpcEvent::Disconnected => self.wm.connected = false,
            IpcEvent::Reply { tag: NIGHT_LIGHT_TAG, response: aizome_ipc::Response::NightLight { state } } => {
                self.night = Some(state)
            }
            IpcEvent::Reply { tag: SHARES_TAG, response: aizome_ipc::Response::Shares { shares } } => {
                self.shares_changed(shares)
            }
            IpcEvent::Reply { .. } => {}
        }
    }

    fn binding(&mut self, action: &str, pointer: Option<(f64, f64)>, output: Option<String>, cx: &mut Cx<Msg>) {
        match action {
            // On the bar of the display with the focus, where the keys are
            // (as the palette opens there), wherever the pointer is.
            "toolchest" => self.open_toolchest(None, cx),
            "menu-bar" => {
                self.focus_menu_bar(None, cx);
            }
            "palette" => self.toggle_palette(None, String::new(), cx),
            "lock" => self.lock(cx),
            "power-key" => self.power_key(cx),
            "record" => self.toggle_recording(cx),
            "record-screen" => {
                // The display under the pointer, as a key gives it.
                let target = match output.clone() {
                    Some(o) => crate::recording::Target::Output(o),
                    None => crate::recording::Target::Screen,
                };
                self.record(crate::recording::Ask { target, audio: None }, cx)
            }
            "record-area" => {
                self.record(crate::recording::Ask { target: crate::recording::Target::Pick, audio: None }, cx)
            }
            "stop-recording" => self.stop_recording(cx),
            "screenshot-tool" => self.open_capture_menu(output.as_deref(), cx),
            "bell" => self.bell(output.as_deref(), cx),
            "app-menu" => {
                if let Some((x, y)) = pointer {
                    self.pointer_menu_soon(x, y, cx);
                }
            }
            "play-pause" => self.sys.media_control(crate::systray::panels::MediaVerb::PlayPause),
            "next" => self.sys.media_control(crate::systray::panels::MediaVerb::Next),
            "previous" => self.sys.media_control(crate::systray::panels::MediaVerb::Previous),
            a if self.media_key(a, cx) => {}
            _ => log::debug!("binding {action} at {pointer:?} on {output:?}"),
        }
    }

    /// Open the Toolchest from the bar of an output (the focused one).
    pub fn open_toolchest(&mut self, output: Option<&str>, cx: &mut Cx<Msg>) {
        let screen = output.and_then(|n| self.screen_named(n)).or(self.focused_screen());
        let Some(bar) = screen.and_then(|s| s.bar) else { return };
        // Under the Toolchest button, its first row lit (a key opened it).
        let rect = self.titles(bar, cx).first().map_or(Rect::new(0.0, 0.0, 92.0, crate::menubar::HEIGHT), |t| t.1);
        self.open_slot(bar, Slot::Toolchest, rect, true, cx);
    }

    fn reload_settings(&mut self) {
        if self.opts.live {
            let old_outputs = self.settings.outputs.clone();
            self.settings = Settings::load_or_default();
            if old_outputs != self.settings.outputs
                && let Some(tx) = &self.greeter_scales
            {
                let _ = tx.send(aizome_config::greeter::DisplayScales::from_outputs(&self.settings.outputs));
            }
        }
        self.read_settings();
        self.sync_power_key();
        self.tray = self.sys.tray(&self.bar_cfg);
        self.sync_status_items();
        self.refresh_wall_paths();
        let day = self.settings.theme_now() == aizome_theme::ThemeName::Day;
        self.theme_changed(day);
    }

    fn take_server(&mut self, e: ServerEvent, cx: &mut Cx<Msg>) {
        match e {
            ServerEvent::Message(conn, proto::AppMessage::Request { request }) => {
                let (ok, message) = self.shell_request(request, cx);
                if let Some(s) = &self.server {
                    s.send(conn, &proto::ShellMessage::Reply { ok, message });
                }
            }
            ServerEvent::Message(_, proto::AppMessage::ServicesText { serial, text }) => {
                self.selection_came(serial, text, cx)
            }
            ServerEvent::Message(conn, m) => {
                if let RegistryChange::Menus(_) | RegistryChange::App(_) = self.registry.take(conn, &m) {
                    // The palette opens with the focused app's menus.
                    self.palette_ahead.bumped += 1;
                }
            }
            ServerEvent::Gone(conn) => {
                self.registry.gone(conn);
                self.palette_ahead.bumped += 1;
            }
        }
    }

    /// A request from the compositor or `aizomectl`.
    pub fn shell_request(&mut self, r: proto::ShellRequest, cx: &mut Cx<Msg>) -> (bool, String) {
        match r {
            proto::ShellRequest::Toolchest => {
                self.open_toolchest(None, cx);
                (true, String::new())
            }
            proto::ShellRequest::Deskbar { collapsed } => {
                let on = collapsed.unwrap_or(!self.deskbar_cfg.collapsed);
                self.run(Action::SetDeskbarCollapsed(on), cx);
                (true, String::new())
            }
            proto::ShellRequest::State => (true, self.state_json()),
            proto::ShellRequest::Palette { scope, query } => {
                let scope = scope.as_deref().and_then(crate::palette::model::scope_named);
                if query.is_empty() && scope.is_none() {
                    self.toggle_palette(None, String::new(), cx);
                } else {
                    self.open_palette(scope, query, cx);
                }
                (true, String::new())
            }
            proto::ShellRequest::AppMenu { x, y } => {
                self.pointer_menu_soon(x, y, cx);
                (true, String::new())
            }
            proto::ShellRequest::Notify { app, summary, body } => {
                let n = crate::notifications::store::Incoming { app_name: app, summary, body, ..Default::default() };
                let id = self.fresh_note_id();
                self.take_note(crate::notifications::server::NoteEvent::Notify(id, Box::new(n)), cx);
                (true, id.to_string())
            }
            proto::ShellRequest::Osd { kind, value } => match crate::osd::kind_named(&kind) {
                Some(k) => {
                    let muted = kind == "mute";
                    self.show_osd(crate::osd::Osd { kind: k, value: value as f32, muted }, cx);
                    (true, String::new())
                }
                None => (false, format!("no OSD for {kind}")),
            },
            proto::ShellRequest::Lock => {
                self.lock(cx);
                (true, String::new())
            }
            proto::ShellRequest::Open { what } => {
                if what == "launches" {
                    self.show_startup(None, cx);
                    return (true, String::new());
                }
                let Some(screen) = self.primary().cloned() else { return (false, "no output".into()) };
                let Some(bar) = screen.bar else { return (false, "no bar".into()) };
                if let Some(w) = TrayWidget::named(&what) {
                    // Under its widget, as a press would put it; a widget
                    // the bar leaves out has its panel under the clock.
                    let rect = cx
                        .element_rect(bar, &crate::menubar::tray_id(w))
                        .or_else(|| cx.element_rect(bar, &crate::menubar::tray_id(TrayWidget::Clock)))
                        .unwrap_or_else(|| {
                            let x = screen.output.frame.width() - 60.0;
                            Rect::new(x, 0.0, x + 16.0, crate::menubar::HEIGHT)
                        });
                    self.toggle_panel(bar, w, rect, cx);
                    (true, String::new())
                } else if what.eq_ignore_ascii_case("toolchest") {
                    self.open_toolchest(None, cx);
                    (true, String::new())
                } else {
                    let (_, menus) = self.focused_menus();
                    match menus.iter().position(|m| m.title.eq_ignore_ascii_case(&what)) {
                        Some(_) => {
                            self.show_in_menu(&what, cx);
                            (true, String::new())
                        }
                        None => (false, format!("nothing called {what}")),
                    }
                }
            }
            proto::ShellRequest::Dismiss => {
                self.close_palette(cx);
                self.close_panel(cx);
                for s in self.screens.clone() {
                    if let Some(b) = s.bar {
                        cx.close_menus(b);
                    }
                }
                (true, String::new())
            }
            proto::ShellRequest::Bell { output } => {
                self.bell(output.as_deref(), cx);
                (true, String::new())
            }
            proto::ShellRequest::Record { target, desktop_audio, microphone } => {
                if self.recording.is_some() {
                    return (false, "a recording is already going".into());
                }
                let target = match target.as_deref() {
                    None | Some("screen") => crate::recording::Target::Screen,
                    Some("area") => crate::recording::Target::Pick,
                    Some(output) => crate::recording::Target::Output(output.to_owned()),
                };
                let audio = match (desktop_audio, microphone) {
                    (None, None) => None,
                    (d, m) => {
                        let s = self.settings.recording_settings();
                        Some(crate::recording::Audio {
                            desktop: d.unwrap_or(s.desktop_audio),
                            microphone: m.unwrap_or(s.microphone),
                        })
                    }
                };
                self.run(Action::Record(crate::recording::Ask { target, audio }), cx);
                (true, String::new())
            }
            proto::ShellRequest::StopRecording => {
                let going = self.recording.is_some();
                self.run(Action::StopRecording, cx);
                (going, if going { String::new() } else { "no recording is going".into() })
            }
            proto::ShellRequest::MenuBar { output } => {
                let ok = self.focus_menu_bar(output.as_deref(), cx);
                (ok, if ok { String::new() } else { "no menu bar there".into() })
            }
        }
    }

    /// What shows, for `aizomectl` and tests.
    pub fn state_json(&self) -> String {
        let surfaces: Vec<String> = self.surfaces.values().map(|s| format!("{s:?}")).collect();
        serde_json::json!({
            "screens": self.screens.iter().map(|s| s.name()).collect::<Vec<_>>(),
            "surfaces": surfaces,
            "focused": self.wm.focused,
            "apps": self.registry.apps().map(|a| a.app_id.clone()).collect::<Vec<_>>(),
            // The bar's open menu, the palette's query, the panel open.
            "menu": self.bar_menu.map(|m| format!("{:?}", m.slot)),
            "palette": self.palette.as_ref().map(|p| p.query.clone()),
            "panel": self.panel.as_ref().map(|p| p.widget.name()),
            // The rectangle of the bar it's under (its widget's).
            "panel_under": self.panel.as_ref().map(|p| [p.under.x0, p.under.y0, p.under.x1, p.under.y1]),
            "pid": std::process::id(),
            // The notifications it keeps (summaries).
            "notes": self.notes.list.iter().map(|n| n.summary.clone()).collect::<Vec<_>>(),
            "launches": self.startup.attempts.iter().map(|(id, a)| serde_json::json!({
                "id": id, "name": a.name, "outcome": a.outcome, "window_observed": a.ready,
                "journal_id": a.journal_id,
            })).collect::<Vec<_>>(),
            "recording": self.recording_json(),
            "sharing": self.sharing_json(),
        })
        .to_string()
    }

    fn refresh_clock(&mut self, cx: &mut Cx<Msg>) {
        self.now = Local::now();
        let day = self.settings.theme_now() == aizome_theme::ThemeName::Day;
        self.theme_changed(day);
        if self.panel.as_ref().is_some_and(|p| p.widget == TrayWidget::Clock) {
            self.refresh_agenda(cx);
        }
    }

    fn arm_clock(&mut self, cx: &mut Cx<Msg>) {
        cx.after(crate::clock::until_next_tick(self.bar_cfg.clock_seconds), Msg::Tick);
    }
}

impl Program for Shell {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Shell", "Aizome", "Desktop Shell")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        if self.opts.live {
            self.start_threads(cx);
            // Once the first frames are out, ready the palette's text.
            cx.after(std::time::Duration::from_secs(2), Msg::WarmPalette);
        }
        self.locate_if_wanted(cx);
        let outputs = cx.outputs();
        self.sync_screens(outputs, cx);
        self.arm_clock(cx);
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        // A menu's closing counts as a press's doing only right before it.
        if !matches!(msg, Msg::Bar(_, BarMsg::Press(..))) {
            self.just_closed = None;
        }
        match msg {
            Msg::Ipc(e) => self.take_ipc(e, cx),
            Msg::Server(e) => self.take_server(e, cx),
            Msg::Apps(AppsLoaded(index)) => {
                if self.opts.live
                    && self._apps_watch.is_none()
                    && let Some(tx) = self.stream.clone()
                {
                    self._apps_watch = Some(index.watch(aizome_sys::Wake::new(move || {
                        tx.send(Msg::AppsChanged);
                    })));
                }
                self.apps = Some(index);
            }
            Msg::AppsChanged => {
                if let Some(tx) = self.stream.clone() {
                    std::thread::spawn(move || {
                        tx.send(Msg::Apps(AppsLoaded(Arc::new(AppIndex::load()))));
                    });
                }
            }
            Msg::Sys => {
                self.tray = self.sys.tray(&self.bar_cfg);
                // What plays, for the overview's "audio playing".
                let playing = self.sys.playing_pids();
                if playing != self.audible_sent {
                    self.ipc.send(aizome_ipc::Request::SetAudible { pids: playing.clone() });
                    self.audible_sent = playing;
                }
                self.tray.awake = (self.awake != aizome_awake::Status::Off).then_some(self.awake);
                self.sync_status_items();
                self.session_events(cx);
            }
            Msg::Meters(r) => self.meters = r,
            Msg::Awake(s) => self.keep_awake_status(s, cx),
            Msg::AwakeSet(r) => self.keep_awake_set(r, cx),
            Msg::Tick => {
                self.refresh_clock(cx);
                self.arm_clock(cx);
            }
            // A timezone change should reach the bar and lock screen now,
            // without waiting for the next minute or starting another timer.
            Msg::TimeChanged => self.refresh_clock(cx),
            Msg::SettingsChanged => {
                self.reload_settings();
                self.locate_if_wanted(cx);
            }
            Msg::Located(found) => self.located(found),
            Msg::VolumeFeedback => self.play_feedback(),
            Msg::Run(a) => self.run(a, cx),
            Msg::Startup(e) => self.startup_event(e, cx),
            Msg::ActivationExpired(request) => {
                if let Some((launch, cwd)) = self.launches.remove(&request) {
                    self.startup_event(crate::startup::Event::Begin { launch, token: None, cwd }, cx);
                }
            }
            Msg::Bar(w, m) => self.bar(w, m, cx),
            Msg::Deskbar(m) => self.deskbar_msg(m, cx),
            Msg::Palette(e) => self.palette_event(e, cx),
            Msg::Panel(m) => self.panel_msg(m, cx),
            Msg::OsdHide(g) => self.hide_osd(g, cx),
            Msg::Desktop(o, m) => self.desktop_msg(o, m, cx),
            Msg::Lock(m) => self.lock_msg(m, cx),
            Msg::Auth(e) => self.auth_in(e, cx),
            Msg::PolkitReady(agent) => match agent {
                Ok(agent) => self.polkit = Some(agent),
                Err(error) => log::warn!("the polkit agent: {error}"),
            },
            Msg::AuthUi(m) => self.auth_msg(m, cx),
            Msg::Tear(m) => self.tear_msg(m, cx),
            Msg::Confirm(m) => self.confirm_msg(m, cx),
            Msg::Card(m) => self.card_msg(m, cx),
            Msg::TearOff { app, path, output, at } => self.tear_off(app, path, output, at, cx),
            Msg::Selection(serial, text) => self.selection_came(serial, text, cx),
            Msg::DesktopItems(items, full) => {
                self.desktop.items = items;
                self.trash_full = full;
            }
            Msg::Note(crate::notifications::server::NoteEvent::NameAcquired) => {
                log::info!("the notifications are this shell's")
            }
            Msg::Note(crate::notifications::server::NoteEvent::NameLost) => {
                // A new shell has the notifications now: its turn.
                log::info!("another shell took over the notifications; saving and stepping down");
                self.save_carried(true);
                self.retiring = true;
            }
            Msg::Note(e) => self.take_note(e, cx),
            Msg::Carried(c) => {
                let c = c.for_restore();
                let added = self.notes.merge(c.notes);
                if added > 0 {
                    log::info!("took in {added} notifications the old shell got meanwhile");
                }
                self.carry_ready = true;
            }
            Msg::BannerDone(id) => self.banner_done(id, cx),
            Msg::Recording(serial, e) => self.recording_event(serial, e, cx),
            Msg::RecordTick(serial, tick) => self.record_tick(serial, tick, cx),
            Msg::MenuClosed(n) => self.menu_closed(n),
            Msg::Agenda(a) => self.agenda = a,
            Msg::FlashDone(n) => self.flash_done(n, cx),
            Msg::Wallpaper(key, image) => self.wallpaper_ready(key, image),
            Msg::WarmPalette => {
                let theme = cx.theme().clone();
                self.warm_palette(&theme);
                cx.reserve_buffers(self.palette_buffer_bytes());
                // From now on the palette's window is kept between
                // openings, drawn ahead: first once the glyphs are in.
                self.palette_ahead.on = true;
                self.palette_ahead_soon(cx);
            }
            Msg::PreparePalette(n) => {
                if n == self.palette_ahead.asked {
                    self.prepare_palette(cx);
                }
            }
            Msg::LogindPowerKey(k) => self.logind_power_key = k,
            Msg::BarSide(d) => self.bar_side(d, cx),
            Msg::ToolchestTyped(text) => self.toolchest_typed(text, cx),
            Msg::ToolchestHandedOver => {
                for s in self.screens.clone() {
                    if let Some(b) = s.bar {
                        cx.close_menus(b);
                    }
                }
            }
            Msg::ItemMenuWanted(bar, slot, rect) => self.item_menu(bar, slot, rect, cx),
            Msg::ItemMenu(bar, slot, rect, menu) => self.show_item_menu(bar, slot, rect, menu, cx),
            Msg::Nothing => {}
        }
        self.after_update(cx);
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        match self.surfaces.get(&window) {
            Some(Surface::MenuBar { output }) => {
                let Some(screen) = self.screens.iter().find(|s| s.output.id == *output) else {
                    return aizome_kit::space(0.0);
                };
                crate::menubar::view(&self.bar_model(screen)).map(move |m| Msg::Bar(window, m))
            }
            Some(Surface::Desktop { output }) => self.desktop_view(*output),
            Some(Surface::Deskbar) => crate::deskbar::view(&self.deskbar_model()).map(Msg::Deskbar),
            Some(Surface::DeskbarPreview) => self.deskbar_preview_view(),
            Some(Surface::DeskbarLanding) => self.deskbar_landing_view(),
            Some(Surface::Palette) => match self.palette_in(window) {
                Some(p) => {
                    let inner = crate::palette::view::view(&self.palette_view_of(p), p.scroll)
                        .map(|m| Msg::Palette(crate::palette::PaletteEvent::Ui(m)));
                    crate::widgets::keys::keys(inner, |k| crate::palette::key_event(k).map(Msg::Palette))
                }
                None => aizome_kit::space(0.0),
            },
            Some(Surface::Lock { output }) => {
                let inner = self.lock_view(*output);
                crate::widgets::keys::keys(inner, |k| {
                    matches!(k.key, aizome_kit::sidestep_ui::Key::Named(aizome_kit::sidestep_ui::NamedKey::Escape))
                        .then_some(Msg::Lock(crate::lock::LockMsg::Escape))
                })
            }
            Some(Surface::TearOff(i)) => self.tear_view(*i),
            Some(Surface::Cards) => crate::notifications::cards::view(&self.card_views()).map(Msg::Card),
            Some(Surface::Startup) => self.startup_view(),
            Some(Surface::Flash) => crate::bell::view(),
            Some(Surface::Confirm) => match &self.confirm {
                Some(c) => crate::confirm::view(c.verb, c.left, c.power).map(Msg::Confirm),
                None => aizome_kit::space(0.0),
            },
            Some(Surface::Auth) => match self.auth_view() {
                Some(v) => crate::auth::view(&v).map(Msg::AuthUi),
                None => aizome_kit::space(0.0),
            },
            Some(Surface::Osd) => match &self.osd {
                Some(o) => crate::osd::view(&o.osd),
                None => aizome_kit::space(0.0),
            },
            Some(Surface::Panel) => match &self.panel {
                Some(p) => crate::systray::panels::view(&self.panel_data(p.widget)).map(Msg::Panel),
                None => aizome_kit::space(0.0),
            },
            None => aizome_kit::space(0.0),
        }
    }

    fn menus(&self) -> MenuBar<Msg> {
        // No menus of its own (it draws the bar), and no shortcuts but the
        // Edit ones its text fields take.
        let i = |label: &str, role: MenuRole, s: Shortcut| MenuItem::role_item(label, role).shortcut(s);
        MenuBar::new().with_menu(
            Menu::new("Edit")
                .item(i("Undo", MenuRole::Undo, Shortcut::cmd('Z')))
                .item(i("Redo", MenuRole::Redo, Shortcut::cmd('Z').shift()))
                .item(i("Cut", MenuRole::Cut, Shortcut::cmd('X')))
                .item(i("Copy", MenuRole::Copy, Shortcut::cmd('C')))
                .item(i("Paste", MenuRole::Paste, Shortcut::cmd('V')))
                .item(i("Select All", MenuRole::SelectAll, Shortcut::cmd('A'))),
        )
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if self.startup.window == Some(window) {
            self.startup.window = None;
        }
        if self.panel.as_ref().is_some_and(|p| p.window == window) {
            self.panel = None;
        }
        // A layer the compositor took away (its output went): forget it; it's
        // opened again when the outputs change.
        if let Some(s) = self.surfaces.remove(&window) {
            if matches!(s, Surface::Lock { .. }) {
                self.lock.surfaces.retain(|(key, _)| *key != window);
                // Reconcile after the kit has destroyed the old surface;
                // creating its replacement here would duplicate its role.
                cx.send(Msg::Nothing);
            }
            for sc in &mut self.screens {
                if sc.bar == Some(window) {
                    sc.bar = None;
                }
                if sc.desktop == Some(window) {
                    sc.desktop = None;
                }
            }
            if s == Surface::Deskbar {
                self.deskbar = None;
                // A drag of its edge or grip can't end now.
                self.end_deskbar_drags(cx);
            }
            if s == Surface::DeskbarPreview {
                self.deskbar_preview = None;
            }
            if s == Surface::DeskbarLanding {
                self.deskbar_landing = None;
            }
        }
        true
    }

    fn outputs_changed(&mut self, cx: &mut Cx<Msg>) {
        let outputs = cx.outputs();
        self.sync_screens(outputs, cx);
    }

    fn pointer_presence(&mut self, window: WindowKey, inside: bool, cx: &mut Cx<Msg>) {
        if self.deskbar == Some(window) {
            // While the button's down the pointer stays the Deskbar's: gone
            // mid-drag, the compositor took it, and the release won't come.
            // The edge or grip drag is over, its width or edge not taken.
            if !inside {
                self.end_deskbar_drags(cx);
            }
            self.deskbar_hover(false, inside, cx);
            self.after_update(cx);
        }
    }

    fn drag_started(&mut self, window: WindowKey, drag: DragId, _cx: &mut Cx<Msg>) {
        if self.deskbar == Some(window) {
            let v = &mut self.deskbar_visibility;
            v.native_drag = Some(drag);
            v.generation = v.generation.wrapping_add(1);
        }
    }

    fn drag_ended(&mut self, drag: DragId, _action: DropAction, cx: &mut Cx<Msg>) {
        if self.deskbar_visibility.native_drag == Some(drag) {
            self.deskbar_visibility.native_drag = None;
            self.arm_deskbar_hide(cx);
            self.after_update(cx);
        }
    }

    fn activation_token(&mut self, request: ActivationRequest, token: Option<String>, cx: &mut Cx<Msg>) {
        if let Some((l, cwd)) = self.launches.remove(&request) {
            self.startup_event(crate::startup::Event::Begin { launch: l, token, cwd }, cx);
        }
    }

    fn quit_when_last_window_closes(&self) -> bool {
        false
    }

    /// The shell is where menus go; it doesn't publish to itself.
    fn menus_to_shell(&self) -> bool {
        false
    }

    fn session_locked(&mut self, cx: &mut Cx<Msg>) {
        self.lock_confirmed(cx);
    }

    fn session_lock_finished(&mut self, _cx: &mut Cx<Msg>) {
        self.lock_ended();
    }
}

/// The pointer's place as a rectangle to anchor a menu at.
pub fn at_point(p: Point) -> Rect {
    Rect::from_origin_size(p, (1.0, 1.0))
}

/// Ask the compositor what app owns a window, for `AppSel`.
pub fn app_sel(app_id: &str) -> AppSel {
    AppSel { app_id: Some(app_id.to_owned()), pid: None }
}

/// A desktop surface's window spec, titled for logs and tests.
/// The tag of the Night Light question asked at connecting.
const NIGHT_LIGHT_TAG: u64 = 0x4e49_4748;
const SHARES_TAG: u64 = 0x5348_4152;

pub fn layer(opts: LayerOptions, width: f64, height: f64, title: &str) -> WindowSpec {
    let mut s = WindowSpec::layer(opts, width, height);
    s.title = title.to_owned();
    s
}

impl Shell {
    /// What every update ends with: surfaces sized to their content, the
    /// geometry drags need.
    pub fn after_update(&mut self, cx: &mut Cx<Msg>) {
        self.sync_lock_surfaces(cx);
        if self.opts.live && self.carry_ready && !self.retiring {
            self.save_carried(false);
        }
        self.find_icons();
        self.prepare_wallpapers(cx);
        for a in std::mem::take(&mut self.pending_actions) {
            self.run(a, cx);
        }
        self.sync_deskbar(cx);
        self.sync_panel(cx);
        self.sync_tearoffs(cx);
        self.sync_cards(cx);
        self.keep_palette_ahead(cx);
        if let Some(screen) = self.primary().cloned() {
            let model = self.bar_model(&screen);
            let theme = cx.theme().clone();
            let text = &mut self.text;
            let mut measure =
                |s: &str, spec: &aizome_kit::text::TextSpec| text.layout(&theme, s, spec, None).size().width;
            self.desk_strip = crate::menubar::desk_rects(&theme, &mut measure, &model, screen.output.frame.width());
        }
    }
}

impl Shell {
    /// Saves what the next shell takes up, if it changed (`last`: always,
    /// as this shell's last word).
    fn save_carried(&mut self, last: bool) {
        let folded: Vec<(String, bool)> = {
            let mut f: Vec<(String, bool)> = self.folded.iter().map(|(k, v)| (k.clone(), *v)).collect();
            f.sort();
            f
        };
        let mark = crate::carry::mark(&self.notes.list, &folded, &self.notes_open);
        if !last && self.carried == Some(mark) {
            return;
        }
        self.carried = Some(mark);
        crate::carry::save(&crate::carry::Carried {
            pid: std::process::id(),
            last,
            notes: self.notes.list.clone(),
            next_id: self.notes.next_id(),
            folded,
            notes_open: self.notes_open.clone(),
        });
    }

    /// About an app: its name, generic name and description, in the bar's
    /// line.
    fn about_app(&mut self, app_id: &str, cx: &mut Cx<Msg>) {
        let (name, _, _) = self.app_look(app_id);
        let entry = self.apps.as_deref().and_then(|i| i.match_window(app_id)).cloned();
        let body = entry
            .map(|e| {
                [e.generic_name.unwrap_or_default(), e.comment.unwrap_or_default()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" — ")
            })
            .unwrap_or_default();
        let n = crate::notifications::store::Incoming {
            app_name: name.clone(),
            summary: name,
            body: if body.is_empty() { app_id.to_owned() } else { body },
            transient: true,
            desktop_entry: Some(app_id.to_owned()),
            ..Default::default()
        };
        let id = self.fresh_note_id();
        self.take_note(crate::notifications::server::NoteEvent::Notify(id, Box::new(n)), cx);
    }

    /// Open the focused app's menu a command is in (its trail's first
    /// part), under the bar.
    pub fn show_in_menu(&mut self, trail: &str, cx: &mut Cx<Msg>) {
        let first = trail.split(" › ").next().unwrap_or("");
        let (_, menus) = self.focused_menus();
        let Some(i) = menus.iter().position(|m| m.title == first) else { return };
        let Some(bar) = self.focused_screen().and_then(|s| s.bar) else { return };
        let Some((_, rect)) = self.titles(bar, cx).into_iter().find(|(s, _)| *s == Slot::Menu(i)) else { return };
        self.open_slot(bar, Slot::Menu(i), rect, false, cx);
    }

    /// Night or Day: written to the settings, which the compositor and every
    /// app follow.
    pub fn set_theme(&mut self, day: bool) {
        self.settings.appearance.theme =
            if day { aizome_config::ThemeChoice::Day } else { aizome_config::ThemeChoice::Night };
        if self.opts.live {
            if let Err(e) = self.settings.save() {
                log::warn!("saving the theme: {e}");
            }
            self.ipc.send(Request::ReloadConfig);
        }
        self.theme_changed(day);
    }

    /// The theme turned: foot instances and GTK follow (foot selects its
    /// dark palette on SIGUSR1 and its light palette on SIGUSR2).
    pub fn theme_changed(&mut self, day: bool) {
        self.follow_gtk();
        if self.theme_day == day {
            return;
        }
        self.theme_day = day;
        self.follow_foot(self.wm.windows.values().filter_map(|w| w.pid));
    }

    /// Tell only mapped Foot processes belonging to this compositor. A
    /// custom app_id still works; checking the process name keeps other
    /// clients from being sent a signal just because they call themselves
    /// "foot". The snapshot also scopes us to the right session, including
    /// sessions with the same WAYLAND_DISPLAY in different runtime dirs.
    fn follow_foot(&self, pids: impl IntoIterator<Item = i32>) {
        if !self.opts.live {
            return;
        }
        let signal = if self.theme_day { libc::SIGUSR2 } else { libc::SIGUSR1 };
        let me = unsafe { libc::getuid() };
        // A foot server can own several windows. Deliver in event order,
        // without racing background scans from successive theme changes.
        for pid in pids.into_iter().collect::<std::collections::BTreeSet<_>>() {
            if pid <= 0 {
                continue;
            }
            let path = PathBuf::from(format!("/proc/{pid}"));
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if std::os::unix::fs::MetadataExt::uid(&meta) != me
                || std::fs::read_to_string(path.join("comm")).unwrap_or_default().trim() != "foot"
            {
                continue;
            }
            // SAFETY: a positive PID of the user's mapped Foot process.
            unsafe { libc::kill(pid, signal) };
        }
    }

    /// GTK and libadwaita apps follow the theme and the focus colour
    /// (dconf), told only when what they'd be told changes.
    pub fn follow_gtk(&mut self) {
        if !self.opts.live {
            return;
        }
        let look = crate::gtk::look(self.settings.theme_now(), self.settings.appearance.focus);
        if self.gtk_look != Some(look) {
            self.gtk_look = Some(look);
            crate::gtk::tell(look);
        }
    }
}

impl Shell {
    /// The focused app's whole menu at a point of the global layout (NeXT's
    /// menu at the pointer), as a popup of that output's bar.
    pub fn pointer_menu(&mut self, x: f64, y: f64, cx: &mut Cx<Msg>) {
        let screen =
            self.screens.iter().find(|s| s.output.frame.contains(Point::new(x, y))).or(self.focused_screen()).cloned();
        let Some(screen) = screen else { return };
        let Some(bar) = screen.bar else { return };
        let (name, menus) = self.focused_menus();
        let window = self.wm.focused_window();
        let hide = Action::Wm(Request::HideApp { app: AppSel { app_id: None, pid: window.and_then(|w| w.pid) } });
        let quit = match window {
            Some(w) => {
                let conn = self.registry.for_window(w.pid, &w.app_id);
                let quit_item = menus
                    .first()
                    .and_then(|m| m.flatten().into_iter().find(|f| f.entry.role.as_deref() == Some("quit")));
                match (conn, quit_item) {
                    (Some(_), Some(f)) => f.entry.action,
                    _ => Action::QuitApp { app_id: w.app_id.clone() },
                }
            }
            None => Action::Session(SessionVerb::LogOut),
        };
        let text = self.selection.clone();
        let menu = build::pointer_menu(&name, &menus, build::services(text.as_deref()), hide, quit);
        let at = Point::new(x - screen.output.frame.x0, y - screen.output.frame.y0);
        let app = match window {
            Some(w) => w.app_id.clone(),
            None => crate::tearoff::DESKTOP.into(),
        };
        let tear = Msg::TearOff { app: app.clone(), path: vec![], output: screen.name(), at };
        let app_menu = menus.first().map(|m| m.title.clone());
        let subs = crate::tearoff::tear_sub(app, menu.clone(), vec![], app_menu, screen.name(), at);
        let style = MenuStyle::at_point().titled(name.clone()).on_tear_off(tear).on_tear_off_sub(subs);
        self.show_menu(bar, at_point(at), &menu, style, cx);
    }
}

/// Whether this shell runs an Aizome session (not a test beside another
/// desktop): only then does it hold logind's sleep, serve polkit, and lock
/// for logind.
pub fn real_session() -> bool {
    let var = |name| std::env::var(name).ok();
    is_real_session(var("AIZOME_SESSION").as_deref(), var("XDG_CURRENT_DESKTOP").as_deref())
}

/// The decision, from `AIZOME_SESSION` and `XDG_CURRENT_DESKTOP`. The
/// compositor says so (`AIZOME_SESSION`): headless and nested compositors
/// run inside the user's real login session too, so the desktop's name
/// alone doesn't tell. Without it (a compositor from before it said) the
/// name decides.
fn is_real_session(session: Option<&str>, desktops: Option<&str>) -> bool {
    match session {
        Some(v) => v == "1",
        None => desktops.is_some_and(|v| v.split(':').any(|d| d.eq_ignore_ascii_case("aizome"))),
    }
}

impl Shell {
    /// logind's session events: lock and unlock asked for, the machine
    /// about to sleep (lock first, then let it).
    fn session_events(&mut self, cx: &mut Cx<Msg>) {
        let events = match &self.sys.logind {
            Some(l) if real_session() => l.take_events(),
            _ => return,
        };
        use aizome_sys::power::SessionEvent;
        for e in events {
            match e {
                SessionEvent::Lock => self.lock(cx),
                SessionEvent::Unlock => self.unlock(cx),
                SessionEvent::PrepareForSleep => {
                    if self.lock_cfg.before_sleep && self.lock.phase == crate::lock::Phase::Unlocked {
                        self.lock.sleep_pending = true;
                        self.lock(cx);
                    } else if let Some(l) = &self.sys.logind {
                        l.sleep_ready();
                    }
                }
                SessionEvent::Resumed => {
                    self.sync_screens(cx.outputs(), cx);
                    // Take the delay again for the next sleep.
                    if let Some(l) = &self.sys.logind
                        && let Err(e) = l.take_sleep_delay()
                    {
                        log::warn!("logind's sleep delay: {e}");
                    }
                }
                SessionEvent::PrepareForShutdown => {}
            }
        }
    }

    /// What the lock screen needs from the system: PAM's service, the
    /// user's picture, and logind's sleep delay.
    fn start_lock(&mut self) {
        self.authenticator = Arc::new(crate::pam::Pam::new(self.lock_cfg.pam_service.clone()));
        if self.lock_cfg.show_picture {
            let home = aizome_config::home_dir();
            let candidates =
                [home.join(".face"), PathBuf::from(format!("/var/lib/AccountsService/icons/{}", self.user.0))];
            self.user_picture = candidates.iter().find(|p| p.is_file()).and_then(|p| aizome_kit::Image::open(p).ok());
        }
        if real_session()
            && self.lock_cfg.before_sleep
            && let Some(l) = &self.sys.logind
            && let Err(e) = l.take_sleep_delay()
        {
            log::warn!("logind's sleep delay: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_settings() -> (aizome_kit::testing::Harness<Shell>, std::sync::mpsc::Receiver<(Request, Option<u64>)>) {
        let mut h = aizome_kit::testing::Harness::new(Shell::new(Options::default()));
        let (ipc, requests) = Ipc::recording();
        let s = h.program_mut();
        s.ipc = ipc;
        s.wm.reset(crate::wm::Snapshot {
            windows: vec![aizome_ipc::WindowInfo {
                id: 42,
                app_id: crate::launch::SETTEI.into(),
                desk: Some(2),
                minimised: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        // Record launch requests without delivering activation tokens or
        // starting any process in the user's session.
        s.opts.live = true;
        (h, requests)
    }

    #[test]
    fn opening_a_settings_pane_raises_the_existing_window_and_forwards_the_pane() {
        let (mut h, requests) = running_settings();
        h.send(Msg::Run(Action::Settings { pane: "keyboard".into(), anchor: Some("caps-lock".into()) }));
        assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: 42 });
        assert!(
            h.program()
                .launches
                .values()
                .any(|(launch, _)| { *launch == crate::launch::settings("keyboard", Some("caps-lock")) })
        );
    }

    #[test]
    fn setting_up_a_keyboard_opens_settei_with_it_chosen() {
        let (mut h, requests) = running_settings();
        let (name, id) = ("Yubico YubiKey OTP+FIDO+CCID", "1050:0407");
        h.send(Msg::Run(Action::SetUpKeyboard { name: name.into(), id: id.into() }));
        assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: 42 });
        let want = crate::launch::keyboard_setup(name, id);
        assert!(h.program().launches.values().any(|(launch, _)| *launch == want), "{:?}", h.program().launches);
    }

    #[test]
    fn opening_a_running_app_focuses_it_but_new_window_still_launches() {
        let (mut h, requests) = running_settings();
        h.send(Msg::Run(Action::Launch { app_id: crate::launch::SETTEI.into() }));
        assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: 42 });
        assert!(h.program().launches.is_empty());
        h.send(Msg::Run(Action::NewWindow { app_id: crate::launch::SETTEI.into() }));
        assert!(requests.try_recv().is_err());
        assert_eq!(h.program().launches.len(), 1);
    }

    #[test]
    fn app_activation_uses_recency_across_desktop_id_and_wm_class_aliases() {
        let (mut h, requests) = running_settings();
        let s = h.program_mut();
        s.apps = Some(crate::apps::test_index(&[("code", "StartupWMClass=EditorWindow")]));
        s.wm.reset(crate::wm::Snapshot {
            windows: ["code", "Code", "EditorWindow"]
                .into_iter()
                .enumerate()
                .map(|(id, app_id)| aizome_ipc::WindowInfo {
                    id: id as u64 + 10,
                    app_id: app_id.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        for (focused, requested) in [(11, "code"), (12, "Code.desktop"), (10, "EditorWindow")] {
            h.program_mut().wm.apply(&aizome_ipc::Event::WindowFocused { id: Some(focused) });
            h.send(Msg::Run(Action::Launch { app_id: requested.into() }));
            assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: focused });
            assert!(h.program().launches.is_empty());
        }
    }

    #[test]
    fn app_activation_does_not_confuse_reverse_dns_suffixes() {
        let (mut h, requests) = running_settings();
        let s = h.program_mut();
        s.apps = Some(crate::apps::test_index(&[("com.spotify.Client", "")]));
        s.wm.reset(crate::wm::Snapshot {
            windows: vec![aizome_ipc::WindowInfo { id: 10, app_id: "org.other.Client".into(), ..Default::default() }],
            ..Default::default()
        });
        h.send(Msg::Run(Action::Launch { app_id: "com.spotify.Client".into() }));
        assert!(requests.try_recv().is_err());
        assert_eq!(h.program().launches.len(), 1);
    }

    #[test]
    fn notification_without_a_window_launches_only_a_known_desktop_identity() {
        use crate::notifications::{cards::CardMsg, store::Incoming};
        for (name, hint, expected_app) in [
            ("Editor", Some("org.example.Editor.desktop"), Some("org.example.Editor")),
            ("org.other.Editor", None, None),
            ("unknown-program --flag", None, None),
            ("org.example.Editor", Some("org.other.Editor.desktop"), None),
        ] {
            let mut h = aizome_kit::testing::Harness::new(Shell::new(Options::default()));
            let (ipc, requests) = Ipc::recording();
            let s = h.program_mut();
            s.ipc = ipc;
            s.apps = Some(crate::apps::test_index(&[("org.example.Editor", "")]));
            // Activation requests are recorded; no token is delivered, so
            // the harness cannot start an actual application.
            s.opts.live = true;
            let incoming =
                Incoming { app_name: name.into(), desktop_entry: hint.map(str::to_owned), ..Default::default() };
            let filed = s.file_under(&incoming);
            s.notes.post_as(7, incoming, filed, false, 0);
            h.send(Msg::Card(CardMsg::Open(7)));
            assert!(requests.try_recv().is_err(), "there is no running window to focus");
            let launches: Vec<_> = h.program().launches.values().map(|(launch, _)| launch.clone()).collect();
            let expected: Vec<_> =
                expected_app.map(|app_id| Launch::App { app_id: app_id.into(), targets: vec![] }).into_iter().collect();
            assert_eq!(launches, expected, "sender {name:?}, desktop hint {hint:?}");
            assert!(h.program().notes.get(7).is_none());
        }
    }

    fn app_identity_fixture() -> (aizome_kit::testing::Harness<Shell>, std::sync::mpsc::Receiver<(Request, Option<u64>)>)
    {
        use aizome_ipc::WindowInfo;
        use aizome_sys::apps::{Locale, XdgDirs};
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("aizome-shell-identity-{}-{n}", std::process::id()));
        let dirs = XdgDirs::rooted(&root);
        let apps = dirs.data_home.join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("org.example.Editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=example-editor\nStartupWMClass=EditorWindow\n",
        )
        .unwrap();
        let mut shell = Shell::new(Options::default());
        shell.apps = Some(Arc::new(AppIndex::load_with(dirs, Locale::default())));
        std::fs::remove_dir_all(root).unwrap();
        shell.wm.reset(crate::wm::Snapshot {
            current_desk: 1,
            windows: ["org.example.Editor", "EditorWindow", "org.unrelated.Editor", "EditorWindow", "example-editor"]
                .into_iter()
                .enumerate()
                .map(|(i, app)| WindowInfo {
                    id: i as u64 + 1,
                    app_id: app.into(),
                    desk: Some(1),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        shell.wm.apply(&Event::WindowFocused { id: Some(2) });
        shell.wm.apply(&Event::WindowFocused { id: Some(3) });
        let (ipc, requests) = Ipc::recording();
        shell.ipc = ipc;
        (aizome_kit::testing::Harness::new(shell), requests)
    }

    #[test]
    fn activation_and_quit_do_not_target_apps_with_similar_names() {
        let (mut h, requests) = app_identity_fixture();
        for app_id in ["org.example.Editor", "org.example.Editor.desktop", "editorwindow"] {
            h.send(Msg::Run(Action::FocusApp { app_id: app_id.into() }));
            assert_eq!(requests.try_recv().unwrap(), (Request::Focus { id: 2 }, None));
            h.send(Msg::Run(Action::QuitApp { app_id: app_id.into() }));
            let mut ids: Vec<_> = requests
                .try_iter()
                .map(|(req, _)| match req {
                    Request::Close { id: Some(id) } => id,
                    other => panic!("unexpected request: {other:?}"),
                })
                .collect();
            ids.sort();
            assert_eq!(ids, [1, 2, 4], "only the desktop ID and its explicit alias may be closed");
        }
        for (app_id, id) in [("org.unrelated.Editor", 3), ("example-editor", 5)] {
            h.send(Msg::Run(Action::QuitApp { app_id: app_id.into() }));
            assert_eq!(requests.try_recv().unwrap(), (Request::Close { id: Some(id) }, None));
            assert!(requests.try_recv().is_err(), "unindexed apps still match their own exact ID");
        }
    }

    #[test]
    fn deskbar_hide_resolves_aliases_and_deduplicates_window_app_ids() {
        use crate::deskbar::DeskbarMsg;
        use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
        let (mut h, requests) = app_identity_fixture();
        // Only the StartupWMClass alias is running; the pinned desktop ID
        // must still offer Hide and send the ID the compositor recognizes.
        h.program_mut().wm.apply(&Event::WindowClosed { id: 1 });
        h.program_mut().deskbar_cfg.pinned = vec!["org.example.Editor".into()];
        h.set_outputs(vec![Output {
            id: 1,
            name: "test".into(),
            frame: Rect::new(0.0, 0.0, 1280.0, 800.0),
            scale: 1.0,
            integer_scale: 1,
            ..Default::default()
        }]);
        h.send(Msg::Nothing);
        let deskbar = h.program().deskbar.expect("Deskbar open");
        requests.try_iter().for_each(drop);
        h.send(Msg::Deskbar(DeskbarMsg::AppMenu("org.example.Editor".into(), Point::new(10.0, 10.0))));
        assert_eq!(h.popups(deskbar).len(), 1);
        h.type_text(deskbar, "Hide");
        h.named(deskbar, NamedKey::Enter, Modifiers::NONE);
        assert!(h.popups(deskbar).is_empty());
        assert_eq!(requests.try_recv().unwrap(), (Request::HideApp { app: app_sel("EditorWindow") }, None));
        assert!(requests.try_recv().is_err(), "two alias windows need one HideApp; unrelated apps are excluded");

        // An app may have windows using both its desktop ID and its alias.
        h.program_mut().wm.apply(&Event::WindowOpened {
            window: aizome_ipc::WindowInfo {
                id: 1,
                app_id: "org.example.Editor".into(),
                desk: Some(1),
                ..Default::default()
            },
        });
        h.send(Msg::Run(Action::HideApp { app_id: "org.example.Editor.desktop".into() }));
        let sent: Vec<_> = requests.try_iter().collect();
        assert_eq!(
            sent,
            [
                (Request::HideApp { app: app_sel("EditorWindow") }, None),
                (Request::HideApp { app: app_sel("org.example.Editor") }, None),
            ]
        );
        h.send(Msg::Run(Action::HideApp { app_id: "not-running".into() }));
        assert!(requests.try_recv().is_err(), "an app with no windows has nothing to hide");
    }

    #[test]
    fn launchers_and_deskbar_activate_the_local_window_while_new_window_launches() {
        use crate::deskbar::DeskbarMsg;
        use aizome_ipc::WindowInfo;
        use aizome_kit::testing::Harness;

        let mut shell = Shell::new(Options::default());
        shell.wm.reset(crate::wm::Snapshot {
            current_desk: 1,
            windows: vec![
                WindowInfo { id: 1, app_id: "editor".into(), desk: Some(1), minimised: true, ..Default::default() },
                WindowInfo { id: 2, app_id: "editor".into(), desk: Some(2), ..Default::default() },
            ],
            focused: Some(2),
            ..Default::default()
        });
        let (ipc, requests) = Ipc::recording();
        shell.ipc = ipc;
        let mut h = Harness::new(shell);
        for msg in [
            Msg::Run(Action::Launch { app_id: "editor".into() }),
            Msg::Run(Action::FocusApp { app_id: "editor".into() }),
            Msg::Deskbar(DeskbarMsg::FocusApp("editor".into())),
        ] {
            h.send(msg);
            assert_eq!(requests.try_recv().unwrap(), (Request::Focus { id: 1 }, None));
            assert!(requests.try_recv().is_err(), "one request reveals and focuses the selected window");
        }
        h.send(Msg::Run(Action::NewWindow { app_id: "editor".into() }));
        assert!(requests.try_recv().is_err(), "New Window never substitutes a focus request");
    }

    #[test]
    fn the_compositor_says_whether_the_session_is_real() {
        // aizome.service's compositor, on a seat.
        assert!(is_real_session(Some("1"), Some("Aizome")));
        // scripts/headless-session, the tests, a nested development session:
        // Aizome's desktop, inside the user's real session.
        assert!(!is_real_session(Some("0"), Some("Aizome")));
        assert!(!is_real_session(Some(""), Some("Aizome")));
        // A compositor from before AIZOME_SESSION: the desktop's name decides.
        assert!(is_real_session(None, Some("foo:aizome")));
        assert!(!is_real_session(None, Some("Hyprland")));
        assert!(!is_real_session(None, None));
    }

    // ---- two displays, each showing a Desk of its own ------------------

    /// The laptop's panel at the layout's origin, a monitor right of it.
    fn two_displays() -> Vec<aizome_kit::Output> {
        let out = |id: u32, name: &str, x: f64, w: f64, h: f64| aizome_kit::Output {
            id,
            name: name.into(),
            connector: Some(name.into()),
            description: None,
            make: String::new(),
            model: String::new(),
            frame: Rect::new(x, 0.0, x + w, h),
            scale: 2.0,
            integer_scale: 2,
            physical_size: None,
            refresh_hz: 60.0,
            work_area: None,
        };
        vec![out(1, "eDP-1", 0.0, 1440.0, 900.0), out(2, "DP-1", 1440.0, 2560.0, 1440.0)]
    }

    fn infos(primary: &str) -> Vec<OutputInfo> {
        [("eDP-1", 0, 1440, 900), ("DP-1", 1440, 2560, 1440)]
            .into_iter()
            .map(|(name, x, w, h)| OutputInfo {
                name: name.into(),
                enabled: true,
                primary: name == primary,
                scale: 2.0,
                logical: aizome_ipc::Rect::new(x, 0, w, h),
                usable: aizome_ipc::Rect::new(x, 24, w, h - 24),
                ..Default::default()
            })
            .collect()
    }

    /// Mail, Build and Write: the monitor shows `monitor`, the laptop
    /// `laptop`, and `current` has the focus.
    fn desks(monitor: u32, laptop: u32, current: u32) -> Vec<aizome_ipc::DeskInfo> {
        ["Mail", "Build", "Write"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let index = i as u32 + 1;
                aizome_ipc::DeskInfo {
                    index,
                    id: index as u64,
                    name: name.into(),
                    current: index == current,
                    output: match index {
                        i if i == monitor => Some("DP-1".into()),
                        i if i == laptop => Some("eDP-1".into()),
                        _ => None,
                    },
                    ..Default::default()
                }
            })
            .collect()
    }

    fn connected(primary: &str, desks: Vec<aizome_ipc::DeskInfo>) -> Msg {
        let current_desk = desks.iter().find(|d| d.current).map_or(1, |d| d.index);
        let s = crate::wm::Snapshot { desks, current_desk, outputs: infos(primary), ..Default::default() };
        Msg::Ipc(IpcEvent::Connected(Box::new(crate::ipc::SnapshotBox(s))))
    }

    fn two_displays_shell() -> (aizome_kit::testing::Harness<Shell>, std::sync::mpsc::Receiver<(Request, Option<u64>)>)
    {
        let mut h = aizome_kit::testing::Harness::new(Shell::new(Options::default()));
        let (ipc, requests) = Ipc::recording();
        h.program_mut().ipc = ipc;
        h.set_outputs(two_displays());
        (h, requests)
    }

    #[test]
    fn the_deskbar_shows_on_the_main_display_and_follows_it() {
        let (mut h, _) = two_displays_shell();
        // Before the compositor says, the display at the layout's origin.
        assert_eq!(h.program().deskbar_output, Some(1));
        let first = h.program().deskbar;
        h.send(connected("DP-1", desks(1, 2, 1)));
        assert_eq!(h.program().primary().map(|s| s.name()).as_deref(), Some("DP-1"));
        assert_eq!(h.program().deskbar_output, Some(2), "on the main display, right of the origin");
        assert!(h.program().deskbar.is_some() && h.program().deskbar != first, "opened again there");
        // Another main display chosen (Settings, `aizomectl primary`).
        h.send(Msg::Ipc(IpcEvent::Event(Event::OutputsChanged { outputs: infos("eDP-1") })));
        assert_eq!(h.program().deskbar_output, Some(1));
        let bars: Vec<bool> = h.program().screens.iter().map(|s| h.program().bar_model(s).primary).collect();
        assert_eq!(bars, vec![true, false], "the laptop's bar is the main one now");
    }

    #[test]
    fn with_no_window_focused_the_palette_opens_on_the_display_with_the_focus() {
        let (mut h, _) = two_displays_shell();
        // The focus is on the laptop (showing Build), the main display is
        // the monitor.
        h.send(connected("DP-1", desks(1, 2, 2)));
        assert_eq!(h.program().focused_screen().map(|s| s.name()).as_deref(), Some("eDP-1"));
        h.send(Msg::Ipc(IpcEvent::Event(Event::Binding {
            action: "palette".into(),
            pointer: None,
            output: Some("DP-1".into()),
        })));
        assert!(h.program().palette.is_some());
        assert_eq!(h.program().palette_ahead.output, 1, "on the laptop, where the keys are, not under the pointer");
        // The focus goes to the monitor.
        h.send(Msg::Ipc(IpcEvent::Event(Event::DesksChanged { desks: desks(1, 2, 1) })));
        assert_eq!(h.program().focused_screen().map(|s| s.name()).as_deref(), Some("DP-1"));
    }

    #[test]
    fn each_bar_lights_the_desk_its_display_shows_and_shows_desks_there() {
        let (mut h, requests) = two_displays_shell();
        h.send(connected("DP-1", desks(3, 1, 3)));
        let models: Vec<BarModel> = h.program().screens.iter().map(|s| h.program().bar_model(s)).collect();
        assert_eq!(models.iter().map(|m| m.output.as_str()).collect::<Vec<_>>(), ["eDP-1", "DP-1"]);
        for m in &models {
            let lit: Vec<&str> =
                m.desks.iter().filter(|d| d.output.as_deref() == Some(&m.output)).map(|d| d.name.as_str()).collect();
            assert_eq!(lit, [if m.output == "DP-1" { "Write" } else { "Mail" }]);
        }
        // Build, on no display, from the laptop's bar: shown there.
        let laptop = h.program().screens[0].bar.unwrap();
        h.resize(laptop, aizome_kit::Size::new(1440.0, crate::menubar::HEIGHT));
        while requests.try_recv().is_ok() {}
        assert!(h.click_text(laptop, "2 Build"), "{:?}", h.texts(laptop));
        let sent: Vec<Request> = requests.try_iter().map(|(r, _)| r).collect();
        assert!(
            sent.contains(&Request::SetDesk { desk: aizome_ipc::DeskSel::Index(2), output: Some("eDP-1".into()) }),
            "{sent:?}"
        );
    }
}
