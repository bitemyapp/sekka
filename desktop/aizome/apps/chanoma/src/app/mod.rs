//! Chanoma, the program: which screen shows, what the controller does
//! there, and what's playing.
//!
//! **Input.** Controllers come from [`crate::pad`] as messages; a keyboard
//! (and a mouse, for grown-ups) through the window. Both become an
//! [`Input`] for menus. A game takes its controllers straight from the
//! shared state; a page in the browser gets keys and the pointer through
//! the compositor. The Home button (or Select and Start held) always
//! brings up a menu with the way home.
//!
//! **Kids and grown-ups.** Kids see what the grown-ups allowed, for the
//! time allowed. The PIN unlocks grown-up mode: everything shows, nothing
//! counts, and the settings open. It locks again by itself after a while
//! untouched (not in the middle of a film), or when asked.

pub mod grownups;
pub mod play;
pub mod rows;
pub mod state;
pub mod time;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::MsgSender;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{KeyEvent, NamedKey};
use aizome_kit::widget::{Event as WEvent, FocusRing};
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions, WindowState, over_video};

use crate::clock::{Clock, Local, SystemClock};
use crate::library::Library;
use crate::pad::mapping::{Database, Mapping};
use crate::pad::{Button, Dir, Family, PadEvent, PadInfo, Pads, Shared};
use crate::rules::{Confirm, Gate, Lockout, Settings, Usage};
use crate::store::Dirs;
use crate::ui::{self, Chrome, Focus, Overlay, PadBadge, Scene, Target, View};
use state::Recent;

pub use crate::APP_ID;

/// The window's videos: a game's picture, a film's.
pub const GAME: aizome_kit::video::VideoId = 1;
pub const FILM: aizome_kit::video::VideoId = 2;
/// Pictures kept before those far from the screen go.
const ART_KEPT: usize = 200;

/// What menus hear, from a controller or the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Dir(Dir),
    Ok,
    Back,
    /// The Home button.
    Home,
    /// Start.
    Menu,
    /// The shoulder buttons.
    Prev,
    Next,
    Digit(u8),
    Erase,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Pad(PadEvent),
    Input(Input),
    Click(Target),
    Tick,
    Scanned(Arc<Library>),
    Art(PathBuf, Option<Image>),
    /// A PIN typed was checked: right or not.
    PinChecked(bool),
    /// A new PIN's hash, made off the interface thread.
    PinMade(crate::rules::PinHash),
    Game(crate::retro::Event),
    Browser(crate::browser::Event),
    Film(aizome_media::Event),
    Bluetooth,
    Paired(String, Result<(), String>),
    Folder(grownups::FolderKind, Option<Vec<PathBuf>>),
    SiteName(String),
    SiteUrl(String),
    SiteAdd,
    SiteCancel,
    Outputs,
    /// Another launch: come forward.
    Activate,
    Quit,
}

/// The screen showing.
#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    /// First run: a grown-up sets Chanoma up.
    Welcome,
    Home,
    /// Every game, by system.
    Games,
    /// A show's episodes.
    Episodes(usize),
    /// Something is starting (its name).
    Starting(String),
    Playing,
    Watching,
    Film,
    GrownUps,
    /// Time's up or bedtime.
    Closed,
    Pairing,
    SetUpPad,
    /// Something went wrong: what.
    Problem(String, String),
}

/// What happens once the PIN is right.
#[derive(Clone, Debug, PartialEq)]
pub enum AfterPin {
    /// Grown-up mode.
    Unlock,
    /// Grown-up mode, then the settings.
    Settings,
    /// Grown-up mode, then what to do about time.
    Time,
    /// Leave Chanoma.
    Leave,
}

/// The PIN pad's own state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PinState {
    pub typed: String,
    pub error: Option<String>,
    pub checking: bool,
    /// Setting a new PIN: the first time it was typed.
    pub first: Option<String>,
    pub new: bool,
}

/// What's over the screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Modal {
    Pin { pin: PinState, after: AfterPin },
    Menu { menu: ui::menu::Menu, kind: MenuKind },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// Over a game: keep playing, start again, stop.
    Game,
    /// Over a film.
    Film,
    /// Over a page in the browser (on the overlay).
    Page,
    /// Time's up, for a grown-up: give more, or carry on as a grown-up.
    Time,
    /// The controller playing went away.
    NoController,
}

/// What the program runs with: real devices, or a test's stand-ins.
pub struct Config {
    pub dirs: Dirs,
    pub clock: Box<dyn Clock>,
    /// Read controllers (off in tests: they send `PadEvent`s).
    pub pads: bool,
    /// Grab controllers from other programs.
    pub grab: bool,
    /// Play without sound.
    pub silent: bool,
    /// PBKDF2 rounds for a new PIN.
    pub pin_rounds: u32,
    /// Where cores are looked for.
    pub core_dirs: Vec<PathBuf>,
    /// SDL's controller database.
    pub database: Option<PathBuf>,
    /// Open full screen.
    pub fullscreen: bool,
    /// Scan the folders at start.
    pub scan: bool,
    /// Play every game with these entry points rather than a core
    /// library (tests).
    pub core_override: Option<crate::retro::ffi::Api>,
    /// Stop tidily when asked by a signal (`signals::block` was called).
    pub signals: bool,
    /// Clocks count 24 hours, as the menu bar's does unless it's set to
    /// twelve.
    pub h24: bool,
}

impl Config {
    pub fn standard() -> Config {
        Config {
            dirs: Dirs::standard(),
            clock: Box::new(SystemClock),
            pads: true,
            grab: true,
            silent: false,
            pin_rounds: crate::rules::ROUNDS,
            core_dirs: crate::library::Cores::search_path(),
            database: std::env::var_os("CHANOMA_GAMECONTROLLERDB")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("SDL_GAMECONTROLLERCONFIG_FILE").map(PathBuf::from)),
            fullscreen: true,
            scan: true,
            core_override: None,
            signals: false,
            h24: aizome_config::Settings::load_or_default().menubar_settings().clock == aizome_config::ClockFormat::H24,
        }
    }
}

