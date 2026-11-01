//! Choosing a games or videos folder with the controller, for the
//! settings' *Add a … folder*. Folders are read off the interface thread,
//! each answer marked with the reading it answers, so a slow folder left
//! behind doesn't land in the one showing.
//!
//! In the list: up and down move; the confirm button or right opens a
//! folder; Back goes up one (from the top of the computer, to the places);
//! left goes to the places. Among the places, moving shows each; the
//! confirm button or right goes into it; Back puts the picker away. From
//! anywhere, the top face button (Y on an Xbox controller, △, Tab) or
//! Start uses the folder showing; so does the list's first row.

use std::path::{Path, PathBuf};

use aizome_kit::Icon;
use aizome_kit::prelude::*;

use super::grownups::FolderKind;
use super::{Chanoma, Input, Msg, Screen};
use crate::folders::{self, Entry, Found, Place, PlaceKind};
use crate::pad::{Button, Dir};
use crate::ui::{Focus, picker};

/// The picker's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Picker {
    pub kind: FolderKind,
    pub places: Vec<Place>,
    pub dir: PathBuf,
    /// Its folders, once read.
    pub listing: Option<Result<Vec<Entry>, String>>,
    /// What Chanoma would find in it, once looked.
    pub found: Option<Found>,
    pub in_places: bool,
    /// The place the focus is on among the places.
    pub place: usize,
    /// The list's row the focus is on: 0 is the choice.
    pub row: usize,
    pub first: usize,
    /// Counts readings: an answer for an older one is dropped.
    pub reading: u64,
    /// The folder to put the focus on when the listing comes (the one
    /// left, going up).
    pub land: Option<PathBuf>,
}

impl Picker {
    fn entries(&self) -> &[Entry] {
        match &self.listing {
            Some(Ok(e)) => e,
            _ => &[],
        }
    }

    pub fn focus(&self) -> Focus {
        if self.in_places { Focus { row: self.place, col: 0 } } else { Focus { row: self.row, col: 1 } }
    }
}

impl Chanoma {
    pub fn open_picker(&mut self, kind: FolderKind, cx: &mut Cx<Msg>) {
        let places = folders::places(&self.config.home, &self.config.mounts);
        let home = self.config.home.clone();
        self.picker = Some(Picker {
            kind,
            places,
            dir: home.clone(),
            listing: None,
            found: None,
            in_places: false,
            place: 0,
            row: 0,
            first: 0,
            reading: 0,
            land: None,
        });
        self.go(Screen::Folders, cx);
        self.browse(home, None, cx);
    }

    /// Show `dir`: read its folders and what's in it, off this thread.
    fn browse(&mut self, dir: PathBuf, land: Option<PathBuf>, cx: &mut Cx<Msg>) {
        let cores = self.library.cores.clone();
        let Some(p) = &mut self.picker else { return };
        p.reading += 1;
        p.dir = dir.clone();
        p.listing = None;
        p.found = None;
        p.row = 0;
        p.first = 0;
        p.land = land;
        if !p.in_places {
            p.place = folders::place_of(&p.places, &dir).unwrap_or(0);
        }
        let (reading, kind) = (p.reading, p.kind);
        let d = dir.clone();
        cx.spawn(move || Msg::Listed(reading, folders::list(&d)));
        cx.spawn(move || {
            Msg::Found(
                reading,
                match kind {
                    FolderKind::Games => folders::games_in(&dir, &cores),
                    FolderKind::Videos => folders::videos_in(&dir),
                },
            )
        });
        self.picker_focus();
    }

