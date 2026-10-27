//! Printers and print jobs, through CUPS's command-line tools.
//!
//! CUPS's own API is IPP; its command-line tools speak it for us (over the
//! scheduler's domain socket, with its authentication) and are always
//! installed with it, so [`Printers`] runs them and parses what they print
//! (see the `parse` module for each format). Every command runs with
//! `LC_ALL=C`, stdin from `/dev/null`, in a session of its own (a tool that
//! wants a password fails instead of waiting on a terminal) and under a
//! timeout: 10 s for queries, 60 s for discovery and actions, 120 s for
//! adding a printer.
//!
//! | To know | Runs |
//! |---|---|
//! | whether CUPS is there | `lpstat -r` (`scheduler is running`) |
//! | the printers, state, message, reasons, description, location | `lpstat -l -p` |
//! | whether each accepts jobs | `lpstat -a` |
//! | device URIs | `lpstat -v` |
//! | make and model, sharing, temporary, the scheduler's default | `lpoptions -p NAME` |
//! | the default printer | `lpstat -d`; the user's own from `LPDEST`/`PRINTER` or `~/.cups/lpoptions` |
//! | jobs | `lpstat -W not-completed -l -o`; titles from `lpq -a` |
//! | finished jobs | `lpstat -W completed -l -o` |
//! | devices to add | `lpinfo -l --timeout 30 -v` and cups-filters' `driverless list` |
//! | drivers | `lpinfo -l [--device-id ID] [--make-and-model M] -m` |
//!
//! | To do | Runs |
//! |---|---|
//! | add a printer | `lpadmin -p NAME -E -v URI -m everywhere\|DRIVER` (or `-P PPD`) `[-D DESC] [-L LOC]` |
//! | remove one | `lpadmin -x NAME` |
//! | system default | `lpadmin -d NAME` |
//! | the user's default | `lpoptions -d NAME`; cleared by editing `~/.cups/lpoptions` |
//! | pause, resume | `cupsdisable NAME`, `cupsenable NAME` |
//! | accept, reject jobs | `cupsaccept NAME`, `cupsreject NAME` |
//! | share | `lpadmin -p NAME -o printer-is-shared=true\|false` |
//! | print the test page, a file | `lp -d NAME -t TITLE FILE` |
//! | cancel a job, all of a printer's | `cancel NAME-ID`, `cancel -a NAME` |
//!
//! CUPS only reports changes through IPP subscriptions, so a background
//! thread polls every 5 seconds and publishes a new snapshot only when
//! something changed; actions refresh as soon as they finish.
//!
//! Needs CUPS (`services.printing.enable` on NixOS). Changing printers
//! (adding, removing, pausing, sharing, the system default) needs CUPS's
//! administrator policy: root, or a member of the `SystemGroup` in
//! `cups-files.conf` (`wheel` and `lpadmin` on NixOS), over the local
//! socket; anyone else gets [`Error::NotAuthorized`]. Printing, cancelling
//! one's own jobs and choosing one's own default need nothing. IPP
//! Everywhere discovery uses `driverless` from cups-filters when it is
//! installed (on NixOS it is found in CUPS's own environment,
//! `/var/lib/cups/path/bin`).

mod cli;
mod parse;

pub use cli::Tools;

use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, lock, service_handle};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How often CUPS is asked what changed.
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// How often the poller checks whether its handle was dropped.
const STOP_CHECK: Duration = Duration::from_millis(250);
/// The longest printer name CUPS takes, in bytes.
const MAX_NAME: usize = 127;
/// The longest description or location CUPS keeps (IPP's `text(127)`).
const MAX_TEXT: usize = 127;

