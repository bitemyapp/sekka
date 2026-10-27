//! What CUPS's command-line tools print, turned into data.
//!
//! Every function here takes the text a tool printed (run with `LC_ALL=C`,
//! so messages are untranslated and dates are in the C locale's `%c` form,
//! `Sat Oct 17 15:10:00 2026`) and returns plain values; nothing here runs
//! a command. Lines that don't have the expected shape are skipped: a newer
//! CUPS that words something differently loses that detail, never the rest
//! of the list, and never panics.
//!
//! The formats, from CUPS 2.4's `lpstat.c`, `lpq.c`, `lpoptions.c` and
//! `lpinfo.c`:
//!
//! - `lpstat -l -p`: a `printer NAME is idle.  enabled since DATE` line
//!   (or `now printing NAME-ID.`, `is holding new jobs.`, `disabled since
//!   DATE -`), then tab-indented details: the state message right after it
//!   (`reason unknown` when a stopped printer has none), `Description:`,
//!   `Alerts:` (the state reasons), `Location:`, `Connection:` and more.
//! - `lpstat -a`: `NAME accepting requests since DATE` or `NAME not
//!   accepting requests since DATE -` and the reason on the next line.
//! - `lpstat -v`: `device for NAME: URI`.
//! - `lpstat -d`: `system default destination: NAME[/INSTANCE]` or `no
//!   system default destination`.
//! - `lpstat -l -o`: `NAME-ID OWNER SIZE DATE` (the size in bytes), then
//!   `Status:`, `Alerts:` (job state reasons) and `queued for NAME`.
//! - `lpq -a`: `RANK OWNER ID TITLE SIZE bytes`, the owner cut to 7 bytes
//!   and the title to 31.
//! - `lpoptions -p NAME`: one line of `name=value` words, values with
//!   spaces in single quotes, and bare names for empty values.
//! - `lpinfo -v`: `CLASS URI`; with `-l`, `Device: uri = URI` and indented
//!   `key = value` lines.
//! - `lpinfo -m`: `NAME MAKE-AND-MODEL`; with `-l`, `Model:  name = NAME`
//!   and indented `key = value` lines.
//! - `driverless list`: `"driverless:URI" LANG "MAKE" "MAKE AND MODEL"
//!   "DEVICE-ID"`; plain `driverless` prints bare URIs.
//! - `lp`: `request id is NAME-ID (1 file(s))`.

use super::{Device, DeviceClass, Driver, Job, JobState, PrinterState};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Bits of CUPS's `printer-type` (`cups_ptype_t`) that the module reads.
pub(crate) mod ptype {
    /// A class of printers rather than one queue.
    pub(crate) const CLASS: u32 = 0x0001;
    /// A queue on another CUPS server.
    pub(crate) const REMOTE: u32 = 0x0002;
    /// The scheduler's default destination.
    pub(crate) const DEFAULT: u32 = 0x0002_0000;
    /// Not shared with the network.
    pub(crate) const NOT_SHARED: u32 = 0x0020_0000;
    /// Found on the network (a temporary queue CUPS made by itself).
    pub(crate) const DISCOVERED: u32 = 0x0100_0000;
}

/// How long `lpq` keeps of a job's title, in bytes (`%-31.31s`).
const LPQ_TITLE_WIDTH: usize = 31;

/// One printer as `lpstat -p` (with `-l`) describes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StatusEntry {
    pub(crate) name: String,
    pub(crate) state: PrinterState,
    pub(crate) since: Option<SystemTime>,
    /// The job it is printing, from `now printing NAME-ID.`.
    pub(crate) current_job: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) location: Option<String>,
    pub(crate) reasons: Vec<String>,
    /// `Connection: remote`.
    pub(crate) remote: bool,
}

/// What `lpstat -r` says: `Some(true)` for `scheduler is running`.
pub(crate) fn scheduler_running(text: &str) -> Option<bool> {
    if text.contains("scheduler is not running") || text.contains("Scheduler is not running") {
        Some(false)
    } else if text.contains("scheduler is running") {
        Some(true)
    } else {
        None
    }
}

/// The printers in `lpstat -p` or `lpstat -l -p` output, in order.
/// Instances (`NAME/INSTANCE`, the user's saved option sets) are skipped:
/// they are the same queue.
pub(crate) fn printer_status(text: &str) -> Vec<StatusEntry> {
    let mut printers = Vec::new();
    let mut current: Option<StatusEntry> = None;
    let mut first_detail = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("printer ") {
            printers.extend(current.take());
            current = status_line(rest);
            first_detail = true;
            continue;
        }
        if !line.starts_with([' ', '\t']) {
            // Anything else at the margin (an error, an empty line) ends the
            // printer's details.
            printers.extend(current.take());
            continue;
        }
        let Some(entry) = current.as_mut() else { continue };
        let is_first = std::mem::replace(&mut first_detail, false);
        if line.starts_with("\t\t") {
            // The lists under "Users allowed:" and the like.
            continue;
        }
        let detail = line.trim();
        if is_first && !is_status_key(detail) {
            // The state message comes right after the state line, before
            // any of the keyed details.
            if !detail.is_empty() && detail != "reason unknown" {
                entry.message = Some(detail.to_owned());
            }
        } else if let Some(value) = detail.strip_prefix("Description:") {
            entry.description = Some(value.trim().to_owned());
        } else if let Some(value) = detail.strip_prefix("Location:") {
            entry.location = Some(value.trim().to_owned());
        } else if let Some(value) = detail.strip_prefix("Alerts:") {
            entry.reasons = words(value);
        } else if let Some(value) = detail.strip_prefix("Connection:") {
            entry.remote = value.trim() == "remote";
        }
    }
    printers.extend(current);
    printers
}

/// The keyed details `lpstat -l -p` prints under a printer.
fn is_status_key(detail: &str) -> bool {
    const KEYS: [&str; 18] = [
        "Form mounted:",
        "Content types:",
        "Printer types:",
        "Description:",
        "Alerts:",
        "Location:",
        "Connection:",
        "Interface:",
        "On fault:",
        "After fault:",
        "Users allowed:",
        "Users denied:",
        "Forms allowed:",
        "Banner required",
        "Charset sets:",
        "Default pitch:",
        "Default page size:",
        "Default port settings:",
    ];
    KEYS.iter().any(|key| detail.starts_with(key))
}

