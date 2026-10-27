//! The greeter as a kit program: a surface over every display, the people
//! to pick from, the password field, the session and power menus, and the
//! conversation with greetd (on a thread of its own).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{Anchors, Image, KeyboardInteractivity, Layer, LayerOptions, Margins, Menu, MenuItem, MenuStyle};

use crate::clock::Now;
use crate::config::{Config, State};
use crate::greetd::{self, Request, Response};
use crate::login::{Launch, Login, Step};
use crate::sessions::Session;
use crate::users::User;

/// Where the login is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Picking a person from the row.
    People,
    /// The picked person's password (and any more questions).
    Password,
    /// Typing a user name (no list, or nobody to list).
    Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerVerb {
    Sleep,
    Restart,
    ShutDown,
}

impl PowerVerb {
    pub fn label(self) -> &'static str {
        match self {
            PowerVerb::Sleep => "Sleep",
            PowerVerb::Restart => "Restart",
            PowerVerb::ShutDown => "Shut Down",
        }
    }

    fn systemctl(self) -> &'static str {
        match self {
            PowerVerb::Sleep => "suspend",
            PowerVerb::Restart => "reboot",
            PowerVerb::ShutDown => "poweroff",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Nothing,
    /// greetd answered.
    Greetd(Response),
    /// The connection to greetd broke.
    GreetdLost(String),
    Password(String),
    Submit,
    Caps(bool),
    /// The user name typed (the `Name` stage).
    Name(String),
    SubmitName,
    /// A person picked in the row (by index), or the selection moved.
    Pick(usize),
    Select(usize),
    Next,
    Previous,
    /// Back to the people (Escape).
    People,
    /// Type a user name instead.
    Other,
    SessionMenu,
    PickSession(usize),
    PowerMenu,
    Power(PowerVerb),
    PowerFailed(String),
    Shake(u32),
    Tick,
    /// A display's wallpaper, scaled to its pixels (`None`: none reads).
    Wallpaper {
        output: u32,
        path: PathBuf,
        size: (u32, u32),
        rgba: Option<Vec<u8>>,
    },
    Avatar(String, Option<Image>),
}

/// A display's wallpaper: which file, at what size in pixels, and the
/// picture once it's ready.
type Wall = (PathBuf, (u32, u32), Option<Image>);

/// How the greetd thread asks: greetd itself, or the demo's stand-in.
type Link = Box<dyn FnMut(&Request) -> std::io::Result<Response>>;

/// The greeter.
pub struct Greeter {
    pub cfg: Config,
    pub state: State,
    pub users: Vec<User>,
    pub sessions: Vec<Session>,
    pub stage: Stage,
    /// The person in the row with the selection (the `People` stage) or
    /// logging in (the others).
    pub selected: usize,
    /// Each person's session, as picked (by id).
    pub session_of: HashMap<String, String>,
    pub password: String,
    pub name: String,
    pub caps: bool,
    pub login: Login,
    pub shake: u32,
    shaken: u32,
    pub now: Now,
    pub hostname: String,
    /// Each display's surface, by output id.
    pub windows: Vec<(WindowKey, u32)>,
    /// Each display's size in points, by output id.
    pub frames: HashMap<u32, Size>,
    /// The display with the login on it.
    pub main_output: Option<u32>,
    wallpapers: HashMap<u32, Wall>,
    pub avatars: HashMap<String, Image>,
    to_greetd: Option<mpsc::Sender<Request>>,
    /// Real greetd and real displays (not a test).
    pub live: bool,
    /// Pretend to be greetd (`--demo`): any user, the password `aizome`.
    pub demo: bool,
    pub power_error: Option<String>,
}

impl Greeter {
    pub fn new(cfg: Config, live: bool, demo: bool) -> Greeter {
        let state = State::load(&cfg.state);
        let users = crate::users::find(&cfg.hidden_users);
        let sessions = crate::sessions::find(&cfg.sessions, &cfg.hidden_sessions, !cfg.x11_prefix.is_empty());
        let hostname = cfg.hostname.clone().unwrap_or_else(hostname);
        Greeter::with(cfg, state, users, sessions, hostname, live, demo)
    }

