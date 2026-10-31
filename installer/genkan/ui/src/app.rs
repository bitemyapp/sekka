// SPDX-License-Identifier: GPL-3.0-or-later
//! The program: Genkan's state, the messages that change it, and the work it
//! starts away from the interface thread.
//!
//! Nothing slow runs on the interface thread. Disk discovery, memory, time
//! zone detection and GitHub run on the kit's workers; page cache warming,
//! review (validation, the Wi-Fi snapshot, starting the helper through
//! Polkit) and the helper's events come from threads of their own through
//! the kit's message stream. Answers from an older request (its epoch or
//! generation) are dropped, as are stale sessions, which cancels them.
use crate::{format::size, media, zone};
use aizome_ipc::{KeyboardInfo, setup::Beside};
use aizome_kit::cx::MsgSender;
use aizome_kit::prelude::*;
use calamares_nixos::{
    Desktop, Filesystem, Firmware, Hostname, InstallPlan, KEYBOARDS, LOCALES, RawRequest, Settings,
    SuperKey, Username, applications,
    disk::{self, Disk, Layout},
    github::{self, GitIdentity, GithubUser},
    install::{Event, Summary},
    memory::{self, MemInfo},
    precache,
    session::{Session, Update},
    timezone::{self, Source},
    zonemap::Zones,
};
use std::{
    collections::{BTreeSet, VecDeque},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Welcome,
    Disk,
    Account,
    GitHub,
    Desktop,
    Applications,
    Location,
    Keyboard,
    Review,
    Install,
}
impl Step {
    pub const ALL: [Self; 10] = [
        Self::Welcome,
        Self::Disk,
        Self::Account,
        Self::GitHub,
        Self::Desktop,
        Self::Applications,
        Self::Location,
        Self::Keyboard,
        Self::Review,
        Self::Install,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Disk => "Disk",
            Self::Account => "Account",
            Self::GitHub => "GitHub",
            Self::Desktop => "Desktop",
            Self::Applications => "Applications",
            Self::Location => "Location",
            Self::Keyboard => "Keyboard",
            Self::Review => "Review",
            Self::Install => "Install",
        }
    }
    pub fn heading(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Disk => "Choose a Disk",
            Self::Account => "Create Your Account",
            Self::GitHub => "GitHub, SSH and Git",
            Self::Desktop => "Choose Your Desktops",
            Self::Applications => "Choose Applications",
            Self::Location => "Time Zone and Language",
            Self::Keyboard => "Set Up Your Keyboard",
            Self::Review => "Review and Confirm",
            Self::Install => "Installing",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            Self::Welcome => Icon::House,
            Self::Disk => Icon::HardDrive,
            Self::Account => Icon::User,
            Self::GitHub => Icon::KeyRound,
            Self::Desktop => Icon::Monitor,
            Self::Applications => Icon::LayoutGrid,
            Self::Location => Icon::MapPin,
            Self::Keyboard => Icon::Keyboard,
            Self::Review => Icon::ListChecks,
            Self::Install => Icon::HardDriveDownload,
        }
    }
    /// Its name for previews (`CALAMARES_UI_PREVIEW=disk`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Disk => "disk",
            Self::Account => "account",
            Self::GitHub => "github",
            Self::Desktop => "desktop",
            Self::Applications => "applications",
            Self::Location => "location",
            Self::Keyboard => "keyboard",
            Self::Review => "review",
            Self::Install => "install",
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Installation steps reported as `Event::Progress { step: 1..=6 }`.
pub const INSTALL_STEPS: [&str; 6] = [
    "Partition the disk",
    "Format and mount the filesystems",
    "Write configuration and account settings",
    "Copy the prepared system",
    "Install the bootloader",
    "Flush writes and unmount",
];

/// Share of the overall progress bar for each step: copying dominates.
const STEP_WEIGHTS: [f64; 6] = [0.04, 0.08, 0.02, 0.72, 0.10, 0.04];

pub fn overall_fraction(step: u8, copy_fraction: f64) -> f64 {
    let step = usize::from(step.clamp(1, 6));
    let done: f64 = STEP_WEIGHTS[..step - 1].iter().sum();
    let current = if step == 4 {
        STEP_WEIGHTS[3] * copy_fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (done + current).clamp(0.0, 1.0)
}

/// Suggest a valid username from a full name: "Ada Lovelace" → "ada".
pub fn suggest_username(full_name: &str) -> String {
    let first = full_name.split_whitespace().next().unwrap_or("");
    let name: String = first
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .take(31)
        .collect();
    if name.starts_with(|c: char| c.is_ascii_lowercase()) && Username::parse(&name).is_ok() {
        name
    } else {
        String::new()
    }
}

pub const LOCALE_NAMES: [&str; 8] = [
    "English (United States)",
    "English (United Kingdom)",
    "Deutsch (Deutschland)",
    "Français (France)",
    "Español (España)",
    "Italiano (Italia)",
    "日本語 (日本)",
    "Português (Brasil)",
];
pub const KEYBOARD_NAMES: [&str; 8] = [
    "English (US)",
    "English (UK)",
    "German",
    "French",
    "Spanish",
    "Italian",
    "Japanese",
    "Portuguese (Brazil)",
];

/// Status line under the GitHub username before any lookup.
pub const GITHUB_HINT: &str = "Optional. Press Return or Look Up to fetch the account's public keys. Leave it empty to skip GitHub.";

/// How long a note stays in the bar at the bottom.
const NOTE_FOR: Duration = Duration::from_secs(6);

/// The modifier arrangements the keyboard step offers, in
/// `SuperKey::ALL`'s order, named as Settei names them.
pub const SUPER_KEY_NAMES: [&str; 3] = [
    "As They Are",
    "Swap Alt and Super",
    "Swap Control and Super",
];

/// The keyboard step's question: the key just left of the space bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyCheck {
    /// Not asked yet, or Esc or Stop gave up on it.
    Asking,
    /// The compositor is catching the next key for Genkan.
    Waiting,
    /// The key, and the keyboard it came from (none for a key from IPC).
    Caught {
        keyboard: Option<KeyboardInfo>,
        beside: Beside,
    },
    /// The key can't be caught here (outside Aizome): why.
    Unavailable(String),
}

/// A field a failed check points at: it's outlined in red and focused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Passphrase,
    PassphraseRepeat,
    FullName,
    Username,
    Password,
    Repeat,
    Hostname,
    GithubUser,
    GitName,
    GitEmail,
}
impl Field {
    /// The element's name (`.id(..)`), for focusing it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Passphrase => "passphrase",
            Self::PassphraseRepeat => "passphrase-repeat",
            Self::FullName => "full-name",
            Self::Username => "username",
            Self::Password => "password",
            Self::Repeat => "password-repeat",
            Self::Hostname => "hostname",
            Self::GithubUser => "github-user",
            Self::GitName => "git-name",
            Self::GitEmail => "git-email",
        }
    }
}

/// Something that can't be cloned (a running helper) carried in a message
/// once: whoever handles the message takes it. A message dropped unhandled
/// drops what it carries.
pub struct Handoff<T>(Arc<Mutex<Option<T>>>);
impl<T> Handoff<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }
    pub fn take(&self) -> Option<T> {
        self.0.lock().ok()?.take()
    }
}
impl<T> Clone for Handoff<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> std::fmt::Debug for Handoff<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handoff")
    }
}

