//! Printers & Scanners: the printers with their state and queue, the
//! default printer, a test page, pausing and resuming, sharing, removing,
//! and adding one: found on the network or plugged in, or by its address,
//! with IPP Everywhere when it can print without a driver or a driver
//! chosen from CUPS's. Through CUPS's tools (`aizome-sys::printers`).
//!
//! Scanners aren't covered yet: `aizome-sys` has no SANE service.

use std::collections::HashSet;
use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_sys::printers::{
    Device, DeviceClass, Driver, DriverChoice, DriverFilter, Job, JobState, NewPrinter, Printer, PrinterState,
    Printers as Cups, PrintersState, validate_name,
};
use aizome_sys::{Error, Task};

use crate::form::{self, Row};
use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::panes::hw::spinner;
use crate::system::Cell;
use crate::widgets::{PressStyle, press, tile};

/// CUPS, live or the sample.
pub enum Spool {
    None,
    Live(Cups),
    Sample(Cell<PrintersState>),
}

impl Spool {
    pub fn snapshot(&self) -> Arc<PrintersState> {
        match self {
            Spool::None => Arc::new(PrintersState::default()),
            Spool::Live(c) => c.snapshot(),
            Spool::Sample(c) => c.get(),
        }
    }

    fn sample(&self, f: impl FnOnce(&mut PrintersState)) -> Task<()> {
        match self {
            Spool::Sample(c) => c.update(f),
            _ => Task::ready(Err(Error::Unavailable("CUPS".into()))),
        }
    }
}

/// The sheet for adding a printer.
#[derive(Clone, Debug, PartialEq)]
pub struct Add {
    /// Devices found, once looked for.
    pub devices: Option<Result<Vec<Device>, String>>,
    /// The one chosen, by index; `None` with an address typed instead.
    pub chosen: Option<usize>,
    pub by_address: bool,
    pub address: String,
    pub name: String,
    pub location: String,
    /// Drivers for the chosen device, when it needs one.
    pub drivers: Option<Result<Vec<Driver>, String>>,
    pub driver: usize,
    pub adding: bool,
    pub error: Option<String>,
}

impl Add {
    fn new() -> Add {
        Add {
            devices: None,
            chosen: None,
            by_address: false,
            address: String::new(),
            name: String::new(),
            location: String::new(),
            drivers: None,
            driver: 0,
            adding: false,
            error: None,
        }
    }

    fn device(&self) -> Option<&Device> {
        self.devices.as_ref()?.as_ref().ok()?.get(self.chosen?)
    }

    /// The URI for an address typed: `ipp://HOST/ipp/print`, or as typed
    /// when it's a URI already.
    pub fn address_uri(&self) -> Option<String> {
        let a = self.address.trim();
        if a.is_empty() || a.contains(char::is_whitespace) {
            return None;
        }
        Some(if a.contains("://") { a.to_owned() } else { format!("ipp://{a}/ipp/print") })
    }

    /// Whether a driver is needed (not driverless, and not by address,
    /// which tries IPP Everywhere).
    fn needs_driver(&self) -> bool {
        !self.by_address && self.device().is_some_and(|d| !d.driverless)
    }

    fn ready(&self) -> bool {
        let target = if self.by_address { self.address_uri().is_some() } else { self.device().is_some() };
        target
            && validate_name(self.name.trim()).is_ok()
            && !self.adding
            && (!self.needs_driver() || self.drivers.as_ref().is_some_and(|d| d.as_ref().is_ok_and(|d| !d.is_empty())))
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Default(usize),
    Expand(Option<String>),
    TestPage(String),
    Printed(String, Result<String, Error>),
    Pause(String, bool),
    Accepting(String, bool),
    Shared(String, bool),
    Remove(String),
    RemoveNow(String),
    Cancel(String),
    CancelAll(String),
    AddPrinter,
    Found(Result<Vec<Device>, Error>),
    Choose(usize),
    ByAddress(bool),
    Address(String),
    Name(String),
    Location(String),
    Drivers(Result<Vec<Driver>, Error>),
    Driver(usize),
    Submit,
    Added(String, Result<(), Error>),
    CloseSheet,
    Done(String, Result<(), Error>),
}

pub struct Printers {
    spool: Spool,
    expanded: Option<String>,
    busy: HashSet<String>,
    add: Option<Add>,
}

impl Default for Printers {
    fn default() -> Printers {
        Printers { spool: Spool::None, expanded: None, busy: HashSet::new(), add: None }
    }
}

/// The printer's state in words, from its state, message and reasons.
pub fn state_words(p: &Printer, jobs: &[&Job]) -> String {
    let reason = p.state_reasons.iter().find_map(|r| {
        let r = r.trim_end_matches("-error").trim_end_matches("-warning").trim_end_matches("-report");
        Some(match r {
            "media-empty" | "media-needed" => "Out of paper",
            "media-jam" => "Paper jam",
            "toner-empty" | "marker-supply-empty" => "Out of toner or ink",
            "toner-low" | "marker-supply-low" => "Low on toner or ink",
            "door-open" | "cover-open" => "A door is open",
            "offline" | "connecting-to-device" => "Not responding",
            _ => return None,
        })
    });
    match p.state {
        PrinterState::Stopped if p.state_reasons.iter().any(|r| r == "paused") => "Paused".into(),
        PrinterState::Stopped => {
            reason.map(str::to_owned).or_else(|| p.state_message.clone()).unwrap_or_else(|| "Stopped".into())
        }
        PrinterState::Printing => {
            let title = jobs.iter().find(|j| j.state == JobState::Processing).and_then(|j| j.title.clone());
            match title {
                Some(t) => format!("Printing “{t}”"),
                None => "Printing".into(),
            }
        }
        PrinterState::Idle => match reason {
            Some(r) => format!("Ready · {r}"),
            None if !p.accepting => "Not taking new jobs".into(),
            None => "Ready".into(),
        },
    }
}

fn size_words(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", (bytes / 1024).max(1))
    }
}