    /// From what was read (tests give their own).
    pub fn with(
        cfg: Config,
        state: State,
        users: Vec<User>,
        sessions: Vec<Session>,
        hostname: String,
        live: bool,
        demo: bool,
    ) -> Greeter {
        let last = state.last_user.as_ref().and_then(|u| users.iter().position(|x| &x.name == u));
        let stage = if !cfg.user_list || users.is_empty() {
            Stage::Name
        } else if last.is_some() || users.len() == 1 {
            Stage::Password
        } else {
            Stage::People
        };
        Greeter {
            session_of: state.sessions.clone().into_iter().collect(),
            cfg,
            state,
            users,
            sessions,
            stage,
            selected: last.unwrap_or(0),
            password: String::new(),
            name: String::new(),
            caps: false,
            login: Login::default(),
            shake: 0,
            shaken: 0,
            now: Now::local(),
            hostname,
            windows: Vec::new(),
            frames: HashMap::new(),
            main_output: None,
            wallpapers: HashMap::new(),
            avatars: HashMap::new(),
            to_greetd: None,
            live,
            demo,
            power_error: None,
        }
    }

    /// The person logging in, or selected in the row.
    pub fn user(&self) -> Option<&User> {
        match self.stage {
            Stage::Name => None,
            _ => self.users.get(self.selected),
        }
    }

    /// The user name the login is for.
    fn username(&self) -> Option<String> {
        match self.stage {
            Stage::Name => self.login.user.clone(),
            _ => self.user().map(|u| u.name.clone()),
        }
    }

    /// The session picked for whoever is logging in: theirs from last
    /// time, else the default, else the first.
    pub fn session(&self) -> Option<&Session> {
        let by_id = |id: &str| self.sessions.iter().find(|s| s.id == id);
        self.username()
            .and_then(|u| self.session_of.get(&u).cloned())
            .and_then(|id| by_id(&id))
            .or_else(|| self.cfg.default_session.as_deref().and_then(by_id))
            .or_else(|| self.sessions.first())
    }

    fn launch(&self) -> Launch {
        match self.session() {
            Some(s) => Launch { cmd: s.command(&self.cfg.x11_prefix), env: s.env() },
            None => Launch { cmd: Vec::new(), env: Vec::new() },
        }
    }

    fn send(&mut self, requests: Vec<Request>) {
        for r in requests {
            match &self.to_greetd {
                Some(tx) => {
                    if tx.send(r).is_err() {
                        self.login.lost("its thread stopped");
                    }
                }
                // A test: what would be sent is the test's to answer.
                None => drop(r),
            }
        }
    }

    /// Begin the selected person's login (the `Password` stage).
    fn begin(&mut self) {
        greetd::zero_string(&mut self.password);
        if let Some(name) = self.user().map(|u| u.name.clone()) {
            let r = self.login.begin(&name);
            self.send(r);
        }
    }

    fn submit(&mut self, cx: &mut Cx<Msg>) {
        match self.login.step {
            Step::Ask { .. } => {
                let mut answer = std::mem::take(&mut self.password);
                if let Some(r) = self.login.answer(answer.clone()) {
                    self.send(vec![r]);
                }
                greetd::zero_string(&mut answer);
            }
            Step::Failed => self.begin(),
            _ => {}
        }
        let _ = cx;
    }

    fn heard(&mut self, response: Response, cx: &mut Cx<Msg>) {
        let launch = self.launch();
        let failed = self.login.failed;
        let next = self.login.heard(response, &launch);
        self.send(next);
        if self.login.failed != failed {
            greetd::zero_string(&mut self.password);
            self.shake = 1;
            self.shaken = 1;
            cx.after(Duration::from_millis(30), Msg::Shake(1));
        }
        if self.login.step == Step::Done {
            if let (Some(user), Some(session)) = (self.username(), self.session().map(|s| s.id.clone())) {
                self.state.logged_in(&user, &session);
                if self.live
                    && let Err(e) = self.state.save(&self.cfg.state)
                {
                    log::warn!("remembering {user}'s session: {e}");
                }
            }
            if self.demo {
                log::info!("would start {:?} with {:?}", launch.cmd, launch.env);
            }
            cx.quit();
        }
    }

