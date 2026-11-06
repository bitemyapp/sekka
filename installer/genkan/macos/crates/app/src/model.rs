// SPDX-License-Identifier: GPL-3.0-or-later
//! The app's state and every change to it, with no AppKit here: the window
//! draws a [`Model`], and everything the person does or a worker reports is
//! a [`Msg`] that [`Model::update`] turns into a new state and the
//! [`Effect`]s to run next. Effects run on worker threads, never on the main
//! thread.
use std::collections::{BTreeSet, VecDeque};

use sekka_mac_contract::{
    Acknowledgement, ArtifactRole, Begin, Candidate, Confirmation, Digest, DiskChanged,
    HELPER_PROTOCOL, HelperRefusal, HelperRefusalCode, InstallState, Inspection, InstallationKind,
    InstallationState, IssueCode, Place, Plan, PlanError, PlanRequest, Progress, Record, Release,
    Stage, StagedFile as WireStagedFile, Summary, Uuid, capacity, derive_plan, suggested_size,
};
use sekka_mac_trust::Verified;

use crate::env::{DownloadProgress, Events, HelperProblem, Registration, Secret, Staged};
use crate::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Inspecting this Mac and loading the catalog.
    Checking,
    /// This Mac isn't one Sekka supports.
    Unsupported,
    /// Something needs fixing first (charger, space, FileVault…).
    NotReady,
    /// Sekka (or something unfinished) is already on this disk.
    Existing,
    /// No release could be offered (offline, expired, none for this Mac).
    NoRelease,
    /// This Mac has a release, but this app can't install it.
    NeedsUpdate,
    /// Choose Sekka's size.
    Plan,
    Downloading,
    /// The exact plan, the backup confirmation, and Install.
    Review,
    /// The helper must be allowed in System Settings first.
    Approval,
    Installing,
    /// macOS's part is done; finish in Recovery.
    Recovery,
    /// An installation stopped; what changed and what's safe next.
    Stopped,
}

/// The sidebar's steps, `text::JOURNEY`'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Check,
    Size,
    Download,
    Install,
    Recovery,
    Account,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Pending,
    Good,
    Warning,
    Bad,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRow {
    pub label: &'static str,
    pub detail: String,
    pub state: CheckState,
}

/// Asking the helper to bring a stopped installation's record up to date
/// with the Mac ([`Model::decide`]). It is asked once in each check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reconciling {
    /// The helper hasn't answered yet.
    Asked(Uuid),
    /// It answered, or couldn't.
    Answered(Uuid),
}

/// The authorization sheet, while it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authorization {
    pub owner: String,
    /// The last refusal, shown under the password field.
    pub error: Option<String>,
    /// Waiting for the helper's answer.
    pub busy: bool,
    /// Authorizing a retry of the startup disk, not an installation.
    pub retry: bool,
}

#[derive(Debug)]
pub enum Msg {
    Inspected(Result<Inspection, String>),
    Engine(Result<Digest, String>),
    Catalog(Result<Verified, String>),
    HelperStatus(Result<Vec<Summary>, HelperProblem>),
    ToDownload(u64),
    DownloadProgress(DownloadProgress),
    Downloaded(Result<Vec<Staged>, String>),
    Registration(Result<Registration, String>),
    Begun(Result<Uuid, HelperRefusal>),
    Events(Result<Events, String>),
    CancelAnswered(Result<(), HelperRefusal>),
    Retried(Result<Events, HelperRefusal>),
    Reconciled(Result<Events, HelperRefusal>),
    Diagnostics(Vec<String>),

    CheckAgain,
    SetSize(u64),
    SetBackup(bool),
    SetAcknowledged(Acknowledgement, bool),
    StartDownload,
    StopDownload,
    Review,
    BackToPlan,
    Install,
    Authorize { owner: String, password: Secret },
    CancelAuthorization,
    OpenLoginItems,
    /// Once a second while waiting for the helper to be allowed.
    Tick,
    CancelInstallation,
    RetryStartupDisk,
    CopyDiagnostics,
    ShutDown,
    ToggleDetails,
    DismissNotice,
}

#[derive(Debug)]
pub enum Effect {
    Inspect,
    CheckEngine,
    LoadCatalog,
    HelperStatus,
    MeasureDownload(Box<Release>),
    Download(Box<Release>),
    StopDownload,
    CheckRegistration,
    RegisterHelper,
    OpenLoginItems,
    Begin(Box<Begin>, Secret),
    Poll { installation: Uuid, after: u64, delay_ms: u64 },
    Cancel(Uuid),
    RetryStartupDisk(Uuid, Secret),
    Reconcile(Uuid),
    Diagnostics(Uuid),
    CopyToPasteboard(String),
    ShutDown,
    RemoveStaging,
}