/// Everything CUPS reports: printers, their jobs, the defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrintersState {
    /// CUPS has been asked at least once; until then the rest is empty
    /// because nothing is known yet, not because there is nothing.
    pub loaded: bool,
    /// The CUPS tools are installed and the scheduler answers.
    pub available: bool,
    /// The print queues (and classes), in CUPS's order.
    pub printers: Vec<Printer>,
    /// Jobs not finished yet, on every printer, oldest first.
    pub jobs: Vec<Job>,
    /// The default printer for everyone: the scheduler's (set with
    /// `lpadmin -d`), or `/etc/cups/lpoptions`'.
    pub system_default: Option<String>,
    /// The user's own default (`lpoptions -d`, or `LPDEST`/`PRINTER`),
    /// which wins over the system's; `None` when they haven't chosen one.
    /// It may name a printer that no longer exists.
    pub user_default: Option<String>,
    /// Why the last refresh failed, if it did. The lists then keep what
    /// the refresh before found (they are cleared when CUPS is gone).
    /// Actions report their own failures through their [`Task`].
    pub error: Option<Error>,
}

impl PrintersState {
    /// The printer called `name` (CUPS names ignore case).
    pub fn printer(&self, name: &str) -> Option<&Printer> {
        self.printers.iter().find(|printer| printer.name.eq_ignore_ascii_case(name))
    }

    /// The printer jobs go to when none is named: the user's default if it
    /// exists, else the system's.
    pub fn default_printer(&self) -> Option<&Printer> {
        self.user_default
            .as_deref()
            .and_then(|name| self.printer(name))
            .or_else(|| self.system_default.as_deref().and_then(|name| self.printer(name)))
    }

    /// The jobs waiting on, or printing on, `printer`.
    pub fn jobs_on<'a>(&'a self, printer: &'a str) -> impl Iterator<Item = &'a Job> + 'a {
        self.jobs.iter().filter(move |job| job.printer.eq_ignore_ascii_case(printer))
    }

    /// A name for a new printer that no queue has yet: [`suggest_name`]'s,
    /// with `_2`, `_3`… added if it is taken.
    pub fn suggest_name(&self, make_and_model: &str) -> String {
        let base = suggest_name(make_and_model);
        let taken = |name: &str| self.printer(name).is_some();
        if !taken(&base) {
            return base;
        }
        let free = (2..u32::MAX)
            .map(|n| {
                let suffix = format!("_{n}");
                let mut name = base.clone();
                truncate(&mut name, MAX_NAME.saturating_sub(suffix.len()));
                name + &suffix
            })
            .find(|name| !taken(name));
        free.unwrap_or(base)
    }
}

/// A print queue, or a class of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Printer {
    /// The queue's name, as commands take it (`Brother_HL_L2390DW`).
    pub name: String,
    /// What people call it (`printer-info`); may be empty, see
    /// [`display_name`](Printer::display_name).
    pub description: String,
    /// Where it is (`printer-location`); may be empty.
    pub location: String,
    /// Its make and model as its driver names it (`Brother HL-L2390DW
    /// series - IPP Everywhere`), when `lpoptions` could say.
    pub make_and_model: Option<String>,
    /// How CUPS reaches it (`ipp://…`, `usb://…`, `dnssd://…`).
    pub device_uri: Option<String>,
    pub state: PrinterState,
    /// When the state last changed.
    pub state_since: Option<SystemTime>,
    /// What the printer or CUPS says about its state ("Paused", "Rendering
    /// completed", "The printer is not responding.").
    pub state_message: Option<String>,
    /// IPP `printer-state-reasons` without `none`: `paused`,
    /// `media-empty-warning`, `toner-low-report`, `offline-report`…
    pub state_reasons: Vec<String>,
    /// The job it is printing (`Name-42`).
    pub current_job: Option<String>,
    /// It takes new jobs (`cupsaccept`/`cupsreject`).
    pub accepting: bool,
    /// It prints the jobs it has (not paused or stopped): `state !=
    /// Stopped`.
    pub enabled: bool,
    /// It is shared with the network, when known.
    pub shared: Option<bool>,
    /// A class: jobs go to whichever of its printers is free.
    pub is_class: bool,
    /// A queue on another CUPS server.
    pub remote: bool,
    /// A queue CUPS made by itself for a printer found on the network,
    /// which goes away when unused.
    pub temporary: bool,
    /// It prints without a vendor driver (IPP Everywhere, AirPrint,
    /// cups-filters' driverless); `None` when nothing says.
    pub driverless: Option<bool>,
    /// It is [`PrintersState::system_default`].
    pub is_system_default: bool,
    /// It is [`PrintersState::user_default`].
    pub is_user_default: bool,
}