pub struct Chanoma {
    pub config: Config,
    pub settings: Settings,
    pub usage: Usage,
    pub lockout: Lockout,
    pub recent: Recent,
    pub layouts: HashMap<String, Mapping>,
    pub library: Arc<Library>,
    pub scanned: bool,
    pub pads: Option<Pads>,
    pub shared: Arc<Shared>,
    pub controllers: Vec<PadInfo>,
    /// The controller used last: whose buttons the hints show.
    pub family: Family,
    pub window: WindowKey,
    /// The overlay over a page in the browser.
    pub hud: Option<WindowKey>,
    /// The display's height in pixels.
    pub lines: u32,
    pub screen: Screen,
    pub focus: Focus,
    /// The focus in a menu or the PIN pad over the screen.
    pub modal_focus: usize,
    /// Each home shelf's focused tile, and the shelf.
    pub home_cols: Vec<usize>,
    pub home_row: usize,
    pub games_tab: usize,
    pub games_focus: usize,
    pub episode_focus: usize,
    pub grownups: grownups::State,
    pub modal: Option<Modal>,
    pub toast: Option<(String, ui::Note, Instant)>,
    /// Grown-up mode, and its last use.
    pub grownup: Option<Instant>,
    pub game: Option<play::Game>,
    pub page: Option<play::Page>,
    pub film: Option<play::Film>,
    pub gate: Gate,
    /// Warnings given today: 1 at five minutes, 2 at one.
    pub warned: u8,
    pub last_tick: Option<Instant>,
    /// Seconds counted toward today and not yet whole.
    pub counted: f64,
    pub dirty_usage: bool,
    pub usage_saved: Option<Instant>,
    pub local: Local,
    pub art: Rc<HashMap<PathBuf, Image>>,
    pub art_rev: u64,
    pub art_asked: HashSet<PathBuf>,
    pub sender: Option<MsgSender<Msg>>,
    pub pairing: Option<grownups::Pairing>,
    pub setup: Option<grownups::PadSetUp>,
    pub site: Option<(WindowKey, String, String)>,
    /// The one-instance socket, served once the program runs.
    pub instance: Option<std::os::unix::net::UnixListener>,
    /// The stage id the keyboard was last given to.
    pub stage_asked: &'static str,
}

impl Chanoma {
    pub fn new(config: Config) -> Chanoma {
        let mut settings: Settings = crate::store::read(&config.dirs.settings());
        if !settings.setup_done {
            default_folders(&mut settings);
        }
        let mut usage: Usage = crate::store::read(&config.dirs.state.join("usage.json"));
        let lockout: Lockout = crate::store::read(&config.dirs.state.join("lockout.json"));
        let local = config.clock.local();
        usage.roll(&local);
        let recent = Recent::load(&config.dirs);
        let layouts = state::load_layouts(&config.dirs);
        let screen = if settings.setup_done && settings.pin.is_some() { Screen::Home } else { Screen::Welcome };
        Chanoma {
            settings,
            usage,
            lockout,
            recent,
            layouts,
            library: Arc::new(Library::default()),
            scanned: false,
            pads: None,
            shared: Arc::new(Shared::default()),
            controllers: Vec::new(),
            family: Family::Generic,
            window: WindowKey::default(),
            hud: None,
            lines: 1080,
            screen,
            focus: Focus::default(),
            modal_focus: 0,
            home_cols: Vec::new(),
            home_row: 0,
            games_tab: 0,
            games_focus: 0,
            episode_focus: 0,
            grownups: grownups::State::default(),
            modal: None,
            toast: None,
            grownup: None,
            game: None,
            page: None,
            film: None,
            gate: Gate::Open { left: None },
            warned: 0,
            last_tick: None,
            counted: 0.0,
            dirty_usage: false,
            usage_saved: None,
            local,
            art: Rc::new(HashMap::new()),
            art_rev: 0,
            art_asked: HashSet::new(),
            sender: None,
            pairing: None,
            setup: None,
            site: None,
            instance: None,
            stage_asked: "",
            config,
        }
    }

    pub fn is_grownup(&self) -> bool {
        self.grownup.is_some()
    }

    /// Unix seconds now.
    pub fn now_secs(&self) -> u64 {
        self.config.clock.now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
    }

    pub fn save_settings(&self) {
        if let Err(e) = crate::store::write(&self.config.dirs.settings(), &self.settings) {
            log::warn!("chanoma: keeping the settings: {e}");
        }
    }

    pub fn save_lockout(&self) {
        let _ = crate::store::write(&self.config.dirs.state.join("lockout.json"), &self.lockout);
    }

    /// The button that says yes on a controller of `family`.
    pub fn ok_button(&self, family: Family) -> Button {
        match (self.settings.confirm, family) {
            (Confirm::Right, _) | (Confirm::Auto, Family::Nintendo) => Button::East,
            _ => Button::South,
        }
    }

    pub fn back_button(&self, family: Family) -> Button {
        if self.ok_button(family) == Button::East { Button::South } else { Button::East }
    }

    fn family_of(&self, id: u32) -> Family {
        self.controllers.iter().find(|p| p.id == id).map_or(Family::Generic, |p| p.family)
    }

    /// Show `screen`, focus at its start.
    pub fn go(&mut self, screen: Screen, cx: &mut Cx<Msg>) {
        if screen != self.screen {
            log::info!("chanoma: {screen:?}");
        }
        self.screen = screen;
        self.focus = Focus::default();
        match &self.screen {
            Screen::Home => {
                self.focus = Focus { row: self.home_row, col: self.home_cols.get(self.home_row).copied().unwrap_or(0) };
                self.clamp_home_focus();
            }
            Screen::Games => self.focus = Focus { row: 0, col: self.games_focus },
            Screen::Episodes(_) => self.focus = Focus { row: 0, col: self.episode_focus },
            Screen::GrownUps => self.focus = Focus { row: self.grownups.section, col: 0 },
            _ => {}
        }
        self.stage_asked = "";
        self.refocus(cx);
        self.want_art(cx);
        self.sync_hud(cx);
    }

    /// Something happened that a grown-up did: grown-up mode stays.
    fn touched(&mut self) {
        if self.grownup.is_some() {
            self.grownup = Some(Instant::now());
        }
    }

    pub fn lock(&mut self, cx: &mut Cx<Msg>) {
        self.grownup = None;
        self.toast_note(ui::Note::Kids, "Kids' mode", cx);
        if matches!(self.screen, Screen::GrownUps | Screen::Pairing | Screen::SetUpPad) {
            self.leave_grownups(cx);
            self.go(Screen::Home, cx);
        }
        self.check_gate(cx);
    }

