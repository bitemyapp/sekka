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

pub mod display;
pub mod drives;
pub mod grownups;
pub mod picker;
pub mod play;
pub mod reset;
pub mod rows;
pub mod saver;
pub mod search;
pub mod splash;
pub mod state;
pub mod time;
pub mod typing;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
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
use crate::rules::{Gate, Lockout, Settings, Usage};
use crate::store::Dirs;
use crate::ui::{self, Activity, Chrome, Focus, Overlay, PadBadge, Scene, Target, View};
use state::Recent;

pub use crate::APP_ID;

/// The window's videos: a game's picture, a film's.
pub const GAME: aizome_kit::video::VideoId = 1;
pub const FILM: aizome_kit::video::VideoId = 2;
/// Pictures kept before those far from the screen go.
const ART_KEPT: usize = 200;
/// Box art fetched at once, at most.
const FETCHING: usize = 4;
/// How long box art isn't fetched after the server couldn't be reached.
const OFFLINE_FOR: Duration = Duration::from_secs(120);
/// Chanoma's films on the desktop's media controls: it opens nothing it's
/// sent (kids see what grown-ups chose), plays at one speed, and leaving
/// takes the PIN.
const MPRIS: aizome_mpris::Identity = aizome_mpris::Identity {
    bus: "chanoma",
    name: "Chanoma",
    desktop_entry: APP_ID,
    uri_schemes: &[],
    mime_types: &[],
    can_quit: false,
    can_raise: true,
    rates: (1.0, 1.0),
};
/// The strip along the top of the screen over a page, in points: thin
/// enough to miss, but the pointer stops there pushed up against the edge.
const EDGE_H: f64 = 2.0;
/// The overlay's canvas, for the keyboard.
const HUD: &str = "hud";
/// How long the bar stays over a game or a film once the pointer stops.
const BAR_SHOWS: Duration = Duration::from_secs(3);
/// And over a page, with the pointer resting in it.
const BAR_SHOWS_OVER_PAGE: Duration = Duration::from_secs(8);
/// Home held this long is a controller being switched off (an Xbox, an
/// 8BitDo or a PlayStation controller switches off so), not Home.
const HOME_HELD: Duration = Duration::from_secs(1);
/// How long what a look through the folders found stays in the header:
/// longer than a toast, which may cover it.
pub const LOOK_SAID: Duration = Duration::from_secs(8);

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
    /// Backspace: Back, but on the PIN pad.
    Erase,
    /// The space bar: OK, but on the PIN pad (the left face button).
    Space,
    /// A digit: on the PIN pad, the legend's button of that number.
    Digit(u8),
    /// The top face button (Y on an Xbox controller, △, Tab): a screen's
    /// other action (using the folder showing).
    Alt,
    /// A letter or a mark typed on a keyboard (searching).
    Typed(char),
}

#[derive(Clone, Debug)]
pub enum Msg {
    Pad(PadEvent),
    Input(Input),
    Click(Target),
    /// The pointer moved over Chanoma: in the bar's band at the top, or
    /// under it.
    Moved {
        in_bar: bool,
    },
    /// The pointer left the overlay over a page.
    Left,
    /// The pointer reached the top of the screen over a page.
    Edge,
    Tick,
    /// What the folders hold: as the files index knew it, or (`true`) read
    /// again.
    Scanned(u64, Arc<Library>, bool),
    /// What a look found has been in the header long enough.
    LookSaid(u64),
    /// Starting has shown long enough ([`splash`]).
    SplashOver,
    /// UDisks has had its time to say of the drives.
    DrivesWaited,
    Art(PathBuf, Option<Image>),
    /// Box art fetched (or not), and its picture if it was.
    ArtFetched(PathBuf, crate::art::Fetched, Option<Image>),
    /// A PIN typed was checked: right or not.
    PinChecked(u64, bool),
    /// A new PIN's hash, made off the interface thread.
    PinMade(u64, u8, crate::rules::PinHash),
    /// What the game of a start (by its number) says.
    Game(u64, crate::retro::Event),
    /// A start (by its number) has gone on a while.
    StartWait(u64),
    Browser(u64, crate::browser::Event),
    Film(aizome_media::Event),
    /// What a film's subtitles show changes now.
    CueTick,
    /// The desktop's media controls (a media key) ask something of the film.
    Mpris(aizome_mpris::Action),
    /// The sound server or BlueZ changed (the grown-ups' settings).
    Services,
    /// What the sound server or BlueZ said to a change asked of it.
    ServiceDone(&'static str, Result<(), String>),
    /// The compositor's displays (Settings › Display).
    Displays(Result<Vec<aizome_ipc::OutputInfo>, String>),
    /// Its answer to a display change, and its displays after.
    DisplayChanged(display::Change, String, Result<Vec<aizome_ipc::OutputInfo>, String>),
    Bluetooth(u64),
    /// BlueZ's answer to switching the pairing screen's adapter on.
    SwitchedOn(u64, Result<(), String>),
    /// BlueZ's answer to looking for controllers with it.
    Searching(u64, Result<(), String>),
    Paired(u64, String, Result<(), String>),
    /// The drives changed, or one finished opening.
    Drives,
    /// Drives finished opening, or couldn't be opened.
    Opened(Vec<aizome_sys::disks::Outcome>),
    /// A folder's folders read, and what's directly in it, for the
    /// picker's reading.
    Listed(u64, Result<Vec<crate::folders::Entry>, String>, Option<crate::folders::Inside>),
    /// What the files index already knew of the folder the picker shows.
    Known(u64, Option<crate::library::Held>),
    /// The folders that look like a collection, for the picker.
    Suggested(Vec<crate::folders::Suggestion>),
    /// The account's password checked, for a forgotten PIN: right or not.
    SignedIn(bool),
    /// How far looking through the folders has got.
    Looking(u64, crate::library::Look),
    /// Looking for games on the drives: where, and how many folders so far.
    Finding(String, usize),
    /// The folders of games found on the drives (`true`: looked again, done).
    GamesFound(Vec<crate::library::FoundFolder>, bool),
    SiteName(String),
    SiteUrl(String),
    SiteAdd,
    SiteCancel,
    Outputs,
    /// Ask the page what has its keyboard (for Chanoma's).
    AskPage,
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
    /// Adding a folder.
    Folders,
    /// Looking for a game, a film or a show by name.
    Search,
    /// Signing in with the computer's account, the PIN forgotten.
    Reset,
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
    /// A new PIN to choose: asked of a grown-up changing it, so that
    /// grown-up mode left on isn't enough to take the PIN over.
    NewPin,
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

impl Modal {
    /// Whether it's a grown-up's to answer, so it goes when grown-up mode
    /// does: a new PIN being chosen (or the PIN asked for first), more
    /// time to give, something to remove. A PIN asked for otherwise stays:
    /// it's how a grown-up comes back.
    fn grown_ups(&self) -> bool {
        match self {
            Modal::Pin { pin, after } => pin.new || *after == AfterPin::NewPin,
            Modal::Menu { kind, .. } => matches!(kind, MenuKind::Time | MenuKind::Remove(_) | MenuKind::KeepDisplay),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// Over a game: keep playing, start again, stop.
    Game,
    /// Over a film.
    Film,
    /// From the film's menu: its subtitles, or its sound's language.
    Subtitles,
    Audio,
    /// Over a page in the browser (on the overlay).
    Page,
    /// Time's up, for a grown-up: give more, or carry on as a grown-up.
    Time,
    /// The controller playing went away.
    NoController,
    /// A grown-up asked to remove something: asked again.
    Remove(Removal),
    /// A game is taking a long time to start: stop it, or keep waiting.
    Slow,
    /// A display's new resolution or refresh rate: keep it, or go back
    /// (by itself, unanswered).
    KeepDisplay,
}

/// What a grown-up may remove from the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    /// One of the folders, by its place in them.
    Folder(usize),
    /// A website added, by its place.
    Site(usize),
    /// The Bluetooth controller being forgotten (`Chanoma::forget`).
    Controller,
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
    /// Where the games' databases are (`$CHANOMA_METADATA`).
    pub metadata: Option<PathBuf>,
    /// Where box art is fetched (`art::THUMBNAILS`; `$CHANOMA_ART_URL`), if
    /// anywhere.
    pub art_url: Option<String>,
    /// SDL's controller database.
    pub database: Option<PathBuf>,
    /// Open full screen.
    pub fullscreen: bool,
    /// Scan the folders at start.
    pub scan: bool,
    /// Play every game with these entry points rather than a core
    /// library (tests).
    pub core_override: Option<crate::retro::ffi::Api>,
    /// Offer cores only the free BIOS images packaged with Chanoma
    /// (`CHANOMA_FREE_BIOS=1`): none kept by the games, and a system folder
    /// of their own, where no console's own BIOS was linked before. Every
    /// system plays as on a new installation.
    pub free_bios: bool,
    /// Stop tidily when asked by a signal (`signals::block` was called).
    pub signals: bool,
    /// The computer's account Chanoma runs as (its login name and name),
    /// and what checks its password, for a forgotten PIN.
    pub account: (String, String),
    pub authenticator: std::sync::Arc<dyn aizome_sys::pam::Authenticator>,
    /// Clocks count 24 hours, as the menu bar's does unless it's set to
    /// twelve.
    pub h24: bool,
    /// Where folders are chosen from: home, and the folders drives
    /// plugged in appear in.
    pub home: PathBuf,
    pub mounts: Vec<PathBuf>,
    /// Films on the desktop's media controls (the session bus).
    pub mpris: bool,
    /// Open the drives plugged in, as Chanoma starts and as they come
    /// (UDisks), so their folders are there without anyone opening them.
    pub drives: bool,
    /// The TV account's Chanoma (`--tv`, `aizome_sys::tv`): its session is
    /// Chanoma alone, so leaving logs out, kids may too, and a forgotten PIN
    /// is forgotten from a grown-up's own account.
    pub tv: bool,
    /// Started again after a crash (`watch`): what happened, shown first.
    pub after_crash: Option<String>,
    /// Talk to the compositor about idleness: tell it of controller
    /// presses, which it can't see, and on the TV account hold the
    /// display awake for the screensaver ([`saver`]).
    pub compositor: bool,
    /// Untouched this long, the screensaver comes.
    pub saver_after: Duration,
    /// Aizome's settings, where the displays chosen in Settings › Display
    /// are kept.
    pub display_settings: PathBuf,
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
            metadata: crate::metadata::dir(),
            art_url: Some(
                std::env::var("CHANOMA_ART_URL")
                    .ok()
                    .filter(|u| !u.is_empty())
                    .unwrap_or_else(|| crate::art::THUMBNAILS.to_owned()),
            ),
            database: Database::standard_path(),
            fullscreen: true,
            scan: true,
            core_override: None,
            free_bios: std::env::var("CHANOMA_FREE_BIOS").is_ok_and(|v| v == "1"),
            signals: false,
            account: aizome_sys::pam::current_user(),
            authenticator: std::sync::Arc::new(aizome_sys::pam::Pam::new("aizome")),
            h24: aizome_config::Settings::load_or_default().menubar_settings().clock == aizome_config::ClockFormat::H24,
            home: std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from),
            mounts: {
                let user = std::env::var("USER").unwrap_or_default();
                vec![
                    PathBuf::from(format!("/run/media/{user}")),
                    PathBuf::from(format!("/media/{user}")),
                    PathBuf::from("/media"),
                    PathBuf::from("/mnt"),
                ]
            },
            mpris: true,
            drives: true,
            tv: false,
            after_crash: None,
            compositor: true,
            saver_after: saver::AFTER,
            display_settings: aizome_config::Settings::path(),
        }
    }
}