    fn pick(&mut self, i: usize, cx: &mut Cx<Msg>) {
        if i >= self.users.len() {
            return;
        }
        self.selected = i;
        self.stage = Stage::Password;
        self.login.message = None;
        self.begin();
        cx.focus("greeter-password");
    }

    fn back_to_people(&mut self, cx: &mut Cx<Msg>) {
        if self.users.len() < 2 || !self.cfg.user_list {
            return;
        }
        greetd::zero_string(&mut self.password);
        self.login.message = None;
        self.stage = Stage::People;
        // Whoever was half logged in: greetd lets go.
        let cancel = self.login.cancel();
        self.send(cancel);
        cx.focus("greeter-people");
    }

    fn session_menu(&mut self, cx: &mut Cx<Msg>) {
        let Some(win) = self.main_window() else { return };
        let Some(r) = cx.element_rect(win, "greeter-session") else { return };
        // Its names under the button's, as a pop-up button's menu: the
        // menu's check-mark column (28.5pt to its text) less the button's
        // own 8pt.
        let anchor = r - Vec2::new(MENU_TEXT_INSET - 8.0, 0.0);
        let current = self.session().map(|s| s.id.clone());
        let menu =
            Menu::new("Sessions").items(self.sessions.iter().enumerate().map(|(i, s)| {
                MenuItem::new(s.name.clone(), Msg::PickSession(i)).checked(current.as_ref() == Some(&s.id))
            }));
        cx.show_menu(win, anchor, menu, MenuStyle::below());
    }

    fn power_menu(&mut self, cx: &mut Cx<Msg>) {
        if !self.cfg.power {
            return;
        }
        let Some(win) = self.main_window() else { return };
        let Some(anchor) = cx.element_rect(win, "greeter-power") else { return };
        let menu = Menu::new("Power").items(
            [PowerVerb::Sleep, PowerVerb::Restart, PowerVerb::ShutDown]
                .into_iter()
                .map(|v| MenuItem::new(v.label(), Msg::Power(v))),
        );
        cx.show_menu(win, anchor, menu, MenuStyle::below());
    }

    fn power(&mut self, verb: PowerVerb, cx: &mut Cx<Msg>) {
        if !self.live {
            return;
        }
        // logind decides (polkit lets the person at the login screen).
        cx.spawn(move || match std::process::Command::new("systemctl").arg(verb.systemctl()).output() {
            Ok(o) if o.status.success() => Msg::Nothing,
            Ok(o) => Msg::PowerFailed(String::from_utf8_lossy(&o.stderr).trim().to_owned()),
            Err(e) => Msg::PowerFailed(e.to_string()),
        });
    }

    pub fn main_window(&self) -> Option<WindowKey> {
        self.windows.iter().find(|(_, o)| Some(*o) == self.main_output).map(|(w, _)| *w)
    }

    /// A surface on every display, the login on the first.
    fn sync_windows(&mut self, cx: &mut Cx<Msg>) {
        let outputs = cx.outputs();
        let ids: Vec<u32> = outputs.iter().map(|o| o.id).collect();
        self.frames = outputs.iter().map(|o| (o.id, o.frame.size())).collect();
        for (w, o) in std::mem::take(&mut self.windows) {
            if ids.contains(&o) {
                self.windows.push((w, o));
            } else {
                cx.close_window(w);
            }
        }
        let main = self.main_output.filter(|m| ids.contains(m)).or_else(|| ids.first().copied());
        let main_changed = main != self.main_output;
        self.main_output = main;
        if main_changed {
            // Keyboard and all: open each afresh as main or not.
            for (w, _) in std::mem::take(&mut self.windows) {
                cx.close_window(w);
            }
        }
        for o in &outputs {
            if self.windows.iter().any(|(_, x)| *x == o.id) {
                continue;
            }
            let is_main = Some(o.id) == self.main_output;
            let opts = LayerOptions {
                layer: Layer::Overlay,
                namespace: "aizome-greeter".into(),
                anchors: Anchors::ALL,
                exclusive_zone: -1,
                margins: Margins::default(),
                keyboard: if is_main { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None },
                output: Some(o.id),
            };
            let mut spec = WindowSpec::layer(opts, 0.0, 0.0);
            spec.title = if is_main { "Login".into() } else { format!("Login on {}", o.name) };
            let w = cx.open_window(spec);
            self.windows.push((w, o.id));
            self.load_wallpaper(o.id, o.frame.size(), o.scale, cx);
        }
    }