/// What the helper said, as a message.
#[derive(Clone, Debug)]
pub enum Helper {
    Event(Event),
    /// The helper exited: `Ok` when its exit status was success.
    Finished(Result<(), String>),
}
impl From<Update> for Helper {
    fn from(update: Update) -> Self {
        match update {
            Update::Event(event) => Self::Event(event),
            Update::Finished(result) => Self::Finished(result),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    // Navigation.
    Next,
    Back,
    Jump(Step),
    ClearNote(u64),
    // Disk.
    Rescan,
    Scanned(Result<(Vec<Disk>, Firmware), String>),
    Memory(Result<MemInfo, String>),
    SelectDisk(usize),
    Filesystem(usize),
    Swap(bool),
    Encrypt(bool),
    Passphrase(String),
    PassphraseRepeat(String),
    Tuning(bool),
    // Account.
    FullName(String),
    Username(String),
    Password(String),
    Repeat(String),
    Hostname(String),
    // GitHub, SSH and Git.
    GithubUser(String),
    Lookup,
    Github(u64, Result<github::Profile, String>),
    Authorize(bool),
    SshServer(bool),
    GitName(String),
    GitEmail(String),
    // Desktops and software.
    ToggleDesktop(Desktop, bool),
    DefaultDesktop(usize),
    Wifi(bool),
    Unfree(bool),
    AppSearch(String),
    ToggleApp(usize, bool),
    // Location.
    PickZone(usize),
    HoverZone(Option<usize>),
    ZoneFromList(usize),
    Detect,
    Zone(u64, Result<timezone::Detection, String>),
    Internet(bool),
    Locale(usize),
    ZoneClock,
    // Keyboard.
    Keyboard(usize),
    /// Wait for the key beside the space bar, or stop waiting.
    ListenForKey,
    StopListening,
    /// Wait `n`'s key: its evdev code and keyboard, or `None` if given up on.
    KeyCaught(u64, Result<Option<(u32, Option<KeyboardInfo>)>, String>),
    SuperKey(usize),
    // Page cache warming.
    WarmStart(u64),
    Warm {
        generation: u64,
        read: u64,
        total: u64,
        done: bool,
    },
    // Review and installation.
    Reviewed(u64, Handoff<Result<(Box<Review>, Session), String>>),
    Session(u64, Helper),
    Erase(String),
    Consent(bool),
    Tick,
    ToggleLog,
    CopyFailure,
    Power(&'static str),
    PowerRequested(&'static str, Result<(), String>),
    Close,
    CloseRefused,
    Dismissed,
}

/// Display copy of a reviewed plan. The plan itself moves to the helper.
#[derive(Clone, Debug, PartialEq)]
pub struct Review {
    pub disk: disk::Identity,
    pub firmware: Firmware,
    pub filesystem: Filesystem,
    pub encrypt: bool,
    pub hostname: String,
    pub username: String,
    pub full_name: String,
    pub locale: String,
    pub timezone: String,
    pub keyboard: String,
    pub super_key: SuperKey,
    pub desktops: Vec<Desktop>,
    pub default_desktop: Desktop,
    pub applications: String,
    pub wifi: bool,
    pub wifi_profiles: usize,
    pub unfree: bool,
    pub swap: bool,
    pub tuning: bool,
    pub graphics: String,
    pub ssh_keys: String,
    pub ssh_server: String,
    pub git: String,
}
impl Review {
    pub fn of(plan: &InstallPlan) -> Self {
        let onboarding = plan.onboarding();
        let ssh_keys = match (onboarding.keys().len(), onboarding.github()) {
            (0, _) | (_, None) => "None".to_string(),
            (1, Some(user)) => format!("1 key from github.com/{}", user.as_str()),
            (n, Some(user)) => format!("{n} keys from github.com/{}", user.as_str()),
        };
        let ssh_server = match (onboarding.ssh_server(), onboarding.password_login()) {
            (false, _) => "Not installed",
            (true, false) => "OpenSSH, key-only login",
            (true, true) => "OpenSSH, password login",
        };
        Self {
            disk: plan.disk().clone(),
            firmware: plan.firmware(),
            filesystem: plan.filesystem(),
            encrypt: plan.encrypt(),
            hostname: plan.hostname().as_str().into(),
            username: plan.username().as_str().into(),
            full_name: plan.full_name().into(),
            locale: plan.locale().into(),
            timezone: plan.timezone().as_str().into(),
            keyboard: plan.keyboard().into(),
            super_key: plan.super_key(),
            desktops: plan.desktops().selected().to_vec(),
            default_desktop: plan.desktops().default(),
            applications: plan.applications().names(),
            wifi: plan.wifi().enabled(),
            wifi_profiles: plan.wifi().profile_count(),
            unfree: plan.allow_unfree(),
            swap: plan.swap(),
            tuning: plan.tuning(),
            graphics: plan.graphics().describe(plan.allow_unfree()),
            ssh_keys,
            ssh_server: ssh_server.into(),
            git: onboarding.git().map_or_else(
                || "Not set".into(),
                |id| format!("{} <{}>", id.name(), id.email()),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepState {
    Working,
    Done,
    Failed,
}

/// The Review page's line about preparation in the background.
#[derive(Clone, Debug, PartialEq)]
pub struct Preparation {
    pub state: PrepState,
    pub title: String,
    pub subtitle: String,
    /// Reading the prepared system into memory, as a fraction.
    pub progress: Option<f64>,
    pub failure: Option<String>,
}

/// Each installation step's state on the Install page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Current,
    Done,
    Failed,
}

/// What the bar at the bottom says, between Back and Next.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub text: String,
    pub busy: bool,
    /// A check that failed, rather than work going on.
    pub warning: bool,
}

pub struct Genkan {
    pub window: Option<WindowKey>,
    /// The sample page shown instead of this computer's (`preview`).
    pub preview: Option<String>,
    pub sender: Option<MsgSender<Msg>>,
    pub step: Step,
    pub reached: Step,
    pub activity: Option<Activity>,
    note: u64,
    /// The field the last failed check pointed at.
    pub flagged: Option<Field>,

    // Disk.
    pub disks: Vec<Disk>,
    pub firmware: Firmware,
    pub selected_disk: Option<usize>,
    pub memory: Option<MemInfo>,
    pub scanning: bool,
    pub filesystem: Filesystem,
    pub swap: bool,
    pub encrypt: bool,
    pub passphrase: Zeroizing<String>,
    pub passphrase_repeat: Zeroizing<String>,
    pub tuning: bool,

    // Account.
    pub full_name: String,
    pub username: String,
    pub username_edited: bool,
    pub password: Zeroizing<String>,
    pub repeat: Zeroizing<String>,
    pub hostname: String,

    // GitHub, SSH and Git.
    pub github_user: String,
    /// The last GitHub lookup that answered, for the username it was for.
    pub github_profile: Option<github::Profile>,
    github_epoch: u64,
    pub github_pending: Option<u64>,
    /// Next was pressed before the lookup: continue when it succeeds.
    github_advance: bool,
    pub github_status: String,
    pub authorize: bool,
    pub ssh_server: bool,
    pub git_name: String,
    pub git_email: String,
    /// The Git fields follow the account and GitHub until edited.
    git_name_edited: bool,
    git_email_edited: bool,

    // Desktops and software.
    /// The chosen desktops, in `Desktop::ALL`'s order.
    pub desktops: Vec<Desktop>,
    pub default_choice: Option<Desktop>,
    pub wifi: bool,
    pub unfree: bool,
    pub app_query: String,
    /// Chosen applications, by their place in the catalog.
    pub apps: Vec<bool>,

    // Location.
    pub zones: Option<Rc<Zones>>,
    pub map: Option<Rc<crate::view::map::Shapes>>,
    pub zoneinfo: String,
    /// Index into `zones.places` of the chosen time zone.
    pub zone: Option<usize>,
    pub zone_hover: Option<usize>,
    /// Detections apply only while the user has not chosen a zone.
    zone_manual: bool,
    /// The last detection found nothing and fell back to New York.
    zone_fallback: bool,
    zone_epoch: u64,
    pub zone_pending: Option<u64>,
    pub zone_status: String,
    pub zone_clock: Option<zone::Clock>,
    pub internet: bool,
    pub locale: usize,

    // Keyboard.
    pub keyboard: usize,
    /// The key just left of the space bar: waited for, or what it was.
    pub key_check: KeyCheck,
    /// A key that wasn't the one asked for, said while waiting again.
    pub key_error: Option<String>,
    /// The wait going on; moving it on gives that wait up.
    key_waits: Arc<AtomicU64>,
    /// What the installed system makes of that key.
    pub super_key: SuperKey,

    // Review.
    pub reviewing: bool,
    generation: u64,
    session: Option<Session>,
    pub review: Option<Review>,
    pub phrase: String,
    pub erase: String,
    pub consent: bool,
    pub prepared: bool,
    pub preparation: Preparation,

    // Installation.
    pub installing: bool,
    started: Option<Instant>,
    pub elapsed: u64,
    pub install_step: u8,
    pub steps: [StepState; 6],
    pub copy_fraction: f64,
    pub copied: Option<String>,
    pub install_message: String,
    pub completed: bool,
    pub failed: bool,
    pub failure: Option<String>,
    pub total_seconds: Option<f64>,
    pub log: Rc<VecDeque<String>>,
    pub log_open: bool,
    pub powering: bool,
    tick: Option<TimerHandle>,

    // Page cache warming.
    warm_generation: u64,
    warm_cancel: Option<Arc<AtomicBool>>,
    warm_timer: Option<TimerHandle>,
}

impl Genkan {
    pub fn new(preview: Option<String>) -> Self {
        let zoneinfo = zone::zoneinfo_dir();
        let zones = match Zones::load(std::path::Path::new(&zoneinfo)) {
            Ok(zones) => Some(Rc::new(zones)),
            Err(error) => {
                eprintln!("time zone map unavailable: {error:#}");
                None
            }
        };
        Self::with_zones(zones, zoneinfo, preview)
    }

    pub fn with_zones(zones: Option<Rc<Zones>>, zoneinfo: String, preview: Option<String>) -> Self {
        let map = zones
            .as_ref()
            .map(|z| Rc::new(crate::view::map::Shapes::of(z)));
        let mut app = Self {
            window: None,
            preview,
            sender: None,
            step: Step::Welcome,
            reached: Step::Welcome,
            activity: None,
            note: 0,
            flagged: None,
            disks: Vec::new(),
            firmware: Firmware::Bios,
            selected_disk: None,
            memory: None,
            scanning: false,
            filesystem: Filesystem::default(),
            swap: true,
            encrypt: false,
            passphrase: Zeroizing::default(),
            passphrase_repeat: Zeroizing::default(),
            tuning: true,
            full_name: String::new(),
            username: String::new(),
            username_edited: false,
            password: Zeroizing::default(),
            repeat: Zeroizing::default(),
            hostname: "nixos".into(),
            github_user: String::new(),
            github_profile: None,
            github_epoch: 0,
            github_pending: None,
            github_advance: false,
            github_status: GITHUB_HINT.into(),
            authorize: true,
            ssh_server: false,
            git_name: String::new(),
            git_email: String::new(),
            git_name_edited: false,
            git_email_edited: false,
            // Aizome, logged in to by default, and Tatami beside it.
            desktops: vec![Desktop::Aizome, Desktop::Tatami],
            default_choice: Some(Desktop::Aizome),
            wifi: true,
            unfree: calamares_nixos::DEFAULT_ALLOW_UNFREE,
            app_query: String::new(),
            apps: applications::catalog()
                .iter()
                .map(|app| app.id == "firefox")
                .collect(),
            zones,
            map,
            zoneinfo,
            zone: None,
            zone_hover: None,
            zone_manual: false,
            zone_fallback: false,
            zone_epoch: 0,
            zone_pending: None,
            zone_status: "Finding your location…".into(),
            zone_clock: None,
            internet: true,
            locale: 0,
            keyboard: 0,
            key_check: KeyCheck::Asking,
            key_error: None,
            key_waits: Arc::default(),
            super_key: SuperKey::AsIs,
            reviewing: false,
            generation: 0,
            session: None,
            review: None,
            phrase: String::new(),
            erase: String::new(),
            consent: false,
            prepared: false,
            preparation: Preparation {
                state: PrepState::Working,
                title: "Waiting for authorization…".into(),
                subtitle: String::new(),
                progress: None,
                failure: None,
            },
            installing: false,
            started: None,
            elapsed: 0,
            install_step: 0,
            steps: [StepState::Pending; 6],
            copy_fraction: 0.0,
            copied: None,
            install_message: "Starting…".into(),
            completed: false,
            failed: false,
            failure: None,
            total_seconds: None,
            log: Rc::default(),
            log_open: false,
            powering: false,
            tick: None,
            warm_generation: 0,
            warm_cancel: None,
            warm_timer: None,
        };
        app.normalize_apps();
        app
    }

    // ---- Notes and navigation ---------------------------------------------

    /// Say `text` in the bar at the bottom for a while.
    pub fn note(&mut self, text: &str, cx: &mut Cx<Msg>) {
        self.note += 1;
        self.activity = Some(Activity {
            text: text.into(),
            busy: false,
            warning: true,
        });
        cx.after(NOTE_FOR, Msg::ClearNote(self.note));
    }

    fn flag(&mut self, field: Field, cx: &mut Cx<Msg>) {
        self.flagged = Some(field);
        cx.focus(field.id());
    }

    pub fn go(&mut self, step: Step, cx: &mut Cx<Msg>) {
        // The network may have come up since detection found nothing.
        if step == Step::Location
            && self.step != step
            && self.zone_fallback
            && !self.zone_manual
            && self.zone_pending.is_none()
        {
            self.detect_zone(cx);
        }
        // The keyboard step asks for its key as it opens, and stops asking
        // when it's left: the compositor holds back the next key from every
        // app while Genkan waits for it.
        if step != Step::Keyboard {
            self.stop_listening();
        } else if self.step != step && self.key_check == KeyCheck::Asking && self.preview.is_none()
        {
            self.listen_for_key();
        }
        self.step = step;
        if step > self.reached {
            self.reached = step;
        }
        if step == Step::Location {
            self.refresh_zone_clock();
        }
    }

    /// Review and installation can't be left through the sidebar.
    pub fn locked(&self) -> bool {
        self.reviewing || self.installing || self.step >= Step::Review
    }

    /// Whether the sidebar's row for `step` can be chosen.
    pub fn reachable(&self, step: Step) -> bool {
        step == self.step || (!self.locked() && step <= self.reached && step <= Step::Keyboard)
    }

    fn jump(&mut self, step: Step, cx: &mut Cx<Msg>) {
        if !self.locked() && step <= self.reached && step <= Step::Keyboard {
            self.go(step, cx);
        }
    }

    pub fn finished(&self) -> bool {
        self.completed || (self.failed && !self.installing)
    }

    /// Next's label, and whether it's the destructive one.
    pub fn next_label(&self) -> &'static str {
        match self.step {
            Step::Welcome => "Get Started",
            Step::Keyboard if self.reviewing => "Checking…",
            Step::Keyboard => "Review",
            Step::Review => "Erase Disk and Install",
            Step::Install => "Close Installer",
            _ => "Next",
        }
    }

    pub fn next_enabled(&self) -> bool {
        match self.step {
            Step::Review => self.confirmation_ready(),
            Step::Install => self.finished(),
            _ => !self.reviewing,
        }
    }

    fn forward(&mut self, cx: &mut Cx<Msg>) {
        let result = match self.step {
            Step::Welcome => Ok(Step::Disk),
            Step::Disk => self.check_disk(cx).map(|()| Step::Account),
            Step::Account => self.check_account(cx).map(|()| Step::GitHub),
            Step::GitHub => {
                // Look the account up first, then continue on success.
                if self.github_pending.is_some() || self.github_needs_lookup() {
                    self.github_advance = true;
                    if self.github_pending.is_none() {
                        self.lookup_github(cx);
                    }
                    return;
                }
                self.check_github(cx).map(|()| Step::Desktop)
            }
            Step::Desktop => self.check_desktops().map(|()| Step::Applications),
            Step::Applications => Ok(Step::Location),
            Step::Location => Ok(Step::Keyboard),
            Step::Keyboard => {
                self.start_review(cx);
                return;
            }
            Step::Review => {
                self.confirm_install(cx);
                return;
            }
            Step::Install => {
                if self.finished() {
                    self.close(cx);
                }
                return;
            }
        };
        match result {
            Ok(step) => self.go(step, cx),
            Err(message) => self.note(&message, cx),
        }
    }

    fn backward(&mut self, cx: &mut Cx<Msg>) {
        if self.reviewing {
            return;
        }
        match self.step {
            Step::Welcome | Step::Install => {}
            Step::Review => {
                // Withdraw the unconfirmed request; nothing was written.
                self.session.take();
                self.generation += 1;
                self.go(Step::Keyboard, cx);
                self.schedule_warm(cx);
            }
            step => self.go(Step::ALL[step.index() - 1], cx),
        }
    }

    /// Close the window, which ends the program; never while installing.
    fn close(&mut self, cx: &mut Cx<Msg>) {
        if let Some(window) = self.window {
            cx.close_window(window);
        }
    }

    // ---- Disk -------------------------------------------------------------

    fn rescan(&mut self, cx: &mut Cx<Msg>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        cx.spawn(|| {
            Msg::Scanned(
                disk::discover()
                    .map(|d| (d, Firmware::current()))
                    .map_err(|e| format!("{e:#}")),
            )
        });
    }

    fn read_memory(&mut self, cx: &mut Cx<Msg>) {
        cx.spawn(|| Msg::Memory(memory::read().map_err(|e| format!("{e:#}"))));
    }

    pub fn show_disks(&mut self, found: Vec<Disk>, firmware: Firmware) {
        self.selected_disk = None;
        self.firmware = firmware;
        self.disks = found;
    }

    pub fn swap_bytes(&self) -> Option<u64> {
        self.swap
            .then(|| self.memory.map(|m| memory::swap_bytes(m.total)))
            .flatten()
    }

    pub fn selected(&self) -> Option<&Disk> {
        self.selected_disk.and_then(|i| self.disks.get(i))
    }

    /// The new partitions for the chosen disk, or why there are none yet.
    pub fn layout(&self) -> Result<Layout, String> {
        let Some(disk) = self.selected() else {
            return Err("Select a disk to preview its new partitions.".into());
        };
        if self.swap && self.memory.is_none() {
            return Err("Reading installed memory…".into());
        }
        Layout::for_install(
            disk.identity.bytes,
            self.firmware,
            self.swap_bytes(),
            self.encrypt,
        )
        .map_err(|error| format!("{error:#}"))
    }

    pub fn passphrase_error(&self) -> Option<String> {
        calamares_nixos::encryption::validate_passphrase(&self.passphrase)
            .err()
            .map(|e| e.to_string())
    }

    fn check_disk(&mut self, cx: &mut Cx<Msg>) -> Result<(), String> {
        if self.encrypt {
            if let Some(error) = self.passphrase_error() {
                self.flag(Field::Passphrase, cx);
                return Err(error);
            }
            if *self.passphrase != *self.passphrase_repeat {
                self.flag(Field::PassphraseRepeat, cx);
                return Err("Disk passphrases do not match.".into());
            }
        }
        let disk = self
            .selected()
            .ok_or("Select the disk to erase. No disk is selected automatically.")?;
        if let Some(reason) = &disk.blocked {
            return Err(reason.clone());
        }
        if self.swap && self.memory.is_none() {
            return Err("Still reading installed memory; try again in a moment.".into());
        }
        self.layout().map(|_| ())
    }

    // ---- Account ----------------------------------------------------------

    pub fn account_errors(&self) -> Vec<(Field, String)> {
        let mut errors = Vec::new();
        let full_name = &self.full_name;
        if full_name.len() > 128
            || full_name.contains(':')
            || full_name.chars().any(char::is_control)
        {
            errors.push((Field::FullName, "The full name cannot contain a colon or control characters, and is limited to 128 bytes.".to_string()));
        }
        if let Err(e) = Username::parse(&self.username) {
            errors.push((Field::Username, e.to_string()));
        }
        let password = &self.password;
        if password.chars().count() < 12
            || password.len() > 1024
            || password.chars().any(char::is_control)
        {
            errors.push((
                Field::Password,
                "The password needs at least 12 characters and no control characters.".into(),
            ));
        } else if **password != *self.repeat {
            errors.push((Field::Repeat, "The passwords do not match.".into()));
        }
        if let Err(e) = Hostname::parse(&self.hostname) {
            errors.push((Field::Hostname, e.to_string()));
        }
        errors
    }

    /// A field's error, shown once it holds text (or a check pointed at
    /// it), without nagging about fields the user hasn't reached yet.
    pub fn account_error(&self, field: Field) -> Option<String> {
        let text: &str = match field {
            Field::FullName => &self.full_name,
            Field::Username => &self.username,
            Field::Password => &self.password,
            Field::Repeat => &self.repeat,
            Field::Hostname => &self.hostname,
            _ => return None,
        };
        if text.is_empty() && self.flagged != Some(field) {
            return None;
        }
        self.account_errors()
            .into_iter()
            .find(|(f, _)| *f == field)
            .map(|(_, message)| message)
    }

    pub fn password_hint(&self) -> String {
        let length = self.password.chars().count();
        if length == 0 {
            "Use at least 12 characters. Passwords are typed with this live session's keyboard layout.".to_string()
        } else if length < 12 {
            format!(
                "{} more characters needed. Passwords are typed with this live session's keyboard layout.",
                12 - length
            )
        } else if *self.password != *self.repeat {
            "Long enough. Now confirm it in the second field.".to_string()
        } else {
            "✓ Password set. Passwords are typed with this live session's keyboard layout."
                .to_string()
        }
    }

    fn check_account(&mut self, cx: &mut Cx<Msg>) -> Result<(), String> {
        match self.account_errors().into_iter().next() {
            Some((field, message)) => {
                self.flag(field, cx);
                Err(message)
            }
            None => Ok(()),
        }
    }

    fn full_name_changed(&mut self, text: String) {
        self.full_name = text;
        if !self.username_edited {
            self.username = suggest_username(&self.full_name);
        }
        // The Git name follows the full name until it is edited.
        if !self.git_name_edited {
            let name = self.git_name_suggestion();
            self.fill_git(Some(&name), None);
        }
    }

    // ---- GitHub, SSH and Git ----------------------------------------------

    /// The GitHub username as typed; empty skips GitHub.
    fn github_text(&self) -> String {
        self.github_user.trim().to_string()
    }

    /// The last lookup answered for the username now in the field.
    pub fn github_profile_current(&self) -> bool {
        let text = self.github_text();
        !text.is_empty()
            && self
                .github_profile
                .as_ref()
                .is_some_and(|p| p.user.as_str().eq_ignore_ascii_case(&text))
    }

    fn github_needs_lookup(&self) -> bool {
        let text = self.github_text();
        !text.is_empty() && GithubUser::parse(&text).is_ok() && !self.github_profile_current()
    }

    /// The username's error, as typed.
    pub fn github_error(&self) -> Option<String> {
        let text = self.github_text();
        (!text.is_empty())
            .then(|| GithubUser::parse(&text).err().map(|e| e.to_string()))
            .flatten()
    }

    /// Look Up is offered for a valid username not looked up yet.
    pub fn can_look_up(&self) -> bool {
        !self.github_text().is_empty()
            && self.github_error().is_none()
            && !self.github_profile_current()
            && self.github_pending.is_none()
    }

    /// The keys found for the username in the field.
    pub fn github_keys(&self) -> &[github::AuthorizedKey] {
        match &self.github_profile {
            Some(profile) if self.github_profile_current() => &profile.keys,
            _ => &[],
        }
    }

    fn lookup_github(&mut self, cx: &mut Cx<Msg>) {
        let text = self.github_text();
        if text.is_empty() || self.github_profile_current() {
            return;
        }
        let user = match GithubUser::parse(&text) {
            Ok(user) => user,
            Err(error) => {
                self.github_advance = false;
                self.flagged = Some(Field::GithubUser);
                self.github_status = error.to_string();
                return;
            }
        };
        self.github_epoch = self.github_epoch.wrapping_add(1);
        let epoch = self.github_epoch;
        self.github_pending = Some(epoch);
        self.github_status = format!("Looking up {} on GitHub…", user.as_str());
        cx.spawn(move || Msg::Github(epoch, github::fetch(&user).map_err(|e| format!("{e:#}"))));
    }

    /// Keys found for one username never apply to another.
    fn github_username_changed(&mut self, text: String) {
        self.github_user = text;
        if self.flagged == Some(Field::GithubUser) {
            self.flagged = None;
        }
        if self.github_pending.take().is_some() {
            self.github_epoch = self.github_epoch.wrapping_add(1);
        }
        self.github_advance = false;
        let text = self.github_text();
        self.github_status = if self.github_profile_current() {
            self.profile_status()
        } else {
            match self.github_error() {
                Some(error) => error,
                None if text.is_empty() => GITHUB_HINT.into(),
                None => "Press Return or Look Up to fetch this account's public keys.".into(),
            }
        };
    }

    fn profile_status(&self) -> String {
        let Some(profile) = &self.github_profile else {
            return String::new();
        };
        let login = profile.user.as_str();
        let mut status = match profile.keys.len() {
            0 => format!("github.com/{login} has no public SSH keys."),
            1 => format!("Found 1 public SSH key on github.com/{login}."),
            n => format!("Found {n} public SSH keys on github.com/{login}."),
        };
        if profile.skipped > 0 {
            status.push_str(&format!(
                " {} of a type OpenSSH does not accept left out.",
                profile.skipped
            ));
        }
        status
    }

    /// The account's full name, or the GitHub profile's when it has none: a
    /// name typed into this installer wins over a profile's display name.
    fn git_name_suggestion(&self) -> String {
        let typed = self.full_name.trim().to_string();
        if !typed.is_empty() || !self.github_profile_current() {
            return typed;
        }
        self.github_profile
            .as_ref()
            .and_then(|p| p.name.clone())
            .unwrap_or_default()
    }

    /// Fill the Git fields that the user has not edited.
    fn fill_git(&mut self, name: Option<&str>, email: Option<&str>) {
        if let Some(name) = name.filter(|_| !self.git_name_edited) {
            self.git_name = name.into();
        }
        if let Some(email) = email.filter(|_| !self.git_email_edited) {
            self.git_email = email.into();
        }
    }

    /// Git's name and address for the request. A name filled in from the
    /// account alone, with no address, means no identity.
    pub fn git_fields(&self) -> (String, String) {
        let name = self.git_name.trim().to_string();
        let email = self.git_email.trim().to_string();
        if email.is_empty() && !self.git_name_edited {
            (String::new(), String::new())
        } else {
            (name, email)
        }
    }

    /// What's wrong with the Git name as typed, or with the identity when a
    /// check pointed at the name.
    pub fn git_name_error(&self) -> Option<String> {
        let name = self.git_name.trim();
        if !name.is_empty()
            && let Err(error) = GitIdentity::parse(name, "git@example.com")
        {
            return Some(error.to_string());
        }
        self.flagged_git_error(Field::GitName)
    }

    pub fn git_email_error(&self) -> Option<String> {
        let email = self.git_email.trim();
        if !email.is_empty()
            && let Err(error) = GitIdentity::parse("Git", email)
        {
            return Some(error.to_string());
        }
        self.flagged_git_error(Field::GitEmail)
    }

    fn flagged_git_error(&self, field: Field) -> Option<String> {
        if self.flagged != Some(field) {
            return None;
        }
        let (name, email) = self.git_fields();
        GitIdentity::parse(&name, &email)
            .err()
            .map(|e| e.to_string())
    }

    /// The GitHub account and the keys to authorize, as the request lists them.
    pub fn github_request(&self) -> (String, Vec<String>) {
        if !self.github_profile_current() {
            return (String::new(), vec![]);
        }
        let Some(profile) = &self.github_profile else {
            return (String::new(), vec![]);
        };
        let keys = if self.authorize {
            profile.keys.iter().map(|k| k.text()).collect()
        } else {
            vec![]
        };
        (profile.user.as_str().into(), keys)
    }

    pub fn ssh_server_description(&self) -> &'static str {
        if self.github_request().1.is_empty() {
            "OpenSSH, open in the firewall. With no authorized keys it accepts your account's password."
        } else {
            "OpenSSH, open in the firewall. Only your GitHub keys can log in: passwords are refused."
        }
    }

    fn check_github(&mut self, cx: &mut Cx<Msg>) -> Result<(), String> {
        let text = self.github_text();
        if !text.is_empty() {
            if let Err(error) = GithubUser::parse(&text) {
                self.flag(Field::GithubUser, cx);
                return Err(error.to_string());
            }
            if !self.github_profile_current() {
                cx.focus(Field::GithubUser.id());
                return Err(
                    "Look up the GitHub account, or clear its username to skip GitHub.".into(),
                );
            }
        }
        let (name, email) = self.git_fields();
        if let Err(error) = GitIdentity::parse(&name, &email) {
            let field = if name.is_empty() || GitIdentity::parse(&name, "git@example.com").is_err()
            {
                Field::GitName
            } else {
                Field::GitEmail
            };
            self.flag(field, cx);
            return Err(error.to_string());
        }
        Ok(())
    }

    // ---- Desktops and applications ----------------------------------------

    fn toggle_desktop(&mut self, desktop: Desktop, on: bool) {
        let mut selected: BTreeSet<usize> = self
            .desktops
            .iter()
            .filter_map(|d| Desktop::ALL.iter().position(|a| a == d))
            .collect();
        if let Some(index) = Desktop::ALL.iter().position(|d| *d == desktop) {
            if on {
                selected.insert(index);
            } else {
                selected.remove(&index);
            }
        }
        self.desktops = selected.into_iter().map(|i| Desktop::ALL[i]).collect();
        if self
            .default_choice
            .is_none_or(|c| !self.desktops.contains(&c))
        {
            self.default_choice = self.desktops.first().copied();
        }
    }

    fn check_desktops(&self) -> Result<(), String> {
        if self.desktops.is_empty() {
            return Err("Select at least one desktop environment.".into());
        }
        Ok(())
    }

    /// Selection rules the helper's parser enforces: proprietary choices need
    /// the unfree-software permission, and Rustup brings the build tools.
    fn normalize_apps(&mut self) {
        let catalog = applications::catalog();
        for (app, on) in catalog.iter().zip(self.apps.iter_mut()) {
            if app.unfree && !self.unfree {
                *on = false;
            }
        }
        for id in self.required_apps() {
            if let Some(index) = catalog.iter().position(|app| app.id == id) {
                self.apps[index] = true;
            }
        }
    }

    /// Applications another chosen application needs.
    pub fn required_apps(&self) -> BTreeSet<String> {
        applications::catalog()
            .iter()
            .zip(&self.apps)
            .filter(|(_, on)| **on)
            .flat_map(|(app, _)| app.requires.iter().cloned())
            .collect()
    }

    pub fn selected_apps(&self) -> Vec<String> {
        applications::catalog()
            .iter()
            .zip(&self.apps)
            .filter(|(_, on)| **on)
            .map(|(app, _)| app.id.clone())
            .collect()
    }

    // ---- Time zone --------------------------------------------------------

    fn detect_zone(&mut self, cx: &mut Cx<Msg>) {
        self.zone_epoch = self.zone_epoch.wrapping_add(1);
        let generation = self.zone_epoch;
        self.zone_pending = Some(generation);
        self.zone_status = "Finding your location…".into();
        let internet = self.internet;
        let zoneinfo = self.zoneinfo.clone();
        cx.spawn(move || {
            let offset_now = |zone: &str| zone::offset_minutes(&zoneinfo, zone);
            Msg::Zone(
                generation,
                timezone::detect(std::path::Path::new(&zoneinfo), internet, &offset_now)
                    .map_err(|e| format!("{e:#}")),
            )
        });
    }

    /// A zone the user picked; later detections no longer replace it.
    fn choose_zone(&mut self, index: usize, note: &str) {
        self.zone_manual = true;
        self.zone_fallback = false;
        self.zone_epoch = self.zone_epoch.wrapping_add(1);
        self.zone_pending = None;
        self.show_zone(index, note);
    }

    pub fn show_zone(&mut self, index: usize, note: &str) {
        if self.zones.as_ref().is_none_or(|z| index >= z.places.len()) {
            return;
        }
        self.zone = Some(index);
        self.refresh_zone_clock();
        self.zone_status = note.into();
    }

    pub fn selected_place(&self) -> Option<&calamares_nixos::zonemap::Place> {
        self.zones.as_ref()?.places.get(self.zone?)
    }

    fn refresh_zone_clock(&mut self) {
        self.zone_clock = self
            .selected_place()
            .map(|place| zone::Clock::of(&self.zoneinfo, place));
    }

    // ---- Speculative caching ----------------------------------------------

    // ---- Keyboard ---------------------------------------------------------

    /// Ask the compositor for the next key, on a thread of its own (the wait
    /// lasts until a key comes or it's given up on), as Settei's keyboard
    /// setup does.
    pub fn listen_for_key(&mut self) {
        let Some(send) = self.sender.clone() else {
            return;
        };
        let n = self.key_waits.fetch_add(1, Ordering::SeqCst) + 1;
        let waits = self.key_waits.clone();
        self.key_check = KeyCheck::Waiting;
        thread::spawn(move || {
            let tag = format!("genkan-{}-{n}", std::process::id());
            let caught = aizome_ipc::setup::catch_key(&tag, || waits.load(Ordering::SeqCst) != n);
            send.send(Msg::KeyCaught(n, caught));
        });
    }

    /// Give the wait up, if there is one; the key stays unasked.
    fn stop_listening(&mut self) {
        self.key_waits.fetch_add(1, Ordering::SeqCst);
        self.key_error = None;
        if self.key_check == KeyCheck::Waiting {
            self.key_check = KeyCheck::Asking;
        }
    }

    /// Wait `n` answered.
    fn key_caught(&mut self, n: u64, caught: Result<Option<(u32, Option<KeyboardInfo>)>, String>) {
        if n != self.key_waits.load(Ordering::SeqCst) || self.key_check != KeyCheck::Waiting {
            return;
        }
        match caught {
            Ok(None) => {}
            Ok(Some((aizome_ipc::setup::ESCAPE, _))) => self.stop_listening(),
            Ok(Some((code, keyboard))) => self.caught(code, keyboard),
            Err(error) => {
                self.key_error = None;
                self.key_check = KeyCheck::Unavailable(error);
            }
        }
    }

    /// The key beside the space bar came: what it is says where ❖ goes, and
    /// Genkan suggests the arrangement that puts it there. The space bar, or
    /// a key right of it, asks again.
    pub fn caught(&mut self, code: u32, keyboard: Option<KeyboardInfo>) {
        match aizome_ipc::setup::beside_space(code) {
            Err(again) => {
                self.key_error = Some(again.into());
                self.listen_for_key();
            }
            Ok(beside) => {
                if let Some(key) = beside.suggest.and_then(SuperKey::from_xkb_options) {
                    self.super_key = key;
                }
                self.key_error = None;
                self.key_check = KeyCheck::Caught { keyboard, beside };
            }
        }
    }

    /// What the review says of the key beside the space bar.
    pub fn super_key_name(key: SuperKey) -> &'static str {
        SUPER_KEY_NAMES[SuperKey::ALL.iter().position(|k| *k == key).unwrap_or(0)]
    }

    fn stop_warm(&mut self, cx: &mut Cx<Msg>) {
        if let Some(timer) = self.warm_timer.take() {
            cx.cancel(timer);
        }
        if let Some(cancel) = self.warm_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.warm_generation += 1;
        if self.activity.as_ref().is_some_and(|a| !a.warning) {
            self.activity = None;
        }
    }

    /// Warm the page cache with the media's reference closures for the
    /// current selection, a couple of seconds after it stops changing.
    fn schedule_warm(&mut self, cx: &mut Cx<Msg>) {
        if self.preview.is_some() || self.step >= Step::Review || self.reviewing {
            return;
        }
        self.stop_warm(cx);
        self.warm_timer =
            Some(cx.after(Duration::from_secs(2), Msg::WarmStart(self.warm_generation)));
    }

    fn start_warm(&mut self, generation: u64) {
        let Some(send) = self.sender.clone() else {
            return;
        };
        self.warm_timer = None;
        let desktops = self.desktops.clone();
        let applications = self.selected_apps();
        let cancel = Arc::new(AtomicBool::new(false));
        self.warm_cancel = Some(cancel.clone());
        thread::spawn(move || {
            let lists = precache::reference_lists(&desktops, &applications);
            let outcome = if lists.is_empty() {
                Ok(precache::Outcome::default())
            } else {
                precache::read_lists(&lists).and_then(|paths| {
                    // Progress is advisory: a few updates a second at most.
                    let mut last = Instant::now() - Duration::from_secs(1);
                    precache::warm(&paths, &cancel, |read, total| {
                        if last.elapsed() >= Duration::from_millis(250) {
                            last = Instant::now();
                            send.send(Msg::Warm {
                                generation,
                                read,
                                total,
                                done: false,
                            });
                        }
                    })
                })
            };
            let (read, total) = outcome.map(|o| (o.bytes, o.total)).unwrap_or((0, 0));
            send.send(Msg::Warm {
                generation,
                read,
                total,
                done: true,
            });
        });
    }

    // ---- Review -----------------------------------------------------------

    fn start_review(&mut self, cx: &mut Cx<Msg>) {
        if self.reviewing {
            return;
        }
        // In order, stopping at the first that fails, so the field it points
        // at is on the page it goes to.
        for step in [Step::Disk, Step::Account, Step::GitHub, Step::Desktop] {
            let check = match step {
                Step::Disk => self.check_disk(cx),
                Step::Account => self.check_account(cx),
                Step::GitHub => self.check_github(cx),
                _ => self.check_desktops(),
            };
            if let Err(message) = check {
                self.go(step, cx);
                self.note(&message, cx);
                return;
            }
        }
        self.flagged = None;
        let Some(timezone) = self.selected_place().map(|p| p.zone.clone()) else {
            self.go(Step::Location, cx);
            self.note("Choose your time zone on the map.", cx);
            return;
        };
        let Some(disk) = self.selected().cloned() else {
            self.go(Step::Disk, cx);
            return;
        };
        let Some(send) = self.sender.clone() else {
            return;
        };
        let (github_user, ssh_keys) = self.github_request();
        let (git_name, git_email) = self.git_fields();
        let request = RawRequest {
            confirmation: String::new(),
            disk: disk.identity,
            firmware: self.firmware,
            filesystem: self.filesystem,
            encrypt: self.encrypt,
            encryption_password: if self.encrypt {
                self.passphrase.to_string()
            } else {
                String::new()
            },
            hostname: self.hostname.clone(),
            username: self.username.clone(),
            full_name: self.full_name.clone(),
            password: self.password.to_string(),
            locale: LOCALES.get(self.locale).unwrap_or(&LOCALES[0]).to_string(),
            timezone,
            keyboard: KEYBOARDS
                .get(self.keyboard)
                .unwrap_or(&KEYBOARDS[0])
                .to_string(),
            super_key: self.super_key,
            desktops: self.desktops.clone(),
            default_desktop: self.default_choice.unwrap_or(Desktop::Aizome),
            applications: self.selected_apps(),
            copy_wifi: self.wifi,
            wifi_profiles: vec![],
            allow_unfree: self.unfree,
            swap: self.swap,
            tuning: self.tuning,
            graphics: calamares_nixos::graphics::Graphics::detect(),
            github_user,
            ssh_keys,
            ssh_server: self.ssh_server,
            git_name,
            git_email,
        };
        self.stop_warm(cx);
        self.reviewing = true;
        self.generation += 1;
        let generation = self.generation;
        self.activity = Some(Activity {
            text: "Checking your settings…".into(),
            busy: true,
            warning: false,
        });
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<(Box<Review>, Session)> {
                let plan = request.parse(&Settings::load()?)?;
                disk::revalidate(plan.disk())?;
                let plan = plan.snapshot_wifi()?;
                let review = Box::new(Review::of(&plan));
                let events = send.clone();
                let session = Session::start(plan, move |update| {
                    events.send(Msg::Session(generation, update.into()));
                })?;
                Ok((review, session))
            })()
            .map_err(|e| format!("{e:#}"));
            send.send(Msg::Reviewed(generation, Handoff::new(result)));
        });
    }

