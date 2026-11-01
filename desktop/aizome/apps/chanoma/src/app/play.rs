//! What plays: a game (a core on its own thread, its picture through the
//! window's video), a service (a page in the browser, driven through the
//! compositor) and a film or an episode (Utsushi's engine).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aizome_kit::Cx;
use aizome_media::{AudioChoice, Event as FilmEvent, Options, PlayState, Player, StreamKind};

use super::state::Item;
use super::{Chanoma, FILM, GAME, Input, Modal, Msg, Screen};
use crate::browser::{self, Action, Browser};
use crate::pad::{Button, Dir, Family};
use crate::retro::{CoreSource, Event as GameEvent, Launch, Session};
use crate::services::{self, Drive};
use crate::ui::{self, Chrome, Overlay, Scene, View};

/// A game being played.
pub struct Game {
    pub key: String,
    pub title: String,
    pub session: Session,
    /// Its picture's shape.
    pub aspect: f64,
    pub started: bool,
}

/// A page open in the browser.
pub struct Page {
    pub name: String,
    pub browser: Browser,
    /// Its window shows.
    pub shown: bool,
    pub drive: Drive,
}

/// A film or episode playing.
pub struct Film {
    pub path: PathBuf,
    pub title: String,
    pub player: Player,
    /// The episodes it's among (none: a film), and which it is.
    pub shelf: Option<usize>,
    pub duration: f64,
    pub display: Option<(f64, f64)>,
    pub playing: bool,
    pub controls_until: Option<Instant>,
    /// The bar shows over the picture.
    pub bar: bool,
    pub saved_at: Instant,
}

/// How long the film's bar stays after a press.
const BAR_STAYS: Duration = Duration::from_secs(4);

impl Chanoma {
    /// Whether kids may start something now; if not, the closed screen.
    fn may_start(&mut self, cx: &mut Cx<Msg>) -> bool {
        if self.is_grownup() || self.gate.open() {
            return true;
        }
        self.go(Screen::Closed, cx);
        false
    }

    // Games.

    pub fn start_game(&mut self, key: &str, cx: &mut Cx<Msg>) {
        let Some(g) = self.library.games.iter().find(|g| g.key == key).cloned() else { return };
        if !self.is_grownup() && !self.settings.game_allowed(key) {
            return;
        }
        if !self.may_start(cx) {
            return;
        }
        let Some(core) = self.library.cores.for_system(g.system).map(Path::to_path_buf) else {
            self.go(
                Screen::Problem(
                    "This game can't start".into(),
                    format!(
                        "There's no {} player installed. A grown-up can add one in Sekka's settings.",
                        g.system.name
                    ),
                ),
                cx,
            );
            return;
        };
        let file = g.file_key();
        let dirs = &self.config.dirs;
        let state = dirs.states().join(format!("{file}.state"));
        let core = match self.config.core_override {
            Some(api) => CoreSource::Api(api),
            None => CoreSource::Library(core),
        };
        let launch = Launch {
            core,
            game: g.path.clone(),
            save_ram: dirs.saves().join(format!("{file}.srm")),
            resume: state.is_file(),
            picture: dirs.states().join(format!("{file}.png")),
            state,
            system_dir: dirs.system(),
            save_dir: dirs.saves(),
            players: g.system.players,
            prefer: g.system.prefer.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            screen_lines: self.lines,
            silent: self.config.silent,
        };
        self.start_launch(&g.key, &g.title, launch, cx);
    }

    /// Start `launch` (also tests', with a core of their own).
    pub fn start_launch(&mut self, key: &str, title: &str, launch: Launch, cx: &mut Cx<Msg>) {
        if let Some(old) = self.game.take() {
            old.session.stop(true);
        }
        let sink = cx.video_sink(self.window, GAME);
        let tx = self.sender.clone();
        // Rumble goes straight to the controllers' thread, until the core
        // says otherwise.
        let rumble = self.pads.as_ref().map(|p| {
            let control = p.control();
            Box::new(move |slot: usize, strong: u16, weak: u16| control.rumble(slot, strong, weak, 0))
                as Box<dyn Fn(usize, u16, u16) + Send>
        });
        let session = Session::start(launch, sink, self.shared.clone(), rumble, move |e| {
            if let Some(t) = &tx {
                t.send(Msg::Game(e));
            }
        });
        self.game =
            Some(Game { key: key.to_owned(), title: title.to_owned(), session, aspect: 4.0 / 3.0, started: false });
        self.recent.touch(Item::Game { key: key.to_owned() });
        self.recent.save(&self.config.dirs);
        self.go(Screen::Starting(title.to_owned()), cx);
    }