pub struct Model {
    pub screen: Screen,
    pub app_version: sekka_mac_contract::Version,
    pub user: (String, String),
    pub inspection: Option<Result<Inspection, String>>,
    /// The digest of the engine this app carries.
    pub engine: Option<Result<Digest, String>>,
    pub catalog: Option<Result<Verified, String>>,
    pub helper: Option<Result<Vec<Summary>, HelperProblem>>,
    /// The release offered to this Mac.
    pub release: Option<Release>,
    pub place: Option<Place>,
    /// Bytes for Sekka.
    pub size: u64,
    /// Bytes still to download before installing; none until measured for
    /// the release offered.
    pub to_download: Option<u64>,
    pub download: Option<DownloadProgress>,
    pub download_error: Option<String>,
    pub downloading: bool,
    pub staged: Vec<Staged>,
    pub plan: Option<Plan>,
    pub plan_error: Option<String>,
    /// This Mac is being looked at again, and the review follows.
    pub rechecking: bool,
    pub backup: bool,
    pub acknowledged: BTreeSet<Acknowledgement>,
    pub authorization: Option<Authorization>,
    pub registration: Option<Registration>,
    pub installation: Option<Uuid>,
    pub records: Vec<Record>,
    pub state: Option<InstallState>,
    pub progress: Option<Progress>,
    pub running: bool,
    pub details: bool,
    pub log: VecDeque<String>,
    pub notice: Option<String>,
    /// The cancel button was pressed and the helper hasn't answered.
    pub cancelling: bool,
    /// The installation the helper was asked to reconcile in this check.
    pub reconciling: Option<Reconciling>,
}

impl Model {
    pub fn new(app_version: sekka_mac_contract::Version, user: (String, String)) -> (Self, Vec<Effect>) {
        let model = Self {
            screen: Screen::Checking,
            app_version,
            user,
            inspection: None,
            engine: None,
            catalog: None,
            helper: None,
            release: None,
            place: None,
            size: 0,
            to_download: None,
            download: None,
            download_error: None,
            downloading: false,
            staged: Vec::new(),
            plan: None,
            plan_error: None,
            rechecking: false,
            backup: false,
            acknowledged: BTreeSet::new(),
            authorization: None,
            registration: None,
            installation: None,
            records: Vec::new(),
            state: None,
            progress: None,
            running: false,
            details: false,
            log: VecDeque::new(),
            notice: None,
            cancelling: false,
            reconciling: None,
        };
        (
            model,
            vec![Effect::Inspect, Effect::LoadCatalog, Effect::HelperStatus, Effect::CheckEngine],
        )
    }

    fn log(&mut self, line: impl Into<String>) {
        if self.log.len() == 500 {
            self.log.pop_front();
        }
        self.log.push_back(line.into());
    }

    /// Where the person is in the journey.
    pub fn step(&self) -> Step {
        match self.screen {
            // Recovery's step is done: all that's left is Sekka's own.
            Screen::Existing if self.installed() => Step::Account,
            Screen::Checking
            | Screen::Unsupported
            | Screen::NotReady
            | Screen::Existing
            | Screen::NoRelease
            | Screen::NeedsUpdate => Step::Check,
            Screen::Plan => Step::Size,
            Screen::Downloading => Step::Download,
            Screen::Review | Screen::Approval | Screen::Installing | Screen::Stopped => Step::Install,
            Screen::Recovery => Step::Recovery,
        }
    }

    pub fn inspection(&self) -> Option<&Inspection> {
        self.inspection.as_ref().and_then(|r| r.as_ref().ok())
    }
    pub fn verified(&self) -> Option<&Verified> {
        self.catalog.as_ref().and_then(|r| r.as_ref().ok())
    }

    pub fn candidate(&self) -> Option<&Candidate> {
        let place = self.place.as_ref()?;
        self.inspection()?.candidates.iter().find(|c| match (c, place) {
            (Candidate::FreeSpace { extent }, Place::FreeSpace { offset }) => extent.offset == *offset,
            (Candidate::ShrinkContainer { container, .. }, Place::ShrinkContainer { container: c }) => container == c,
            _ => false,
        })
    }

    /// The smallest and largest sizes the plan screen allows.
    pub fn size_range(&self) -> Option<(u64, u64)> {
        let release = self.release.as_ref()?;
        let candidate = self.candidate()?;
        let min = release.allocation.minimum.div_ceil(sekka_mac_contract::ALIGN) * sekka_mac_contract::ALIGN;
        let max = capacity(release, candidate, self.to_download?);
        (max >= min).then_some((min, max))
    }

