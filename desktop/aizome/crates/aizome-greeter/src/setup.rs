//! First-boot setup on the login screen (`docs/greeter.md`): where no one
//! has an account yet, the greeter asks the computer's owner for theirs
//! instead of showing the people. A root helper does the work (on Sekka,
//! `sekka-setup-helper`, over the socket in `setup_socket`): it says
//! whether setup is needed, what it offers and suggests, judges each field
//! as it's typed, and makes the account, streaming its progress. The
//! greeter only asks and shows; it runs unprivileged as ever.
//!
//! The pages: Welcome, then Region (language, keyboard and time zone, the
//! keyboard first so the password is typed on the layout it will be typed
//! on at every login), Account, Desktop, Review, the work, and then the
//! login screen with the new person picked.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc;

use aizome_kit::prelude::*;
use aizome_kit::{Menu, MenuItem, MenuStyle};
use serde::{Deserialize, Serialize};

use crate::app::{Greeter, Msg, Stage};
use crate::greetd;
use crate::login::Tone;

// The helper's protocol, as the greeter speaks it: one JSON object a line.

/// The choices so far.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    pub full_name: String,
    pub username: String,
    pub hostname: String,
    pub locale: String,
    pub keyboard: String,
    pub time_zone: String,
    pub desktop: String,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum Request<'a> {
    Status {},
    Check {
        choices: &'a Draft,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<&'a str>,
    },
    Complete {
        choices: &'a Draft,
        password: &'a str,
    },
    Retry {},
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Response {
    Status(Status),
    Checked(Checked),
    Progress(Progress),
    Done(Done),
    Failed(Failure),
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Needed,
    Interrupted,
    Complete,
    NotNeeded,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Status {
    pub state: State,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub offer: Offer,
    #[serde(default)]
    pub suggested: Draft,
    #[serde(default)]
    pub interrupted: Option<Interrupted>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Offer {
    pub locales: Vec<Choice>,
    pub keyboards: Vec<Choice>,
    pub time_zones: Vec<Zone>,
    pub desktops: Vec<DesktopChoice>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Zone {
    pub id: String,
    pub city: String,
    pub country: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DesktopChoice {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Interrupted {
    pub username: String,
    pub full_name: String,
    pub account_exists: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Checked {
    pub problems: BTreeMap<Field, String>,
    pub suggested_username: String,
    pub suggested_hostname: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    FullName,
    Username,
    Password,
    Hostname,
    Locale,
    Keyboard,
    TimeZone,
    Desktop,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub step: String,
    pub index: usize,
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Done {
    pub username: String,
    pub full_name: String,
    pub uid: u32,
    pub desktop: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Failure {
    pub message: String,
    pub account_exists: bool,
    pub can_retry: bool,
}

/// A request as the line sent, which the link zeroes once it's written
/// (a completion holds the password).
fn line(r: &Request) -> Vec<u8> {
    let mut v = serde_json::to_vec(r).unwrap_or_default();
    v.push(b'\n');
    v
}

/// The steps of the work, as the helper names them, in the person's words.
pub const STEPS: [(&str, &str); 6] = [
    ("check", "Checking your choices"),
    ("account", "Creating your account"),
    ("settings", "Applying your settings"),
    ("configuration", "Saving your configuration"),
    ("verify", "Checking the configuration"),
    ("finish", "Finishing up"),
];

// The flow.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    /// Asking the helper whether setup is needed.
    Connecting,
    Welcome,
    Region,
    Account,
    Desktop,
    Review,
    /// The helper at work.
    Working,
    Failed,
    /// No helper to ask.
    Unavailable,
}

impl Page {
    /// The pages with choices, in order, for the dots at the foot.
    pub const STEPS: [Page; 4] = [Page::Region, Page::Account, Page::Desktop, Page::Review];

    pub fn next(self) -> Page {
        match self {
            Page::Welcome => Page::Region,
            Page::Region => Page::Account,
            Page::Account => Page::Desktop,
            Page::Desktop => Page::Review,
            p => p,
        }
    }

    pub fn back(self) -> Page {
        match self {
            Page::Region => Page::Welcome,
            Page::Account => Page::Region,
            Page::Desktop => Page::Account,
            Page::Review => Page::Desktop,
            // Nothing was made yet: the choices can still change.
            Page::Failed => Page::Review,
            p => p,
        }
    }
}

/// The account page's text fields, in their order on the page, with their
/// element ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    FullName,
    Username,
    Password,
    Repeat,
    Hostname,
}

impl Input {
    pub const ALL: [Input; 5] = [Input::FullName, Input::Username, Input::Password, Input::Repeat, Input::Hostname];

    pub fn id(self) -> &'static str {
        match self {
            Input::FullName => "setup-full-name",
            Input::Username => "setup-username",
            Input::Password => "setup-password",
            Input::Repeat => "setup-repeat",
            Input::Hostname => "setup-hostname",
        }
    }
}

/// Setup's state on the login screen.
#[derive(Debug)]
pub struct Setup {
    pub page: Page,
    pub status: Option<Status>,
    pub draft: Draft,
    pub password: String,
    pub repeat: String,
    /// The person typed their own user name and computer name: suggestions
    /// stop replacing them.
    pub edited_username: bool,
    pub edited_hostname: bool,
    /// The helper's latest verdicts, and whether they're for the draft as
    /// it is (no check is out, and nothing changed since).
    pub checked: Checked,
    checking: bool,
    stale: bool,
    /// Continue was pressed with something wrong: say what, everywhere.
    pub flagged: bool,
    /// Continue was pressed while a check was out: go on once it says yes.
    pending_continue: bool,
    /// Continue found the passwords different: say so until the second is
    /// typed again.
    repeat_flagged: bool,
    pub zone_query: String,
    /// The highlighted match in the time zone list.
    pub zone_pick: usize,
    /// The keyboard's try-it field.
    pub try_text: String,
    pub progress: Option<Progress>,
    pub failure: Option<Failure>,
    /// Why there is no helper to ask.
    pub unavailable: Option<String>,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            page: Page::Connecting,
            status: None,
            draft: Draft::default(),
            password: String::new(),
            repeat: String::new(),
            edited_username: false,
            edited_hostname: false,
            checked: Checked::default(),
            checking: false,
            stale: false,
            flagged: false,
            pending_continue: false,
            repeat_flagged: false,
            zone_query: String::new(),
            zone_pick: 0,
            try_text: String::new(),
            progress: None,
            failure: None,
            unavailable: None,
        }
    }
}

impl Setup {
    pub fn offer(&self) -> &Offer {
        match &self.status {
            Some(s) => &s.offer,
            None => &NO_OFFER,
        }
    }

    /// What to call the computer: "Mac", else "computer".
    pub fn device(&self) -> String {
        self.status.as_ref().map(|s| s.device.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| "computer".into())
    }

    /// The helper's verdict on `field`, if there's something wrong.
    pub fn problem(&self, field: Field) -> Option<&str> {
        self.checked.problems.get(&field).map(String::as_str)
    }

    /// The verdicts are for the draft as it is now.
    pub fn settled(&self) -> bool {
        !self.checking && !self.stale
    }

    /// What's wrong with `input` that should show now: everything once
    /// Continue was tried; before, only what's wrong with text the person
    /// has typed, and a short password not until they're done.
    pub fn shown_problem(&self, input: Input) -> Option<String> {
        match input {
            Input::Repeat => {
                // Not while it's still being typed: once it's as long as
                // the first, or left empty when Continue was tried.
                let typed = !self.repeat.is_empty() && self.repeat.chars().count() >= self.password.chars().count();
                ((typed || self.repeat_flagged) && self.repeat != self.password)
                    .then(|| "The passwords don't match. Type the same password twice.".to_owned())
            }
            Input::Password => {
                if self.flagged && self.password.is_empty() {
                    return Some("Choose a password of at least 12 characters.".into());
                }
                self.problem(Field::Password).filter(|_| self.flagged).map(str::to_owned)
            }
            Input::FullName => self
                .problem(Field::FullName)
                .filter(|_| !self.draft.full_name.is_empty() || self.flagged)
                .map(str::to_owned),
            Input::Username => {
                if self.flagged && self.draft.username.is_empty() {
                    return Some("Choose a user name: lowercase letters, digits, - and _.".into());
                }
                self.problem(Field::Username).filter(|_| !self.draft.username.is_empty()).map(str::to_owned)
            }
            Input::Hostname => {
                if self.flagged && self.draft.hostname.is_empty() {
                    return Some("Give this computer a name.".into());
                }
                self.problem(Field::Hostname).filter(|_| !self.draft.hostname.is_empty()).map(str::to_owned)
            }
        }
    }

    /// The account page can be left: Genkan's rules take every field, as
    /// checked for the draft as it is, and the passwords match.
    pub fn account_ready(&self) -> bool {
        self.settled()
            && !self.password.is_empty()
            && self.password == self.repeat
            && ![Field::FullName, Field::Username, Field::Password, Field::Hostname]
                .iter()
                .any(|f| self.problem(*f).is_some())
            && !self.draft.username.is_empty()
            && !self.draft.hostname.is_empty()
    }

    /// The first field with something to fix (or to fill), for the focus.
    pub fn first_unready(&self) -> Option<Input> {
        Input::ALL.into_iter().find(|i| match i {
            Input::FullName => self.problem(Field::FullName).is_some(),
            Input::Username => self.draft.username.is_empty() || self.problem(Field::Username).is_some(),
            Input::Password => self.password.is_empty() || self.problem(Field::Password).is_some(),
            Input::Repeat => self.repeat != self.password,
            Input::Hostname => self.draft.hostname.is_empty() || self.problem(Field::Hostname).is_some(),
        })
    }

    /// The zones matching what's typed, best first: the city's start, then
    /// anywhere in it, then the country's, then the zone's name.
    pub fn zone_matches(&self, limit: usize) -> Vec<Zone> {
        let q = fold(self.zone_query.trim());
        if q.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(u8, &Zone)> = self
            .status
            .iter()
            .flat_map(|s| s.offer.time_zones.iter())
            .filter_map(|z| {
                let (city, country, id) = (fold(&z.city), fold(&z.country), fold(&z.id));
                let score = if city.starts_with(&q) {
                    0
                } else if city.split(' ').any(|w| w.starts_with(&q)) {
                    1
                } else if country.starts_with(&q) || country.split(' ').any(|w| w.starts_with(&q)) {
                    2
                } else if city.contains(&q) || id.contains(&q) || country.contains(&q) {
                    3
                } else {
                    return None;
                };
                Some((score, z))
            })
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.city.cmp(&b.1.city)));
        scored.into_iter().take(limit).map(|(_, z)| z.clone()).collect()
    }

    pub fn zone(&self) -> Option<Zone> {
        self.status.as_ref()?.offer.time_zones.iter().find(|z| z.id == self.draft.time_zone).cloned()
    }

    pub fn label(list: &[Choice], id: &str) -> String {
        list.iter().find(|c| c.id == id).map(|c| c.label.clone()).unwrap_or_else(|| id.to_owned())
    }

    fn forget_secrets(&mut self) {
        greetd::zero_string(&mut self.password);
        greetd::zero_string(&mut self.repeat);
    }
}

static NO_OFFER: Offer =
    Offer { locales: Vec::new(), keyboards: Vec::new(), time_zones: Vec::new(), desktops: Vec::new() };

/// Lowercase without accents, for matching place names as typed.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            '_' => ' ',
            c => c,
        })
        .collect()
}