    /// Which wallpaper shows now.
    fn wallpaper_path(&self) -> Option<PathBuf> {
        let day = match self.cfg.wallpaper.as_str() {
            "day" => true,
            "night" => false,
            _ => self.now.is_day(),
        };
        if day { self.cfg.wallpaper_day.clone() } else { self.cfg.wallpaper_night.clone() }
    }

    fn load_wallpaper(&mut self, output: u32, size: Size, scale: f64, cx: &mut Cx<Msg>) {
        let Some(path) = self.wallpaper_path() else { return };
        let px = ((size.width * scale).round() as u32, (size.height * scale).round() as u32);
        if self.wallpapers.get(&output).is_some_and(|(p, s, _)| *p == path && *s == px) {
            return;
        }
        self.wallpapers.insert(output, (path.clone(), px, None));
        cx.spawn(move || {
            let rgba = aizome_face::wallpaper::prepare(&path, px.0, px.1);
            Msg::Wallpaper { output, path, size: px, rgba }
        });
    }

    pub fn wallpaper(&self, output: u32) -> Option<Image> {
        self.wallpapers.get(&output).and_then(|(_, _, i)| i.clone())
    }

    fn load_avatars(&mut self, cx: &mut Cx<Msg>) {
        for u in &self.users {
            if let Some(path) = u.icon.clone() {
                let name = u.name.clone();
                cx.spawn(move || Msg::Avatar(name, Image::open(&path).ok()));
            }
        }
    }

    fn start_greetd(&mut self, cx: &mut Cx<Msg>) {
        if !self.live {
            return;
        }
        let (tx, rx) = mpsc::channel::<Request>();
        let out = cx.stream();
        let demo = self.demo;
        std::thread::Builder::new()
            .name("greetd".into())
            .spawn(move || {
                let mut link: Link = if demo {
                    Box::new(greetd::Demo.into_fn())
                } else {
                    match greetd::Greetd::connect() {
                        Ok(mut g) => Box::new(move |r: &Request| g.ask(r)),
                        Err(e) => {
                            out.send(Msg::GreetdLost(e.to_string()));
                            return;
                        }
                    }
                };
                while let Ok(mut request) = rx.recv() {
                    let answer = link(&request);
                    if let Request::PostAuthMessageResponse { response: Some(s) } = &mut request {
                        greetd::zero_string(s);
                    }
                    match answer {
                        Ok(r) => {
                            if !out.send(Msg::Greetd(r)) {
                                return;
                            }
                        }
                        Err(e) => {
                            out.send(Msg::GreetdLost(e.to_string()));
                            return;
                        }
                    }
                }
            })
            .ok();
        self.to_greetd = Some(tx);
    }
}