/// `NAME is idle.  enabled since DATE` and the other state lines, without
/// the leading `printer `.
fn status_line(rest: &str) -> Option<StatusEntry> {
    let (name, tail) = rest.split_once(' ')?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let mut entry = StatusEntry { name: name.to_owned(), ..StatusEntry::default() };
    if let Some(after) = tail.strip_prefix("now printing ") {
        entry.state = PrinterState::Printing;
        entry.current_job = after
            .split_whitespace()
            .next()
            .map(|job| job.trim_end_matches('.'))
            .filter(|job| split_job_id(job).is_some())
            .map(str::to_owned);
    } else if tail.starts_with("disabled") {
        entry.state = PrinterState::Stopped;
    } else {
        // "is idle.", "is holding new jobs." and whatever a newer CUPS says
        // about an enabled printer.
        entry.state = PrinterState::Idle;
    }
    entry.since = tail.split_once("since ").and_then(|(_, date)| c_date(date));
    Some(entry)
}

/// Whether each printer accepts jobs, from `lpstat -a`.
pub(crate) fn accepting(text: &str) -> HashMap<String, bool> {
    let mut accepting = HashMap::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((name, rest)) = line.split_once(' ') else { continue };
        if name.is_empty() || name.contains('/') {
            continue;
        }
        if rest.starts_with("accepting requests") {
            accepting.insert(name.to_owned(), true);
        } else if rest.starts_with("not accepting requests") {
            accepting.insert(name.to_owned(), false);
        }
    }
    accepting
}

/// Each printer's device URI, from `lpstat -v`. Printer names can contain
/// `:` but never a space, so the first `": "` ends the name.
pub(crate) fn device_uris(text: &str) -> HashMap<String, String> {
    let mut devices = HashMap::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("device for ") else { continue };
        let Some((name, uri)) = rest.split_once(": ") else { continue };
        let uri = uri.trim();
        if name.is_empty() || name.contains('/') || uri.is_empty() {
            continue;
        }
        devices.insert(name.to_owned(), uri.to_owned());
    }
    devices
}

/// The default destination `lpstat -d` names, without its instance.
pub(crate) fn default_destination(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("system default destination:")?;
        printer_part(rest.trim())
    })
}

/// `NAME` from `NAME/INSTANCE`, if not empty.
fn printer_part(destination: &str) -> Option<String> {
    let name = destination.split('/').next().unwrap_or_default().trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Split a job id, `NAME-ID`, into its printer and number. Printer names
/// may contain dashes, so the number is after the last one.
pub(crate) fn split_job_id(id: &str) -> Option<(&str, u32)> {
    let (printer, number) = id.rsplit_once('-')?;
    if printer.is_empty() || !is_digits(number) {
        return None;
    }
    Some((printer, number.parse().ok()?))
}

fn is_digits(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit())
}

/// The jobs in `lpstat -o` output (with or without `-l`). Their state is
/// [`JobState::Pending`] until [`job_state`] sorts them, which needs to
/// know which jobs the printers are printing.
pub(crate) fn jobs(text: &str) -> Vec<Job> {
    let mut jobs = Vec::new();
    let mut current: Option<Job> = None;
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            let Some(job) = current.as_mut() else { continue };
            let detail = line.trim();
            if let Some(value) = detail.strip_prefix("Status:") {
                let value = value.trim();
                job.status = (!value.is_empty()).then(|| value.to_owned());
            } else if let Some(value) = detail.strip_prefix("Alerts:") {
                job.reasons = words(value);
            } else if let Some(printer) = detail.strip_prefix("queued for ").and_then(printer_part) {
                job.printer = printer;
            }
            continue;
        }
        jobs.extend(current.take());
        current = job_line(line);
    }
    jobs.extend(current);
    jobs
}

/// `NAME-ID OWNER SIZE DATE`. The owner may contain spaces (a name from
/// another system), so the size and date are found from the end: the date
/// is the last five words, the size the word before.
fn job_line(line: &str) -> Option<Job> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let (&id, rest) = words.split_first()?;
    let (printer, number) = split_job_id(id)?;

    let from_end = rest.len().checked_sub(6).and_then(|cut| {
        let (owner, tail) = rest.split_at_checked(cut)?;
        let (size, date) = tail.split_first()?;
        let submitted = c_date_words(date)?;
        (!owner.is_empty() && is_digits(size)).then_some((owner, *size, Some(submitted)))
    });
    let (owner, size, submitted) = match from_end {
        Some(found) => found,
        None => {
            // No date we can read: the size is the first number after the
            // owner's first word.
            let at = rest.iter().skip(1).position(|word| is_digits(word))? + 1;
            let (owner, tail) = rest.split_at_checked(at)?;
            let (size, date) = tail.split_first()?;
            (owner, *size, c_date_words(date))
        }
    };
    Some(Job {
        id: id.to_owned(),
        number,
        printer: printer.to_owned(),
        owner: owner.join(" "),
        size: size.parse().unwrap_or(0),
        submitted,
        ..Job::default()
    })
}

/// A job's state from its state reasons (`lpstat -l -o`'s `Alerts:`),
/// whether a printer says it is printing it, and whether it came from the
/// list of completed jobs.
pub(crate) fn job_state(reasons: &[String], printing: bool, completed: bool) -> JobState {
    let has = |part: &str| reasons.iter().any(|reason| reason.contains(part));
    if completed {
        if has("canceled") {
            JobState::Canceled
        } else if has("aborted") {
            JobState::Aborted
        } else {
            JobState::Completed
        }
    } else if printing
        || has("job-printing")
        || has("job-transforming")
        || has("job-outgoing")
        || has("processing-to-stop-point")
    {
        JobState::Processing
    } else if has("job-stopped") {
        JobState::Stopped
    } else if has("job-hold") || has("held") || has("job-password-wait") {
        JobState::Held
    } else {
        JobState::Pending
    }
}

