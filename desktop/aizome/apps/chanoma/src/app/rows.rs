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
    /// Every game the controllers connected can play together.
    AllTogether,
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
        self.shown_games().filter(|g| self.is_grownup() || self.settings.game_allowed(&g.key)).collect()
    }

    /// The films and shows kids may watch now (all of them for a grown-up),
    /// by their place in the library.
    pub fn watchable(&self) -> Vec<usize> {
        let grownup = self.is_grownup();
        (0..self.library.shelves.len())
            .filter(|i| grownup || self.settings.film_allowed(&self.library.shelves[*i].key))
            .collect()
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

    /// Where `g`'s box art from libretro-thumbnails is, fetched or to be,
    /// when it may be fetched and hasn't been found missing.
    fn box_art(&self, g: &Game) -> Option<PathBuf> {
        if !self.settings.fetch_art || self.config.art_url.is_none() {
            return None;
        }
        let p = crate::art::cached(&self.config.dirs.art(), g.system.id, g.known_as.as_ref()?);
        (!self.art_missing.contains(&p)).then_some(p)
    }

    pub(crate) fn game_tile(&self, g: &Game, picture: bool) -> Tile {
        let state = self.config.dirs.states().join(format!("{}.png", g.file_key()));
        // On *Keep playing*, where it was left; else its box (beside it, or
        // fetched); else where it was left, if it was played; else none,
        // and the tile draws its sprite.
        let played = || self.recent.items.iter().any(|i| matches!(i, Item::Game { key } if *key == g.key));
        let (art, pixels) = if picture && state.is_file() {
            (Some(state), true)
        } else if let Some(a) = g.art.clone().or_else(|| self.box_art(g)) {
            (Some(a), false)
        } else if played() {
            (Some(state), true)
        } else {
            (None, false)
        };
        let waiting = self.is_grownup() && self.settings.game_waiting(&g.key);
        Tile {
            label: g.title.clone(),
            sub: g.system.short.to_owned(),
            look: TileLook::Picture { art, chip: Some(g.system.color), pixels },
            badge: waiting.then(|| "New".to_owned()),
            progress: None,
            more: g.info.as_ref().map(crate::metadata::Info::summary).unwrap_or_default(),
        }
    }

    fn service_tile(s: &services::Service, allowed: bool) -> Tile {
        Tile {
            label: s.short.to_owned(),
            sub: if allowed { String::new() } else { "Grown-ups only".to_owned() },
            look: TileLook::Brand { color: s.color, ink: s.ink },
            badge: None,
            progress: None,
            more: String::new(),
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
                                more: String::new(),
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
                    // A show once, at its latest episode; one kept from kids
                    // isn't theirs to carry on.
                    if !shown_shelves.insert(si) || !(grownup || self.settings.film_allowed(&shelf.key)) {
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
                            look: TileLook::Picture { art: shelf.poster.clone(), chip: None, pixels: false },
                            badge: None,
                            progress,
                            more: String::new(),
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
                    more: String::new(),
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
        let watchable = self.watchable();
        if !watchable.is_empty() {
            let mut films: Vec<(Tile, Act)> = watchable
                .iter()
                .take(SHELF_FILMS)
                .map(|&i| {
                    let s = &self.library.shelves[i];
                    let sub = if s.videos.len() > 1 { format!("{} episodes", s.videos.len()) } else { String::new() };
                    (
                        Tile {
                            label: s.title.clone(),
                            sub,
                            look: TileLook::Picture { art: s.poster.clone(), chip: None, pixels: false },
                            badge: None,
                            progress: None,
                            more: String::new(),
                        },
                        Act::Shelf(i),
                    )
                })
                .collect();
            if watchable.len() > SHELF_FILMS {
                films.push((
                    Tile {
                        label: "Everything".into(),
                        sub: String::new(),
                        look: TileLook::Symbol { symbol: Symbol::Film },
                        badge: None,
                        progress: None,
                        more: String::new(),
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
                        look: TileLook::Symbol { symbol: Symbol::Grid },
                        badge: None,
                        progress: None,
                        more: String::new(),
                    },
                    Act::AllGames,
                ));
            }
            out.insert(games_at, shelf("Games", TileSize::Wide, tiles));
            // With two controllers or more, the games that many can play
            // together, before the rest.
            if let Some(n) = self.together() {
                let together: Vec<&Game> = games.iter().copied().filter(|g| plays_with(g, n)).collect();
                if !together.is_empty() {
                    let mut tiles: Vec<(Tile, Act)> = together
                        .iter()
                        .take(SHELF_GAMES)
                        .map(|g| (self.game_tile(g, false), Act::Game(g.key.clone())))
                        .collect();
                    if together.len() > SHELF_GAMES {
                        tiles.push((
                            Tile {
                                label: "All of them".into(),
                                sub: format!("{}", together.len()),
                                look: TileLook::Symbol { symbol: Symbol::Grid },
                                badge: None,
                                progress: None,
                                more: String::new(),
                            },
                            Act::AllTogether,
                        ));
                    }
                    out.insert(games_at, shelf(&format!("For {n} players"), TileSize::Wide, tiles));
                }
            }
        } else if grownup {
            out.insert(
                games_at,
                shelf(
                    "Games",
                    TileSize::Wide,
                    vec![(
                        Tile {
                            label: "Add games".into(),
                            sub: "Show Chanoma their folder".into(),
                            look: TileLook::Symbol { symbol: Symbol::Plus },
                            badge: None,
                            progress: None,
                            more: String::new(),
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
                        look: TileLook::Symbol { symbol: Symbol::Gear },
                        badge: None,
                        progress: None,
                        more: String::new(),
                    },
                    Act::Settings,
                ),
                (
                    Tile {
                        label: "Kids' mode".into(),
                        sub: "Lock grown-up mode".into(),
                        look: TileLook::Symbol { symbol: Symbol::Lock },
                        badge: None,
                        progress: None,
                        more: String::new(),
                    },
                    Act::Lock,
                ),
                self.leave_tile(),
            ]
        } else {
            let mut v = vec![(
                Tile {
                    label: "Grown-ups".into(),
                    sub: "Needs the PIN".into(),
                    look: TileLook::Symbol { symbol: Symbol::Lock },
                    badge: None,
                    progress: None,
                    more: String::new(),
                },
                Act::GrownUps,
            )];
            // The TV account's kids may log out: the login screen is all
            // there is beyond, and anyone else's account asks for its own
            // password.
            if self.config.tv {
                v.push(self.leave_tile());
            }
            v
        };
        out.push(shelf("More", TileSize::Wide, more));
        out
    }

    /// The way out: to the desktop, or for the TV account, to the login
    /// screen.
    fn leave_tile(&self) -> (Tile, Act) {
        let (label, sub) = if self.config.tv {
            ("Log Out", "Back to the login screen")
        } else {
            ("Leave Chanoma", "Back to the desktop")
        };
        (
            Tile {
                label: label.into(),
                sub: sub.into(),
                look: TileLook::Symbol { symbol: Symbol::Door },
                badge: None,
                progress: None,
                more: String::new(),
            },
            Act::Leave,
        )
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

    /// Home's shelves' titles and where each was left (nothing off Home),
    /// to put the focus back on its shelf after one comes or goes above it
    /// (a second controller brings one).
    pub fn home_marks(&self) -> Vec<(String, usize)> {
        if self.screen != Screen::Home {
            return Vec::new();
        }
        let at =
            |i: usize| if i == self.focus.row { self.focus.col } else { self.home_cols.get(i).copied().unwrap_or(0) };
        self.shelves().into_iter().enumerate().map(|(i, (s, _))| (s.title, at(i))).collect()
    }

    pub fn restore_home_marks(&mut self, marks: &[(String, usize)]) {
        if marks.is_empty() || self.screen != Screen::Home {
            return;
        }
        let titles: Vec<String> = self.shelves().into_iter().map(|(s, _)| s.title).collect();
        if titles.iter().eq(marks.iter().map(|(t, _)| t)) {
            return;
        }
        let focused = marks.get(self.focus.row).map(|(t, _)| t.clone());
        self.home_cols = titles.iter().map(|t| marks.iter().find(|(m, _)| m == t).map_or(0, |(_, c)| *c)).collect();
        if let Some(row) = focused.and_then(|f| titles.iter().position(|t| *t == f)) {
            self.focus.row = row;
            self.focus.col = self.home_cols[row];
        }
        self.clamp_home_focus();
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
            // Looking for something by name.
            Input::Alt => return self.open_search(None, cx),
            Input::Typed(c) => return self.open_search(Some(c), cx),
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
                            self.recent.positions.get(&v.path).is_none_or(|(p, d)| !aizome_media::finished(*p, *d))
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
            Act::AllTogether => {
                self.games_focus = 0;
                self.games_tab = self.game_tabs().iter().position(|t| matches!(t, GameTab::Together(_))).unwrap_or(0);
                self.go(Screen::Games, cx);
            }
            Act::AllFilms => {
                self.episode_focus = 0;
                self.go(Screen::Episodes(usize::MAX), cx);
            }
            Act::GrownUps => self.ask_pin(super::AfterPin::Unlock, cx),
            Act::Settings => self.open_grownups(None, cx),
            Act::AddGames => self.open_picker(super::picker::From::Home, cx),
            Act::Lock => self.lock(cx),
            Act::Leave => {
                self.shutdown();
                cx.quit();
            }
        }
    }

    /// How many controllers are connected, when there's more than one.
    pub fn together(&self) -> Option<usize> {
        let n = self.controllers.len();
        (n >= 2).then_some(n)
    }

    /// The grid's tabs: all, those the controllers connected can play
    /// together, then each system with games here.
    /// How many tabs the games grid has.
    pub(crate) fn game_tab_count(&self) -> usize {
        self.game_tabs().len()
    }

    fn game_tabs(&self) -> Vec<GameTab> {
        let games = self.playable();
        let mut tabs = vec![GameTab::All];
        if let Some(n) = self.together()
            && games.iter().any(|g| plays_with(g, n))
        {
            tabs.push(GameTab::Together(n));
        }
        tabs.extend(SYSTEMS.iter().filter(|s| games.iter().any(|g| g.system.id == s.id)).map(GameTab::System));
        tabs
    }

    fn games_in_tab(&self) -> Vec<&Game> {
        let games = self.playable();
        match self.game_tabs().get(self.games_tab) {
            Some(GameTab::Together(n)) => games.into_iter().filter(|g| plays_with(g, *n)).collect(),
            Some(GameTab::System(sys)) => games.into_iter().filter(|g| g.system.id == sys.id).collect(),
            _ => games,
        }
    }

    pub fn games_view(&self) -> Grid {
        let tabs = self.game_tabs();
        let games = self.games_in_tab();
        let mut g = Grid {
            title: "Games".into(),
            subtitle: format!("{}", games.len()),
            tabs: tabs.iter().map(GameTab::label).collect(),
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
        let tabs = self.game_tabs().len();
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
            Input::Alt => return self.open_search(None, cx),
            Input::Typed(c) => return self.open_search(Some(c), cx),
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
                .watchable()
                .into_iter()
                .map(|i| &self.library.shelves[i])
                .map(|s| Tile {
                    label: s.title.clone(),
                    sub: String::new(),
                    look: TileLook::Picture { art: s.poster.clone(), chip: None, pixels: false },
                    badge: None,
                    progress: None,
                    more: String::new(),
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
                    look: TileLook::Picture { art: None, chip: None, pixels: false },
                    badge: None,
                    progress,
                    more: String::new(),
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
        let n =
            if all { self.watchable().len() } else { self.library.shelves.get(shelf).map_or(0, |s| s.videos.len()) };
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
                    let Some(&i) = self.watchable().get(self.focus.col) else { return };
                    return self.act(Act::Shelf(i), cx);
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
            Screen::Games | Screen::Episodes(_) | Screen::Search => {
                let g = if self.screen == Screen::Games {
                    self.games_view()
                } else if self.screen == Screen::Search {
                    self.search_view()
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

/// A tab of every game's grid.
#[derive(Clone, Copy, Debug, PartialEq)]
enum GameTab {
    All,
    /// The games this many can play together.
    Together(usize),
    System(&'static crate::library::System),
}

impl GameTab {
    fn label(&self) -> String {
        match self {
            GameTab::All => "All".into(),
            GameTab::Together(n) => format!("{n} players"),
            GameTab::System(s) => s.short.into(),
        }
    }
}

/// Whether `n` can play `g` at once: its database says so, and Chanoma
/// gives its system that many controllers.
fn plays_with(g: &Game, n: usize) -> bool {
    g.system.players >= n && g.info.as_ref().and_then(|i| i.players).is_some_and(|p| usize::from(p) >= n)
}
