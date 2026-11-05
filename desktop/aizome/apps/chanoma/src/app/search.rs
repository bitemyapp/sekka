//! Search: the games, films and shows whoever's watching may open, by
//! what their names start with, typed on Chanoma's keyboard (or a real
//! one), the results under it as each letter goes in.
//!
//! Y (the top face button, Tab) on Home or among the games opens it, as
//! does typing on a keyboard there. While the keyboard shows, the arrows
//! move between its keys and the confirm button presses one; X (the left
//! face button, Backspace) takes a letter back, Y types a space, and Start
//! (Enter on a keyboard) or Back puts it away to go through the results.
//! Up from their first line brings it back; Back from them goes back
//! where search was opened.

use aizome_kit::prelude::*;

use super::rows::Act;
use super::{Chanoma, Input, Msg, Screen};
use crate::devtools::Field;
use crate::pad::{Button, Dir, Family};
use crate::ui::home::{self, Grid, Tile, TileLook, TileSize};
use crate::ui::keyboard::{Key, Keyboard, Typed};

/// Searching.
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    pub query: String,
    /// The keyboard, while typing; the results alone once it's away.
    pub keys: Option<Keyboard>,
    /// Where search was opened, to go back to.
    pub from: Screen,
}

/// A result: a game, by key, or a film or show, by its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Game(String),
    Shelf(usize),
}

/// How many results show at most.
const MOST: usize = 200;

/// A name's words, as compared: lower case letters and digits, the
/// accents people don't type taken off.
pub fn words(s: &str) -> Vec<String> {
    let folded: String = s
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            c => c,
        })
        .collect();
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

/// How well `query`'s words match a name's (and `also`, its system's
/// names): 0 when the name starts with them, 1 when each starts one of its
/// words (or is one of `also`), 2 when each of three letters or more is
/// in it; lower is better, `None` no match.
pub fn rank(query: &[String], name: &[String], also: &[String]) -> Option<u8> {
    if query.is_empty() {
        return None;
    }
    let joined = name.join(" ");
    if joined.starts_with(&query.join(" ")) {
        return Some(0);
    }
    if query.iter().all(|q| name.iter().any(|w| w.starts_with(q.as_str())) || also.contains(q)) {
        return Some(1);
    }
    if query.iter().all(|q| q.chars().count() >= 3 && joined.contains(q.as_str())) {
        return Some(2);
    }
    None
}

impl Chanoma {
    /// Open search, from where the focus is, with `first` typed already.
    pub fn open_search(&mut self, first: Option<char>, cx: &mut Cx<Msg>) {
        let from = self.screen.clone();
        self.search =
            Some(Search { query: first.map(String::from).unwrap_or_default(), keys: Some(Keyboard::new(None)), from });
        self.go(Screen::Search, cx);
        self.focus = crate::ui::Focus::default();
        log::info!("chanoma: search");
        self.want_art(cx);
    }