    pub fn unlock(&mut self, cx: &mut Cx<Msg>) {
        self.grownup = Some(Instant::now());
        self.toast_note(ui::Note::GrownUp, "Grown-up mode", cx);
        let _ = cx;
    }

    pub fn toast(&mut self, text: impl Into<String>, cx: &mut Cx<Msg>) {
        self.toast_note(ui::Note::Time, text, cx);
    }

    pub fn toast_note(&mut self, note: ui::Note, text: impl Into<String>, cx: &mut Cx<Msg>) {
        self.toast = Some((text.into(), note, Instant::now()));
        self.sync_hud(cx);
    }

    /// Ask for the pictures the screen shows and those next to them.
    pub fn want_art(&mut self, cx: &mut Cx<Msg>) {
        let paths = self.wanted_art();
        let height = (f64::from(self.lines) / 1080.0 * 330.0).round() as u32;
        // Kept while near the screen: a library of hundreds of games
        // scrolled through holds only those around where it is.
        if self.art.len() > ART_KEPT {
            let near: HashSet<&PathBuf> = paths.iter().collect();
            Rc::make_mut(&mut self.art).retain(|k, _| near.contains(k));
            self.art_asked.retain(|k| near.contains(k));
            self.art_rev += 1;
        }
        for p in paths {
            if self.art.contains_key(&p) || !self.art_asked.insert(p.clone()) {
                continue;
            }
            let path = p.clone();
            cx.spawn(move || Msg::Art(path.clone(), load_picture(&path, height.max(160))));
        }
    }

    /// Open, close or redraw the overlay over a page in the browser.
    pub fn sync_hud(&mut self, cx: &mut Cx<Msg>) {
        let wanted = self.screen == Screen::Watching
            && self.page.as_ref().is_some_and(|p| p.shown)
            && (matches!(self.modal, Some(Modal::Menu { kind: MenuKind::Page, .. })) || self.toast.is_some());
        match (wanted, self.hud) {
            (true, None) => {
                let mut opts = LayerOptions::new(Layer::Overlay, "chanoma");
                opts.anchors = Anchors::ALL;
                opts.exclusive_zone = -1;
                opts.keyboard = KeyboardInteractivity::None;
                self.hud = Some(cx.open_window(WindowSpec::layer(opts, 0.0, 0.0)));
            }
            (false, Some(w)) => {
                cx.close_window(w);
                self.hud = None;
            }
            _ => {}
        }
        if let Some(w) = self.hud {
            // A toast alone lets the pointer through to the page.
            let menu = matches!(self.modal, Some(Modal::Menu { kind: MenuKind::Page, .. }));
            cx.set_input_region(w, if menu { None } else { Some(Vec::new()) });
        }
    }

    // Input.

    fn pad(&mut self, e: PadEvent, cx: &mut Cx<Msg>) {
        match e {
            PadEvent::Connected(info) => {
                let player_one = info.slot == 0;
                self.controllers.retain(|p| p.id != info.id);
                self.family = info.family;
                self.controllers.push(info);
                self.controllers.sort_by_key(|p| p.slot);
                // Player 1 back: the game carries on by itself.
                if player_one && self.modal_is(MenuKind::NoController) {
                    self.close_modal(cx);
                }
                if let Some(p) = self.pairing.as_mut() {
                    p.connected += 1;
                }
            }
            PadEvent::Disconnected { id, slot } => {
                self.controllers.retain(|p| p.id != id);
                // A game whose player 1 went: wait for them.
                if self.screen == Screen::Playing && slot == 0 && self.modal.is_none() {
                    self.open_menu(MenuKind::NoController, cx);
                }
            }
            PadEvent::Battery { id, battery } => {
                if let Some(p) = self.controllers.iter_mut().find(|p| p.id == id) {
                    p.battery = battery;
                }
            }
            PadEvent::Raw { id, input } => self.setup_raw(id, input, cx),
            PadEvent::Nav { id, dir, .. } => {
                if self.setup.as_ref().is_some_and(|s| s.pad == id) {
                    return;
                }
                self.family = self.family_of(id);
                match self.screen {
                    // The game and the page take the arrows themselves.
                    Screen::Playing | Screen::Watching | Screen::Film if self.modal.is_none() => {
                        if self.screen == Screen::Film {
                            self.input(Input::Dir(dir), cx);
                        }
                    }
                    _ => self.input(Input::Dir(dir), cx),
                }
            }
            PadEvent::Button { id, button, pressed, .. } => {
                if self.setup.as_ref().is_some_and(|s| s.pad == id) {
                    return;
                }
                let family = self.family_of(id);
                self.family = family;
                if self.screen == Screen::Watching && self.modal.is_none() && button != Button::Guide {
                    self.page_button(button, family, pressed);
                    return;
                }
                if !pressed {
                    return;
                }
                let input = if button == self.ok_button(family) {
                    Some(Input::Ok)
                } else if button == self.back_button(family) {
                    Some(Input::Back)
                } else {
                    match button {
                        Button::Guide => Some(Input::Home),
                        Button::Start => Some(Input::Menu),
                        Button::L1 => Some(Input::Prev),
                        Button::R1 => Some(Input::Next),
                        _ => None,
                    }
                };
                match (self.screen.clone(), input) {
                    // A game takes every button but Home.
                    (Screen::Playing, Some(Input::Home)) if self.modal.is_none() => self.open_menu(MenuKind::Game, cx),
                    (Screen::Playing, _) if self.modal.is_none() => {}
                    (_, Some(i)) => self.input(i, cx),
                    _ => {}
                }
            }
        }
    }

    fn modal_is(&self, kind: MenuKind) -> bool {
        matches!(&self.modal, Some(Modal::Menu { kind: k, .. }) if *k == kind)
    }