    /// The rows of the checking screen.
    pub fn checks(&self) -> Vec<CheckRow> {
        let mut rows = Vec::new();
        let i = self.inspection();
        let pending = |label| CheckRow {
            label,
            detail: "Checking…".into(),
            state: CheckState::Pending,
        };
        match (&self.inspection, i) {
            (None, _) => {
                for label in ["This Mac", "macOS", "Power", "FileVault", "Disk"] {
                    rows.push(pending(label));
                }
            }
            (Some(Err(e)), _) => rows.push(CheckRow {
                label: "This Mac",
                detail: e.clone(),
                state: CheckState::Bad,
            }),
            (Some(Ok(_)), Some(i)) => {
                let supported = self.refusals().is_empty();
                rows.push(CheckRow {
                    label: "This Mac",
                    detail: format!("{} ({})", i.machine.product_name, i.machine.product_type),
                    state: if supported { CheckState::Good } else { CheckState::Bad },
                });
                rows.push(CheckRow {
                    label: "macOS",
                    detail: format!("macOS {} ({})", i.macos.version, i.macos.build),
                    state: if i.issues.iter().any(|x| x.code == IssueCode::MacosTooOld) {
                        CheckState::Bad
                    } else if i.issues.iter().any(|x| x.code == IssueCode::KnownMacosIssue) {
                        CheckState::Warning
                    } else {
                        CheckState::Good
                    },
                });
                let power = if !i.power.has_battery {
                    "Connected to power".to_string()
                } else if i.power.on_ac {
                    format!("On the charger{}", i.power.battery_percent.map(|p| format!(", {p}%")).unwrap_or_default())
                } else {
                    format!("On battery{}", i.power.battery_percent.map(|p| format!(", {p}%")).unwrap_or_default())
                };
                rows.push(CheckRow {
                    label: "Power",
                    detail: power,
                    state: if i.power.on_ac || !i.power.has_battery { CheckState::Good } else { CheckState::Bad },
                });
                rows.push(CheckRow {
                    label: "FileVault",
                    detail: text::filevault(i.filevault).into(),
                    state: match i.filevault {
                        sekka_mac_contract::FileVault::Converting => CheckState::Bad,
                        sekka_mac_contract::FileVault::Unknown => CheckState::Warning,
                        _ => CheckState::Good,
                    },
                });
                let best = i.candidates.iter().map(|c| c.capacity()).max().unwrap_or(0);
                rows.push(CheckRow {
                    label: "Disk",
                    detail: format!("Up to {} can be given to Sekka", text::bytes(best)),
                    state: if i.issues.iter().any(|x| x.code == IssueCode::NotEnoughSpace) {
                        CheckState::Bad
                    } else {
                        CheckState::Good
                    },
                });
            }
            _ => {}
        }
        rows.push(match &self.catalog {
            None => pending("Release"),
            Some(Err(e)) => CheckRow {
                label: "Release",
                detail: e.clone(),
                state: CheckState::Bad,
            },
            Some(Ok(_)) => match &self.release {
                Some(r) => CheckRow {
                    label: "Release",
                    detail: r.title.to_string(),
                    state: CheckState::Good,
                },
                None if self.needs_update() => CheckRow {
                    label: "Release",
                    detail: "Needs a different Sekka Installer".into(),
                    state: CheckState::Bad,
                },
                None => CheckRow {
                    label: "Release",
                    detail: "No release for this Mac yet".into(),
                    state: CheckState::Bad,
                },
            },
        });
        rows
    }

    /// Why this Mac can't have the release, from its profile.
    pub fn refusals(&self) -> Vec<sekka_mac_contract::Refusal> {
        let (Some(i), Some(v)) = (self.inspection(), self.verified()) else {
            return Vec::new();
        };
        let facts = i.machine_facts();
        // The release for this Mac: the first whose profile admits it, or
        // the first at all to explain why not.
        let releases = &v.catalog().releases;
        let admitted = releases.iter().any(|r| {
            v.catalog()
                .profile(&r.profile)
                .is_some_and(|p| p.admit(&facts).admitted())
        });
        if admitted || releases.is_empty() {
            return Vec::new();
        }
        let r = &releases[0];
        v.catalog()
            .profile(&r.profile)
            .map(|p| p.admit(&facts).refusals)
            .unwrap_or_default()
    }

    /// Whether the installation the helper knows is installed: Recovery's
    /// step is done, by its record or by the disk.
    pub fn installed(&self) -> bool {
        self.state == Some(InstallState::WaitingForSetup)
            || self
                .installation
                .is_some_and(|id| self.on_disk(id).is_some_and(|x| x.state == InstallationState::Complete))
    }

    /// What this Mac's own inspection shows of an installation the helper
    /// knows: the installation on the disk that names it, if any.
    fn on_disk(&self, installation: Uuid) -> Option<&sekka_mac_contract::Installation> {
        self.inspection()?
            .installations
            .iter()
            .find(|x| x.installation_id == Some(installation))
    }

    /// The catalog's releases whose profile admits this Mac.
    fn admitted(&self) -> Vec<&Release> {
        let (Some(i), Some(v)) = (self.inspection(), self.verified()) else { return Vec::new() };
        let facts = i.machine_facts();
        v.catalog()
            .releases
            .iter()
            .filter(|r| v.catalog().profile(&r.profile).is_some_and(|p| p.admit(&facts).admitted()))
            .collect()
    }

    /// Whether this app can install a release, as its helper checks before
    /// it begins: the catalog speaks this app's protocol and doesn't ask for
    /// a newer app, and the release was made with the engine this app
    /// carries.
    fn installable(&self, release: &Release) -> bool {
        let Some(v) = self.verified() else { return false };
        let catalog = v.catalog();
        catalog.protocol == HELPER_PROTOCOL
            && self.app_version >= catalog.minimum_app_version
            && matches!(&self.engine, Some(Ok(d)) if *d == release.artifacts.engine.digest)
    }

    /// Whether this Mac has a release, but none this app can install.
    pub fn needs_update(&self) -> bool {
        self.engine.is_some() && self.release.is_none() && !self.admitted().is_empty()
    }

    /// Installations the helper knows that need the person's attention.
    pub fn unfinished(&self) -> Option<&Summary> {
        let Some(Ok(list)) = &self.helper else { return None };
        list.iter().rev().find(|s| s.running || !s.state.allows_new_installation() || s.state == InstallState::WaitingForSetup)
    }