    pub fn show_review(&mut self, review: Review) {
        self.phrase = format!("ERASE {}", review.disk.path);
        self.review = Some(review);
        self.erase.clear();
        self.consent = false;
        self.prepared = false;
        self.preparation = Preparation {
            state: PrepState::Working,
            title: "Waiting for authorization…".into(),
            subtitle: "The installation helper repeats every safety check before any disk write."
                .into(),
            progress: None,
            failure: None,
        };
    }

    pub fn confirmation_ready(&self) -> bool {
        self.consent
            && self.preparation.state != PrepState::Failed
            && (self.session.is_some() || self.preview.is_some())
            && !self.phrase.is_empty()
            && self.erase == self.phrase
    }

    fn prepared_text(summary: &Summary) -> (String, String) {
        if summary.deferred {
            return (
                "Ready to install".into(),
                format!(
                    "The selection needs more downloads than fit in memory, so the system will be built on the new disk after formatting. This takes longer. Prepared in {}.",
                    crate::format::duration(summary.seconds)
                ),
            );
        }
        let cached = if summary.cache_limited {
            format!(
                "{} of it cached in memory (limited to keep memory free)",
                size(summary.cached_bytes)
            )
        } else {
            "fully cached in memory".to_string()
        };
        (
            "Ready to install".into(),
            format!(
                "The {} system is built and {cached}. Prepared in {}.",
                size(summary.closure_bytes),
                crate::format::duration(summary.seconds)
            ),
        )
    }