pub struct Chanoma {
    pub config: Config,
    /// The settings file is there and couldn't be read: nothing opens, and
    /// nothing is written over it, until a grown-up sets Chanoma up again.
    pub unreadable: bool,
    /// Something the rules stand on (the settings, the wrong PINs' count,
    /// the time played) couldn't be written, and why: a grown-up is told
    /// as soon as one is here ([`Chanoma::say_unsaved`]).
    pub unsaved: Option<std::io::ErrorKind>,
    pub settings: Settings,
    pub usage: Usage,
    pub lockout: Lockout,
    pub recent: Recent,
    pub layouts: HashMap<String, Mapping>,
    pub library: Arc<Library>,
    /// Which of its games show, by place ([`crate::collection`]).
    pub shown: Vec<usize>,
    pub scanned: bool,
    /// Only the latest scan may update the library and its progress.
    pub scan_generation: u64,
    /// Starting, Chanoma's name up until Home has its games.
    pub splash: Option<splash::Splash>,
    /// The screensaver: since when it shows, if it does.
    pub saver: Option<Instant>,
    /// The last press, click or move of anyone's.
    pub last_input: Instant,
    /// When the compositor was last told of a controller's presses.
    pub activity_told: Option<Instant>,
    pub pads: Option<Pads>,
    pub shared: Arc<Shared>,
    pub controllers: Vec<PadInfo>,
    /// The controller used last: whose buttons the hints show.
    pub family: Family,
    /// And which one it was, for the buttons it has.
    pub in_hand: Option<u32>,
    /// The controller whose Home is down, and since when: Home acts as
    /// it's let go.
    pub home_down: Option<(u32, Instant)>,
    /// Each controller's shift on the PIN pad, for the digits a pad
    /// short of buttons presses as chords.
    pub chords: ui::pinpad::Chords,
    pub window: WindowKey,
    /// The overlay over a page in the browser.
    pub hud: Option<WindowKey>,
    /// Whether the overlay has the menu (and the keyboard), as last set.
    pub hud_menu: bool,
    /// The strip along the top of the screen over a page: the pointer
    /// reaching it brings the bar down.
    pub edge: Option<WindowKey>,
    /// Until when the bar shows (the pointer's way home), while it does.
    pub bar_until: Option<Instant>,
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
    /// Identifies the PIN check or hash started by the current pad.
    pin_request: u64,
    pub toast: Option<(String, ui::Note, Instant)>,
    /// Grown-up mode, and its last use.
    pub grownup: Option<Instant>,
    pub game: Option<play::Game>,
    /// Games starting, and one given up on.
    pub starts: play::Starts,
    pub page: Option<play::Page>,
    /// Queued events belong only to the browser that sent them.
    pub page_serial: u64,
    pub film: Option<play::Film>,
    /// Films on the desktop's media controls, which read the playing
    /// film's clock (none between films), count the films played, and
    /// set their volume.
    pub mpris: Option<aizome_mpris::Mpris>,
    pub film_clock: Arc<Mutex<Option<Arc<aizome_media::clock::Clock>>>>,
    pub film_items: u64,
    pub film_volume: f32,
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
    /// Box art being fetched, found missing, and when the server last
    /// couldn't be reached; the agent fetching it.
    pub art_fetching: HashSet<PathBuf>,
    pub art_missing: HashSet<PathBuf>,
    pub art_offline: Option<Instant>,
    pub art_agent: Option<ureq::Agent>,
    pub sender: Option<MsgSender<Msg>>,
    pub pairing: Option<grownups::Pairing>,
    /// The sound server, for the grown-ups' Sound section: connected while
    /// the settings show.
    pub audio: Option<aizome_sys::audio::Audio>,
    /// BlueZ, for the controllers paired: connected while the settings
    /// show.
    pub bt: Option<aizome_sys::bluetooth::Bluetooth>,
    /// The Bluetooth controller asked about forgetting (its path, its name).
    pub forget: Option<(String, String)>,
    /// When the controllers were last asked to sit as players 1, 2, …
    seat_asked: Option<Instant>,
    /// Settings › Display: the compositor's displays, and a change to keep.
    pub displays: display::Displays,
    pairing_session: u64,
    /// Choosing a folder.
    pub picker: Option<picker::Picker>,
    pub search: Option<search::Search>,
    pub reset: Option<reset::Reset>,
    pub setup: Option<grownups::PadSetUp>,
    /// Looking for games on the drives, and what was found.
    pub finding: Option<grownups::Finding>,
    /// How far looking through the folders has got, while it goes.
    pub looking: Option<crate::library::Look>,
    /// The look has gone on long enough to show (an unfinished folder's
    /// read was told of): one the index answers at once isn't shown.
    pub look_shown: bool,
    /// The folders the look shown went through, for what it found.
    pub look_folders: Vec<PathBuf>,
    /// What it found, in the header for a while once it's over (its
    /// number, for the timer that takes it away).
    pub look_said: Option<(u64, Activity)>,
    pub looks_said: u64,
    pub site: Option<(WindowKey, String, String)>,
    /// The one-instance socket, served once the program runs.
    pub instance: Option<std::os::unix::net::UnixListener>,
    /// The stage id the keyboard was last given to.
    pub stage_asked: &'static str,
    /// Asks the page what has its keyboard while Chanoma's is up.
    pub keyboard_watch: Option<aizome_kit::TimerHandle>,
    /// The drives, opened as they come ([`drives`]).
    pub drives: drives::Drives,
}

impl Chanoma {
    pub fn new(config: Config) -> Chanoma {
        // Settings that are there and can't be read aren't a first setup:
        // whoever held the controller would choose the PIN, and the rules
        // kept would be written over. Closed, until a grown-up sets
        // Chanoma up again ([`Chanoma::unreadable_screen`]).
        let (mut settings, unreadable) = match crate::store::read_kept::<Settings>(&config.dirs.settings()) {
            crate::store::Kept::Read(s) => (s, false),
            crate::store::Kept::Missing => (Settings::default(), false),
            crate::store::Kept::Unreadable(why) => {
                log::warn!(
                    "chanoma: {} can't be read ({why}): closed until a grown-up sets Chanoma up again",
                    config.dirs.settings().display()
                );
                (Settings::default(), true)
            }
        };
        settings.merge_folders();
        if !settings.setup_done {
            default_folders(&mut settings);
        }
        let mut usage: Usage = crate::store::read(&config.dirs.state.join("usage.json"));
        let lockout: Lockout = crate::store::read(&config.dirs.state.join("lockout.json"));
        let local = config.clock.local();
        usage.roll(&local);
        let recent = Recent::load(&config.dirs);
        let layouts = state::load_layouts(&config.dirs);
        let screen = match &config.after_crash {
            _ if unreadable => Self::unreadable_screen(config.tv),
            _ if !settings.setup_done || settings.pin.is_none() => Screen::Welcome,
            Some(what) => Screen::Problem("Chanoma started again".into(), what.clone()),
            None => Screen::Home,
        };
        Chanoma {
            unreadable,
            unsaved: None,
            settings,
            usage,
            lockout,
            recent,
            layouts,
            library: Arc::new(Library::default()),
            shown: Vec::new(),
            scanned: false,
            scan_generation: 0,
            splash: None,
            saver: None,
            last_input: Instant::now(),
            activity_told: None,
            pads: None,
            shared: Arc::new(Shared::default()),
            controllers: Vec::new(),
            family: Family::Generic,
            in_hand: None,
            home_down: None,
            chords: ui::pinpad::Chords::default(),
            window: WindowKey::default(),
            hud: None,
            hud_menu: false,
            edge: None,
            bar_until: None,
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
            pin_request: 0,
            toast: None,
            grownup: None,
            game: None,
            starts: play::Starts::default(),
            page: None,
            page_serial: 0,
            film: None,
            mpris: None,
            film_clock: Arc::default(),
            film_items: 0,
            film_volume: 1.0,
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
            art_fetching: HashSet::new(),
            art_missing: HashSet::new(),
            art_offline: None,
            art_agent: None,
            sender: None,
            pairing: None,
            audio: None,
            bt: None,
            forget: None,
            seat_asked: None,
            displays: display::Displays::default(),
            pairing_session: 0,
            picker: None,
            search: None,
            reset: None,
            looking: None,
            look_shown: false,
            look_folders: Vec::new(),
            look_said: None,
            looks_said: 0,
            setup: None,
            finding: None,
            site: None,
            instance: None,
            stage_asked: "",
            keyboard_watch: None,
            drives: drives::Drives::load(&config.dirs),
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

    /// Keep the settings: whether the file has them now. Settings that
    /// couldn't be written still hold until Chanoma closes, and a grown-up
    /// is told they weren't kept ([`Chanoma::say_unsaved`]).
    pub fn save_settings(&mut self) -> bool {
        // Never over settings that couldn't be read: what's here stands in
        // for them only until a grown-up has set Chanoma up again.
        if self.unreadable {
            return false;
        }
        match crate::store::write_kept(&self.config.dirs.settings(), &self.settings) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("chanoma: keeping the settings: {e}");
                self.unsaved = Some(e.kind());
                false
            }
        }
    }

