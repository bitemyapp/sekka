//! About: the mark, the computer's name (renamed here), the system and its
//! hardware in mono.

use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_sys::datetime::{Hostname, HostnameState};
use aizome_sys::sysinfo::{Chassis, CpuInfo, Disk, Gpu, Hardware, HostModel};

use super::Msg as G;
use crate::form;
use crate::pane::{Confirm, PaneCx, SettingEntry, ViewCx};
use crate::system::Cell;

pub enum Names {
    Live(Hostname),
    Sample(Cell<HostnameState>),
}

impl Names {
    fn snapshot(&self) -> Arc<HostnameState> {
        match self {
            Names::Live(h) => h.snapshot(),
            Names::Sample(c) => c.get(),
        }
    }
}

#[derive(Default)]
pub struct State {
    names: Option<Names>,
    hardware: Option<Arc<Hardware>>,
    /// The computer name as typed, before Return.
    draft: Option<String>,
    host_draft: Option<String>,
    /// The displays, as the compositor describes them.
    displays: Option<Vec<String>>,
    /// The system disk: free and total bytes.
    disk: Option<(u64, u64)>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Hardware(Arc<Hardware>),
    Name(String),
    SaveName,
    HostName(String),
    SaveHostName,
    ConfirmHostName(String),
    Done(&'static str, Result<(), String>),
    CopyDetails,
    Displays(Vec<String>),
    Disk(Option<(u64, u64)>),
    Open(&'static str, Option<&'static str>),
}

pub fn entries() -> Vec<SettingEntry> {
    vec![
        SettingEntry::new("computer-name", "Computer name").under("About").keywords(&[
            "rename",
            "pretty hostname",
            "name",
        ]),
        SettingEntry::new("hostname", "Host name").under("About").keywords(&["network name", "hostname"]),
        SettingEntry::new("hardware", "Hardware")
            .under("About")
            .keywords(&["cpu", "memory", "ram", "graphics", "gpu", "disk", "model"]),
        SettingEntry::new("displays-glance", "Displays and storage").under("About").keywords(&[
            "specs",
            "screens",
            "free space",
        ]),
    ]
}

/// The sample computer's hardware.
pub fn sample_hardware() -> Hardware {
    Hardware {
        cpu: CpuInfo {
            model: Some("Intel(R) Core(TM) Ultra 7 268V".into()),
            vendor: Some("GenuineIntel".into()),
            cores: Some(8),
            threads: 8,
        },
        memory: Some(33_241_268_224),
        gpus: vec![Gpu {
            card: "card1".into(),
            name: "Intel Arc 140V GPU (Lunar Lake)".into(),
            vendor_id: Some(0x8086),
            device_id: Some(0x64a0),
            vendor_name: Some("Intel Corporation".into()),
            device_name: Some("Lunar Lake [Intel Arc Graphics 130V / 140V]".into()),
            driver: Some("xe".into()),
            vram: None,
            boot_vga: true,
            bus_address: Some("0000:00:02.0".into()),
        }],
        disks: vec![Disk {
            name: "nvme0n1".into(),
            model: Some("SAMSUNG MZVL81T0HFLB-00BLL".into()),
            vendor: None,
            size: 1_024_209_543_168,
            rotational: false,
            removable: false,
        }],
        os: aizome_sys::sysinfo::parse_os_release(
            "NAME=Sekka\nID=sekka\nID_LIKE=nixos\nPRETTY_NAME=\"Sekka 26.11 (Zokor)\"\nVERSION=\"26.11 (Zokor)\"\nVERSION_ID=26.11\nBUILD_ID=26.11.20261011.7fd4004\n",
        ),
        kernel: Some("7.2.9".into()),
        architecture: "x86_64".into(),
        model: HostModel {
            vendor: Some("LENOVO".into()),
            product_name: Some("21NX0013US".into()),
            product_version: Some("ThinkPad X1 Carbon Gen 13 Aura Edition".into()),
            product_family: Some("ThinkPad X1 Carbon Gen 13 Aura Edition".into()),
            chassis_type: Some(10),
            chassis: Chassis::Laptop,
            ..HostModel::default()
        },
        hostname: crate::system::sample::HOSTNAME.into(),
        pretty_hostname: Some(crate::system::sample::PRETTY_HOSTNAME.into()),
    }
}

fn sample_names() -> HostnameState {
    HostnameState {
        available: true,
        hostname: crate::system::sample::HOSTNAME.into(),
        static_hostname: crate::system::sample::HOSTNAME.into(),
        static_hostname_managed: true,
        pretty_hostname: crate::system::sample::PRETTY_HOSTNAME.into(),
        pretty_hostname_managed: false,
        chassis: "laptop".into(),
        icon_name: "computer-laptop".into(),
        os_pretty_name: "Sekka 26.11 (Zokor)".into(),
        kernel_name: "Linux".into(),
        kernel_release: "7.2.9".into(),
        hardware_vendor: "Lenovo".into(),
        hardware_model: "ThinkPad X1 Carbon Gen 13 Aura Edition".into(),
        ..HostnameState::default()
    }
}

/// Bytes as people say a memory size: 32 GB for 31.0 GiB usable.
pub fn memory_size(bytes: u64) -> String {
    let gib = bytes as f64 / (1u64 << 30) as f64;
    // Firmware and the kernel keep some; round up to what was installed.
    let installed = if gib > 6.0 { (gib / 2.0).ceil() * 2.0 } else { gib.ceil() };
    format!("{installed:.0} GB")
}

/// Bytes as a disk's maker says them (decimal): 1 TB, 512 GB.
pub fn disk_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e12 {
        let t = b / 1e12;
        if (t - t.round()).abs() < 0.06 { format!("{:.0} TB", t.round()) } else { format!("{t:.1} TB") }
    } else if b >= 1e9 {
        format!("{:.0} GB", b / 1e9)
    } else {
        format!("{:.0} MB", b / 1e6)
    }
}