    pub fn update(&mut self, msg: Msg) -> Vec<Effect> {
        match msg {
            Msg::Inspected(result) => {
                if let Err(e) = &result {
                    self.log(format!("Inspection failed: {e}"));
                }
                let rechecking = std::mem::take(&mut self.rechecking);
                if rechecking && let Err(e) = &result {
                    // The plan screen keeps the last good look at the disk,
                    // and says this one failed.
                    self.plan = None;
                    self.plan_error = Some(text::recheck_failed(e));
                    self.screen = Screen::Plan;
                    return Vec::new();
                }
                self.inspection = Some(result);
                let mut effects = self.choose_release();
                effects.extend(if rechecking { self.prepare_review() } else { self.decide() });
                effects
            }
            Msg::Engine(result) => {
                if let Err(e) = &result {
                    self.log(format!("Engine: {e}"));
                }
                self.engine = Some(result);
                let mut effects = self.choose_release();
                effects.extend(self.decide());
                effects
            }
            Msg::Catalog(result) => {
                if let Err(e) = &result {
                    self.log(format!("Catalog: {e}"));
                }
                self.catalog = Some(result);
                let mut effects = self.choose_release();
                effects.extend(self.decide());
                effects
            }
            Msg::HelperStatus(result) => {
                self.helper = Some(result);
                self.decide()
            }
            Msg::ToDownload(bytes) => {
                self.to_download = Some(bytes);
                self.clamp_size();
                self.decide()
            }
            Msg::DownloadProgress(p) => {
                if let Some(reason) = &p.retrying {
                    self.log(format!("Download retrying: {reason}"));
                }
                self.download = Some(p);
                Vec::new()
            }
            Msg::Downloaded(Ok(files)) => {
                self.downloading = false;
                self.download_error = None;
                self.staged = files;
                self.to_download = Some(0);
                // The downloads took space macOS could have given; plan
                // from a fresh look at the disk.
                self.rechecking = true;
                vec![Effect::Inspect]
            }
            Msg::Downloaded(Err(e)) => {
                self.downloading = false;
                self.log(format!("Download stopped: {e}"));
                self.download_error = Some(e);
                Vec::new()
            }
            // A running installation or download is already being watched.
            Msg::CheckAgain if self.running || self.downloading => Vec::new(),
            Msg::CheckAgain => {
                self.notice = None;
                self.check_again()
            }
            Msg::SetSize(bytes) => {
                self.size = bytes;
                self.clamp_size();
                Vec::new()
            }
            Msg::SetBackup(on) => {
                self.backup = on;
                Vec::new()
            }
            Msg::SetAcknowledged(a, on) => {
                if on {
                    self.acknowledged.insert(a);
                } else {
                    self.acknowledged.remove(&a);
                }
                Vec::new()
            }
            Msg::StartDownload => {
                let Some(release) = self.release.clone() else { return Vec::new() };
                self.screen = Screen::Downloading;
                self.downloading = true;
                self.download_error = None;
                self.download = None;
                vec![Effect::Download(Box::new(release))]
            }
            Msg::StopDownload => {
                if self.downloading {
                    vec![Effect::StopDownload]
                } else {
                    self.screen = Screen::Plan;
                    Vec::new()
                }
            }
            // Nothing was downloaded in this session: the download checks
            // what an earlier one left, keeps it, and then reviews.
            Msg::Review if self.staged.is_empty() => self.update(Msg::StartDownload),
            // The review is of the disk as it is now: the charger may have
            // been unplugged, or the disk changed, since the last look.
            Msg::Review => {
                self.screen = Screen::Downloading;
                self.plan_error = None;
                self.rechecking = true;
                vec![Effect::Inspect]
            }
            Msg::BackToPlan => {
                self.screen = Screen::Plan;
                self.plan = None;
                Vec::new()
            }
            Msg::Install => {
                if !self.ready_to_install() {
                    return Vec::new();
                }
                match self.registration {
                    Some(Registration::Ready) => {
                        self.open_authorization(false);
                        Vec::new()
                    }
                    _ => vec![Effect::RegisterHelper],
                }
            }
            Msg::Registration(Ok(r)) => {
                self.registration = Some(r);
                match (r, self.screen) {
                    (Registration::Ready, Screen::Review | Screen::Approval) => {
                        self.screen = Screen::Review;
                        self.open_authorization(false);
                        Vec::new()
                    }
                    (Registration::NeedsApproval, Screen::Review) => {
                        self.screen = Screen::Approval;
                        vec![Effect::OpenLoginItems]
                    }
                    _ => Vec::new(),
                }
            }
            Msg::Registration(Err(e)) => {
                self.notice = Some(format!("Sekka Installer's helper couldn't be set up: {e}"));
                Vec::new()
            }
            Msg::Tick => {
                if self.screen == Screen::Approval {
                    vec![Effect::CheckRegistration]
                } else {
                    Vec::new()
                }
            }
            Msg::OpenLoginItems => vec![Effect::OpenLoginItems],
            Msg::Authorize { owner, password } => self.authorize(owner, password),
            Msg::CancelAuthorization => {
                self.authorization = None;
                Vec::new()
            }
            Msg::Begun(Ok(installation)) => {
                self.authorization = None;
                self.installation = Some(installation);
                self.records.clear();
                self.screen = Screen::Installing;
                self.running = true;
                vec![Effect::Poll {
                    installation,
                    after: 0,
                    delay_ms: 0,
                }]
            }
            Msg::Begun(Err(refusal)) => self.refused(refusal),
            Msg::Events(Ok(events)) => self.events(events),
            Msg::Events(Err(e)) => {
                self.log(format!("Lost touch with the helper: {e}"));
                // Keep asking: the helper keeps going without the app.
                self.installation
                    .map(|installation| {
                        vec![Effect::Poll {
                            installation,
                            after: self.records.last().map_or(0, |r| r.sequence),
                            delay_ms: 2000,
                        }]
                    })
                    .unwrap_or_default()
            }
            Msg::CancelInstallation => match self.installation {
                Some(id) if !self.cancelling => {
                    self.cancelling = true;
                    vec![Effect::Cancel(id)]
                }
                _ => Vec::new(),
            },
            Msg::CancelAnswered(result) => {
                self.cancelling = false;
                match (result, self.installation) {
                    (Err(r), _) => {
                        self.notice = Some(r.message.to_string());
                        Vec::new()
                    }
                    // A stopped installation reports nothing more by
                    // itself: ask for the record that closes it.
                    (Ok(()), Some(installation)) if !self.running => vec![Effect::Poll {
                        installation,
                        after: self.records.last().map_or(0, |r| r.sequence),
                        delay_ms: 0,
                    }],
                    (Ok(()), _) => Vec::new(),
                }
            }
            Msg::RetryStartupDisk => {
                self.open_authorization(true);
                Vec::new()
            }
            Msg::Retried(Ok(events)) => {
                self.authorization = None;
                self.events(events)
            }
            Msg::Retried(Err(refusal)) => self.refused(refusal),
            Msg::Reconciled(result) => {
                let Some(Reconciling::Asked(id)) = self.reconciling else { return Vec::new() };
                self.reconciling = Some(Reconciling::Answered(id));
                match result {
                    Ok(events) => {
                        if let Some(Ok(list)) = &mut self.helper
                            && let Some(summary) = list.iter_mut().find(|s| s.installation == id)
                        {
                            summary.state = events.state.clone();
                            summary.running = events.running;
                        }
                        if events.state == InstallState::Removed {
                            self.notice = Some(text::REMOVED.into());
                        }
                    }
                    Err(refusal) => {
                        self.log(format!("The helper didn't bring the installation's record up to date: {}", refusal.message));
                    }
                }
                self.decide()
            }
            Msg::CopyDiagnostics => match self.installation {
                Some(id) => vec![Effect::Diagnostics(id)],
                None => vec![Effect::CopyToPasteboard(self.report(&[]))],
            },
            Msg::Diagnostics(lines) => vec![Effect::CopyToPasteboard(self.report(&lines))],
            Msg::ShutDown => vec![Effect::ShutDown],
            Msg::ToggleDetails => {
                self.details = !self.details;
                Vec::new()
            }
            Msg::DismissNotice => {
                self.notice = None;
                Vec::new()
            }
        }
    }