    /// An input for whatever has the focus.
    pub fn input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        self.touched();
        // Backspace is Back but on the PIN pad.
        let i = if i == Input::Erase && !matches!(self.modal, Some(Modal::Pin { .. })) { Input::Back } else { i };
        if self.site.is_some() {
            return;
        }
        if self.modal.is_some() {
            return self.modal_input(i, cx);
        }
        if i == Input::Home {
            return self.home_button(cx);
        }
        match self.screen.clone() {
            Screen::Welcome => self.welcome_input(i, cx),
            Screen::Home => self.home_input(i, cx),
            Screen::Games => self.games_input(i, cx),
            Screen::Episodes(s) => self.episodes_input(s, i, cx),
            Screen::Starting(_) => {}
            Screen::Playing => {}
            Screen::Watching => {}
            Screen::Film => self.film_input(i, cx),
            Screen::GrownUps => self.grownups_input(i, cx),
            Screen::Closed => self.closed_input(i, cx),
            Screen::Pairing => self.pairing_input(i, cx),
            Screen::SetUpPad => self.setup_input(i, cx),
            Screen::Problem(..) => {
                if matches!(i, Input::Ok | Input::Back) {
                    self.go(Screen::Home, cx);
                }
            }
        }
    }

    /// Home: from anywhere, the way back.
    fn home_button(&mut self, cx: &mut Cx<Msg>) {
        match self.screen {
            Screen::Playing => self.open_menu(MenuKind::Game, cx),
            Screen::Watching => self.open_menu(MenuKind::Page, cx),
            Screen::Film => self.open_menu(MenuKind::Film, cx),
            Screen::SetUpPad => self.cancel_setup(cx),
            Screen::Pairing => self.stop_pairing(cx),
            Screen::GrownUps => {
                self.leave_grownups(cx);
                self.go(Screen::Home, cx);
            }
            // A game starting is a moment: it would only come up over Home.
            Screen::Welcome | Screen::Closed | Screen::Starting(_) => {}
            _ => {
                self.home_row = 0;
                self.go(Screen::Home, cx);
            }
        }
    }

    pub fn open_menu(&mut self, kind: MenuKind, cx: &mut Cx<Msg>) {
        use ui::menu::Menu;
        let menu = match kind {
            MenuKind::Game => {
                let title = self.game.as_ref().map_or_else(String::new, |g| g.title.clone());
                self.pause_game(true);
                Menu::new(title, &["Keep playing", "Start again", "Stop playing"]).with_subtitle("Paused")
            }
            MenuKind::Film => {
                let title = self.film.as_ref().map_or_else(String::new, |f| f.title.clone());
                Menu::new(title, &["Keep watching", "Stop watching"])
            }
            MenuKind::Page => {
                let title = self.page.as_ref().map_or_else(String::new, |p| p.name.clone());
                if let Some(p) = &self.page {
                    p.browser.bridge.release_all();
                }
                Menu::new(title, &["Keep watching", "Go home"])
            }
            MenuKind::Time => Menu::new(
                "More time for the kids?",
                &["15 more minutes", "30 more minutes", "An hour more", "Carry on as a grown-up"],
            ),
            MenuKind::NoController => {
                self.pause_game(true);
                Menu::new("Where did your controller go?", &["Keep playing", "Stop playing"])
                    .with_subtitle("Turn it on again to carry on")
            }
        };
        log::info!("chanoma: the {kind:?} menu");
        self.modal = Some(Modal::Menu { menu, kind });
        self.modal_focus = 0;
        self.sync_hud(cx);
    }

    fn close_modal(&mut self, cx: &mut Cx<Msg>) {
        let was = self.modal.take();
        if let Some(Modal::Menu { kind: MenuKind::Game | MenuKind::NoController, .. }) = was {
            self.pause_game(false);
        }
        if self.screen == Screen::Watching
            && let Some(p) = &self.page
        {
            p.browser.focus();
        }
        self.sync_hud(cx);
    }

    fn modal_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        match self.modal.clone() {
            Some(Modal::Pin { .. }) => self.pin_input(i, cx),
            Some(Modal::Menu { menu, kind }) => {
                let n = menu.items.len();
                match i {
                    Input::Dir(Dir::Up) => self.modal_focus = self.modal_focus.saturating_sub(1),
                    Input::Dir(Dir::Down) => self.modal_focus = (self.modal_focus + 1).min(n.saturating_sub(1)),
                    // Not deciding about time is deciding none: kids' mode again.
                    Input::Back if kind == MenuKind::Time => {
                        self.close_modal(cx);
                        self.grownup = None;
                    }
                    Input::Back | Input::Home => self.close_modal(cx),
                    Input::Ok => {
                        let choice = self.modal_focus;
                        self.menu_chosen(kind, choice, cx);
                    }
                    _ => {}
                }
            }
            None => {}
        }
    }

    fn menu_chosen(&mut self, kind: MenuKind, choice: usize, cx: &mut Cx<Msg>) {
        match (kind, choice) {
            (MenuKind::Game | MenuKind::NoController, 0) => self.close_modal(cx),
            (MenuKind::Game, 1) => {
                self.modal = None;
                if let Some(g) = &self.game {
                    g.session.reset();
                }
                self.pause_game(false);
            }
            (MenuKind::Game, _) | (MenuKind::NoController, _) => {
                self.modal = None;
                self.stop_game(true, cx);
            }
            (MenuKind::Film, 0) | (MenuKind::Page, 0) => self.close_modal(cx),
            (MenuKind::Film, _) => {
                self.modal = None;
                self.stop_film(cx);
            }
            (MenuKind::Page, _) => {
                self.modal = None;
                self.close_page(cx);
            }
            (MenuKind::Time, 3) => {
                self.modal = None;
                self.unlock(cx);
                self.go(Screen::Home, cx);
            }
            (MenuKind::Time, c) => {
                let minutes = [15, 30, 60][c.min(2)];
                let now = self.now_secs();
                crate::rules::grant(&self.settings.limits, &mut self.usage, &self.local, now, minutes);
                self.save_usage();
                self.modal = None;
                self.grownup = None;
                self.warned = 0;
                self.check_gate(cx);
                self.go(Screen::Home, cx);
                self.toast(format!("{minutes} more minutes"), cx);
            }
        }
    }

    // The PIN.

    pub fn ask_pin(&mut self, after: AfterPin, cx: &mut Cx<Msg>) {
        self.modal = Some(Modal::Pin { pin: PinState::default(), after });
        self.modal_focus = 0;
        self.sync_hud(cx);
    }

    fn ask_new_pin(&mut self, cx: &mut Cx<Msg>) {
        self.modal = Some(Modal::Pin { pin: PinState { new: true, ..PinState::default() }, after: AfterPin::Settings });
        self.modal_focus = 0;
        let _ = cx;
    }

    fn pin_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let Some(Modal::Pin { pin, .. }) = &self.modal else { return };
        if pin.checking || self.lockout.wait(self.now_secs()).is_some() {
            if i == Input::Back {
                self.close_modal(cx);
            }
            return;
        }
        let key = match i {
            Input::Dir(d) => {
                self.modal_focus = ui::pinpad::step(self.modal_focus, d);
                return;
            }
            Input::Ok => ui::pinpad::KEYS[self.modal_focus.min(11)],
            Input::Digit(d) => {
                // Typed on a keyboard: Return then says OK.
                self.modal_focus = 11;
                ui::pinpad::Key::Digit(d)
            }
            Input::Erase => ui::pinpad::Key::Erase,
            Input::Back => {
                let empty = pin.typed.is_empty();
                if empty {
                    if self.screen == Screen::Welcome || (pin.new && !self.settings.setup_done) {
                        self.modal = None;
                    } else {
                        self.close_modal(cx);
                    }
                    return;
                }
                ui::pinpad::Key::Erase
            }
            _ => return,
        };
        self.pin_key(key, cx);
    }

    pub fn pin_key(&mut self, key: ui::pinpad::Key, cx: &mut Cx<Msg>) {
        let stored_len = self.pin_length();
        let Some(Modal::Pin { pin, .. }) = &mut self.modal else { return };
        pin.error = None;
        match key {
            ui::pinpad::Key::Digit(d) => {
                if pin.typed.len() < 8 {
                    pin.typed.push(char::from(b'0' + d));
                }
            }
            ui::pinpad::Key::Erase => {
                pin.typed.pop();
            }
            ui::pinpad::Key::Done => {}
        }
        let done = key == ui::pinpad::Key::Done || (!pin.new && stored_len == Some(pin.typed.len()));
        if !done {
            return;
        }
        if pin.new {
            let typed = pin.typed.clone();
            if !crate::rules::valid_pin(&typed) {
                pin.error = Some("Four to eight digits".into());
                return;
            }
            match pin.first.clone() {
                None => {
                    pin.first = Some(typed);
                    pin.typed.clear();
                }
                Some(first) if first == typed => {
                    pin.checking = true;
                    let rounds = self.config.pin_rounds;
                    cx.spawn(move || Msg::PinMade(crate::rules::PinHash::new(&typed, rounds)));
                }
                Some(_) => {
                    // From the start: which of the two was meant isn't known.
                    pin.first = None;
                    pin.typed.clear();
                    pin.error = Some("They didn't match. Try again".into());
                }
            }
            return;
        }
        let Some(hash) = self.settings.pin.clone() else { return };
        let typed = std::mem::take(&mut pin.typed);
        pin.checking = true;
        cx.spawn(move || Msg::PinChecked(hash.verify(&typed)));
    }

    /// The PIN's length, where it was kept.
    fn pin_length(&self) -> Option<usize> {
        self.settings.pin.as_ref().and(self.settings.pin_length).map(usize::from)
    }

    fn pin_checked(&mut self, ok: bool, cx: &mut Cx<Msg>) {
        let now = self.now_secs();
        let Some(Modal::Pin { pin, after }) = &mut self.modal else { return };
        pin.checking = false;
        if !ok {
            self.lockout.failed(now);
            pin.error = Some("That's not it".into());
            self.save_lockout();
            return;
        }
        let after = after.clone();
        self.lockout.succeeded();
        self.save_lockout();
        self.modal = None;
        self.unlock(cx);
        match after {
            AfterPin::Unlock => self.close_modal(cx),
            AfterPin::Settings => self.open_grownups(None, cx),
            AfterPin::Time => self.open_menu(MenuKind::Time, cx),
            AfterPin::Leave => {
                self.shutdown();
                cx.quit();
            }
        }
    }

    fn pin_made(&mut self, hash: crate::rules::PinHash, cx: &mut Cx<Msg>) {
        let len = match &self.modal {
            Some(Modal::Pin { pin, .. }) => pin.first.as_ref().map_or(0, String::len).max(4),
            _ => 4,
        };
        let first_run = !self.settings.setup_done;
        self.settings.pin = Some(hash);
        self.settings.pin_length = u8::try_from(len).ok();
        self.save_settings();
        self.modal = None;
        self.grownup = Some(Instant::now());
        if first_run {
            self.open_grownups(Some(0), cx);
        } else {
            self.toast_note(ui::Note::GrownUp, "The PIN is changed", cx);
        }
    }

    // Screens.

    fn welcome_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        if matches!(i, Input::Ok) {
            if self.settings.pin.is_some() {
                // Set up before, but not finished: the PIN opens it.
                self.ask_pin(AfterPin::Settings, cx);
            } else {
                self.ask_new_pin(cx);
            }
        }
    }

    fn closed_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        if i == Input::Ok {
            if self.is_grownup() {
                self.open_menu(MenuKind::Time, cx);
            } else {
                self.ask_pin(AfterPin::Time, cx);
            }
        }
    }

    // Keys and clicks.

    fn key(e: &KeyEvent) -> Option<Input> {
        if !e.pressed {
            return None;
        }
        use aizome_kit::sidestep_ui::Key;
        Some(match &e.key {
            Key::Named(NamedKey::ArrowUp) => Input::Dir(Dir::Up),
            Key::Named(NamedKey::ArrowDown) => Input::Dir(Dir::Down),
            Key::Named(NamedKey::ArrowLeft) => Input::Dir(Dir::Left),
            Key::Named(NamedKey::ArrowRight) => Input::Dir(Dir::Right),
            Key::Named(NamedKey::Enter) => Input::Ok,
            Key::Named(NamedKey::Escape) => Input::Back,
            Key::Named(NamedKey::Backspace) => Input::Erase,
            Key::Named(NamedKey::Home) => Input::Home,
            Key::Named(NamedKey::F(1)) => Input::Home,
            Key::Named(NamedKey::Menu) => Input::Menu,
            Key::Named(NamedKey::PageUp) => Input::Prev,
            Key::Named(NamedKey::PageDown) => Input::Next,
            Key::Character(c) if c == " " => Input::Ok,
            Key::Character(c) if c.len() == 1 && c.as_bytes()[0].is_ascii_digit() => {
                Input::Digit(c.as_bytes()[0] - b'0')
            }
            _ => return None,
        })
    }

    fn click(&mut self, t: Target, cx: &mut Cx<Msg>) {
        self.touched();
        match (t, self.modal.is_some()) {
            (Target::Key(k), true) => {
                self.modal_focus = k;
                self.pin_input(Input::Ok, cx);
            }
            (Target::Item(i), true) => {
                self.modal_focus = i;
                self.modal_input(Input::Ok, cx);
            }
            (_, true) => {}
            (Target::Tile(row, col), false) => {
                self.focus = Focus { row, col };
                self.remember_home_focus();
                self.input(Input::Ok, cx);
            }
            (Target::Item(i), false) => {
                match self.screen {
                    Screen::Games | Screen::Episodes(_) => self.focus.col = i,
                    Screen::GrownUps => self.focus = Focus { row: i, col: 1 },
                    _ => self.focus.row = i,
                }
                if self.screen == Screen::GrownUps {
                    self.grownups.set_focus(self.focus);
                }
                self.input(Input::Ok, cx);
            }
            (Target::Section(i), false) => {
                self.focus = Focus { row: i, col: 0 };
                self.grownups.set_focus(self.focus);
                self.input(Input::Ok, cx);
            }
            (Target::Key(_), false) => {}
        }
    }

    // What's drawn.

    pub fn chrome(&self, hints: Vec<(Button, String)>) -> Chrome {
        let (left, short) = match self.gate {
            _ if self.is_grownup() => (None, false),
            Gate::Open { left: Some(s) } => {
                let mins = s.div_ceil(60);
                let text = if mins >= 120 && mins % 60 == 0 {
                    format!("{} hours left", mins / 60)
                } else if mins >= 90 {
                    format!("{}h {}m left", mins / 60, mins % 60)
                } else if mins == 1 {
                    "1 minute left".into()
                } else {
                    format!("{mins} min left")
                };
                (Some(text), mins <= 10)
            }
            Gate::Open { left: None } => (None, false),
            Gate::TimeUp | Gate::Bedtime { .. } => (Some("No time left".into()), true),
        };
        let banner = if self.controllers.is_empty() && self.config.pads {
            Some("Turn on your controller: press the button in its middle".to_owned())
        } else {
            self.controllers
                .iter()
                .find(|p| p.battery.is_some_and(|(pct, charging)| pct < 15 && !charging))
                .map(|p| format!("Player {}'s controller needs charging soon", p.slot + 1))
        };
        Chrome {
            brand: true,
            clock: self.local.time_label(self.config.h24),
            left,
            short,
            grownup: self.is_grownup(),
            pads: self
                .controllers
                .iter()
                .map(|p| PadBadge { slot: p.slot, family: p.family, battery: p.battery })
                .collect(),
            hints,
            family: self.family,
            ok: self.ok_button(self.family),
            banner,
        }
    }

    /// The header without Chanoma's name, for screens with a title there.
    fn chrome_titled(&self, hints: Vec<(Button, String)>) -> Chrome {
        Chrome { brand: false, ..self.chrome(hints) }
    }

    fn hints(&self, ok: &str, back: Option<&str>) -> Vec<(Button, String)> {
        let mut h = vec![(self.ok_button(self.family), ok.to_owned())];
        if let Some(b) = back {
            h.push((self.back_button(self.family), b.to_owned()));
        }
        h
    }

    fn overlay(&self) -> Option<Overlay> {
        match &self.modal {
            Some(Modal::Pin { pin, .. }) => {
                let (title, prompt) = if pin.new {
                    if pin.first.is_some() {
                        ("Type it again", "The same PIN, to be sure")
                    } else {
                        ("Choose a PIN", "Four to eight digits, for grown-ups only")
                    }
                } else {
                    ("Grown-ups only", "Type the PIN")
                };
                Some(Overlay::Pin(ui::pinpad::Pad {
                    title: title.into(),
                    prompt: prompt.into(),
                    typed: pin.typed.len(),
                    length: if pin.new { None } else { self.pin_length() },
                    error: pin.error.clone(),
                    wait: self.lockout.wait(self.now_secs()),
                    checking: pin.checking,
                }))
            }
            Some(Modal::Menu { menu, .. }) => Some(Overlay::Menu(menu.clone())),
            None => self.toast.as_ref().map(|(t, n, _)| Overlay::Toast(t.clone(), *n)),
        }
    }

    fn scene(&self, view: View, chrome: Option<Chrome>) -> Scene {
        Scene::new(view, chrome, self.overlay(), self.focus, self.modal_focus, self.art.clone(), self.art_rev)
    }

    /// Whether something is drawn over the picture (a game's or a film's).
    fn over_picture(&self) -> bool {
        self.modal.is_some()
            || self.toast.is_some()
            || (self.screen == Screen::Film && self.film.as_ref().is_some_and(|f| f.bar))
    }

    /// The stage canvas's id. Its place in the view changes when a picture
    /// comes or something is drawn over one, and the keyboard has to follow
    /// it there: an id per place, asked for whenever it changes.
    fn stage_id(&self) -> &'static str {
        match self.screen {
            Screen::Playing | Screen::Film if self.over_picture() => "stage-over",
            Screen::Playing | Screen::Film => "stage-video",
            _ => "stage",
        }
    }

    /// Give the keyboard to the stage where it now is.
    pub fn refocus(&mut self, cx: &mut Cx<Msg>) {
        let id = self.stage_id();
        if self.stage_asked != id {
            self.stage_asked = id;
            cx.focus(id);
        }
    }

    fn stage(&self, scene: Scene) -> Element<Msg> {
        canvas(scene, |c, b, s: &Scene, theme| s.paint(c, b, theme))
            .parts(|s: &Scene, size| s.parts(size))
            .on_event(|e, size, s: &Scene| match e {
                WEvent::Key(k) => Chanoma::key(k).map(Msg::Input),
                WEvent::PointerDown { pos, .. } => s.hit(size, *pos).map(Msg::Click),
                _ => None,
            })
            .focusable()
            .focus_ring(FocusRing::Never)
            .id(self.stage_id())
            .into()
    }

    fn main_view(&self) -> Element<Msg> {
        let none = self.hints("Choose", Some("Back"));
        match &self.screen {
            Screen::Playing => {
                let aspect = self.game.as_ref().map_or(4.0 / 3.0, |g| g.aspect);
                let mut layers: Vec<Element<Msg>> = vec![
                    video_surface(GAME).picture(aspect * 1000.0, 1000.0).inhibit_idle(true).ground(Color::BLACK).into(),
                ];
                if self.over_picture() {
                    let chrome = Chrome { family: self.family, ok: self.ok_button(self.family), ..Chrome::default() };
                    layers.push(over_video(self.stage(self.scene(View::Clear, Some(chrome)))));
                } else {
                    layers.push(self.stage(self.scene(View::Clear, None)).width(1.0).height(1.0));
                }
                stack(layers).into()
            }
            Screen::Film => {
                let f = self.film.as_ref();
                let (w, h) = f.and_then(|f| f.display).unwrap_or((16.0, 9.0));
                let playing = f.is_some_and(|f| f.playing);
                let mut layers: Vec<Element<Msg>> =
                    vec![video_surface(FILM).picture(w, h).inhibit_idle(playing).ground(Color::BLACK).into()];
                if self.over_picture() {
                    let chrome = Chrome { family: self.family, ok: self.ok_button(self.family), ..Chrome::default() };
                    let mut scene = self.scene(View::Clear, Some(chrome));
                    if self.modal.is_none()
                        && self.toast.is_none()
                        && let Some(f) = f
                    {
                        scene = self.film_scene(f);
                    }
                    layers.push(over_video(self.stage(scene)));
                } else {
                    layers.push(self.stage(self.scene(View::Clear, None)).width(1.0).height(1.0));
                }
                stack(layers).into()
            }
            Screen::Welcome => {
                let m = ui::screens::Message::new(
                    ui::screens::Art::Mark,
                    "Welcome to Chanoma",
                    "The family TV: shows, films and games, all from a controller. First, a grown-up chooses a PIN and what the kids may do.",
                )
                .actions(&["Set it up"]);
                self.stage(self.scene(View::Message(m), Some(self.chrome_titled(self.hints("Start", None)))))
            }
            Screen::Home => {
                let home = self.home_view();
                let mut hints = self.hints("Open", None);
                if !self.is_grownup() {
                    hints.push((Button::Guide, "Home".into()));
                }
                self.stage(self.scene(View::Home(home), Some(self.chrome(hints))))
            }
            Screen::Games => self.stage(self.scene(View::Grid(self.games_view()), Some(self.chrome_titled(none)))),
            Screen::Episodes(s) => {
                let g = self.episodes_view(*s);
                self.stage(self.scene(View::Grid(g), Some(self.chrome_titled(self.hints("Watch", Some("Back"))))))
            }
            Screen::Starting(what) => {
                let m = ui::screens::Message::new(ui::screens::Art::Waiting, what.clone(), "Just a moment…");
                let mut ch = self.chrome(Vec::new());
                ch.banner = None;
                self.stage(self.scene(View::Message(m), Some(ch)))
            }
            Screen::Watching => {
                let name = self.page.as_ref().map_or_else(String::new, |p| p.name.clone());
                let m = ui::screens::Message::new(
                    ui::screens::Art::Waiting,
                    format!("Opening {name}…"),
                    "Press the Home button to come back here.",
                );
                self.stage(self.scene(View::Message(m), Some(self.chrome(Vec::new()))))
            }
            Screen::GrownUps => {
                let p = self.grownups_page();
                let back = if self.settings.setup_done { "Back" } else { "Done" };
                self.stage(self.scene(View::Settings(p), Some(self.chrome_titled(self.hints("Choose", Some(back))))))
            }
            Screen::Closed => {
                let m = self.closed_message();
                self.stage(self.scene(View::Message(m), Some(self.chrome(self.hints("Ask a grown-up", None)))))
            }
            Screen::Pairing => {
                let m = self.pairing_message();
                self.stage(self.scene(View::Message(m), Some(self.chrome(self.hints("Done", None)))))
            }
            Screen::SetUpPad => {
                let m = self.setup_message();
                let mut ch = self.chrome(Vec::new());
                ch.banner = None;
                self.stage(self.scene(View::Message(m), Some(ch)))
            }
            Screen::Problem(title, body) => {
                let m = ui::screens::Message::new(ui::screens::Art::Oops, title.clone(), body.clone())
                    .actions(&["Back home"]);
                self.stage(self.scene(View::Message(m), Some(self.chrome(self.hints("Choose", None)))))
            }
        }
    }

    fn hud_view(&self) -> Element<Msg> {
        let chrome = Chrome { family: self.family, ok: self.ok_button(self.family), ..Chrome::default() };
        canvas(self.scene(View::Clear, Some(chrome)), |c, b, s: &Scene, theme| s.paint(c, b, theme)).into()
    }

    fn site_view(&self) -> Element<Msg> {
        let (name, url) = self.site.as_ref().map(|(_, n, u)| (n.clone(), u.clone())).unwrap_or_default();
        let ok = !name.trim().is_empty() && crate::services::valid_url(&url);
        aizome_kit::column![
            text("Add a website").size(18.0).semibold(),
            text("It shows on Home under Watch, opens full screen, and keeps its own sign-in.")
                .role(Role::Text2)
                .wrap(),
            text_field(&name).placeholder("Name, as the tile says it").on_change(Msg::SiteName),
            text_field(&url).placeholder("https://…").on_change(Msg::SiteUrl).on_submit(Msg::SiteAdd),
            aizome_kit::row![
                spacer(),
                button("Cancel").cancel().on_press(Msg::SiteCancel),
                button("Add").default().enabled(ok).on_press(Msg::SiteAdd)
            ]
            .spacing(8.0),
        ]
        .spacing(12.0)
        .padding(20.0)
        .width(460.0)
        .into()
    }
}