    /// What's found for what's typed, the best first.
    pub fn search_hits(&self) -> Vec<Hit> {
        let Some(s) = &self.search else { return Vec::new() };
        let q = words(&s.query);
        if q.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(u8, String, Hit)> = Vec::new();
        for g in self.playable() {
            let also = [g.system.id, g.system.short, g.system.name].iter().flat_map(|n| words(n)).collect::<Vec<_>>();
            if let Some(r) = rank(&q, &words(&g.title), &also) {
                hits.push((r, g.title.to_lowercase(), Hit::Game(g.key.clone())));
            }
        }
        for i in self.watchable() {
            let shelf = &self.library.shelves[i];
            if let Some(r) = rank(&q, &words(&shelf.title), &[]) {
                hits.push((r, shelf.title.to_lowercase(), Hit::Shelf(i)));
            }
        }
        hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| crate::library::natural(&a.1, &b.1)));
        hits.into_iter().take(MOST).map(|(.., h)| h).collect()
    }

    fn hit_tile(&self, h: &Hit) -> Option<Tile> {
        match h {
            Hit::Game(key) => self.library.games.iter().find(|g| &g.key == key).map(|g| self.game_tile(g, false)),
            Hit::Shelf(i) => self.library.shelves.get(*i).map(|s| Tile {
                label: s.title.clone(),
                sub: if s.videos.len() > 1 {
                    format!("A show · {} episodes", s.videos.len())
                } else {
                    "A film".into()
                },
                look: TileLook::Picture { art: s.poster.clone(), chip: None, pixels: false },
                badge: None,
                progress: None,
                more: String::new(),
            }),
        }
    }

    /// What search draws: the results, under the keyboard while typing.
    pub fn search_view(&self) -> Grid {
        let query = self.search.as_ref().map_or("", |s| s.query.as_str());
        let tiles: Vec<Tile> = self.search_hits().iter().filter_map(|h| self.hit_tile(h)).collect();
        let subtitle = match (query.trim().is_empty(), tiles.len()) {
            (true, _) => "Games, films and shows: type a name".to_owned(),
            (false, 0) => format!("Nothing called “{query}”"),
            (false, 1) => format!("“{query}”: 1 found"),
            (false, n) => format!("“{query}”: {n} found"),
        };
        let mut g = Grid {
            title: "Search".into(),
            subtitle,
            tabs: Vec::new(),
            tab: 0,
            size: TileSize::Wide,
            tiles,
            first_line: 0,
        };
        g.first_line = g.first_line_for(self.focus.col);
        g
    }

    /// The keyboard over the results, as the overlay draws it.
    pub fn search_keyboard(&self) -> Option<Keyboard> {
        let s = self.search.as_ref()?;
        let keys = s.keys.as_ref()?;
        let family = match self.family {
            Family::Keyboard => self.controllers.first().map_or(Family::Generic, |p| p.family),
            f => f,
        };
        let n = s.query.chars().count();
        Some(Keyboard {
            family,
            ok: self.ok_button(family),
            back: self.back_button(family),
            field: Some(Field {
                label: "Search games, films and shows".into(),
                text: s.query.clone(),
                length: n,
                caret: n,
                secret: false,
            }),
            ..keys.clone()
        })
    }

    pub fn search_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let Some(s) = &mut self.search else { return };
        if s.keys.is_some() {
            match i {
                Input::Dir(d) => {
                    if let Some(k) = &mut s.keys {
                        k.step(d);
                    }
                }
                // On a keyboard, Enter puts this one away.
                Input::Ok if self.family == Family::Keyboard => self.done_typing(cx),
                Input::Ok => {
                    let key = s.keys.as_ref().map_or(Key::Space, Keyboard::key);
                    self.search_key(key, cx);
                }
                Input::Typed(c) => self.search_type(&c.to_string(), cx),
                Input::Digit(d) => self.search_type(&d.to_string(), cx),
                Input::Space | Input::Alt => self.search_type(" ", cx),
                Input::Erase => self.search_erase(cx),
                Input::Menu => self.done_typing(cx),
                Input::Back => {
                    if s.query.trim().is_empty() {
                        return self.close_search(cx);
                    }
                    self.done_typing(cx);
                }
                _ => {}
            }
            return;
        }
        let n = self.search_hits().len();
        let cols = home::across(TileSize::Wide);
        let f = &mut self.focus.col;
        match i {
            Input::Dir(Dir::Left) => *f = f.saturating_sub(1),
            Input::Dir(Dir::Right) => *f = (*f + 1).min(n.saturating_sub(1)),
            // Up from the first line: typing again.
            Input::Dir(Dir::Up) if *f < cols => return self.type_again(None, cx),
            Input::Dir(Dir::Up) => *f = f.saturating_sub(cols),
            Input::Dir(Dir::Down) => *f = (*f + cols).min(n.saturating_sub(1)),
            Input::Alt | Input::Menu => return self.type_again(None, cx),
            Input::Typed(c) => return self.type_again(Some(c.to_string()), cx),
            Input::Digit(d) => return self.type_again(Some(d.to_string()), cx),
            Input::Erase => {
                self.type_again(None, cx);
                return self.search_erase(cx);
            }
            Input::Back => return self.close_search(cx),
            Input::Ok => {
                let hit = self.search_hits().get(self.focus.col).cloned();
                match hit {
                    Some(Hit::Game(key)) => self.start_game(&key, cx),
                    Some(Hit::Shelf(i)) => self.act(Act::Shelf(i), cx),
                    None => {}
                }
                return;
            }
            _ => {}
        }
        self.want_art(cx);
    }

    /// Key `n` of the keyboard clicked ([`Keyboard::hit`]).
    pub fn search_click_key(&mut self, n: usize, cx: &mut Cx<Msg>) {
        let Some(k) = self.search.as_mut().and_then(|s| s.keys.as_mut()) else { return };
        k.go_to(n);
        let key = k.key();
        self.search_key(key, cx);
    }

    fn search_key(&mut self, key: Key, cx: &mut Cx<Msg>) {
        let Some(k) = self.search.as_mut().and_then(|s| s.keys.as_mut()) else { return };
        match k.press(key) {
            Some(Typed::Text(t)) => self.search_type(&t, cx),
            Some(Typed::Edit(crate::devtools::Key::Backspace)) => self.search_erase(cx),
            Some(Typed::Edit(crate::devtools::Key::Enter)) => self.done_typing(cx),
            _ => {}
        }
    }

    fn search_type(&mut self, t: &str, cx: &mut Cx<Msg>) {
        if let Some(s) = &mut self.search {
            s.query.push_str(t);
        }
        self.focus.col = 0;
        self.want_art(cx);
    }

    fn search_erase(&mut self, cx: &mut Cx<Msg>) {
        if let Some(s) = &mut self.search {
            s.query.pop();
        }
        self.focus.col = 0;
        self.want_art(cx);
    }

    /// The keyboard away, the focus on the first result (none: it stays).
    pub fn done_typing(&mut self, cx: &mut Cx<Msg>) {
        if self.search_hits().is_empty() {
            return;
        }
        if let Some(s) = &mut self.search {
            s.keys = None;
        }
        self.focus.col = 0;
        self.want_art(cx);
    }

    fn type_again(&mut self, typed: Option<String>, cx: &mut Cx<Msg>) {
        if let Some(s) = &mut self.search
            && s.keys.is_none()
        {
            s.keys = Some(Keyboard::new(None));
        }
        if let Some(t) = typed {
            self.search_type(&t, cx);
        }
    }

    /// Back where search was opened.
    pub fn close_search(&mut self, cx: &mut Cx<Msg>) {
        let from = self.search.take().map_or(Screen::Home, |s| s.from);
        self.go(from, cx);
    }

    /// The hints while searching.
    pub fn search_hints(&self) -> Vec<(Button, String)> {
        if self.search.as_ref().is_some_and(|s| s.keys.is_some()) {
            return Vec::new();
        }
        let mut h = self.hints("Open", Some("Back"));
        h.push((Button::North, "Type".into()));
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        words(s)
    }

    #[test]
    fn names_match_by_what_their_words_start_with() {
        let mario = w("Super Mario World");
        let snes = w("snes Super NES Super Nintendo");
        assert_eq!(rank(&w("super ma"), &mario, &snes), Some(0), "the name starts so");
        assert_eq!(rank(&w("mar"), &mario, &snes), Some(1), "a word starts so");
        assert_eq!(rank(&w("world mar"), &mario, &snes), Some(1), "in any order");
        assert_eq!(rank(&w("snes mario"), &mario, &snes), Some(1), "the system's name too");
        assert_eq!(rank(&w("ario"), &mario, &snes), Some(2), "three letters inside a word");
        assert_eq!(rank(&w("ar"), &mario, &snes), None, "two aren't enough inside one");
        assert_eq!(rank(&w("zelda"), &mario, &snes), None);
        assert_eq!(rank(&w(""), &mario, &snes), None);
        assert_eq!(rank(&w("pokemon"), &w("Pokémon - Red Version"), &[]), Some(0), "accents as typed");
    }
}
