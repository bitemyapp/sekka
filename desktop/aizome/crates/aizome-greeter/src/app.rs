//! The greeter as a kit program: a surface over every display, the people
//! to pick from, the password field, the session and power menus, and the
//! conversation with greetd (on a thread of its own).
// Every stage is routed by name: a new one forces a decision.
#![deny(clippy::wildcard_enum_match_arm)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{Anchors, Image, KeyboardInteractivity, Layer, LayerOptions, Margins, Menu, MenuItem, MenuStyle};

use crate::clock::Now;
use crate::config::{Config, State};
use crate::greetd::{self, Request, Response};
use crate::login::{Launch, Login, LoginAny, Outcome, Returned, Screen, Step, Went, Who};
use crate::sessions::Session;
use crate::users::User;

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
    /// Return: in the row, the name or the field.
    Submit,
    Caps(bool),
    /// The user name typed (the `Name` stage).
    Name(String),
    /// A person clicked in the row (by index), or the selection moved.
    Pick(usize),
    Select(usize),
    Next,
    Previous,
    /// Escape, or "Log in as someone else": back to where the person was
    /// chosen.
    Back,
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

/// The greeter. Its fields are its own: the view reads what it draws
/// through the methods below, and the stage only as the login's
/// [`Screen`]. The view once decided the way back from the configuration's
/// `user_list`, the number of people and its own idea of the stage; none
/// of those is the view's to read now (SVC-05):
///
/// ```compile_fail,E0616
/// use aizome_greeter::app::Greeter;
/// fn others(g: &Greeter, typed: bool) -> bool {
///     g.cfg.user_list && g.users.len() > 1 && !typed
/// }
/// ```
///
/// ```
/// use aizome_greeter::app::Greeter;
/// use aizome_greeter::login::{EscapeTo, Screen};
/// fn others(g: &Greeter) -> bool {
///     match g.login().screen() {
///         Screen::Login { way_back, .. } => way_back.to() != EscapeTo::Nowhere,
///         Screen::Unstarted | Screen::People { .. } | Screen::Name { .. } => false,
///     }
/// }
/// ```
pub struct Greeter {
    cfg: Config,
    state: State,
    sessions: Vec<Session>,
    /// Who's logging in, and how far greetd has got.
    login: Login,
    /// The people's pictures to read (AccountsService's), by user name.
    icons: Vec<(String, PathBuf)>,
    /// Each person's session, as picked (by id).
    session_of: HashMap<String, String>,
    /// What's in the password field (the view builds the field from it).
    pub(crate) password: String,
    caps: bool,
    shake: u32,
    shaken: u32,
    now: Now,
    hostname: String,
    /// Each display's surface, by output id.
    windows: Vec<(WindowKey, u32)>,
    /// Each display's size in points, by output id.
    frames: HashMap<u32, Size>,
    /// The display with the login on it.
    main_output: Option<u32>,
    wallpapers: HashMap<u32, Wall>,
    avatars: HashMap<String, Image>,
    to_greetd: Option<mpsc::Sender<Request>>,
    /// Real greetd and real displays (not a test).
    live: bool,
    /// Pretend to be greetd (`--demo`): any user, the password `aizome`.
    demo: bool,
    power_error: Option<String>,
}

/// The session picked for `user`: theirs from last time, else the
/// default, else the first.
fn session_for<'a>(
    sessions: &'a [Session],
    session_of: &HashMap<String, String>,
    cfg: &Config,
    user: Option<&str>,
) -> Option<&'a Session> {
    let by_id = |id: &str| sessions.iter().find(|s| s.id == id);
    user.and_then(|u| session_of.get(u))
        .and_then(|id| by_id(id))
        .or_else(|| cfg.default_session.as_deref().and_then(by_id))
        .or_else(|| sessions.first())
}

/// What to start for `user` once they're authenticated.
fn launch_for(sessions: &[Session], session_of: &HashMap<String, String>, cfg: &Config, user: &str) -> Launch {
    match session_for(sessions, session_of, cfg, Some(user)) {
        Some(s) => Launch { cmd: s.command(&cfg.x11_prefix), env: s.env() },
        None => Launch { cmd: Vec::new(), env: Vec::new() },
    }
}