/// "Samsung MZVL81T0HFLB" from a disk's model.
fn disk_name(d: &Disk) -> String {
    let model = d.model.clone().unwrap_or_else(|| d.name.clone());
    let model = model.trim().to_owned();
    let pretty = match model.split_whitespace().next() {
        Some(w) if w.chars().all(|c| c.is_ascii_uppercase()) && w.len() > 2 => {
            let mut c = w.to_lowercase();
            c[..1].make_ascii_uppercase();
            model.replacen(w, &c, 1)
        }
        _ => model,
    };
    match &d.vendor {
        Some(v) if !pretty.to_lowercase().contains(&v.to_lowercase()) => format!("{v} {pretty}"),
        _ => pretty,
    }
}

/// The system's name and version: "Sekka 26.11 (Zokor)".
pub fn system_name(hw: &Hardware) -> String {
    hw.os.pretty_name.clone()
}

/// The rows of hardware, label and value.
pub fn specs(hw: &Hardware) -> Vec<(&'static str, String)> {
    let mut v = Vec::new();
    if let Some(m) = hw.model.display_name() {
        v.push(("Model", m));
    }
    let cpu = hw.cpu.display_name().unwrap_or_else(|| "Unknown".into());
    let cores = match (hw.cpu.cores, hw.cpu.threads) {
        (Some(c), t) if t > c => format!(" · {c} cores, {t} threads"),
        (Some(c), _) => format!(" · {c} cores"),
        (None, t) if t > 0 => format!(" · {t} threads"),
        _ => String::new(),
    };
    v.push(("Processor", format!("{cpu}{cores}")));
    if let Some(m) = hw.memory {
        v.push(("Memory", memory_size(m)));
    }
    for g in &hw.gpus {
        v.push(("Graphics", g.name.clone()));
    }
    for d in hw.disks.iter().filter(|d| !d.removable) {
        v.push(("Disk", format!("{} · {}", disk_size(d.size), disk_name(d))));
    }
    let kernel = hw.kernel.clone().unwrap_or_default();
    v.push(("Kernel", format!("Linux {kernel} · {}", hw.architecture)));
    v.push(("Desktop", format!("Aizome {}", env!("CARGO_PKG_VERSION"))));
    if let Some(b) = &hw.os.build_id {
        v.push(("Build", b.clone()));
    }
    v
}

impl State {
    pub fn shown(&mut self, cx: &mut PaneCx<G>) {
        if self.names.is_none() {
            self.names = Some(if cx.sample() {
                Names::Sample(Cell::new(sample_names(), cx.wake()))
            } else {
                Names::Live(Hostname::new(cx.wake()))
            });
        }
        if self.hardware.is_none() {
            if cx.sample() {
                self.hardware = Some(Arc::new(sample_hardware()));
            } else {
                cx.spawn(|| G::About(Msg::Hardware(Arc::new(aizome_sys::sysinfo::hardware()))));
            }
        }
        // At a glance: the displays and the disk, read again each time.
        if cx.sample() {
            self.displays = Some(vec![
                "Built-in display · 2880 × 1800 at 120 Hz".into(),
                "Dell U2723QE · 3840 × 2160 at 60 Hz".into(),
            ]);
            self.disk = Some((412_000_000_000, 1_000_000_000_000));
        } else {
            cx.compositor(aizome_ipc::Request::Outputs, |r| {
                G::About(Msg::Displays(match r {
                    Ok(aizome_ipc::Response::Outputs { outputs }) => {
                        outputs.iter().filter(|o| o.enabled).map(display_line).collect()
                    }
                    _ => Vec::new(),
                }))
            });
            cx.spawn(|| {
                G::About(Msg::Disk(
                    super::storage::statvfs(std::path::Path::new("/")).map(|(total, free)| (free, total)),
                ))
            });
        }
    }

