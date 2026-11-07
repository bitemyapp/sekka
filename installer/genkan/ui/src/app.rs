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
use crate::flow::{
    Erasing, Flow, FlowAny, FlowState, HelperAnswer, ReviewAnswer, ReviewRequest, Said,
};
use crate::ram::{Memory, MemoryAnswer, SwapSize};
use crate::{format::size, media, zone};
use aizome_ipc::{KeyboardInfo, setup::Beside};
use aizome_kit::agree::{Agreed, Pressed};
use aizome_kit::cx::MsgSender;
use aizome_kit::prelude::*;
use aizome_typestate::{Answer, Fresh, Generation, Ticket};
use calamares_nixos::{
    Desktop, Filesystem, Firmware, Hostname, InstallPlan, KEYBOARDS, LOCALES, RawRequest, Settings,
    SuperKey, Username, applications,
    disk::{self, Disk, Layout},
    github::{self, GitIdentity, GithubUser},
    install::{Event, Summary},
    memory::{self, MemInfo},
    precache,
    session::{Ready, Session, Update},
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

/// Whether the key beside the space bar is Alt, which swapping left Alt and Super
/// makes ❖ (the arrangement Settei suggests for it).
pub fn beside_alt(beside: &Beside) -> bool {
    beside.suggest == Some(aizome_ipc::setup::SWAP_LEFT_ALT_SUPER)
}

/// Whether ❖ is beside the space bar already (a Mac's ⌘).
pub fn beside_super(beside: &Beside) -> bool {
    beside.suggest == Some("")
}

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
    /// It prepared the installation: the proof the confirmation needs.
    Prepared(Ready),
    /// The helper exited: `Ok` when its exit status was success.
    Finished(Result<(), String>),
}
impl From<Update> for Helper {
    fn from(update: Update) -> Self {
        match update {
            Update::Event(event) => Self::Event(event),
            Update::Prepared(ready) => Self::Prepared(ready),
            Update::Finished(result) => Self::Finished(result),
        }
    }
}

/// How a review is confirmed: by the person's press of the Review page's
/// button, or, for a preview's sample with no helper behind it, without one.
enum How {
    Agreed(Agreed<Erasing>),
    Sample,
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
    Scanned(ScanAnswer),
    Memory(MemoryAnswer),
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
    /// Swap left Alt and Super, or keep the keys as they are.
    SwapAltSuper(bool),
    // Page cache warming.
    WarmStart(u64),
    Warm {
        generation: u64,
        read: u64,
        total: u64,
        done: bool,
    },
    // Review and installation.
    Reviewed(Handoff<ReviewAnswer>),
    Session(HelperAnswer),
    Erase(String),
    /// The Review page's Erase Disk and Install button: the person agreed to
    /// the review it showed, with the phrase typed.
    Install(Pressed<Erasing>),
    /// Return in the phrase field: on to the Erase Disk and Install button,
    /// which only a press of its own confirms.
    ToConfirm,
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

    /// What the person types to confirm erasing its disk.
    pub fn phrase(&self) -> String {
        format!("ERASE {}", self.disk.path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepState {
    Working,
    Done,
    Failed,
}

/// The Review page's line about preparation in the background. Whether
/// it's working, done or failed is the review's state ([`Genkan::prep_state`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Preparation {
    pub title: String,
    pub subtitle: String,
    /// Reading the prepared system into memory, as a fraction.
    pub progress: Option<f64>,
    pub failure: Option<String>,
}

/// Why swap can't be had when installed memory couldn't be read.
fn swap_unsized(error: &str) -> String {
    format!(
        "Installed memory could not be read ({error}), so a swap partition can't be sized. Turn swap off to continue."
    )
}

/// The stage a transition reached, or the one it refused to leave.
fn either<'s>(reached: Result<impl Into<FlowAny<'s>>, impl Into<FlowAny<'s>>>) -> FlowAny<'s> {
    match reached {
        Ok(stage) => stage.into(),
        Err(stage) => stage.into(),
    }
}

/// How the helper stopped.
enum Stop {
    Cancelled,
    Failed(String),
    Exited(Result<(), String>),
}

/// Names a disk scan: its answer carries the ticket.
pub enum DiskScan {}

/// A disk scan's answer: the disks and the firmware, or why there are none.
#[derive(Clone, Debug)]
pub struct ScanAnswer {
    ticket: Ticket<DiskScan>,
    result: Result<(Vec<Disk>, Firmware), String>,
}

impl Answer<DiskScan> for ScanAnswer {
    fn ticket(&self) -> Ticket<DiskScan> {
        self.ticket
    }
}

const UNPREPARED: &str =
    "No disk was written. Go back to change your choices, or copy the details below.";

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
    /// Installed memory, which sizes the swap partition.
    memory: Memory,
    scans: Generation<DiskScan>,
    /// The scan under way.
    scan: Option<Ticket<DiskScan>>,
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

    // Review and installation: where they are (`flow`), the latest review's
    // ticket, and what the pages show of them.
    flow: Flow,
    review_ticket: Option<Ticket<ReviewRequest>>,
    pub phrase: String,
    pub erase: String,
    pub consent: bool,
    pub preparation: Preparation,
    started: Option<Instant>,
    pub elapsed: u64,
    pub copy_fraction: f64,
    pub copied: Option<String>,
    pub install_message: String,
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
            memory: Memory::new(Generation::new()),
            scans: Generation::new(),
            scan: None,
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
            flow: Flow::new(Generation::new()),
            review_ticket: None,
            phrase: String::new(),
            erase: String::new(),
            consent: false,
            preparation: Preparation {
                title: "Waiting for authorization…".into(),
                subtitle: String::new(),
                progress: None,
                failure: None,
            },
            started: None,
            elapsed: 0,
            copy_fraction: 0.0,
            copied: None,
            install_message: "Starting…".into(),
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
        self.reviewing() || self.installing() || self.step >= Step::Review
    }

