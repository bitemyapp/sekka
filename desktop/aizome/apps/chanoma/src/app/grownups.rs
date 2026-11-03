//! The grown-ups' settings, pairing controllers and setting up their
//! buttons.
//!
//! A big collection of games is managed from Games: folders added by hand
//! or found on the drives (home and every drive plugged in, through the
//! files index), whole systems turned off, variants left out and a game's
//! regions shown as one ([`crate::collection`]), and kids' games chosen a
//! system at a time, all at once or one by one, the shoulder buttons
//! running down the list a letter at a time.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aizome_kit::prelude::*;

use super::{Chanoma, Input, Msg, Screen};
use crate::clock::minute_label;
use crate::pad::mapping::{Input as PadInput, Mapping, Output, Source};
use crate::pad::{Axis, Button, Dir};
use crate::rules::{Bedtime, Confirm, CustomService, NewGames};
use crate::services::CATALOG;
use crate::ui::Focus;
use crate::ui::screens::{Art, Message};
use crate::ui::settings::{Page, Row, Value};

pub const SECTIONS: [&str; 7] = ["Time", "Watch", "Games", "Films and shows", "Controllers", "PIN and lock", "Chanoma"];

/// Each section's icon and the colour of its square, as Settei's side list
/// has them.
const SECTION_ICONS: [(aizome_kit::Icon, aizome_kit::theme::Role); 7] = {
    use aizome_kit::Icon;
    use aizome_kit::theme::Role;
    [
        (Icon::Clock, Role::Warning),
        (Icon::Tv, Role::Danger),
        (Icon::Gamepad2, Role::Violet),
        (Icon::Film, Role::Cyan),
        (Icon::Bluetooth, Role::Accent),
        (Icon::Lock, Role::Success),
        (Icon::Settings, Role::Text3),
    ]
};

/// Where the settings' focus is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub section: usize,
    pub row: usize,
    pub in_list: bool,
    pub first: usize,
    /// A page opened from a row (a system's games), and the row it was
    /// opened from.
    pub sub: Option<Sub>,
    pub back_to: usize,
}

/// A page a section's row opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sub {
    /// Every system with games, each on or off.
    Systems,
    /// One system's games, for kids.
    System(&'static str),
    /// The folders of games found on the drives.
    Found,
}

/// Looking for games on the drives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Finding {
    pub running: bool,
    /// Where it's reading, and how many folders so far.
    pub at: String,
    pub folders: usize,
    pub found: Vec<crate::library::FoundFolder>,
}

impl State {
    pub fn focus(&self) -> Focus {
        if self.in_list { Focus { row: self.row, col: 1 } } else { Focus { row: self.section, col: 0 } }
    }

    pub fn set_focus(&mut self, f: Focus) {
        self.in_list = f.col == 1;
        if self.in_list {
            self.row = f.row;
        } else {
            self.section = f.row;
        }
    }
}

/// Which folders a file panel chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderKind {
    Games,
    Videos,
}

