//! Home's shelves and the grids, built from the library, the rules and
//! what was played lately; and moving around them.

use std::path::PathBuf;

use aizome_kit::Cx;

use super::state::Item;
use super::{Chanoma, Input, Msg, Screen};
use crate::library::{Game, SYSTEMS};
use crate::pad::Dir;
use crate::services::{self, CATALOG};
use crate::ui::Focus;
use crate::ui::home::{self, Grid, Home, Shelf, Symbol, Tile, TileLook, TileSize};

/// Games a shelf shows before "All games".
const SHELF_GAMES: usize = 11;
const SHELF_FILMS: usize = 15;

/// What a tile does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Game(String),
    Service(String),
    Site(usize),
    Shelf(usize),
    Video(PathBuf),
    AllGames,
    AllFilms,
    GrownUps,
    Settings,
    Lock,
    Leave,
    AddGames,
}

impl Chanoma {
    /// Games kids may play now (all of them for a grown-up).
    pub fn playable(&self) -> Vec<&Game> {
        self.library.games.iter().filter(|g| self.is_grownup() || self.settings.game_allowed(&g.key)).collect()
    }

    /// The services on Home: ids, in order.
    pub fn services_shown(&self) -> Vec<&'static services::Service> {
        if self.is_grownup() {
            // Those kids see first, then the rest.
            let mut v: Vec<&'static services::Service> =
                self.settings.services.iter().filter_map(|id| services::by_id(id)).collect();
            v.extend(CATALOG.iter().filter(|s| !self.settings.services.iter().any(|id| id == s.id)));
            v
        } else {
            self.settings.services.iter().filter_map(|id| services::by_id(id)).collect()
        }
    }

    fn game_tile(&self, g: &Game, picture: bool) -> Tile {
        let state = self.config.dirs.states().join(format!("{}.png", g.file_key()));
        let (art, pixels) = if picture && state.is_file() { (Some(state), true) } else { (g.art.clone(), false) };
        let waiting = self.is_grownup() && self.settings.game_waiting(&g.key);
        Tile {
            label: g.title.clone(),
            sub: g.system.short.to_owned(),
            look: TileLook::Picture { art, color: g.system.color, pixels },
            badge: waiting.then(|| "New".to_owned()),
            progress: None,
        }
    }

    fn service_tile(s: &services::Service, allowed: bool) -> Tile {
        Tile {
            label: s.short.to_owned(),
            sub: if allowed { String::new() } else { "Grown-ups only".to_owned() },
            look: TileLook::Brand { color: s.color, ink: s.ink },
            badge: None,
            progress: None,
        }
    }

    /// Home's shelves and what each tile does.
    pub fn shelves(&self) -> Vec<(Shelf, Vec<Act>)> {
        let mut out = Vec::new();
        let grownup = self.is_grownup();
        let shelf = |title: &str, size: TileSize, tiles: Vec<(Tile, Act)>| {
            let (tiles, acts) = tiles.into_iter().unzip();
            (Shelf { title: title.to_owned(), size, tiles, start: 0 }, acts)
        };

        // Keep playing.
        let mut recent = Vec::new();
        let mut shown_shelves = std::collections::HashSet::new();
        for item in &self.recent.items {
            match item {
                Item::Game { key } => {
                    if let Some(g) = self.library.games.iter().find(|g| &g.key == key)
                        && (grownup || self.settings.game_allowed(key))
                    {
                        recent.push((self.game_tile(g, true), Act::Game(key.clone())));
                    }
                }
                Item::Service { id } => {
                    if let Some(s) = services::by_id(id)
                        && (grownup || self.settings.services.iter().any(|x| x == id))
                    {
                        recent.push((Self::service_tile(s, true), Act::Service(id.clone())));
                    }
                }
                Item::Site { url } => {
                    if let Some(i) = self.settings.custom_services.iter().position(|c| &c.url == url) {
                        let c = &self.settings.custom_services[i];
                        let (color, ink) = services::custom(&c.name);
                        recent.push((
                            Tile {
                                label: c.name.clone(),
                                sub: String::new(),
                                look: TileLook::Brand { color, ink },
                                badge: None,
                                progress: None,
                            },
                            Act::Site(i),
                        ));
                    }
                }
                Item::Video { path, position, duration } => {
                    let Some((si, shelf)) =
                        self.library.shelves.iter().enumerate().find(|(_, s)| s.videos.iter().any(|v| &v.path == path))
                    else {
                        continue;
                    };
                    // A show once, at its latest episode.
                    if !shown_shelves.insert(si) {
                        continue;
                    }
                    let at = shelf.videos.iter().position(|v| &v.path == path).expect("found above");
                    let finished = *duration > 0.0 && *position >= duration - 30.0;
                    // Finished: the next episode, from its start; a film or the
                    // last episode finished isn't kept playing.
                    let (v, progress) = match (finished, shelf.videos.get(at + 1)) {
                        (false, _) => (
                            &shelf.videos[at],
                            (*duration > 0.0).then(|| ((position / duration) * 1000.0).clamp(0.0, 1000.0) as u16),
                        ),
                        (true, Some(next)) => (next, None),
                        (true, None) => continue,
                    };
                    let path = &v.path;
                    let sub = if shelf.videos.len() > 1 { v.title.clone() } else { String::new() };
                    recent.push((
                        Tile {
                            label: shelf.title.clone(),
                            sub,
                            look: TileLook::Picture { art: shelf.poster.clone(), color: 0x30366e, pixels: false },
                            badge: None,
                            progress,
                        },
                        Act::Video(path.clone()),
                    ));
                }
            }
        }
        if !recent.is_empty() {
            out.push(shelf("Keep playing", TileSize::Wide, recent));
        }

        // Watch.
        let mut watch: Vec<(Tile, Act)> = self
            .services_shown()
            .into_iter()
            .map(|s| {
                let allowed = self.settings.services.iter().any(|id| id == s.id);
                (Self::service_tile(s, allowed), Act::Service(s.id.to_owned()))
            })
            .collect();
        for (i, c) in self.settings.custom_services.iter().enumerate() {
            let (color, ink) = services::custom(&c.name);
            watch.push((
                Tile {
                    label: c.name.clone(),
                    sub: String::new(),
                    look: TileLook::Brand { color, ink },
                    badge: None,
                    progress: None,
                },
                Act::Site(i),
            ));
        }
        if !watch.is_empty() {
            out.push(shelf("Watch", TileSize::Wide, watch));
        }

        // Games ahead of films: posters are tall, and the shelves after
        // them would wait below the screen.
        let games_at = out.len();
        // Films and shows.
        if !self.library.shelves.is_empty() {
            let mut films: Vec<(Tile, Act)> = self
                .library
                .shelves
                .iter()
                .enumerate()
                .take(SHELF_FILMS)
                .map(|(i, s)| {
                    let sub = if s.videos.len() > 1 { format!("{} episodes", s.videos.len()) } else { String::new() };
                    (
                        Tile {
                            label: s.title.clone(),
                            sub,
                            look: TileLook::Picture { art: s.poster.clone(), color: 0x30366e, pixels: false },
                            badge: None,
                            progress: None,
                        },
                        Act::Shelf(i),
                    )
                })
                .collect();
            if self.library.shelves.len() > SHELF_FILMS {
                films.push((
                    Tile {
                        label: "Everything".into(),
                        sub: String::new(),
                        look: TileLook::Symbol { symbol: Symbol::Film, color: 0x30366e },
                        badge: None,
                        progress: None,
                    },
                    Act::AllFilms,
                ));
            }
            out.push(shelf("Films and shows", TileSize::Poster, films));
        }

        // Games.
        let games = self.playable();
        if !games.is_empty() {
            let mut tiles: Vec<(Tile, Act)> =
                games.iter().take(SHELF_GAMES).map(|g| (self.game_tile(g, false), Act::Game(g.key.clone()))).collect();
            if games.len() > SHELF_GAMES || grownup {
                tiles.push((
                    Tile {
                        label: "All games".into(),
                        sub: format!("{}", games.len()),
                        look: TileLook::Symbol { symbol: Symbol::Grid, color: 0x3a3f86 },
                        badge: None,
                        progress: None,
                    },
                    Act::AllGames,
                ));
            }
            out.insert(games_at, shelf("Games", TileSize::Wide, tiles));
        } else if grownup {
            out.insert(
                games_at,
                shelf(
                    "Games",
                    TileSize::Wide,
                    vec![(
                        Tile {
                            label: "Add games".into(),
                            sub: "Choose their folder".into(),
                            look: TileLook::Symbol { symbol: Symbol::Plus, color: 0x3a3f86 },
                            badge: None,
                            progress: None,
                        },
                        Act::AddGames,
                    )],
                ),
            );
        }

        // The grown-ups' door.
        let more: Vec<(Tile, Act)> = if grownup {
            vec![
                (
                    Tile {
                        label: "Settings".into(),
                        sub: String::new(),
                        look: TileLook::Symbol { symbol: Symbol::Gear, color: 0x5a3e2a },
                        badge: None,
                        progress: None,
                    },
                    Act::Settings,
                ),
                (
                    Tile {
                        label: "Kids' mode".into(),
                        sub: "Lock grown-up mode".into(),
                        look: TileLook::Symbol { symbol: Symbol::Lock, color: 0x5a3e2a },
                        badge: None,
                        progress: None,
                    },
                    Act::Lock,
                ),
                (
                    Tile {
                        label: "Leave Chanoma".into(),
                        sub: "Back to the desktop".into(),
                        look: TileLook::Symbol { symbol: Symbol::Door, color: 0x5a3e2a },
                        badge: None,
                        progress: None,
                    },
                    Act::Leave,
                ),
            ]
        } else {
            vec![(
                Tile {
                    label: "Grown-ups".into(),
                    sub: "Needs the PIN".into(),
                    look: TileLook::Symbol { symbol: Symbol::Lock, color: 0x3a3f86 },
                    badge: None,
                    progress: None,
                },
                Act::GrownUps,
            )]
        };
        out.push(shelf("More", TileSize::Wide, more));
        out
    }

    /// Home as drawn.
    pub fn home_view(&self) -> Home {
        let mut shelves: Vec<Shelf> = self.shelves().into_iter().map(|(s, _)| s).collect();
        for (i, s) in shelves.iter_mut().enumerate() {
            let col = if i == self.focus.row { self.focus.col } else { self.home_cols.get(i).copied().unwrap_or(0) };
            s.start = home::start_for(col, s.tiles.len(), s.size);
        }
        Home { top: home::top_for(self.focus.row), shelves }
    }

    pub fn clamp_home_focus(&mut self) {
        if self.screen != Screen::Home {
            return;
        }
        let shelves = self.shelves();
        let rows = shelves.len();
        self.home_cols.resize(rows, 0);
        self.focus.row = self.focus.row.min(rows.saturating_sub(1));
        let len = shelves.get(self.focus.row).map_or(0, |(s, _)| s.tiles.len());
        self.focus.col = self.focus.col.min(len.saturating_sub(1));
        self.remember_home_focus();
    }

    pub fn remember_home_focus(&mut self) {
        if self.home_cols.len() <= self.focus.row {
            self.home_cols.resize(self.focus.row + 1, 0);
        }
        self.home_cols[self.focus.row] = self.focus.col;
        self.home_row = self.focus.row;
    }

    pub fn home_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let shelves = self.shelves();
        let rows = shelves.len();
        let len = |r: usize| shelves.get(r).map_or(0, |(s, _)| s.tiles.len());
        match i {
            Input::Dir(Dir::Left) => self.focus.col = self.focus.col.saturating_sub(1),
            Input::Dir(Dir::Right) => self.focus.col = (self.focus.col + 1).min(len(self.focus.row).saturating_sub(1)),
            Input::Dir(Dir::Up) | Input::Dir(Dir::Down) => {
                let down = i == Input::Dir(Dir::Down);
                let row = if down {
                    (self.focus.row + 1).min(rows.saturating_sub(1))
                } else {
                    self.focus.row.saturating_sub(1)
                };
                if row != self.focus.row {
                    self.remember_home_focus();
                    self.focus.row = row;
                    self.focus.col = self.home_cols.get(row).copied().unwrap_or(0).min(len(row).saturating_sub(1));
                }
            }
            Input::Prev => self.focus.col = 0,
            Input::Next => self.focus.col = len(self.focus.row).saturating_sub(1),
            Input::Back => {
                // Back on Home goes to the first tile of the first shelf.
                self.focus = Focus::default();
            }
            Input::Ok => {
                let act = shelves.get(self.focus.row).and_then(|(_, a)| a.get(self.focus.col)).cloned();
                self.remember_home_focus();
                if let Some(act) = act {
                    self.act(act, cx);
                }
                return;
            }
            _ => {}
        }
        self.remember_home_focus();
        self.want_art(cx);
    }

    /// Do what a tile does.
    pub fn act(&mut self, act: Act, cx: &mut Cx<Msg>) {
        match act {
            Act::Game(key) => self.start_game(&key, cx),
            Act::Service(id) => self.open_service(&id, cx),
            Act::Site(i) => self.open_site(i, cx),
            Act::Shelf(i) => {
                let Some(s) = self.library.shelves.get(i) else { return };
                if s.videos.len() == 1 {
                    let path = s.videos[0].path.clone();
                    self.play_video(&path, cx);
                } else {
                    // Start on the first episode not yet finished.
                    self.episode_focus = s
                        .videos
                        .iter()
                        .position(|v| {
                            self.recent.positions.get(&v.path).is_none_or(|(p, d)| *d <= 0.0 || *p < d - 30.0)
                        })
                        .unwrap_or(0);
                    self.go(Screen::Episodes(i), cx);
                }
            }
            Act::Video(path) => self.play_video(&path, cx),
            Act::AllGames => {
                self.games_focus = 0;
                self.games_tab = 0;
                self.go(Screen::Games, cx);
            }
            Act::AllFilms => {
                self.episode_focus = 0;
                self.go(Screen::Episodes(usize::MAX), cx);
            }
            Act::GrownUps => self.ask_pin(super::AfterPin::Unlock, cx),
            Act::Settings => self.open_grownups(None, cx),
            Act::AddGames => self.open_grownups(Some(2), cx),
            Act::Lock => self.lock(cx),
            Act::Leave => {
                self.shutdown();
                cx.quit();
            }
        }
    }

    /// The systems with games here, for the grid's tabs.
    fn game_tabs(&self) -> Vec<&'static crate::library::System> {
        let games = self.playable();
        SYSTEMS.iter().filter(|s| games.iter().any(|g| g.system.id == s.id)).collect()
    }

    fn games_in_tab(&self) -> Vec<&Game> {
        let tabs = self.game_tabs();
        let games = self.playable();
        match self.games_tab.checked_sub(1).and_then(|i| tabs.get(i)) {
            Some(sys) => games.into_iter().filter(|g| g.system.id == sys.id).collect(),
            None => games,
        }
    }

    pub fn games_view(&self) -> Grid {
        let tabs = self.game_tabs();
        let games = self.games_in_tab();
        let mut g = Grid {
            title: "Games".into(),
            subtitle: format!("{}", games.len()),
            tabs: std::iter::once("All".to_owned()).chain(tabs.iter().map(|s| s.short.to_owned())).collect(),
            tab: self.games_tab,
            size: TileSize::Wide,
            tiles: games.iter().map(|g| self.game_tile(g, false)).collect(),
            first_line: 0,
        };
        if g.tabs.len() <= 2 {
            g.tabs.clear();
        }
        g.first_line = g.first_line_for(self.focus.col);
        g
    }

    pub fn games_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let n = self.games_in_tab().len();
        let cols = home::across(TileSize::Wide);
        let tabs = self.game_tabs().len() + 1;
        let f = &mut self.focus.col;
        match i {
            Input::Dir(Dir::Left) => *f = f.saturating_sub(1),
            Input::Dir(Dir::Right) => *f = (*f + 1).min(n.saturating_sub(1)),
            Input::Dir(Dir::Up) => *f = f.saturating_sub(cols),
            Input::Dir(Dir::Down) => *f = (*f + cols).min(n.saturating_sub(1)),
            Input::Prev | Input::Next if tabs > 2 => {
                self.games_tab =
                    if i == Input::Next { (self.games_tab + 1) % tabs } else { (self.games_tab + tabs - 1) % tabs };
                self.focus.col = 0;
            }
            Input::Back => return self.go(Screen::Home, cx),
            Input::Ok => {
                let key = self.games_in_tab().get(self.focus.col).map(|g| g.key.clone());
                self.games_focus = self.focus.col;
                if let Some(key) = key {
                    self.start_game(&key, cx);
                }
                return;
            }
            _ => {}
        }
        self.games_focus = self.focus.col;
        self.want_art(cx);
    }

    /// A show's episodes, or (`usize::MAX`) every film and show.
    pub fn episodes_view(&self, shelf: usize) -> Grid {
        if shelf == usize::MAX {
            let tiles = self
                .library
                .shelves
                .iter()
                .map(|s| Tile {
                    label: s.title.clone(),
                    sub: String::new(),
                    look: TileLook::Picture { art: s.poster.clone(), color: 0x30366e, pixels: false },
                    badge: None,
                    progress: None,
                })
                .collect();
            let mut g = Grid {
                title: "Films and shows".into(),
                subtitle: String::new(),
                tabs: Vec::new(),
                tab: 0,
                size: TileSize::Poster,
                tiles,
                first_line: 0,
            };
            g.first_line = g.first_line_for(self.focus.col);
            return g;
        }
        let Some(s) = self.library.shelves.get(shelf) else {
            return Grid {
                title: String::new(),
                subtitle: String::new(),
                tabs: Vec::new(),
                tab: 0,
                size: TileSize::Wide,
                tiles: Vec::new(),
                first_line: 0,
            };
        };
        let tiles = s
            .videos
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let progress = self
                    .recent
                    .positions
                    .get(&v.path)
                    .filter(|(_, d)| *d > 0.0)
                    .map(|(p, d)| ((p / d) * 1000.0).clamp(0.0, 1000.0) as u16);
                Tile {
                    label: v.title.clone(),
                    // Its number, where its name doesn't already say it.
                    sub: if v.title.starts_with("Episode") || v.title.starts_with("Season") {
                        String::new()
                    } else {
                        format!("Episode {}", i + 1)
                    },
                    look: TileLook::Picture { art: None, color: 0x30366e, pixels: false },
                    badge: None,
                    progress,
                }
            })
            .collect();
        let mut g = Grid {
            title: s.title.clone(),
            subtitle: format!("{} episodes", s.videos.len()),
            tabs: Vec::new(),
            tab: 0,
            size: TileSize::Wide,
            tiles,
            first_line: 0,
        };
        g.first_line = g.first_line_for(self.focus.col);
        g
    }

    pub fn episodes_input(&mut self, shelf: usize, i: Input, cx: &mut Cx<Msg>) {
        let all = shelf == usize::MAX;
        let n = if all {
            self.library.shelves.len()
        } else {
            self.library.shelves.get(shelf).map_or(0, |s| s.videos.len())
        };
        let cols = home::across(if all { TileSize::Poster } else { TileSize::Wide });
        let f = &mut self.focus.col;
        match i {
            Input::Dir(Dir::Left) => *f = f.saturating_sub(1),
            Input::Dir(Dir::Right) => *f = (*f + 1).min(n.saturating_sub(1)),
            Input::Dir(Dir::Up) => *f = f.saturating_sub(cols),
            Input::Dir(Dir::Down) => *f = (*f + cols).min(n.saturating_sub(1)),
            Input::Back => return self.go(Screen::Home, cx),
            Input::Ok => {
                self.episode_focus = self.focus.col;
                if all {
                    return self.act(Act::Shelf(self.focus.col), cx);
                }
                let path =
                    self.library.shelves.get(shelf).and_then(|s| s.videos.get(self.focus.col)).map(|v| v.path.clone());
                if let Some(p) = path {
                    self.play_video(&p, cx);
                }
                return;
            }
            _ => {}
        }
        self.episode_focus = self.focus.col;
        self.want_art(cx);
    }

    /// Pictures the screen shows and those beside them.
    pub fn wanted_art(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut take = |t: &Tile| {
            if let TileLook::Picture { art: Some(p), .. } = &t.look {
                out.push(p.clone());
            }
        };
        match self.screen {
            Screen::Home => {
                let h = self.home_view();
                for (i, s) in h.shelves.iter().enumerate() {
                    // The shelves showing and the next; their tiles showing
                    // and a few past either edge.
                    if i + 1 < h.top || i > h.top + 3 {
                        continue;
                    }
                    let n = home::across(s.size);
                    for t in s.tiles.iter().skip(s.start.saturating_sub(2)).take(n + 4) {
                        take(t);
                    }
                }
            }
            Screen::Games | Screen::Episodes(_) => {
                let g = if self.screen == Screen::Games {
                    self.games_view()
                } else if let Screen::Episodes(s) = self.screen {
                    self.episodes_view(s)
                } else {
                    unreachable!()
                };
                let cols = g.columns().max(1);
                for t in g.tiles.iter().skip(g.first_line.saturating_sub(1) * cols).take(cols * 6) {
                    take(t);
                }
            }
            _ => {}
        }
        out
    }
}