    // ---- Where the review and the installation are --------------------------

    /// The review worker is checking the settings and starting the helper.
    pub fn reviewing(&self) -> bool {
        matches!(self.flow.stored(), FlowState::Checking(()))
    }

    /// The person confirmed and the helper hasn't finished: the window stays
    /// open.
    pub fn installing(&self) -> bool {
        match self.flow.stored() {
            FlowState::Queued(_) | FlowState::Installing(_) => true,
            FlowState::Failed(failure) => failure.helper_running(),
            _ => false,
        }
    }

    pub fn completed(&self) -> bool {
        matches!(self.flow.stored(), FlowState::Installed(()))
    }

    pub fn failed(&self) -> bool {
        matches!(self.flow.stored(), FlowState::Failed(_))
    }

    /// The helper has prepared, and waits for the confirmation.
    pub fn prepared(&self) -> bool {
        matches!(self.flow.stored(), FlowState::Ready(_))
    }

    /// The plan on the Review page.
    pub fn review(&self) -> Option<&Review> {
        match self.flow.stored() {
            FlowState::Preparing(pending) | FlowState::Queued(pending) => Some(pending.review()),
            FlowState::Ready(primed) => Some(primed.review()),
            FlowState::Unprepared(review) => Some(review.as_ref()),
            _ => None,
        }
    }

    pub fn prep_state(&self) -> PrepState {
        match self.flow.stored() {
            FlowState::Ready(_) => PrepState::Done,
            FlowState::Unprepared(_) => PrepState::Failed,
            _ => PrepState::Working,
        }
    }

    /// The last step the helper reported: 0 before the first, 7 once done.
    pub fn install_step(&self) -> u8 {
        match self.flow.stored() {
            FlowState::Installing(running) => running.step().unwrap_or(0),
            FlowState::Failed(failure) => failure.step().unwrap_or(0),
            FlowState::Installed(()) => 7,
            _ => 0,
        }
    }

    /// Whether the installation reached the disk: the helper reported a
    /// step, and the first starts by erasing it.
    pub fn disk_written(&self) -> bool {
        self.install_step() > 0
    }

    /// Each step's state on the Install page: the steps before the last
    /// reported are done, and that one is going on or failed.
    pub fn steps(&self) -> [StepState; 6] {
        let step = self.install_step();
        let failed = self.failed();
        std::array::from_fn(|index| match (index as u8 + 1).cmp(&step) {
            std::cmp::Ordering::Less => StepState::Done,
            std::cmp::Ordering::Equal if failed => StepState::Failed,
            std::cmp::Ordering::Equal => StepState::Current,
            std::cmp::Ordering::Greater => StepState::Pending,
        })
    }

    /// The latest review's ticket, for previews and tests, which stand in
    /// for the helper's events.
    pub fn review_ticket(&self) -> Option<Ticket<ReviewRequest>> {
        self.review_ticket
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
        self.completed() || (self.failed() && !self.installing())
    }