    pub fn game_event(&mut self, e: GameEvent, cx: &mut Cx<Msg>) {
        match e {
            GameEvent::Started { aspect, resumed, .. } => {
                if let Some(g) = &mut self.game {
                    g.aspect = aspect;
                    g.started = true;
                }
                self.go(Screen::Playing, cx);
                cx.hide_cursor_until_moved();
                if resumed {
                    self.toast("Carrying on where you left off", cx);
                }
            }
            GameEvent::Aspect(a) => {
                if let Some(g) = &mut self.game {
                    g.aspect = a;
                }
            }
            GameEvent::Failed(why) => {
                log::warn!("chanoma: a game didn't start: {why}");
                self.game = None;
                self.go(Screen::Problem("This game didn't start".into(), why), cx);
            }
            GameEvent::Ended { .. } => {
                // A core that asked to end (or one stopped from here).
                if self.screen == Screen::Playing {
                    self.game = None;
                    self.go(Screen::Home, cx);
                }
            }
        }
    }

    pub fn pause_game(&self, on: bool) {
        if let Some(g) = &self.game {
            g.session.pause(on);
        }
    }

    /// Stop the game, keeping where it was left; back to Home (or the
    /// closed screen).
    pub fn stop_game(&mut self, keep: bool, cx: &mut Cx<Msg>) {
        if let Some(g) = self.game.take() {
            g.session.stop(keep);
            // Its picture of where it was left changed.
            let file = self.library.games.iter().find(|x| x.key == g.key).map(|x| x.file_key());
            if let Some(f) = file {
                let pic = self.config.dirs.states().join(format!("{f}.png"));
                std::rc::Rc::make_mut(&mut self.art).remove(&pic);
                self.art_asked.remove(&pic);
                self.art_rev += 1;
            }
        }
        self.back_to_top();
        let next = if self.is_grownup() || self.gate.open() { Screen::Home } else { Screen::Closed };
        self.go(next, cx);
    }

    /// Home from something played: at the top, where it now leads "Keep
    /// playing".
    fn back_to_top(&mut self) {
        self.home_row = 0;
        self.home_cols.clear();
    }

    // Services.

    pub fn open_service(&mut self, id: &str, cx: &mut Cx<Msg>) {
        let Some(s) = services::by_id(id) else { return };
        if !self.is_grownup() && !self.settings.services.iter().any(|x| x == id) {
            return;
        }
        let agent = s.tv_agent.then_some(services::TV_AGENT);
        self.open_page(s.id, s.name, s.url, agent, s.drive, Item::Service { id: id.to_owned() }, cx);
    }