    // ---- Installation -----------------------------------------------------

    fn confirm_install(&mut self, cx: &mut Cx<Msg>) {
        if !self.confirmation_ready() {
            self.note(
                "Type the exact phrase and confirm that the disk will be erased.",
                cx,
            );
            return;
        }
        match &self.session {
            Some(session) => session.confirm(&self.phrase),
            None if self.preview.is_some() => {}
            None => {
                self.note(
                    "The installation helper is no longer running. Go back and review again.",
                    cx,
                );
                return;
            }
        }
        self.password = Zeroizing::default();
        self.repeat = Zeroizing::default();
        self.passphrase = Zeroizing::default();
        self.passphrase_repeat = Zeroizing::default();
        self.installing = true;
        self.started = Some(Instant::now());
        self.elapsed = 0;
        self.install_step = 0;
        self.copy_fraction = 0.0;
        self.install_message = if self.prepared {
            "Starting the installation…"
        } else {
            "Finishing preparation; the installation starts as soon as it is ready."
        }
        .into();
        // A preview's still picture keeps its sample time.
        if self.preview.is_none() {
            self.tick = Some(cx.every(Duration::from_millis(500), Msg::Tick));
        }
        self.activity = None;
        self.go(Step::Install, cx);
    }

    pub fn set_install_step(&mut self, step: u8) {
        for (index, state) in self.steps.iter_mut().enumerate() {
            let number = index as u8 + 1;
            *state = if number < step {
                StepState::Done
            } else if number == step {
                StepState::Current
            } else {
                StepState::Pending
            };
        }
        self.install_step = step;
    }