    pub fn save_lockout(&mut self) {
        if let Err(e) = crate::store::write_kept(&self.config.dirs.state.join("lockout.json"), &self.lockout) {
            log::warn!("chanoma: keeping the wrong PINs' count: {e}");
            self.unsaved = Some(e.kind());
        }
    }

    /// Tell a grown-up that something couldn't be written, once one is
    /// here: what changed holds for now, and is gone when Chanoma next
    /// starts. Wrong PINs and time played fail to keep with the kids at
    /// the television, so that waits for grown-up mode.
    fn say_unsaved(&mut self, cx: &mut Cx<Msg>) {
        if !self.is_grownup() {
            return;
        }
        let Some(kind) = self.unsaved.take() else { return };
        let words = match unwritable(kind) {
            Some(why) => format!("Chanoma couldn't save: {why}"),
            None => "Chanoma couldn't save what changed".to_owned(),
        };
        self.toast_note(ui::Note::Drive, words, cx);
    }

    /// What shows while the settings can't be read, and how a grown-up
    /// sets Chanoma up again: with the computer account's password, or on
    /// the TV account (whose password may be nothing) from their own
    /// account's Settings.
    fn unreadable_screen(tv: bool) -> Screen {
        let how = if tv {
            "A grown-up can set Chanoma up again from their own account: in Settings, under Users & Groups, the TV \
             account's Forget Chanoma's PIN. Then Chanoma asks for a new one."
        } else {
            "A grown-up can set Chanoma up again by signing in with the password of this computer's account."
        };
        Screen::Problem(
            "Chanoma's settings can't be read".into(),
            format!("Nothing opens until they're set again. {how}"),
        )
    }

    /// The button that says yes on a controller of `family`.
    pub fn ok_button(&self, family: Family) -> Button {
        self.settings.confirm.ok(family)
    }

    pub fn back_button(&self, family: Family) -> Button {
        self.settings.confirm.back(family)
    }

    fn family_of(&self, id: u32) -> Family {
        self.controllers.iter().find(|p| p.id == id).map_or(Family::Generic, |p| p.family)
    }

    /// The buttons controller `id` has (every one, for one not known).
    fn buttons_of(&self, id: u32) -> u32 {
        self.controllers.iter().find(|p| p.id == id).map_or(Button::all_bits(), |p| p.buttons)
    }

    /// Show `screen`, focus at its start.
    pub fn go(&mut self, screen: Screen, cx: &mut Cx<Msg>) {
        // Search may return to a show's episodes after grown-up mode
        // locked while typing. Recheck the destination, as playback does.
        let screen = match screen {
            // With settings that can't be read, every way leads back to
            // saying so, but a grown-up's sign-in.
            Screen::Reset | Screen::Problem(..) if self.unreadable => screen,
            _ if self.unreadable => Self::unreadable_screen(self.config.tv),
            Screen::Episodes(i)
                if i != usize::MAX
                    && !self.is_grownup()
                    && self.library.shelves.get(i).is_none_or(|s| !self.settings.film_allowed(&s.key)) =>
            {
                Screen::Home
            }
            screen => screen,
        };
        if screen != self.screen {
            log::info!("chanoma: {screen:?}");
        }
        if self.screen == Screen::Pairing && screen != Screen::Pairing {
            self.end_pairing();
        }
        if self.screen == Screen::SetUpPad && screen != Screen::SetUpPad {
            self.setup = None;
            if let Some(pads) = &self.pads {
                pads.raw(false);
            }
        }
        self.screen = screen;
        self.focus = Focus::default();
        self.bar_until = None;
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
        log::info!("chanoma: kids' mode");
        // What a grown-up left over the screen goes too: a kid finding a
        // new PIN's pad still up would choose the PIN.
        if self.modal.as_ref().is_some_and(Modal::grown_ups) {
            self.modal = None;
        }
        if let Some((w, ..)) = self.site.take() {
            cx.close_window(w);
        }
        self.toast_note(ui::Note::Kids, "Kids' mode", cx);
        // An episode list opened by a grown-up may no longer be visible.
        if let Screen::Episodes(i) = self.screen
            && self.library.shelves.get(i).is_some_and(|s| !self.settings.film_allowed(&s.key))
        {
            self.go(Screen::Home, cx);
        }
        if matches!(self.screen, Screen::GrownUps | Screen::Pairing | Screen::SetUpPad | Screen::Folders) {
            self.picker = None;
            self.leave_grownups(cx);
            self.go(Screen::Home, cx);
        }
        self.check_gate(cx);
    }

    pub fn unlock(&mut self, cx: &mut Cx<Msg>) {
        let marks = self.home_marks();
        self.grownup = Some(Instant::now());
        self.restore_home_marks(&marks);
        log::info!("chanoma: grown-up mode");
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
        let fetched = self.config.dirs.art();
        let online = self.art_offline.is_none_or(|t| t.elapsed() >= OFFLINE_FOR);
        for p in paths {
            if self.art.contains_key(&p) || self.art_asked.contains(&p) {
                continue;
            }
            // Box art not fetched yet: fetched first, a few at a time, then
            // shown; the rest wait for the next look at what's wanted.
            if p.starts_with(&fetched) && !p.is_file() {
                let Some(base) = self.config.art_url.clone() else { continue };
                if !online || self.art_fetching.len() >= FETCHING {
                    continue;
                }
                self.art_fetching.insert(p.clone());
                self.art_asked.insert(p.clone());
                let agent = self.art_agent.get_or_insert_with(crate::art::agent).clone();
                let h = height.max(160);
                cx.spawn(move || {
                    use crate::art::Fetched;
                    let how = if crate::art::known_missing(&p) {
                        Fetched::Missing
                    } else {
                        crate::art::fetch(&agent, &base, &p)
                    };
                    let img = (how == Fetched::Found).then(|| load_picture(&p, h)).flatten();
                    Msg::ArtFetched(p, how, img)
                });
                continue;
            }
            self.art_asked.insert(p.clone());
            let path = p.clone();
            cx.spawn(move || Msg::Art(path.clone(), load_picture(&path, height.max(160))));
        }
    }

    /// Open, close or change what's over a page in the browser: the strip
    /// along the top that brings the bar down, while the page shows, and
    /// the overlay with the bar, the menu or a note.
    pub fn sync_hud(&mut self, cx: &mut Cx<Msg>) {
        let page = self.screen == Screen::Watching && self.page.as_ref().is_some_and(|p| p.shown);
        let menu = matches!(self.modal, Some(Modal::Menu { kind: MenuKind::Page, .. }));
        let bar = self.bar_until.is_some() && self.modal.is_none();
        match (page, self.edge) {
            (true, None) => {
                let mut opts = LayerOptions::new(Layer::Overlay, "chanoma-edge");
                opts.anchors = Anchors::TOP | Anchors::LEFT | Anchors::RIGHT;
                opts.exclusive_zone = -1;
                opts.keyboard = KeyboardInteractivity::None;
                self.edge = Some(cx.open_window(WindowSpec::layer(opts, 0.0, EDGE_H)));
            }
            (false, Some(w)) => {
                cx.close_window(w);
                self.edge = None;
            }
            _ => {}
        }
        let typing = self.is_typing();
        let wanted = page && (menu || bar || typing || self.toast.is_some());
        match (wanted, self.hud) {
            (true, None) => {
                let mut opts = LayerOptions::new(Layer::Overlay, "chanoma");
                opts.anchors = Anchors::ALL;
                opts.exclusive_zone = -1;
                opts.keyboard = if menu { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None };
                self.hud = Some(cx.open_window(WindowSpec::layer(opts, 0.0, 0.0)));
                self.hud_menu = menu;
                if menu {
                    cx.focus(HUD);
                }
            }
            (false, Some(w)) => {
                cx.close_window(w);
                self.hud = None;
            }
            _ => {}
        }
        if let Some(w) = self.hud {
            // A note alone lets the pointer through to the page.
            cx.set_input_region(w, if menu || bar || typing { None } else { Some(Vec::new()) });
            // The menu takes the keys from the page while it shows, so a
            // keyboard can answer it.
            if menu != self.hud_menu {
                self.hud_menu = menu;
                let keys = if menu { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None };
                cx.set_layer_keyboard(w, keys);
                if menu {
                    cx.focus(HUD);
                }
            }
        }
    }

    /// The pointer moved over Chanoma. Over a game or a film the bar comes
    /// down for a while; over a page, it stays while the pointer is in it
    /// and goes as the pointer leaves it for the page.
    fn pointer_moved(&mut self, in_bar: bool, cx: &mut Cx<Msg>) {
        if self.modal.is_some() {
            return;
        }
        match self.screen {
            Screen::Playing | Screen::Film => self.bar_until = Some(Instant::now() + BAR_SHOWS),
            Screen::Watching if self.bar_until.is_some() => {
                self.bar_until = in_bar.then(|| Instant::now() + BAR_SHOWS_OVER_PAGE);
                self.sync_hud(cx);
            }
            _ => {}
        }
    }