/// What Return does: picks in the row, logs in the name typed, answers
/// PAM's question, or begins a failed login again. Nothing while greetd is
/// asked or the session starts.
pub(crate) fn on_return(state: LoginAny<'_>) -> Returned<'_> {
    match state {
        LoginAny::Unstarted(unstarted) => unstarted.busy(),
        LoginAny::People(people) => people.pick(),
        LoginAny::Name(name) => name.submit(),
        LoginAny::ChosenLogin(login) => step_return(login.step()),
        LoginAny::OnlyLogin(login) => step_return(login.step()),
        LoginAny::TypedLogin(login) => step_return(login.step()),
    }
}

fn step_return<'s, W: Who>(step: Step<'s, W>) -> Returned<'s> {
    match step {
        Step::Waiting(waiting) => waiting.busy(),
        Step::Asking(asking) => asking.answer(),
        Step::Starting(starting) => starting.busy(),
        Step::Failed(failed) => failed.retry(),
        Step::Done(done) => done.busy(),
    }
}

/// What Escape does: back to where the person was chosen (the row, or the
/// name as typed). Nothing where choosing starts, for the only person
/// listed, or once the session starts.
pub(crate) fn on_escape(state: LoginAny<'_>) -> Went<'_> {
    match state {
        LoginAny::Unstarted(unstarted) => unstarted.stay(),
        LoginAny::People(people) => people.stay(),
        LoginAny::Name(name) => name.stay(),
        LoginAny::ChosenLogin(login) => match login.step() {
            Step::Waiting(waiting) => waiting.back().into(),
            Step::Asking(asking) => asking.back().into(),
            Step::Failed(failed) => failed.back().into(),
            Step::Starting(starting) => starting.stay(),
            Step::Done(done) => done.stay(),
        },
        LoginAny::OnlyLogin(login) => match login.step() {
            Step::Waiting(waiting) => waiting.stay(),
            Step::Asking(asking) => asking.stay(),
            Step::Failed(failed) => failed.stay(),
            Step::Starting(starting) => starting.stay(),
            Step::Done(done) => done.stay(),
        },
        LoginAny::TypedLogin(login) => match login.step() {
            Step::Waiting(waiting) => waiting.back().into(),
            Step::Asking(asking) => asking.back().into(),
            Step::Failed(failed) => failed.back().into(),
            Step::Starting(starting) => starting.stay(),
            Step::Done(done) => done.stay(),
        },
    }
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
        let icons = users.iter().filter_map(|u| Some((u.name.clone(), u.icon.clone()?))).collect();
        Greeter {
            session_of: state.sessions.clone().into_iter().collect(),
            cfg,
            state,
            sessions,
            login: Login::with_users(users),
            icons,
            password: String::new(),
            caps: false,
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

    /// Who's logging in, and how far greetd has got.
    pub fn login(&self) -> &Login {
        &self.login
    }

    /// What's in the password field.
    pub fn password(&self) -> &str {
        &self.password
    }

    /// What's remembered between logins.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// The step of the field's shake (0: still).
    pub fn shake(&self) -> u32 {
        self.shake
    }

    /// The session picked for whoever is logging in: theirs from last
    /// time, else the default, else the first.
    pub fn session(&self) -> Option<&Session> {
        session_for(&self.sessions, &self.session_of, &self.cfg, self.login.user())
    }

    /// Whether there's a choice of sessions to offer.
    pub(crate) fn several_sessions(&self) -> bool {
        self.sessions.len() > 1
    }

    /// Whether the power menu is offered.
    pub(crate) fn offers_power(&self) -> bool {
        self.cfg.power
    }

    pub(crate) fn hostname(&self) -> &str {
        &self.hostname
    }

    pub(crate) fn now(&self) -> &Now {
        &self.now
    }

    pub(crate) fn caps(&self) -> bool {
        self.caps
    }

    pub(crate) fn power_error(&self) -> Option<&str> {
        self.power_error.as_deref()
    }

    /// `user`'s picture, once it's read.
    pub(crate) fn avatar(&self, user: &str) -> Option<Image> {
        self.avatars.get(user).cloned()
    }

    /// A display's size in points.
    pub(crate) fn frame(&self, output: u32) -> Option<Size> {
        self.frames.get(&output).copied()
    }

    /// Whether the login is on display `output`.
    pub(crate) fn is_main(&self, output: u32) -> bool {
        self.main_output == Some(output)
    }

    /// Sends greetd what the login asks. If its thread has stopped, greetd
    /// is gone, as when the link reports it.
    fn send(&mut self, requests: Vec<Request>, cx: &mut Cx<Msg>) {
        for r in requests {
            match &self.to_greetd {
                Some(tx) => {
                    if tx.send(r).is_err() {
                        self.lost("its thread stopped", cx);
                        return;
                    }
                }
                // A test: what would be sent is the test's to answer.
                None => drop(r),
            }
        }
    }

    /// The connection to greetd is gone: say so, and quit when it's real
    /// greetd (it starts the greeter again; nothing here can reach it).
    fn lost(&mut self, why: &str, cx: &mut Cx<Msg>) {
        log::error!("greetd: {why}");
        self.login.lost(why);
        if self.live && !self.demo {
            cx.quit();
        }
    }

    /// The keyboard to what the stage shows.
    fn focus(&self, cx: &mut Cx<Msg>) {
        match self.login.screen() {
            Screen::People { .. } => cx.focus("greeter-people"),
            Screen::Name { .. } => cx.focus("greeter-name"),
            Screen::Login { .. } => cx.focus("greeter-password"),
            Screen::Unstarted => {}
        }
    }

    /// Return, in whatever the stage shows.
    fn submit(&mut self, cx: &mut Cx<Msg>) {
        let requests = self.login.on_return(&mut self.password, on_return);
        self.send(requests, cx);
        self.focus(cx);
    }

    /// Escape, or "Log in as someone else".
    fn back(&mut self, cx: &mut Cx<Msg>) {
        let requests = self.login.on_escape(&mut self.password, on_escape);
        self.send(requests, cx);
        self.focus(cx);
    }

    fn heard(&mut self, response: Response, cx: &mut Cx<Msg>) {
        let (sessions, session_of, cfg) = (&self.sessions, &self.session_of, &self.cfg);
        let heard = self.login.heard(response, |user| launch_for(sessions, session_of, cfg, user));
        self.send(heard.requests, cx);
        match heard.outcome {
            Outcome::Quiet => {}
            Outcome::Refused => {
                greetd::zero_string(&mut self.password);
                self.shake = 1;
                self.shaken = 1;
                cx.after(Duration::from_millis(30), Msg::Shake(1));
            }
            Outcome::LoggedIn(user) => {
                if let Some(session) = session_for(&self.sessions, &self.session_of, &self.cfg, Some(&user)) {
                    let id = session.id.clone();
                    self.state.logged_in(&user, &id);
                    if self.live
                        && let Err(e) = self.state.save(&self.cfg.state)
                    {
                        log::warn!("remembering {user}'s session: {e}");
                    }
                }
                if self.demo {
                    let launch = launch_for(&self.sessions, &self.session_of, &self.cfg, &user);
                    log::info!("would start {:?} with {:?}", launch.cmd, launch.env);
                }
                cx.quit();
            }
        }
    }

    /// Person `i` clicked in the row.
    fn pick(&mut self, i: usize, cx: &mut Cx<Msg>) {
        let requests = self.login.pick(i, &mut self.password);
        self.send(requests, cx);
        self.focus(cx);
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
        for (name, path) in &self.icons {
            let (name, path) = (name.clone(), path.clone());
            cx.spawn(move || Msg::Avatar(name, Image::open(&path).ok()));
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
        let requests = self.login.start(self.cfg.user_list, self.state.last_user.as_deref());
        self.send(requests, cx);
        self.focus(cx);
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
            Msg::GreetdLost(why) => self.lost(&why, cx),
            Msg::Password(p) => {
                greetd::zero_string(&mut self.password);
                self.password = p;
            }
            Msg::Submit => self.submit(cx),
            Msg::Caps(on) => self.caps = on,
            Msg::Name(n) => self.login.type_name(n),
            Msg::Pick(i) => self.pick(i, cx),
            Msg::Select(i) => self.login.select(i),
            Msg::Next => self.login.next(),
            Msg::Previous => self.login.previous(),
            Msg::Back => self.back(cx),
            Msg::SessionMenu => self.session_menu(cx),
            Msg::PickSession(i) => {
                if let (Some(user), Some(s)) = (self.login.user(), self.sessions.get(i)) {
                    self.session_of.insert(user.to_owned(), s.id.clone());
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