    pub fn fraction(&self) -> f64 {
        if self.completed {
            1.0
        } else if self.install_step == 0 {
            0.0
        } else {
            overall_fraction(self.install_step, self.copy_fraction)
        }
    }

    fn append_log(&mut self, line: &str) {
        let log = Rc::make_mut(&mut self.log);
        log.push_back(line.to_string());
        // Bounded display work, even when Nix reports thousands of paths.
        while log.len() > 300 {
            log.pop_front();
        }
    }

    fn stop_tick(&mut self, cx: &mut Cx<Msg>) {
        let started = self.started.take();
        if let Some(tick) = self.tick.take() {
            cx.cancel(tick);
            if let Some(started) = started {
                self.elapsed = started.elapsed().as_secs();
            }
        }
    }

    pub fn show_failure(&mut self, message: &str, cx: &mut Cx<Msg>) {
        let message: String = message.chars().take(8000).collect();
        if self.installing || self.step == Step::Install {
            self.failed = true;
            self.stop_tick(cx);
            self.failure = Some(message);
            self.install_message = "Installation failed. See the details below.".into();
            let index = usize::from(self.install_step.max(1)) - 1;
            if let Some(state) = self.steps.get_mut(index) {
                *state = StepState::Failed;
            }
        } else {
            self.preparation = Preparation {
                state: PrepState::Failed,
                title: "Preparation failed".into(),
                subtitle: "No disk was written. Go back to change your choices, or copy the details below.".into(),
                progress: None,
                failure: Some(message),
            };
        }
    }