    /// The pointer reached the top of the screen over a page.
    fn edge_reached(&mut self, cx: &mut Cx<Msg>) {
        if self.screen == Screen::Watching && self.modal.is_none() {
            if self.bar_until.is_none() {
                log::info!("chanoma: the bar over the page");
            }
            self.bar_until = Some(Instant::now() + BAR_SHOWS_OVER_PAGE);
            self.sync_hud(cx);
        }
    }

    /// The bar's time up: it goes, and over a picture the pointer with it
    /// until it moves.
    pub fn bar_tick(&mut self, cx: &mut Cx<Msg>) {
        if self.bar_until.is_some_and(|t| Instant::now() >= t) {
            self.bar_until = None;
            if matches!(self.screen, Screen::Playing | Screen::Film) {
                cx.hide_cursor_until_moved();
            }
            self.sync_hud(cx);
        }
    }

    /// What the bar says: what shows, and the time.
    fn top_bar(&self) -> ui::TopBar {
        let title = match self.screen {
            Screen::Playing => self.game.as_ref().map(|g| g.title.clone()),
            Screen::Film => self.film.as_ref().map(|f| f.title.clone()),
            Screen::Watching => self.page.as_ref().map(|p| p.name.clone()),
            _ => None,
        };
        ui::TopBar { title: title.unwrap_or_default(), clock: self.local.time_label(self.config.h24) }
    }

