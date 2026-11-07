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

use crate::env::{DownloadProgress, Events, HelperProblem, Registration, Secret, Staged, ToDownload};
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
    /// The installations the helper keeps a record of aren't known: it
    /// refused to list them or didn't answer, or macOS doesn't let it run.
    /// One of them may be unfinished, so nothing is planned until they
    /// are known.
    HelperUnknown,
    /// The helper's record keeps an unfinished installation this Mac's own
    /// inspection doesn't find, and the helper couldn't close the record.
    Missing,
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
    /// It looked, and its record says what it found.
    Answered(Uuid),
    /// It couldn't look: it refused, or wasn't reached.
    Refused(Uuid),
}

/// How many times the helper is asked again, a moment apart, when it is
/// busy as it is asked to reconcile.
const RECONCILE_TRIES: u8 = 5;

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
    ToDownload(ToDownload),
    DownloadProgress(DownloadProgress),
    Downloaded(Result<Vec<Staged>, String>),
    Registration(Result<Registration, String>),
    Begun(Result<Uuid, HelperRefusal>),
    Events(Result<Events, String>),
    CancelAnswered(Result<(), HelperRefusal>),
    Retried(Result<Events, HelperRefusal>),
    Reconciled(Result<Events, HelperRefusal>),
    /// An account's full name, when this Mac knows it.
    FullName { account: String, name: Option<String> },
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
    /// Set up the helper, so its installations can be known.
    RegisterHelper,
    /// Once a second while waiting for the helper to be allowed.
    Tick,
    CancelInstallation,
    RetryStartupDisk,
    CopyDiagnostics,
    /// Install again past a finished installation this Mac can't find.
    InstallAgain,
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
    /// Ask the helper to look at the Mac for a stopped installation, after
    /// a moment when it was busy.
    Reconcile { installation: Uuid, delay_ms: u64 },
    /// Look up the full name recoveryOS lists an account by.
    FullName(String),
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
    /// What is still to download before installing: the bytes to fetch,
    /// which the plan screen states, and the space fetching them takes,
    /// which the plan's sizes leave room for. None until measured for the
    /// release offered.
    pub to_download: Option<ToDownload>,
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
    /// Install was chosen before the helper could say which installations
    /// it knows: the authorization sheet opens once it is allowed to run
    /// and has said, and only if none of them stands in the way.
    pub authorize_next: bool,
    pub installation: Option<Uuid>,
    /// The administrator who authorized the installation, by short name, as
    /// the helper recorded it. Recovery asks for the same account, and so
    /// does setting the startup disk again.
    pub owner: Option<String>,
    /// That administrator's full name, once looked up.
    pub owner_full_name: Option<String>,
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
    /// How many times in this check the helper was busy when asked.
    pub reconcile_busy: u8,
    /// Why the helper didn't set the startup disk again for an
    /// installation, when it refused before trying. Nothing is recorded
    /// then, and looking at the Mac again doesn't check what a retry
    /// checks, so it stands until a retry gets further.
    pub retry_refused: Option<(Uuid, String)>,
    /// A finished installation this Mac can't find, which the owner chose
    /// to install again past.
    pub set_aside: Option<Uuid>,
    /// One set aside, past which the helper then wouldn't install: its own
    /// look at the disk found something that stops installation, which may
    /// be that installation. This Mac's own look can't say otherwise.
    pub refused_past: Option<Uuid>,
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
            authorize_next: false,
            installation: None,
            owner: None,
            owner_full_name: None,
            records: Vec::new(),
            state: None,
            progress: None,
            running: false,
            details: false,
            log: VecDeque::new(),
            notice: None,
            cancelling: false,
            reconciling: None,
            reconcile_busy: 0,
            retry_refused: None,
            set_aside: None,
            refused_past: None,
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
            | Screen::HelperUnknown
            | Screen::Missing
            | Screen::NoRelease
            | Screen::NeedsUpdate => Step::Check,
            Screen::Plan => Step::Size,
            // Looking at the Mac again is the review's first part.
            Screen::Downloading if self.rechecking && !self.downloading => Step::Install,
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
        let max = capacity(release, candidate, self.to_download?.space);
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
        if matches!(self.reconciling, Some(Reconciling::Asked(_))) {
            rows.push(CheckRow {
                label: "Earlier installation",
                detail: text::RECONCILING.into(),
                state: CheckState::Pending,
            });
        }
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

    /// Whether the installation the helper knows is installed: complete on
    /// the disk, or Recovery's step done by its record and there on the
    /// disk, or found complete by the helper's own look in this check. This
    /// Mac's own look can't read the boot policy, so it usually can't call
    /// a finished installation complete; and one it finds only in part, or
    /// not at all, isn't installed by its record alone.
    pub fn installed(&self) -> bool {
        let found = self.found();
        let confirmed = match (self.installation, &self.state) {
            (Some(id), Some(state)) => self.confirmed(id, state),
            _ => false,
        };
        confirmed
            || found == Some(InstallationState::Complete)
            || (self.state == Some(InstallState::WaitingForSetup)
                && found.is_some_and(|s| s != InstallationState::Partial))
    }

    /// Whether the helper, looking at the Mac as root in this check, found
    /// an installation it knows complete, made of the parts its record
    /// names, and recorded Recovery's steps. Only that look brings a record
    /// to waiting for setup, and the helper isn't asked about one that
    /// waits for setup already. This Mac's own look, as the person using
    /// the app, may not even identify it, when it can't read the
    /// installation's records.
    fn confirmed(&self, installation: Uuid, state: &InstallState) -> bool {
        *state == InstallState::WaitingForSetup && self.reconciling == Some(Reconciling::Answered(installation))
    }

    /// What this Mac's own inspection shows of the installation the helper
    /// knows.
    pub fn found(&self) -> Option<InstallationState> {
        self.on_disk(self.installation?).map(|x| x.state)
    }

    /// Whether the record says the installation finished, and this Mac's
    /// own look, which did look, doesn't find it: something it can't
    /// identify is on the disk, and may be it. Never one the helper found
    /// there itself.
    pub fn finished_unfound(&self) -> bool {
        self.state == Some(InstallState::WaitingForSetup)
            && self.installation.is_some()
            && self.found().is_none()
            && self.unidentified()
            && !self.installed()
    }

    /// What this Mac's own inspection shows of an installation the helper
    /// knows: the installation on the disk that names it, if any.
    fn on_disk(&self, installation: Uuid) -> Option<&sekka_mac_contract::Installation> {
        self.inspection()?
            .installations
            .iter()
            .find(|x| x.installation_id == Some(installation))
    }

    /// Whether the helper wouldn't install past the finished installation
    /// shown, when the owner set it aside.
    pub fn install_again_refused(&self) -> bool {
        self.refused_past.is_some() && self.refused_past == self.installation
    }

    /// Whether this Mac's own inspection found a system or partition it
    /// couldn't identify. Run as the person using the app, it can miss an
    /// installation's records, so any of these could be the one it looks
    /// for.
    fn unidentified(&self) -> bool {
        self.inspection()
            .is_some_and(|i| i.installations.iter().any(|x| x.installation_id.is_none()))
    }

    /// Whether nothing on the disk could be an installation the helper
    /// knows, by this Mac's own look: its record says something would name
    /// it, nothing does, and nothing the look couldn't identify could be it
    /// either. Never one the helper found there itself in this check.
    fn gone(&self, summary: &Summary) -> bool {
        self.inspection().is_some()
            && self.on_disk(summary.installation).is_none()
            && named_on_disk(&summary.state)
            && !self.unidentified()
            && !self.confirmed(summary.installation, &summary.state)
    }

    /// Whether the record waits for Recovery, and the helper couldn't look
    /// at the boot policy this time to say whether Recovery's steps are
    /// done already.
    pub fn recovery_unconfirmed(&self) -> bool {
        self.screen == Screen::Recovery
            && matches!(self.reconciling, Some(Reconciling::Refused(id)) if Some(id) == self.installation)
    }

    /// Whether the download is only checking files an earlier run left
    /// whole, which can't be paused part way.
    pub fn checking_download(&self) -> bool {
        self.downloading && self.to_download.is_some_and(|d| d.fetch == 0)
    }

    /// The bytes the download still fetches, as the plan screen says.
    pub fn to_fetch(&self) -> u64 {
        self.to_download.map_or(0, |d| d.fetch)
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

    /// What the helper said when it couldn't list its installations.
    pub fn helper_problem(&self) -> Option<&HelperProblem> {
        self.helper.as_ref()?.as_ref().err()
    }

    /// Installations the helper knows that need the person's attention:
    /// one running, or stopped in the way of another, and a finished one,
    /// unless the owner set it aside or this Mac's own look shows it gone.
    pub fn unfinished(&self) -> Option<&Summary> {
        let Some(Ok(list)) = &self.helper else { return None };
        list.iter().rev().find(|s| {
            let finished = s.state == InstallState::WaitingForSetup;
            (s.running || !s.state.allows_new_installation() || finished)
                && !(finished && (self.set_aside == Some(s.installation) || self.gone(s)))
        })
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
                if let Err(problem) = &result {
                    self.log(format!("The helper's installations aren't known: {problem}"));
                }
                self.helper = Some(result);
                if self.authorize_next && self.screen == Screen::Review {
                    return self.authorize_once_known();
                }
                self.decide()
            }
            Msg::ToDownload(left) => {
                self.to_download = Some(left);
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
                self.to_download = Some(ToDownload::NOTHING);
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
            // A running installation or download is already being watched,
            // an open sheet answers for the page it was opened from, and
            // the helper looking at the Mac answers for this check: asked
            // again meanwhile, it would refuse as busy, and its answer to
            // the first would go unheard.
            Msg::CheckAgain
                if self.running
                    || self.downloading
                    || self.authorization.is_some()
                    || matches!(self.reconciling, Some(Reconciling::Asked(_))) =>
            {
                Vec::new()
            }
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
            // Nothing is downloaded for a plan macOS has no room for.
            Msg::StartDownload if self.macos_room().is_err() => Vec::new(),
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
                self.authorize_next = false;
                Vec::new()
            }
            // The sheet opens once the helper runs and has said which
            // installations it knows: it may have been set up, or allowed,
            // only now.
            Msg::Install => {
                if !self.ready_to_install() {
                    return Vec::new();
                }
                match (self.registration, &self.helper) {
                    (Some(Registration::Ready), Some(Ok(_))) => {
                        self.open_authorization(false);
                        Vec::new()
                    }
                    (Some(Registration::Ready), _) => {
                        self.authorize_next = true;
                        vec![Effect::HelperStatus]
                    }
                    _ => {
                        self.authorize_next = true;
                        vec![Effect::RegisterHelper]
                    }
                }
            }
            Msg::Registration(Ok(r)) => {
                let before = self.registration.replace(r);
                match (r, self.screen) {
                    (Registration::Ready, Screen::Review | Screen::Approval) if self.authorize_next => {
                        self.screen = Screen::Review;
                        match self.helper {
                            Some(Ok(_)) => self.authorize_once_known(),
                            _ => vec![Effect::HelperStatus],
                        }
                    }
                    (Registration::NeedsApproval, Screen::Review) if self.authorize_next => {
                        self.screen = Screen::Approval;
                        vec![Effect::OpenLoginItems]
                    }
                    // Allowed to run now: its installations can be asked for.
                    (Registration::Ready, Screen::HelperUnknown) if self.helper_off() => self.check_again(),
                    // Set up from that page just now: the owner allows it next.
                    (Registration::NeedsApproval, Screen::HelperUnknown) if before == Some(Registration::Absent) => {
                        vec![Effect::OpenLoginItems]
                    }
                    _ => Vec::new(),
                }
            }
            Msg::Registration(Err(e)) => {
                self.authorize_next = false;
                self.notice = Some(format!("Sekka Installer's helper couldn't be set up: {e}"));
                Vec::new()
            }
            Msg::Tick => match self.screen {
                Screen::Approval => vec![Effect::CheckRegistration],
                Screen::HelperUnknown if self.awaiting_approval() => vec![Effect::CheckRegistration],
                _ => Vec::new(),
            },
            Msg::OpenLoginItems => vec![Effect::OpenLoginItems],
            Msg::RegisterHelper => vec![Effect::RegisterHelper],
            Msg::Authorize { owner, password } => self.authorize(owner, password),
            Msg::CancelAuthorization => {
                self.authorization = None;
                Vec::new()
            }
            Msg::Begun(Ok(installation)) => {
                let owner = self.authorization.take().map(|a| a.owner);
                let mut effects = self.set_owner(owner);
                self.installation = Some(installation);
                self.records.clear();
                self.screen = Screen::Installing;
                self.running = true;
                effects.push(Effect::Poll {
                    installation,
                    after: 0,
                    delay_ms: 0,
                });
                effects
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
                // What came of the last try is said again when this one ends.
                self.notice = None;
                self.open_authorization(true);
                Vec::new()
            }
            Msg::Retried(Ok(events)) => {
                self.authorization = None;
                self.retry_refused = None;
                let effects = self.events(events);
                // Tried, and failed again: the failure on the page is this
                // one's.
                if self.state == Some(InstallState::StartupDiskFailed) {
                    self.notice = Some(text::RETRY_FAILED.into());
                }
                effects
            }
            // Refused before trying (the installation isn't as it was left,
            // say), so nothing was recorded: the page says why, and no
            // longer that everything else is in place.
            Msg::Retried(Err(refusal)) if refusal.code == HelperRefusalCode::State => {
                self.log(format!("The helper didn't set the startup disk again: {}", refusal.message));
                self.authorization = None;
                self.retry_refused = self.installation.map(|id| (id, refusal.message.to_string()));
                self.notice = Some(text::RETRY_REFUSED.into());
                Vec::new()
            }
            Msg::Retried(Err(refusal)) => self.refused(refusal),
            Msg::Reconciled(result) => {
                let Some(Reconciling::Asked(id)) = self.reconciling else { return Vec::new() };
                match result {
                    Ok(events) => {
                        self.reconciling = Some(Reconciling::Answered(id));
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
                    // Busy with another request, or an engine still holding
                    // the disk: not a look that failed. Asked again shortly,
                    // a few times.
                    Err(refusal) if refusal.code == HelperRefusalCode::Busy && self.reconcile_busy < RECONCILE_TRIES => {
                        self.reconcile_busy += 1;
                        return vec![Effect::Reconcile {
                            installation: id,
                            delay_ms: 2000,
                        }];
                    }
                    Err(refusal) => {
                        self.reconciling = Some(Reconciling::Refused(id));
                        self.log(format!("The helper didn't bring the installation's record up to date: {}", refusal.message));
                    }
                }
                self.decide()
            }
            Msg::FullName { account, name } => {
                if self.owner.as_deref() == Some(account.as_str()) {
                    self.owner_full_name = name;
                }
                Vec::new()
            }
            // The owner says the finished installation this Mac can't find
            // is gone: it no longer keeps a plan from being offered. (Its
            // record would let another begin; it is what may be the
            // unidentified system beside it that the owner decides about,
            // until the helper's own look at the disk finds Sekka there.)
            Msg::InstallAgain => match self.installation {
                Some(id) if self.screen == Screen::Existing && self.finished_unfound() && !self.install_again_refused() => {
                    self.set_aside = Some(id);
                    self.notice = None;
                    self.check_again()
                }
                _ => Vec::new(),
            },
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
        self.authorize_next = false;
        self.installation = None;
        self.owner = None;
        self.owner_full_name = None;
        self.records.clear();
        self.state = None;
        self.reconciling = None;
        vec![Effect::Inspect, Effect::LoadCatalog, Effect::HelperStatus]
    }

    /// Open the authorization sheet Install asked for, now that the helper
    /// has said which installations it knows: never while one of them
    /// stands in the way, or while they aren't known.
    fn authorize_once_known(&mut self) -> Vec<Effect> {
        self.authorize_next = false;
        match &self.helper {
            Some(Ok(_)) if self.unfinished().is_none() => {
                self.open_authorization(false);
                Vec::new()
            }
            // That one comes first, as it would have when the app opened.
            Some(Ok(_)) => {
                let effects = self.check_again();
                self.notice = Some(text::EARLIER_FIRST.into());
                effects
            }
            _ => {
                self.screen = Screen::HelperUnknown;
                Vec::new()
            }
        }
    }

    /// Whether the helper waits for the owner to allow it in Login Items,
    /// by what the status or macOS last said.
    pub fn awaiting_approval(&self) -> bool {
        self.registration == Some(Registration::NeedsApproval)
            || matches!(self.helper, Some(Err(HelperProblem::NeedsApproval)))
    }

    /// Whether the helper's installations aren't known because macOS
    /// doesn't let it run: it isn't set up, or isn't allowed yet.
    pub fn helper_off(&self) -> bool {
        matches!(
            self.helper,
            Some(Err(HelperProblem::NotRegistered | HelperProblem::NeedsApproval))
        )
    }

    fn open_authorization(&mut self, retry: bool) {
        // A retry is authorized by the installation's own administrator.
        let owner = match (&self.owner, retry) {
            (Some(owner), true) => owner.clone(),
            _ => self.user.0.clone(),
        };
        self.authorization = Some(Authorization {
            owner,
            error: None,
            busy: false,
            retry,
        });
    }

    fn authorize(&mut self, owner: String, password: Secret) -> Vec<Effect> {
        let Some(auth) = &self.authorization else { return Vec::new() };
        if auth.busy {
            return Vec::new();
        }
        let request = if password.is_empty() {
            Err("Enter the password for this account.".to_string())
        } else if auth.retry {
            match (&self.owner, self.installation) {
                // The helper authenticates the administrator it recorded,
                // whatever name is typed: a different one is refused here,
                // not ignored.
                (Some(recorded), _) if !same_account(&owner, recorded, &self.user) => Err(text::retry_owner(recorded)),
                (_, Some(id)) => Ok(Effect::RetryStartupDisk(id, password)),
                (_, None) => Err(text::NOTHING_TO_RETRY.to_string()),
            }
        } else {
            self.begin_request(owner.clone()).map(|begin| Effect::Begin(Box::new(begin), password))
        };
        let auth = self.authorization.as_mut().expect("open");
        match request {
            // The sheet waits only while the helper has something to answer.
            Ok(effect) => {
                auth.owner = owner;
                auth.error = None;
                auth.busy = true;
                vec![effect]
            }
            Err(e) => {
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
                // Installing past a finished installation this Mac can't
                // find: what the helper found may be it, so it is shown
                // again, and not offered to be installed past.
                if let Some(id) = self.set_aside.take() {
                    self.refused_past = Some(id);
                }
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
        let recorded = self.records.iter().find_map(|r| match &r.event {
            sekka_mac_contract::Event::Completed {
                stage: Stage::Import,
                facts,
                ..
            } => facts.iter().find(|f| f.name.as_str() == "owner").map(|f| f.value.to_string()),
            _ => None,
        });
        let mut effects = match recorded {
            Some(owner) => self.set_owner(Some(owner)),
            None => Vec::new(),
        };
        self.progress = events.progress;
        self.running = events.running;
        let state = events.state;
        self.state = Some(state.clone());
        effects.extend(self.follow(state));
        effects
    }

    /// Where an installation's latest state leaves the person.
    fn follow(&mut self, state: InstallState) -> Vec<Effect> {
        let Some(installation) = self.installation else { return Vec::new() };
        if self.running {
            self.screen = Screen::Installing;
            return vec![Effect::Poll {
                installation,
                after: self.records.last().map_or(0, |r| r.sequence),
                delay_ms: 400,
            }];
        }
        match state {
            // What this Mac's own inspection found stands until the record
            // closes the installation.
            _ if self.screen == Screen::Missing && !matches!(state, InstallState::Cancelled | InstallState::Removed) => {
                Vec::new()
            }
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

    /// Keep the administrator who authorized the installation, and look up
    /// the full name recoveryOS lists that account by.
    fn set_owner(&mut self, owner: Option<String>) -> Vec<Effect> {
        if owner == self.owner {
            return Vec::new();
        }
        self.owner = owner;
        self.owner_full_name = None;
        match &self.owner {
            Some(owner) if *owner != self.user.0 => vec![Effect::FullName(owner.clone())],
            _ => Vec::new(),
        }
    }

    /// The installation's administrator as recoveryOS lists accounts: by
    /// full name, once it is known, and by short name until then.
    pub fn owner_name(&self) -> Option<String> {
        let owner = self.owner.as_ref()?;
        Some(if *owner == self.user.0 && !self.user.1.is_empty() {
            self.user.1.clone()
        } else {
            self.owner_full_name.clone().unwrap_or_else(|| owner.clone())
        })
    }

    pub fn failure(&self) -> Option<&sekka_mac_contract::Failure> {
        self.failed().map(|(_, f)| f)
    }

    /// The latest failure, with its place in the record: a retry that
    /// fails again is a new one, though it may read the same.
    pub fn failed(&self) -> Option<(u64, &sekka_mac_contract::Failure)> {
        self.records.iter().rev().find_map(|r| match &r.event {
            sekka_mac_contract::Event::Failed { failure, .. } => Some((r.sequence, failure)),
            _ => None,
        })
    }

    /// Why the helper last refused to set the startup disk again for the
    /// installation shown, before trying, while setting it is still what
    /// failed.
    pub fn retry_refusal(&self) -> Option<&str> {
        match (&self.retry_refused, &self.state) {
            (Some((id, why)), Some(InstallState::StartupDiskFailed)) if self.installation == Some(*id) => Some(why),
            _ => None,
        }
    }

    /// The stopped page's headline and explanation.
    pub fn stopped_words(&self) -> Option<(String, String)> {
        let state = self.state.as_ref()?;
        Some(match self.retry_refusal() {
            Some(why) => text::retry_refused(why),
            None => text::stopped(state),
        })
    }

    /// The plan the installation follows: the one reviewed here, or, for
    /// one reconnected to, the plan its record opened with.
    pub fn recorded_plan(&self) -> Option<&Plan> {
        self.plan.as_ref().or_else(|| match &self.records.first()?.event {
            sekka_mac_contract::Event::Opened { plan, .. } => Some(&**plan),
            _ => None,
        })
    }

    /// The stages to show while installing, with their state: those its
    /// plan runs, so Shrink only when it shrinks macOS. Until the record of
    /// one reconnected to arrives, Shrink is listed.
    pub fn stages(&self) -> Vec<(Stage, CheckState)> {
        let shrinks = self
            .recorded_plan()
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
        let (Some(inspection), Some(catalog), Some(helper), Some(_)) =
            (&self.inspection, &self.catalog, &self.helper, &self.engine)
        else {
            return Vec::new();
        };
        // Install found one of the helper's installations in the way, and
        // this look, with everything known, doesn't stop on it: nothing
        // comes first after all.
        if self.notice.as_deref() == Some(text::EARLIER_FIRST) {
            self.notice = None;
        }
        let Ok(inspection) = inspection else {
            self.screen = Screen::NotReady;
            return Vec::new();
        };
        let sekka = inspection.installations.iter().any(|x| x.kind == InstallationKind::Sekka);
        // Nothing is planned while the helper's installations aren't known:
        // one may be unfinished, and the helper would refuse another only
        // after the download and the password. A helper that was never set
        // up has none from this Mac, unless Sekka is on the disk.
        if matches!(helper, Err(problem) if *problem != HelperProblem::NotRegistered || sekka) {
            self.screen = Screen::HelperUnknown;
            return Vec::new();
        }
        if sekka {
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
            .max_by_key(|c| capacity(&release, c, to_download.space));
        match best.and_then(|c| suggested_size(&release, c, to_download.space).map(|s| (c, s))) {
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
                if let Err(e) = self.macos_room() {
                    self.plan_error = Some(text::plan_error(&e));
                }
            }
            None => {
                self.screen = Screen::NotReady;
            }
        }
        Vec::new()
    }

    /// Reconnect to a stopped installation the helper knows, or `None` when
    /// it no longer stands in the way. Its record knows nothing of what
    /// Recovery did, or of partitions removed by hand, and this Mac's own
    /// inspection, run as the person using the app, can't read the boot
    /// policy or tell what is in the space the plan gave Sekka. So the
    /// helper is asked (once in each check) to look as root and bring the
    /// record up to date, for any installation it would look at
    /// ([`helper_looks`]), and this is decided again from its answer. If it
    /// can't, the inspection decides what it can.
    fn reconnect(&mut self, summary: &Summary) -> Option<Vec<Effect>> {
        let id = summary.installation;
        let Some(inspection) = &self.inspection else { return Some(Vec::new()) };
        let inspected = inspection.is_ok();
        match self.reconciling {
            Some(Reconciling::Asked(asked)) if asked == id => return Some(Vec::new()),
            Some(Reconciling::Answered(answered) | Reconciling::Refused(answered)) if answered == id => {}
            _ if helper_looks(&summary.state) => {
                self.reconciling = Some(Reconciling::Asked(id));
                self.reconcile_busy = 0;
                return Some(vec![Effect::Reconcile {
                    installation: id,
                    delay_ms: 0,
                }]);
            }
            _ => {}
        }
        let complete = self.on_disk(id).is_some_and(|x| x.state == InstallationState::Complete);
        let gone = self.gone(summary);
        // Of a finished one on a disk this Mac couldn't look at, there is
        // nothing to say: that the look failed is said instead. (One gone
        // from the disk isn't unfinished.)
        if summary.state == InstallState::WaitingForSetup && !inspected {
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
            // Its steps, or a retry, can't work on an installation that
            // isn't there.
            _ if gone => {
                self.screen = Screen::Missing;
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
        let (Some(inspection), Some(verified), Some(release), Some(place)) = (
            self.inspection().cloned(),
            self.verified().cloned(),
            self.release.clone(),
            self.place.clone(),
        ) else {
            // This look took away what the plan was for (the release no
            // longer admits this Mac, say): decide afresh what it can have.
            self.screen = Screen::Checking;
            return self.decide();
        };
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
        match derive_plan(&inspection, verified.catalog(), &request, self.to_download.map_or(0, |d| d.space)) {
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
        let mut names = vec![self.user.0.as_str()];
        names.extend(self.owner.as_deref());
        names.extend(self.owner_full_name.as_deref());
        out.iter()
            .map(|l| sekka_redact(l, &names))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The staged file for a role.
    pub fn staged(&self, role: ArtifactRole) -> Option<&Staged> {
        self.staged.iter().find(|s| s.role == role)
    }

    /// What the review says will change, in order: the plan's actions, then
    /// the startup disk, which every installation sets.
    pub fn changes(&self) -> Vec<String> {
        let Some(plan) = &self.plan else { return Vec::new() };
        let mut out: Vec<String> = plan.actions.iter().map(text::change).collect();
        out.push(text::startup_disk_change(&self.macos_volume()));
        out
    }

    /// macOS's volume as the startup options list it: the one running
    /// now, or else the one Sekka's space comes from.
    pub fn macos_volume(&self) -> String {
        let Some(i) = self.inspection() else { return text::MACOS_VOLUME.into() };
        let groups = || {
            i.disk
                .containers
                .iter()
                .flat_map(|c| c.volume_groups.iter().map(move |g| (c.uuid, g)))
        };
        let shrunk = match self.candidate() {
            Some(Candidate::ShrinkContainer { container, .. }) => Some(*container),
            _ => None,
        };
        groups()
            .find(|(_, g)| g.uuid == i.macos.volume_group)
            .or_else(|| {
                groups().find(|(c, g)| Some(*c) == shrunk && matches!(g.os, sekka_mac_contract::GroupOs::Macos { .. }))
            })
            .map(|(_, g)| g.name.to_string())
            .unwrap_or_else(|| text::MACOS_VOLUME.into())
    }

    /// What a stopped installation lets the person do.
    pub fn stopped_actions(&self) -> Vec<sekka_mac_contract::NextAction> {
        self.state.as_ref().map(|s| s.permitted().to_vec()).unwrap_or_default()
    }

    /// Whether macOS has room, beside free space, for what goes there while
    /// Sekka installs, the downloads still to come among it. It depends on
    /// the place alone, so the plan screen says so before anything is
    /// downloaded, and offers no download until it has. Ok until the place
    /// and what is left to download are known.
    pub fn macos_room(&self) -> Result<(), PlanError> {
        let (Some(inspection), Some(release), Some(candidate), Some(to_download)) =
            (self.inspection(), self.release.as_ref(), self.candidate(), self.to_download)
        else {
            return Ok(());
        };
        sekka_mac_contract::macos_room(inspection, release, candidate, to_download)
    }

    pub fn disk_changed(&self) -> Option<DiskChanged> {
        match &self.state {
            Some(InstallState::Failed { disk_changed, .. }) => Some(*disk_changed),
            _ => None,
        }
    }
}

/// Whether the helper is asked to look at the Mac for an installation
/// stopped in this state: any that stands in the way of another. It judges
/// by what lies in the space the plan gave Sekka, not by what names the
/// installation, so it can close one that never named itself on the disk,
/// and it records nothing while anything is there. One that never started a
/// stage that changes the disk is cancelled instead; one that lets another
/// begin, finished or not, it doesn't look at again.
fn helper_looks(state: &InstallState) -> bool {
    !state.allows_new_installation() && !matches!(state, InstallState::Planned | InstallState::AssetsVerified)
}

/// Whether an installation's record shows it got far enough to name itself
/// on the disk: the handoff on its boot partition is written once the ESP's
/// stage completes, and Recovery's configuration after that. Until then
/// nothing an inspection finds carries its identity, so not finding it says
/// nothing about whether it is there.
fn named_on_disk(state: &InstallState) -> bool {
    match state {
        InstallState::MutationStarted { stage } | InstallState::Failed { stage, .. } => *stage > Stage::Esp,
        InstallState::StartupDiskFailed
        | InstallState::WaitingForRecovery
        | InstallState::EnrollmentUncertain
        | InstallState::WaitingForSetup => true,
        InstallState::Planned | InstallState::AssetsVerified | InstallState::Cancelled | InstallState::Removed => false,
    }
}

/// Whether a typed account name is the recorded administrator: its short
/// name in any case, or the full name of the account running the app.
fn same_account(typed: &str, recorded: &str, user: &(String, String)) -> bool {
    let typed = typed.trim();
    typed.eq_ignore_ascii_case(recorded) || (recorded == user.0 && !user.1.is_empty() && typed == user.1)
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