    pub fn open_site(&mut self, i: usize, cx: &mut Cx<Msg>) {
        let Some(c) = self.settings.custom_services.get(i).cloned() else { return };
        let profile =
            format!("site-{}", c.url.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))));
        self.open_page(&profile, &c.name, &c.url, None, Drive::Pointer, Item::Site { url: c.url.clone() }, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn open_page(
        &mut self,
        profile: &str,
        name: &str,
        url: &str,
        agent: Option<&str>,
        drive: Drive,
        item: Item,
        cx: &mut Cx<Msg>,
    ) {
        if !self.may_start(cx) {
            return;
        }
        let Some(exe) = browser::find(self.settings.browser.as_deref()) else {
            self.go(
                Screen::Problem(
                    "There's no browser to watch with".into(),
                    "Shows play in Google Chrome (or Chromium). A grown-up can install it from Yukimi, Sekka's Software.".into(),
                ),
                cx,
            );
            return;
        };
        if let Some(old) = self.page.take() {
            old.browser.close();
        }
        let tx = self.sender.clone();
        let dir = self.config.dirs.browser().join(profile);
        match Browser::open(&exe, &dir, profile, url, agent, drive, self.shared.clone(), move |e| {
            if let Some(t) = &tx {
                t.send(Msg::Browser(e));
            }
        }) {
            Ok(b) => {
                self.page = Some(Page { name: name.to_owned(), browser: b, shown: false, drive });
                self.recent.touch(item);
                self.recent.save(&self.config.dirs);
                self.go(Screen::Watching, cx);
            }
            Err(e) => self.go(Screen::Problem(format!("{name} didn't open"), e.to_string()), cx),
        }
    }

    pub fn browser_event(&mut self, e: browser::Event, cx: &mut Cx<Msg>) {
        match e {
            browser::Event::Shown(_) => {
                if let Some(p) = &mut self.page {
                    p.shown = true;
                }
                // The page covers Chanoma: how to come back, said over it.
                self.toast_note(ui::Note::Home, self.way_back(), cx);
            }
            browser::Event::Gone => {
                if self.page.take().is_some() && self.screen == Screen::Watching {
                    self.modal = None;
                    self.after_activity(cx);
                }
            }
            // Home on the keyboard, kept from the page by the compositor:
            // as the controller's Home button, the menu with the way home.
            browser::Event::Home => {
                self.family = Family::Keyboard;
                self.input(Input::Home, cx);
            }
        }
    }

    /// A button while a page shows: what it does there.
    pub fn page_button(&mut self, b: Button, family: Family, pressed: bool) {
        let Some(p) = &self.page else { return };
        let ok = self.ok_button(family);
        let back = self.back_button(family);
        let action = if b == ok {
            Some(Action::Ok)
        } else if b == back {
            Some(Action::Back)
        } else {
            match b {
                Button::Up => Some(Action::Up),
                Button::Down => Some(Action::Down),
                Button::Left => Some(Action::Left),
                Button::Right => Some(Action::Right),
                Button::Start | Button::West => Some(Action::PlayPause),
                Button::North => Some(Action::FullScreen),
                Button::R1 => Some(Action::NextControl),
                Button::L1 => Some(Action::PreviousControl),
                _ => None,
            }
        };
        if let Some(a) = action {
            p.browser.bridge.act(a, pressed);
        }
    }

    pub fn close_page(&mut self, cx: &mut Cx<Msg>) {
        if let Some(p) = self.page.take() {
            p.browser.close();
        }
        self.after_activity(cx);
    }

    /// Back from a page or a film: Home, or the closed screen when time
    /// ran out meanwhile.
    fn after_activity(&mut self, cx: &mut Cx<Msg>) {
        cx.activate(self.window);
        self.back_to_top();
        let next = if self.is_grownup() || self.gate.open() { Screen::Home } else { Screen::Closed };
        self.go(next, cx);
    }

    // Films.

    pub fn play_video(&mut self, path: &Path, cx: &mut Cx<Msg>) {
        if !self.may_start(cx) {
            return;
        }
        let shelf = self.library.shelves.iter().position(|s| s.videos.iter().any(|v| v.path == path));
        let title = shelf
            .and_then(|i| {
                let s = &self.library.shelves[i];
                let v = s.videos.iter().find(|v| v.path == path)?;
                Some(if s.videos.len() > 1 { format!("{} · {}", s.title, v.title) } else { s.title.clone() })
            })
            .unwrap_or_else(|| crate::library::title_of(path));
        let player = match self.film.take() {
            Some(f) => {
                let pos = f.player.position();
                self.recent.set_position(&f.path, pos, f.duration);
                f.player
            }
            None => {
                let sink = cx.video_sink(self.window, FILM);
                let audio = if self.config.silent {
                    AudioChoice::Null
                } else {
                    AudioChoice::PipeWire(StreamKind {
                        latency: 0.170,
                        role: "Movie",
                        app_name: "Chanoma".into(),
                        app_id: super::APP_ID.into(),
                    })
                };
                let tx = self.sender.clone();
                Player::new(Options { sink, audio, hardware: true, caps: None }, move |e| {
                    if let Some(t) = &tx {
                        t.send(Msg::Film(e));
                    }
                })
            }
        };
        let start = self.recent.resume_at(path).unwrap_or(0.0);
        player.open(path.to_string_lossy().into_owned(), start, true);
        let duration = self.recent.positions.get(path).map_or(0.0, |p| p.1);
        self.recent.touch(Item::Video { path: path.to_owned(), position: start, duration });
        self.recent.save(&self.config.dirs);
        self.film = Some(Film {
            path: path.to_owned(),
            title,
            player,
            shelf,
            duration,
            display: None,
            playing: false,
            controls_until: Some(Instant::now() + BAR_STAYS),
            bar: true,
            saved_at: Instant::now(),
        });
        self.go(Screen::Film, cx);
        cx.hide_cursor_until_moved();
    }

    pub fn film_event(&mut self, e: FilmEvent, cx: &mut Cx<Msg>) {
        let Some(f) = &mut self.film else { return };
        match e {
            FilmEvent::Opened(info) => {
                f.duration = info.duration.unwrap_or(0.0);
                f.display = info.video.as_ref().map(|v| v.display);
            }
            FilmEvent::State(PlayState::Playing) => f.playing = true,
            FilmEvent::State(PlayState::Paused) => {
                f.playing = false;
                f.bar = true;
            }
            FilmEvent::State(PlayState::Ended) => {
                f.playing = false;
                let (path, duration) = (f.path.clone(), f.duration);
                self.recent.set_position(&path, duration, duration);
                // The next episode, if there is one.
                let next = f.shelf.and_then(|s| {
                    let vids = &self.library.shelves.get(s)?.videos;
                    let i = vids.iter().position(|v| v.path == path)?;
                    vids.get(i + 1).map(|v| v.path.clone())
                });
                match next {
                    Some(n) if self.is_grownup() || self.gate.open() => self.play_video(&n, cx),
                    _ => self.stop_film(cx),
                }
            }
            FilmEvent::Failed(why) => {
                self.film = None;
                self.go(Screen::Problem("This didn't play".into(), why), cx);
            }
            _ => {}
        }
    }

    pub fn film_input(&mut self, i: Input, cx: &mut Cx<Msg>) {
        let Some(f) = &mut self.film else { return };
        f.controls_until = Some(Instant::now() + BAR_STAYS);
        f.bar = true;
        match i {
            Input::Ok | Input::Menu => {
                if f.playing {
                    f.player.pause();
                } else {
                    f.player.play();
                }
            }
            Input::Dir(Dir::Left) => f.player.seek((f.player.position() - 10.0).max(0.0), false),
            Input::Dir(Dir::Right) => f.player.seek(f.player.position() + 10.0, false),
            Input::Back => self.open_menu(super::MenuKind::Film, cx),
            Input::Prev | Input::Next => {
                if let Some(n) = self.neighbour_episode(i == Input::Next) {
                    self.play_video(&n, cx);
                }
            }
            _ => {}
        }
    }

    fn neighbour_episode(&self, forward: bool) -> Option<PathBuf> {
        let f = self.film.as_ref()?;
        let vids = &self.library.shelves.get(f.shelf?)?.videos;
        let i = vids.iter().position(|v| v.path == f.path)?;
        let j = if forward { i.checked_add(1)? } else { i.checked_sub(1)? };
        vids.get(j).map(|v| v.path.clone())
    }

    pub fn stop_film(&mut self, cx: &mut Cx<Msg>) {
        if let Some(f) = self.film.take() {
            let pos = f.player.position();
            let dur = f.duration;
            if pos < dur - 1.0 || dur <= 0.0 {
                self.recent.set_position(&f.path, pos, dur);
            }
            self.recent.save(&self.config.dirs);
            f.player.close();
        }
        self.after_activity(cx);
    }

    /// The bar over a film: its name, where it is, and whether it plays.
    pub fn film_scene(&self, f: &Film) -> Scene {
        let pos = f.player.position();
        let chrome = Chrome { family: self.family, ok: self.ok_button(self.family), ..Chrome::default() };
        let label =
            format!("{}{}  {} / {}", if f.playing { "" } else { "Paused · " }, f.title, clock(pos), clock(f.duration));
        let progress = if f.duration > 0.0 { (pos / f.duration).clamp(0.0, 1.0) } else { 0.0 };
        Scene::new(
            View::Clear,
            Some(chrome),
            Some(Overlay::Bar(label, (progress * 1000.0) as u16)),
            self.focus,
            self.modal_focus,
            self.art.clone(),
            self.art_rev,
        )
    }

    /// Save where the film is, now and then.
    pub fn film_tick(&mut self) {
        let Some(f) = &mut self.film else { return };
        // The bar goes a while after the last press, while it plays.
        f.bar = !f.playing || f.controls_until.is_some_and(|t| Instant::now() < t);
        if f.saved_at.elapsed() < Duration::from_secs(10) {
            return;
        }
        f.saved_at = Instant::now();
        let (path, pos, dur) = (f.path.clone(), f.player.position(), f.duration);
        self.recent.set_position(&path, pos, dur);
        self.recent.save(&self.config.dirs);
    }

    /// End whatever plays: time ran out.
    pub fn end_activity(&mut self, cx: &mut Cx<Msg>) {
        if matches!(self.modal, Some(Modal::Menu { .. })) {
            self.modal = None;
        }
        if self.game.is_some() {
            self.stop_game(true, cx);
        }
        if self.page.is_some() {
            self.close_page(cx);
        }
        if self.film.is_some() {
            self.stop_film(cx);
        }
    }
}

/// `1:02:03` or `4:05`.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