/// What the person and the helper do on the setup pages.
#[derive(Clone, Debug)]
pub enum SetupMsg {
    /// The helper answered.
    Heard(Response),
    /// The connection to the helper broke (or never was).
    Lost(String),
    Continue,
    Back,
    FullName(String),
    Username(String),
    Password(String),
    Repeat(String),
    Hostname(String),
    /// Return in an account field: the next one, or Continue from the last.
    Submit(Input),
    LanguageMenu,
    KeyboardMenu,
    Language(usize),
    Keyboard(usize),
    TryText(String),
    ZoneQuery(String),
    /// ↑ ↓ in the time zone list.
    ZoneMove(i32),
    /// A zone picked, by its id.
    Zone(String),
    /// Return in the time zone field: the highlighted match.
    ZoneSubmit,
    Desktop(usize),
    DesktopStep(i32),
    /// Review's Set Up.
    SetUp,
    Retry,
    /// Leave a failed setup for the login screen (the account exists).
    LogIn,
    /// Ask the helper again (after losing it).
    Reconnect,
}

/// The connection to the helper: lines to send (written, then zeroed),
/// and the socket, to close.
pub struct Link {
    tx: mpsc::Sender<Vec<u8>>,
    stream: UnixStream,
}

impl Drop for Link {
    fn drop(&mut self) {
        // The reader sees the end, and the helper is free for another.
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// Connect to the helper at `path`: a thread writes what's sent, another
/// reads its answers into messages.
pub fn connect(path: &Path, out: aizome_kit::MsgSender<Msg>) -> std::io::Result<Link> {
    let stream = UnixStream::connect(path)?;
    let handle = stream.try_clone()?;
    let mut writer = stream.try_clone()?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let lost = out.clone();
    std::thread::Builder::new().name("setup-write".into()).spawn(move || {
        while let Ok(mut line) = rx.recv() {
            let r = writer.write_all(&line).and_then(|_| writer.flush());
            greetd::zero(&mut line);
            if let Err(e) = r {
                lost.send(Msg::Setup(SetupMsg::Lost(e.to_string())));
                return;
            }
        }
    })?;
    std::thread::Builder::new().name("setup-read".into()).spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut text = String::new();
        loop {
            text.clear();
            match reader.read_line(&mut text) {
                Ok(0) => {
                    out.send(Msg::Setup(SetupMsg::Lost("the setup service closed the connection".into())));
                    return;
                }
                Ok(_) => match serde_json::from_str::<Response>(text.trim_end()) {
                    Ok(r) => {
                        if !out.send(Msg::Setup(SetupMsg::Heard(r))) {
                            return;
                        }
                    }
                    Err(e) => {
                        out.send(Msg::Setup(SetupMsg::Lost(format!(
                            "the setup service said something unexpected ({e})"
                        ))));
                        return;
                    }
                },
                Err(e) => {
                    out.send(Msg::Setup(SetupMsg::Lost(e.to_string())));
                    return;
                }
            }
        }
    })?;
    Ok(Link { tx, stream: handle })
}