    /// Look at everything again from the checking screen: this Mac, the
    /// catalog and the helper's installations.
    fn check_again(&mut self) -> Vec<Effect> {
        self.screen = Screen::Checking;
        self.inspection = None;
        self.catalog = None;
        self.helper = None;
        self.release = None;
        self.to_download = None;
        self.plan = None;
        self.plan_error = None;
        self.rechecking = false;
        self.installation = None;
        self.records.clear();
        self.state = None;
        self.reconciling = None;
        vec![Effect::Inspect, Effect::LoadCatalog, Effect::HelperStatus]
    }

    fn open_authorization(&mut self, retry: bool) {
        self.authorization = Some(Authorization {
            owner: self.user.0.clone(),
            error: None,
            busy: false,
            retry,
        });
    }

    fn authorize(&mut self, owner: String, password: Secret) -> Vec<Effect> {
        let Some(auth) = self.authorization.as_mut() else { return Vec::new() };
        if auth.busy {
            return Vec::new();
        }
        if password.is_empty() {
            auth.error = Some("Enter the password for this account.".into());
            return Vec::new();
        }
        auth.owner = owner.clone();
        auth.error = None;
        auth.busy = true;
        if auth.retry {
            return match self.installation {
                Some(id) => vec![Effect::RetryStartupDisk(id, password)],
                None => Vec::new(),
            };
        }
        match self.begin_request(owner) {
            Ok(begin) => vec![Effect::Begin(Box::new(begin), password)],
            Err(e) => {
                let auth = self.authorization.as_mut().expect("open");
                auth.busy = false;
                auth.error = Some(e);
                Vec::new()
            }
        }
    }

