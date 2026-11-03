//! Adding a folder with the controller, for games, films and shows alike:
//! everything in it, to any depth, is Chanoma's to show.
//!
//! It opens on *Suggested*: the folders that look like a collection (a
//! drive's `roms`, home's `Movies`), found by reading the top two levels
//! of home and each drive, never a whole drive. Or any folder is browsed
//! to and added. Browsing reads a folder and the folders in it, no
//! deeper; what the files index already knows of one is said beside it.
//! A folder is looked through to the bottom once added, in the background,
//! the settings showing how far it's got. Folders are read off the
//! interface thread, each answer marked with the reading it answers, so a
//! slow folder left behind doesn't land in the one showing.
//!
//! In the list: up and down move; the confirm button or right opens a
//! folder; Back goes up one (from the top of the computer, to the places);
//! left goes to the places. Among the places, moving shows each; the
//! confirm button or right goes into it; Back puts the picker away. From
//! anywhere, the top face button (Y on an Xbox controller, △, Tab) or
//! Start adds the folder showing; so does the list's first row. Among the
//! suggestions, the confirm button adds the one in focus, and right looks
//! inside it first.

use std::path::{Path, PathBuf};

use aizome_kit::Icon;
use aizome_kit::prelude::*;

use super::{Chanoma, Input, Msg, Screen};
use crate::folders::{self, Entry, Inside, Place, PlaceKind, Suggestion};
use crate::library::Held;
use crate::pad::{Button, Dir};
use crate::ui::{Focus, picker};

/// Beside the title: what adding a folder does.
const NOTE: &str = "Everything in it, however deep";

/// The picker's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Picker {
    pub places: Vec<Place>,
    /// The folders that look like a collection (`None` while looking).
    pub suggested: Option<Vec<Suggestion>>,
    /// *Suggested* is showing rather than a folder.
    pub on_suggested: bool,
    pub dir: PathBuf,
    /// Its folders, once read.
    pub listing: Option<Result<Vec<Entry>, String>>,
    /// What's directly in it, once read.
    pub inside: Option<Inside>,
    /// What the files index already knows is in it, if anything.
    pub known: Option<Held>,
    pub in_places: bool,
    /// The side list's row the focus is on: 0 is *Suggested*, then each
    /// place.
    pub place: usize,
    /// The list's row the focus is on: in a folder, 0 is the choice.
    pub row: usize,
    pub first: usize,
    /// Counts readings: an answer for an older one is dropped.
    pub reading: u64,
    /// The folder to put the focus on when the listing comes (the one
    /// left, going up).
    pub land: Option<PathBuf>,
    /// Where it was opened from, to go back to.
    pub from: From,
    /// Whether anything has been pressed: until then, *Suggested* with
    /// nothing in it gives way to home.
    pub touched: bool,
}

/// Where the picker was opened.
#[derive(Clone, Debug, PartialEq)]
pub enum From {
    /// The settings, as they were.
    Settings(super::grownups::State),
    Home,
}

impl Picker {
    fn entries(&self) -> &[Entry] {
        match &self.listing {
            Some(Ok(e)) => e,
            _ => &[],
        }
    }

    fn suggestions(&self) -> &[Suggestion] {
        self.suggested.as_deref().unwrap_or(&[])
    }

    /// The list's rows: the suggestions, or the choice and each folder.
    fn rows(&self) -> usize {
        if self.on_suggested { self.suggestions().len() } else { 1 + self.entries().len() }
    }

    pub fn focus(&self) -> Focus {
        if self.in_places { Focus { row: self.place, col: 0 } } else { Focus { row: self.row, col: 1 } }
    }
}

impl Chanoma {
    /// Open the picker, on *Suggested*, from the settings or Home.
    pub fn open_picker(&mut self, from: From, cx: &mut Cx<Msg>) {
        let places = folders::places(&self.config.home, &self.config.mounts);
        let home = self.config.home.clone();
        self.picker = Some(Picker {
            places,
            suggested: None,
            on_suggested: true,
            dir: home,
            listing: None,
            inside: None,
            known: None,
            in_places: false,
            place: 0,
            row: 0,
            first: 0,
            reading: 0,
            land: None,
            from,
            touched: false,
        });
        self.go(Screen::Folders, cx);
        self.look_for_suggestions(cx);
        self.picker_focus();
    }