impl Greeter {
    /// Start asking the helper, if this login screen has one.
    pub(crate) fn start_setup(&mut self, cx: &mut Cx<Msg>) {
        let Some(path) = self.cfg.setup_socket.clone() else { return };
        if self.setup.is_none() {
            self.setup = Some(Setup::default());
        }
        if !self.live {
            // A test: it answers itself.
            return;
        }
        let out = cx.stream();
        self.setup_out = Some(out.clone());
        match connect(&path, out) {
            Ok(tx) => {
                self.to_setup = Some(tx);
                self.setup_send(&Request::Status {});
            }
            Err(e) => self.setup_update(SetupMsg::Lost(format!("{}: {e}", path.display())), cx),
        }
    }

    fn setup_send(&mut self, r: &Request) {
        let mut l = line(r);
        // Lost meanwhile (the helper restarted): ask on a new connection.
        if self.to_setup.is_none()
            && self.live
            && let (Some(path), Some(out)) = (self.cfg.setup_socket.clone(), self.setup_out.clone())
        {
            match connect(&path, out) {
                Ok(link) => self.to_setup = Some(link),
                Err(e) => log::warn!("setup: {}: {e}", path.display()),
            }
        }
        if let Some(link) = &self.to_setup
            && link.tx.send(std::mem::take(&mut l)).is_err()
        {
            self.to_setup = None;
        }
        greetd::zero(&mut l);
    }