    fn begin_request(&self, owner: String) -> Result<Begin, String> {
        let plan = self.plan.as_ref().ok_or("There's no plan to install.")?;
        let verified = self.verified().ok_or("The release catalog isn't loaded.")?;
        let release = self.release.as_ref().ok_or("No release is chosen.")?;
        let place = self.place.clone().ok_or("No place is chosen.")?;
        let mut confirmation = Confirmation::for_plan(plan, self.backup).map_err(|e| e.to_string())?;
        confirmation.acknowledgements = plan
            .acknowledgements
            .iter()
            .copied()
            .filter(|a| self.acknowledged.contains(a))
            .collect();
        let staged = self
            .staged
            .iter()
            .map(|s| WireStagedFile {
                role: s.role,
                path: s.file.path.to_string_lossy().parse().expect("staging paths are text"),
                size: s.file.size,
                digest: s.file.digest,
            })
            .collect();
        Ok(Begin {
            app: self.app_version.clone(),
            catalog: verified.signed().clone(),
            request: PlanRequest {
                release: release.id.clone(),
                place,
                sekka: plan.allocation.sekka,
            },
            plan: plan.digest().map_err(|e| e.to_string())?,
            confirmation,
            staged,
            owner: owner.parse().map_err(|_| "That account name can't be used.".to_string())?,
        })
    }

    fn refused(&mut self, refusal: HelperRefusal) -> Vec<Effect> {
        let message = refusal.message.to_string();
        self.log(format!("The helper refused: {message}"));
        match refusal.code {
            HelperRefusalCode::Authorization => {
                if let Some(auth) = self.authorization.as_mut() {
                    auth.busy = false;
                    auth.error = Some(message);
                }
                Vec::new()
            }
            HelperRefusalCode::PlanChanged => {
                self.authorization = None;
                self.plan = None;
                self.notice = Some(message);
                self.screen = Screen::Checking;
                self.inspection = None;
                vec![Effect::Inspect]
            }
            _ => {
                self.authorization = None;
                self.notice = Some(message);
                Vec::new()
            }
        }
    }

    fn events(&mut self, events: Events) -> Vec<Effect> {
        for r in events.records {
            if self.records.last().is_none_or(|last| r.sequence > last.sequence) {
                self.records.push(r);
            }
        }
        self.progress = events.progress;
        self.running = events.running;
        let state = events.state;
        self.state = Some(state.clone());
        let Some(installation) = self.installation else { return Vec::new() };
        if events.running {
            self.screen = Screen::Installing;
            return vec![Effect::Poll {
                installation,
                after: self.records.last().map_or(0, |r| r.sequence),
                delay_ms: 400,
            }];
        }
        match state {
            InstallState::WaitingForRecovery => {
                self.screen = Screen::Recovery;
                vec![Effect::RemoveStaging]
            }
            InstallState::Cancelled | InstallState::Removed => {
                let notice = if state == InstallState::Removed { text::REMOVED } else { text::CANCELLED };
                // An installation started in this run goes back to its
                // plan; one reconnected to has none, so this Mac is looked
                // at again.
                let effects = if self.plan.is_some() {
                    self.installation = None;
                    self.screen = Screen::Plan;
                    Vec::new()
                } else {
                    self.check_again()
                };
                self.notice = Some(notice.into());
                effects
            }
            InstallState::WaitingForSetup => {
                self.screen = Screen::Existing;
                Vec::new()
            }
            _ => {
                self.screen = Screen::Stopped;
                Vec::new()
            }
        }
    }

    pub fn failure(&self) -> Option<&sekka_mac_contract::Failure> {
        self.records.iter().rev().find_map(|r| match &r.event {
            sekka_mac_contract::Event::Failed { failure, .. } => Some(failure),
            _ => None,
        })
    }

    /// The stages to show while installing, with their state.
    pub fn stages(&self) -> Vec<(Stage, CheckState)> {
        let shrinks = self
            .plan
            .as_ref()
            .map(|p| p.actions.iter().any(|a| matches!(a, sekka_mac_contract::Action::ShrinkContainer { .. })))
            .unwrap_or(true);
        let mut stages = vec![Stage::Import];
        stages.extend(sekka_mac_contract::macos_stages(shrinks));
        stages
            .into_iter()
            .map(|s| {
                let mut state = CheckState::Pending;
                for r in &self.records {
                    match &r.event {
                        sekka_mac_contract::Event::Intent { stage } if *stage == s => state = CheckState::Warning,
                        sekka_mac_contract::Event::Completed { stage, .. } if *stage == s => state = CheckState::Good,
                        sekka_mac_contract::Event::Failed { stage, .. } if *stage == s => state = CheckState::Bad,
                        _ => {}
                    }
                }
                (s, state)
            })
            .collect()
    }

    /// Whether cancelling could still leave the disk untouched.
    pub fn cancel_allowed(&self) -> bool {
        self.running
            && !self.cancelling
            && !self.records.iter().any(|r| {
                matches!(&r.event, sekka_mac_contract::Event::Intent { stage } if stage.mutates())
            })
    }

    pub fn ready_to_install(&self) -> bool {
        let Some(plan) = &self.plan else { return false };
        self.backup && plan.acknowledgements.iter().all(|a| self.acknowledged.contains(a))
    }