fn job_words(j: &Job) -> &'static str {
    match j.state {
        JobState::Pending => "Waiting",
        JobState::Held => "Held",
        JobState::Processing => "Printing",
        JobState::Stopped => "Stopped",
        JobState::Canceled => "Cancelled",
        JobState::Aborted => "Failed",
        JobState::Completed => "Printed",
    }
}

impl Printers {
    fn act(&mut self, cx: &mut PaneCx<Msg>, what: &str, printer: &str, task: Task<()>) {
        if !printer.is_empty() {
            self.busy.insert(printer.to_owned());
        }
        let (what, printer) = (what.to_owned(), printer.to_owned());
        cx.wait(task, move |r| Msg::Done(format!("{what}\u{1f}{printer}"), r));
    }

    fn with_printer(&self, name: &str, f: impl FnOnce(&mut Printer) + Send + 'static) -> Task<()> {
        let name = name.to_owned();
        self.spool.sample(move |s| {
            if let Some(p) = s.printers.iter_mut().find(|p| p.name == name) {
                f(p);
            }
        })
    }

    fn look_for_devices(&mut self, cx: &mut PaneCx<Msg>) {
        match &self.spool {
            Spool::Live(c) => {
                let t = c.discover();
                cx.wait(t, Msg::Found);
            }
            _ => cx.send(Msg::Found(Ok(sample_devices()))),
        }
    }
}