    /// Ask the helper about the draft (and the password, on its page),
    /// unless a check is out; then ask again once it answers.
    fn check(&mut self) {
        let Some(s) = self.setup.as_mut() else { return };
        if s.checking {
            s.stale = true;
            return;
        }
        s.checking = true;
        s.stale = false;
        let draft = s.draft.clone();
        let password = (!s.password.is_empty()).then(|| s.password.clone());
        let mut password = password;
        self.setup_send(&Request::Check { choices: &draft, password: password.as_deref() });
        if let Some(p) = password.as_mut() {
            greetd::zero_string(p);
        }
    }

    fn go(&mut self, page: Page, cx: &mut Cx<Msg>) {
        let Some(s) = self.setup.as_mut() else { return };
        log::info!("setup: {page:?}");
        s.page = page;
        s.flagged = false;
        let focus = match page {
            Page::Welcome => "setup-start",
            Page::Region => "setup-language",
            Page::Account => s.first_unready().map(Input::id).unwrap_or(Input::FullName.id()),
            Page::Desktop => {
                cx.focus(&format!("setup-desktop-{}", s.draft.desktop));
                return;
            }
            Page::Review => "setup-set-up",
            Page::Failed => "setup-retry",
            _ => "setup-page",
        };
        cx.focus(focus);
        if page == Page::Account {
            self.check();
        }
    }