    pub fn show_done(&mut self, cx: &mut Cx<Msg>) {
        self.installing = false;
        self.completed = true;
        self.set_install_step(7);
        self.stop_tick(cx);
    }

    /// The installation's length as the done page gives it.
    pub fn installed_in(&self) -> Option<f64> {
        self.total_seconds
            .or_else(|| (self.elapsed > 0).then_some(self.elapsed as f64))
    }

    /// What the helper said, as if it had (previews and tests).
    pub fn helper_event(&mut self, event: Event, cx: &mut Cx<Msg>) {
        self.session_update(Helper::Event(event), cx);
    }

    fn session_update(&mut self, update: Helper, cx: &mut Cx<Msg>) {
        let installing = self.installing;
        let failed = self.preparation.state == PrepState::Failed;
        match update {
            Helper::Event(Event::Preparing { message }) => {
                if installing && self.install_step == 0 {
                    self.install_message = format!("Finishing preparation: {message}");
                } else if !failed {
                    self.preparation.state = PrepState::Working;
                    self.preparation.title = message.clone();
                    self.preparation.subtitle.clear();
                }
                self.append_log(&message);
            }
            Helper::Event(Event::Log { line }) => {
                if !installing && !self.prepared && !failed {
                    self.preparation.subtitle = line.clone();
                }
                self.append_log(&line);
            }
            Helper::Event(Event::Timing { stage, seconds }) => {
                if stage == "install" {
                    self.total_seconds = Some(seconds);
                }
                self.append_log(&format!("{stage}: {seconds:.1} s"));
            }
            Helper::Event(Event::Caching { read, total }) => {
                if !installing {
                    self.preparation.progress = (total > 0).then(|| read as f64 / total as f64);
                    self.preparation.subtitle =
                        format!("{} of {} read into memory", size(read), size(total));
                }
            }
            Helper::Event(Event::Prepared { summary }) => {
                self.prepared = true;
                let (title, subtitle) = Self::prepared_text(&summary);
                self.preparation = Preparation {
                    state: PrepState::Done,
                    title,
                    subtitle: subtitle.clone(),
                    progress: None,
                    failure: None,
                };
                if installing && self.install_step == 0 {
                    self.install_message =
                        "Preparation complete. Starting the installation…".into();
                }
                self.append_log(&subtitle);
            }
            Helper::Event(Event::Progress { step, message }) => {
                self.set_install_step(step);
                self.append_log(&message);
                self.install_message = message;
            }
            Helper::Event(Event::Copying { bytes, total }) => {
                if total > 0 {
                    self.copy_fraction = bytes as f64 / total as f64;
                    self.copied = Some(format!("{} of {}", size(bytes.min(total)), size(total)));
                }
            }
            Helper::Event(Event::Complete) => self.show_done(cx),
            Helper::Event(Event::Cancelled) => {
                if !installing {
                    self.preparation.state = PrepState::Failed;
                    self.preparation.title = "Preparation cancelled".into();
                    self.preparation.subtitle =
                        "Nothing was written. Go back and review again to continue.".into();
                }
            }
            Helper::Event(Event::Failed { message }) => self.show_failure(&message, cx),
            Helper::Finished(result) => {
                self.session.take();
                let was_installing = std::mem::replace(&mut self.installing, false);
                if self.completed {
                    return;
                }
                if was_installing || self.step == Step::Install {
                    if !self.failed {
                        self.show_failure(
                            &result.err().unwrap_or_else(|| {
                                "The installation helper exited without confirming completion."
                                    .into()
                            }),
                            cx,
                        );
                    }
                } else if self.preparation.state != PrepState::Failed {
                    self.show_failure(
                        &result.err().unwrap_or_else(|| {
                            "The installation helper stopped before the installation was confirmed."
                                .into()
                        }),
                        cx,
                    );
                }
            }
        }
    }