    fn names(&self) -> Arc<HostnameState> {
        self.names.as_ref().map(Names::snapshot).unwrap_or_default()
    }

    /// The computer's name to show: pretty, else static, else the
    /// hardware's.
    pub fn computer_name(&self) -> String {
        let n = self.names();
        if !n.pretty_hostname.is_empty() {
            return n.pretty_hostname.clone();
        }
        if let Some(hw) = &self.hardware {
            if let Some(p) = &hw.pretty_hostname {
                return p.clone();
            }
            if !hw.hostname.is_empty() {
                return hw.hostname.clone();
            }
        }
        n.hostname.clone()
    }

    pub fn update(&mut self, msg: Msg, cx: &mut PaneCx<G>) {
        match msg {
            Msg::Hardware(hw) => self.hardware = Some(hw),
            Msg::Displays(d) => self.displays = Some(d),
            Msg::Disk(d) => self.disk = d,
            Msg::Open(pane, entry) => cx.show_pane(pane, entry),
            Msg::Name(s) => self.draft = Some(s),
            Msg::SaveName => {
                let Some(name) = self.draft.take() else { return };
                let name = name.trim().to_owned();
                let task = match &self.names {
                    Some(Names::Live(h)) => h.set_pretty_hostname(&name),
                    Some(Names::Sample(c)) => {
                        let n = name.clone();
                        c.update(move |s| s.pretty_hostname = n)
                    }
                    None => return,
                };
                cx.wait(task, |r| G::About(Msg::Done("Renamed the computer", r.map_err(|e| e.to_string()))));
            }
            Msg::HostName(s) => self.host_draft = Some(s),
            Msg::SaveHostName => {
                let Some(name) = self.host_draft.take() else { return };
                let name = name.trim().to_lowercase();
                if name == self.names().static_hostname {
                    return;
                }
                if !aizome_sys::datetime::valid_hostname(&name) {
                    cx.failed(
                        "Host name",
                        "letters, digits and hyphens, up to 63 of them, not starting or ending with a hyphen",
                    );
                    return;
                }
                cx.confirm(
                    Confirm::new(
                        format!("Change the host name to “{name}”?"),
                        "Other computers on the network will know this one by the new name, and some programs need restarting to notice.",
                        "Change",
                        G::About(Msg::ConfirmHostName(name)),
                    ),
                );
            }
            Msg::ConfirmHostName(name) => {
                let task = match &self.names {
                    Some(Names::Live(h)) => h.set_static_hostname(&name),
                    Some(Names::Sample(c)) => {
                        let n = name.clone();
                        c.update(move |s| {
                            s.static_hostname = n.clone();
                            s.hostname = n;
                        })
                    }
                    None => return,
                };
                cx.wait(task, |r| G::About(Msg::Done("Changed the host name", r.map_err(|e| e.to_string()))));
            }
            Msg::Done(what, Ok(())) => cx.say(what),
            Msg::Done(what, Err(e)) => cx.failed(what, e),
            Msg::CopyDetails => {
                if let Some(hw) = &self.hardware {
                    let mut s = format!("{}\n{}\n", self.computer_name(), system_name(hw));
                    for (k, v) in specs(hw) {
                        s.push_str(&format!("{k:<10} {v}\n"));
                    }
                    cx.set_clipboard_text(&s);
                    cx.say("Copied the computer's details");
                }
            }
        }
    }

    pub fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let n = self.names();
        let hero = self.hero(true);
        let spec_rows: Vec<form::Row<Msg>> = match &self.hardware {
            None => {
                vec![form::whole(container(row![progress_indeterminate().width(120.0)]).padding([14.0, form::INSET]))]
            }
            Some(hw) => specs(hw)
                .into_iter()
                .map(|(k, v)| form::row(k, text(v).mono().size(12.0).role(Role::Text).selectable()).compact())
                .collect(),
        };
        let mut spec_rows = spec_rows;
        if !spec_rows.is_empty() {
            let first = spec_rows.remove(0);
            spec_rows.insert(0, first.entry("hardware"));
        }