    /// A folder's folders, read.
    pub fn listed(&mut self, reading: u64, listing: Result<Vec<Entry>, String>) {
        let Some(p) = &mut self.picker else { return };
        if p.reading != reading {
            return;
        }
        if let Err(e) = &listing {
            log::warn!("{}: {e}", p.dir.display());
        }
        p.listing = Some(listing);
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

    /// What's in the folder showing, found.
    pub fn found(&mut self, reading: u64, found: Found) {
        if let Some(p) = &mut self.picker
            && p.reading == reading
        {
            p.found = Some(found);
        }
    }

    fn picker_focus(&mut self) {
        if self.screen == Screen::Folders
            && let Some(p) = &self.picker
        {
            self.focus = p.focus();
        }
    }

    pub fn picker_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.picker else { return };
        if matches!(i, Input::Alt | Input::Menu) {
            return self.use_folder(cx);
        }
        if p.in_places {
            match i {
                Input::Dir(Dir::Up) | Input::Dir(Dir::Down) => {
                    let n = p.places.len();
                    let to =
                        if i == Input::Dir(Dir::Up) { p.place.saturating_sub(1) } else { (p.place + 1).min(n - 1) };
                    if to != p.place {
                        p.place = to;
                        let dir = p.places[to].path.clone();
                        self.browse(dir, None, cx);
                    }
                }
                Input::Dir(Dir::Right) | Input::Ok => {
                    p.in_places = false;
                    p.row = usize::from(!p.entries().is_empty());
                    p.first = picker::first_for(0, p.row);
                }
                Input::Back => return self.close_picker(None, cx),
                _ => {}
            }
            return self.picker_focus();
        }
        let rows = 1 + p.entries().len();
        let page = picker::rows_showing();
        match i {
            Input::Dir(Dir::Up) => p.row = p.row.saturating_sub(1),
            Input::Dir(Dir::Down) => p.row = (p.row + 1).min(rows - 1),
            Input::Prev => p.row = p.row.saturating_sub(page),
            Input::Next => p.row = (p.row + page).min(rows - 1),
            Input::Ok if p.row == 0 => return self.use_folder(cx),
            Input::Ok | Input::Dir(Dir::Right) => {
                if let Some(e) = p.row.checked_sub(1).and_then(|r| p.entries().get(r)) {
                    let dir = e.path.clone();
                    return self.browse(dir, None, cx);
                }
            }
            Input::Dir(Dir::Left) => {
                p.in_places = true;
                p.place = folders::place_of(&p.places, &p.dir).unwrap_or(0);
            }
            Input::Back => match p.dir.parent() {
                Some(up) => {
                    let (up, from) = (up.to_path_buf(), p.dir.clone());
                    return self.browse(up, Some(from), cx);
                }
                None => {
                    p.in_places = true;
                    p.place = folders::place_of(&p.places, &p.dir).unwrap_or(0);
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
        if let Some(i) = place.filter(|i| *i < p.places.len()) {
            p.in_places = true;
            p.place = i;
            let dir = p.places[i].path.clone();
            return self.browse(dir, None, cx);
        }
        if let Some(r) = row {
            p.in_places = false;
            p.row = r;
            self.picker_input(Input::Ok, cx);
        }
    }

    /// Use the folder showing.
    fn use_folder(&mut self, cx: &mut Cx<Msg>) {
        if let Some(dir) = self.picker.as_ref().map(|p| p.dir.clone()) {
            self.close_picker(Some(dir), cx);
        }
    }

    /// Back to the settings, with the folder chosen if one was: the focus
    /// where it was, on *Add a … folder*.
    pub fn close_picker(&mut self, chosen: Option<PathBuf>, cx: &mut Cx<Msg>) {
        let Some(p) = self.picker.take() else { return };
        if let Some(dir) = chosen {
            let name = self.folder_name(&dir);
            let had = match p.kind {
                FolderKind::Games => self.settings.game_folders.contains(&dir),
                FolderKind::Videos => self.settings.video_folders.contains(&dir),
            };
            self.folder_chosen(p.kind, dir, cx);
            self.toast(if had { format!("{name} is already there") } else { format!("Added {name}") }, cx);
        }
        self.go(Screen::GrownUps, cx);
        self.settings_on_add(p.kind);
    }

    /// A folder as people know it: under home as `~/Games`.
    pub fn folder_name(&self, dir: &Path) -> String {
        match dir.strip_prefix(&self.config.home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => dir.display().to_string(),
        }
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
        let was = p.places.get(p.place).map(|q| q.path.clone());
        p.places = places;
        p.place = was.and_then(|w| p.places.iter().position(|q| q.path == w)).unwrap_or(0).min(p.places.len() - 1);
        // A drive taken out from under the folder showing: back home.
        if !p.dir.is_dir() {
            p.in_places = true;
            p.place = 0;
            let home = p.places[0].path.clone();
            self.browse(home, None, cx);
        }
        self.picker_focus();
    }

    /// What the picker draws.
    pub fn picker_view(&self) -> picker::Picker {
        let Some(p) = &self.picker else {
            return picker::Picker {
                title: String::new(),
                places: Vec::new(),
                place: 0,
                crumbs: Vec::new(),
                found: None,
                body: picker::Body::Reading,
                first: 0,
            };
        };
        let games = p.kind == FolderKind::Games;
        picker::Picker {
            title: if games { "Choose a games folder".into() } else { "Choose a videos folder".into() },
            places: p
                .places
                .iter()
                .map(|q| {
                    let glyph = match q.kind {
                        PlaceKind::Home => Icon::House,
                        PlaceKind::Folder => Icon::Folder,
                        PlaceKind::Drive => Icon::Usb,
                        PlaceKind::Computer => Icon::HardDrive,
                    };
                    (q.name.clone(), glyph)
                })
                .collect(),
            place: folders::place_of(&p.places, &p.dir).unwrap_or(0),
            crumbs: folders::crumbs(&p.places, &p.dir),
            found: p.found.map(|f| (f.describe(), f.any())),
            body: match &p.listing {
                None => picker::Body::Reading,
                Some(Err(e)) => picker::Body::Failed(e.clone()),
                Some(Ok(entries)) => picker::Body::Folders(
                    entries
                        .iter()
                        .map(|e| (e.name.clone(), e.inside.map_or_else(String::new, |n| n.describe(games))))
                        .collect(),
                ),
            },
            first: p.first,
        }
    }

    /// The hints along the foot: what the buttons do where the focus is.
    pub fn picker_hints(&self) -> Vec<(Button, String)> {
        let Some(p) = &self.picker else { return Vec::new() };
        let back = if p.in_places {
            "Cancel"
        } else if p.dir.parent().is_none() {
            "Places"
        } else {
            "Up"
        };
        let mut h = self.hints("Open", Some(back));
        h.push((Button::North, "Use this folder".into()));
        h
    }
}