/// Job titles by job number, from `lpq -a`'s raw output. `lpq` cuts titles
/// to 31 bytes, so a long one comes back shortened (and a character cut in
/// half dropped). The columns are counted in bytes, so this reads bytes:
/// decoding first would turn a cut character into a longer replacement
/// character and shift the columns.
pub(crate) fn lpq_titles(output: &[u8]) -> HashMap<u32, String> {
    let mut titles = HashMap::new();
    for line in output.split(|&b| b == b'\n') {
        let Some(head) = line.trim_ascii_end().strip_suffix(b" bytes") else { continue };
        let Some(space) = head.iter().rposition(|&b| b == b' ') else { continue };
        let Some((head, size)) = head.split_at_checked(space) else { continue };
        let size = size.get(1..).unwrap_or_default();
        if size.is_empty() || !size.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Some(cut) = head.len().checked_sub(LPQ_TITLE_WIDTH) else { continue };
        let Some((front, title)) = head.split_at_checked(cut) else { continue };
        // `%-7s %-7.7s %-7d ` comes before the title: rank, owner, number.
        if !front.ends_with(b" ") {
            continue;
        }
        let front = String::from_utf8_lossy(front);
        let Some(number) = front.split_whitespace().last().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let title = String::from_utf8_lossy(title);
        let title = title.trim_end().trim_end_matches('\u{fffd}').trim();
        if !title.is_empty() {
            titles.insert(number, title.to_owned());
        }
    }
    titles
}

/// The options `lpoptions -p NAME` prints: `name=value`, `name='a value'`
/// (quoted when it has spaces) and bare `name` for an empty value. A quote
/// inside a quoted value (`'Bob's printer'`) only ends it when a space or
/// the end follows.
pub(crate) fn options(text: &str) -> HashMap<String, String> {
    let mut options = HashMap::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            break;
        }
        let mut name = String::new();
        while let Some(c) = chars.next_if(|c| !c.is_whitespace() && *c != '=') {
            name.push(c);
        }
        let mut value = String::new();
        if chars.next_if_eq(&'=').is_some() {
            match chars.peek().copied() {
                Some(quote @ ('\'' | '"')) => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == quote && chars.peek().is_none_or(|next| next.is_whitespace()) {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                        if c == '\\' {
                            value.extend(chars.next());
                        } else {
                            value.push(c);
                        }
                    }
                }
            }
        }
        if !name.is_empty() {
            options.insert(name, value);
        }
    }
    options
}

/// The default destination an lpoptions file (`~/.cups/lpoptions`) names:
/// its first `Default NAME[/INSTANCE] [options]` line, as CUPS reads it.
pub(crate) fn lpoptions_default(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let keyword = words.next()?;
        if !keyword.eq_ignore_ascii_case("default") {
            return None;
        }
        printer_part(words.next()?)
    })
}

/// An lpoptions file without a default: each `Default NAME options` line
/// becomes `Dest NAME options`, keeping the printer's saved options.
pub(crate) fn without_default(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let keyword = trimmed.split_whitespace().next().unwrap_or_default();
        if keyword.eq_ignore_ascii_case("default") {
            let indent = line.strip_suffix(trimmed).unwrap_or_default();
            out.push_str(indent);
            out.push_str("Dest");
            out.push_str(trimmed.get(keyword.len()..).unwrap_or_default());
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The devices `lpinfo -v` or `lpinfo -l -v` lists.
pub(crate) fn devices(text: &str) -> Vec<Device> {
    let mut devices = Vec::new();
    let mut current: Option<Device> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Device:") {
            devices.extend(current.take());
            let uri = rest.trim().strip_prefix("uri").and_then(|r| r.trim_start().strip_prefix('='));
            current = uri
                .map(str::trim)
                .filter(|uri| !uri.is_empty())
                .map(|uri| Device { uri: uri.to_owned(), ..Device::default() });
        } else if line.starts_with([' ', '\t']) {
            let Some(device) = current.as_mut() else { continue };
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "class" => device.class = DeviceClass::from_lpinfo(value),
                "info" => device.info = value.to_owned(),
                "make-and-model" => device.make_and_model = known(value),
                "device-id" => device.device_id = known(value),
                "location" => device.location = known(value),
                _ => {}
            }
        } else {
            devices.extend(current.take());
            let Some((class, uri)) = line.trim().split_once(char::is_whitespace) else { continue };
            let uri = uri.trim();
            if uri.is_empty() {
                continue;
            }
            devices.push(Device { class: DeviceClass::from_lpinfo(class), uri: uri.to_owned(), ..Device::default() });
        }
    }
    devices.extend(current);
    devices
}

/// `None` for the empty and `Unknown` values lpinfo prints for what a
/// backend doesn't know.
fn known(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "Unknown").then(|| value.to_owned())
}

/// The drivers `lpinfo -m` or `lpinfo -l -m` lists, parsed as they are
/// read: the list can run to tens of thousands of models.
pub(crate) fn drivers(text: &str) -> impl Iterator<Item = Driver> + '_ {
    DriverLines { lines: text.lines(), pending: None }
}

struct DriverLines<'a> {
    lines: std::str::Lines<'a>,
    /// The model whose details are being read; `None` before the first and
    /// after a `Model:` line without a name, whose details are skipped.
    pending: Option<Driver>,
}

impl Iterator for DriverLines<'_> {
    type Item = Driver;

    fn next(&mut self) -> Option<Driver> {
        for line in self.lines.by_ref() {
            let started = if let Some(rest) = line.strip_prefix("Model:") {
                let name = rest.trim().strip_prefix("name").and_then(|r| r.trim_start().strip_prefix('='));
                match name.map(str::trim).filter(|name| !name.is_empty()) {
                    Some(name) => Driver { name: name.to_owned(), ..Driver::default() },
                    None => match self.pending.take() {
                        Some(done) => return Some(done),
                        None => continue,
                    },
                }
            } else if line.starts_with([' ', '\t']) {
                let Some(driver) = self.pending.as_mut() else { continue };
                let Some((key, value)) = line.split_once('=') else { continue };
                match key.trim() {
                    "natural_language" => driver.language = known(value),
                    "make-and-model" => driver.make_and_model = value.trim().to_owned(),
                    "device-id" => driver.device_id = known(value),
                    _ => {}
                }
                continue;
            } else {
                let Some((name, model)) = line.trim().split_once(char::is_whitespace) else {
                    continue;
                };
                Driver { name: name.to_owned(), make_and_model: model.trim().to_owned(), ..Driver::default() }
            };
            if let Some(done) = self.pending.replace(started) {
                return Some(done);
            }
        }
        self.pending.take()
    }
}