    /// The setup pages' messages.
    pub(crate) fn setup_update(&mut self, m: SetupMsg, cx: &mut Cx<Msg>) {
        let Some(s) = self.setup.as_mut() else { return };
        match m {
            SetupMsg::Heard(r) => self.heard_setup(r, cx),
            SetupMsg::Lost(why) => {
                log::error!("setup: {why}");
                self.to_setup = None;
                // A check that was out is asked again, on a new connection.
                s.stale |= s.checking;
                s.checking = false;
                if matches!(s.page, Page::Connecting) {
                    s.page = Page::Unavailable;
                    s.unavailable = Some(why);
                    cx.focus("setup-retry");
                } else if s.page == Page::Working {
                    s.failure = Some(Failure {
                        message: format!("The setup service stopped ({why}). Try again: setup finishes what it began."),
                        account_exists: false,
                        can_retry: true,
                    });
                    s.page = Page::Failed;
                    cx.focus("setup-retry");
                } else {
                    s.unavailable = Some(why);
                }
            }
            SetupMsg::Reconnect => {
                s.page = Page::Connecting;
                s.unavailable = None;
                self.start_setup(cx);
            }
            SetupMsg::Continue => self.setup_continue(cx),
            SetupMsg::Back => {
                let back = s.page.back();
                if back != s.page {
                    self.go(back, cx);
                }
            }
            SetupMsg::FullName(v) => {
                s.draft.full_name = v;
                self.check();
            }
            SetupMsg::Username(v) => {
                s.edited_username = !v.is_empty();
                s.draft.username = v;
                self.check();
            }
            SetupMsg::Hostname(v) => {
                s.edited_hostname = !v.is_empty();
                s.draft.hostname = v;
                self.check();
            }
            SetupMsg::Password(v) => {
                greetd::zero_string(&mut s.password);
                s.password = v;
                self.check();
            }
            SetupMsg::Repeat(v) => {
                greetd::zero_string(&mut s.repeat);
                s.repeat = v;
                s.repeat_flagged = false;
            }
            SetupMsg::Submit(input) => {
                let at = Input::ALL.iter().position(|i| *i == input).unwrap_or(0);
                match Input::ALL.get(at + 1) {
                    Some(next) => cx.focus(next.id()),
                    None => self.setup_continue(cx),
                }
            }
            SetupMsg::LanguageMenu => self.choice_menu(true, cx),
            SetupMsg::KeyboardMenu => self.choice_menu(false, cx),
            SetupMsg::Language(i) => {
                if let Some(id) = s.offer().locales.get(i).map(|c| c.id.clone()) {
                    s.draft.locale = id;
                }
                self.close_menus(cx);
                cx.focus("setup-language");
            }
            SetupMsg::Keyboard(i) => {
                if let Some(layout) = s.offer().keyboards.get(i).map(|c| c.id.clone()) {
                    s.draft.keyboard = layout.clone();
                    self.use_keyboard(&layout);
                }
                self.close_menus(cx);
                cx.focus("setup-try");
            }
            SetupMsg::TryText(t) => s.try_text = t,
            SetupMsg::ZoneQuery(q) => {
                s.zone_query = q;
                s.zone_pick = 0;
            }
            SetupMsg::ZoneMove(d) => {
                let n = s.zone_matches(ZONE_ROWS).len();
                if n > 0 {
                    s.zone_pick = (s.zone_pick as i64 + i64::from(d)).clamp(0, n as i64 - 1) as usize;
                }
            }
            SetupMsg::Zone(id) => {
                s.draft.time_zone = id;
                s.zone_query.clear();
                s.zone_pick = 0;
                cx.focus("setup-zone");
            }
            SetupMsg::ZoneSubmit => {
                let matches = s.zone_matches(ZONE_ROWS);
                match matches.get(s.zone_pick) {
                    Some(z) => {
                        s.draft.time_zone = z.id.clone();
                        s.zone_query.clear();
                        s.zone_pick = 0;
                    }
                    // Nothing typed: Return goes on.
                    None if s.zone_query.trim().is_empty() => self.setup_continue(cx),
                    None => {}
                }
            }
            SetupMsg::Desktop(i) => {
                if let Some(id) = s.offer().desktops.get(i).map(|d| d.id.clone()) {
                    s.draft.desktop = id;
                }
                cx.focus(&format!("setup-desktop-{}", s.draft.desktop));
            }
            SetupMsg::DesktopStep(d) => {
                let ids: Vec<String> = s.offer().desktops.iter().map(|x| x.id.clone()).collect();
                if !ids.is_empty() {
                    let at = ids.iter().position(|x| *x == s.draft.desktop).unwrap_or(0) as i64;
                    let to = (at + i64::from(d)).rem_euclid(ids.len() as i64) as usize;
                    s.draft.desktop = ids[to].clone();
                }
                cx.focus(&format!("setup-desktop-{}", s.draft.desktop));
            }
            SetupMsg::SetUp => {
                if !s.account_ready() {
                    self.go(Page::Account, cx);
                    return;
                }
                s.page = Page::Working;
                s.progress = None;
                s.failure = None;
                let draft = s.draft.clone();
                let mut password = s.password.clone();
                self.setup_send(&Request::Complete { choices: &draft, password: &password });
                greetd::zero_string(&mut password);
                cx.focus("setup-page");
            }
            SetupMsg::Retry => {
                s.page = Page::Working;
                s.progress = None;
                let failure = s.failure.take();
                if self.live && self.to_setup.is_none() {
                    // Lost: ask again, and finish what was begun.
                    if let Some(s) = self.setup.as_mut() {
                        s.page = Page::Connecting;
                    }
                    self.start_setup(cx);
                    return;
                }
                if failure.is_some_and(|f| !f.can_retry) {
                    self.go(Page::Account, cx);
                    return;
                }
                self.setup_send(&Request::Retry {});
            }
            SetupMsg::LogIn => self.leave_setup(None, cx),
        }
    }