    /// Next's label, and whether it's the destructive one.
    pub fn next_label(&self) -> &'static str {
        match self.step {
            Step::Welcome => "Get Started",
            Step::Keyboard if self.reviewing() => "Checking…",
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
            _ => !self.reviewing(),
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
            // The Review page's button agrees (`Msg::Install`): only a
            // person's press of it confirms, never Next sent for them.
            Step::Review => {
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
        if self.reviewing() {
            return;
        }
        match self.step {
            Step::Welcome | Step::Install => {}
            Step::Review => {
                // Withdraw the unconfirmed request; nothing was written.
                self.withdraw();
                self.go(Step::Keyboard, cx);
                self.schedule_warm(cx);
            }
            step => self.go(Step::ALL[step.index() - 1], cx),
        }
    }

    /// Leaves the review, dropping its helper, which withdraws the
    /// unconfirmed request. Nothing once the person has confirmed.
    fn withdraw(&mut self) {
        self.flow.with_typed(|state| {
            let state = match state {
                FlowAny::Preparing(preparing) => preparing.withdraw().into(),
                FlowAny::Ready(ready) => ready.withdraw().into(),
                FlowAny::Unprepared(unprepared) => unprepared.withdraw().into(),
                other => other,
            };
            (state, ())
        });
        if matches!(self.flow.stored(), FlowState::Editing(())) {
            self.review_ticket = None;
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
        if self.scanning() {
            return;
        }
        let ticket = self.scans.issue();
        self.scan = Some(ticket);
        cx.spawn(move || {
            Msg::Scanned(ScanAnswer {
                ticket,
                result: disk::discover()
                    .map(|d| (d, Firmware::current()))
                    .map_err(|e| format!("{e:#}")),
            })
        });
    }

    /// A disk scan is under way.
    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// The scan answered, bound to the proof that it's the latest scan's:
    /// its disks, or why there are none.
    fn scanned(&mut self, answer: Fresh<DiskScan, ScanAnswer>, cx: &mut Cx<Msg>) {
        self.scan = None;
        match answer.into_answer().result {
            Ok((found, firmware)) => {
                self.selected_disk = None;
                self.firmware = firmware;
                self.disks = found;
            }
            Err(error) => self.note(&format!("Could not list disks: {error}"), cx),
        }
    }

    /// A preview's sample disks, as if a scan had found them.
    pub(crate) fn sample_disks(&mut self, found: Vec<Disk>, firmware: Firmware, cx: &mut Cx<Msg>) {
        let ticket = self.scans.issue();
        self.scan = Some(ticket);
        let result = Ok((found, firmware));
        self.update(Msg::Scanned(ScanAnswer { ticket, result }), cx);
    }

    fn read_memory(&mut self, cx: &mut Cx<Msg>) {
        if let Some(ticket) = self.memory.ask() {
            cx.spawn(move || {
                let result = memory::read().map_err(|e| format!("{e:#}"));
                Msg::Memory(MemoryAnswer::new(ticket, result))
            });
        }
    }

    /// A preview's sample in place of the read.
    pub(crate) fn sample_memory(&mut self, result: Result<MemInfo, String>, cx: &mut Cx<Msg>) {
        if let Some(ticket) = self.memory.ask() {
            self.update(Msg::Memory(MemoryAnswer::new(ticket, result)), cx);
        }
    }

    /// The read answered. A failure turns swap off: nothing can size it.
    fn memory_read(&mut self, answer: MemoryAnswer, cx: &mut Cx<Msg>) {
        self.memory.answer(answer);
        if let SwapSize::Unreadable(error) = self.memory.swap_size(true) {
            self.swap = false;
            self.note(&format!("Could not read installed memory: {error}"), cx);
        }
    }

    /// Installed memory, which sizes the swap partition.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Swap on or off. On needs installed memory, which sizes it: refused
    /// when it couldn't be read.
    fn set_swap(&mut self, on: bool, cx: &mut Cx<Msg>) {
        match self.memory.swap_size(on) {
            SwapSize::Unreadable(error) => self.note(&swap_unsized(&error), cx),
            SwapSize::Off | SwapSize::Known(_) | SwapSize::Reading => self.swap = on,
        }
    }

    pub fn swap_bytes(&self) -> Option<u64> {
        match self.memory.swap_size(self.swap) {
            SwapSize::Known(bytes) => Some(bytes),
            SwapSize::Off | SwapSize::Reading | SwapSize::Unreadable(_) => None,
        }
    }

    pub fn selected(&self) -> Option<&Disk> {
        self.selected_disk.and_then(|i| self.disks.get(i))
    }

    /// The new partitions for the chosen disk, or why there are none yet.
    pub fn layout(&self) -> Result<Layout, String> {
        let Some(disk) = self.selected() else {
            return Err("Select a disk to preview its new partitions.".into());
        };
        let swap = match self.memory.swap_size(self.swap) {
            SwapSize::Off => None,
            SwapSize::Known(bytes) => Some(bytes),
            SwapSize::Reading => return Err("Reading installed memory…".into()),
            SwapSize::Unreadable(error) => return Err(swap_unsized(&error)),
        };
        Layout::for_install(disk.identity.bytes, self.firmware, swap, self.encrypt)
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
        match self.memory.swap_size(self.swap) {
            SwapSize::Off | SwapSize::Known(_) => {}
            SwapSize::Reading => {
                return Err("Still reading installed memory; try again in a moment.".into());
            }
            SwapSize::Unreadable(error) => return Err(swap_unsized(&error)),
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

    /// The key beside the space bar came. Alt there (most PC keyboards):
    /// Genkan suggests swapping left Alt and Super, to put ❖ there. ❖ there
    /// already, or any other key: the keys stay as they are; Genkan swaps
    /// nothing else. The space bar, or a key right of it, asks again.
    pub fn caught(&mut self, code: u32, keyboard: Option<KeyboardInfo>) {
        match aizome_ipc::setup::beside_space(code) {
            Err(again) => {
                self.key_error = Some(again.into());
                self.listen_for_key();
            }
            Ok(beside) => {
                self.super_key = if beside_alt(&beside) {
                    SuperKey::SwapAlt
                } else {
                    SuperKey::AsIs
                };
                self.key_error = None;
                self.key_check = KeyCheck::Caught { keyboard, beside };
            }
        }
    }

    /// What the review says of the key beside the space bar.
    pub fn super_key_name(key: SuperKey) -> &'static str {
        match key {
            SuperKey::AsIs => "As they are",
            SuperKey::SwapAlt => "Swap left Alt and Super",
        }
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
        if self.preview.is_some() || self.step >= Step::Review || self.reviewing() {
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
        if !matches!(self.flow.stored(), FlowState::Editing(())) {
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
        let ticket = self.flow.with_typed(|state| match state {
            FlowAny::Editing(editing) => {
                let (checking, ticket) = editing.review();
                (checking.into(), ticket)
            }
            // Only from the pages, as the check above says.
            _ => unreachable!(),
        });
        self.review_ticket = Some(ticket);
        self.activity = Some(Activity {
            text: "Checking your settings…".into(),
            busy: true,
            warning: false,
        });
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<(Box<Review>, Session)> {
                let plan = request.parse(&Settings::load()?)?;
                disk::Reviewed::of(&plan).revalidate()?;
                let plan = plan.snapshot_wifi()?;
                let review = Box::new(Review::of(&plan));
                let events = send.clone();
                let session = Session::start(plan, move |update| {
                    events.send(Msg::Session(HelperAnswer::new(ticket, update.into())));
                })?;
                Ok((review, session))
            })()
            .map_err(|e| format!("{e:#}"));
            send.send(Msg::Reviewed(Handoff::new(ReviewAnswer::new(
                ticket, result,
            ))));
        });
    }

    /// The review worker answered: the Review page, while the helper
    /// prepares, or back to the pages with what failed.
    fn reviewed(&mut self, answer: Fresh<ReviewRequest, ReviewAnswer>, cx: &mut Cx<Msg>) {
        self.activity = None;
        let (moved, refused) = self.moved(|state| match state {
            FlowAny::Checking(checking) => match checking.answered(answer) {
                Ok(preparing) => (preparing.into(), None),
                Err((editing, error)) => (editing.into(), Some(error)),
            },
            // A current answer comes only while checking; its helper is
            // dropped with it otherwise.
            FlowAny::Editing(s) => (s.into(), None),
            FlowAny::Preparing(s) => (s.into(), None),
            FlowAny::Ready(s) => (s.into(), None),
            FlowAny::Unprepared(s) => (s.into(), None),
            FlowAny::Queued(s) => (s.into(), None),
            FlowAny::Installing(s) => (s.into(), None),
            FlowAny::Installed(s) => (s.into(), None),
            FlowAny::Failed(s) => (s.into(), None),
        });
        if !moved {
            return;
        }
        match self.flow.stored() {
            FlowState::Preparing(pending) => {
                let review = pending.review().clone();
                self.show_review(&review);
                self.go(Step::Review, cx);
            }
            FlowState::Editing(()) => {
                self.review_ticket = None;
                self.note(&refused.unwrap_or_default(), cx);
                self.schedule_warm(cx);
            }
            _ => {}
        }
    }

    /// Runs `transition` on the review: whether it moved to another stage,
    /// and what the transition said. The pages follow the stage reached.
    fn moved<R>(
        &mut self,
        transition: impl for<'s> FnOnce(FlowAny<'s>) -> (FlowAny<'s>, R),
    ) -> (bool, R) {
        let before = std::mem::discriminant(self.flow.stored());
        let said = self.flow.with_typed(transition);
        (std::mem::discriminant(self.flow.stored()) != before, said)
    }

    /// A preview's sample review, with no helper behind it.
    pub(crate) fn show_sample_review(&mut self, review: Review) {
        self.show_review(&review);
        let ticket = self.flow.with_typed(|state| match state {
            FlowAny::Editing(editing) => {
                let (preparing, ticket) = editing.sample(Box::new(review));
                (preparing.into(), Some(ticket))
            }
            other => (other, None),
        });
        if ticket.is_some() {
            self.review_ticket = ticket;
        }
    }

    /// The Review page's fields, for a new review.
    fn show_review(&mut self, review: &Review) {
        self.phrase = review.phrase();
        self.erase.clear();
        self.consent = false;
        self.preparation = Preparation {
            title: "Waiting for authorization…".into(),
            subtitle: "The installation helper repeats every safety check before any disk write."
                .into(),
            progress: None,
            failure: None,
        };
    }

    pub fn confirmation_ready(&self) -> bool {
        self.consent
            && matches!(
                self.flow.stored(),
                FlowState::Preparing(_) | FlowState::Ready(_)
            )
            && !self.phrase.is_empty()
            && self.erase == self.phrase
    }

    /// What the Review page's Erase Disk and Install button agrees to: the
    /// review it shows, with the phrase as typed. None without a review.
    pub fn erasing(&self) -> Option<Erasing> {
        self.review_ticket
            .map(|review| Erasing::new(review, self.erase.as_str()))
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

    /// Preparation finished: the Review page says so.
    fn show_prepared(&mut self, summary: &Summary) {
        let (title, subtitle) = Self::prepared_text(summary);
        self.preparation = Preparation {
            title,
            subtitle: subtitle.clone(),
            progress: None,
            failure: None,
        };
        self.append_log(&subtitle);
    }

    /// Preparation failed or was cancelled, or the helper stopped: the
    /// Review page says so, and that nothing was written. A confirmation
    /// queued meanwhile goes back to it.
    fn show_unprepared(
        &mut self,
        title: &str,
        subtitle: &str,
        failure: Option<String>,
        cx: &mut Cx<Msg>,
    ) {
        self.preparation = Preparation {
            title: title.into(),
            subtitle: subtitle.into(),
            progress: None,
            failure: failure.map(|message| message.chars().take(8000).collect()),
        };
        if self.step == Step::Install {
            self.stop_tick(cx);
            self.go(Step::Review, cx);
        }
    }

    // ---- Installation -----------------------------------------------------

    /// A preview's sample review, confirmed as if pressed: it has no helper
    /// to send the confirmation to, and the flow refuses a running helper's.
    pub(crate) fn confirm_sample(&mut self, cx: &mut Cx<Msg>) {
        self.confirm_install(How::Sample, cx);
    }

    /// The person agreed to the review on the page (or a preview confirms
    /// its sample): confirm it, sending it to the helper once prepared.
    fn confirm_install(&mut self, how: How, cx: &mut Cx<Msg>) {
        if !self.confirmation_ready() {
            self.note(
                "Type the exact phrase and confirm that the disk will be erased.",
                cx,
            );
            return;
        }
        // The flow checks this too, and refuses; here it's to say why.
        if let How::Agreed(agreed) = &how
            && Some(agreed.target()) != self.erasing().as_ref()
        {
            self.note(
                "The review changed as you confirmed. Check it and confirm again.",
                cx,
            );
            return;
        }
        let (moved, ()) = self.moved(|state| {
            let state = match state {
                FlowAny::Ready(ready) => match how {
                    How::Agreed(agreed) => ready.confirm(agreed),
                    How::Sample => ready.confirm_sample(),
                }
                .map_or_else(Into::into, Into::into),
                FlowAny::Preparing(preparing) => match how {
                    How::Agreed(agreed) => preparing.confirm(agreed),
                    How::Sample => preparing.confirm_sample(),
                }
                .map_or_else(Into::into, Into::into),
                FlowAny::Editing(s) => s.into(),
                FlowAny::Checking(s) => s.into(),
                FlowAny::Unprepared(s) => s.into(),
                FlowAny::Queued(s) => s.into(),
                FlowAny::Installing(s) => s.into(),
                FlowAny::Installed(s) => s.into(),
                FlowAny::Failed(s) => s.into(),
            };
            (state, ())
        });
        match self.flow.stored() {
            FlowState::Installing(_) if moved => {
                self.forget_secrets();
                self.begin_install("Starting the installation…", cx);
            }
            FlowState::Queued(_) if moved => self.begin_install(
                "Finishing preparation; the installation starts as soon as it is ready.",
                cx,
            ),
            _ => self.note(
                "The installation helper is no longer running. Go back and review again.",
                cx,
            ),
        }
    }

    /// The Install page, from the moment the person confirms.
    fn begin_install(&mut self, message: &str, cx: &mut Cx<Msg>) {
        self.started = Some(Instant::now());
        self.elapsed = 0;
        self.copy_fraction = 0.0;
        self.copied = None;
        self.failure = None;
        self.install_message = message.into();
        // A preview's still picture keeps its sample time.
        if self.preview.is_none() {
            self.tick = Some(cx.every(Duration::from_millis(500), Msg::Tick));
        }
        self.activity = None;
        self.go(Step::Install, cx);
    }

    /// The confirmation reached the helper, which has every secret it
    /// needs: the window forgets them.
    fn forget_secrets(&mut self) {
        self.password = Zeroizing::default();
        self.repeat = Zeroizing::default();
        self.passphrase = Zeroizing::default();
        self.passphrase_repeat = Zeroizing::default();
    }

    pub fn fraction(&self) -> f64 {
        if self.completed() {
            1.0
        } else if self.install_step() == 0 {
            0.0
        } else {
            overall_fraction(self.install_step(), self.copy_fraction)
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

    /// The installation failed: the Install page says how far it got.
    fn show_install_failure(&mut self, message: &str, cx: &mut Cx<Msg>) {
        self.stop_tick(cx);
        self.failure = Some(message.chars().take(8000).collect());
        self.install_message = if self.disk_written() {
            "Installation failed. See the details below."
        } else {
            "The installation stopped before writing to the disk. See the details below."
        }
        .into();
    }

    /// The installation's length as the done page gives it.
    pub fn installed_in(&self) -> Option<f64> {
        self.total_seconds
            .or_else(|| (self.elapsed > 0).then_some(self.elapsed as f64))
    }

    /// What the helper said, as if it had (previews and tests).
    pub fn helper_event(&mut self, event: Event, cx: &mut Cx<Msg>) {
        if let Some(ticket) = self.review_ticket {
            self.update(
                Msg::Session(HelperAnswer::new(ticket, Helper::Event(event))),
                cx,
            );
        }
    }

    /// What the helper said, proven to be about the latest review: the
    /// transitions read their payload from the answer itself.
    fn session_update(&mut self, said: Said, cx: &mut Cx<Msg>) {
        match crate::flow::heard(&said).clone() {
            // What the helper is doing, the most frequent: the pages say it,
            // and the review stays where it is.
            Helper::Event(Event::Log { line }) => {
                if matches!(self.flow.stored(), FlowState::Preparing(_)) {
                    self.preparation.subtitle = line.clone();
                }
                self.append_log(&line);
            }
            Helper::Event(Event::Copying { bytes, total }) => {
                if total > 0 {
                    self.copy_fraction = bytes as f64 / total as f64;
                    self.copied = Some(format!("{} of {}", size(bytes.min(total)), size(total)));
                }
            }
            Helper::Event(Event::Caching { read, total }) => {
                if matches!(self.flow.stored(), FlowState::Preparing(_)) {
                    self.preparation.progress = (total > 0).then(|| read as f64 / total as f64);
                    self.preparation.subtitle =
                        format!("{} of {} read into memory", size(read), size(total));
                }
            }
            Helper::Event(Event::Timing { stage, seconds }) => {
                if stage == "install" {
                    self.total_seconds = Some(seconds);
                }
                self.append_log(&format!("{stage}: {seconds:.1} s"));
            }
            Helper::Event(Event::Preparing { message }) => {
                match self.flow.stored() {
                    FlowState::Queued(_) => {
                        self.install_message = format!("Finishing preparation: {message}");
                    }
                    FlowState::Preparing(_) => {
                        self.preparation.title = message.clone();
                        self.preparation.subtitle.clear();
                    }
                    _ => {}
                }
                self.append_log(&message);
            }
            Helper::Prepared(ready) => {
                let summary = ready.summary().clone();
                let (moved, ()) = self.moved(|state| {
                    let state = match state {
                        FlowAny::Preparing(preparing) => match preparing.prepared(&said) {
                            Ok(ready) => ready.into(),
                            Err(preparing) => preparing.into(),
                        },
                        FlowAny::Queued(queued) => queued.prepared(&said).into(),
                        // Ahead of the worker's answer: review again.
                        FlowAny::Checking(checking) => either(checking.stopped(&said)),
                        // The review moved on: late.
                        FlowAny::Editing(s) => s.into(),
                        FlowAny::Ready(s) => s.into(),
                        FlowAny::Unprepared(s) => s.into(),
                        FlowAny::Installing(s) => s.into(),
                        FlowAny::Installed(s) => s.into(),
                        FlowAny::Failed(s) => s.into(),
                    };
                    (state, ())
                });
                self.prepped(moved, &summary, cx);
            }
            // A preview's sample: a running helper's comes with its proof.
            Helper::Event(Event::Prepared { summary }) => {
                let (moved, ()) = self.moved(|state| {
                    let state = match state {
                        FlowAny::Preparing(preparing) => match preparing.sampled(&said) {
                            Ok(ready) => ready.into(),
                            Err(preparing) => preparing.into(),
                        },
                        FlowAny::Queued(queued) => match queued.sampled(&said) {
                            Ok(installing) => installing.into(),
                            Err(queued) => queued.into(),
                        },
                        FlowAny::Editing(s) => s.into(),
                        FlowAny::Checking(s) => s.into(),
                        FlowAny::Ready(s) => s.into(),
                        FlowAny::Unprepared(s) => s.into(),
                        FlowAny::Installing(s) => s.into(),
                        FlowAny::Installed(s) => s.into(),
                        FlowAny::Failed(s) => s.into(),
                    };
                    (state, ())
                });
                self.prepped(moved, &summary, cx);
            }
            Helper::Event(Event::Progress { message, .. }) => {
                self.moved(|state| {
                    let state = match state {
                        FlowAny::Installing(installing) => installing.progress(&said).into(),
                        FlowAny::Editing(s) => s.into(),
                        FlowAny::Checking(s) => s.into(),
                        FlowAny::Preparing(s) => s.into(),
                        FlowAny::Ready(s) => s.into(),
                        FlowAny::Unprepared(s) => s.into(),
                        FlowAny::Queued(s) => s.into(),
                        FlowAny::Installed(s) => s.into(),
                        FlowAny::Failed(s) => s.into(),
                    };
                    (state, ())
                });
                self.append_log(&message);
                self.install_message = message;
            }
            Helper::Event(Event::Complete) => {
                let (moved, ()) = self.moved(|state| {
                    let state = match state {
                        FlowAny::Installing(installing) => either(installing.complete(&said)),
                        FlowAny::Editing(s) => s.into(),
                        FlowAny::Checking(s) => s.into(),
                        FlowAny::Preparing(s) => s.into(),
                        FlowAny::Ready(s) => s.into(),
                        FlowAny::Unprepared(s) => s.into(),
                        FlowAny::Queued(s) => s.into(),
                        FlowAny::Installed(s) => s.into(),
                        FlowAny::Failed(s) => s.into(),
                    };
                    (state, ())
                });
                if moved && self.completed() {
                    self.stop_tick(cx);
                }
            }
            Helper::Event(Event::Cancelled) => self.stopped(&said, Stop::Cancelled, cx),
            Helper::Event(Event::Failed { message }) => {
                self.stopped(&said, Stop::Failed(message), cx);
            }
            Helper::Finished(result) => self.stopped(&said, Stop::Exited(result), cx),
        }
    }

    /// The pages, after the helper prepared: what the stage reached says.
    fn prepped(&mut self, moved: bool, summary: &Summary, cx: &mut Cx<Msg>) {
        if !moved {
            return;
        }
        match self.flow.stored() {
            FlowState::Ready(_) => self.show_prepared(summary),
            FlowState::Installing(_) => {
                self.show_prepared(summary);
                self.forget_secrets();
                self.install_message = "Preparation complete. Starting the installation…".into();
            }
            // The queued confirmation had no helper left to go to.
            FlowState::Unprepared(_) => self.show_unprepared(
                "The installation helper stopped",
                "No disk was written. Go back and review again to continue.",
                None,
                cx,
            ),
            FlowState::Editing(()) => self.helper_left_early(None, cx),
            FlowState::Checking(())
            | FlowState::Preparing(_)
            | FlowState::Queued(_)
            | FlowState::Installed(())
            | FlowState::Failed(_) => {}
        }
    }

    /// The helper cancelled, failed or exited.
    fn stopped(&mut self, said: &Said, why: Stop, cx: &mut Cx<Msg>) {
        let exited = matches!(why, Stop::Exited(_));
        let cancelled = matches!(why, Stop::Cancelled);
        let (moved, ()) = self.moved(|state| {
            let state = match state {
                // The helper stopped before the review opened.
                FlowAny::Checking(checking) => either(checking.stopped(said)),
                FlowAny::Preparing(preparing) => either(preparing.stopped(said)),
                FlowAny::Ready(ready) => either(ready.stopped(said)),
                // Before the confirmation was sent: nothing was written (GEN-34).
                FlowAny::Queued(queued) => either(queued.stopped(said)),
                // The helper never cancels once it's confirmed.
                FlowAny::Installing(installing) if cancelled => installing.into(),
                FlowAny::Installing(installing) if exited => either(installing.exited(said)),
                FlowAny::Installing(installing) => either(installing.failed(said)),
                FlowAny::Failed(failed) if exited => either(failed.exited(said)),
                FlowAny::Failed(s) => s.into(),
                FlowAny::Editing(s) => s.into(),
                FlowAny::Unprepared(s) => s.into(),
                FlowAny::Installed(s) => s.into(),
            };
            (state, ())
        });
        if !moved {
            return;
        }
        let message = match why {
            Stop::Cancelled => None,
            Stop::Failed(message) => Some(message),
            Stop::Exited(result) => result.err(),
        };
        match self.flow.stored() {
            FlowState::Editing(()) => self.helper_left_early(message, cx),
            FlowState::Unprepared(_) if cancelled => self.show_unprepared(
                "Preparation cancelled",
                "Nothing was written. Go back and review again to continue.",
                None,
                cx,
            ),
            FlowState::Unprepared(_) => self.show_unprepared(
                "Preparation failed",
                UNPREPARED,
                Some(message.unwrap_or_else(|| {
                    "The installation helper stopped before the installation was confirmed.".into()
                })),
                cx,
            ),
            FlowState::Failed(_) => self.show_install_failure(
                &message.unwrap_or_else(|| {
                    "The installation helper exited without confirming completion.".into()
                }),
                cx,
            ),
            FlowState::Checking(())
            | FlowState::Preparing(_)
            | FlowState::Ready(_)
            | FlowState::Queued(_)
            | FlowState::Installing(_)
            | FlowState::Installed(()) => {}
        }
    }

    /// The helper stopped, or got ahead of the review worker, before the
    /// review opened: back on the pages, with why.
    fn helper_left_early(&mut self, message: Option<String>, cx: &mut Cx<Msg>) {
        self.review_ticket = None;
        self.activity = None;
        let why = message.unwrap_or_else(|| "it got ahead of the review".into());
        self.note(
            &format!("The installation helper stopped before the review opened ({why}). Review again to continue."),
            cx,
        );
        self.schedule_warm(cx);
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
            Msg::Install(pressed) => {
                if let Some(agreed) = pressed.take() {
                    self.confirm_install(How::Agreed(agreed), cx);
                }
            }
            Msg::Back => self.backward(cx),
            Msg::Jump(step) => self.jump(step, cx),
            Msg::ClearNote(note) => {
                if note == self.note && self.activity.as_ref().is_some_and(|a| a.warning) {
                    self.activity = None;
                }
            }

            Msg::Rescan => self.rescan(cx),
            Msg::Scanned(answer) => {
                if let Ok(fresh) = self.scans.check_answer(answer) {
                    self.scanned(fresh, cx);
                }
            }
            Msg::Memory(answer) => self.memory_read(answer, cx),
            Msg::SelectDisk(index) => {
                if self.disks.get(index).is_some_and(|d| d.blocked.is_none()) {
                    self.selected_disk = Some(index);
                }
            }
            Msg::Filesystem(index) => {
                self.filesystem = Filesystem::ALL.get(index).copied().unwrap_or_default();
            }
            Msg::Swap(on) => self.set_swap(on, cx),
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
            Msg::SwapAltSuper(on) => {
                self.super_key = if on {
                    SuperKey::SwapAlt
                } else {
                    SuperKey::AsIs
                }
            }
            Msg::ZoneClock => {
                if self.step == Step::Location {
                    self.refresh_zone_clock();
                }
            }

            Msg::WarmStart(generation) => {
                if generation == self.warm_generation
                    && self.step < Step::Review
                    && !self.reviewing()
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

            Msg::Reviewed(answer) => {
                // Dropping a stale answer drops its session, which cancels it.
                if let Some(answer) = answer.take()
                    && let Ok(fresh) = self.flow.ctx().check_answer(answer)
                {
                    self.reviewed(fresh, cx);
                }
            }
            Msg::Session(answer) => {
                if let Ok(said) = self.flow.ctx().check_answer(answer) {
                    self.session_update(said, cx);
                }
            }
            Msg::Erase(text) => self.erase = text,
            Msg::ToConfirm => {
                if self.confirmation_ready() {
                    cx.focus("next");
                }
            }
            Msg::Consent(on) => self.consent = on,
            Msg::Tick => {
                if let Some(started) = self.started
                    && self.installing()
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
        if self.installing() {
            // Through a message, so the window is drawn again with the note.
            cx.send(Msg::CloseRefused);
            return false;
        }
        self.stop_warm(cx);
        self.stop_listening();
        // Dropping an unconfirmed session withdraws its request.
        self.withdraw();
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
    }
    #[test]
    fn the_keyboard_step_writes_what_settei_would() {
        // The backend can't use Aizome's crates (the helper is built from it
        // alone), so it names the arrangements itself.
        use aizome_ipc::setup::{SWAP_LEFT_ALT_SUPER, beside_space};
        assert_eq!(SuperKey::SwapAlt.xkb_options(), SWAP_LEFT_ALT_SUPER);
        // Alt is the one Genkan swaps; ❖ needs nothing; Control and the
        // rest stay as they are.
        let key = |code| beside_space(code).unwrap();
        assert!(beside_alt(&key(56)) && !beside_super(&key(56)));
        assert!(beside_super(&key(125)) && !beside_alt(&key(125)));
        for other in [29, 94, 15] {
            assert!(
                !beside_alt(&key(other)) && !beside_super(&key(other)),
                "{other}"
            );
        }
    }
}