    /// How the way back is told, for what's in hand.
    fn way_back(&self) -> &'static str {
        if self.controllers.is_empty() {
            "Press Home, or move the pointer to the top, to come back"
        } else {
            "Press Home on the controller to come back"
        }
    }

    // Input.

    fn pad(&mut self, e: PadEvent, cx: &mut Cx<Msg>) {
        match e {
            PadEvent::Connected(info) => {
                let marks = self.home_marks();
                let player_one = info.slot == 0;
                self.controllers.retain(|p| p.id != info.id);
                self.family = info.family;
                self.in_hand = Some(info.id);
                self.controllers.push(info);
                self.controllers.sort_by_key(|p| p.slot);
                // Player 1 back: the game carries on by itself.
                if player_one && self.modal_is(MenuKind::NoController) {
                    self.close_modal(cx);
                }
                if let Some(p) = self.pairing.as_mut() {
                    p.connected += 1;
                }
                self.restore_home_marks(&marks);
            }
            PadEvent::Disconnected { id, slot } => {
                let marks = self.home_marks();
                self.controllers.retain(|p| p.id != id);
                self.chords.forget(id);
                // Held as it switched off: never let go.
                if self.home_down.is_some_and(|(pad, _)| pad == id) {
                    self.home_down = None;
                }
                // Its page goes with it.
                if self.grownups.sub == Some(grownups::Sub::Pad(id)) && self.setup.is_none() {
                    self.grownups.sub = None;
                    self.grownups.row = self.grownups.back_to;
                    self.grownups.first = 0;
                }
                if self.setup.as_ref().is_some_and(|s| s.pad == id) {
                    self.cancel_setup(cx);
                    self.toast_note(ui::Note::Controller, "Controller disconnected. Its layout was kept.", cx);
                }
                self.restore_home_marks(&marks);
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
            PadEvent::Seated { id, slot } => {
                if let Some(p) = self.controllers.iter_mut().find(|p| p.id == id) {
                    p.slot = slot;
                }
                self.controllers.sort_by_key(|p| p.slot);
                self.seat_asked = None;
            }
            PadEvent::Raw { id, input } => self.setup_raw(id, input, cx),
            PadEvent::Nav { id, dir, .. } => {
                // The pointer goes while the controller's in hand, as on a console.
                cx.hide_cursor_until_moved();
                if self.setup.as_ref().is_some_and(|s| s.pad == id) {
                    return;
                }
                self.family = self.family_of(id);
                self.in_hand = Some(id);
                // A PIN's arrows are the direction pad's presses, once
                // each: not the stick, nor a press held.
                if matches!(self.modal, Some(Modal::Pin { .. })) {
                    return;
                }
                match self.screen {
                    // The game and the page take the arrows themselves, but
                    // for Chanoma's keyboard over the page.
                    Screen::Watching if self.modal.is_none() && self.is_typing() => self.keyboard_nav(dir),
                    Screen::Playing | Screen::Watching | Screen::Film if self.modal.is_none() => {
                        if self.screen == Screen::Film {
                            self.input(Input::Dir(dir), cx);
                        }
                    }
                    _ => self.input(Input::Dir(dir), cx),
                }
            }
            PadEvent::Button { id, button, pressed, .. } => {
                if pressed {
                    cx.hide_cursor_until_moved();
                }
                if self.setup.as_ref().is_some_and(|s| s.pad == id) {
                    return;
                }
                let family = self.family_of(id);
                self.family = family;
                self.in_hand = Some(id);
                if self.screen == Screen::Watching && self.modal.is_none() && button != Button::Guide {
                    self.page_button(button, family, pressed, cx);
                    return;
                }
                // The PIN hears Select (shift, on a pad short of buttons)
                // go down and up, as well as presses.
                if matches!(self.modal, Some(Modal::Pin { .. })) {
                    let have = self.buttons_of(id);
                    return match self.chords.button(id, have, button, pressed) {
                        ui::pinpad::Chorded::Digit(b) => self.pin_key(ui::pinpad::Key::Press(b), cx),
                        ui::pinpad::Chorded::Other(b) => self.pin_button(b, family, cx),
                        ui::pinpad::Chorded::Nothing => {}
                    };
                }
                // Home acts as it's let go, after a press: held, it's the
                // controller being switched off, which takes no one home.
                if button == Button::Guide {
                    if pressed {
                        self.home_down = Some((id, Instant::now()));
                        return;
                    }
                    if !self.home_down.take().is_some_and(|(pad, at)| pad == id && at.elapsed() < HOME_HELD) {
                        return;
                    }
                } else if !pressed {
                    return;
                }
                match (self.screen.clone(), self.button_input(button, family)) {
                    // A game takes every button but Home.
                    (Screen::Playing, Some(Input::Home)) if self.modal.is_none() => self.open_menu(MenuKind::Game, cx),
                    (Screen::Playing, _) if self.modal.is_none() => {}
                    (_, Some(i)) => self.input(i, cx),
                    _ => {}
                }
            }
        }
    }

    /// What button `b` of a controller of `family` says to menus (and a
    /// hint clicked, its button).
    fn button_input(&self, b: Button, family: Family) -> Option<Input> {
        if b == self.ok_button(family) {
            return Some(Input::Ok);
        }
        if b == self.back_button(family) {
            return Some(Input::Back);
        }
        match b {
            Button::Guide => Some(Input::Home),
            Button::Start => Some(Input::Menu),
            Button::L1 => Some(Input::Prev),
            Button::R1 => Some(Input::Next),
            Button::North => Some(Input::Alt),
            // Searching, the left face button takes a letter back.
            Button::West if matches!(self.screen, Screen::Search | Screen::Reset) => Some(Input::Erase),
            _ => None,
        }
    }

    fn modal_is(&self, kind: MenuKind) -> bool {
        matches!(&self.modal, Some(Modal::Menu { kind: k, .. }) if *k == kind)
    }

    /// An input for whatever has the focus.
    pub fn input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        self.touched();
        // Backspace is Back and the space bar OK, but on the PIN pad.
        let i = match i {
            _ if matches!(self.modal, Some(Modal::Pin { .. })) => i,
            // Searching, Backspace takes a letter back and the space bar types.
            _ if matches!(self.screen, Screen::Search | Screen::Reset) => i,
            Input::Erase => Input::Back,
            Input::Space => Input::Ok,
            _ => i,
        };
        if let Some(w) = self.site.as_ref().map(|s| s.0) {
            // The sheet's fields are a keyboard's (its Backspace is no way
            // out). A controller's Back or Home puts it away, or a
            // television with no keyboard would be stuck under it.
            if matches!(i, Input::Back | Input::Home) && self.family != Family::Keyboard {
                self.site = None;
                cx.close_window(w);
            }
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
            Screen::Home if self.splash.is_some() => {}
            Screen::Home => self.home_input(i, cx),
            Screen::Games => self.games_input(i, cx),
            Screen::Episodes(s) => self.episodes_input(s, i, cx),
            Screen::Starting(_) if i == Input::Back => self.stop_starting(cx),
            Screen::Starting(_) => {}
            // A controller's buttons are the game's and the page's, so
            // Back here is Esc on the keyboard (or a click): the menu, with
            // the way home.
            Screen::Playing if i == Input::Back => self.open_menu(MenuKind::Game, cx),
            Screen::Watching if i == Input::Back => self.open_menu(MenuKind::Page, cx),
            Screen::Playing | Screen::Watching => {}
            Screen::Film => self.film_input(i, cx),
            Screen::GrownUps => self.grownups_input(i, cx),
            Screen::Closed => self.closed_input(i, cx),
            Screen::Pairing => self.pairing_input(i, cx),
            Screen::SetUpPad => self.setup_input(i, cx),
            Screen::Folders => self.picker_input(i, cx),
            Screen::Search => self.search_input(i, cx),
            Screen::Reset => self.reset_input(i, cx),
            // The settings can't be read: the one way on is a grown-up's
            // sign-in (on the TV account, from their own account instead).
            Screen::Problem(..) if self.unreadable => {
                if i == Input::Ok && !self.config.tv {
                    self.start_reset(cx);
                }
            }
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
            Screen::GrownUps | Screen::Folders => {
                self.picker = None;
                self.leave_grownups(cx);
                self.go(Screen::Home, cx);
            }
            // A game starting is called off, however long it takes.
            Screen::Starting(_) => self.stop_starting(cx),
            Screen::Welcome | Screen::Closed => {}
            _ => {
                self.home_row = 0;
                self.go(Screen::Home, cx);
            }
        }
    }

    pub fn open_menu(&mut self, kind: MenuKind, cx: &mut Cx<Msg>) {
        use ui::menu::Menu;
        // Where the focus starts: the first item, or the track playing.
        let mut focus = 0;
        let menu = match kind {
            MenuKind::Game => {
                let title = self.game.as_ref().map_or_else(String::new, |g| g.title.clone());
                self.pause_game(true);
                Menu::new(title, &["Keep playing", "Start again", "Stop playing"]).with_subtitle("Paused")
            }
            MenuKind::Film => {
                let title = self.film.as_ref().map_or_else(String::new, |f| f.title.clone());
                let (_, words) = self.film_menu();
                Menu::new(title, &words.iter().map(String::as_str).collect::<Vec<_>>())
            }
            MenuKind::Subtitles | MenuKind::Audio => {
                let (words, now) = self.track_choices(kind == MenuKind::Subtitles);
                focus = now;
                let title = if kind == MenuKind::Subtitles { "Subtitles" } else { "Language" };
                Menu::new(title, &words.iter().map(String::as_str).collect::<Vec<_>>())
            }
            MenuKind::Page => {
                let title = self.page.as_ref().map_or_else(String::new, |p| p.name.clone());
                self.close_keyboard(cx);
                if let Some(p) = &mut self.page {
                    p.browser.bridge.release_all();
                    // Select and Start held for Home are let go under the menu.
                    (p.select, p.start) = (None, None);
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
            // Keeping it comes first: a press too many keeps it.
            MenuKind::Remove(Removal::Controller) => {
                let name = self.forget.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
                Menu::new(format!("Forget {name}?"), &["Keep it", "Forget it"])
                    .with_subtitle("To use it again, pair it again")
            }
            MenuKind::Remove(what) => {
                let (name, note) = match what {
                    Removal::Controller => (String::new(), ""),
                    Removal::Folder(i) => {
                        (self.settings.folders.get(i).map(|f| self.said(f).0).unwrap_or_default(), "Files stay put")
                    }
                    Removal::Site(i) => {
                        (self.settings.custom_services.get(i).map(|c| c.name.clone()).unwrap_or_default(), "")
                    }
                };
                Menu::new(format!("Remove {name}?"), &["Keep it", "Remove it from Chanoma"]).with_subtitle(note)
            }
            // A start this long is more likely stuck than slow: stopping
            // comes first. The game's name beside, where it fits.
            MenuKind::Slow => {
                let name = match &self.screen {
                    Screen::Starting(what) => what.clone(),
                    _ => String::new(),
                };
                let title =
                    if self.starts.behind() { "The game before is stuck" } else { "Taking a long time to start" };
                Menu::new(title, &["Stop it now", "Keep waiting"]).with_subtitle(name)
            }
            // Going back comes first: presses on a television gone dark
            // go back.
            MenuKind::KeepDisplay => Menu::new("Keep this display setting?", &["Go back", "Keep it"])
                .with_subtitle(display::back_in(display::KEEP_FOR)),
        };
        log::info!("chanoma: the {kind:?} menu");
        self.modal = Some(Modal::Menu { menu, kind });
        self.modal_focus = focus;
        self.bar_until = None;
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
                    // Not deciding about time is deciding none: kids' mode
                    // again, whichever button put the menu away.
                    Input::Back | Input::Home if kind == MenuKind::Time => {
                        self.close_modal(cx);
                        self.grownup = None;
                    }
                    // From the subtitles or the language, back to the film's menu.
                    Input::Back if matches!(kind, MenuKind::Subtitles | MenuKind::Audio) => {
                        self.open_menu(MenuKind::Film, cx);
                    }
                    // Not keeping a display setting is going back.
                    Input::Back | Input::Home if kind == MenuKind::KeepDisplay => self.display_back(cx),
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
        // A grown-up's menu answers to a grown-up only, however it came to
        // be showing.
        if matches!(kind, MenuKind::Time | MenuKind::Remove(_) | MenuKind::KeepDisplay) && !self.is_grownup() {
            return self.close_modal(cx);
        }
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
            (MenuKind::Page, 0) => self.close_modal(cx),
            (MenuKind::Remove(_), 0) => {
                self.close_modal(cx);
                self.focus = self.grownups.focus();
            }
            (MenuKind::Remove(what), _) => {
                self.close_modal(cx);
                self.remove(what, cx);
            }
            (MenuKind::Slow, 0) => {
                self.modal = None;
                self.stop_starting(cx);
            }
            (MenuKind::Slow, _) => self.close_modal(cx),
            (MenuKind::KeepDisplay, 1) => {
                self.close_modal(cx);
                self.keep_display();
                self.focus = self.grownups.focus();
            }
            (MenuKind::KeepDisplay, _) => {
                self.display_back(cx);
                self.focus = self.grownups.focus();
            }
            (MenuKind::Film, c) => self.film_menu_chosen(c, cx),
            (MenuKind::Subtitles, c) => self.track_chosen(true, c, cx),
            (MenuKind::Audio, c) => self.track_chosen(false, c, cx),
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
        log::info!("chanoma: the PIN pad");
        self.modal = Some(Modal::Pin { pin: PinState::default(), after });
        self.modal_focus = 0;
        self.sync_hud(cx);
    }

    fn ask_new_pin(&mut self, cx: &mut Cx<Msg>) {
        self.modal = Some(Modal::Pin { pin: PinState { new: true, ..PinState::default() }, after: AfterPin::Settings });
        self.modal_focus = 0;
        let _ = cx;
    }

    /// The keyboard on the PIN pad: the legend's numbers (1 to 9 and 0),
    /// or its keys (the arrows, Page Up and Down, Tab and Space);
    /// Backspace, Return, and Esc.
    fn pin_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        use ui::pinpad::Key;
        let key = match i {
            Input::Dir(Dir::Up) => Key::Press(Button::Up),
            Input::Dir(Dir::Down) => Key::Press(Button::Down),
            Input::Dir(Dir::Left) => Key::Press(Button::Left),
            Input::Dir(Dir::Right) => Key::Press(Button::Right),
            Input::Prev => Key::Press(Button::L1),
            Input::Next => Key::Press(Button::R1),
            Input::Alt => Key::Press(Button::North),
            Input::Space => Key::Press(Button::West),
            Input::Digit(d) => match ui::pinpad::button_of(d) {
                Some(b) => Key::Press(b),
                None => return,
            },
            Input::Erase => Key::Delete,
            Input::Ok => Key::Done,
            Input::Menu => Key::Forgot,
            Input::Back => return self.pin_back(cx),
            _ => return,
        };
        self.pin_key(key, cx);
    }

    /// A controller's button on the PIN pad that isn't one of the
    /// legend's ([`ui::pinpad::Chords`] has said which those are): yes is
    /// Done, no is Delete.
    fn pin_button(&mut self, b: Button, family: Family, cx: &mut Cx<Msg>) {
        use ui::pinpad::Key;
        if b == self.ok_button(family) {
            self.pin_key(Key::Done, cx);
        } else if b == self.back_button(family) {
            self.pin_back(cx);
        } else if b == Button::Start {
            self.pin_key(Key::Forgot, cx);
        }
    }

    /// Back on the pad: a press taken back, or with none, the pad put away.
    fn pin_back(&mut self, cx: &mut Cx<Msg>) {
        let locked = self.lockout.wait(self.now_secs()).is_some();
        let Some(Modal::Pin { pin, .. }) = &self.modal else { return };
        if !pin.typed.is_empty() && !pin.checking && !locked {
            return self.pin_key(ui::pinpad::Key::Delete, cx);
        }
        if self.screen == Screen::Welcome || (pin.new && !self.settings.setup_done) {
            self.modal = None;
        } else {
            self.close_modal(cx);
        }
    }

    pub fn pin_key(&mut self, key: ui::pinpad::Key, cx: &mut Cx<Msg>) {
        use ui::pinpad::Key;
        // Forgetting is of a PIN asked for, not one being chosen.
        if key == Key::Forgot {
            let asked = matches!(&self.modal, Some(Modal::Pin { pin, .. }) if !pin.new) && self.settings.pin.is_some();
            if asked {
                self.forgot_pin(cx);
            }
            return;
        }
        // A new PIN is a grown-up's to choose, or anyone's where there's
        // none yet (the first setup, a PIN forgotten).
        if matches!(&self.modal, Some(Modal::Pin { pin, .. }) if pin.new) && !self.may_choose_pin() {
            return self.close_modal(cx);
        }
        let stored_len = self.pin_length();
        let locked = self.lockout.wait(self.now_secs()).is_some();
        let Some(Modal::Pin { pin, .. }) = &mut self.modal else { return };
        if pin.checking || locked {
            return;
        }
        pin.error = None;
        match key {
            Key::Press(b) => {
                if let Some(d) = ui::pinpad::digit(b)
                    && pin.typed.len() < 8
                {
                    pin.typed.push(d);
                }
            }
            Key::Delete => {
                pin.typed.pop();
            }
            Key::Done | Key::Forgot => {}
        }
        let done = match stored_len {
            Some(n) if !pin.new => pin.typed.len() == n,
            _ => key == Key::Done,
        };
        if !done {
            return;
        }
        if pin.new {
            let typed = pin.typed.clone();
            if !crate::rules::valid_pin(&typed) {
                pin.error = Some("Four to eight presses".into());
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
                    self.pin_request += 1;
                    let request = self.pin_request;
                    let len = typed.len() as u8; // valid_pin checked the four-to-eight digits.
                    cx.spawn(move || Msg::PinMade(request, len, crate::rules::PinHash::new(&typed, rounds)));
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
        self.pin_request += 1;
        let request = self.pin_request;
        cx.spawn(move || Msg::PinChecked(request, hash.verify(&typed)));
    }

    /// The PIN's length, where it was kept.
    fn pin_length(&self) -> Option<usize> {
        self.settings.pin.as_ref().and(self.settings.pin_length).map(usize::from)
    }

    /// Whether a new PIN may be chosen now: in grown-up mode, or where
    /// there's no PIN to have asked for.
    fn may_choose_pin(&self) -> bool {
        self.is_grownup() || self.settings.pin.is_none()
    }

    fn pin_checked(&mut self, request: u64, ok: bool, cx: &mut Cx<Msg>) {
        let now = self.now_secs();
        let Some(Modal::Pin { pin, after }) = &mut self.modal else { return };
        if self.pin_request != request || !pin.checking || pin.new {
            return;
        }
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
        // Asked of a grown-up already (changing the PIN), it's theirs still.
        if self.is_grownup() {
            self.touched();
        } else {
            self.unlock(cx);
        }
        match after {
            AfterPin::Unlock => self.close_modal(cx),
            AfterPin::Settings => self.open_grownups(None, cx),
            AfterPin::Time => self.open_menu(MenuKind::Time, cx),
            AfterPin::Leave => {
                self.shutdown();
                cx.quit();
            }
            AfterPin::NewPin => self.ask_new_pin(cx),
        }
    }

    fn pin_made(&mut self, request: u64, len: u8, hash: crate::rules::PinHash, cx: &mut Cx<Msg>) {
        if self.pin_request != request
            || !matches!(&self.modal, Some(Modal::Pin { pin, .. }) if pin.new && pin.checking)
            || !self.may_choose_pin()
        {
            return;
        }
        let first_run = !self.settings.setup_done;
        let had = (self.settings.pin.replace(hash), self.settings.pin_length.replace(len));
        if !self.save_settings() {
            // Not kept: the PIN stays the one the file has (none, the
            // first time), which is the one a restart would bring back.
            // The pad says so and takes the new one again.
            (self.settings.pin, self.settings.pin_length) = had;
            let why = self.unsaved.take().and_then(unwritable);
            if let Some(Modal::Pin { pin, .. }) = &mut self.modal {
                let error = match why {
                    Some(why) => format!("It wasn't saved: {why}"),
                    None => "It couldn't be saved. Try again".to_owned(),
                };
                *pin = PinState { new: true, error: Some(error), ..PinState::default() };
            }
            self.sync_hud(cx);
            return;
        }
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
            Key::Named(NamedKey::Tab) => Input::Alt,
            Key::Character(c) if c == " " => Input::Space,
            Key::Character(c) if c.len() == 1 && c.as_bytes()[0].is_ascii_digit() => {
                Input::Digit(c.as_bytes()[0] - b'0')
            }
            Key::Character(c) if c.chars().count() == 1 => Input::Typed(c.chars().next()?),
            _ => return None,
        })
    }

    fn click(&mut self, t: Target, cx: &mut Cx<Msg>) {
        self.touched();
        match (t, self.modal.is_some()) {
            (Target::Key(k), true) => self.pin_key(ui::pinpad::key_at(k), cx),
            (Target::Item(i), true) => {
                self.modal_focus = i;
                self.modal_input(Input::Ok, cx);
            }
            (Target::Outside, true) => self.modal_input(Input::Back, cx),
            (_, true) => {}
            (Target::Hint(b), false) => {
                if let Some(i) = self.button_input(b, self.family) {
                    self.input(i, cx);
                }
            }
            // A click on a game or a film: its menu, as Home would. On a
            // page, under the bar: the bar goes.
            (Target::Picture, false) => match self.screen {
                Screen::Playing | Screen::Film => self.input(Input::Home, cx),
                Screen::Watching => {
                    self.bar_until = None;
                    self.sync_hud(cx);
                }
                _ => {}
            },
            (Target::Outside, false) if self.screen == Screen::Search => self.done_typing(cx),
            // Beside the keyboard over a page: it goes.
            (Target::Outside, false) => self.close_keyboard(cx),
            (Target::Section(i), false) if self.screen == Screen::Folders => self.picker_click(Some(i), None, cx),
            // A system's tab.
            (Target::Section(i), false) if self.screen == Screen::Games => {
                self.games_tab = i;
                self.focus.col = 0;
                self.games_focus = 0;
                self.want_art(cx);
            }
            (Target::Item(i), false) if self.screen == Screen::Folders => self.picker_click(None, Some(i), cx),
            (Target::Tile(row, col), false) => {
                self.focus = Focus { row, col };
                self.remember_home_focus();
                self.input(Input::Ok, cx);
            }
            (Target::Item(i), false) => {
                match self.screen {
                    Screen::Games | Screen::Episodes(_) | Screen::Search => self.focus.col = i,
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
            (Target::Key(k), false) if self.screen == Screen::Search => self.search_click_key(k, cx),
            (Target::Key(k), false) if self.screen == Screen::Reset => self.reset_click_key(k, cx),
            (Target::Key(k), false) => self.keyboard_click(k, cx),
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
            activity: self.activity(),
        }
    }

    /// The folders being read, for the header: which, how far through its
    /// own folders (the bar) and how many files so far; then the games
    /// being recognised; then, for a while, what was found.
    fn activity(&self) -> Option<Activity> {
        let Some(look) = self.looking.as_ref().filter(|_| self.look_shown) else {
            return self.look_said.as_ref().map(|(_, a)| a.clone());
        };
        Some(match &look.folder {
            // Every start looks again, for games added since: only what
            // changed is read.
            Some(folder) => Activity {
                label: format!("Checking {} for new games", self.said(folder).0),
                step: look.parts.map(|(done, n)| format!("{done} of {n} folders")).unwrap_or_default(),
                count: format!("{} files", ui::grouped(look.files)),
                progress: look.parts.map(|(done, n)| (done * 1000 / n.max(1)) as u16),
            },
            None => Activity { label: "Recognising the games".into(), progress: Some(1000), ..Activity::default() },
        })
    }

    /// What the look just over found, in the header for a while: the games
    /// new since the last look, and those gone. Without it, a look that
    /// ends unseen (under a toast, on a screen with a title) leaves no word
    /// of whether it ended or stopped.
    fn say_look(&mut self, found: &Library, cx: &mut Cx<Msg>) {
        let before: HashSet<&str> = self.library.games.iter().map(|g| g.key.as_str()).collect();
        let after: HashSet<&str> = found.games.iter().map(|g| g.key.as_str()).collect();
        let (new, gone) = (after.difference(&before).count(), before.difference(&after).count());
        let label = match self.look_folders.as_slice() {
            [one] => format!("Checked {}", self.said(one).0),
            [] => "Checked your folders".into(),
            many => format!("Checked {} folders", many.len()),
        };
        let noun = |n: usize| if n == 1 { "game" } else { "games" };
        let games = |n: usize| format!("{} {}", ui::grouped(n), noun(n));
        let count = match (new, gone) {
            (0, 0) => "Nothing new".to_owned(),
            (n, 0) if before.is_empty() => format!("{} found", games(n)),
            (n, 0) => format!("{} new {}", ui::grouped(n), noun(n)),
            (0, g) => format!("{} gone", games(g)),
            (n, g) => format!("{} new, {} gone", ui::grouped(n), ui::grouped(g)),
        };
        self.looks_said += 1;
        self.look_said =
            Some((self.looks_said, Activity { label, count, progress: Some(1000), ..Activity::default() }));
        cx.after(LOOK_SAID, Msg::LookSaid(self.looks_said));
    }

    /// The header without Chanoma's name, for screens with a title there.
    fn chrome_titled(&self, hints: Vec<(Button, String)>) -> Chrome {
        Chrome { brand: false, ..self.chrome(hints) }
    }

    pub(crate) fn hints(&self, ok: &str, back: Option<&str>) -> Vec<(Button, String)> {
        let mut h = vec![(self.ok_button(self.family), ok.to_owned())];
        if let Some(b) = back {
            h.push((self.back_button(self.family), b.to_owned()));
        }
        h
    }

    pub fn overlay(&self) -> Option<Overlay> {
        match &self.modal {
            Some(Modal::Pin { pin, after }) => {
                let (title, prompt) = if pin.new {
                    if pin.first.is_some() {
                        ("Press it again", "The same buttons, to be sure")
                    } else {
                        ("Choose a PIN", "Four to eight presses of these buttons. Nobody sees which")
                    }
                } else if *after == AfterPin::NewPin {
                    ("The PIN you have", "Press it first, then choose the new one")
                } else {
                    ("Grown-ups only", "Press the PIN's buttons")
                };
                // The legend in the buttons of the controller pressed last
                // (a keyboard's keys if it was a keyboard).
                let family = self.family;
                Some(Overlay::Pin(ui::pinpad::Pad {
                    title: title.into(),
                    prompt: prompt.into(),
                    typed: pin.typed.len(),
                    length: if pin.new { None } else { self.pin_length() },
                    error: pin.error.clone(),
                    wait: self.lockout.wait(self.now_secs()),
                    checking: pin.checking,
                    family,
                    have: self.in_hand.map_or(Button::all_bits(), |id| self.buttons_of(id)),
                    ok: self.ok_button(family),
                    back: self.back_button(family),
                    done: pin.new || self.pin_length().is_none(),
                    forgot: !pin.new && self.settings.pin.is_some(),
                }))
            }
            Some(Modal::Menu { menu, .. }) => Some(Overlay::Menu(menu.clone())),
            None if self.screen == Screen::Watching && self.is_typing() => {
                let t = self.page.as_ref().and_then(|p| p.typing.as_ref())?;
                // The hints in the controller's buttons, though a keyboard
                // was used last (it types into the page, not here).
                let family = match self.family {
                    Family::Keyboard => self.controllers.first().map_or(Family::Generic, |p| p.family),
                    f => f,
                };
                Some(Overlay::Keyboard(ui::keyboard::Keyboard {
                    family,
                    ok: self.ok_button(family),
                    back: self.back_button(family),
                    ..t.keys.clone()
                }))
            }
            None if self.screen == Screen::Search && self.search.as_ref().is_some_and(|s| s.keys.is_some()) => {
                self.search_keyboard().map(Overlay::Keyboard)
            }
            None if self.screen == Screen::Reset => self.reset_keyboard().map(Overlay::Keyboard),
            None if self.bar_until.is_some() => Some(Overlay::Top(self.top_bar())),
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
            || self.bar_until.is_some()
            || (self.screen == Screen::Film && self.film.as_ref().is_some_and(|f| f.bar))
            || (self.screen == Screen::Film && self.film_subtitles().is_some())
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
                // Over a game or a film: the bar comes down.
                WEvent::PointerMove { pos, .. } if matches!(s.view, View::Clear | View::Saver(_)) => {
                    Some(Msg::Moved { in_bar: ui::in_top_bar(size, *pos) })
                }
                _ => None,
            })
            .focusable()
            .focus_ring(FocusRing::Never)
            .id(self.stage_id())
            .into()
    }

    fn main_view(&self) -> Element<Msg> {
        if let Some(s) = self.saver_view() {
            let scene = Scene::new(View::Saver(s), None, None, Focus::default(), 0, self.art.clone(), self.art_rev);
            return self.stage(scene);
        }
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
                    // Nothing drawn, but there: for the keyboard, and for the
                    // pointer, which brings the bar down.
                    layers.push(self.stage(self.scene(View::Clear, None)));
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
                        && self.bar_until.is_none()
                        && let Some(f) = f
                        && f.bar
                    {
                        scene = self.film_scene(f);
                    }
                    let scene = scene.with_subtitles(self.film_subtitles());
                    layers.push(over_video(self.stage(scene)));
                } else {
                    // Nothing drawn, but there: for the keyboard, and for the
                    // pointer, which brings the bar down.
                    layers.push(self.stage(self.scene(View::Clear, None)));
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
            // Starting: the name, and nothing to press.
            Screen::Home if self.splash.is_some() => {
                self.stage(self.scene(View::Message(self.splash_message()), Some(self.chrome_titled(Vec::new()))))
            }
            Screen::Home => {
                let home = self.home_view();
                let mut hints = self.hints("Open", None);
                hints.push((Button::North, "Search".into()));
                if !self.is_grownup() {
                    hints.push((Button::Guide, "Home".into()));
                }
                self.stage(self.scene(View::Home(home), Some(self.chrome(hints))))
            }
            Screen::Games => {
                let mut h = none.clone();
                if self.game_tab_count() > 2 {
                    h.push((Button::R1, "Next system".into()));
                }
                h.push((Button::North, "Search".into()));
                self.stage(self.scene(View::Grid(self.games_view()), Some(self.chrome_titled(h))))
            }
            // The keyboard over the foot: no banner beside it.
            Screen::Search => {
                let mut ch = self.chrome_titled(self.search_hints());
                if self.search.as_ref().is_some_and(|s| s.keys.is_some()) {
                    ch.banner = None;
                }
                self.stage(self.scene(View::Grid(self.search_view()), Some(ch)))
            }
            Screen::Reset => {
                let mut ch = self.chrome_titled(Vec::new());
                ch.banner = None;
                self.stage(self.scene(View::SignIn(self.reset_view()), Some(ch)))
            }
            Screen::Episodes(s) => {
                let g = self.episodes_view(*s);
                let mut hints = self.hints("Watch", Some("Back"));
                hints.push((Button::North, "Search".into()));
                self.stage(self.scene(View::Grid(g), Some(self.chrome_titled(hints))))
            }
            Screen::Starting(what) => {
                let body = match (self.starts.slow(), self.starts.behind()) {
                    (false, _) => "Just a moment…",
                    (true, false) => "It's taking a long time. Back or Home stops it.",
                    (true, true) => {
                        "The game before it is still stuck. If it never lets go, leave Chanoma and open it again."
                    }
                };
                let m = ui::screens::Message::new(ui::screens::Art::Waiting, what.clone(), body);
                let mut ch = self.chrome(vec![(self.back_button(self.family), "Back".to_owned())]);
                ch.banner = None;
                self.stage(self.scene(View::Message(m), Some(ch)))
            }
            Screen::Watching => {
                let name = self.page.as_ref().map_or_else(String::new, |p| p.name.clone());
                let m = ui::screens::Message::new(
                    ui::screens::Art::Waiting,
                    format!("Opening {name}…"),
                    format!("{}.", self.way_back()),
                );
                let hints = vec![(Button::Guide, "Home".to_owned())];
                self.stage(self.scene(View::Message(m), Some(self.chrome(hints))))
            }
            Screen::GrownUps => {
                let p = self.grownups_page();
                self.stage(self.scene(View::Settings(p), Some(self.chrome_titled(self.grownups_hints()))))
            }
            Screen::Closed => {
                let m = self.closed_message();
                self.stage(self.scene(View::Message(m), Some(self.chrome(self.hints("Ask a grown-up", None)))))
            }
            Screen::Folders => {
                let view = View::Picker(self.picker_view());
                self.stage(self.scene(view, Some(self.chrome_titled(self.picker_hints()))))
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
                // Settings that can't be read: the way on is to sign in,
                // and on the TV account there's none from here.
                let actions: &[&str] = match (self.unreadable, self.config.tv) {
                    (true, true) => &[],
                    (true, false) => &["Sign in"],
                    (false, _) => &["Back home"],
                };
                let m = ui::screens::Message::new(ui::screens::Art::Oops, title.clone(), body.clone()).actions(actions);
                // Nothing to choose, no hint that there is.
                let hints = if actions.is_empty() { Vec::new() } else { self.hints("Choose", None) };
                self.stage(self.scene(View::Message(m), Some(self.chrome(hints))))
            }
        }
    }

    /// Over a page: the bar, the menu or a note. It has the keyboard while
    /// the menu shows.
    fn hud_view(&self) -> Element<Msg> {
        let chrome = Chrome { family: self.family, ok: self.ok_button(self.family), ..Chrome::default() };
        canvas(self.scene(View::Clear, Some(chrome)), |c, b, s: &Scene, theme| s.paint(c, b, theme))
            .on_event(|e, size, s: &Scene| match e {
                WEvent::Key(k) => Chanoma::key(k).map(Msg::Input),
                WEvent::PointerDown { pos, .. } => s.hit(size, *pos).map(Msg::Click),
                WEvent::PointerMove { pos, .. } => Some(Msg::Moved { in_bar: ui::in_top_bar(size, *pos) }),
                WEvent::PointerLeave => Some(Msg::Left),
                _ => None,
            })
            .focusable()
            .focus_ring(FocusRing::Never)
            .id(HUD)
            .into()
    }

    /// The strip along the top over a page: nothing to see, but the
    /// pointer reaching it brings the bar down.
    fn edge_view(&self) -> Element<Msg> {
        canvas((), |_, _, _: &(), _| {})
            .on_event(|e, _, _: &()| matches!(e, WEvent::PointerMove { .. }).then_some(Msg::Edge))
            .into()
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

/// Why a file couldn't be written, where a grown-up can do something about
/// it: in the words that follow "Chanoma couldn't save:".
fn unwritable(kind: std::io::ErrorKind) -> Option<&'static str> {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => Some("the disk is full"),
        ErrorKind::ReadOnlyFilesystem => Some("the disk can't be written to"),
        _ => None,
    }
}