    fn setup_continue(&mut self, cx: &mut Cx<Msg>) {
        let Some(s) = self.setup.as_mut() else { return };
        match s.page {
            Page::Account if !s.account_ready() => {
                let unready = s.first_unready();
                if unready.is_none() && !s.settled() {
                    // Only the helper's verdict is missing: go on when it comes.
                    log::info!("setup: waiting for the helper's check to go on");
                    s.pending_continue = true;
                    return;
                }
                log::info!("setup: the account isn't ready ({unready:?})");
                s.flagged = true;
                s.repeat_flagged = s.repeat != s.password;
                if let Some(i) = unready {
                    cx.focus(i.id());
                }
            }
            Page::Review => self.setup_update(SetupMsg::SetUp, cx),
            Page::Welcome | Page::Region | Page::Account | Page::Desktop => {
                let next = s.page.next();
                self.go(next, cx);
            }
            _ => {}
        }
    }

    fn heard_setup(&mut self, r: Response, cx: &mut Cx<Msg>) {
        let Some(s) = self.setup.as_mut() else { return };
        match r {
            Response::Status(status) => {
                let state = status.state;
                let suggested = status.suggested.clone();
                let fresh = s.status.is_none();
                s.status = Some(status);
                match state {
                    State::Complete | State::NotNeeded => self.leave_setup(None, cx),
                    State::Needed => {
                        if fresh {
                            s.draft = suggested;
                            self.use_keyboard(
                                &self.setup.as_ref().map(|s| s.draft.keyboard.clone()).unwrap_or_default(),
                            );
                        }
                        self.stage = Stage::Setup;
                        if matches!(self.setup.as_ref().map(|s| s.page), Some(Page::Connecting | Page::Unavailable)) {
                            self.go(Page::Welcome, cx);
                        }
                    }
                    State::Interrupted => {
                        // Begun before, and stopped: finish it.
                        self.stage = Stage::Setup;
                        if let Some(s) = self.setup.as_mut() {
                            s.page = Page::Working;
                            s.progress = None;
                        }
                        cx.focus("setup-page");
                        self.setup_send(&Request::Retry {});
                    }
                }
            }
            Response::Checked(checked) => {
                s.checking = false;
                if !s.edited_username && s.draft.username != checked.suggested_username {
                    s.draft.username = checked.suggested_username.clone();
                    s.stale = true;
                }
                if !s.edited_hostname && s.draft.hostname != checked.suggested_hostname {
                    s.draft.hostname = checked.suggested_hostname.clone();
                    s.stale = true;
                }
                s.checked = checked;
                if s.stale {
                    self.check();
                } else if std::mem::take(&mut s.pending_continue) && s.page == Page::Account {
                    self.setup_continue(cx);
                }
            }
            Response::Progress(p) => s.progress = Some(p),
            Response::Done(done) => {
                s.forget_secrets();
                self.leave_setup(Some(done), cx);
            }
            Response::Failed(f) => {
                // The choices were refused before anything changed: back to
                // the page with the field, saying why.
                s.failure = Some(f);
                s.page = Page::Failed;
                cx.focus("setup-retry");
            }
            Response::Error(e) => self.setup_update(SetupMsg::Lost(e), cx),
        }
    }