impl Printer {
    /// The description, or the name when there is none.
    pub fn display_name(&self) -> &str {
        if self.description.trim().is_empty() { &self.name } else { &self.description }
    }

    /// Paused or stopped: [`PrinterState::Stopped`].
    pub fn is_paused(&self) -> bool {
        self.state == PrinterState::Stopped
    }
}

/// What a printer is doing (IPP `printer-state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PrinterState {
    /// Ready, nothing to print (possibly holding new jobs).
    #[default]
    Idle,
    /// Printing a job.
    Printing,
    /// Not printing: paused with `cupsdisable` (reason `paused`) or stopped
    /// by an error; [`Printer::state_message`] says which.
    Stopped,
}

/// A print job.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Job {
    /// CUPS's id for it, `PRINTER-NUMBER` (`Office-Laser-42`), as `cancel`
    /// takes it.
    pub id: String,
    /// The number in the id, unique on the server.
    pub number: u32,
    /// The printer it is queued for.
    pub printer: String,
    /// Who printed it; CUPS shows `unknown` or a blank for other people's
    /// jobs when its privacy settings hide them.
    pub owner: String,
    /// Its size in bytes (CUPS counts kilobytes, so a multiple of 1024).
    pub size: u64,
    /// Its title, when `lpq` shows it, cut to 31 bytes; other users' are
    /// usually hidden.
    pub title: Option<String>,
    /// When it was submitted.
    pub submitted: Option<SystemTime>,
    pub state: JobState,
    /// What the printer says about it ("Spooling job, 0% completed.").
    pub status: Option<String>,
    /// IPP `job-state-reasons` without `none`: `job-printing`,
    /// `job-hold-until-specified`…
    pub reasons: Vec<String>,
}

/// Where a job is (IPP `job-state`, read from its state reasons).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum JobState {
    /// Waiting for its turn.
    #[default]
    Pending,
    /// Held until released (or until a time).
    Held,
    /// Printing.
    Processing,
    /// Was printing; the printer stopped.
    Stopped,
    /// Cancelled.
    Canceled,
    /// Given up on by CUPS (a filter or backend failed).
    Aborted,
    /// Printed.
    Completed,
}

impl JobState {
    /// Not finished: pending, held, processing or stopped.
    pub fn is_active(self) -> bool {
        matches!(self, JobState::Pending | JobState::Held | JobState::Processing | JobState::Stopped)
    }
}

/// A device a printer can be added on, from `lpinfo -v` or `driverless`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Device {
    pub class: DeviceClass,
    /// The device URI for [`NewPrinter::device_uri`]; for a backend that
    /// needs an address ([`is_manual`](Device::is_manual)), just its scheme.
    pub uri: String,
    /// A description (`Brother HL-L2390DW`, `Internet Printing Protocol
    /// (ipp)`).
    pub info: String,
    pub make_and_model: Option<String>,
    /// The IEEE 1284 device ID (`MFG:Brother;MDL:HL-L2390DW;CMD:…;`), for
    /// [`DriverFilter::device_id`].
    pub device_id: Option<String>,
    pub location: Option<String>,
    /// It can most likely be added with [`DriverChoice::Everywhere`]:
    /// `driverless` found it, or it is an IPP printer whose device ID lists
    /// PWG or Apple raster.
    pub driverless: bool,
}

impl Device {
    /// A backend without a device (`ipp`, `socket`, `lpd`, `smb`): the
    /// person types the printer's address, making a URI such as
    /// `ipp://HOST/ipp/print` or `socket://HOST:9100`.
    pub fn is_manual(&self) -> bool {
        !self.uri.contains(':')
    }