impl Pane for Printers {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "printers"
    }

    fn title(&self) -> &'static str {
        "Printers & Scanners"
    }

    fn icon(&self) -> Icon {
        Icon::Printer
    }

    fn tile(&self) -> Tile {
        Tile::Slate
    }

    fn group(&self) -> Group {
        Group::Devices
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["printer", "print", "cups", "queue", "jobs", "scanner", "airprint", "ipp"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("default-printer", "Default printer"),
            SettingEntry::new("printers", "Printers").keywords(&[
                "status",
                "queue",
                "jobs",
                "pause",
                "resume",
                "test page",
            ]),
            SettingEntry::new("add-printer", "Add Printer").keywords(&[
                "new",
                "network printer",
                "usb",
                "address",
                "ip",
            ]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        if matches!(self.spool, Spool::None) {
            let wake = cx.wake();
            self.spool = if cx.sample() {
                Spool::Sample(Cell::new(
                    crate::panes::hw::live_snapshot(
                        || Cups::new(aizome_sys::Wake::none()),
                        |c| c.snapshot().loaded,
                        |c| c.snapshot(),
                    )
                    .unwrap_or_else(sample),
                    wake,
                ))
            } else {
                Spool::Live(Cups::new(wake))
            };
        } else if let Spool::Live(c) = &self.spool {
            let t = c.refresh();
            cx.wait(t, |_| Msg::Done(String::new(), Ok(())));
        }
    }

    fn hidden(&mut self, _cx: &mut PaneCx<Msg>) {
        // CUPS is polled (it says nothing by itself): only while the pane
        // shows.
        if matches!(self.spool, Spool::Live(_)) && self.add.is_none() {
            self.spool = Spool::None;
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Default(i) => {
                let state = self.spool.snapshot();
                let Some(p) = state.printers.get(i) else { return };
                let name = p.name.clone();
                let t = match &self.spool {
                    Spool::Live(c) => c.set_user_default(&name),
                    _ => self.spool.sample(move |s| {
                        s.user_default = Some(name.clone());
                        for p in &mut s.printers {
                            p.is_user_default = p.name == name;
                        }
                    }),
                };
                self.act(cx, "Couldn't make it the default", "", t);
            }
            Msg::Expand(name) => self.expanded = if self.expanded == name { None } else { name },
            Msg::TestPage(name) => match &self.spool {
                Spool::Live(c) => {
                    self.busy.insert(name.clone());
                    let t = c.print_test_page(&name);
                    cx.wait(t, move |r| Msg::Printed(name, r));
                }
                _ => cx.send(Msg::Printed(name, Ok("sample-1".into()))),
            },
            Msg::Printed(name, result) => {
                self.busy.remove(&name);
                match result {
                    Ok(_) => cx.say("The test page is on its way"),
                    Err(e) => cx.failed("Couldn't print the test page", e),
                }
            }
            Msg::Pause(name, pause) => {
                let t = match &self.spool {
                    Spool::Live(c) if pause => c.pause(&name),
                    Spool::Live(c) => c.resume(&name),
                    _ => self.with_printer(&name, move |p| {
                        p.state = if pause { PrinterState::Stopped } else { PrinterState::Idle };
                        p.enabled = !pause;
                        p.state_reasons = if pause { vec!["paused".into()] } else { Vec::new() };
                    }),
                };
                self.act(cx, if pause { "Couldn't pause it" } else { "Couldn't resume it" }, &name, t);
            }
            Msg::Accepting(name, on) => {
                let t = match &self.spool {
                    Spool::Live(c) => c.set_accepting(&name, on),
                    _ => self.with_printer(&name, move |p| p.accepting = on),
                };
                self.act(cx, "Couldn't change it", &name, t);
            }
            Msg::Shared(name, on) => {
                let t = match &self.spool {
                    Spool::Live(c) => c.set_shared(&name, on),
                    _ => self.with_printer(&name, move |p| p.shared = Some(on)),
                };
                self.act(cx, "Couldn't change sharing", &name, t);
            }
            Msg::Remove(name) => {
                let shown = self
                    .spool
                    .snapshot()
                    .printer(&name)
                    .map(|p| p.display_name().to_owned())
                    .unwrap_or_else(|| name.clone());
                cx.confirm(
                    Confirm::new(
                        format!("Remove “{shown}”?"),
                        "Its waiting jobs are cancelled. You can add it again later.",
                        "Remove Printer",
                        Msg::RemoveNow(name),
                    )
                    .destructive(),
                );
            }
            Msg::RemoveNow(name) => {
                self.expanded = None;
                let t = match &self.spool {
                    Spool::Live(c) => c.remove_printer(&name),
                    _ => {
                        let n = name.clone();
                        self.spool.sample(move |s| s.printers.retain(|p| p.name != n))
                    }
                };
                self.act(cx, "Couldn't remove it", &name, t);
            }
            Msg::Cancel(job) => {
                let t = match &self.spool {
                    Spool::Live(c) => c.cancel_job(&job),
                    _ => {
                        let j = job.clone();
                        self.spool.sample(move |s| s.jobs.retain(|x| x.id != j))
                    }
                };
                self.act(cx, "Couldn't cancel the job", "", t);
            }
            Msg::CancelAll(name) => {
                let t = match &self.spool {
                    Spool::Live(c) => c.cancel_all_jobs(&name),
                    _ => {
                        let n = name.clone();
                        self.spool.sample(move |s| s.jobs.retain(|x| x.printer != n))
                    }
                };
                self.act(cx, "Couldn't cancel the jobs", "", t);
            }
            Msg::AddPrinter => {
                self.add = Some(Add::new());
                cx.open_sheet();
                self.look_for_devices(cx);
            }
            Msg::Found(r) => {
                let printers = self.spool.snapshot().printers.clone();
                if let Some(a) = &mut self.add {
                    a.devices = Some(
                        r.map(|mut d| {
                            // Not the printers already here (cups-browsed
                            // sets up the network's by itself); network
                            // printers that need no driver first.
                            d.retain(|x| x.class != DeviceClass::File && !already_added(x, &printers));
                            d.sort_by_key(|x| (x.is_manual(), !x.driverless, x.class != DeviceClass::Network));
                            d
                        })
                        .map_err(|e| e.to_string()),
                    );
                }
            }
            Msg::Choose(i) => {
                let state = self.spool.snapshot();
                let Some(a) = &mut self.add else { return };
                a.chosen = Some(i);
                a.by_address = false;
                a.error = None;
                a.drivers = None;
                a.driver = 0;
                if let Some(d) = a.device().cloned() {
                    a.name = state.suggest_name(&d.model_name());
                    if let Some(l) = &d.location {
                        a.location = l.clone();
                    }
                    if d.is_manual() {
                        a.by_address = true;
                    } else if !d.driverless {
                        let filter = DriverFilter {
                            device_id: d.device_id.clone(),
                            make_and_model: d.make_and_model.clone(),
                            language: None,
                        };
                        match &self.spool {
                            Spool::Live(c) => {
                                let t = c.drivers(filter);
                                cx.wait(t, Msg::Drivers);
                            }
                            _ => cx.send(Msg::Drivers(Ok(sample_drivers()))),
                        }
                    }
                }
            }
            Msg::ByAddress(on) => {
                if let Some(a) = &mut self.add {
                    a.by_address = on;
                    a.error = None;
                    if on {
                        a.chosen = None;
                        cx.focus("printers-address");
                    }
                }
            }
            Msg::Address(t) => {
                let state = self.spool.snapshot();
                if let Some(a) = &mut self.add {
                    let host =
                        t.trim().split("://").last().unwrap_or("").split(['/', ':']).next().unwrap_or("").to_owned();
                    let suggested = state.suggest_name(&a.address);
                    if a.name.is_empty() || a.name == suggested {
                        a.name = if host.is_empty() { String::new() } else { state.suggest_name(&host) };
                    }
                    a.address = t;
                }
            }
            Msg::Name(t) => {
                if let Some(a) = &mut self.add {
                    a.name = t;
                    a.error = None;
                }
            }
            Msg::Location(t) => {
                if let Some(a) = &mut self.add {
                    a.location = t;
                }
            }
            Msg::Drivers(r) => {
                if let Some(a) = &mut self.add {
                    let list = r.map_err(|e| e.to_string());
                    // The best match first: IPP Everywhere, then the recommended.
                    a.drivers = Some(list);
                    a.driver = 0;
                }
            }
            Msg::Driver(i) => {
                if let Some(a) = &mut self.add {
                    a.driver = i;
                }
            }
            Msg::Submit => {
                let Some(a) = &mut self.add else { return };
                if !a.ready() {
                    if let Err(e) = validate_name(a.name.trim()) {
                        a.error = Some(e.to_string());
                    }
                    return;
                }
                let uri = if a.by_address { a.address_uri() } else { a.device().map(|d| d.uri.clone()) };
                let Some(uri) = uri else { return };
                let driver = if a.needs_driver() {
                    match a.drivers.as_ref().and_then(|d| d.as_ref().ok()).and_then(|d| d.get(a.driver)) {
                        Some(d) if d.is_everywhere() => DriverChoice::Everywhere,
                        Some(d) => DriverChoice::Model(d.name.clone()),
                        None => DriverChoice::Everywhere,
                    }
                } else {
                    DriverChoice::Everywhere
                };
                let name = a.name.trim().to_owned();
                let description = a.device().map(Device::model_name).filter(|m| !m.is_empty());
                let new = NewPrinter {
                    name: name.clone(),
                    device_uri: uri,
                    driver,
                    description: description.clone(),
                    location: Some(a.location.trim().to_owned()).filter(|l| !l.is_empty()),
                };
                a.adding = true;
                let t = match &self.spool {
                    Spool::Live(c) => c.add_printer(new),
                    _ => self.spool.sample(move |s| {
                        s.printers.push(Printer {
                            name: new.name.clone(),
                            description: description.unwrap_or_default(),
                            location: new.location.clone().unwrap_or_default(),
                            device_uri: Some(new.device_uri.clone()),
                            accepting: true,
                            enabled: true,
                            driverless: Some(new.driver == DriverChoice::Everywhere),
                            ..Printer::default()
                        })
                    }),
                };
                cx.wait(t, move |r| Msg::Added(name, r));
            }
            Msg::Added(name, result) => match result {
                Ok(()) => {
                    self.add = None;
                    cx.close_sheet();
                    cx.say(format!("Added {name}"));
                    self.expanded = Some(name);
                }
                Err(e) => {
                    if let Some(a) = &mut self.add {
                        a.adding = false;
                        a.error = Some(e.to_string());
                    }
                }
            },
            Msg::CloseSheet => {
                self.add = None;
                cx.close_sheet();
            }
            Msg::Done(key, result) => {
                let (what, printer) = key.split_once('\u{1f}').unwrap_or((key.as_str(), ""));
                if !printer.is_empty() {
                    self.busy.remove(printer);
                }
                if let Err(e) = result
                    && !what.is_empty()
                {
                    let why = match &e {
                        Error::NotAuthorized(_) => "only an administrator can change printers".to_owned(),
                        other => other.to_string(),
                    };
                    cx.failed(what, why);
                }
            }
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let state = self.spool.snapshot();
        if matches!(self.spool, Spool::Live(_)) {
            if !state.loaded {
                return form::page(vec![
                    row![spinner(14.0), text("Asking CUPS about the printers…").role(Role::Text3)].spacing(8.0).into(),
                ]);
            }
            if !state.available {
                return form::page(vec![
                    form::heading("Printing isn't set up", "Printers are managed by CUPS, which isn't running."),
                    form::note(
                        "On NixOS it's turned on with services.printing.enable = true; services.avahi.enable finds network printers.",
                    ),
                ]);
            }
        }
        let mut sections: Vec<Element<Msg>> = Vec::new();
        // The default printer.
        if !state.printers.is_empty() {
            let labels: Vec<&str> = state.printers.iter().map(|p| p.display_name()).collect();
            // None chosen shows none, so the first printer can be chosen too.
            let at = state.default_printer().and_then(|d| state.printers.iter().position(|p| p.name == d.name));
            let shown = |n: &str| state.printer(n).map(|p| p.display_name().to_owned()).unwrap_or_else(|| n.to_owned());
            let help = match (&state.user_default, &state.system_default) {
                (None, Some(s)) => format!("Everyone's default is {}", shown(s)),
                (Some(u), Some(s)) if u == s => "Yours, and everyone's on this computer".to_owned(),
                (Some(_), Some(s)) => format!("Yours; everyone's is {}", shown(s)),
                _ => "Where documents print unless you choose another".to_owned(),
            };
            sections.push(vcx.section(
                "",
                vec![
                        form::row(
                            "Default printer",
                            dropdown(labels.iter().map(|l| (*l).to_owned()), at)
                                .placeholder("None")
                                .on_select(Msg::Default)
                                .min_width(160.0),
                        )
                            .help(help)
                            .entry("default-printer"),
                    ],
            ));
        }
        // The printers.
        let mut rows: Vec<Row<Msg>> = Vec::new();
        for p in &state.printers {
            let jobs: Vec<&Job> = state.jobs_on(&p.name).collect();
            let default = state.default_printer().is_some_and(|d| d.name == p.name);
            rows.push(printer_row(self, p, &jobs, default));
            if self.expanded.as_deref() == Some(p.name.as_str()) {
                rows.extend(printer_details(self, p, &jobs));
            }
        }
        if rows.is_empty() {
            rows.push(form::row("No printers", space(0.0)).help("Add one that's on your network or plugged in."));
        }
        let first = rows.remove(0);
        rows.insert(0, first.entry("printers"));
        rows.push(
            form::row("Add a printer on the network or plugged in", button("Add Printer…").on_press(Msg::AddPrinter))
                .leading(container(icon(Icon::CirclePlus).size(16.0).role(Role::Text2)).width(28.0).center())
                .entry("add-printer"),
        );
        sections.push(vcx.section("Printers", rows));
        if let Some(e) = &state.error {
            sections.push(form::note(format!("CUPS didn't answer the last time it was asked: {e}")));
        }
        form::page(sections)
    }

    fn sheet(&self, _vcx: &ViewCx) -> Option<Element<Msg>> {
        self.add.as_ref().map(add_sheet)
    }

    fn back(&mut self, cx: &mut PaneCx<Msg>) -> bool {
        if self.add.take().is_some() {
            cx.close_sheet();
            return true;
        }
        self.expanded.take().is_some()
    }
}

fn printer_row(p: &Printers, printer: &Printer, jobs: &[&Job], default: bool) -> Row<Msg> {
    let words = state_words(printer, jobs);
    let colour = match printer.state {
        PrinterState::Stopped => Tile::Red,
        PrinterState::Printing => Tile::Kohaku,
        PrinterState::Idle
            if printer.state_reasons.iter().any(|r| r.ends_with("-warning") || r.ends_with("-report")) =>
        {
            Tile::Kohaku
        }
        PrinterState::Idle => Tile::Green,
    };
    let mut detail = words;
    if !printer.location.is_empty() {
        detail = format!("{detail} · {}", printer.location);
    }
    let mut line = row![
        tile(Icon::Printer, colour, 28.0),
        column![text(printer.display_name().to_owned()).weight(500.0), text(detail).size(11.0).role(Role::Text3)]
            .spacing(2.0)
            .width(Fill),
    ]
    .spacing(10.0)
    .align(Align::Center)
    .width(Fill);
    // The one documents go to: yours, else everyone's (never both).
    if default {
        line = line.push(badge("Default").role(Role::Success));
    }
    if !jobs.is_empty() {
        line = line
            .push(badge(format!("{} job{}", jobs.len(), if jobs.len() == 1 { "" } else { "s" })).role(Role::Accent));
    }
    if p.busy.contains(&printer.name) {
        line = line.push(spinner(14.0));
    }
    let open = p.expanded.as_deref() == Some(printer.name.as_str());
    line = line.push(icon(if open { Icon::ChevronUp } else { Icon::ChevronDown }).size(14.0).role(Role::Text3));
    form::whole(
        press(line)
            .style(PressStyle::Plain)
            .padding([8.0, 12.0])
            .on_press(Msg::Expand(Some(printer.name.clone())))
            .named(printer.display_name().to_owned())
            .width(Fill),
    )
}

fn printer_details(p: &Printers, printer: &Printer, jobs: &[&Job]) -> Vec<Row<Msg>> {
    let name = printer.name.clone();
    let busy = p.busy.contains(&name);
    let mut rows: Vec<Row<Msg>> = Vec::new();
    if let Some(m) = &printer.make_and_model {
        rows.push(form::value("Kind", m.clone()).detail(match printer.driverless {
            Some(true) => "Prints without a driver (IPP Everywhere)".to_owned(),
            _ => String::new(),
        }));
    }
    if let Some(uri) = &printer.device_uri {
        rows.push(form::value_mono("Address", uri.clone()));
    }
    let paused = printer.is_paused();
    let n1 = name.clone();
    rows.push(form::row(
        if !paused {
            "Printing"
        } else if printer.state_reasons.iter().any(|r| r == "paused") {
            "Paused: jobs wait until it resumes"
        } else {
            "Stopped by a problem: jobs wait until it resumes"
        },
        row![
            button(if paused { "Resume" } else { "Pause" })
                .compact()
                .on_press(Msg::Pause(n1.clone(), !paused))
                .enabled(!busy),
            button("Print Test Page").compact().on_press(Msg::TestPage(n1.clone())).enabled(!busy && !paused),
        ]
        .spacing(8.0),
    ));
    let n2 = name.clone();
    rows.push(form::row(
        "Take new jobs",
        toggle(printer.accepting).on_toggle(move |on| Msg::Accepting(n2.clone(), on)),
    ));
    if !printer.remote {
        let n3 = name.clone();
        rows.push(
            form::row(
                "Share on the network",
                toggle(printer.shared.unwrap_or(false)).on_toggle(move |on| Msg::Shared(n3.clone(), on)),
            )
            .help("Other computers here can print to it"),
        );
    }
    for j in jobs {
        let title = j.title.clone().unwrap_or_else(|| format!("Job {}", j.number));
        let who = if j.owner.is_empty() || j.owner == "unknown" { String::new() } else { format!(" · {}", j.owner) };
        rows.push(
            form::row(title, button("Cancel").compact().on_press(Msg::Cancel(j.id.clone())))
                .leading(
                    icon(if j.state == JobState::Processing { Icon::Printer } else { Icon::Hourglass })
                        .size(14.0)
                        .role(Role::Text3),
                )
                .detail(format!("{} · {}{who}", job_words(j), size_words(j.size))),
        );
    }
    if jobs.len() > 1 {
        rows.push(form::row("", button("Cancel All Jobs").compact().on_press(Msg::CancelAll(name.clone()))));
    }
    rows.push(form::row(
        "Remove it from this computer",
        button("Remove Printer…").destructive().compact().on_press(Msg::Remove(name)),
    ));
    rows
}

/// A DNS-SD service's name from a URI that names one
/// (`ipps://Brother%20HL._ipps._tcp.local/`, `dnssd://…`,
/// `driverless:ipp://…`): `Brother HL`.
fn service_name(uri: &str) -> Option<String> {
    let rest = uri.split_once("://")?.1;
    let host = rest.split('/').next()?;
    let at = host.find("._ipp").or_else(|| host.find("._pdl-datastream")).or_else(|| host.find("._printer"))?;
    let mut name = Vec::new();
    let bytes = &host.as_bytes()[..at];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = host.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            name.push(b);
            i += 3;
        } else {
            name.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(name).ok().filter(|n| !n.is_empty())
}

/// The queue name CUPS and cups-browsed make of a printer's name: what
/// isn't a letter, digit or `-` becomes `_`.
fn queue_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// Whether a printer found on the network is one already set up: the same
/// address, or the same DNS-SD service (a queue cups-browsed made for it,
/// `implicitclass://Its_Name/`, or one added by its service's address).
pub(crate) fn already_added(device: &Device, printers: &[Printer]) -> bool {
    let uri = device.uri.strip_prefix("driverless:").unwrap_or(&device.uri);
    let service = service_name(uri);
    printers.iter().any(|p| {
        let Some(theirs) = p.device_uri.as_deref() else { return false };
        if theirs == uri || theirs == device.uri {
            return true;
        }
        let Some(service) = &service else { return false };
        if let Some(q) = theirs.strip_prefix("implicitclass://") {
            return q.trim_end_matches('/').eq_ignore_ascii_case(&queue_name(service));
        }
        service_name(theirs).is_some_and(|s| s.eq_ignore_ascii_case(service))
    })
}

fn add_sheet(a: &Add) -> Element<Msg> {
    let mut col = column![text("Add a Printer").size(15.0).weight(650.0)].spacing(12.0).width(Fill);
    // What was found.
    let found: Element<Msg> = match &a.devices {
        None => row![spinner(14.0), text("Looking for printers on the network and plugged in…").role(Role::Text3)]
            .spacing(8.0)
            .into(),
        Some(Err(e)) => text(format!("Couldn't look for printers: {e}")).role(Role::Danger).wrap().into(),
        Some(Ok(list)) => {
            let shown: Vec<(usize, &Device)> = list.iter().enumerate().filter(|(_, d)| !d.is_manual()).collect();
            if shown.is_empty() {
                text("No printers found. Type one's address instead.").role(Role::Text3).into()
            } else {
                let items: Vec<Element<Msg>> = shown
                    .iter()
                    .map(|(i, d)| {
                        let selected = a.chosen == Some(*i) && !a.by_address;
                        let kind = match d.class {
                            DeviceClass::Direct => "Plugged in",
                            DeviceClass::Network if d.driverless => "Network · no driver needed",
                            DeviceClass::Network => "Network",
                            _ => "",
                        };
                        press(
                            row![
                                icon(if d.class == DeviceClass::Direct { Icon::Usb } else { Icon::Printer }).size(16.0),
                                column![text(d.model_name()), text(kind).size(11.0).role(Role::Text3)]
                                    .spacing(1.0)
                                    .width(Fill),
                            ]
                            .spacing(10.0)
                            .align(Align::Center)
                            .width(Fill),
                        )
                        .style(PressStyle::Row)
                        .selected(selected)
                        .padding([6.0, 10.0])
                        .on_press(Msg::Choose(*i))
                        .named(d.model_name())
                        .width(Fill)
                        .into()
                    })
                    .collect();
                container(column(items).width(Fill))
                    .fill(Tone::Sunk)
                    .border(Edge::Line)
                    .padding(1.0)
                    .width(Fill)
                    .max_height(200.0)
                    .into()
            }
        }
    };
    col = col.push(found);
    col = col.push(checkbox("Add one by its address", a.by_address).on_toggle(Msg::ByAddress));
    if a.by_address {
        col = col.push(labelled(
            "Address",
            text_field(&a.address)
                .placeholder("192.168.1.40, printer.local or ipp://…")
                .mono()
                .on_change(Msg::Address)
                .id("printers-address")
                .width(Fill),
        ));
        if let Some(uri) = a.address_uri() {
            col = col.push(text(format!("Added as {uri}, with IPP Everywhere")).size(11.0).role(Role::Text3).wrap());
        }
    }
    if a.by_address || a.chosen.is_some() {
        col = col.push(
            row![
                labelled("Name", text_field(&a.name).on_change(Msg::Name).width(Fill)).width(Fill),
                labelled(
                    "Location",
                    text_field(&a.location).placeholder("Optional").on_change(Msg::Location).width(Fill)
                )
                .width(Fill)
            ]
            .spacing(12.0)
            .width(Fill),
        );
    }
    if a.needs_driver() {
        let drivers: Element<Msg> = match &a.drivers {
            None => row![spinner(14.0), text("Finding drivers…").role(Role::Text3)].spacing(8.0).into(),
            Some(Err(e)) => text(format!("Couldn't list drivers: {e}")).role(Role::Danger).wrap().into(),
            Some(Ok(list)) if list.is_empty() => {
                text("CUPS has no driver for this printer.").role(Role::Danger).wrap().into()
            }
            Some(Ok(list)) => dropdown(list.iter().map(|d| d.make_and_model.clone()), a.driver)
                .on_select(Msg::Driver)
                .width(Fill)
                .into(),
        };
        col = col.push(labelled("Driver", drivers));
    }
    if let Some(e) = &a.error {
        col = col.push(text(e.clone()).size(11.0).role(Role::Danger).wrap());
    }
    let mut buttons = row![spacer()].spacing(8.0).width(Fill);
    if a.adding {
        buttons = buttons.push(spinner(14.0));
    }
    buttons = buttons.push(button("Cancel").cancel().on_press(Msg::CloseSheet));
    buttons = buttons.push(button("Add").primary().default().on_press(Msg::Submit).enabled(a.ready()));
    col = col.push(buttons);
    container(col).padding([20.0, 20.0, 16.0, 20.0]).width(460.0).into()
}

fn labelled(label: &str, field: impl Into<Element<Msg>>) -> Element<Msg> {
    column![text(label.to_owned()).size(11.0).role(Role::Text3), field.into()].spacing(4.0).width(Fill).into()
}

/// The sample: a network laser that's the default, an inkjet out of paper
/// with a job waiting, and a label printer paused.
pub fn sample() -> PrintersState {
    let printer = |name: &str, description: &str, location: &str, model: &str, uri: &str| Printer {
        name: name.into(),
        description: description.into(),
        location: location.into(),
        make_and_model: Some(model.into()),
        device_uri: Some(uri.into()),
        accepting: true,
        enabled: true,
        shared: Some(false),
        driverless: Some(true),
        ..Printer::default()
    };
    let mut laser = printer(
        "Brother_HL_L2390DW",
        "Brother HL-L2390DW",
        "Study",
        "Brother HL-L2390DW series - IPP Everywhere",
        "ipp://brother.local:631/ipp/print",
    );
    laser.is_user_default = true;
    laser.is_system_default = true;
    let mut inkjet = printer(
        "Canon_TS8350",
        "Canon TS8350",
        "Living room",
        "Canon TS8300 series - IPP Everywhere",
        "ipp://canon-ts8350.local:631/ipp/print",
    );
    inkjet.state = PrinterState::Stopped;
    inkjet.state_reasons = vec!["media-empty-error".into()];
    inkjet.state_message = Some("Media tray empty".into());
    let mut labels = printer(
        "DYMO_LabelWriter_450",
        "DYMO LabelWriter 450",
        "",
        "DYMO LabelWriter 450",
        "usb://DYMO/LabelWriter%20450?serial=01010112345600",
    );
    labels.state = PrinterState::Stopped;
    labels.state_reasons = vec!["paused".into()];
    labels.driverless = Some(false);
    PrintersState {
        loaded: true,
        available: true,
        printers: vec![laser, inkjet, labels],
        jobs: vec![Job {
            id: "Canon_TS8350-42".into(),
            number: 42,
            printer: "Canon_TS8350".into(),
            owner: "aiko".into(),
            size: 2_400_000,
            title: Some("Kohaku boarding pass.pdf".into()),
            state: JobState::Pending,
            ..Job::default()
        }],
        system_default: Some("Brother_HL_L2390DW".into()),
        user_default: Some("Brother_HL_L2390DW".into()),
        error: None,
    }
}

fn sample_devices() -> Vec<Device> {
    vec![
        Device {
            class: DeviceClass::Network,
            uri: "ipp://EPSON-ET-2850.local:631/ipp/print".into(),
            info: "EPSON ET-2850 Series".into(),
            make_and_model: Some("EPSON ET-2850 Series".into()),
            device_id: None,
            location: Some("Office".into()),
            driverless: true,
        },
        Device {
            class: DeviceClass::Direct,
            uri: "usb://HP/LaserJet%20P1102".into(),
            info: "HP LaserJet Professional P1102".into(),
            make_and_model: Some("HP LaserJet Professional P1102".into()),
            device_id: Some("MFG:HP;MDL:LaserJet Professional P1102;".into()),
            location: None,
            driverless: false,
        },
        Device {
            class: DeviceClass::Network,
            uri: "ipp".into(),
            info: "Internet Printing Protocol (ipp)".into(),
            ..Device::default()
        },
    ]
}

fn sample_drivers() -> Vec<Driver> {
    vec![
        Driver {
            name: "drv:///hpcups.drv/hp-laserjet_professional_p1102.ppd".into(),
            make_and_model: "HP LaserJet Professional P1102, hpcups 3.25".into(),
            language: Some("en".into()),
            device_id: None,
        },
        Driver {
            name: "drv:///sample.drv/generic.ppd".into(),
            make_and_model: "Generic PCL Laser Printer".into(),
            language: Some("en".into()),
            device_id: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(uri: &str) -> Device {
        Device { uri: uri.into(), ..Device::default() }
    }

    fn printer(name: &str, uri: &str) -> Printer {
        Printer { name: name.into(), device_uri: Some(uri.into()), ..Printer::default() }
    }

    #[test]
    fn a_service_is_named_from_its_uri() {
        assert_eq!(
            service_name("ipps://Aizome%20Test%20Printer._ipps._tcp.local/").as_deref(),
            Some("Aizome Test Printer")
        );
        assert_eq!(
            service_name("dnssd://Brother%20HL-L2390DW._ipp._tcp.local/?uuid=e3248000").as_deref(),
            Some("Brother HL-L2390DW")
        );
        assert_eq!(service_name("ipp://192.168.1.40/ipp/print"), None);
        assert_eq!(service_name("usb://Brother/HL"), None);
    }

    #[test]
    fn a_printer_cups_browsed_set_up_isnt_offered_again() {
        let found = device("driverless:ipps://Aizome%20Test%20Printer._ipps._tcp.local/");
        let browsed = printer("Aizome_Test_Printer", "implicitclass://Aizome_Test_Printer/");
        assert!(already_added(&found, &[browsed]));
        // Added before by its service's address, or the very same URI.
        let added = printer("Desk", "ipps://Aizome%20Test%20Printer._ipps._tcp.local/");
        assert!(already_added(&found, std::slice::from_ref(&added)));
        assert!(already_added(&device("ipp://10.0.0.5/ipp/print"), &[printer("Lab", "ipp://10.0.0.5/ipp/print")]));
    }

    #[test]
    fn other_printers_and_backends_still_are() {
        let found = device("dnssd://Office%20Laser._ipp._tcp.local/");
        let others = [
            printer("Aizome_Test_Printer", "implicitclass://Aizome_Test_Printer/"),
            printer("Lab", "ipp://10.0.0.5/ipp/print"),
        ];
        assert!(!already_added(&found, &others));
        // A backend that asks for an address is never "added".
        assert!(!already_added(&device("ipp"), &others));
    }
}

#[cfg(test)]
mod default_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use aizome_kit::testing::Harness;

    use super::Msg;
    use crate::app::{AppMsg, Settei, Start};

    #[test]
    fn only_the_printer_documents_go_to_says_default() {
        let dir = std::env::temp_dir().join(format!("settei-default-printer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = Harness::new(Settei::sample(dir.join("settings.toml"), Start::parse("printers")));
        let w = h.window();
        let badges = |h: &Harness<Settei>| h.texts(w).iter().filter(|t| *t == "Default").count();
        for _ in 0..20 {
            h.advance(Duration::from_millis(50));
        }
        assert_eq!(badges(&h), 1);
        // Yours becomes the Canon; everyone's stays the Brother.
        h.send(AppMsg::Pane("printers", Arc::new(Msg::Default(1))));
        for _ in 0..20 {
            h.advance(Duration::from_millis(50));
        }
        assert!(h.texts(w).iter().any(|t| t.starts_with("Yours; everyone's is")), "{:?}", h.texts(w));
        assert_eq!(badges(&h), 1, "{:?}", h.texts(w));
    }
}
