//! What plays: a game (a core on its own thread, its picture through the
//! window's video), a service (a page in the browser, driven through the
//! compositor) and a film or an episode (Utsushi's engine, its subtitles
//! and languages chosen in the film's menu, and on the desktop's media
//! controls as Utsushi is).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::{Cx, TimerHandle};
use aizome_media::{AudioChoice, Cues, Event as FilmEvent, MediaInfo, Options, PlayState, Player, StreamKind, Track};

use super::state::{Item, Languages, Pick, Subtitles};
use super::typing::{Typing, Why};
use super::{Chanoma, FILM, GAME, Input, MenuKind, Modal, Msg, Screen};
use crate::browser::{self, Action, Browser};
use crate::pad::{Button, Dir, Family};
use crate::retro::{CoreSource, Event as GameEvent, Launch, Session};
use crate::services::{self, Drive, Sites};
use crate::ui::{self, Chrome, Overlay, Scene, View};

/// A game being played.
pub struct Game {
    /// Which start it is: its session's words carry this.
    pub id: u64,
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
    /// Chanoma's keyboard comes up for its text boxes.
    pub keyboard: bool,
    /// The keyboard, while it's up.
    pub typing: Option<Typing>,
    /// The text box that had the page's keyboard when last asked.
    pub field: Option<crate::devtools::Field>,
    /// Why the page is being asked what has its keyboard.
    pub asked: Option<Why>,
    /// Select and Start while held: whether the other went down too.
    pub select: Option<bool>,
    pub start: Option<bool>,
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
    /// What it holds, once open: its sound and subtitle tracks.
    pub info: Option<Arc<MediaInfo>>,
    /// The tracks playing.
    pub audio: Option<usize>,
    pub subtitles: Option<usize>,
    /// The subtitles' cues, and the timer for when what shows changes.
    pub cues: Cues,
    pub cue_timer: Option<TimerHandle>,
    /// Whose languages it keeps: its show's (folder) or its own (file).
    pub key: String,
    /// What the desktop's media controls know it by, and its seeks.
    pub item: u64,
    pub seeks: u64,
}

/// What the film's menu holds, after Keep watching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilmItem {
    Keep,
    Subtitles,
    Language,
    Stop,
}

/// How long the film's bar stays after a press.
const BAR_STAYS: Duration = Duration::from_secs(4);

/// How long a game starts before Chanoma asks whether to keep waiting. A
/// core reads its game and sets up in a second or two, from a slow drive
/// in a few more; Back and Home call it off before then and after.
pub const START_WAIT: Duration = Duration::from_secs(15);

/// Games starting: their numbers, the one waited for, and one given up on.
#[derive(Default)]
pub struct Starts {
    /// Games started so far.
    count: u64,
    /// The start waited for, and the timer that asks about it.
    wait: Option<(u64, TimerHandle)>,
    /// The start waited for took too long (and was asked about).
    slow: bool,
    /// A start given up on whose core hasn't let go: no game starts until
    /// it has, since two of one core in a process don't mix.
    given_up: Option<(u64, std::thread::JoinHandle<()>)>,
    /// A game waiting for that.
    queued: Option<Queued>,
}

impl Starts {
    /// Whether a start given up on still holds its core.
    pub fn stuck(&self) -> bool {
        self.given_up.is_some()
    }

    /// Whether the game starting is taking too long.
    pub fn slow(&self) -> bool {
        self.slow
    }

    /// Whether the game starting waits for one given up on.
    pub fn behind(&self) -> bool {
        self.queued.is_some()
    }
}