    /// The login screen as usual; with `done`, the new person picked, their
    /// desktop theirs, and a word that their account is ready.
    fn leave_setup(&mut self, done: Option<Done>, cx: &mut Cx<Msg>) {
        if let Some(s) = self.setup.as_mut() {
            s.forget_secrets();
        }
        let was_setup = self.stage == Stage::Setup;
        self.setup = None;
        self.to_setup = None;
        if self.live {
            self.users = crate::users::find(&self.cfg.hidden_users);
            // The computer has the name setup gave it now.
            self.hostname = self.cfg.hostname.clone().unwrap_or_else(crate::app::hostname);
        }
        if let Some(d) = &done {
            if !self.users.iter().any(|u| u.name == d.username) {
                let real_name = if d.full_name.is_empty() { d.username.clone() } else { d.full_name.clone() };
                self.users.push(crate::users::User { name: d.username.clone(), real_name, uid: d.uid, icon: None });
            }
            let session = self.sessions.iter().find(|s| s.id == d.desktop).map(|s| s.id.clone());
            if let Some(session) = session {
                self.session_of.insert(d.username.clone(), session.clone());
                self.state.logged_in(&d.username, &session);
                if self.live
                    && let Err(e) = self.state.save(&self.cfg.state)
                {
                    log::warn!("remembering {}'s desktop: {e}", d.username);
                }
            }
            self.selected = self.users.iter().position(|u| u.name == d.username).unwrap_or(0);
        }
        if !was_setup && done.is_none() {
            return;
        }
        if self.users.is_empty() || !self.cfg.user_list {
            self.stage = Stage::Name;
            cx.focus("greeter-name");
            return;
        }
        self.stage = Stage::Password;
        self.begin();
        if done.is_some() {
            self.login.message = Some(("Your account is ready. Log in with your new password.".into(), Tone::Info));
        }
        cx.focus("greeter-password");
    }

    /// The compositor's keyboard: what's typed on the login screen from now.
    fn use_keyboard(&self, layout: &str) {
        if !self.live || layout.is_empty() {
            return;
        }
        if let Some(path) = &self.settings_file
            && let Err(e) = crate::displays::set_keyboard(path, layout)
        {
            log::warn!("the login screen's keyboard: {e}");
        }
    }

    fn choice_menu(&mut self, language: bool, cx: &mut Cx<Msg>) {
        let Some(s) = self.setup.as_ref() else { return };
        let Some(win) = self.main_window() else { return };
        let id = if language { "setup-language" } else { "setup-keyboard" };
        let Some(r) = cx.element_rect(win, id) else { return };
        let offer = s.offer();
        let (list, current) =
            if language { (&offer.locales, &s.draft.locale) } else { (&offer.keyboards, &s.draft.keyboard) };
        let items = list.iter().enumerate().map(|(i, c)| {
            let msg = if language { SetupMsg::Language(i) } else { SetupMsg::Keyboard(i) };
            MenuItem::new(c.label.clone(), Msg::Setup(msg)).checked(&c.id == current)
        });
        let title = if language { "Languages" } else { "Keyboards" };
        let menu = Menu::new(title).items(items.collect::<Vec<_>>());
        cx.show_menu(win, r, menu, MenuStyle::below());
    }

    fn close_menus(&mut self, cx: &mut Cx<Msg>) {
        if let Some(w) = self.main_window() {
            cx.close_menus(w);
        }
    }
}