    /// Choose the release this Mac is offered once it, the catalog and the
    /// bundled engine are known, whichever arrives last, and measure what is
    /// left to download for it. A release this app can't install is never
    /// offered, so nothing is downloaded for it.
    fn choose_release(&mut self) -> Vec<Effect> {
        if self.inspection().is_none() || self.verified().is_none() || self.engine.is_none() {
            return Vec::new();
        }
        let chosen = self.admitted().into_iter().find(|r| self.installable(r)).cloned();
        if chosen.as_ref().map(|r| &r.id) == self.release.as_ref().map(|r| &r.id) {
            return Vec::new();
        }
        self.release = chosen;
        self.to_download = None;
        self.release
            .iter()
            .map(|r| Effect::MeasureDownload(Box::new(r.clone())))
            .collect()
    }

    fn clamp_size(&mut self) {
        if let Some((min, max)) = self.size_range() {
            self.size = (self.size.clamp(min, max) / sekka_mac_contract::ALIGN) * sekka_mac_contract::ALIGN;
        }
    }

    /// Once everything is known on the checking screen, where to go.
    fn decide(&mut self) -> Vec<Effect> {
        if self.screen != Screen::Checking {
            return Vec::new();
        }
        // An installation the helper is running, or one that needs
        // attention, comes first: reopening the app reconnects to it rather
        // than starting another.
        if let Some(summary) = self.unfinished().cloned() {
            if summary.running {
                self.installation = Some(summary.installation);
                self.state = Some(summary.state.clone());
                self.screen = Screen::Installing;
                self.running = true;
                return vec![Effect::Poll {
                    installation: summary.installation,
                    after: 0,
                    delay_ms: 0,
                }];
            }
            if let Some(effects) = self.reconnect(&summary) {
                return effects;
            }
        }
        let (Some(inspection), Some(catalog), Some(_), Some(_)) =
            (&self.inspection, &self.catalog, &self.helper, &self.engine)
        else {
            return Vec::new();
        };
        let Ok(inspection) = inspection else {
            self.screen = Screen::NotReady;
            return Vec::new();
        };
        if inspection.installations.iter().any(|x| x.kind == InstallationKind::Sekka) {
            self.screen = Screen::Existing;
            return Vec::new();
        }
        if inspection
            .issues
            .iter()
            .any(|x| x.blocking && x.code == IssueCode::UnsupportedMachine)
            || !self.refusals().is_empty()
        {
            self.screen = Screen::Unsupported;
            return Vec::new();
        }
        // Before anything to fix on this Mac: this app couldn't install
        // once it was fixed.
        if self.needs_update() {
            self.screen = Screen::NeedsUpdate;
            return Vec::new();
        }
        if inspection.blocking().next().is_some() {
            self.screen = Screen::NotReady;
            return Vec::new();
        }
        if catalog.is_err() || self.release.is_none() {
            self.screen = Screen::NoRelease;
            return Vec::new();
        }
        // The plan's sizes depend on what is left to download.
        let Some(to_download) = self.to_download else { return Vec::new() };
        // The largest place, at the release's suggested size.
        let release = self.release.clone().expect("checked");
        let best = inspection
            .candidates
            .iter()
            .max_by_key(|c| capacity(&release, c, to_download));
        match best.and_then(|c| suggested_size(&release, c, to_download).map(|s| (c, s))) {
            Some((c, size)) => {
                self.place = Some(match c {
                    Candidate::FreeSpace { extent } => Place::FreeSpace { offset: extent.offset },
                    Candidate::ShrinkContainer { container, .. } => Place::ShrinkContainer { container: *container },
                });
                if self.size == 0 {
                    self.size = size;
                }
                self.clamp_size();
                self.screen = if self.staged.is_empty() { Screen::Plan } else { Screen::Review };
                if self.screen == Screen::Review {
                    return self.prepare_review();
                }
            }
            None => {
                self.screen = Screen::NotReady;
            }
        }
        Vec::new()
    }

    /// Reconnect to a stopped installation the helper knows, or `None` when
    /// it no longer stands in the way. Its record is read against this Mac's
    /// own inspection first: the record knows nothing of what Recovery did,
    /// or of partitions removed by hand. When the two disagree, the helper
    /// is asked (once) to look as root and bring the record up to date, and
    /// this is decided again from its answer.
    fn reconnect(&mut self, summary: &Summary) -> Option<Vec<Effect>> {
        let id = summary.installation;
        let Some(inspection) = &self.inspection else { return Some(Vec::new()) };
        let inspected = inspection.is_ok();
        let found = self.on_disk(id).map(|x| x.state);
        let complete = found == Some(InstallationState::Complete);
        let gone = inspected && found.is_none();
        match self.reconciling {
            Some(Reconciling::Asked(asked)) if asked == id => return Some(Vec::new()),
            Some(Reconciling::Answered(answered)) if answered == id => {}
            _ => {
                let recovered = complete
                    && matches!(
                        summary.state,
                        InstallState::WaitingForRecovery | InstallState::EnrollmentUncertain
                    );
                let removed = gone && !summary.state.allows_new_installation();
                if recovered || removed {
                    self.reconciling = Some(Reconciling::Asked(id));
                    return Some(vec![Effect::Reconcile(id)]);
                }
            }
        }
        // Finished, then removed by hand: nothing of it stands in the way.
        if summary.state == InstallState::WaitingForSetup && gone {
            return None;
        }
        self.installation = Some(id);
        self.state = Some(summary.state.clone());
        let poll = vec![Effect::Poll {
            installation: id,
            after: 0,
            delay_ms: 0,
        }];
        Some(match summary.state {
            // Whatever the record says, a complete Sekka on the disk is
            // installed and never needs finishing in Recovery.
            _ if complete => {
                self.screen = Screen::Existing;
                Vec::new()
            }
            InstallState::WaitingForRecovery => {
                self.screen = Screen::Recovery;
                poll
            }
            InstallState::WaitingForSetup => {
                self.screen = Screen::Existing;
                Vec::new()
            }
            _ => {
                self.screen = Screen::Stopped;
                poll
            }
        })
    }