/// The printers `driverless list` (or plain `driverless`) found: their
/// printer URI with what `list` says about them. Debug chatter and other
/// lines are skipped.
pub(crate) fn driverless(text: &str) -> Vec<Device> {
    let mut devices: Vec<Device> = Vec::new();
    for line in text.lines() {
        let fields = quoted_fields(line);
        let Some(first) = fields.first() else { continue };
        let uri = first.strip_prefix("driverless:").unwrap_or(first);
        if !uri.contains("://") || first.starts_with("driverless-fax:") {
            continue;
        }
        if devices.iter().any(|device| device.uri == uri) {
            continue;
        }
        // "driverless:URI" LANG "MAKE" "MAKE AND MODEL" "DEVICE-ID"
        let make_and_model = fields.get(3).and_then(|model| known(model));
        let info = make_and_model
            .as_deref()
            .map(|model| model.split(", driverless").next().unwrap_or(model).to_owned())
            .or_else(|| service_name(uri))
            .unwrap_or_else(|| uri.to_owned());
        devices.push(Device {
            class: DeviceClass::Network,
            uri: uri.to_owned(),
            info,
            make_and_model,
            device_id: fields.get(4).and_then(|id| known(id)),
            location: None,
            driverless: true,
        });
    }
    devices
}

/// A line's words, where a double-quoted string is one word.
fn quoted_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(first) = chars.next() else { break };
        let mut field = String::new();
        if first == '"' {
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                field.push(c);
            }
        } else {
            field.push(first);
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                field.push(c);
            }
        }
        fields.push(field);
    }
    fields
}

/// The job id in `lp`'s `request id is NAME-ID (1 file(s))`.
pub(crate) fn request_id(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (_, rest) = line.split_once("request id is ")?;
        let id = rest.split_whitespace().next()?;
        split_job_id(id).map(|_| id.to_owned())
    })
}

/// The DNS-SD service name in a URI such as
/// `dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=…` or
/// `ipp://Brother%20HL-L2390DW._ipp._tcp.local/`: `Brother HL-L2390DW`.
pub(crate) fn service_name(uri: &str) -> Option<String> {
    let (_, rest) = uri.split_once("://")?;
    let host = rest.split(['/', '?']).next()?;
    let host = percent_decode(host);
    let (service, _) = host.split_once("._tcp").or_else(|| host.split_once("._udp"))?;
    let (name, _) = service.rsplit_once("._")?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// `%XX` escapes decoded (as UTF-8, lossily); anything else kept.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%'
            && let Some(hex) = bytes.get(i + 1..i + 3)
            && let Ok(hex) = std::str::from_utf8(hex)
            && let Ok(decoded) = u8::from_str_radix(hex, 16)
        {
            out.push(decoded);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A field of an IEEE 1284 device ID (`MFG:Brother;MDL:HL-L2390DW;CMD:…;`),
/// by any of its names (`CMD` is also `COMMAND SET`).
pub(crate) fn device_id_field<'a>(device_id: &'a str, keys: &[&str]) -> Option<&'a str> {
    device_id.split(';').find_map(|field| {
        let (key, value) = field.split_once(':')?;
        let key = key.trim();
        keys.iter().any(|k| key.eq_ignore_ascii_case(k)).then(|| value.trim())
    })
}

/// Whether a device ID's command set has a raster format CUPS's IPP
/// Everywhere driver prints (PWG raster, or Apple raster/URF).
pub(crate) fn speaks_driverless(device_id: &str) -> bool {
    device_id_field(device_id, &["CMD", "COMMAND SET"]).is_some_and(|set| {
        set.split(',').any(|lang| {
            let lang = lang.trim();
            ["URF", "PWG", "PWGRaster", "AppleRaster"].iter().any(|l| lang.eq_ignore_ascii_case(l))
        })
    })
}

/// A setting in a CUPS configuration file such as `cups-files.conf`
/// (`DataDir /usr/share/cups`).
pub(crate) fn conf_path(text: &str, key: &str) -> Option<PathBuf> {
    text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once(char::is_whitespace)?;
        let value = value.trim();
        (name.eq_ignore_ascii_case(key) && !value.is_empty()).then(|| PathBuf::from(value))
    })
}

/// Words separated by spaces or commas, without `none`: state reasons.
pub(crate) fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|word| !word.is_empty() && *word != "none")
        .map(str::to_owned)
        .collect()
}

/// A date in the C locale's `%c` form (`Sat Oct 17 15:10:00 2026`) at the
/// start of `text`, read in the local time zone as CUPS wrote it.
pub(crate) fn c_date(text: &str) -> Option<SystemTime> {
    let words: Vec<&str> = text.split_whitespace().take(5).collect();
    c_date_words(&words)
}