/// How many time zones the list shows.
pub const ZONE_ROWS: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_the_helpers_lines() {
        let d = Draft { full_name: "Ada".into(), ..Draft::default() };
        assert_eq!(line(&Request::Status {}), b"{\"op\":\"status\"}\n");
        assert_eq!(line(&Request::Retry {}), b"{\"op\":\"retry\"}\n");
        let check = String::from_utf8(line(&Request::Check { choices: &d, password: None })).unwrap();
        assert!(check.starts_with(r#"{"op":"check","choices":{"full_name":"Ada","username":"""#), "{check}");
        assert!(!check.contains("password"));
        let complete = String::from_utf8(line(&Request::Complete { choices: &d, password: "pw" })).unwrap();
        assert!(complete.ends_with(",\"password\":\"pw\"}\n"), "{complete}");
    }

    #[test]
    fn answers_are_read_and_unknown_fields_tolerated() {
        let r: Response = serde_json::from_str(
            r#"{"checked":{"problems":{"username":"Reserved username."},"suggested_username":"ada","suggested_hostname":"adas-mac","later":1}}"#,
        )
        .unwrap();
        let Response::Checked(c) = r else { panic!() };
        assert_eq!(c.problems[&Field::Username], "Reserved username.");
        let r: Response = serde_json::from_str(r#"{"progress":{"step":"account","index":1,"count":6}}"#).unwrap();
        assert_eq!(r, Response::Progress(Progress { step: "account".into(), index: 1, count: 6 }));
        let r: Response = serde_json::from_str(r#"{"status":{"version":1,"state":"interrupted","device":"Mac","offer":{"locales":[],"keyboards":[],"time_zones":[],"desktops":[]},"suggested":{},"interrupted":{"username":"ada","full_name":"Ada","account_exists":true}}}"#).unwrap();
        let Response::Status(s) = r else { panic!() };
        assert_eq!(s.state, State::Interrupted);
        assert!(s.interrupted.unwrap().account_exists);
    }

    #[test]
    fn zones_match_by_city_then_country() {
        let zone =
            |id: &str, city: &str, country: &str| Zone { id: id.into(), city: city.into(), country: country.into() };
        let mut s = Setup::default();
        s.status = Some(Status {
            state: State::Needed,
            device: "Mac".into(),
            offer: Offer {
                time_zones: vec![
                    zone("America/New_York", "New York", "United States"),
                    zone("Europe/Berlin", "Berlin", "Germany"),
                    zone("Europe/Busingen", "Busingen", "Germany"),
                    zone("America/Sao_Paulo", "Sao Paulo", "Brazil"),
                    zone("Pacific/Auckland", "Auckland", "New Zealand"),
                ],
                ..Offer::default()
            },
            suggested: Draft::default(),
            interrupted: None,
        });
        s.zone_query = "ber".into();
        assert_eq!(s.zone_matches(5)[0].id, "Europe/Berlin");
        s.zone_query = "germ".into();
        assert_eq!(s.zone_matches(5).iter().map(|z| z.city.as_str()).collect::<Vec<_>>(), ["Berlin", "Busingen"]);
        s.zone_query = "york".into();
        assert_eq!(s.zone_matches(5)[0].city, "New York");
        s.zone_query = "são".into();
        assert_eq!(s.zone_matches(5)[0].city, "Sao Paulo");
        s.zone_query = "new".into();
        // A city's own name first, then a country's.
        assert_eq!(s.zone_matches(5).iter().map(|z| z.city.as_str()).collect::<Vec<_>>(), ["New York", "Auckland"]);
        s.zone_query = "  ".into();
        assert!(s.zone_matches(5).is_empty());
    }

    #[test]
    fn account_readiness_waits_for_the_helpers_verdict() {
        let mut s = Setup::default();
        s.draft.username = "ada".into();
        s.draft.hostname = "analytical".into();
        s.password = "correct horse battery".into();
        s.repeat = "correct horse battery".into();
        assert!(s.account_ready());
        s.checking = true;
        assert!(!s.account_ready(), "a check is out");
        s.checking = false;
        s.checked.problems.insert(Field::Username, "Reserved username.".into());
        assert!(!s.account_ready());
        assert_eq!(s.first_unready(), Some(Input::Username));
        s.checked.problems.clear();
        s.repeat = "correct horse batter".into();
        assert_eq!(s.first_unready(), Some(Input::Repeat));
        // A mismatch shows only once the second is as long as the first.
        assert!(s.shown_problem(Input::Repeat).is_none());
        s.repeat = "correct horse batteRy".into();
        assert!(s.shown_problem(Input::Repeat).is_some());
        // A short password's problem waits for Continue.
        s.checked.problems.insert(Field::Password, "Password must have at least 12 characters.".into());
        assert!(s.shown_problem(Input::Password).is_none());
        s.flagged = true;
        assert!(s.shown_problem(Input::Password).is_some());
    }
}