    /// The printer's make and model, for showing it and for
    /// [`suggest_name`]: [`make_and_model`](Device::make_and_model), else
    /// the device ID's manufacturer and model (`MFG`, `MDL`), else
    /// [`info`](Device::info).
    pub fn model_name(&self) -> String {
        if let Some(model) = self.make_and_model.as_deref().filter(|m| !m.trim().is_empty()) {
            return model.trim().to_owned();
        }
        let field = |keys: &[&str]| {
            let id = self.device_id.as_deref()?;
            parse::device_id_field(id, keys).filter(|value| !value.is_empty())
        };
        match (field(&["MFG", "MANUFACTURER"]), field(&["MDL", "MODEL"])) {
            // Some printers repeat the make in the model.
            (Some(make), Some(model)) if model.starts_with(make) => model.to_owned(),
            (Some(make), Some(model)) => format!("{make} {model}"),
            (None, Some(model)) => model.to_owned(),
            _ => self.info.trim().to_owned(),
        }
    }
}

/// How a device is connected, as `lpinfo` classes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeviceClass {
    /// On the network.
    #[default]
    Network,
    /// USB or parallel.
    Direct,
    /// A serial port.
    Serial,
    /// A file (a PDF printer, for instance).
    File,
    /// Something else `lpinfo` named.
    Other(String),
}

impl DeviceClass {
    fn from_lpinfo(class: &str) -> DeviceClass {
        match class.trim() {
            "network" => DeviceClass::Network,
            "direct" => DeviceClass::Direct,
            "serial" => DeviceClass::Serial,
            "file" => DeviceClass::File,
            other => DeviceClass::Other(other.to_owned()),
        }
    }
}

/// A driver CUPS can use for a new printer, from `lpinfo -m`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Driver {
    /// The name [`DriverChoice::Model`] takes (`everywhere`,
    /// `drv:///sample.drv/generic.ppd`, `gutenprint.5.3://…`).
    pub name: String,
    pub make_and_model: String,
    /// The language of its strings (`en`).
    pub language: Option<String>,
    /// The device ID it was made for, if it says.
    pub device_id: Option<String>,
}

impl Driver {
    /// CUPS's IPP Everywhere driver, which asks the printer itself.
    pub fn is_everywhere(&self) -> bool {
        self.name == "everywhere"
    }
}

/// Which drivers [`Printers::drivers`] lists; CUPS does the matching. The
/// full list can run to tens of thousands of models.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DriverFilter {
    /// Drivers for this device ID (`lpinfo --device-id`), such as a
    /// [`Device::device_id`].
    pub device_id: Option<String>,
    /// Drivers whose make and model matches (`lpinfo --make-and-model`),
    /// such as `HP LaserJet`.
    pub make_and_model: Option<String>,
    /// Drivers in this language (`lpinfo --language`), such as `en`.
    pub language: Option<String>,
}

/// A printer to add with [`Printers::add_printer`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewPrinter {
    /// The queue's name; see [`validate_name`] and [`suggest_name`].
    pub name: String,
    /// Where it is: a [`Device::uri`], or one made from a manual backend.
    pub device_uri: String,
    pub driver: DriverChoice,
    /// What people call it (up to 127 bytes).
    pub description: Option<String>,
    /// Where it is (up to 127 bytes).
    pub location: Option<String>,
}

/// The driver for a new printer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DriverChoice {
    /// CUPS's IPP Everywhere driver (`-m everywhere`), for IPP Everywhere
    /// and AirPrint printers: CUPS asks the printer what it can do.
    #[default]
    Everywhere,
    /// A driver from [`Printers::drivers`], by [`Driver::name`] (`-m`).
    Model(String),
    /// A PPD file (`-P`).
    Ppd(PathBuf),
}