fn c_date_words(words: &[&str]) -> Option<SystemTime> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let [weekday, month, day, time, year] = words else { return None };
    if !DAYS.contains(weekday) {
        return None;
    }
    let month = MONTHS.iter().position(|m| m == month)?;
    let day: i32 = day.parse().ok().filter(|d| (1..=31).contains(d))?;
    let mut clock = time.split(':').map(|part| part.parse::<i32>().ok());
    let (Some(Some(hour)), Some(Some(minute)), Some(Some(second)), None) =
        (clock.next(), clock.next(), clock.next(), clock.next())
    else {
        return None;
    };
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    let year: i32 = year.parse().ok().filter(|y| (1970..=9999).contains(y))?;

    // SAFETY: an all-zero `tm` is valid (the time-zone name pointer is
    // null); mktime only reads and normalises the struct it is given.
    let seconds = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = year - 1900;
        tm.tm_mon = i32::try_from(month).ok()?;
        tm.tm_mday = day;
        tm.tm_hour = hour;
        tm.tm_min = minute;
        tm.tm_sec = second;
        tm.tm_isdst = -1;
        libc::mktime(&mut tm)
    };
    let seconds = u64::try_from(seconds).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `lpstat -l -p` as CUPS 2.4 prints it: an idle IPP Everywhere queue,
    /// one printing with a message and a unicode description, one paused,
    /// one stopped without a message, one holding new jobs, a remote one,
    /// an instance and a stray error line.
    pub(crate) const LPSTAT_LP: &str = "\
printer Brother_HL_L2390DW is idle.  enabled since Sat Oct 17 15:10:00 2026
\tForm mounted:
\tContent types: any
\tPrinter types: unknown
\tDescription: Brother HL-L2390DW
\tAlerts: none
\tLocation: Study, 2nd floor
\tConnection: direct
\tInterface: /etc/cups/ppd/Brother_HL_L2390DW.ppd
\tOn fault: no alert
\tAfter fault: continue
\tUsers allowed:
\t\t(all)
\tForms allowed:
\t\t(none)
\tBanner required
\tCharset sets:
\t\t(none)
\tDefault pitch:
\tDefault page size:
\tDefault port settings:
printer Brother_HL_L2390DW/duplex is idle.  enabled since Sat Oct 17 15:10:00 2026
\tDescription: Brother HL-L2390DW
printer Office-Laser now printing Office-Laser-42.  enabled since Sat Oct 17 15:11:02 2026
\tRendering completed
\tForm mounted:
\tContent types: any
\tPrinter types: unknown
\tDescription: Bürodrucker – 3. Stock 藍
\tAlerts: toner-low-report media-empty-warning
\tLocation:
\tConnection: direct
\tInterface: /etc/cups/ppd/Office-Laser.ppd
printer old_inkjet disabled since Thu Oct  1 08:00:00 2026 -
\tPaused
\tForm mounted:
\tDescription: Old inkjet
\tAlerts: paused
\tLocation: Garage
\tConnection: direct
printer Stopped_one disabled since Fri Oct 16 09:30:00 2026 -
\treason unknown
\tForm mounted:
\tDescription:
\tAlerts: paused
lpstat: Unable to talk to the printer for a moment.
\tThis line belongs to no printer.
printer Front-Desk_2 is holding new jobs.  enabled since Fri Oct 16 18:00:00 2026
\tForm mounted:
\tDescription: Front desk
\tAlerts: hold-new-jobs
\tLocation: Lobby
\tConnection: remote
\tInterface: ipp://print.example.org/printers/frontdesk.ppd
";

    pub(crate) const LPSTAT_A: &str = "\
Brother_HL_L2390DW accepting requests since Sat Oct 17 15:10:00 2026
Office-Laser accepting requests since Sat Oct 17 15:11:02 2026
old_inkjet not accepting requests since Thu Oct  1 08:00:00 2026 -
\tRejecting Jobs
Stopped_one accepting requests since Fri Oct 16 09:30:00 2026
Front-Desk_2 accepting requests since Fri Oct 16 18:00:00 2026
";

    pub(crate) const LPSTAT_V: &str = "\
device for Brother_HL_L2390DW: dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=e3248000-80ce-11db-8000-c0b5d7e5cc7d
device for Office-Laser: ipp://10.0.0.5/ipp/print
device for old_inkjet: usb://Canon/iP4950%20series?serial=1234AB
device for Stopped_one: socket://10.0.0.9:9100
device for Front-Desk_2: ipp://print.example.org/printers/frontdesk
";

    /// `lpstat -W not-completed -l -o`: a job printing, a held one, one
    /// with an owner with a space from another system and one whose date
    /// can't be read.
    pub(crate) const LPSTAT_LO: &str = "\
Office-Laser-42         callen            1024   Sat Oct 17 15:11:02 2026
\tStatus: Spooling job, 0% completed.
\tAlerts: job-printing
\tqueued for Office-Laser
Brother_HL_L2390DW-43   alice             3072   Sat Oct 17 15:12:00 2026
\tAlerts: job-hold-until-specified
\tqueued for Brother_HL_L2390DW
Front-Desk_2-44         Ana María            0   Fri Oct 16 18:05:09 2026
\tAlerts: job-incoming
\tqueued for Front-Desk_2
Office-Laser-45         bob               2048   someday
not a job line
";

    /// `lpq -a` for the jobs above, with a title cut to 31 bytes in the
    /// middle of a character.
    pub(crate) fn lpq_fixture() -> Vec<u8> {
        let jobs = [
            ("active", "callen", 42, "Quarterly report.pdf", 1024),
            ("1st", "alice", 43, "Aizome tests, chapter 藍染 – the long one", 3072),
            ("2nd", "Ana María", 44, "", 0),
            ("3rd", "bob", 45, "untitled", 2048),
        ];
        let mut text = b"Rank    Owner   Job     File(s)                         Total Size\n".to_vec();
        for (rank, owner, id, title, size) in jobs {
            // printf's "%-7s %-7.7s %-7d %-31.31s %.0f bytes", which counts
            // bytes.
            let line = [
                c_field(rank.as_bytes(), 7, usize::MAX),
                c_field(owner.as_bytes(), 7, 7),
                c_field(id.to_string().as_bytes(), 7, usize::MAX),
                c_field(title.as_bytes(), 31, 31),
                format!("{size} bytes\n").into_bytes(),
            ]
            .join(&b' ');
            text.extend(line);
        }
        text
    }

    /// `%-WIDTH.PRECISIONs`: cut to `precision` bytes, padded to `width`.
    fn c_field(value: &[u8], width: usize, precision: usize) -> Vec<u8> {
        let mut field = value.get(..precision.min(value.len())).unwrap_or(value).to_vec();
        while field.len() < width {
            field.push(b' ');
        }
        field
    }

    #[test]
    fn scheduler_state() {
        assert_eq!(scheduler_running("scheduler is running\n"), Some(true));
        assert_eq!(scheduler_running("scheduler is not running\n"), Some(false));
        assert_eq!(scheduler_running("lpstat: Scheduler is not running.\n"), Some(false));
        assert_eq!(scheduler_running(""), None);
    }

    #[test]
    fn printers_from_lpstat_lp() {
        let printers = printer_status(LPSTAT_LP);
        let names: Vec<&str> = printers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Brother_HL_L2390DW", "Office-Laser", "old_inkjet", "Stopped_one", "Front-Desk_2"]);

        let [brother, office, old, stopped, front] = printers.as_slice() else { panic!() };
        assert_eq!(brother.state, PrinterState::Idle);
        assert_eq!(brother.message, None);
        assert_eq!(brother.description.as_deref(), Some("Brother HL-L2390DW"));
        assert_eq!(brother.location.as_deref(), Some("Study, 2nd floor"));
        assert!(brother.reasons.is_empty());
        assert!(!brother.remote);
        assert!(brother.since.is_some());

        assert_eq!(office.state, PrinterState::Printing);
        assert_eq!(office.current_job.as_deref(), Some("Office-Laser-42"));
        assert_eq!(office.message.as_deref(), Some("Rendering completed"));
        assert_eq!(office.description.as_deref(), Some("Bürodrucker – 3. Stock 藍"));
        assert_eq!(office.location.as_deref(), Some(""));
        assert_eq!(office.reasons, ["toner-low-report", "media-empty-warning"]);
        let between = office.since.and_then(|o| o.duration_since(brother.since?).ok());
        assert_eq!(between, Some(Duration::from_secs(62)));

        assert_eq!(old.state, PrinterState::Stopped);
        assert_eq!(old.message.as_deref(), Some("Paused"));
        assert_eq!(old.reasons, ["paused"]);
        assert!(old.since.is_some(), "a space-padded day of the month");

        assert_eq!(stopped.state, PrinterState::Stopped);
        assert_eq!(stopped.message, None, "\"reason unknown\" is no message");
        assert_eq!(stopped.description.as_deref(), Some(""));

        assert_eq!(front.state, PrinterState::Idle);
        assert_eq!(front.reasons, ["hold-new-jobs"]);
        assert!(front.remote);
    }

    #[test]
    fn printers_from_short_lpstat_p() {
        let text = "printer a:b is idle.  enabled since Sat Oct 17 15:10:00 2026\n\
                    printer weird something new\n\
                    printer \n\
                    printer x disabled since Sat Oct 17 15:10:00 2026 -\n\
                    \tOut of paper\n";
        let printers = printer_status(text);
        assert_eq!(printers.len(), 3);
        assert_eq!(printers[0].name, "a:b");
        assert_eq!(printers[1].name, "weird");
        assert_eq!(printers[1].state, PrinterState::Idle);
        assert_eq!(printers[1].since, None);
        assert_eq!(printers[2].message.as_deref(), Some("Out of paper"));
        assert!(printer_status("lpstat: No destinations added.\n").is_empty());
    }

    #[test]
    fn accepting_and_devices() {
        let accepting = accepting(LPSTAT_A);
        assert_eq!(accepting.get("Brother_HL_L2390DW"), Some(&true));
        assert_eq!(accepting.get("old_inkjet"), Some(&false));
        assert_eq!(accepting.len(), 5);

        let devices = device_uris(LPSTAT_V);
        assert_eq!(devices.get("Office-Laser").map(String::as_str), Some("ipp://10.0.0.5/ipp/print"));
        assert_eq!(
            devices.get("Brother_HL_L2390DW").map(String::as_str),
            Some("dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=e3248000-80ce-11db-8000-c0b5d7e5cc7d")
        );
        let odd = device_uris("device for a:b: file:///dev/null\ndevice for broken\ndevice for x: \n");
        assert_eq!(odd.get("a:b").map(String::as_str), Some("file:///dev/null"));
        assert_eq!(odd.len(), 1);
    }

    #[test]
    fn default_destinations() {
        assert_eq!(default_destination("system default destination: Office-Laser\n").as_deref(), Some("Office-Laser"));
        assert_eq!(
            default_destination("system default destination: Office-Laser/duplex\n").as_deref(),
            Some("Office-Laser")
        );
        assert_eq!(default_destination("no system default destination\n"), None);
        assert_eq!(
            default_destination("lpstat: error - LPDEST environment variable names non-existent destination \"x\".\n"),
            None
        );
    }

    #[test]
    fn jobs_from_lpstat_lo() {
        let jobs = jobs(LPSTAT_LO);
        let ids: Vec<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
        assert_eq!(ids, ["Office-Laser-42", "Brother_HL_L2390DW-43", "Front-Desk_2-44", "Office-Laser-45"]);
        let [printing, held, remote, undated] = jobs.as_slice() else { panic!() };

        assert_eq!(printing.number, 42);
        assert_eq!(printing.printer, "Office-Laser");
        assert_eq!(printing.owner, "callen");
        assert_eq!(printing.size, 1024);
        assert_eq!(printing.status.as_deref(), Some("Spooling job, 0% completed."));
        assert_eq!(printing.reasons, ["job-printing"]);
        assert!(printing.submitted.is_some());

        assert_eq!(held.printer, "Brother_HL_L2390DW");
        assert_eq!(held.reasons, ["job-hold-until-specified"]);
        assert_eq!(held.status, None);

        assert_eq!(remote.owner, "Ana María");
        assert_eq!(remote.size, 0);
        assert_eq!(remote.printer, "Front-Desk_2");

        assert_eq!(undated.owner, "bob");
        assert_eq!(undated.size, 2048);
        assert_eq!(undated.submitted, None);
    }

    #[test]
    fn jobs_from_short_lpstat_o() {
        let text = "Office-Laser-42         callen            1024   Sat Oct 17 15:11:02 2026\n\
                    My-Printer-7            1000                 5   Sat Oct 17 15:11:02 2026\n\
                    garbage\n\
                    -3 x 1 Sat Oct 17 15:11:02 2026\n";
        let jobs = jobs(text);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].printer, "Office-Laser", "the printer comes from the id");
        assert_eq!(jobs[1].printer, "My-Printer");
        assert_eq!(jobs[1].owner, "1000", "a numeric owner is not the size");
        assert_eq!(jobs[1].size, 5);
    }

    #[test]
    fn job_states() {
        let reasons = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(job_state(&reasons(&["job-printing"]), false, false), JobState::Processing);
        assert_eq!(job_state(&[], true, false), JobState::Processing);
        assert_eq!(job_state(&reasons(&["job-hold-until-specified"]), false, false), JobState::Held);
        assert_eq!(job_state(&reasons(&["job-stopped"]), false, false), JobState::Stopped);
        assert_eq!(job_state(&reasons(&["printer-stopped"]), false, false), JobState::Pending);
        assert_eq!(job_state(&reasons(&["job-incoming"]), false, false), JobState::Pending);
        assert_eq!(job_state(&reasons(&["job-canceled-by-user"]), false, true), JobState::Canceled);
        assert_eq!(job_state(&reasons(&["aborted-by-system"]), false, true), JobState::Aborted);
        assert_eq!(job_state(&reasons(&["job-completed-successfully"]), false, true), JobState::Completed);
    }

    #[test]
    fn titles_from_lpq() {
        let titles = lpq_titles(&lpq_fixture());
        assert_eq!(titles.get(&42).map(String::as_str), Some("Quarterly report.pdf"));
        // 31 bytes, the last character cut in half and dropped.
        assert_eq!(titles.get(&43).map(String::as_str), Some("Aizome tests, chapter 藍染"));
        assert_eq!(titles.get(&44), None, "no title");
        assert_eq!(titles.get(&45).map(String::as_str), Some("untitled"));
        assert!(lpq_titles(b"no entries\n").is_empty());
        assert!(lpq_titles(b"x 1 bytes\n").is_empty());
        assert!(lpq_titles(b" bytes\n bytes").is_empty());
    }

    #[test]
    fn lpoptions_output() {
        let text = "copies=1 device-uri=ipp://10.0.0.5/ipp/print finishings=3 job-sheets=none,none \
                    printer-info='Bob's Büro printer' printer-is-accepting-jobs=true \
                    printer-is-shared=false printer-location printer-make-and-model='Brother \
                    HL-L2390DW series - IPP Everywhere' printer-state=3 \
                    printer-state-reasons=none printer-type=36908 odd=a\\ b quoted=\"x y\" =stray\n";
        let options = options(text);
        let get = |key: &str| options.get(key).map(String::as_str);
        assert_eq!(get("printer-info"), Some("Bob's Büro printer"));
        assert_eq!(get("printer-location"), Some(""));
        assert_eq!(get("printer-make-and-model"), Some("Brother HL-L2390DW series - IPP Everywhere"));
        assert_eq!(get("printer-type"), Some("36908"));
        assert_eq!(get("device-uri"), Some("ipp://10.0.0.5/ipp/print"));
        assert_eq!(get("job-sheets"), Some("none,none"));
        assert_eq!(get("odd"), Some("a b"));
        assert_eq!(get("quoted"), Some("x y"));
        assert!(super::options("").is_empty());
        assert_eq!(super::options("unterminated='abc").get("unterminated").map(String::as_str), Some("abc"));
    }

    #[test]
    fn lpoptions_files() {
        let file = "# saved by lpoptions\nDest Office-Laser media=A4\nDefault Brother_HL_L2390DW/duplex sides=two-sided-long-edge\nDefault Other\n";
        assert_eq!(lpoptions_default(file).as_deref(), Some("Brother_HL_L2390DW"));
        assert_eq!(lpoptions_default("default lower\n").as_deref(), Some("lower"));
        assert_eq!(lpoptions_default("Dest X\nDefault\n"), None);
        assert_eq!(
            without_default(file),
            "# saved by lpoptions\nDest Office-Laser media=A4\nDest Brother_HL_L2390DW/duplex sides=two-sided-long-edge\nDest Other\n"
        );
        assert_eq!(without_default("  default X"), "  Dest X");
    }

    #[test]
    fn devices_from_lpinfo() {
        let short = "network lpd\nnetwork ipp\ndirect usb://HP/LaserJet%20P1102?serial=000000\n\
                     serial serial:/dev/ttyS0?baud=115200\nfile cups-pdf:/\nbogus\n";
        let devices = devices(short);
        assert_eq!(devices.len(), 5);
        assert_eq!(devices[0].class, DeviceClass::Network);
        assert_eq!(devices[0].uri, "lpd");
        assert_eq!(devices[2].class, DeviceClass::Direct);
        assert_eq!(devices[3].class, DeviceClass::Serial);
        assert_eq!(devices[4].class, DeviceClass::File);

        // As lpinfo -l -v printed it on the machine this was written on.
        let long = "Device: uri = ipp\n        class = network\n        info = Internet Printing Protocol (ipp)\n        make-and-model = Unknown\n        device-id = \n        location = \n\
Device: uri = dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=e3248000-80ce-11db-8000-c0b5d7e5cc7d\n        class = network\n        info = Brother HL-L2390DW\n        make-and-model = Brother HL-L2390DW\n        device-id = MFG:Brother;MDL:HL-L2390DW;CMD:PJL,HBP,URF;\n        location = \n\
Device: uri = usb://Canon/iP4950%20series?serial=1234AB\n        class = direct\n        info = Canon iP4950 series\n        make-and-model = Canon iP4950 series\n        device-id = MFG:Canon;MDL:iP4950 series;CMD:BJL,BJRaster3;\n        location = Kitchen = left\n\
Device: uri = \n        class = network\n";
        let devices = super::devices(long);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].uri, "ipp");
        assert_eq!(devices[0].make_and_model, None);
        assert_eq!(devices[0].device_id, None);
        assert_eq!(devices[1].info, "Brother HL-L2390DW");
        assert_eq!(devices[1].device_id.as_deref(), Some("MFG:Brother;MDL:HL-L2390DW;CMD:PJL,HBP,URF;"));
        assert_eq!(devices[2].class, DeviceClass::Direct);
        assert_eq!(devices[2].location.as_deref(), Some("Kitchen = left"));
    }

    #[test]
    fn drivers_from_lpinfo() {
        let short = "drv:///sample.drv/generic.ppd Generic PostScript Printer\neverywhere IPP Everywhere\nraw Raw Queue\n\nlonely\n";
        let list: Vec<Driver> = drivers(short).collect();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "drv:///sample.drv/generic.ppd");
        assert_eq!(list[0].make_and_model, "Generic PostScript Printer");
        assert!(list[1].is_everywhere());

        let long = "Model:  name = drv:///sample.drv/dymo.ppd\n        natural_language = en\n        make-and-model = DYMO Label Printer\n        device-id = \n\
Model:  name = drv:///cupsfilters.drv/pwgrast.ppd\n        natural_language = en\n        make-and-model = Generic IPP Everywhere Printer\n        device-id = MFG:Generic;MDL:IPP Everywhere Printer;CMD:PWGRaster;\n\
Model:  name = \n        make-and-model = nameless\n\
Model:  name = everywhere\n        natural_language = en\n        make-and-model = IPP Everywhere\n        device-id = \n";
        let list: Vec<Driver> = drivers(long).collect();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].make_and_model, "DYMO Label Printer");
        assert_eq!(list[0].language.as_deref(), Some("en"));
        assert_eq!(list[0].device_id, None);
        assert_eq!(list[1].device_id.as_deref(), Some("MFG:Generic;MDL:IPP Everywhere Printer;CMD:PWGRaster;"));
        assert!(list[2].is_everywhere());
        // Lazy: the first model comes before the rest is read.
        assert_eq!(drivers(long).next().map(|d| d.name), Some("drv:///sample.drv/dymo.ppd".to_owned()));
        assert_eq!(drivers("").count(), 0);
    }

    #[test]
    fn driverless_listings() {
        // driverless list, as cups-filters 2.0.1 printed it here.
        let list = "DEBUG: Started ippfind (PID 581758)\n\
\"driverless:ipp://Brother%20HL-L2390DW._ipp._tcp.local/\" en \"Brother\" \"Brother HL-L2390DW, driverless, cups-filters 2.0.1\" \"MFG:Brother;MDL:HL-L2390DW;CMD:PWGRaster,AppleRaster,URF,PWG;\"\n\
\"driverless-fax:ipp://Fax._ipp._tcp.local/\" en \"X\" \"Fax\" \"\"\n\
DEBUG: ippfind (PID 581758) exited with no errors.\n";
        let devices = driverless(list);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].uri, "ipp://Brother%20HL-L2390DW._ipp._tcp.local/");
        assert_eq!(devices[0].info, "Brother HL-L2390DW");
        assert_eq!(devices[0].make_and_model.as_deref(), Some("Brother HL-L2390DW, driverless, cups-filters 2.0.1"));
        assert!(devices[0].driverless);

        // Plain driverless: bare URIs.
        let plain = "ipp://Brother%20HL-L2390DW._ipp._tcp.local/\nipps://10.0.0.7:631/ipp/print\nipps://10.0.0.7:631/ipp/print\n";
        let devices = driverless(plain);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].info, "Brother HL-L2390DW");
        assert_eq!(devices[1].info, "ipps://10.0.0.7:631/ipp/print");
    }

    #[test]
    fn lp_request_ids() {
        assert_eq!(request_id("request id is Office-Laser-42 (1 file(s))\n").as_deref(), Some("Office-Laser-42"));
        assert_eq!(request_id("lp: warning\nrequest id is a-b-c-7 (0 file(s))\n").as_deref(), Some("a-b-c-7"));
        assert_eq!(request_id("request id is nonsense\n"), None);
        assert_eq!(request_id(""), None);
    }

    #[test]
    fn job_ids() {
        assert_eq!(split_job_id("Office-Laser-42"), Some(("Office-Laser", 42)));
        assert_eq!(split_job_id("a_b-1"), Some(("a_b", 1)));
        assert_eq!(split_job_id("-1"), None);
        assert_eq!(split_job_id("Office-"), None);
        assert_eq!(split_job_id("Office-4x"), None);
        assert_eq!(split_job_id("Office-99999999999999"), None);
    }

    #[test]
    fn service_names_and_device_ids() {
        assert_eq!(
            service_name("dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=e3").as_deref(),
            Some("Brother HL-L2390DW")
        );
        assert_eq!(service_name("ipps://My%2E._Printer._ipps._tcp.local/").as_deref(), Some("My.._Printer"));
        assert_eq!(service_name("ipp://10.0.0.5/ipp/print"), None);
        assert_eq!(service_name("usb://HP"), None);
        assert_eq!(percent_decode("a%20b%zz%2"), "a b%zz%2");
        assert_eq!(percent_decode("%E8%97%8D"), "藍");

        let id = "MFG:Brother;MDL:HL-L2390DW;COMMAND SET:PJL,HBP,URF;";
        assert_eq!(device_id_field(id, &["MDL", "MODEL"]), Some("HL-L2390DW"));
        assert!(speaks_driverless(id));
        assert!(!speaks_driverless("MFG:Canon;CMD:BJL,BJRaster3;"));
        assert!(!speaks_driverless(""));
    }

    #[test]
    fn cups_files_conf() {
        let conf = "SystemGroup root wheel lpadmin\n\nServerBin /nix/store/x-cups-progs/lib/cups\nDataDir /nix/store/x-cups-progs/share/cups\n";
        assert_eq!(conf_path(conf, "DataDir"), Some(PathBuf::from("/nix/store/x-cups-progs/share/cups")));
        assert_eq!(conf_path(conf, "TempDir"), None);
    }

    #[test]
    fn c_dates() {
        let a = c_date("Sat Oct 17 15:10:00 2026").expect("a C date");
        let b = c_date("Sun Oct 18 15:10:00 2026 - and more").expect("a C date");
        let day = b.duration_since(a).map(|d| d.as_secs()).unwrap_or_default();
        // A day, or a day and an hour's daylight saving change either way.
        assert!([82_800, 86_400, 90_000].contains(&day), "{day}");
        assert_eq!(c_date("Sat Oct 17 15:10:00"), None);
        assert_eq!(c_date("Sat Foo 17 15:10:00 2026"), None);
        assert_eq!(c_date("Sat Oct 32 15:10:00 2026"), None);
        assert_eq!(c_date("Sat Oct 17 25:10:00 2026"), None);
        assert_eq!(c_date("Sat Oct 17 15:10 2026"), None);
        assert_eq!(c_date("Sat Oct 17 15:10:00:00 2026"), None);
        assert_eq!(c_date("17/10/2026 15:10:00 Sat"), None);
        assert_eq!(c_date(""), None);
    }
}