    fn power(&mut self, action: &'static str, cx: &mut Cx<Msg>) {
        let command = if action == "restart" {
            "reboot"
        } else {
            "poweroff"
        };
        self.powering = true;
        // The live store can still be mounted from the installation media:
        // finish shutting down before anyone disconnects it.
        cx.spawn(move || {
            let result = std::process::Command::new("systemctl")
                .arg(command)
                .status()
                .map_err(|e| e.to_string())
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("systemctl {command} failed ({status})"))
                    }
                });
            Msg::PowerRequested(action, result)
        });
    }
}

impl Program for Genkan {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.calamares.NixOSRust", "Genkan", "Install Sekka")
            .icon(Icon::HardDriveDownload)
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.sender = Some(cx.stream());
        let window = cx.open_window(
            WindowSpec::new("Install Sekka")
                .size(1100.0, 760.0)
                .min_size(720.0, 520.0),
        );
        self.window = Some(window);
        if let Some(page) = self.preview.clone() {
            crate::preview::apply(self, cx, &page);
            return;
        }
        self.rescan(cx);
        self.read_memory(cx);
        self.detect_zone(cx);
        self.schedule_warm(cx);
        cx.every(Duration::from_secs(15), Msg::ZoneClock);
        if media::copy_to_ram_failed() {
            cx.alert(
                window,
                Alert::new(
                    "Keep the installation media connected",
                    "Copy to RAM was unavailable or failed. Sekka started from the installation media instead.\n\nKeep your USB drive or DVD connected while this live session is running.",
                )
                .default_button("OK", Msg::Dismissed),
            );
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Next => self.forward(cx),
            Msg::Back => self.backward(cx),
            Msg::Jump(step) => self.jump(step, cx),
            Msg::ClearNote(note) => {
                if note == self.note && self.activity.as_ref().is_some_and(|a| a.warning) {
                    self.activity = None;
                }
            }

            Msg::Rescan => self.rescan(cx),
            Msg::Scanned(result) => {
                self.scanning = false;
                match result {
                    Ok((found, firmware)) => self.show_disks(found, firmware),
                    Err(error) => self.note(&format!("Could not list disks: {error}"), cx),
                }
            }
            Msg::Memory(result) => match result {
                Ok(info) => self.memory = Some(info),
                Err(error) => {
                    self.swap = false;
                    self.note(&format!("Could not read installed memory: {error}"), cx);
                }
            },
            Msg::SelectDisk(index) => {
                if self.disks.get(index).is_some_and(|d| d.blocked.is_none()) {
                    self.selected_disk = Some(index);
                }
            }
            Msg::Filesystem(index) => {
                self.filesystem = Filesystem::ALL.get(index).copied().unwrap_or_default();
            }
            Msg::Swap(on) => self.swap = on,
            Msg::Encrypt(on) => {
                self.encrypt = on;
                if !on {
                    self.passphrase = Zeroizing::default();
                    self.passphrase_repeat = Zeroizing::default();
                }
            }
            Msg::Passphrase(text) => {
                self.passphrase = Zeroizing::new(text);
                if self.flagged == Some(Field::Passphrase) {
                    self.flagged = None;
                }
            }
            Msg::PassphraseRepeat(text) => {
                self.passphrase_repeat = Zeroizing::new(text);
                if self.flagged == Some(Field::PassphraseRepeat) {
                    self.flagged = None;
                }
            }
            Msg::Tuning(on) => self.tuning = on,

            Msg::FullName(text) => {
                self.full_name_changed(text);
                if self.flagged == Some(Field::FullName) {
                    self.flagged = None;
                }
            }
            Msg::Username(text) => {
                self.username_edited = !text.is_empty();
                self.username = text;
                if self.flagged == Some(Field::Username) {
                    self.flagged = None;
                }
            }
            Msg::Password(text) => {
                self.password = Zeroizing::new(text);
                if self.flagged == Some(Field::Password) {
                    self.flagged = None;
                }
            }
            Msg::Repeat(text) => {
                self.repeat = Zeroizing::new(text);
                if self.flagged == Some(Field::Repeat) {
                    self.flagged = None;
                }
            }
            Msg::Hostname(text) => {
                self.hostname = text;
                if self.flagged == Some(Field::Hostname) {
                    self.flagged = None;
                }
            }

            Msg::GithubUser(text) => self.github_username_changed(text),
            Msg::Lookup => self.lookup_github(cx),
            Msg::Github(epoch, result) => {
                // Superseded by an edit or a newer lookup.
                if self.github_pending != Some(epoch) {
                    return;
                }
                self.github_pending = None;
                let advance = std::mem::replace(&mut self.github_advance, false);
                match result {
                    Ok(profile) => {
                        let email = profile.email.clone();
                        self.github_profile = Some(profile);
                        self.github_status = self.profile_status();
                        let name = self.git_name_suggestion();
                        self.fill_git(Some(&name), email.as_deref());
                        if advance && self.step == Step::GitHub {
                            self.forward(cx);
                        }
                    }
                    Err(error) => {
                        self.github_status = error.clone();
                        if advance {
                            self.note(&error, cx);
                        }
                    }
                }
            }
            Msg::Authorize(on) => self.authorize = on,
            Msg::SshServer(on) => self.ssh_server = on,
            Msg::GitName(text) => {
                self.git_name_edited = !text.is_empty();
                self.git_name = text;
                if self.flagged == Some(Field::GitName) {
                    self.flagged = None;
                }
            }
            Msg::GitEmail(text) => {
                self.git_email_edited = !text.is_empty();
                self.git_email = text;
                if self.flagged == Some(Field::GitEmail) {
                    self.flagged = None;
                }
            }

            Msg::ToggleDesktop(desktop, on) => {
                self.toggle_desktop(desktop, on);
                self.schedule_warm(cx);
            }
            Msg::DefaultDesktop(index) => {
                if let Some(desktop) = self.desktops.get(index) {
                    self.default_choice = Some(*desktop);
                }
            }
            Msg::Wifi(on) => self.wifi = on,
            Msg::Unfree(on) => {
                self.unfree = on;
                self.normalize_apps();
                self.schedule_warm(cx);
            }
            Msg::AppSearch(query) => self.app_query = query,
            Msg::ToggleApp(index, on) => {
                let catalog = applications::catalog();
                let Some(app) = catalog.get(index) else {
                    return;
                };
                if (app.unfree && !self.unfree) || (!on && self.required_apps().contains(&app.id)) {
                    return;
                }
                self.apps[index] = on;
                self.normalize_apps();
                self.schedule_warm(cx);
            }

            Msg::PickZone(index) => self.choose_zone(index, "Chosen on the map."),
            Msg::HoverZone(index) => self.zone_hover = index,
            Msg::ZoneFromList(index) => self.choose_zone(index, "Chosen from the list."),
            Msg::Detect => {
                self.zone_manual = false;
                self.detect_zone(cx);
            }
            Msg::Zone(generation, result) => {
                // Superseded by a manual choice or a newer detection.
                if self.zone_pending != Some(generation) {
                    return;
                }
                self.zone_pending = None;
                match result {
                    Ok(found) => {
                        self.zone_fallback = found.source == Source::Fallback;
                        let index = self.zones.as_ref().and_then(|z| {
                            z.find_equivalent(
                                found.zone.as_str(),
                                std::path::Path::new(&self.zoneinfo),
                            )
                        });
                        match index {
                            Some(index) => {
                                self.show_zone(index, &zone::detection_note(found.source))
                            }
                            None => {
                                self.zone_status = format!(
                                    "Detected {}, which is not on the map. Choose your time zone.",
                                    found.zone.as_str()
                                );
                            }
                        }
                    }
                    Err(error) => self.zone_status = error,
                }
            }
            Msg::Internet(on) => {
                self.internet = on;
                if !self.zone_manual {
                    self.detect_zone(cx);
                }
            }
            Msg::Locale(index) => self.locale = index.min(LOCALES.len() - 1),
            Msg::Keyboard(index) => self.keyboard = index.min(KEYBOARDS.len() - 1),
            Msg::ListenForKey => {
                if self.step == Step::Keyboard {
                    self.key_error = None;
                    self.listen_for_key();
                }
            }
            Msg::StopListening => self.stop_listening(),
            Msg::KeyCaught(n, caught) => self.key_caught(n, caught),
            Msg::SuperKey(index) => {
                self.super_key = SuperKey::ALL[index.min(SuperKey::ALL.len() - 1)]
            }
            Msg::ZoneClock => {
                if self.step == Step::Location {
                    self.refresh_zone_clock();
                }
            }

            Msg::WarmStart(generation) => {
                if generation == self.warm_generation && self.step < Step::Review && !self.reviewing
                {
                    self.start_warm(generation);
                }
            }
            Msg::Warm {
                generation,
                read,
                total,
                done,
            } => {
                if generation != self.warm_generation
                    || self.activity.as_ref().is_some_and(|a| a.warning)
                {
                    return;
                }
                self.activity = (total > 0).then(|| Activity {
                    text: if done {
                        format!("{} of selected software cached in memory", size(read))
                    } else {
                        format!(
                            "Caching selected software in memory · {} of {}",
                            size(read),
                            size(total)
                        )
                    },
                    busy: !done,
                    warning: false,
                });
            }

            Msg::Reviewed(generation, result) => {
                // Dropping a stale session cancels it.
                let Some(result) = result.take() else {
                    return;
                };
                if generation != self.generation {
                    return;
                }
                self.reviewing = false;
                self.activity = None;
                match result {
                    Ok((review, session)) => {
                        self.session = Some(session);
                        self.show_review(*review);
                        self.go(Step::Review, cx);
                    }
                    Err(error) => {
                        self.note(&error, cx);
                        self.schedule_warm(cx);
                    }
                }
            }
            Msg::Session(generation, update) => {
                if generation == self.generation {
                    self.session_update(update, cx);
                }
            }
            Msg::Erase(text) => self.erase = text,
            Msg::Consent(on) => self.consent = on,
            Msg::Tick => {
                if let Some(started) = self.started
                    && self.installing
                {
                    self.elapsed = started.elapsed().as_secs();
                }
            }
            Msg::ToggleLog => self.log_open = !self.log_open,
            Msg::CopyFailure => {
                if let Some(text) = self.failure.as_ref().or(self.preparation.failure.as_ref()) {
                    cx.set_clipboard_text(text);
                }
            }
            Msg::Power(action) => self.power(action, cx),
            Msg::PowerRequested(action, result) => {
                if let Err(error) = result {
                    self.powering = false;
                    self.note(&format!("Could not {action}: {error}"), cx);
                }
            }
            Msg::Close => self.close(cx),
            Msg::CloseRefused => self.note(
                "The installer cannot be closed while it is writing to the disk.",
                cx,
            ),
            Msg::Dismissed => {}
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        crate::view::view(self)
    }