/// Before the first setup: the usual folders, where they're there (Videos,
/// Games or ROMs in the home folder), so a grown-up needn't choose them
/// with a mouse.
fn default_folders(settings: &mut Settings) {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
    if settings.video_folders.is_empty() {
        settings.video_folders.extend(["Videos"].iter().map(|d| home.join(d)).filter(|p| p.is_dir()));
    }
    if settings.game_folders.is_empty() {
        settings
            .game_folders
            .extend(["Games", "ROMs", "roms"].iter().map(|d| home.join(d)).filter(|p| p.is_dir()).take(1));
    }
}

/// A picture from a file, made no taller than `height` pixels.
pub fn load_picture(path: &std::path::Path, height: u32) -> Option<Image> {
    let img = image::open(path).ok()?;
    let img = if img.height() > height {
        let w = (u64::from(img.width()) * u64::from(height) / u64::from(img.height())).max(1) as u32;
        img.resize(w, height, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Image::from_rgba(rgba.width(), rgba.height(), rgba.as_raw())
}

impl Program for Chanoma {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Chanoma", "TV").icon(Icon::Tv)
    }

    fn menus(&self) -> MenuBar<Msg> {
        // None: a television has no menu bar, and kids no Quit.
        MenuBar::new()
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let spec = WindowSpec::new("Chanoma").size(1280.0, 720.0).min_size(640.0, 360.0).video();
        self.window = cx.open_window(spec);
        if self.config.fullscreen {
            cx.set_fullscreen(self.window, true);
        }
        let tx = cx.stream();
        self.sender = Some(tx.clone());
        if let Some(l) = self.instance.take() {
            crate::instance::serve(l, tx.clone(), Msg::Activate);
        }
        if self.config.signals {
            crate::signals::forward(tx.clone(), Msg::Quit);
        }
        if self.config.pads {
            let database = self.config.database.as_deref().map(Database::load).unwrap_or_default();
            log::info!("chanoma: {} controller layouts from SDL's database", database.len());
            let config = crate::pad::Config {
                dir: PathBuf::from("/dev/input"),
                database,
                layouts: self.layouts.clone(),
                grab: self.config.grab,
            };
            let t = tx.clone();
            let pads = Pads::start(config, move |e| {
                t.send(Msg::Pad(e));
            });
            self.shared = pads.shared.clone();
            self.pads = Some(pads);
        }
        self.outputs_changed(cx);
        if self.config.scan {
            self.rescan(cx);
        }
        cx.every(Duration::from_secs(1), Msg::Tick);
        self.update(Msg::Tick, cx);
        let screen = self.screen.clone();
        self.go(screen, cx);
    }

    fn outputs_changed(&mut self, cx: &mut Cx<Msg>) {
        if let Some(o) = cx.outputs().first() {
            self.lines = (o.frame.height() * o.scale).round().max(360.0) as u32;
        }
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if Some(window) == self.site.as_ref().map(|s| s.0) {
            self.site = None;
            return true;
        }
        if window != self.window || self.is_grownup() || !self.settings.setup_done {
            self.shutdown();
            return true;
        }
        // Kids don't close the television: a grown-up's PIN does.
        self.ask_pin(AfterPin::Leave, cx);
        false
    }

    fn window_state(&mut self, window: WindowKey, state: WindowState, _cx: &mut Cx<Msg>) {
        if window == self.window
            && let Some(f) = &self.film
        {
            f.player.set_visible(!state.suspended);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Pad(e) => self.pad(e, cx),
            Msg::Input(i) => {
                self.family = Family::Keyboard;
                self.input(i, cx);
            }
            Msg::Click(t) => self.click(t, cx),
            Msg::Tick => self.tick(cx),
            Msg::Scanned(lib) => {
                self.library = lib;
                self.scanned = true;
                self.clamp_home_focus();
                self.want_art(cx);
            }
            Msg::Art(path, img) => {
                if let Some(img) = img {
                    Rc::make_mut(&mut self.art).insert(path, img);
                    self.art_rev += 1;
                }
            }
            Msg::PinChecked(ok) => self.pin_checked(ok, cx),
            Msg::PinMade(hash) => self.pin_made(hash, cx),
            Msg::Game(e) => self.game_event(e, cx),
            Msg::Browser(e) => self.browser_event(e, cx),
            Msg::Film(e) => self.film_event(e, cx),
            Msg::Bluetooth => self.bluetooth_changed(cx),
            Msg::Paired(device, r) => self.paired(device, r, cx),
            Msg::Folder(kind, paths) => self.folder_chosen(kind, paths, cx),
            Msg::SiteName(s) => {
                if let Some(site) = &mut self.site {
                    site.1 = s;
                }
            }
            Msg::SiteUrl(s) => {
                if let Some(site) = &mut self.site {
                    site.2 = s;
                }
            }
            Msg::SiteAdd => self.add_site(cx),
            Msg::SiteCancel => {
                if let Some((w, ..)) = self.site.take() {
                    cx.close_window(w);
                }
            }
            Msg::Outputs => self.outputs_changed(cx),
            Msg::Activate => cx.activate(self.window),
            Msg::Quit => {
                self.shutdown();
                cx.quit();
            }
        }
        // Not while a sheet has the keyboard (adding a website).
        if self.site.is_none() {
            self.refocus(cx);
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.hud {
            return self.hud_view();
        }
        if self.site.as_ref().is_some_and(|s| s.0 == window) {
            return self.site_view();
        }
        self.main_view()
    }
}

impl Chanoma {
    /// Put away what's playing before going (Leave, or the window closed).
    pub fn shutdown(&mut self) {
        if let Some(g) = self.game.take() {
            g.session.stop(true);
        }
        if let Some(p) = self.page.take() {
            p.browser.close_and_wait();
        }
        if let Some(f) = self.film.take() {
            let pos = f.player.position();
            self.recent.set_position(&f.path, pos, f.duration);
            self.recent.save(&self.config.dirs);
            f.player.close();
        }
        self.save_usage();
    }

    pub fn save_usage(&mut self) {
        let _ = crate::store::write(&self.config.dirs.state.join("usage.json"), &self.usage);
        self.dirty_usage = false;
        self.usage_saved = Some(Instant::now());
    }

    /// Read the folders again, off the interface thread.
    pub fn rescan(&mut self, cx: &mut Cx<Msg>) {
        let games = self.settings.game_folders.clone();
        let videos = self.settings.video_folders.clone();
        let cores = self.config.core_dirs.clone();
        cx.spawn(move || Msg::Scanned(Arc::new(crate::library::scan(&games, &videos, &cores))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nintendos_controllers_say_yes_on_the_right() {
        let dirs = Dirs::under(&std::env::temp_dir().join(format!("chanoma-unit-{}", std::process::id())));
        let app = Chanoma::new(Config { dirs, scan: false, pads: false, ..Config::standard() });
        assert_eq!(app.ok_button(Family::Xbox), Button::South);
        assert_eq!(app.ok_button(Family::Nintendo), Button::East);
        assert_eq!(app.back_button(Family::Nintendo), Button::South);
    }
}