    /// Look for the folders that look like a collection, off this thread.
    fn look_for_suggestions(&mut self, cx: &mut Cx<Msg>) {
        let Some(p) = &self.picker else { return };
        let places = p.places.clone();
        let (home, mounts) = (self.config.home.clone(), self.config.mounts.clone());
        let added = self.settings.folders.clone();
        cx.spawn(move || {
            let skip = |d: &Path| added.iter().any(|f| d.starts_with(f));
            Msg::Suggested(folders::suggest(&places, &home, &mounts, &skip))
        });
    }

    /// The suggestions, found.
    pub fn suggested(&mut self, s: Vec<Suggestion>, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.picker else { return };
        let none = s.is_empty();
        p.suggested = Some(s);
        p.row = p.row.min(p.rows().saturating_sub(1));
        // Nothing stands out, and nothing's been pressed: home instead.
        if none && p.on_suggested && !p.touched {
            p.place = 1;
            let home = self.config.home.clone();
            return self.browse(home, None, cx);
        }
        self.picker_focus();
    }

    /// Show `dir`: read its folders and what's in it, off this thread.
    fn browse(&mut self, dir: PathBuf, land: Option<PathBuf>, cx: &mut Cx<Msg>) {
        let cores = self.library.cores.clone();
        let Some(p) = &mut self.picker else { return };
        p.reading += 1;
        p.on_suggested = false;
        p.dir = dir.clone();
        p.listing = None;
        p.inside = None;
        p.known = None;
        p.row = 0;
        p.first = 0;
        p.land = land;
        if !p.in_places {
            p.place = 1 + folders::place_of(&p.places, &dir).unwrap_or(0);
        }
        let reading = p.reading;
        let d = dir.clone();
        cx.spawn(move || Msg::Listed(reading, folders::list(&d), folders::look_into(&d)));
        // What the index already knows of it: nothing is read for it.
        let dirs = self.config.dirs.clone();
        cx.spawn(move || Msg::Known(reading, folders::known_in(&mut dirs.open_index(), &dir, &cores)));
        self.picker_focus();
    }

    /// A folder's folders read, and what's directly in it.
    pub fn listed(&mut self, reading: u64, listing: Result<Vec<Entry>, String>, inside: Option<Inside>) {
        let Some(p) = &mut self.picker else { return };
        if p.reading != reading || p.on_suggested {
            return;
        }
        if let Err(e) = &listing {
            log::warn!("{}: {e}", p.dir.display());
        }
        p.listing = Some(listing);
        p.inside = inside;
        // Back on the folder left going up; else on the first folder in
        // this one, ready to go on down; else on the choice.
        let land = p.land.take();
        let entries = p.entries();
        let at = land.and_then(|l| entries.iter().position(|e| e.path == l));
        let any = !entries.is_empty();
        if !p.in_places {
            p.row = match at {
                Some(i) => i + 1,
                None => usize::from(any),
            };
        }
        p.first = picker::first_for(0, p.row);
        self.picker_focus();
    }

    /// What the files index knew of the folder showing.
    pub fn known(&mut self, reading: u64, known: Option<Held>) {
        if let Some(p) = &mut self.picker
            && p.reading == reading
        {
            p.known = known;
        }
    }

    fn picker_focus(&mut self) {
        if self.screen == Screen::Folders
            && let Some(p) = &self.picker
        {
            self.focus = p.focus();
        }
    }

    /// Show the side list's row `i`: *Suggested*, or a place.
    fn show_place(&mut self, i: usize, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.picker else { return };
        p.place = i;
        match i.checked_sub(1).and_then(|j| p.places.get(j)) {
            Some(q) => {
                let dir = q.path.clone();
                self.browse(dir, None, cx);
            }
            None => {
                p.on_suggested = true;
                p.reading += 1;
                p.row = 0;
                p.first = 0;
            }
        }
    }