/// Before the first setup: the usual folders, where they're there (Games
/// or ROMs, and Videos, in the home folder), so a grown-up needn't choose
/// them.
fn default_folders(settings: &mut Settings) {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
    if settings.folders.is_empty() {
        let games = ["Games", "ROMs", "roms"].iter().map(|d| home.join(d)).find(|p| p.is_dir());
        let videos = Some(home.join("Videos")).filter(|p| p.is_dir());
        settings.folders.extend(games.into_iter().chain(videos));
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
        // The TV account's television starts with no pointer in the middle of it.
        if self.config.tv {
            cx.hide_cursor_until_moved();
        }
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
        if self.config.mpris {
            let (t, clock) = (tx.clone(), self.film_clock.clone());
            let now = move || clock.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(0.0, |c| c.now());
            self.mpris = Some(aizome_mpris::Mpris::start(
                MPRIS,
                move |a| {
                    t.send(Msg::Mpris(a));
                },
                now,
            ));
            self.publish_film();
        }
        if self.config.drives {
            self.drives.watch(tx.clone());
        }
        self.keep_awake();
        self.outputs_changed(cx);
        self.start_splash(cx);
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
        self.handle(msg, cx);
        self.say_unsaved(cx);
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.hud {
            return self.hud_view();
        }
        if Some(window) == self.edge {
            return self.edge_view();
        }
        if self.site.as_ref().is_some_and(|s| s.0 == window) {
            return self.site_view();
        }
        self.main_view()
    }
}