/// Check a name for a new printer against CUPS's rules: 1 to 127 bytes,
/// no spaces or control characters, none of `/ \ # ? ' " @`. (CUPS also
/// takes a leading `-`, which commands would read as an option, so it is
/// refused here.)
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidArgument("A printer needs a name".into()));
    }
    if name.len() > MAX_NAME {
        return Err(Error::InvalidArgument(format!("A printer name can be at most {MAX_NAME} bytes long")));
    }
    if name.starts_with('-') {
        return Err(Error::InvalidArgument("A printer name can't start with “-”".into()));
    }
    if let Some(c) = name
        .chars()
        .find(|&c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\' | '#' | '?' | '\'' | '"' | '@'))
    {
        let what = if c.is_whitespace() {
            "spaces".to_owned()
        } else if c.is_control() {
            "control characters".to_owned()
        } else {
            format!("“{c}”")
        };
        return Err(Error::InvalidArgument(format!("A printer name can't contain {what}")));
    }
    Ok(())
}

/// A printer name made from a make and model, the way CUPS names the
/// printers it finds: `Brother HL-L2390DW series` becomes
/// `Brother_HL_L2390DW`. Letters and digits are kept (ASCII only), anything
/// else becomes one `_`; what drivers append (`- IPP Everywhere`, `,
/// driverless…`, `series`) is dropped. `Printer` if nothing is left.
pub fn suggest_name(make_and_model: &str) -> String {
    let mut model = make_and_model.trim();
    for cut in [", driverless", " - IPP Everywhere", " - CUPS+Gutenprint", " Foomatic/"] {
        if let Some((before, _)) = model.split_once(cut) {
            model = before;
        }
    }
    let model = model.trim();
    let model = match model.rsplit_once(char::is_whitespace) {
        Some((before, last)) if last.eq_ignore_ascii_case("series") => before,
        _ => model,
    };
    let mut name = String::new();
    for c in model.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c);
        } else if !name.is_empty() && !name.ends_with('_') {
            name.push('_');
        }
    }
    while name.ends_with('_') {
        name.pop();
    }
    truncate(&mut name, MAX_NAME);
    while name.ends_with('_') {
        name.pop();
    }
    if name.is_empty() { "Printer".to_owned() } else { name }
}

/// Cut `text` to at most `bytes` bytes, at a character boundary.
fn truncate(text: &mut String, bytes: usize) {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

/// A name of an existing printer, checked before it goes on a command
/// line: CUPS made it, so only what would break the command is refused.
fn check_target(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Err(Error::InvalidArgument(format!("“{name}” is not a printer name")))
    } else {
        Ok(())
    }
}

/// A job id, `PRINTER-NUMBER` (or just the number).
fn check_job(id: &str) -> Result<()> {
    let number = id.bytes().all(|b| b.is_ascii_digit()) && !id.is_empty();
    if number || (parse::split_job_id(id).is_some() && check_target(id).is_ok()) {
        Ok(())
    } else {
        Err(Error::InvalidArgument(format!("“{id}” is not a job id")))
    }
}

/// Text for a description or location: at most 127 bytes, one line.
fn check_text(what: &str, text: &str) -> Result<()> {
    if text.len() > MAX_TEXT {
        Err(Error::InvalidArgument(format!("The {what} can be at most {MAX_TEXT} bytes long")))
    } else if text.chars().any(char::is_control) {
        Err(Error::InvalidArgument(format!("The {what} must be a single line")))
    } else {
        Ok(())
    }
}