    pub fn picker_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.picker else { return };
        p.touched = true;
        if matches!(i, Input::Alt | Input::Menu) {
            return self.use_folder(cx);
        }
        if p.in_places {
            match i {
                Input::Dir(Dir::Up) | Input::Dir(Dir::Down) => {
                    let n = 1 + p.places.len();
                    let to =
                        if i == Input::Dir(Dir::Up) { p.place.saturating_sub(1) } else { (p.place + 1).min(n - 1) };
                    if to != p.place {
                        self.show_place(to, cx);
                    }
                }
                Input::Dir(Dir::Right) | Input::Ok => {
                    p.in_places = false;
                    p.row = if p.on_suggested { 0 } else { usize::from(!p.entries().is_empty()) };
                    p.first = picker::first_for(0, p.row);
                }
                Input::Back => return self.close_picker(cx),
                _ => {}
            }
            return self.picker_focus();
        }
        let rows = p.rows().max(1);
        let page = picker::rows_showing();
        match i {
            Input::Dir(Dir::Up) => p.row = p.row.saturating_sub(1),
            Input::Dir(Dir::Down) => p.row = (p.row + 1).min(rows - 1),
            Input::Prev => p.row = p.row.saturating_sub(page),
            Input::Next => p.row = (p.row + page).min(rows - 1),
            Input::Ok if p.on_suggested || p.row == 0 => return self.use_folder(cx),
            // A suggestion looked into before it's added.
            Input::Dir(Dir::Right) if p.on_suggested => {
                if let Some(s) = p.suggestions().get(p.row) {
                    let dir = s.path.clone();
                    return self.browse(dir, None, cx);
                }
            }
            Input::Ok | Input::Dir(Dir::Right) => {
                if let Some(e) = p.row.checked_sub(1).and_then(|r| p.entries().get(r)) {
                    let dir = e.path.clone();
                    return self.browse(dir, None, cx);
                }
            }
            Input::Dir(Dir::Left) => {
                p.in_places = true;
                if !p.on_suggested {
                    p.place = 1 + folders::place_of(&p.places, &p.dir).unwrap_or(0);
                }
            }
            Input::Back if p.on_suggested => p.in_places = true,
            Input::Back => match p.dir.parent() {
                Some(up) => {
                    let (up, from) = (up.to_path_buf(), p.dir.clone());
                    return self.browse(up, Some(from), cx);
                }
                None => {
                    p.in_places = true;
                    p.place = 1 + folders::place_of(&p.places, &p.dir).unwrap_or(0);
                }
            },
            _ => {}
        }
        p.first = picker::first_for(p.first, p.row);
        self.picker_focus();
    }

    /// A click: a place, or a row of the list.
    pub fn picker_click(&mut self, place: Option<usize>, row: Option<usize>, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.picker else { return };
        p.touched = true;
        if let Some(i) = place.filter(|i| *i <= p.places.len()) {
            p.in_places = true;
            return self.show_place(i, cx);
        }
        if let Some(r) = row {
            p.in_places = false;
            p.row = r;
            self.picker_input(Input::Ok, cx);
        }
    }

    /// Add the suggestion in focus, or the folder showing.
    fn use_folder(&mut self, cx: &mut Cx<Msg>) {
        let Some(p) = &self.picker else { return };
        let dir = if p.on_suggested { p.suggestions().get(p.row).map(|s| s.path.clone()) } else { Some(p.dir.clone()) };
        let Some(dir) = dir else { return };
        if self.add_folder(dir, cx) {
            self.close_picker(cx);
        }
    }

    /// Back where the picker was opened.
    pub fn close_picker(&mut self, cx: &mut Cx<Msg>) {
        let Some(p) = self.picker.take() else { return };
        match p.from {
            From::Settings(state) => {
                self.go(Screen::GrownUps, cx);
                self.grownups = state;
                self.back_on_add();
            }
            From::Home => self.go(Screen::Home, cx),
        }
    }

    /// A folder as people say it: its name and where it is ("roms", "On
    /// Extreme Pro").
    pub fn said(&self, dir: &Path) -> (String, String) {
        folders::said(&self.config.home, &self.config.mounts, dir)
    }

    /// Every second while it shows: drives come and go.
    pub fn picker_tick(&mut self, cx: &mut Cx<Msg>) {
        if self.screen != Screen::Folders {
            return;
        }
        let places = folders::places(&self.config.home, &self.config.mounts);
        let Some(p) = &mut self.picker else { return };
        if places == p.places {
            return;
        }
        let was = p.place.checked_sub(1).and_then(|i| p.places.get(i)).map(|q| q.path.clone());
        p.places = places;
        p.place = match was {
            Some(w) => 1 + p.places.iter().position(|q| q.path == w).unwrap_or(0),
            None => 0,
        };
        // A drive plugged in or out: what stands out may have changed.
        self.look_for_suggestions(cx);
        let Some(p) = &mut self.picker else { return };
        // A drive taken out from under the folder showing: back home.
        if !p.on_suggested && !p.dir.is_dir() {
            p.in_places = true;
            p.place = 1;
            let home = p.places[0].path.clone();
            self.browse(home, None, cx);
        }
        self.picker_focus();
    }

    /// Whether `dir` is one of the folders, or inside one: that folder.
    fn looked_through(&self, dir: &Path) -> Option<&PathBuf> {
        self.settings.folders.iter().find(|f| dir.starts_with(f))
    }

    /// What the picker draws.
    pub fn picker_view(&self) -> picker::Picker {
        let Some(p) = &self.picker else {
            return picker::Picker {
                title: String::new(),
                note: String::new(),
                places: Vec::new(),
                place: 0,
                crumbs: Vec::new(),
                choice: picker::Choice::Add,
                found: None,
                body: picker::Body::Reading,
                first: 0,
            };
        };
        let mut places = vec![("Suggested".to_owned(), Icon::Star)];
        places.extend(p.places.iter().map(|q| {
            let glyph = match q.kind {
                PlaceKind::Home => Icon::House,
                PlaceKind::Folder => Icon::Folder,
                PlaceKind::Drive => Icon::Usb,
                PlaceKind::Computer => Icon::HardDrive,
            };
            (q.name.clone(), glyph)
        }));
        let added = |d: &Path| self.looked_through(d).is_some();
        if p.on_suggested {
            return picker::Picker {
                title: "Add a folder".into(),
                note: NOTE.into(),
                places,
                place: 0,
                crumbs: vec!["Folders that look like games, films or shows".into()],
                choice: picker::Choice::Add,
                found: None,
                body: match &p.suggested {
                    None => picker::Body::Looking,
                    Some(s) => picker::Body::Suggested(
                        s.iter().map(|s| (s.name.clone(), format!("{} · {}", s.place, s.word))).collect(),
                    ),
                },
                first: p.first,
            };
        }
        // Said beside the choice: what the index knows of it, else what's
        // directly in it.
        let found = match (&p.known, &p.inside) {
            (Some(k), _) => Some((format!("{}, as of the last look", folders::held_word(k)), true)),
            (None, Some(i)) => Some((i.describe(), i.folders + i.games + i.videos > 0)),
            (None, None) => None,
        };
        let choice = match self.looked_through(&p.dir) {
            Some(f) if *f == p.dir => picker::Choice::Already,
            Some(f) => picker::Choice::Inside(self.said(f).0),
            None => picker::Choice::Add,
        };
        picker::Picker {
            title: "Add a folder".into(),
            note: NOTE.into(),
            places,
            place: 1 + folders::place_of(&p.places, &p.dir).unwrap_or(0),
            crumbs: folders::crumbs(&p.places, &p.dir),
            choice,
            found,
            body: match &p.listing {
                None => picker::Body::Reading,
                Some(Err(e)) => picker::Body::Failed(e.clone()),
                Some(Ok(entries)) => picker::Body::Folders(
                    entries
                        .iter()
                        .map(|e| {
                            let mut word = e.inside.as_ref().map_or_else(String::new, Inside::describe);
                            if added(&e.path) {
                                word = format!("Added · {word}");
                            }
                            (e.name.clone(), word)
                        })
                        .collect(),
                ),
            },
            first: p.first,
        }
    }

    /// The hints along the foot: what the buttons do where the focus is.
    pub fn picker_hints(&self) -> Vec<(Button, String)> {
        let Some(p) = &self.picker else { return Vec::new() };
        if p.on_suggested && !p.in_places {
            let mut h = self.hints("Add", Some("Places"));
            h.push((Button::Right, "Look inside".into()));
            return h;
        }
        let back = if p.in_places {
            "Cancel"
        } else if p.dir.parent().is_none() {
            "Places"
        } else {
            "Up"
        };
        let mut h = self.hints("Open", Some(back));
        h.push((Button::North, "Add this folder".into()));
        h
    }
}