    /// Derive the plan the helper will derive, from the latest inspection.
    fn prepare_review(&mut self) -> Vec<Effect> {
        let (Some(inspection), Some(verified), Some(release)) =
            (self.inspection().cloned(), self.verified().cloned(), self.release.clone())
        else {
            return Vec::new();
        };
        let Some(place) = self.place.clone() else { return Vec::new() };
        // Sizes may have shrunk now that the downloads take space.
        if let Some((min, max)) = self.size_range()
            && self.size > max
        {
            self.notice = Some(format!(
                "The downloads took some of the space macOS could give, so Sekka's size is now {}.",
                text::bytes(max.max(min))
            ));
        }
        self.clamp_size();
        let request = PlanRequest {
            release: release.id.clone(),
            place,
            sekka: self.size,
        };
        match derive_plan(&inspection, verified.catalog(), &request, self.to_download.unwrap_or(0)) {
            Ok(plan) => {
                self.plan = Some(plan);
                self.plan_error = None;
                self.screen = Screen::Review;
            }
            Err(e) => {
                self.plan = None;
                // What stops installing has a screen of its own, which says
                // what it is and what to do.
                self.screen = match e {
                    PlanError::Blocked(_) => Screen::NotReady,
                    PlanError::NotAdmitted(_) => Screen::Unsupported,
                    _ => {
                        self.plan_error = Some(text::plan_error(&e));
                        Screen::Plan
                    }
                };
            }
        }
        Vec::new()
    }

    /// A support report: no passwords, no account names, no home folders.
    pub fn report(&self, helper_lines: &[String]) -> String {
        let mut out = vec![format!("{} {}", text::APP_NAME, self.app_version)];
        if let Some(i) = self.inspection() {
            out.push(format!(
                "{} ({}, {}), system firmware {}, macOS {} ({})",
                i.machine.product_name,
                i.machine.product_type,
                i.machine.device_class,
                i.machine.system_firmware,
                i.macos.version,
                i.macos.build
            ));
            for issue in &i.issues {
                out.push(format!("Issue {:?}: {}", issue.code, issue.detail));
            }
        }
        if let Some(Ok(d)) = &self.engine {
            out.push(format!("Engine {d}"));
        }
        if let Some(r) = &self.release {
            out.push(format!("Release {} ({})", r.id, r.source.revision));
        }
        if let Some(p) = &self.plan
            && let Ok(d) = p.digest()
        {
            out.push(format!("Plan {d}"));
        }
        if let Some(s) = &self.state {
            out.push(format!("State {s:?}"));
        }
        out.extend(helper_lines.iter().cloned());
        out.push("App log:".into());
        out.extend(self.log.iter().cloned());
        let names = [self.user.0.as_str()];
        out.iter()
            .map(|l| sekka_redact(l, &names))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The staged file for a role.
    pub fn staged(&self, role: ArtifactRole) -> Option<&Staged> {
        self.staged.iter().find(|s| s.role == role)
    }

    /// What the review says will change, in order.
    pub fn changes(&self) -> Vec<String> {
        let Some(plan) = &self.plan else { return Vec::new() };
        let mut out = Vec::new();
        for action in &plan.actions {
            out.push(match action {
                sekka_mac_contract::Action::ShrinkContainer { from, to, .. } => format!(
                    "macOS shrinks from {} to {}. Apple's own tool does this; if it fails, nothing is changed.",
                    text::bytes(*from),
                    text::bytes(*to)
                ),
                sekka_mac_contract::Action::CreateStub { size } => format!("A {} startup volume for Sekka is created.", text::bytes(*size)),
                sekka_mac_contract::Action::CreateEsp { size } => format!("A {} boot partition is created.", text::bytes(*size)),
                sekka_mac_contract::Action::CreateRoot { size } => format!("Sekka gets {} for itself and your files.", text::bytes(*size)),
            });
        }
        out
    }

    /// What a stopped installation lets the person do.
    pub fn stopped_actions(&self) -> Vec<sekka_mac_contract::NextAction> {
        self.state.as_ref().map(|s| s.permitted().to_vec()).unwrap_or_default()
    }

    pub fn disk_changed(&self) -> Option<DiskChanged> {
        match &self.state {
            Some(InstallState::Failed { disk_changed, .. }) => Some(*disk_changed),
            _ => None,
        }
    }
}

fn sekka_redact(line: &str, names: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(i) = rest.find("/Users/") {
        out.push_str(&rest[..i]);
        out.push_str("/Users/…");
        let after = &rest[i + 7..];
        let end = after.find(['/', ' ']).unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    for name in names.iter().filter(|n| n.len() >= 2) {
        out = out.replace(&format!(" {name} "), " (account) ");
    }
    out
}

#[cfg(test)]
mod tests;