/// The `lpadmin` arguments that add `printer`.
fn add_args(printer: &NewPrinter) -> Result<Vec<OsString>> {
    validate_name(&printer.name)?;
    let uri = printer.device_uri.trim();
    if uri.is_empty() || uri.starts_with('-') || uri.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Error::InvalidArgument(format!("“{uri}” is not a device URI")));
    }
    let mut args: Vec<OsString> =
        ["-p", printer.name.as_str(), "-E", "-v", uri].into_iter().map(OsString::from).collect();
    match &printer.driver {
        DriverChoice::Everywhere => args.extend(["-m", "everywhere"].map(OsString::from)),
        DriverChoice::Model(model) => {
            if model.is_empty() || model.starts_with('-') || model.chars().any(char::is_control) {
                return Err(Error::InvalidArgument(format!("“{model}” is not a driver")));
            }
            args.extend(["-m", model.as_str()].map(OsString::from));
        }
        DriverChoice::Ppd(path) => {
            let path = path.canonicalize().map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => Error::NotFound(format!("{}", path.display())),
                _ => Error::from(error),
            })?;
            if !path.is_file() {
                return Err(Error::InvalidArgument(format!("{} is not a PPD file", path.display())));
            }
            args.extend([OsString::from("-P"), path.into_os_string()]);
        }
    }
    if let Some(description) = printer.description.as_deref().filter(|d| !d.is_empty()) {
        check_text("description", description)?;
        args.extend(["-D", description].map(OsString::from));
    }
    if let Some(location) = printer.location.as_deref().filter(|l| !l.is_empty()) {
        check_text("location", location)?;
        args.extend(["-L", location].map(OsString::from));
    }
    Ok(args)
}

/// CUPS's printers and jobs, kept current by polling.
///
/// Built with a [`Wake`], it starts a thread that asks CUPS what changed
/// every 5 seconds (until the handle is dropped) and wakes the UI when the
/// [`PrintersState`] snapshot changes. Actions return a [`Task`] at once,
/// run their command on a thread, refresh the snapshot, and then finish.
pub struct Printers {
    model: Arc<Model<PrintersState>>,
    shared: Arc<Shared>,
    stop: Stop,
}

/// What the poller and the action threads share.
struct Shared {
    tools: Tools,
    /// Held while a refresh queries CUPS and publishes, so refreshes take
    /// turns and the one published last read CUPS last.
    refreshing: Mutex<()>,
}

service_handle!(Printers, PrintersState);

impl Printers {
    /// Follow CUPS through the tools in `PATH`.
    pub fn new(wake: Wake) -> Printers {
        Printers::with_tools(Tools::default(), wake)
    }

    /// Follow CUPS through `tools`.
    pub fn with_tools(tools: Tools, wake: Wake) -> Printers {
        Printers::with_interval(tools, wake, POLL_INTERVAL)
    }

    fn with_interval(tools: Tools, wake: Wake, interval: Duration) -> Printers {
        let printers = Printers {
            model: Arc::new(Model::new(PrintersState::default(), wake)),
            shared: Arc::new(Shared { tools, refreshing: Mutex::new(()) }),
            stop: Stop::default(),
        };
        printers.poll(interval);
        printers
    }

    fn poll(&self, interval: Duration) {
        let model = self.model.clone();
        let shared = self.shared.clone();
        let stop = self.stop.clone();
        let spawned = std::thread::Builder::new().name("aizome-sys printers".into()).spawn(move || {
            while !stop.stopped() {
                let _ = refresh(&shared, &model, Some(&stop));
                let mut waited = Duration::ZERO;
                while waited < interval {
                    if stop.stopped() {
                        return;
                    }
                    let step = STOP_CHECK.min(interval.saturating_sub(waited));
                    std::thread::sleep(step);
                    waited += step;
                }
            }
        });
        if let Err(error) = spawned {
            self.model.update(|state| {
                state.loaded = true;
                state.error = Some(Error::Failed(format!("could not start following CUPS: {error}")));
            });
        }
    }

    /// Whether CUPS is there: its tools are installed and the scheduler
    /// answers. False until the first query finishes
    /// ([`PrintersState::loaded`]).
    pub fn available(&self) -> bool {
        self.model.get().available
    }

    /// The tools this handle runs.
    pub fn tools(&self) -> &Tools {
        &self.shared.tools
    }

    /// Ask CUPS now instead of at the next poll.
    pub fn refresh(&self) -> Task<()> {
        let shared = self.shared.clone();
        let model = self.model.clone();
        Task::spawn("printers refresh", &self.model.wake(), move || refresh(&shared, &model, None))
    }