    fn close_requested(&mut self, _window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if self.installing {
            // Through a message, so the window is drawn again with the note.
            cx.send(Msg::CloseRefused);
            return false;
        }
        self.stop_warm(cx);
        self.stop_listening();
        // Dropping an unconfirmed session withdraws its request.
        self.session.take();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usernames_are_suggested_only_when_valid() {
        assert_eq!(suggest_username("Ada Lovelace"), "ada");
        assert_eq!(suggest_username("  Grace  Hopper "), "grace");
        assert_eq!(suggest_username("Ünal"), "nal");
        assert_eq!(suggest_username("2pac"), "");
        assert_eq!(suggest_username("root"), "");
        assert_eq!(suggest_username(""), "");
    }
    #[test]
    fn progress_is_monotonic_and_copy_dominates() {
        let mut last = 0.0;
        for step in 1..=6 {
            for copy in [0.0, 0.5, 1.0] {
                let fraction = overall_fraction(step, if step == 4 { copy } else { 0.0 });
                assert!(fraction >= last);
                last = fraction;
            }
        }
        assert!((STEP_WEIGHTS.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(overall_fraction(4, 1.0) - overall_fraction(4, 0.0) > 0.5);
    }
    #[test]
    fn aizome_and_tatami_start_selected_with_aizome_the_default() {
        let app = Genkan::with_zones(None, String::new(), None);
        assert_eq!(app.desktops, [Desktop::Aizome, Desktop::Tatami]);
        assert_eq!(app.default_choice, Some(Desktop::Aizome));
    }
    #[test]
    fn steps_have_unique_names() {
        let names: BTreeSet<_> = Step::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names.len(), Step::ALL.len());
        assert_eq!(LOCALE_NAMES.len(), LOCALES.len());
        assert_eq!(KEYBOARD_NAMES.len(), KEYBOARDS.len());
        assert_eq!(SUPER_KEY_NAMES.len(), SuperKey::ALL.len());
    }
    #[test]
    fn the_keyboard_step_writes_what_settei_would() {
        // The backend can't use Aizome's crates (the helper is built from it
        // alone), so it names the arrangements itself.
        use aizome_ipc::setup::{SWAP_ALT_SUPER, SWAP_CONTROL_SUPER, beside_space};
        assert_eq!(SuperKey::SwapAlt.xkb_options(), SWAP_ALT_SUPER);
        assert_eq!(SuperKey::SwapControl.xkb_options(), SWAP_CONTROL_SUPER);
        let suggests = |code| {
            beside_space(code)
                .unwrap()
                .suggest
                .and_then(SuperKey::from_xkb_options)
        };
        assert_eq!(suggests(125), Some(SuperKey::AsIs));
        assert_eq!(suggests(56), Some(SuperKey::SwapAlt));
        assert_eq!(suggests(29), Some(SuperKey::SwapControl));
        assert_eq!(suggests(94), None);
    }
}
