//! Annai, the program: which page shows, the scene's clock, and "Show at
//! login".

use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, NamedKey, PointerButton};
use aizome_kit::widget::{Event, FocusRing};

use crate::art::{self, Art};
use crate::state::{self, State};

pub const APP_ID: &str = "org.aizome.Annai";

/// The window, fixed: the scene above, the words, the buttons.
pub const WIDTH: f64 = 760.0;
pub const HEIGHT: f64 = 600.0;
pub const SCENE_HEIGHT: f64 = 290.0;

/// A frame of the scenes, 30 a second.
pub const FRAME: Duration = Duration::from_millis(33);

/// The pages.
pub const PAGES: usize = 7;

#[derive(Clone, Debug)]
pub enum Msg {
    Next,
    Back,
    Go(usize),
    Tick,
    ShowAtLogin(bool),
    /// Open Settei at a pane.
    Settings(&'static str),
    Done,
    Raise,
}

pub struct Annai {
    pub window: WindowKey,
    pub page: usize,
    /// Milliseconds into the page's scene.
    pub t: u32,
    ticker: Option<TimerHandle>,
    pub motion: bool,
    pub state: State,
    /// Where the state is kept (none: nowhere, for tests).
    pub state_path: Option<PathBuf>,
    /// Programs started (Settei at a pane), and whether to really start
    /// them.
    pub commands: Vec<Vec<String>>,
    pub dry_run: bool,
    pub instance: Option<std::os::unix::net::UnixListener>,
}

impl Annai {
    pub fn new(state_path: Option<PathBuf>) -> Annai {
        let state = state_path.as_deref().map(state::load).unwrap_or_default();
        Annai {
            window: WindowKey::NONE,
            page: 0,
            t: 0,
            ticker: None,
            motion: true,
            state,
            state_path,
            commands: Vec::new(),
            dry_run: false,
            instance: None,
        }
    }

    fn save(&self) {
        if let Some(p) = &self.state_path
            && let Err(e) = state::save(p, &self.state)
        {
            log::warn!("{}: {e}", p.display());
        }
    }

    /// Show page `p`, its scene from the start (or its key moment).
    fn go(&mut self, p: usize) {
        self.page = p.min(PAGES - 1);
        self.t = if self.motion { 0 } else { art::key_ms(self.page) };
    }

    /// The scene as it shows now.
    pub fn art(&self) -> Art {
        Art { page: self.page, t: self.t }
    }

    fn run(&mut self, args: Vec<String>) {
        if !self.dry_run
            && let Some((cmd, rest)) = args.split_first()
            && let Err(e) = std::process::Command::new(cmd).args(rest).spawn()
        {
            log::warn!("{cmd}: {e}");
        }
        self.commands.push(args);
    }
}

impl Program for Annai {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Annai", "Welcome").icon(Icon::BookOpen).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(
            WindowSpec::new("Welcome to Aizome")
                .size(WIDTH, HEIGHT)
                .min_size(WIDTH, HEIGHT)
                .max_size(WIDTH, HEIGHT)
                .resizable(false),
        );
        // Shown: the first run is over.
        if self.state.shown.is_none() {
            self.state.shown = Some(state::now());
            self.save();
        }
        self.motion = cx.theme().motion();
        self.go(self.page);
        if self.motion {
            self.ticker = Some(cx.every(FRAME, Msg::Tick));
        }
        cx.focus("scene");
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, cx.stream(), || Msg::Raise);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Next => {
                if self.page + 1 < PAGES {
                    self.go(self.page + 1);
                } else {
                    cx.close_window(self.window);
                }
            }
            Msg::Back => {
                if self.page > 0 {
                    self.go(self.page - 1);
                }
            }
            Msg::Go(p) => self.go(p),
            Msg::Tick => {
                // Reduced motion may have come on since.
                if !cx.theme().motion() {
                    if let Some(h) = self.ticker.take() {
                        cx.cancel(h);
                    }
                    self.motion = false;
                    self.t = art::key_ms(self.page);
                } else {
                    self.t = (self.t + FRAME.as_millis() as u32) % art::loop_ms(self.page);
                }
            }
            Msg::ShowAtLogin(on) => {
                self.state.show_at_login = on;
                self.save();
            }
            Msg::Settings(pane) => self.run(vec!["aizome-settei".into(), "--pane".into(), pane.into()]),
            Msg::Done => cx.close_window(self.window),
            Msg::Raise => cx.activate(self.window),
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        let scene = canvas(self.art(), art::paint)
            .on_event(scene_event)
            .focusable()
            .focus_ring(FocusRing::Never)
            .id("scene")
            .label(crate::pages::PAGES[self.page].title)
            .height(SCENE_HEIGHT);
        column![
            container(scene).edges(Edge::BevelLo, Sides::BOTTOM).height(SCENE_HEIGHT + 1.0),
            crate::pages::words(self.page),
            crate::pages::footer(self),
        ]
        .into()
    }

    fn menus(&self) -> MenuBar<Msg> {
        let mut go = Menu::new("Go")
            .item(
                MenuItem::new("Previous Page", Msg::Back)
                    .shortcut(Shortcut::key(ShortcutKey::Left))
                    .enabled(self.page > 0),
            )
            .item(
                MenuItem::new("Next Page", Msg::Next)
                    .shortcut(Shortcut::key(ShortcutKey::Right))
                    .enabled(self.page + 1 < PAGES),
            )
            .separator();
        for (i, p) in crate::pages::PAGES.iter().enumerate() {
            let digit = char::from(b'1' + i as u8);
            go = go.item(MenuItem::new(p.title, Msg::Go(i)).shortcut(Shortcut::cmd(digit)).checked(self.page == i));
        }
        MenuBar::standard(&self.info())
            .with_file_menu(Menu::new("File").item(MenuItem::new("Close", Msg::Done).shortcut(Shortcut::cmd('W'))))
            .with_menu(go)
            .extend("Help", [MenuItem::separator(), MenuItem::new("Aizome Tour", Msg::Go(0))])
    }
}

/// Keys and clicks over the scene: ←/→ step, Return goes on, Escape
/// closes; a click goes on too.
fn scene_event(ev: &Event, _size: Size, _art: &Art) -> Option<Msg> {
    match ev {
        Event::Key(k) if k.pressed => {
            let m = k.modifiers;
            if m.super_key() || m.control() || m.alt() {
                return None;
            }
            match &k.key {
                Key::Named(NamedKey::ArrowRight | NamedKey::Enter | NamedKey::PageDown) => Some(Msg::Next),
                Key::Named(NamedKey::ArrowLeft | NamedKey::PageUp) => Some(Msg::Back),
                Key::Named(NamedKey::Home) => Some(Msg::Go(0)),
                Key::Named(NamedKey::End) => Some(Msg::Go(PAGES - 1)),
                Key::Named(NamedKey::Escape) => Some(Msg::Done),
                Key::Character(s) if s == " " => Some(if m.shift() { Msg::Back } else { Msg::Next }),
                _ => None,
            }
        }
        Event::PointerDown { button: PointerButton::Left, .. } => None,
        _ => None,
    }
}