    /// Run `work` with the tools on a thread, then refresh the snapshot.
    fn act<T: Send + 'static>(&self, name: &str, work: impl FnOnce(&Tools) -> Result<T> + Send + 'static) -> Task<T> {
        let shared = self.shared.clone();
        let model = self.model.clone();
        Task::spawn(name, &self.model.wake(), move || {
            let result = work(&shared.tools);
            let _ = refresh(&shared, &model, None);
            result
        })
    }

    /// Run a tool with `args` as an action.
    fn command(&self, name: &str, tool: fn(&Tools) -> &Path, args: Vec<OsString>, timeout: Duration) -> Task<()> {
        self.act(name, move |tools| cli::act(tool(tools), &args, timeout))
    }

    /// Run a tool on the printer `printer` (`TOOL [ARGS…] PRINTER`).
    fn on_printer(&self, name: &str, tool: fn(&Tools) -> &Path, args: &[&str], printer: &str) -> Task<()> {
        if let Err(error) = check_target(printer) {
            return Task::ready(Err(error));
        }
        let args = args.iter().copied().chain([printer]).map(OsString::from).collect();
        self.command(name, tool, args, cli::ACTION_TIMEOUT)
    }

    /// Devices a printer could be added on: what CUPS's backends find
    /// (`lpinfo -v`: USB, network printers announced over DNS-SD, the
    /// manual backends) and IPP Everywhere printers `driverless` finds. Can
    /// take up to a minute.
    pub fn discover(&self) -> Task<Vec<Device>> {
        let shared = self.shared.clone();
        Task::spawn("printers discover", &self.model.wake(), move || cli::discover(&shared.tools))
    }

    /// The drivers CUPS has for `filter` (`lpinfo -m`).
    pub fn drivers(&self, filter: DriverFilter) -> Task<Vec<Driver>> {
        let shared = self.shared.clone();
        Task::spawn("printers drivers", &self.model.wake(), move || cli::drivers(&shared.tools, &filter))
    }

    /// Jobs CUPS still remembers that are done (printed, cancelled,
    /// aborted).
    pub fn completed_jobs(&self) -> Task<Vec<Job>> {
        let shared = self.shared.clone();
        Task::spawn("printers completed jobs", &self.model.wake(), move || cli::completed_jobs(&shared.tools))
    }

    /// Add a printer, enabled and accepting jobs.
    pub fn add_printer(&self, printer: NewPrinter) -> Task<()> {
        match add_args(&printer) {
            Ok(args) => self.command("printers add", |t| &t.lpadmin, args, cli::ADD_TIMEOUT),
            Err(error) => Task::ready(Err(error)),
        }
    }

    /// Remove a printer and its jobs.
    pub fn remove_printer(&self, printer: &str) -> Task<()> {
        self.on_printer("printers remove", |t| &t.lpadmin, &["-x"], printer)
    }

    /// Make `printer` the default for everyone (`lpadmin -d`).
    pub fn set_system_default(&self, printer: &str) -> Task<()> {
        self.on_printer("printers default", |t| &t.lpadmin, &["-d"], printer)
    }

    /// Make `printer` this user's default (`lpoptions -d`), over the
    /// system's.
    pub fn set_user_default(&self, printer: &str) -> Task<()> {
        self.on_printer("printers user default", |t| &t.lpoptions, &["-d"], printer)
    }

    /// Forget this user's default, so the system's applies again (their
    /// saved options for each printer are kept). `LPDEST` or `PRINTER` in
    /// the environment still win.
    pub fn clear_user_default(&self) -> Task<()> {
        self.act("printers user default", cli::clear_user_default)
    }

    /// Stop printing on `printer`; jobs wait (`cupsdisable`).
    pub fn pause(&self, printer: &str) -> Task<()> {
        self.on_printer("printers pause", |t| &t.cupsdisable, &[], printer)
    }

    /// Print again on `printer` (`cupsenable`).
    pub fn resume(&self, printer: &str) -> Task<()> {
        self.on_printer("printers resume", |t| &t.cupsenable, &[], printer)
    }

    /// Accept new jobs on `printer` (`cupsaccept`), or refuse them
    /// (`cupsreject`).
    pub fn set_accepting(&self, printer: &str, accepting: bool) -> Task<()> {
        if accepting {
            self.on_printer("printers accept", |t| &t.cupsaccept, &[], printer)
        } else {
            self.on_printer("printers reject", |t| &t.cupsreject, &[], printer)
        }
    }

    /// Share `printer` with the network, or stop sharing it. (CUPS also
    /// needs sharing on as a whole, `cupsctl --share-printers`.)
    pub fn set_shared(&self, printer: &str, shared: bool) -> Task<()> {
        if let Err(error) = check_target(printer) {
            return Task::ready(Err(error));
        }
        let option = format!("printer-is-shared={shared}");
        let args = ["-p", printer, "-o", option.as_str()].map(OsString::from).to_vec();
        self.command("printers share", |t| &t.lpadmin, args, cli::ACTION_TIMEOUT)
    }

    /// Print CUPS's test page on `printer`; the job's id (`Name-42`).
    pub fn print_test_page(&self, printer: &str) -> Task<String> {
        if let Err(error) = check_target(printer) {
            return Task::ready(Err(error));
        }
        let printer = printer.to_owned();
        self.act("printers test page", move |tools| {
            let page =
                tools.find_test_page().ok_or_else(|| Error::NotFound("CUPS's test page (data/testprint)".into()))?;
            cli::print(tools, &printer, &page, Some("Test page"))
        })
    }

    /// Print `file` on `printer`, titled `title` (CUPS uses the file's
    /// name otherwise); the job's id.
    pub fn print_file(&self, printer: &str, file: impl AsRef<Path>, title: Option<&str>) -> Task<String> {
        if let Err(error) = check_target(printer) {
            return Task::ready(Err(error));
        }
        // An absolute path can't be mistaken for an option.
        let file = match std::path::absolute(file.as_ref()) {
            Ok(file) if file.is_file() => file,
            Ok(file) if !file.exists() => {
                return Task::ready(Err(Error::NotFound(file.display().to_string())));
            }
            Ok(file) => {
                return Task::ready(Err(Error::InvalidArgument(format!("{} is not a file", file.display()))));
            }
            Err(error) => return Task::ready(Err(error.into())),
        };
        let printer = printer.to_owned();
        let title = title.map(str::to_owned);
        self.act("printers print", move |tools| cli::print(tools, &printer, &file, title.as_deref()))
    }

    /// Cancel a job by its id (`Name-42`).
    pub fn cancel_job(&self, job: &str) -> Task<()> {
        if let Err(error) = check_job(job) {
            return Task::ready(Err(error));
        }
        self.command("printers cancel", |t| &t.cancel, vec![OsString::from(job)], cli::ACTION_TIMEOUT)
    }

    /// Cancel every job on `printer` (`cancel -a`).
    pub fn cancel_all_jobs(&self, printer: &str) -> Task<()> {
        self.on_printer("printers cancel all", |t| &t.cancel, &["-a"], printer)
    }
}

impl Drop for Printers {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// Query CUPS and publish what changed. Refreshes take turns, so a slow
/// poll can't publish over a newer refresh's state.
fn refresh(shared: &Shared, model: &Model<PrintersState>, stop: Option<&Stop>) -> Result<()> {
    let _turn = lock(&shared.refreshing);
    let result = cli::query(&shared.tools, stop);
    if stop.is_some_and(|stop| stop.stopped()) {
        return result.map(|_| ());
    }
    match result {
        Ok(state) => {
            model.set_if_changed(state);
            Ok(())
        }
        Err(error) => {
            let previous = model.get();
            if previous.error.as_ref() != Some(&error) {
                log::debug!("aizome-sys: printers: {error}");
            }
            let state = match &error {
                // CUPS is gone: what it said before no longer holds.
                Error::Unavailable(_) => PrintersState { loaded: true, ..PrintersState::default() },
                _ => PrintersState { loaded: true, ..PrintersState::clone(&previous) },
            };
            model.set_if_changed(PrintersState { error: Some(error.clone()), ..state });
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