struct Queued {
    id: u64,
    key: String,
    title: String,
    launch: Launch,
}

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
            bios: crate::library::bios_dirs(),
            save_dir: dirs.saves(),
            players: g.system.players,
            prefer: g.system.prefer.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            screen_lines: self.lines,
            silent: self.config.silent,
            unpack_dir: dirs.unzipped(),
            extensions: g.system.extensions,
        };
        self.start_launch(&g.key, &g.title, launch, cx);
    }

    /// Start `launch` (also tests', with a core of their own).
    pub fn start_launch(&mut self, key: &str, title: &str, launch: Launch, cx: &mut Cx<Msg>) {
        self.give_up(cx);
        if let Some(old) = self.game.take() {
            old.session.stop(true);
        }
        self.starts.count += 1;
        let id = self.starts.count;
        self.wait_for(id, cx);
        if self.starts.stuck() {
            // It starts when the one given up on lets go of its core.
            self.starts.queued = Some(Queued { id, key: key.to_owned(), title: title.to_owned(), launch });
        } else {
            self.launch(id, key, title, launch, cx);
        }
        self.go(Screen::Starting(title.to_owned()), cx);
    }

    fn launch(&mut self, id: u64, key: &str, title: &str, launch: Launch, cx: &mut Cx<Msg>) {
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
                t.send(Msg::Game(id, e));
            }
        });
        self.game =
            Some(Game { id, key: key.to_owned(), title: title.to_owned(), session, aspect: 4.0 / 3.0, started: false });
    }

    /// Ask about the game if it hasn't started in a while.
    fn wait_for(&mut self, id: u64, cx: &mut Cx<Msg>) {
        self.calm(cx);
        if let Some((_, t)) = self.starts.wait.replace((id, cx.after(START_WAIT, Msg::StartWait(id)))) {
            cx.cancel(t);
        }
    }

    fn done_waiting(&mut self, cx: &mut Cx<Msg>) {
        self.calm(cx);
        if let Some((_, t)) = self.starts.wait.take() {
            cx.cancel(t);
        }
    }

    /// Not slow any more: the question goes if it's up.
    fn calm(&mut self, cx: &mut Cx<Msg>) {
        self.starts.slow = false;
        if matches!(self.modal, Some(Modal::Menu { kind: MenuKind::Slow, .. })) {
            self.modal = None;
            self.sync_hud(cx);
        }
    }

    /// Stop waiting for a game still starting, without waiting for its
    /// core either: told to end, it does when it can ([`Session::abandon`]).
    fn give_up(&mut self, cx: &mut Cx<Msg>) {
        self.done_waiting(cx);
        self.starts.queued = None;
        if let Some(g) = self.game.take_if(|g| !g.started) {
            log::warn!("chanoma: gave up on {} starting", g.title);
            if let Some(thread) = g.session.abandon() {
                self.starts.given_up = Some((g.id, thread));
            }
        }
    }

    /// Back or Home while a game starts, or Stop it now: it's called off.
    /// One that took too long leaves Keep playing too, so that a kid
    /// doesn't come back to it first.
    pub fn stop_starting(&mut self, cx: &mut Cx<Msg>) {
        if self.starts.slow
            && let Some(g) = &self.game
        {
            self.recent.forget(&Item::Game { key: g.key.clone() });
            self.recent.save(&self.config.dirs);
        }
        self.give_up(cx);
        self.go(Screen::Home, cx);
    }

    /// The game hasn't started in a while: stop it now, or keep waiting
    /// (with Back and Home to stop it later).
    pub fn start_waited(&mut self, id: u64, cx: &mut Cx<Msg>) {
        if self.starts.wait.as_ref().is_none_or(|(w, _)| *w != id) {
            return;
        }
        self.starts.wait = None;
        self.starts.slow = true;
        log::warn!("chanoma: a game is taking a long time to start");
        if matches!(self.screen, Screen::Starting(_)) && self.modal.is_none() {
            self.open_menu(MenuKind::Slow, cx);
        }
    }

    pub fn game_event(&mut self, id: u64, e: GameEvent, cx: &mut Cx<Msg>) {
        if self.starts.given_up.as_ref().is_some_and(|(g, _)| *g == id) {
            if matches!(e, GameEvent::Ended { .. } | GameEvent::Failed(_)) {
                self.let_go(cx);
            }
            return;
        }
        // Words from a session already put away.
        if self.game.as_ref().is_none_or(|g| g.id != id) {
            return;
        }
        match e {
            GameEvent::Started { aspect, resumed, .. } => {
                self.done_waiting(cx);
                if let Some(g) = &mut self.game {
                    g.aspect = aspect;
                    g.started = true;
                    self.recent.touch(Item::Game { key: g.key.clone() });
                    self.recent.save(&self.config.dirs);
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
                self.done_waiting(cx);
                if let Some(g) = self.game.take() {
                    self.recent.forget(&Item::Game { key: g.key });
                    self.recent.save(&self.config.dirs);
                }
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

    /// The start given up on let go of its core at last: a game waiting
    /// for that starts.
    fn let_go(&mut self, cx: &mut Cx<Msg>) {
        if let Some((_, thread)) = self.starts.given_up.take() {
            // Its last word comes just before it ends.
            let _ = thread.join();
        }
        if let Some(q) = self.starts.queued.take() {
            self.wait_for(q.id, cx);
            self.launch(q.id, &q.key, &q.title, q.launch, cx);
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
        // One still starting isn't waited for.
        self.give_up(cx);
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
        // Said here rather than by the service's own error page, whose
        // code means nothing to a kid.
        if s.drm && browser::find(self.settings.browser.as_deref()).is_some_and(|b| !browser::is_chrome(&b)) {
            self.go(
                Screen::Problem(
                    format!("{} needs Google Chrome", s.name),
                    "Its shows are protected, and only Google Chrome plays those here. A grown-up can install it \
                     from Yukimi, Sekka's Software."
                        .into(),
                ),
                cx,
            );
            return;
        }
        let agent = s.tv_agent.then_some(services::TV_AGENT);
        let item = Item::Service { id: id.to_owned() };
        self.open_page(s.id, s.name, s.url, Sites::of(s), agent, s.drive, s.keyboard, item, cx);
    }

    pub fn open_site(&mut self, i: usize, cx: &mut Cx<Msg>) {
        let Some(c) = self.settings.custom_services.get(i).cloned() else { return };
        let profile =
            format!("site-{}", c.url.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))));
        let sites = Sites::new(&c.url, &[]);
        let item = Item::Site { url: c.url.clone() };
        self.open_page(&profile, &c.name, &c.url, sites, None, Drive::Pointer, true, item, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn open_page(
        &mut self,
        profile: &str,
        name: &str,
        url: &str,
        sites: Sites,
        agent: Option<&str>,
        drive: Drive,
        keyboard: bool,
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
        let kept = !self.is_grownup();
        match Browser::open(&exe, &dir, profile, url, sites, kept, agent, drive, self.shared.clone(), move |e| {
            if let Some(t) = &tx {
                t.send(Msg::Browser(e));
            }
        }) {
            Ok(b) => {
                self.page = Some(Page {
                    name: name.to_owned(),
                    browser: b,
                    shown: false,
                    drive,
                    keyboard,
                    typing: None,
                    field: None,
                    asked: None,
                    select: None,
                    start: None,
                });
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
            // A link out of the service: the page stays where it was.
            browser::Event::Outside(_) => {
                if let Some(p) = &self.page {
                    let text = format!("That's outside {}", p.name);
                    self.toast_note(ui::Note::Outside, text, cx);
                }
            }
            // It couldn't be kept to the service, so it shows nothing.
            browser::Event::Unkept => {
                if let Some(p) = self.page.take() {
                    p.browser.close();
                    self.modal = None;
                    self.go(
                        Screen::Problem(
                            format!("{} didn't open", p.name),
                            "Chanoma keeps a service's pages to the service, through the browser, and this browser \
                             didn't let it. Google Chrome and Chromium do."
                                .into(),
                        ),
                        cx,
                    );
                }
            }
            browser::Event::Focused(f) => self.page_focused(f, cx),
        }
    }

    /// A button while a page shows: what it does there. Select and Start
    /// count when they're let go alone (held together, they're Home).
    pub fn page_button(&mut self, b: Button, family: Family, pressed: bool, cx: &mut Cx<Msg>) {
        let ok = self.ok_button(family);
        let back = self.back_button(family);
        let Some(p) = &mut self.page else { return };
        let alone = match b {
            Button::Select | Button::Start => {
                let (this, other) =
                    if b == Button::Select { (&mut p.select, &mut p.start) } else { (&mut p.start, &mut p.select) };
                if pressed {
                    *this = Some(other.is_some());
                    if other.is_some() {
                        *other = Some(true);
                    }
                    false
                } else {
                    this.take() == Some(false)
                }
            }
            _ => false,
        };
        if p.typing.is_some() {
            return self.keyboard_button(b, family, pressed, alone, cx);
        }
        // A text box moved to with the shoulder buttons: choosing it brings
        // the keyboard up, rather than pressing Return in it.
        if p.keyboard && b == ok && pressed && !p.browser.bridge.pointing() && p.field.is_some() {
            let field = p.field.clone();
            return self.open_keyboard(false, field, cx);
        }
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
                Button::West => Some(Action::PlayPause),
                Button::Start if alone => Some(Action::PlayPause),
                Button::Select if alone && p.keyboard => return self.open_keyboard(true, None, cx),
                Button::North => Some(Action::FullScreen),
                Button::R1 => Some(Action::NextControl),
                Button::L1 => Some(Action::PreviousControl),
                _ => None,
            }
        };
        if let Some(a) = action {
            // Start alone is known as it's let go: it presses then.
            p.browser.bridge.act(a, pressed || alone);
        }
        if b == ok && !pressed {
            self.ask_page_soon(Why::Chose(Instant::now()), cx);
        } else if matches!(b, Button::L1 | Button::R1) && pressed {
            self.ask_page_soon(Why::Moved, cx);
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
                if let Some(t) = f.cue_timer {
                    cx.cancel(t);
                }
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
                let player = Player::new(Options { sink, audio, hardware: true, caps: None }, move |e| {
                    if let Some(t) = &tx {
                        t.send(Msg::Film(e));
                    }
                });
                // As the desktop's media controls last set it.
                player.set_volume(self.film_volume, false);
                player
            }
        };
        let start = self.recent.resume_at(path).unwrap_or(0.0);
        player.open(path.to_string_lossy().into_owned(), start, true);
        let duration = self.recent.positions.get(path).map_or(0.0, |p| p.1);
        self.recent.touch(Item::Video { path: path.to_owned(), position: start, duration });
        self.recent.save(&self.config.dirs);
        // A show's episodes share its languages; a film keeps its own.
        let episodes = shelf.is_some_and(|i| self.library.shelves[i].videos.len() > 1);
        let key = match path.parent() {
            Some(folder) if episodes => folder.to_string_lossy().into_owned(),
            _ => path.to_string_lossy().into_owned(),
        };
        *self.film_clock.lock().unwrap_or_else(|e| e.into_inner()) = Some(player.clock());
        self.film_items += 1;
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
            info: None,
            audio: None,
            subtitles: None,
            cues: Cues::default(),
            cue_timer: None,
            key,
            item: self.film_items,
            seeks: 0,
        });
        self.go(Screen::Film, cx);
        cx.hide_cursor_until_moved();
        self.publish_film();
    }

    pub fn film_event(&mut self, e: FilmEvent, cx: &mut Cx<Msg>) {
        let Some(f) = &mut self.film else { return };
        match e {
            FilmEvent::Opened(info) => {
                f.duration = info.duration.unwrap_or(0.0);
                f.display = info.video.as_ref().map(|v| v.display);
                f.info = Some(info);
                self.apply_languages();
                self.publish_film();
            }
            FilmEvent::Selected { audio, subtitles } => {
                f.audio = audio;
                f.subtitles = subtitles;
            }
            FilmEvent::Cues(cues) => {
                f.cues.add(cues);
                self.schedule_cues(cx);
            }
            FilmEvent::ClearCues => f.cues.clear(),
            FilmEvent::Seeked(_) => {
                f.seeks += 1;
                self.schedule_cues(cx);
                self.publish_film();
            }
            FilmEvent::State(PlayState::Playing) => {
                f.playing = true;
                self.schedule_cues(cx);
                self.publish_film();
            }
            FilmEvent::State(PlayState::Paused) => {
                f.playing = false;
                f.bar = true;
                self.publish_film();
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
            if let Some(t) = f.cue_timer {
                cx.cancel(t);
            }
            f.player.close();
        }
        *self.film_clock.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.publish_film();
        self.after_activity(cx);
    }

    // Subtitles and languages.

    /// Ask to draw again when what the subtitles show next changes.
    pub fn schedule_cues(&mut self, cx: &mut Cx<Msg>) {
        let Some(f) = &mut self.film else { return };
        if let Some(t) = f.cue_timer.take() {
            cx.cancel(t);
        }
        if !f.playing {
            return;
        }
        let pos = f.player.position();
        if let Some(next) = f.cues.next_change(pos) {
            f.cue_timer = Some(cx.after(Duration::from_secs_f64(next - pos + 0.002), Msg::CueTick));
        }
    }

    /// The subtitles showing now, for the scene.
    pub fn film_subtitles(&self) -> Option<ui::Subtitles> {
        let f = self.film.as_ref()?;
        let cues: Vec<aizome_media::Cue> = f.cues.at(f.player.position()).into_iter().cloned().collect();
        (!cues.is_empty()).then(|| ui::Subtitles { picture: f.display.unwrap_or((16.0, 9.0)), cues })
    }

    /// The film's menu: Keep watching, the subtitles and the language
    /// where there's a choice, Stop watching; and its words.
    pub fn film_menu(&self) -> (Vec<FilmItem>, Vec<String>) {
        let mut items = vec![(FilmItem::Keep, "Keep watching".to_owned())];
        if let Some(f) = &self.film
            && let Some(info) = &f.info
        {
            if !info.subtitles.is_empty() {
                let now = f.subtitles.map_or_else(|| "Off".to_owned(), |i| track_name(&info.subtitles, i));
                items.push((FilmItem::Subtitles, format!("Subtitles: {now}")));
            }
            if info.audio.len() > 1 {
                let now = f.audio.map_or_else(String::new, |i| track_name(&info.audio, i));
                items.push((FilmItem::Language, format!("Language: {now}")));
            }
        }
        items.push((FilmItem::Stop, "Stop watching".to_owned()));
        items.into_iter().unzip()
    }

    /// A choice from the film's menu.
    pub fn film_menu_chosen(&mut self, choice: usize, cx: &mut Cx<Msg>) {
        match self.film_menu().0.get(choice) {
            Some(FilmItem::Keep) | None => self.close_modal(cx),
            Some(FilmItem::Subtitles) => self.open_menu(super::MenuKind::Subtitles, cx),
            Some(FilmItem::Language) => self.open_menu(super::MenuKind::Audio, cx),
            Some(FilmItem::Stop) => {
                self.modal = None;
                self.stop_film(cx);
            }
        }
    }

    /// The choices for the subtitles (Off, then each track) or the sound
    /// (each track), and which is playing.
    pub fn track_choices(&self, subtitles: bool) -> (Vec<String>, usize) {
        let Some(f) = &self.film else { return (Vec::new(), 0) };
        let Some(info) = &f.info else { return (Vec::new(), 0) };
        let names = |tracks: &[Track]| (0..tracks.len()).map(|i| track_name(tracks, i)).collect::<Vec<_>>();
        if subtitles {
            let mut v = vec!["Off".to_owned()];
            v.extend(names(&info.subtitles));
            (v, f.subtitles.map_or(0, |i| i + 1))
        } else {
            (names(&info.audio), f.audio.unwrap_or(0))
        }
    }

    /// A track chosen: played, and kept for the show (or the film).
    pub fn track_chosen(&mut self, subtitles: bool, choice: usize, cx: &mut Cx<Msg>) {
        self.modal = None;
        if let Some(f) = &self.film
            && let Some(info) = &f.info
        {
            let kept = self.recent.languages.entry(f.key.clone()).or_default();
            if subtitles {
                let track = choice.checked_sub(1).filter(|i| *i < info.subtitles.len());
                f.player.select_subtitles(track);
                kept.subtitles = Some(match track {
                    Some(i) => Subtitles::On(Pick::of(&info.subtitles, i)),
                    None => Subtitles::Off,
                });
            } else if choice < info.audio.len() {
                f.player.select_audio(Some(choice));
                kept.audio = Some(Pick::of(&info.audio, choice));
            }
            self.recent.save(&self.config.dirs);
        }
        self.close_modal(cx);
    }

    /// The film just opened: the languages chosen for its show before,
    /// where it has them.
    fn apply_languages(&mut self) {
        let Some(f) = &self.film else { return };
        let Some(info) = &f.info else { return };
        let Some(kept) = self.recent.languages.get(&f.key) else { return };
        let Languages { audio, subtitles } = kept;
        if let Some(i) = audio.as_ref().and_then(|p| p.find(&info.audio)) {
            f.player.select_audio(Some(i));
        }
        match subtitles {
            Some(Subtitles::Off) => f.player.select_subtitles(None),
            Some(Subtitles::On(p)) => {
                if let Some(i) = p.find(&info.subtitles) {
                    f.player.select_subtitles(Some(i));
                }
            }
            None => {}
        }
    }

    // The desktop's media controls.

    /// Tell the desktop's media controls (the menu bar's, a keyboard's or
    /// a remote's media keys) what plays: the film, or nothing.
    pub fn publish_film(&self) {
        let Some(m) = &self.mpris else { return };
        let Some(f) = &self.film else {
            m.publish(aizome_mpris::State::default());
            return;
        };
        let poster = f.shelf.and_then(|i| self.library.shelves.get(i)?.poster.clone());
        m.publish(aizome_mpris::State {
            item: f.item,
            title: f.title.clone(),
            url: Some(aizome_mpris::url_of(&f.path)),
            art: poster.as_deref().map(aizome_mpris::url_of),
            status: if f.playing { "Playing" } else { "Paused" },
            position: f.player.position(),
            duration: f.duration,
            volume: f64::from(self.film_volume),
            can_next: self.neighbour_episode(true).is_some(),
            can_previous: self.neighbour_episode(false).is_some(),
            can_seek: f.info.as_ref().is_some_and(|i| i.seekable),
            seeks: f.seeks,
            ..aizome_mpris::State::default()
        });
    }

    /// What the desktop's media controls ask of the film.
    pub fn on_mpris(&mut self, a: aizome_mpris::Action, cx: &mut Cx<Msg>) {
        use aizome_mpris::Action;
        if a == Action::Raise {
            return cx.activate(self.window);
        }
        let Some(f) = &self.film else { return };
        match a {
            Action::Play => f.player.play(),
            Action::Pause => f.player.pause(),
            Action::Toggle if f.playing => f.player.pause(),
            Action::Toggle => f.player.play(),
            Action::Seek(t) => f.player.seek(t, false),
            Action::Volume(v) => {
                self.film_volume = v as f32;
                f.player.set_volume(self.film_volume, false);
                self.publish_film();
            }
            Action::Next | Action::Previous => {
                if let Some(n) = self.neighbour_episode(a == Action::Next) {
                    self.play_video(&n, cx);
                }
            }
            Action::Stop => {
                self.modal = None;
                self.stop_film(cx);
            }
            // One speed, no repeating, nothing opened from outside, and
            // leaving takes the PIN.
            Action::Rate(_) | Action::Loop(_) | Action::Open(_) | Action::Quit | Action::Raise => {}
        }
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

/// A track as the menus name it, the engine's way: its language or title,
/// how many channels, whether forced ("English · 5.1"), else "Track 2".
fn track_name(tracks: &[Track], i: usize) -> String {
    tracks.get(i).map_or_else(String::new, |t| t.label(i + 1))
}