/// What a row does.
#[derive(Clone, Debug, PartialEq)]
enum Act {
    None,
    Weekdays,
    Weekend,
    Bedtime,
    Wake,
    Give(u32),
    Service(&'static str),
    RemoveSite(usize),
    AddSite,
    NewGames,
    Game(String),
    AllowAll,
    Open(Sub),
    FindGames,
    UseFolder(PathBuf),
    SystemOn(&'static str),
    HideVariants,
    OneOfEach,
    Region,
    KidsAll(&'static str),
    KidsNone(&'static str),
    RemoveFolder(FolderKind, usize),
    AddFolder(FolderKind),
    Rescan,
    Pair,
    SetUp(u32),
    Confirm,
    ChangePin,
    Relock,
    Lock,
    Leave,
}

const LIMITS: [Option<u32>; 9] =
    [None, Some(30), Some(45), Some(60), Some(90), Some(120), Some(180), Some(240), Some(0)];
const BEDTIMES: [Option<u16>; 10] = [
    None,
    Some(18 * 60),
    Some(18 * 60 + 30),
    Some(19 * 60),
    Some(19 * 60 + 30),
    Some(20 * 60),
    Some(20 * 60 + 30),
    Some(21 * 60),
    Some(21 * 60 + 30),
    Some(22 * 60),
];
const WAKES: [u16; 8] = [5 * 60 + 30, 6 * 60, 6 * 60 + 30, 7 * 60, 7 * 60 + 30, 8 * 60, 8 * 60 + 30, 9 * 60];
const RELOCKS: [u32; 5] = [5, 10, 15, 30, 60];

fn limit_label(l: Option<u32>) -> String {
    match l {
        None => "No limit".into(),
        Some(0) => "None at all".into(),
        Some(m) if m % 60 == 0 => format!("{} hour{}", m / 60, if m == 60 { "" } else { "s" }),
        Some(90) => "1½ hours".into(),
        Some(m) => format!("{m} minutes"),
    }
}

/// `v` moved one along `options`, wrapping.
fn cycle<T: PartialEq + Copy>(options: &[T], v: T, forward: bool) -> T {
    let i = options.iter().position(|o| *o == v).unwrap_or(0);
    let n = options.len();
    options[if forward { (i + 1) % n } else { (i + n - 1) % n }]
}

/// Pairing controllers: BlueZ, and what's been tried.
pub struct Pairing {
    pub bt: aizome_sys::bluetooth::Bluetooth,
    pub adapter: Option<String>,
    /// Devices pairing was started for.
    pub tried: HashSet<String>,
    /// What each found controller is doing, for the list.
    pub status: Vec<(String, String)>,
    pub connected: usize,
}

/// Setting a controller's buttons up: which it is and what's been pressed.
pub struct PadSetUp {
    pub pad: u32,
    pub guid: String,
    pub name: String,
    pub family: crate::pad::Family,
    pub step: usize,
    pub binds: Vec<(PadInput, Output)>,
    pub since: Instant,
}

/// What each step asks for.
const STEPS: [(&str, Output); 19] = [
    ("the bottom button", Output::Button(Button::South)),
    ("the right button", Output::Button(Button::East)),
    ("the left button", Output::Button(Button::West)),
    ("the top button", Output::Button(Button::North)),
    ("the left shoulder button", Output::Button(Button::L1)),
    ("the right shoulder button", Output::Button(Button::R1)),
    ("the left trigger", Output::Axis(Axis::L2)),
    ("the right trigger", Output::Axis(Axis::R2)),
    ("Select (the left middle button)", Output::Button(Button::Select)),
    ("Start (the right middle button)", Output::Button(Button::Start)),
    ("Home (the button in the middle)", Output::Button(Button::Guide)),
    ("up on the arrows", Output::Button(Button::Up)),
    ("down on the arrows", Output::Button(Button::Down)),
    ("left on the arrows", Output::Button(Button::Left)),
    ("right on the arrows", Output::Button(Button::Right)),
    ("the left stick to the right", Output::Axis(Axis::LeftX)),
    ("the left stick down", Output::Axis(Axis::LeftY)),
    ("the right stick to the right", Output::Axis(Axis::RightX)),
    ("the right stick down", Output::Axis(Axis::RightY)),
];
/// A step with nothing pressed goes by after this (the controller hasn't it).
const STEP_WAIT: Duration = Duration::from_secs(8);

impl Chanoma {
    pub fn open_grownups(&mut self, section: Option<usize>, cx: &mut Cx<Msg>) {
        if let Some(s) = section {
            self.grownups = State { section: s, ..State::default() };
        }
        self.go(Screen::GrownUps, cx);
        self.focus = self.grownups.focus();
    }

    /// Leaving the settings: the first setup is done, and the folders are
    /// read again.
    pub fn leave_grownups(&mut self, cx: &mut Cx<Msg>) {
        let first = !self.settings.setup_done;
        if first {
            self.settings.setup_done = true;
            self.save_settings();
        }
        self.rescan(cx);
        if first {
            // Set up: over to the kids.
            self.grownup = None;
            self.check_gate(cx);
            self.toast_note(crate::ui::Note::Kids, "Ready for the kids", cx);
        }
    }

    fn section_rows(&self, section: usize) -> Vec<(Row, Act)> {
        let s = &self.settings;
        let mut rows = Vec::new();
        match section {
            0 => {
                let today = (self.usage.used / 60) as u32;
                rows.push((Row::heading("Each day"), Act::None));
                rows.push((
                    Row::new("Monday to Friday", Value::Choice(limit_label(s.limits.daily[0])))
                        .detail("Watching and playing"),
                    Act::Weekdays,
                ));
                rows.push((
                    Row::new("Saturday and Sunday", Value::Choice(limit_label(s.limits.daily[5]))),
                    Act::Weekend,
                ));
                rows.push((Row::heading("Bedtime"), Act::None));
                let bed = s.limits.bedtime.map_or("Off".to_owned(), |b| minute_label(b.from, self.config.h24));
                rows.push((Row::new("Bedtime", Value::Choice(bed)).detail("Nothing plays from then"), Act::Bedtime));
                if let Some(b) = s.limits.bedtime {
                    rows.push((
                        Row::new("Wake up", Value::Choice(minute_label(b.until, self.config.h24))).detail("Until then"),
                        Act::Wake,
                    ));
                }
                rows.push((Row::heading("Today"), Act::None));
                rows.push((Row::new("Watched and played", Value::Note(format!("{today} min"))), Act::None));
                for m in [15, 30, 60] {
                    let label =
                        if m == 60 { "An hour more today".to_owned() } else { format!("{m} more minutes today") };
                    rows.push((Row::new(label, Value::Action), Act::Give(m)));
                }
            }
            1 => {
                rows.push((Row::heading("Shows on Home"), Act::None));
                for svc in CATALOG {
                    let on = s.services.iter().any(|x| x == svc.id);
                    let detail = match (svc.kids, svc.account) {
                        (true, false) => "Made for kids, free",
                        (true, true) => "Made for kids; needs an account",
                        (false, false) => "Free; not made for kids",
                        (false, true) => "Needs an account; choose a kids' profile in it",
                    };
                    rows.push((Row::new(svc.name, Value::Switch(on)).detail(detail), Act::Service(svc.id)));
                }
                rows.push((Row::heading("Websites you added"), Act::None));
                for (i, c) in s.custom_services.iter().enumerate() {
                    rows.push((
                        Row::new(format!("Remove {}", c.name), Value::Action).detail(c.url.clone()),
                        Act::RemoveSite(i),
                    ));
                }
                rows.push((Row::new("Add a website…", Value::Action).detail("Needs a keyboard"), Act::AddSite));
            }
            2 => {
                rows.push((Row::heading("Folders"), Act::None));
                for (i, f) in s.game_folders.iter().enumerate() {
                    rows.push((
                        Row::new(f.display().to_string(), Value::Action).detail("Remove this folder"),
                        Act::RemoveFolder(FolderKind::Games, i),
                    ));
                }
                rows.push((
                    Row::new("Add a games folder…", Value::Action)
                        .detail("Sorted into a folder per system (snes, gba…) or by extension"),
                    Act::AddFolder(FolderKind::Games),
                ));
                rows.push((
                    Row::new("Find games on your drives…", Value::Action)
                        .detail("Looks through home and every drive plugged in"),
                    Act::FindGames,
                ));
                rows.push((Row::new("Look again", Value::Action).detail(self.cores_note()), Act::Rescan));
                rows.push((Row::heading("Which games show"), Act::None));
                let systems = self.systems_found();
                let on = systems.iter().filter(|(sys, _)| !s.systems_off.contains(sys.id)).count();
                rows.push((
                    Row::new("Systems", Value::Note(format!("{on} of {}", systems.len())))
                        .detail("Turn a whole system off"),
                    Act::Open(Sub::Systems),
                ));
                rows.push((
                    Row::new("Hide demos, betas and prototypes", Value::Switch(s.hide_variants)),
                    Act::HideVariants,
                ));
                rows.push((
                    Row::new("One of each game", Value::Switch(s.one_of_each))
                        .detail("A game's regions and revisions shown as one"),
                    Act::OneOfEach,
                ));
                if s.one_of_each {
                    rows.push((Row::new("Region first", Value::Choice(s.region.label().into())), Act::Region));
                }
                rows.push((Row::heading("Which games kids see"), Act::None));
                let new = match s.new_games {
                    NewGames::Ask => "Wait for you",
                    NewGames::Allow => "Show at once",
                };
                rows.push((Row::new("New games", Value::Choice(new.into())), Act::NewGames));
                let waiting = self.shown_games().filter(|g| s.game_waiting(&g.key)).count();
                if waiting > 0 {
                    rows.push((Row::new(format!("Allow all {waiting} new games"), Value::Action), Act::AllowAll));
                }
                // A system at a time: thousands of games in one list
                // would be no list at all.
                let mut by_system: Vec<(&'static crate::library::System, usize, usize)> = Vec::new();
                for g in self.shown_games() {
                    let allowed = usize::from(s.game_allowed(&g.key));
                    match by_system.iter_mut().find(|(sys, ..)| sys.id == g.system.id) {
                        Some(e) => {
                            e.1 += 1;
                            e.2 += allowed;
                        }
                        None => by_system.push((g.system, 1, allowed)),
                    }
                }
                by_system.sort_by_key(|(sys, ..)| sys.name);
                for (sys, n, allowed) in by_system {
                    rows.push((
                        Row::new(sys.name, Value::Note(format!("{allowed} of {n}"))).detail("For kids"),
                        Act::Open(Sub::System(sys.id)),
                    ));
                }
            }
            3 => {
                rows.push((Row::heading("Folders"), Act::None));
                for (i, f) in s.video_folders.iter().enumerate() {
                    rows.push((
                        Row::new(f.display().to_string(), Value::Action).detail("Remove this folder"),
                        Act::RemoveFolder(FolderKind::Videos, i),
                    ));
                }
                rows.push((
                    Row::new("Add a videos folder…", Value::Action)
                        .detail("Kids see everything in it: a folder of episodes is a show"),
                    Act::AddFolder(FolderKind::Videos),
                ));
                let n = self.library.shelves.len();
                rows.push((
                    Row::new("Look again", Value::Action).detail(format!("{n} films and shows found")),
                    Act::Rescan,
                ));
            }
            4 => {
                rows.push((Row::heading("Connected"), Act::None));
                if self.controllers.is_empty() {
                    rows.push((Row::new("No controllers are on", Value::None), Act::None));
                }
                for p in &self.controllers {
                    let battery =
                        p.battery.map_or(String::new(), |(b, c)| format!(", {b}%{}", if c { " charging" } else { "" }));
                    let how = match p.source {
                        Source::SetUp => "buttons set up here",
                        Source::Database => "buttons known",
                        Source::Kernel => "buttons as Linux says",
                    };
                    rows.push((
                        Row::new(format!("Player {}: {}", p.slot + 1, p.name), Value::Action).detail(format!(
                            "{}{battery}, {how}. Set up its buttons…",
                            if p.bluetooth { "Bluetooth" } else { "USB" }
                        )),
                        Act::SetUp(p.id),
                    ));
                }
                rows.push((Row::new("Pair a controller…", Value::Action).detail("Bluetooth"), Act::Pair));
                let confirm = match s.confirm {
                    Confirm::Auto => "As printed",
                    Confirm::Bottom => "Bottom button",
                    Confirm::Right => "Right button",
                };
                rows.push((
                    Row::new("The button that says yes", Value::Choice(confirm.into()))
                        .detail("As printed: right on Nintendo's, bottom on others"),
                    Act::Confirm,
                ));
            }
            5 => {
                rows.push((Row::new("Change the PIN…", Value::Action), Act::ChangePin));
                rows.push((
                    Row::new("Lock grown-up mode after", Value::Choice(format!("{} minutes", s.relock_minutes)))
                        .detail("Untouched; never during a film or game"),
                    Act::Relock,
                ));
                rows.push((Row::new("Lock now", Value::Action).detail("Back to kids' mode"), Act::Lock));
            }
            _ => {
                rows.push((Row::new("Leave Chanoma", Value::Action).detail("Back to the desktop"), Act::Leave));
                rows.push((Row::new("Version", Value::Note(env!("CARGO_PKG_VERSION").into())), Act::None));
                // The databases' licence asks for their credit.
                rows.push((
                    Row::new("Games' years, genres and players", Value::Note("libretro-database".into()))
                        .detail("CC BY-SA 4.0, from the libretro project's contributors"),
                    Act::None,
                ));
            }
        }
        rows
    }

    /// The rows showing: a page opened from a row's, else the section's.
    fn list_rows(&self) -> Vec<(Row, Act)> {
        match &self.grownups.sub {
            Some(sub) => self.sub_rows(sub),
            None => self.section_rows(self.grownups.section),
        }
    }

    /// Each system with games found (shown or not), and how many.
    fn systems_found(&self) -> Vec<(&'static crate::library::System, usize)> {
        let mut out: Vec<(&'static crate::library::System, usize)> = Vec::new();
        for g in &self.library.games {
            match out.iter_mut().find(|(s, _)| s.id == g.system.id) {
                Some(e) => e.1 += 1,
                None => out.push((g.system, 1)),
            }
        }
        out.sort_by_key(|(s, _)| s.name);
        out
    }

    fn sub_rows(&self, sub: &Sub) -> Vec<(Row, Act)> {
        let s = &self.settings;
        let games = |n: usize| if n == 1 { "1 game".to_owned() } else { format!("{n} games") };
        let mut rows = Vec::new();
        match sub {
            Sub::Systems => {
                rows.push((Row::heading("Systems with games"), Act::None));
                for (sys, n) in self.systems_found() {
                    rows.push((
                        Row::new(sys.name, Value::Switch(!s.systems_off.contains(sys.id))).detail(games(n)),
                        Act::SystemOn(sys.id),
                    ));
                }
            }
            Sub::System(id) => {
                let here: Vec<&crate::library::Game> = self.shown_games().filter(|g| g.system.id == *id).collect();
                let name = crate::library::SYSTEMS.iter().find(|x| x.id == *id).map_or("", |x| x.name);
                rows.push((Row::heading(name), Act::None));
                rows.push((Row::new(format!("Let kids play all {}", here.len()), Value::Action), Act::KidsAll(id)));
                rows.push((Row::new(format!("Keep all {} from kids", here.len()), Value::Action), Act::KidsNone(id)));
                rows.push((
                    Row::heading(format!("{}, a letter at a time with the shoulder buttons", games(here.len()))),
                    Act::None,
                ));
                for g in here {
                    let stem = g.path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let t = crate::collection::tags(&stem);
                    let mut detail = t.regions.join(", ");
                    if t.revision > 0 {
                        detail.push_str(if detail.is_empty() { "A revision" } else { ", a revision" });
                    }
                    if let Some(known) = g.info.as_ref().map(crate::metadata::Info::summary).filter(|s| !s.is_empty()) {
                        if !detail.is_empty() {
                            detail.push_str(" · ");
                        }
                        detail.push_str(&known);
                    }
                    let mut row = Row::new(g.title.clone(), Value::Switch(s.game_allowed(&g.key))).detail(detail);
                    if s.game_waiting(&g.key) {
                        row = row.badge("New");
                    }
                    rows.push((row, Act::Game(g.key.clone())));
                }
            }
            Sub::Found => {
                let f = self.finding.clone().unwrap_or_default();
                if f.running {
                    let at = if f.at.is_empty() { "your drives".to_owned() } else { f.at.clone() };
                    rows.push((Row::heading(format!("Looking in {at}… {} folders", f.folders)), Act::None));
                }
                let mut place = None;
                for found in &f.found {
                    if place != Some(&found.place) {
                        place = Some(&found.place);
                        rows.push((Row::heading(found.place.clone()), Act::None));
                    }
                    // Where it is, from the drive: its last two folders.
                    let parts: Vec<String> =
                        found.folder.iter().rev().take(2).map(|p| p.to_string_lossy().into_owned()).collect();
                    let at: Vec<&str> = parts.iter().rev().map(String::as_str).collect();
                    let added = s.game_folders.iter().any(|g| g.canonicalize().ok().as_ref() == Some(&found.folder));
                    rows.push((
                        Row::new(found.system.name, Value::Switch(added)).detail(format!(
                            "{} in {}",
                            games(found.games),
                            at.join(" › ")
                        )),
                        Act::UseFolder(found.folder.clone()),
                    ));
                }
                if !f.running && f.found.is_empty() {
                    rows.push((Row::heading("No games found"), Act::None));
                }
            }
        }
        rows
    }

    /// Look for games at home and on every drive plugged in: what the
    /// files index knows at once, then what's there now.
    pub fn find_games(&mut self) {
        if self.finding.as_ref().is_some_and(|f| f.running) {
            return;
        }
        use crate::folders::PlaceKind;
        let places: Vec<(String, PathBuf)> = crate::folders::places(&self.config.home, &self.config.mounts)
            .into_iter()
            .filter(|p| matches!(p.kind, PlaceKind::Home | PlaceKind::Drive))
            .map(|p| (p.name, p.path))
            .collect();
        let found = self.finding.take().map(|f| f.found).unwrap_or_default();
        self.finding = Some(Finding { running: true, found, ..Finding::default() });
        let Some(tx) = self.sender.clone() else { return };
        let dirs = self.config.dirs.clone();
        log::info!("chanoma: looking for games in {} places", places.len());
        let _ = std::thread::Builder::new().name("chanoma-find".into()).spawn(move || {
            let mut index = dirs.open_index();
            let known = crate::library::find_games(&mut index, &places, false, &|_, _| {});
            if !known.is_empty() {
                tx.send(Msg::GamesFound(known, false));
            }
            let fresh = crate::library::find_games(&mut index, &places, true, &|at, n| {
                tx.send(Msg::Finding(at.to_owned(), n));
            });
            tx.send(Msg::GamesFound(fresh, true));
        });
    }

    fn cores_note(&self) -> String {
        let n = self.library.games.len();
        let cores = self.library.cores.found.len();
        let mut s = format!("{n} games found; {cores} game players installed");
        if self.library.unplayable > 0 {
            s.push_str(&format!("; {} games need a player that isn't", self.library.unplayable));
        }
        s
    }

    pub fn grownups_page(&self) -> Page {
        let rows: Vec<Row> = self.list_rows().into_iter().map(|(r, _)| r).collect();
        let title = match &self.grownups.sub {
            Some(Sub::Systems) => "Games › Systems".into(),
            Some(Sub::System(id)) => {
                let name = crate::library::SYSTEMS.iter().find(|x| x.id == *id).map_or("", |x| x.short);
                format!("Games › {name}")
            }
            Some(Sub::Found) => "Games › On your drives".into(),
            None if self.settings.setup_done => "Settings".into(),
            None => "Set up Chanoma".into(),
        };
        let mut p = Page {
            title,
            sections: SECTIONS.iter().map(|s| (*s).to_owned()).collect(),
            icons: SECTION_ICONS.to_vec(),
            section: self.grownups.section,
            rows,
            first: self.grownups.first,
            note: if self.settings.setup_done { String::new() } else { "Back when you're done".into() },
        };
        p.first = p.first_for(self.grownups.row.min(p.rows.len().saturating_sub(1)));
        p
    }

    pub fn grownups_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let page = self.grownups_page();
        if !self.grownups.in_list {
            let st = &mut self.grownups;
            match i {
                Input::Dir(Dir::Up) => st.section = st.section.saturating_sub(1),
                Input::Dir(Dir::Down) => st.section = (st.section + 1).min(SECTIONS.len() - 1),
                Input::Dir(Dir::Right) | Input::Ok => {
                    st.in_list = true;
                    st.row = page.first_row();
                    st.first = 0;
                }
                Input::Back => {
                    self.leave_grownups(cx);
                    self.go(Screen::Home, cx);
                    return;
                }
                _ => {}
            }
            let st = &mut self.grownups;
            if st.section != page.section {
                st.row = 0;
                st.first = 0;
            }
            self.focus = self.grownups.focus();
            return;
        }
        let row = self.grownups.row;
        let rows = self.list_rows();
        let sub = self.grownups.sub.is_some();
        match i {
            Input::Dir(Dir::Up) => {
                let n = page.next(row, false);
                if n != row {
                    self.grownups.row = n;
                } else if !sub {
                    self.grownups.in_list = false;
                }
            }
            Input::Dir(Dir::Down) => self.grownups.row = page.next(row, true),
            // A letter on or back through a long list.
            Input::Prev | Input::Next => self.grownups.row = page.next_letter(row, i == Input::Next),
            // Out of a page opened from a row, back on that row.
            Input::Back if sub => self.close_sub(),
            Input::Back => self.grownups.in_list = false,
            Input::Dir(Dir::Left) | Input::Dir(Dir::Right) => {
                let forward = i == Input::Dir(Dir::Right);
                match rows.get(row).map(|(r, a)| (r.value.clone(), a.clone())) {
                    Some((Value::Choice(_), act)) => self.change(act, forward, cx),
                    _ if !forward && sub => self.close_sub(),
                    _ if !forward => self.grownups.in_list = false,
                    _ => {}
                }
            }
            Input::Ok => {
                if let Some((_, act)) = rows.get(row).cloned() {
                    self.do_row(act, cx);
                }
            }
            _ => {}
        }
        let page = self.grownups_page();
        self.grownups.row = self.grownups.row.min(page.rows.len().saturating_sub(1));
        self.grownups.first = page.first;
        if self.screen == Screen::GrownUps && self.modal.is_none() {
            self.focus = self.grownups.focus();
        }
    }

    /// Back from a page opened from a row, to that row.
    fn close_sub(&mut self) {
        self.grownups.sub = None;
        self.grownups.row = self.grownups.back_to;
        self.grownups.first = 0;
    }

    /// Open a page from the row the focus is on.
    fn open_sub(&mut self, sub: Sub) {
        self.grownups.back_to = self.grownups.row;
        self.grownups.sub = Some(sub);
        self.grownups.first = 0;
        self.grownups.row = self.grownups_page().first_row();
    }

    /// Left or right on a choice.
    fn change(&mut self, act: Act, forward: bool, cx: &mut Cx<Msg>) {
        let l = &mut self.settings.limits;
        match act {
            Act::Region => {
                use crate::collection::Region;
                let all = [Region::Auto, Region::Usa, Region::Europe, Region::Japan];
                self.settings.region = cycle(&all, self.settings.region, forward);
                self.save_settings();
                self.reshow();
                return;
            }
            Act::Weekdays => {
                let v = cycle(&LIMITS, l.daily[0], forward);
                l.daily[..5].fill(v);
            }
            Act::Weekend => {
                let v = cycle(&LIMITS, l.daily[5], forward);
                l.daily[5..].fill(v);
            }
            Act::Bedtime => {
                let v = cycle(&BEDTIMES, l.bedtime.map(|b| b.from), forward);
                l.bedtime = v.map(|from| Bedtime { from, until: l.bedtime.map_or(7 * 60, |b| b.until) });
            }
            Act::Wake => {
                if let Some(b) = &mut l.bedtime {
                    b.until = cycle(&WAKES, b.until, forward);
                }
            }
            Act::NewGames => {
                self.settings.new_games = cycle(&[NewGames::Ask, NewGames::Allow], self.settings.new_games, forward);
            }
            Act::Confirm => {
                self.settings.confirm =
                    cycle(&[Confirm::Auto, Confirm::Bottom, Confirm::Right], self.settings.confirm, forward);
            }
            Act::Relock => self.settings.relock_minutes = cycle(&RELOCKS, self.settings.relock_minutes, forward),
            _ => return,
        }
        self.save_settings();
        self.check_gate(cx);
    }

    fn do_row(&mut self, act: Act, cx: &mut Cx<Msg>) {
        match act {
            Act::Weekdays | Act::Weekend | Act::Bedtime | Act::Wake | Act::NewGames | Act::Confirm | Act::Relock => {
                self.change(act, true, cx);
            }
            Act::Give(m) => {
                let now = self.now_secs();
                crate::rules::grant(&self.settings.limits, &mut self.usage, &self.local, now, m);
                self.save_usage();
                self.warned = 0;
                self.check_gate(cx);
                self.toast(format!("{m} more minutes today"), cx);
            }
            Act::Service(id) => {
                let s = &mut self.settings.services;
                if let Some(i) = s.iter().position(|x| x == id) {
                    s.remove(i);
                } else {
                    s.push(id.to_owned());
                }
                self.save_settings();
            }
            Act::RemoveSite(i) => {
                if i < self.settings.custom_services.len() {
                    self.settings.custom_services.remove(i);
                    self.save_settings();
                }
            }
            Act::AddSite => {
                let w = cx.show_sheet(self.window);
                self.site = Some((w, String::new(), "https://".into()));
            }
            Act::Game(key) => {
                let s = &mut self.settings;
                if s.game_allowed(&key) {
                    s.games_allowed.remove(&key);
                    s.games_hidden.insert(key);
                } else {
                    s.games_hidden.remove(&key);
                    s.games_allowed.insert(key);
                }
                self.save_settings();
            }
            Act::AllowAll => {
                // Those that show: not the variants left out.
                let keys: Vec<String> =
                    self.shown_games().filter(|g| self.settings.game_waiting(&g.key)).map(|g| g.key.clone()).collect();
                self.settings.games_allowed.extend(keys);
                self.save_settings();
            }
            Act::Open(sub) => self.open_sub(sub),
            Act::FindGames => {
                self.find_games();
                self.open_sub(Sub::Found);
            }
            Act::UseFolder(folder) => {
                let s = &mut self.settings;
                match s.game_folders.iter().position(|g| g.canonicalize().ok().as_ref() == Some(&folder)) {
                    Some(i) => {
                        s.game_folders.remove(i);
                    }
                    None => s.game_folders.push(folder),
                }
                self.save_settings();
                self.rescan(cx);
            }
            Act::SystemOn(id) => {
                let off = &mut self.settings.systems_off;
                if !off.remove(id) {
                    off.insert(id.to_owned());
                }
                self.save_settings();
                self.reshow();
            }
            Act::HideVariants => {
                self.settings.hide_variants = !self.settings.hide_variants;
                self.save_settings();
                self.reshow();
            }
            Act::OneOfEach => {
                self.settings.one_of_each = !self.settings.one_of_each;
                self.save_settings();
                self.reshow();
            }
            Act::Region => self.change(act, true, cx),
            Act::KidsAll(id) | Act::KidsNone(id) => {
                let all = matches!(act, Act::KidsAll(_));
                let keys: Vec<String> =
                    self.shown_games().filter(|g| g.system.id == id).map(|g| g.key.clone()).collect();
                let s = &mut self.settings;
                for k in keys {
                    if all {
                        s.games_hidden.remove(&k);
                        s.games_allowed.insert(k);
                    } else {
                        s.games_allowed.remove(&k);
                        s.games_hidden.insert(k);
                    }
                }
                self.save_settings();
            }
            Act::RemoveFolder(kind, i) => {
                let folders = match kind {
                    FolderKind::Games => &mut self.settings.game_folders,
                    FolderKind::Videos => &mut self.settings.video_folders,
                };
                if i < folders.len() {
                    folders.remove(i);
                    self.save_settings();
                    self.rescan(cx);
                }
            }
            Act::AddFolder(kind) => self.open_picker(kind, cx),
            Act::Rescan => {
                self.rescan(cx);
                self.toast("Looking…", cx);
            }
            Act::Pair => self.start_pairing(cx),
            Act::SetUp(id) => self.start_setup(id, cx),
            Act::ChangePin => {
                self.modal = Some(super::Modal::Pin {
                    pin: super::PinState { new: true, ..super::PinState::default() },
                    after: super::AfterPin::Settings,
                });
                self.modal_focus = 0;
            }
            Act::Lock => self.lock(cx),
            Act::Leave => {
                self.leave_grownups(cx);
                self.shutdown();
                cx.quit();
            }
            Act::None => {}
        }
    }

    pub fn folder_chosen(&mut self, kind: FolderKind, dir: PathBuf, cx: &mut Cx<Msg>) {
        let folders = match kind {
            FolderKind::Games => &mut self.settings.game_folders,
            FolderKind::Videos => &mut self.settings.video_folders,
        };
        if !folders.contains(&dir) {
            folders.push(dir);
            self.save_settings();
            self.rescan(cx);
        }
    }

    /// Back from choosing a folder: the focus on *Add a … folder* (the
    /// folder added is the row above it now).
    pub fn settings_on_add(&mut self, kind: FolderKind) {
        let section = match kind {
            FolderKind::Games => 2,
            FolderKind::Videos => 3,
        };
        let row = self.section_rows(section).iter().position(|(_, a)| *a == Act::AddFolder(kind)).unwrap_or(0);
        self.grownups = State { section, row, in_list: true, ..State::default() };
        self.grownups.first = self.grownups_page().first;
        self.focus = self.grownups.focus();
    }

    pub fn add_site(&mut self, cx: &mut Cx<Msg>) {
        let Some((w, name, url)) = self.site.take() else { return };
        let (name, url) = (name.trim().to_owned(), url.trim().to_owned());
        if name.is_empty() || !crate::services::valid_url(&url) {
            self.site = Some((w, name, url));
            return;
        }
        cx.close_window(w);
        self.settings.custom_services.push(CustomService { name, url });
        self.save_settings();
    }

    // Pairing.

    pub fn start_pairing(&mut self, cx: &mut Cx<Msg>) {
        let tx = self.sender.clone();
        let bt = aizome_sys::bluetooth::Bluetooth::new(aizome_sys::Wake::new(move || {
            if let Some(t) = &tx {
                t.send(Msg::Bluetooth);
            }
        }));
        self.pairing = Some(Pairing { bt, adapter: None, tried: HashSet::new(), status: Vec::new(), connected: 0 });
        self.go(Screen::Pairing, cx);
        self.bluetooth_changed(cx);
    }

    pub fn bluetooth_changed(&mut self, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.pairing else { return };
        let state = p.bt.snapshot();
        if p.adapter.is_none()
            && let Some(a) = state.default_adapter()
        {
            p.adapter = Some(a.path.clone());
            if !a.powered {
                drop(p.bt.set_powered(&a.path, true));
            }
            drop(p.bt.start_discovery(&a.path));
        }
        let mut status = Vec::new();
        for d in state.devices.iter().filter(|d| d.kind == aizome_sys::bluetooth::DeviceKind::Gamepad) {
            let name = if d.name.is_empty() { d.address.clone() } else { d.name.clone() };
            let what = if d.connected {
                "connected"
            } else if d.paired {
                "paired: press its middle button"
            } else {
                "pairing…"
            };
            status.push((name, what.to_owned()));
            if !d.paired && p.tried.insert(d.path.clone()) {
                let task = p.bt.pair(&d.path);
                let path = d.path.clone();
                cx.spawn(move || Msg::Paired(path, task.wait().map_err(|e| e.to_string())));
            }
        }
        p.status = status;
    }

    pub fn paired(&mut self, device: String, result: Result<(), String>, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.pairing else { return };
        match result {
            Ok(()) => {
                // Trusted, it reconnects by itself whenever it's switched on.
                drop(p.bt.set_trusted(&device, true));
                let task = p.bt.connect(&device);
                cx.spawn(move || {
                    let _ = task.wait();
                    Msg::Bluetooth
                });
            }
            Err(e) => {
                log::warn!("chanoma: pairing {device}: {e}");
                // Tried again when it shows up next.
                p.tried.remove(&device);
            }
        }
    }

    pub fn pairing_message(&self) -> Message {
        let mut m = Message::new(
            Art::Controller,
            "Pair a controller",
            "Put it in pairing mode: hold Share and the PS button on a PlayStation controller, the pair button on top of an Xbox controller, or the sync button on a Switch Pro. It pairs by itself.",
        )
        .actions(&["Done"]);
        match &self.pairing {
            Some(p) if p.adapter.is_none() => m.lines.push("No Bluetooth here".into()),
            Some(p) if p.status.is_empty() => m.lines.push("Looking…".into()),
            Some(p) => m.lines = p.status.iter().map(|(n, s)| format!("{n}: {s}")).collect(),
            None => {}
        }
        m
    }

    pub fn pairing_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        if matches!(i, Input::Ok | Input::Back) {
            self.stop_pairing(cx);
        }
    }

    pub fn stop_pairing(&mut self, cx: &mut Cx<Msg>) {
        if let Some(p) = self.pairing.take()
            && let Some(a) = &p.adapter
        {
            drop(p.bt.stop_discovery(a));
        }
        self.open_grownups(Some(4), cx);
    }

    // Setting a controller's buttons up.

    pub fn start_setup(&mut self, pad: u32, cx: &mut Cx<Msg>) {
        let Some(info) = self.controllers.iter().find(|p| p.id == pad).cloned() else { return };
        self.setup = Some(PadSetUp {
            pad,
            guid: info.guid.clone(),
            name: info.name.clone(),
            family: info.family,
            step: 0,
            binds: Vec::new(),
            since: Instant::now(),
        });
        if let Some(p) = &self.pads {
            p.raw(true);
        }
        self.go(Screen::SetUpPad, cx);
    }

    pub fn setup_message(&self) -> Message {
        let Some(s) = &self.setup else { return Message::new(Art::Controller, "", "") };
        let (what, out) = STEPS[s.step.min(STEPS.len() - 1)];
        let art = match out {
            Output::Button(b @ (Button::South | Button::East | Button::West | Button::North)) => {
                Art::Press(b, crate::pad::Family::Generic)
            }
            _ => Art::Controller,
        };
        let verb = if matches!(out, Output::Axis(Axis::LeftX | Axis::LeftY | Axis::RightX | Axis::RightY)) {
            "Push"
        } else {
            "Press"
        };
        let mut m = Message::new(
            art,
            format!("{verb} {what}"),
            format!("Setting up {}: {} of {}. Not there? Wait, and it's skipped.", s.name, s.step + 1, STEPS.len()),
        );
        m.actions = vec!["Skip".into(), "Cancel".into()];
        m
    }

    pub fn setup_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        match i {
            Input::Dir(Dir::Left) => self.focus.row = 0,
            Input::Dir(Dir::Right) => self.focus.row = 1,
            Input::Ok if self.focus.row == 0 => self.setup_next(cx),
            Input::Ok | Input::Back => self.cancel_setup(cx),
            _ => {}
        }
    }

    pub fn cancel_setup(&mut self, cx: &mut Cx<Msg>) {
        self.setup = None;
        if let Some(p) = &self.pads {
            p.raw(false);
        }
        self.open_grownups(Some(4), cx);
    }

    fn setup_next(&mut self, cx: &mut Cx<Msg>) {
        let Some(s) = &mut self.setup else { return };
        s.step += 1;
        s.since = Instant::now();
        if s.step < STEPS.len() {
            return;
        }
        let s = self.setup.take().expect("set up above");
        self.layouts.insert(s.guid.clone(), Mapping { binds: s.binds, source: Source::SetUp });
        super::state::save_layouts(&self.config.dirs, &self.layouts);
        if let Some(p) = &self.pads {
            p.raw(false);
            p.set_layouts(self.layouts.clone());
        }
        self.open_grownups(Some(4), cx);
        self.toast_note(crate::ui::Note::Controller, format!("{} is set up", s.name), cx);
    }

    pub fn setup_raw(&mut self, id: u32, input: PadInput, cx: &mut Cx<Msg>) {
        let Some(s) = &mut self.setup else { return };
        if s.pad != id || s.since.elapsed() < Duration::from_millis(250) {
            return;
        }
        let (_, out) = STEPS[s.step];
        // An input given to an earlier step isn't this one.
        if s.binds.iter().any(|(i, _)| same_input(i, &input)) {
            return;
        }
        let bind = match (out, input) {
            (Output::Axis(a @ (Axis::L2 | Axis::R2)), PadInput::Key(k)) => (PadInput::Key(k), Output::HalfAxis(a, 1)),
            (Output::Axis(a @ (Axis::L2 | Axis::R2)), PadInput::Abs { code, half, .. }) => {
                (PadInput::Abs { code, half: 0, invert: half < 0 }, Output::Axis(a))
            }
            (Output::Axis(a), PadInput::Abs { code, half, .. }) => {
                (PadInput::Abs { code, half: 0, invert: half < 0 }, Output::Axis(a))
            }
            // A stick can't be a button.
            (Output::Axis(_), _) => return,
            (out, input) => (input, out),
        };
        s.binds.push(bind);
        self.setup_next(cx);
    }

    /// Skip a step nothing was pressed for.
    pub fn setup_tick(&mut self, cx: &mut Cx<Msg>) {
        if self.setup.as_ref().is_some_and(|s| s.since.elapsed() >= STEP_WAIT) {
            self.setup_next(cx);
        }
    }
}

fn same_input(a: &PadInput, b: &PadInput) -> bool {
    match (a, b) {
        (PadInput::Abs { code: x, .. }, PadInput::Abs { code: y, .. }) => x == y,
        _ => a == b,
    }
}