        let pretty = self.draft.clone().unwrap_or_else(|| self.computer_name());
        let name_row = if n.pretty_hostname_managed {
            form::managed(
                "Computer name",
                self.computer_name(),
                "Its file, /etc/machine-info, comes from the configuration.",
            )
        } else {
            form::row(
                "Computer name",
                text_field(&pretty)
                    .on_change(Msg::Name)
                    .on_submit(Msg::SaveName)
                    .on_blur(Msg::SaveName)
                    .placeholder("A name people see")
                    .width(240.0),
            )
            .help("What people see: in the menu bar's panels, on the network, in Bluetooth.")
        }
        .entry("computer-name");
        let host = if n.static_hostname.is_empty() {
            self.hardware.as_ref().map(|h| h.hostname.clone()).unwrap_or_default()
        } else {
            n.static_hostname.clone()
        };
        let host_row = if n.static_hostname_managed || !n.available {
            form::managed("Host name", host, "Change networking.hostName and rebuild.")
        } else {
            form::row(
                "Host name",
                text_field(self.host_draft.as_deref().unwrap_or(&host))
                    .mono()
                    .on_change(Msg::HostName)
                    .on_submit(Msg::SaveHostName)
                    .width(240.0),
            )
            .help("The computer's name on the network: letters, digits and hyphens.")
        }
        .entry("hostname");

        // At a glance, each with the pane that changes it.
        let mut glance: Vec<form::Row<Msg>> = Vec::new();
        let displays = self.displays.clone().unwrap_or_default();
        let shown =
            if displays.is_empty() { vec!["Not known: the compositor didn't say".to_owned()] } else { displays };
        let list = column(shown.into_iter().map(|d| text(d).mono().size(11.0).role(Role::Text2).into()).collect())
            .spacing(3.0);
        glance.push(
            form::row(
                "Displays",
                row![list, button("Displays…").compact().on_press(Msg::Open("displays", None))]
                    .spacing(12.0)
                    .align(Align::Center),
            )
            .leading(icon(Icon::Monitor).size(16.0).role(Role::Text2))
            .entry("displays-glance"),
        );
        if let Some((free, total)) = self.disk {
            let used = total.saturating_sub(free);
            let bar =
                crate::widgets::bar(vec![(used as f64, crate::widgets::Segment::Ai)], total as f64, 8.0).width(120.0);
            glance.push(
                form::row(
                    "Storage",
                    row![
                        bar,
                        text(format!("{} free of {}", super::storage::size(free), super::storage::size(total)))
                            .mono()
                            .size(11.0)
                            .role(Role::Text2),
                        button("Storage…").compact().on_press(Msg::Open("general", Some("storage"))),
                    ]
                    .spacing(12.0)
                    .align(Align::Center),
                )
                .leading(icon(Icon::HardDrive).size(16.0).role(Role::Text2)),
            );
        }
        form::page(vec![
            hero,
            vcx.section("Name", vec![name_row, host_row]),
            vcx.section_with("Hardware", spec_rows, None),
            vcx.section("At a Glance", glance),
            row![spacer(), button("Copy Details").icon(Icon::Copy).on_press(Msg::CopyDetails)].width(Fill).into(),
        ])
    }

    /// The mark, the computer's name and its system, centred.
    pub fn hero(&self, big: bool) -> Element<Msg> {
        let module = if big { 12.0 } else { 8.0 };
        let name = self.computer_name();
        let system = self.hardware.as_ref().map(|h| system_name(h)).unwrap_or_default();
        let model = self.hardware.as_ref().and_then(|h| h.model.display_name()).unwrap_or_default();
        let mut col = column![
            crate::widgets::mark::<Msg>(module),
            space(if big { 10.0 } else { 4.0 }),
            text(if name.is_empty() { "This computer".to_owned() } else { name })
                .size(if big { 22.0 } else { 17.0 })
                .weight(650.0),
        ]
        .spacing(4.0)
        .align(Align::Center);
        if !system.is_empty() {
            col = col.push(text(system).role(Role::Text2));
        }
        if !model.is_empty() {
            col = col.push(text(model).mono().size(11.0).role(Role::Text3));
        }
        container(col).padding([if big { 12.0 } else { 6.0 }, 0.0]).width(Fill).align_x(Align::Center).into()
    }
}

/// The main page's head: the mark, the name, the system.
pub fn summary(state: &State, _vcx: &ViewCx) -> Element<G> {
    let hero = state.hero(false).map(G::About);
    crate::widgets::press(hero)
        .style(crate::widgets::PressStyle::Plain)
        .on_press(G::Page(super::Page::About))
        .named("About")
        .width(Fill)
        .into()
}

/// "BOE NE135A1M · 2880 × 1800 at 120 Hz", or "Built-in display" for a
/// laptop's panel.
pub fn display_line(o: &aizome_ipc::OutputInfo) -> String {
    let name = if o.name.starts_with("eDP") || o.name.starts_with("LVDS") || o.name.starts_with("DSI") {
        "Built-in display".to_owned()
    } else if o.make.is_empty() && o.model.is_empty() {
        o.name.clone()
    } else {
        format!("{} {}", o.make, o.model).trim().to_owned()
    };
    match &o.current_mode {
        Some(m) => format!("{name} · {} × {} at {:.0} Hz", m.width, m.height, f64::from(m.refresh) / 1000.0),
        None => name,
    }
}