impl Program for Greeter {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Greeter", "Login", "Login Screen")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.set_appearance(aizome_kit::theme::Appearance::default());
        self.start_greetd(cx);
        self.sync_windows(cx);
        self.load_avatars(cx);
        if self.stage == Stage::Password {
            self.begin();
            cx.focus("greeter-password");
        } else if self.stage == Stage::People {
            cx.focus("greeter-people");
        } else {
            cx.focus("greeter-name");
        }
        cx.after(self.now.until_next_minute(), Msg::Tick);
    }

    fn outputs_changed(&mut self, cx: &mut Cx<Msg>) {
        self.sync_windows(cx);
        for o in cx.outputs() {
            self.load_wallpaper(o.id, o.frame.size(), o.scale, cx);
        }
    }

    fn close_requested(&mut self, _window: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        false
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Nothing => {}
            Msg::Greetd(r) => self.heard(r, cx),
            Msg::GreetdLost(why) => {
                log::error!("greetd: {why}");
                self.login.lost(&why);
                if self.live && !self.demo {
                    // greetd starts the greeter again.
                    cx.quit();
                }
            }
            Msg::Password(p) => {
                greetd::zero_string(&mut self.password);
                self.password = p;
            }
            Msg::Submit => self.submit(cx),
            Msg::Caps(on) => self.caps = on,
            Msg::Name(n) => self.name = n,
            Msg::SubmitName => {
                let name = self.name.trim().to_owned();
                if !name.is_empty() {
                    let r = self.login.begin(&name);
                    self.send(r);
                    cx.focus("greeter-name");
                }
            }
            // Return in the row: whoever has the selection.
            Msg::Pick(i) => self.pick(if i == usize::MAX { self.selected } else { i }, cx),
            Msg::Select(i) => self.selected = i.min(self.users.len().saturating_sub(1)),
            Msg::Next => {
                if !self.users.is_empty() {
                    self.selected = (self.selected + 1) % self.users.len();
                }
            }
            Msg::Previous => {
                if !self.users.is_empty() {
                    self.selected = (self.selected + self.users.len() - 1) % self.users.len();
                }
            }
            Msg::People => self.back_to_people(cx),
            Msg::Other => {
                greetd::zero_string(&mut self.password);
                self.stage = Stage::Name;
                let cancel = self.login.cancel();
                self.send(cancel);
                cx.focus("greeter-name");
            }
            Msg::SessionMenu => self.session_menu(cx),
            Msg::PickSession(i) => {
                if let (Some(user), Some(s)) = (self.username(), self.sessions.get(i)) {
                    self.session_of.insert(user, s.id.clone());
                }
                if let Some(w) = self.main_window() {
                    cx.close_menus(w);
                }
                cx.focus("greeter-password");
            }
            Msg::PowerMenu => self.power_menu(cx),
            Msg::Power(v) => self.power(v, cx),
            Msg::PowerFailed(e) => self.power_error = Some(e),
            Msg::Shake(step) => {
                if step == self.shaken && step < 8 {
                    self.shake = step + 1;
                    self.shaken = step + 1;
                    cx.after(Duration::from_millis(30), Msg::Shake(step + 1));
                } else if step >= 8 {
                    self.shake = 0;
                }
            }
            Msg::Tick => {
                let was_day = self.now.is_day();
                self.now = Now::local();
                if was_day != self.now.is_day() {
                    for o in cx.outputs() {
                        self.load_wallpaper(o.id, o.frame.size(), o.scale, cx);
                    }
                }
                cx.after(self.now.until_next_minute(), Msg::Tick);
            }
            Msg::Wallpaper { output, path, size, rgba } => {
                let scale = cx.outputs().iter().find(|o| o.id == output).map(|o| o.scale).unwrap_or(1.0);
                if let Some(entry) = self.wallpapers.get_mut(&output)
                    && entry.0 == path
                    && entry.1 == size
                {
                    entry.2 = rgba.and_then(|px| aizome_face::wallpaper::image(&px, size.0, size.1, scale));
                }
            }
            Msg::Avatar(name, image) => {
                if let Some(i) = image {
                    self.avatars.insert(name, i);
                }
            }
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        let output = self.windows.iter().find(|(w, _)| *w == window).map(|(_, o)| *o);
        crate::view::view(self, window, output)
    }
}

/// The host name, from the kernel.
fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|s| s.trim().to_owned()).unwrap_or_default()
}

/// Where a kit menu's item text begins, from its left edge.
const MENU_TEXT_INSET: f64 = 28.5;

/// The field's sideways offset at a step of the shake (the lock screen's).
pub fn shake_offset(step: u32) -> f64 {
    const STEPS: [f64; 9] = [0.0, -10.0, 9.0, -7.0, 6.0, -4.0, 3.0, -1.0, 0.0];
    STEPS.get(step as usize).copied().unwrap_or(0.0)
}