impl Chanoma {
    /// Handle a message. [`Program::update`] does, and then says what
    /// couldn't be saved while it did.
    fn handle(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        if self.input_seen(&msg, cx) {
            return;
        }
        match msg {
            Msg::Pad(e) => self.pad(e, cx),
            Msg::Input(i) => {
                self.family = Family::Keyboard;
                self.input(i, cx);
            }
            Msg::Click(t) => self.click(t, cx),
            Msg::Moved { in_bar } => self.pointer_moved(in_bar, cx),
            Msg::Left => {
                if self.screen == Screen::Watching && self.bar_until.take().is_some() {
                    self.sync_hud(cx);
                }
            }
            Msg::Edge => self.edge_reached(cx),
            Msg::Tick => self.tick(cx),
            Msg::Scanned(generation, lib, fresh) => {
                if generation != self.scan_generation {
                    return;
                }
                if fresh && self.look_shown {
                    self.say_look(&lib, cx);
                }
                self.library = lib;
                if fresh {
                    self.looking = None;
                    self.look_shown = false;
                    self.look_folders.clear();
                }
                self.reshow();
                self.scanned |= fresh;
                self.clamp_home_focus();
                self.want_art(cx);
                if let Some(s) = &mut self.splash {
                    s.scan_due = false;
                }
                self.splash_check();
            }
            Msg::SplashOver => self.splash_over(),
            Msg::DrivesWaited => {
                if let Some(s) = &mut self.splash {
                    s.drives_waited = true;
                }
                self.splash_check();
            }
            Msg::Art(path, img) => {
                if let Some(img) = img {
                    Rc::make_mut(&mut self.art).insert(path, img);
                    self.art_rev += 1;
                }
            }
            Msg::ArtFetched(path, how, img) => {
                use crate::art::Fetched;
                self.art_fetching.remove(&path);
                match how {
                    Fetched::Missing => {
                        self.art_missing.insert(path);
                    }
                    Fetched::Unreachable => {
                        // Asked again once the server may be back.
                        self.art_offline = Some(Instant::now());
                        self.art_asked.remove(&path);
                    }
                    Fetched::Found => {
                        if let Some(img) = img {
                            Rc::make_mut(&mut self.art).insert(path, img);
                        }
                    }
                }
                self.art_rev += 1;
                // The next ones wanted.
                self.want_art(cx);
            }
            Msg::PinChecked(request, ok) => self.pin_checked(request, ok, cx),
            Msg::PinMade(request, len, hash) => self.pin_made(request, len, hash, cx),
            Msg::Game(id, e) => self.game_event(id, e, cx),
            Msg::StartWait(id) => self.start_waited(id, cx),
            Msg::Browser(serial, e) => self.browser_event(serial, e, cx),
            Msg::Film(e) => self.film_event(e, cx),
            Msg::CueTick => self.schedule_cues(cx),
            Msg::Mpris(a) => self.on_mpris(a, cx),
            Msg::Services => {}
            Msg::ServiceDone(what, result) => self.service_done(what, result, cx),
            Msg::Displays(result) => self.displays_listed(result),
            Msg::DisplayChanged(change, name, result) => self.display_changed(change, name, result, cx),
            Msg::Bluetooth(session) => self.bluetooth_changed(session, cx),
            Msg::SwitchedOn(session, r) => self.switched_on(session, r),
            Msg::Searching(session, r) => self.searching(session, r),
            Msg::Paired(session, device, r) => self.paired(session, device, r, cx),
            Msg::Drives => self.drives_changed(cx),
            Msg::Opened(outcomes) => self.opened(outcomes, cx),
            Msg::Listed(reading, listing, inside) => self.listed(reading, listing, inside),
            Msg::Known(reading, known) => self.known(reading, known),
            Msg::Suggested(s) => self.suggested(s, cx),
            Msg::Looking(generation, look) => {
                if generation != self.scan_generation {
                    return;
                }
                self.look_shown |= !look.over;
                if let Some(f) = &look.folder
                    && !self.look_folders.contains(f)
                {
                    self.look_folders.push(f.clone());
                }
                self.looking = Some(look);
            }
            Msg::LookSaid(n) => {
                if self.look_said.as_ref().is_some_and(|(m, _)| *m == n) {
                    self.look_said = None;
                }
            }
            Msg::SignedIn(ok) => self.signed_in(ok, cx),
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
            Msg::Finding(at, folders) => {
                if let Some(f) = &mut self.finding {
                    f.at = at;
                    f.folders = folders;
                }
            }
            Msg::GamesFound(found, done) => {
                if let Some(f) = &mut self.finding {
                    f.found = found;
                    f.running &= !done;
                }
                // Opened before anything was found, the focus was on the
                // page's heading: on the first folder found now.
                if self.grownups.sub == Some(grownups::Sub::Found) {
                    let page = self.grownups_page();
                    if page.rows.get(self.grownups.row).is_none_or(|r| r.heading) {
                        self.grownups.row = page.first_row();
                        self.focus = self.grownups.focus();
                    }
                }
                if done {
                    log::info!(
                        "chanoma: games found in {} folders",
                        self.finding.as_ref().map_or(0, |f| f.found.len())
                    );
                }
            }
            Msg::AskPage => self.ask_page(cx),
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
        // A page goes anywhere only in grown-up mode.
        if let Some(p) = &self.page {
            p.browser.keep_to_sites(!self.is_grownup());
        }
    }

    /// Put away what's playing before going (Leave, or the window closed).
    pub fn shutdown(&mut self) {
        self.end_pairing();
        // One still starting may never come back to be stopped.
        match self.game.take() {
            Some(g) if g.started => g.session.stop(true),
            Some(g) => drop(g.session.abandon()),
            None => {}
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
        if let Err(e) = crate::store::write_kept(&self.config.dirs.state.join("usage.json"), &self.usage) {
            log::warn!("chanoma: keeping today's minutes: {e}");
            self.unsaved = Some(e.kind());
        }
        self.dirty_usage = false;
        self.usage_saved = Some(Instant::now());
    }

    /// Read the folders again, off the interface thread: what the files
    /// index knows at once, then what's there now (only folders that
    /// changed are read again).
    pub fn rescan(&mut self, cx: &mut Cx<Msg>) {
        self.scan_generation += 1;
        let generation = self.scan_generation;
        self.looking = None;
        self.look_shown = false;
        self.look_folders.clear();
        if let Some(s) = &mut self.splash {
            s.scan_due = true;
        }
        let folders = self.settings.folders.clone();
        let cores = self.config.core_dirs.clone();
        let metadata = self.config.metadata.clone();
        let dirs = self.config.dirs.clone();
        let scan = move |index: &mut aizome_index::Index,
                         catalogs: &mut crate::metadata::Catalogs,
                         refresh: bool,
                         progress: crate::library::Progress| {
            Arc::new(crate::library::scan(index, &folders, &cores, metadata.as_deref(), catalogs, refresh, progress))
        };
        match self.sender.clone() {
            Some(tx) => {
                let _ = std::thread::Builder::new().name("chanoma-scan".into()).spawn(move || {
                    let mut index = dirs.open_index();
                    // The databases read once, for both looks.
                    let mut catalogs = crate::metadata::Catalogs::default();
                    tx.send(Msg::Scanned(generation, scan(&mut index, &mut catalogs, false, &|_| {}), false));
                    // How far the look has got, for Home and the settings.
                    let tell = |l: &crate::library::Look| {
                        tx.send(Msg::Looking(generation, l.clone()));
                    };
                    tx.send(Msg::Scanned(generation, scan(&mut index, &mut catalogs, true, &tell), true));
                });
            }
            None => cx.spawn(move || {
                let mut catalogs = crate::metadata::Catalogs::default();
                Msg::Scanned(generation, scan(&mut dirs.open_index(), &mut catalogs, true, &|_| {}), true)
            }),
        }
    }

    /// Work out again which games show ([`crate::collection`]), after the
    /// library or the settings that say so change.
    pub fn reshow(&mut self) {
        let lang = std::env::var("LC_ALL").or_else(|_| std::env::var("LANG")).unwrap_or_default();
        self.shown = crate::collection::shown(&self.library.games, &self.settings, &lang);
    }

    /// The games that show, in order.
    pub fn shown_games(&self) -> impl Iterator<Item = &crate::library::Game> {
        self.shown.iter().filter_map(|i| self.library.games.get(*i))
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

    #[test]
    fn box_art_missing_by_the_databases_name_is_asked_for_by_the_files() {
        let dirs = Dirs::under(&std::env::temp_dir().join(format!("chanoma-unit-art-{}", std::process::id())));
        let art = dirs.art();
        let mut app = Chanoma::new(Config { dirs, scan: false, pads: false, ..Config::standard() });
        let g = crate::library::Game {
            key: "dreamcast/Crazy Taxi (USA).chd".into(),
            title: "Crazy Taxi".into(),
            path: "/games/dreamcast/Crazy Taxi (USA).chd".into(),
            system: crate::library::system("dreamcast").unwrap(),
            art: None,
            info: None,
            known_as: Some("Crazy Taxi (USA) (En,Ja)".into()),
        };
        let by_name = crate::art::cached(&art, "dreamcast", "Crazy Taxi (USA) (En,Ja)");
        let by_file = crate::art::cached(&art, "dreamcast", "Crazy Taxi (USA)");
        assert_eq!(app.box_art(&g), Some(by_name.clone()));
        app.art_missing.insert(by_name);
        assert_eq!(app.box_art(&g), Some(by_file.clone()));
        app.art_missing.insert(by_file);
        assert_eq!(app.box_art(&g), None);
        // One its database doesn't know isn't asked for by its file's name.
        let unknown = crate::library::Game { known_as: None, ..g };
        assert_eq!(app.box_art(&unknown), None);
    }

    #[test]
    fn with_only_free_bios_the_bios_by_the_games_is_left_out() {
        let root = std::env::temp_dir().join(format!("chanoma-unit-free-bios-{}", std::process::id()));
        let games = root.join("Games");
        std::fs::create_dir_all(games.join("psx")).unwrap();
        std::fs::create_dir_all(root.join("bios")).unwrap();
        std::fs::write(root.join("bios/scph5501.bin"), b"a console's own").unwrap();
        let dirs = Dirs::under(&root);
        let mut app = Chanoma::new(Config { dirs: dirs.clone(), scan: false, pads: false, ..Config::standard() });
        app.settings.folders = vec![games.clone()];
        let g = crate::library::Game {
            key: "psx/Test Quest (USA).cue".into(),
            title: "Test Quest".into(),
            path: games.join("psx/Test Quest (USA).cue"),
            system: crate::library::system("psx").unwrap(),
            art: None,
            info: None,
            known_as: None,
        };
        let own = vec![(PathBuf::from("scph5501.bin"), root.join("bios/scph5501.bin"))];
        assert_eq!(app.bios_for(&g), (dirs.system(), own));
        app.config.free_bios = true;
        assert_eq!(app.bios_for(&g), (dirs.free_system(), Vec::new()), "a folder of its own, none of the games'");
        let _ = std::fs::remove_dir_all(&root);
    }
}
