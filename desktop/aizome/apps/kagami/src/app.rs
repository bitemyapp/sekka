use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::backend::{self, Backend, Device, Event, Request, Status};
use crate::bluetooth;
use crate::tracking::{Tracker, strongest_first};

#[derive(Clone)]
pub enum Msg {
    Backend(Event),
    Bluetooth(bluetooth::Snapshot),
    Peer(crate::peer::Event),
    StartPeer,
    StopPeer,
    RestartSender,
    ShowDiscovery,
    InspectBeacon(String),
    TrackBeacon(String),
    StopTracking,
    SortBySignal(bool),
    Tick,
    CopyDiscovery,
    CopyBeacon(String),
    Connect(Device),
    Address(String),
    ConnectAddress,
    Code(String),
    SubmitCode,
    Disconnect,
    Mute,
    Close,
}

pub struct Kagami {
    backend: Option<Backend>,
    bluetooth: Option<crate::bluetooth::Discovery>,
    peer: Option<crate::peer::Session>,
    peer_probe: serde_json::Value,
    peer_report: serde_json::Value,
    peer_devices: Vec<Device>,
    peer_status: String,
    start_backend: bool,
    ready: bool,
    closing: bool,
    status: Status,
    devices: Vec<Device>,
    network_devices: Vec<Device>,
    bluetooth_devices: Vec<Device>,
    discovery: bluetooth::Snapshot,
    discovery_window: Option<WindowKey>,
    inspected_beacon: Option<String>,
    tracker: Option<Tracker>,
    sort_by_signal: bool,
    open_discovery_on_launch: bool,
    selected: Option<Device>,
    address: String,
    code: String,
    error: Option<String>,
    autotest: Option<crate::battery::Autotest>,
}

impl Kagami {
    pub fn new() -> Self {
        Self {
            backend: None,
            bluetooth: None,
            peer: None,
            peer_probe: serde_json::Value::Null,
            peer_report: serde_json::Value::Null,
            peer_devices: Vec::new(),
            peer_status: String::new(),
            start_backend: true,
            ready: false,
            closing: false,
            status: Status::default(),
            devices: Vec::new(),
            network_devices: Vec::new(),
            bluetooth_devices: Vec::new(),
            discovery: bluetooth::Snapshot::default(),
            discovery_window: None,
            inspected_beacon: None,
            tracker: None,
            sort_by_signal: true,
            open_discovery_on_launch: false,
            selected: None,
            address: String::new(),
            code: String::new(),
            error: None,
            autotest: crate::battery::Autotest::from_env(),
        }
    }

    pub fn with_discovery_details(mut self, open: bool) -> Self {
        self.open_discovery_on_launch = open;
        self
    }

    fn discovery_view(&self) -> Element<Msg> {
        let mut content: Vec<Element<Msg>> = vec![
            row![text("Discovery Details").size(26.0).weight(650.0).width(Length::Fill),
                button("Copy All Results").on_press(Msg::CopyDiscovery)]
                .spacing(16.0).width(Length::Fill).into(),
            checkbox("Sort by signal strength", self.sort_by_signal).on_toggle(Msg::SortBySignal).into(),
            text("With signal sorting off, beacons stay in Bluetooth address order. Track a beacon to keep it fixed while you move. RSSI closer to zero means a stronger signal, not an exact distance. Cached values are marked until BlueZ reports them again.")
                .wrap().role(Role::Text2).into(),
            text(if self.discovery.scan_status.is_empty() { "Starting Bluetooth discovery…" } else { &self.discovery.scan_status })
                .wrap().role(Role::Text2).into(),
            text(format!("Bluetooth · {} AirPlay beacons", self.discovery.records.len())).size(20.0).weight(600.0).into(),
        ];
        if self.tracker.is_some() {
            content.clear();
        }
        if self.discovery.records.is_empty() {
            content.push(text("No AirPlay target advertisements in the current snapshot.").wrap().into());
        }
        let records: Vec<_> = if let Some(tracker) = &self.tracker {
            vec![&tracker.record]
        } else {
            self.discovery.records.iter().collect()
        };
        for record in records {
            let expanded = self.tracker.is_some() || self.inspected_beacon.as_ref() == Some(&record.path);
            let rssi = record.rssi_dbm.map(|r| format!("{r} dBm")).unwrap_or("RSSI unavailable".into());
            let endpoint = record
                .advertisement
                .as_ref()
                .map(|a| format!("{}:{}", a.ip, a.port))
                .unwrap_or("Undecodable endpoint".into());
            content.push(
                row![
                    column![
                        text(record.title()).weight(650.0).wrap(),
                        text(format!("{} · {} · {rssi}", record.address, record.address_type)).role(Role::Text2).wrap(),
                        text(format!("Advertised endpoint: {endpoint}")).role(Role::Text2).wrap(),
                        text(report_age(record)).role(Role::Text3).wrap(),
                        text(record.probe.status.as_str()).wrap(),
                    ]
                    .spacing(4.0)
                    .width(Length::Fill),
                    column![
                        button(if self.tracker.is_some() { "Tracking" } else { "Track" })
                            .enabled(self.tracker.is_none())
                            .on_press(Msg::TrackBeacon(record.path.clone())),
                        button(if expanded { "Hide Details" } else { "Details" })
                            .enabled(self.tracker.is_none())
                            .on_press(Msg::InspectBeacon(record.path.clone())),
                    ]
                    .spacing(8.0),
                ]
                .align(Align::Start)
                .spacing(14.0)
                .width(Length::Fill)
                .key(record.path.clone())
                .into(),
            );
            if expanded {
                content.push(button("Copy This Result").on_press(Msg::CopyBeacon(record.path.clone())).into());
                content.push(
                    text(format!(
                        "Bluetooth name: {}\nBlueZ alias: {}\nAdapter: {}\nTX power: {}",
                        if record.name.is_empty() { "not supplied" } else { &record.name },
                        if record.alias.is_empty() { "not supplied" } else { &record.alias },
                        record.adapter,
                        record.tx_power_dbm.map(|v| format!("{v} dBm")).unwrap_or("not supplied".into())
                    ))
                    .wrap()
                    .selectable()
                    .into(),
                );
                if let Some(a) = &record.advertisement {
                    content.push(
                        text(format!(
                            "AirPlay type 0x09 · flags 0x{:02x} · seed 0x{:02x}\nIPv4 {} · port {}",
                            a.flags, a.seed, a.ip, a.port
                        ))
                        .mono()
                        .size(13.0)
                        .wrap()
                        .selectable()
                        .into(),
                    );
                }
                if let Some(awdl) = &record.awdl_advertisement_hex {
                    content.push(text(format!("Peer-to-peer Wi-Fi advertisement (type 0x16): {awdl}\nThis advertises an Apple direct-link discovery path. Use Direct Wi-Fi can try that path; this beacon alone does not identify the TV."))
                        .wrap().selectable().into());
                }
                if !record.parse_error.is_empty() {
                    content.push(text(record.parse_error.as_str()).wrap().into());
                }
                content.push(
                    text(format!("Apple manufacturer data (0x004c):\n{}", record.manufacturer_hex))
                        .mono()
                        .size(13.0)
                        .wrap()
                        .selectable()
                        .into(),
                );
                if let Some(device) = &record.probe.device {
                    content.push(
                        text(format!(
                            "/info name: {}\nModel: {}\nDevice ID: {}",
                            device.name, device.model, device.device_id
                        ))
                        .wrap()
                        .selectable()
                        .into(),
                    );
                }
                if !record.probe.info_xml.is_empty() {
                    content.push(text("Full /info response (decoded plist)").weight(600.0).into());
                    content.push(text(record.probe.info_xml.as_str()).mono().size(12.0).wrap().selectable().into());
                }
            }
            content.push(divider().into());
        }
        if self.tracker.is_none() {
            content.push(text("Direct Wi-Fi").size(20.0).weight(600.0).into());
            content.push(
                text(
                    serde_json::to_string_pretty(&serde_json::json!({"capabilities": &self.peer_probe,
                "radio": &self.peer_report, "receivers": &self.peer_devices}))
                    .unwrap_or_default(),
                )
                .mono()
                .size(12.0)
                .wrap()
                .selectable()
                .into(),
            );
            content.push(
                text(format!("Bonjour · {} receivers", self.network_devices.len())).size(20.0).weight(600.0).into(),
            );
            content.push(
                text("These are the discovery records returned by the sender. They may overlap the Bluetooth results.")
                    .wrap()
                    .role(Role::Text2)
                    .into(),
            );
            if self.network_devices.is_empty() {
                content.push(text("No Bonjour receivers returned.").into());
            }
            for device in &self.network_devices {
                content.push(
                    text(serde_json::to_string_pretty(device).unwrap_or_default())
                        .mono()
                        .size(13.0)
                        .wrap()
                        .selectable()
                        .into(),
                );
            }
        }
        let body = scroll(column(content).spacing(16.0).padding(24.0).width(Length::Fill))
            .key(if self.tracker.is_some() { "tracked-beacon-scroll" } else { "all-beacons-scroll" })
            .height(Length::Fill);
        if let Some(tracker) = &self.tracker {
            let rssi = tracker.record.rssi_dbm.map(|v| format!("{v} dBm")).unwrap_or("No RSSI".into());
            let trend = if let (Some(baseline), Some(current)) = (tracker.baseline_dbm, tracker.record.rssi_dbm) {
                let strongest = tracker.samples.iter().map(|s| s.rssi_dbm).max().unwrap_or(current);
                let weakest = tracker.samples.iter().map(|s| s.rssi_dbm).min().unwrap_or(current);
                format!(
                    "{:+} dB since tracking began · recent range {weakest} to {strongest} dBm",
                    i32::from(current) - i32::from(baseline)
                )
            } else {
                "Waiting for a fresh Bluetooth report; the displayed signal may be cached.".into()
            };
            let presence = if tracker.present {
                report_age(&tracker.record)
            } else {
                "Beacon absent from the current snapshot. Keeping the last result; tracking will resume only for this Bluetooth identity.".into()
            };
            column![
                column![
                    row![
                        text("Tracking Beacon").size(24.0).weight(650.0).width(Length::Fill),
                        button("Back to All Signals").on_press(Msg::StopTracking)
                    ]
                    .spacing(12.0)
                    .width(Length::Fill),
                    text(format!("{} · {}", tracker.record.title(), tracker.record.address)).wrap(),
                    text(rssi).size(42.0).weight(650.0),
                    text(trend).wrap().role(Role::Text2),
                    text(presence).wrap().role(Role::Text2),
                    button("Copy Tracking Results").on_press(Msg::CopyDiscovery),
                ]
                .spacing(8.0)
                .padding(24.0)
                .width(Length::Fill),
                divider(),
                body,
            ]
            .height(Length::Fill)
            .into()
        } else {
            body.into()
        }
    }

    fn active(&self) -> bool {
        matches!(self.status.state.as_str(), "connecting" | "pin_required" | "streaming" | "stopping")
    }

    fn send(&self, request: Request) {
        if let Some(backend) = &self.backend {
            backend.request(request);
        }
    }

    fn launch_sender(&mut self, cx: &mut Cx<Msg>) {
        let sender = cx.stream();
        self.backend = Some(Backend::start(move |event| {
            sender.send(Msg::Backend(event));
        }));
    }

    fn connect(&mut self, device: Device) {
        if let Ok(target) = std::env::var("SEKKA_AIRPLAY_TEST_TARGET") {
            if device.name != target {
                self.error = Some(format!("This test is restricted to {target}."));
                return;
            }
        }
        if !self.ready || self.active() || device.ip.is_empty() || device.port == 0 {
            return;
        }
        self.error = None;
        self.code.clear();
        self.send(Request::connect(&device, String::new()));
        self.selected = Some(device);
        self.status.state = "connecting".into();
        if self.autotest.is_some() {
            eprintln!("[airplay-state] {}", serde_json::json!({"state":"connecting", "attemptStarted":true}));
        }
    }

    fn name(&self) -> &str {
        self.selected.as_ref().map(|d| d.name.as_str()).unwrap_or("TV")
    }

    fn code_valid(&self) -> bool {
        if self.status.credential_kind == "password" {
            !self.code.is_empty() && self.code.len() <= 256
        } else {
            (4..=8).contains(&self.code.len()) && self.code.bytes().all(|c| c.is_ascii_digit())
        }
    }

    fn merge_devices(&mut self) {
        let mut devices = self.network_devices.clone();
        for device in self.bluetooth_devices.iter().chain(&self.peer_devices) {
            if !devices.iter().any(|d| d.ip == device.ip && d.port == device.port) {
                devices.push(device.clone());
            }
        }
        devices.sort_by_key(|d| (d.name.to_lowercase(), d.ip.clone(), d.port));
        devices.dedup_by(|a, b| a.ip == b.ip && a.port == b.port);
        self.devices = devices;
    }

    fn sort_records(&self, records: &mut [bluetooth::Record]) {
        if self.sort_by_signal {
            strongest_first(records);
        } else {
            records.sort_by(|a, b| a.address.cmp(&b.address).then(a.path.cmp(&b.path)));
        }
    }
}

impl Program for Kagami {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("computer.sekka.AirPlay", "AirPlay", "Screen Mirroring")
            .icon(Icon::Tv)
            .version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("AirPlay — Screen Mirroring").size(580.0, 620.0).min_size(460.0, 460.0));
        cx.every(std::time::Duration::from_secs(1), Msg::Tick);
        if self.open_discovery_on_launch {
            self.update(Msg::ShowDiscovery, cx);
        }
        if self.start_backend {
            self.launch_sender(cx);
            let sender = cx.stream();
            self.bluetooth = Some(crate::bluetooth::Discovery::start(move |devices| {
                sender.send(Msg::Bluetooth(devices));
            }));
            let sender = cx.stream();
            crate::peer::probe(move |event| {
                sender.send(Msg::Peer(event));
            });
        }
        if self.autotest.is_some() {
            self.update(Msg::StartPeer, cx);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::StartPeer if self.peer.is_none() && !self.active() => {
                if self.backend.is_none() && self.start_backend {
                    self.launch_sender(cx);
                }
                self.peer_status = "Waiting for administrator authentication…".into();
                self.peer_report = serde_json::json!({});
                self.error = None;
                let sender = cx.stream();
                self.peer = Some(crate::peer::Session::start(move |event| {
                    sender.send(Msg::Peer(event));
                }));
            }
            Msg::StartPeer => {}
            Msg::RestartSender if self.backend.is_none() && !self.closing => {
                self.error = None;
                self.launch_sender(cx);
            }
            Msg::RestartSender => {}
            Msg::StopPeer => {
                if self.active() {
                    self.send(Request::new("disconnect"));
                }
                if let Some(peer) = &self.peer {
                    peer.stop();
                }
                self.peer_status = "Restoring normal Wi-Fi…".into();
            }
            Msg::Peer(crate::peer::Event::Probe(value)) => self.peer_probe = value,
            Msg::Peer(crate::peer::Event::Devices(devices)) => {
                self.peer_devices = devices;
                self.merge_devices();
            }
            Msg::Peer(crate::peer::Event::Status(value)) => {
                match value["state"].as_str() {
                    Some("starting") => self.peer_status = "Starting the direct Wi-Fi connection…".into(),
                    Some("up") => self.peer_status = "Direct Wi-Fi is active. Looking for nearby TVs…".into(),
                    Some("stopping") => self.peer_status = "Restoring normal Wi-Fi…".into(),
                    Some("idle") => self.peer_status = "Finishing Direct Wi-Fi cleanup…".into(),
                    Some("error" | "discovery_error" | "bluetooth_notice") => {
                        self.error = value["error"].as_str().map(str::to_owned);
                    }
                    _ => {}
                }
                let state = value["state"].as_str().unwrap_or("unknown").to_owned();
                if !self.peer_report.is_object() {
                    self.peer_report = serde_json::json!({});
                }
                self.peer_report[state] = value;
            }
            Msg::Peer(crate::peer::Event::Stopped) => {
                self.peer.take();
                self.peer_status = "Direct Wi-Fi ended.".into();
                if std::env::var_os("SEKKA_AIRPLAY_RADIO_RELAY").is_some() {
                    self.closing = true;
                    self.code.clear();
                    if let Some(bluetooth) = &self.bluetooth {
                        bluetooth.stop();
                    }
                    if let Some(backend) = &self.backend {
                        backend.shutdown();
                    }
                }
                self.peer_devices.clear();
                self.merge_devices();
                if self.closing && !self.ready {
                    cx.quit();
                }
            }
            Msg::Tick => {
                let ready = self.ready && !self.active() && !self.closing;
                if let Some(test) = &mut self.autotest {
                    if let Some(device) = test.next(&self.peer_devices, ready) {
                        self.connect(device);
                    }
                }
            }
            Msg::Backend(Event::Ready) => self.ready = true,
            Msg::Backend(Event::Snapshot(status, devices)) => {
                if self.autotest.is_some() && (status.state != self.status.state || status.error != self.status.error) {
                    // Fixed status fields only; credentials and requests are never logged.
                    eprintln!(
                        "[airplay-state] {}",
                        serde_json::json!({"state":status.state,
                        "error":status.error, "hasAudio":status.has_audio})
                    );
                }
                let new_prompt = status.state == "pin_required" && self.status.state != "pin_required";
                if !status.error.is_empty() {
                    self.error = Some(status.error.clone());
                }
                self.network_devices = devices;
                self.merge_devices();
                self.status = status;
                if new_prompt {
                    self.code.clear();
                    cx.focus("airplay-code");
                }
                if self.status.state == "idle" {
                    self.code.clear();
                }
            }
            Msg::Backend(Event::Error(error)) => self.error = Some(error),
            Msg::Bluetooth(mut snapshot) => {
                self.sort_records(&mut snapshot.records);
                if let Some(tracker) = &mut self.tracker {
                    tracker.update(&snapshot);
                }
                self.bluetooth_devices = snapshot.devices();
                self.discovery = snapshot;
                self.merge_devices();
            }
            Msg::ShowDiscovery => {
                if let Some(window) = self.discovery_window {
                    cx.activate(window);
                } else {
                    self.discovery_window = Some(cx.open_window(
                        WindowSpec::new("AirPlay — Discovery Details").size(850.0, 700.0).min_size(620.0, 460.0),
                    ));
                }
            }
            Msg::InspectBeacon(path) => {
                self.inspected_beacon = if self.inspected_beacon.as_ref() == Some(&path) { None } else { Some(path) };
            }
            Msg::TrackBeacon(path) => {
                if let Some(record) = self.discovery.records.iter().find(|r| r.path == path) {
                    self.tracker = Some(Tracker::new(record.clone()));
                }
            }
            Msg::StopTracking => self.tracker = None,
            Msg::SortBySignal(enabled) => {
                self.sort_by_signal = enabled;
                let mut records = std::mem::take(&mut self.discovery.records);
                self.sort_records(&mut records);
                self.discovery.records = records;
            }
            Msg::CopyDiscovery => {
                let report = serde_json::json!({"bluetooth": &self.discovery, "bonjour": &self.network_devices, "tracking": &self.tracker,
                    "direct_wifi": {"capabilities": &self.peer_probe, "radio": &self.peer_report, "receivers": &self.peer_devices}});
                if let Ok(json) = serde_json::to_string_pretty(&report) {
                    cx.set_clipboard_text(&json);
                }
            }
            Msg::CopyBeacon(path) => {
                if let Some(record) = self
                    .discovery
                    .records
                    .iter()
                    .find(|r| r.path == path)
                    .or_else(|| self.tracker.as_ref().map(|t| &t.record).filter(|r| r.path == path))
                {
                    if let Ok(json) = serde_json::to_string_pretty(record) {
                        cx.set_clipboard_text(&json);
                    }
                }
            }
            Msg::Backend(Event::Stopped) => {
                self.ready = false;
                self.backend.take();
                self.network_devices.clear();
                self.merge_devices();
                self.status.state = "idle".into();
                self.code.clear();
                if self.closing && self.peer.is_none() {
                    cx.quit();
                }
            }
            Msg::Connect(device) => self.connect(device),
            Msg::Address(value) => self.address = value,
            Msg::ConnectAddress => match backend::manual_device(&self.address) {
                Ok(device) => self.connect(device),
                Err(error) => self.error = Some(error),
            },
            Msg::Code(value) => self.code = value,
            Msg::SubmitCode if self.code_valid() && self.status.state == "pin_required" => {
                if let Some(device) = &self.selected {
                    let code = std::mem::take(&mut self.code);
                    self.send(Request::connect(device, code));
                    self.error = None;
                    self.status.state = "connecting".into();
                }
            }
            Msg::SubmitCode => {}
            Msg::Disconnect if self.active() => {
                self.send(Request::new("disconnect"));
                self.status.state = "stopping".into();
                self.code.clear();
            }
            Msg::Disconnect => {}
            Msg::Mute if self.status.state == "streaming" && self.status.has_audio => {
                self.send(Request::new(if self.status.audio_muted { "unmute" } else { "mute" }));
            }
            Msg::Mute => {}
            Msg::Close => {
                self.closing = true;
                self.code.clear();
                if let Some(bluetooth) = &self.bluetooth {
                    bluetooth.stop();
                }
                if let Some(backend) = &self.backend {
                    backend.shutdown();
                }
                if let Some(peer) = &self.peer {
                    peer.stop();
                }
                if !self.ready && self.peer.is_none() {
                    cx.quit();
                }
            }
        }
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if self.discovery_window == Some(window) {
            self.discovery_window = None;
            return true;
        }
        self.update(Msg::Close, cx);
        false
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if self.discovery_window == Some(window) {
            return self.discovery_view();
        }
        let mut content: Vec<Element<Msg>> = vec![
            text("Screen Mirroring").size(26.0).weight(650.0).into(),
            text("Share a screen and laptop audio with an AirPlay TV.").role(Role::Text2).wrap().into(),
            button(format!(
                "Discovery Details ({} Bluetooth · {} LAN · {} Direct)",
                self.discovery.records.len(),
                self.network_devices.len(),
                self.peer_devices.len()
            ))
            .on_press(Msg::ShowDiscovery)
            .into(),
        ];
        if let Ok(label) = std::env::var("SEKKA_AIRPLAY_TEST_LABEL") {
            content.push(text(format!("Hardware test: {label}")).wrap().into());
        }
        if let Some(error) = &self.error {
            content.push(text(format!("Could not complete the connection: {error}")).wrap().into());
        }
        if !self.closing {
            if self.peer.is_some() {
                content.push(text(self.peer_status.as_str()).wrap().into());
                content.push(button("Stop Direct Wi-Fi").on_press(Msg::StopPeer).into());
            } else if !self.active() {
                let ready = self.peer_probe["missing"].as_array().is_some_and(|v| v.is_empty());
                content.push(button("Use Direct Wi-Fi").enabled(ready).on_press(Msg::StartPeer).into());
                let notice = match self.peer_probe["mode"].as_str() {
                    Some("exclusive" | "active-monitor") => {
                        "This adapter disconnects from guest Wi-Fi while the direct link is active. Internet access pauses; stopping direct Wi-Fi restores the previous connection. Administrator authentication is required."
                    }
                    Some("shared") => {
                        "Connect directly to nearby AirPlay TVs while keeping Wi-Fi connected. Administrator authentication is required."
                    }
                    _ if self.peer_probe["missing"].as_array().is_some_and(|v| !v.is_empty()) => {
                        "Direct Wi-Fi is unavailable. See Discovery Details for the capability report."
                    }
                    _ => "Checking direct Wi-Fi support…",
                };
                content.push(text(notice).wrap().role(Role::Text2).into());
            }
        }
        if self.closing {
            content.push(text("Stopping screen and audio sharing…").wrap().into());
        } else if self.active() {
            content.push(text(self.name()).size(21.0).weight(600.0).wrap().into());
            match self.status.state.as_str() {
                "pin_required" => {
                    let password = self.status.credential_kind == "password";
                    content.push(
                        text(if password {
                            "Enter the password configured on this TV."
                        } else {
                            "Enter the passcode shown on the TV."
                        })
                        .wrap()
                        .into(),
                    );
                    let field = text_field(&self.code)
                        .id("airplay-code")
                        .placeholder(if password { "TV password" } else { "TV passcode" })
                        .on_change(Msg::Code)
                        .on_submit(Msg::SubmitCode)
                        .width(Length::Fill);
                    content.push(if password { field.password().into() } else { field.into() });
                    content.push(
                        button("Continue")
                            .primary()
                            .default()
                            .enabled(self.code_valid())
                            .on_press(Msg::SubmitCode)
                            .into(),
                    );
                }
                "streaming" => {
                    content.push(text("Mirroring your screen").size(18.0).into());
                    content.push(
                        text(if !self.status.has_audio {
                            "Screen only · TV audio is unavailable"
                        } else if self.status.audio_muted {
                            "TV audio is muted"
                        } else {
                            "Laptop audio is playing on the TV"
                        })
                        .role(Role::Text2)
                        .wrap()
                        .into(),
                    );
                    if self.status.has_audio {
                        content.push(
                            button(if self.status.audio_muted { "Unmute TV Audio" } else { "Mute TV Audio" })
                                .on_press(Msg::Mute)
                                .into(),
                        );
                    }
                }
                "stopping" => content.push(text("Stopping screen and audio sharing…").wrap().into()),
                _ => content.push(
                    text("Connecting… If the screen chooser opens, select the screen you want to share.")
                        .role(Role::Text2)
                        .wrap()
                        .into(),
                ),
            }
            content.push(
                button(if self.status.state == "streaming" { "Stop Mirroring" } else { "Cancel" })
                    .destructive()
                    .on_press(Msg::Disconnect)
                    .enabled(self.status.state != "stopping")
                    .into(),
            );
        } else {
            content.push(
                text(if self.ready {
                    "Nearby TVs"
                } else if self.backend.is_some() {
                    "Starting AirPlay…"
                } else {
                    "AirPlay sender stopped"
                })
                .size(18.0)
                .weight(600.0)
                .into(),
            );
            if !self.ready && self.backend.is_none() {
                content.push(button("Restart AirPlay").on_press(Msg::RestartSender).into());
            }
            if self.devices.is_empty() && self.ready {
                content.push(text("Looking for TVs… Open AirPlay on the TV and follow its instructions. On a hospital TV, you may need to press OK on the remote first.")
                    .wrap().role(Role::Text2).into());
            }
            for device in &self.devices {
                content.push(
                    row![
                        column![
                            text(device.name.as_str()).weight(600.0).wrap(),
                            text(format!("{} · {}:{}", device.model, device.ip, device.port)).role(Role::Text3).wrap(),
                            text(if device.ip.contains("%awdl0") {
                                "Direct Wi-Fi"
                            } else {
                                match (
                                    self.bluetooth_devices.iter().any(|d| d.ip == device.ip && d.port == device.port),
                                    self.network_devices.iter().any(|d| d.ip == device.ip && d.port == device.port),
                                ) {
                                    (true, true) => "Bluetooth + Bonjour",
                                    (true, false) => "Bluetooth",
                                    _ => "Bonjour",
                                }
                            })
                            .role(Role::Text3)
                        ]
                        .spacing(3.0)
                        .width(Length::Fill),
                        button("Mirror").enabled(self.ready).on_press(Msg::Connect(device.clone())),
                    ]
                    .spacing(12.0)
                    .align(Align::Center)
                    .width(Length::Fill)
                    .into(),
                );
            }
            content.push(text("TV not listed?").weight(600.0).into());
            content.push(text("Use the Wi-Fi network the TV specifies. Some guest networks prevent devices from discovering or reaching each other. If the TV shows an IP address, you can enter it below.")
                .wrap().role(Role::Text2).into());
            content.push(
                row![
                    text_field(&self.address)
                        .placeholder("TV IP address (optional :port)")
                        .on_change(Msg::Address)
                        .on_submit(Msg::ConnectAddress)
                        .width(Length::Fill),
                    button("Connect")
                        .enabled(self.ready && !self.address.trim().is_empty())
                        .on_press(Msg::ConnectAddress),
                ]
                .spacing(8.0)
                .width(Length::Fill)
                .into(),
            );
        }
        content.push(text("Closing this window stops sharing and forgets TV pairing. Everything visible on the selected screen may appear on the TV. Some protected videos cannot be mirrored.")
            .role(Role::Text3).wrap().into());
        scroll(column(content).spacing(16.0).padding(24.0).width(Length::Fill)).height(Length::Fill).into()
    }
}

fn report_age(record: &bluetooth::Record) -> String {
    let Some(last) = record.last_report_unix_ms else {
        return "Cached Bluetooth data · no fresh report in this scan yet".into();
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let seconds = now.saturating_sub(last) / 1000;
    if seconds < 5 {
        format!("Live Bluetooth reports · last received {seconds}s ago")
    } else {
        format!("No recent Bluetooth report · last received {seconds}s ago (showing last signal)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_kit::testing::Harness;

    fn app() -> Harness<Kagami> {
        let mut app = Kagami::new();
        app.start_backend = false;
        app.ready = true;
        Harness::new(app)
    }

    #[test]
    fn signal_sort_can_be_disabled_across_updates_without_losing_tracking() {
        let mut h = app();
        let a = bluetooth::Record { path: "a".into(), address: "AA".into(), rssi_dbm: Some(-80), ..Default::default() };
        let b = bluetooth::Record { path: "b".into(), address: "BB".into(), rssi_dbm: Some(-40), ..Default::default() };
        let snapshot = bluetooth::Snapshot { records: vec![b, a], ..Default::default() };
        h.send(Msg::Bluetooth(snapshot.clone()));
        assert_eq!(h.program().discovery.records[0].path, "b");
        h.send(Msg::TrackBeacon("b".into()));
        h.send(Msg::SortBySignal(false));
        assert_eq!(h.program().discovery.records[0].path, "a");
        h.send(Msg::Bluetooth(snapshot));
        assert_eq!(h.program().discovery.records[0].path, "a");
        assert_eq!(h.program().tracker.as_ref().unwrap().record.path, "b");
        h.send(Msg::SortBySignal(true));
        assert_eq!(h.program().discovery.records[0].path, "b");
    }

    #[test]
    fn stopped_sender_offers_recovery_and_drops_its_stale_discovery() {
        let mut h = app();
        h.send(Msg::Backend(Event::Snapshot(
            Status::default(),
            vec![Device { name: "Old LAN receiver".into(), ip: "192.0.2.1".into(), port: 7000, ..Default::default() }],
        )));
        h.send(Msg::Backend(Event::Stopped));
        assert!(!h.program().ready);
        assert!(h.program().devices.is_empty());
        let texts = h.texts(h.window());
        assert!(texts.iter().any(|t| t == "AirPlay sender stopped"));
        assert!(texts.iter().any(|t| t == "Restart AirPlay"));
        assert!(!texts.iter().any(|t| t == "Starting AirPlay…"));
    }

    #[test]
    fn pairing_and_active_sharing_have_distinct_controls() {
        let mut h = app();
        let w = h.window();
        h.send(Msg::Connect(Device {
            name: "Example TV".into(),
            ip: "192.0.2.1".into(),
            port: 7000,
            ..Device::default()
        }));
        h.send(Msg::Backend(Event::Snapshot(
            Status { state: "pin_required".into(), credential_kind: "pin".into(), ..Status::default() },
            vec![],
        )));
        assert!(h.texts(w).iter().any(|t| t.contains("passcode shown")));
        assert!(!h.texts(w).iter().any(|t| t == "Stop Mirroring"));
        h.send(Msg::Code("0042".into()));
        h.send(Msg::SubmitCode);
        assert!(!h.texts(w).iter().any(|t| t.contains("0042")));
        h.send(Msg::Backend(Event::Snapshot(
            Status { state: "streaming".into(), has_audio: true, ..Status::default() },
            vec![],
        )));
        assert!(h.texts(w).iter().any(|t| t == "Stop Mirroring"));
        assert!(h.texts(w).iter().any(|t| t == "Mute TV Audio"));
    }

    #[test]
    fn configured_password_does_not_claim_a_code_is_on_the_tv() {
        let mut h = app();
        let w = h.window();
        h.send(Msg::Backend(Event::Snapshot(
            Status { state: "pin_required".into(), credential_kind: "password".into(), ..Status::default() },
            vec![],
        )));
        assert!(h.texts(w).iter().any(|t| t.contains("password configured")));
        assert!(!h.texts(w).iter().any(|t| t.contains("passcode shown")));
    }

    #[test]
    fn raw_results_remain_visible_and_details_close_without_stopping_the_app() {
        let mut h = app();
        let main = h.window();
        let beacon = bluetooth::Record {
            path: "/org/bluez/hci0/dev_example".into(),
            adapter: "/org/bluez/hci0".into(),
            address: "00:11:22:33:44:55".into(),
            address_type: "random".into(),
            rssi_dbm: Some(-42),
            manufacturer_hex: "09 06 03 00 00 00 00 00".into(),
            advertisement: Some(bluetooth::Advertisement {
                flags: 3,
                seed: 0,
                ip: std::net::Ipv4Addr::UNSPECIFIED,
                port: 7000,
            }),
            probe: bluetooth::Probe { status: "Not probed: no IPv4 address".into(), ..bluetooth::Probe::default() },
            ..bluetooth::Record::default()
        };
        let mut failed = beacon.clone();
        failed.path = "/org/bluez/hci0/dev_failed".into();
        failed.address = "00:11:22:33:44:66".into();
        failed.advertisement.as_mut().unwrap().ip = std::net::Ipv4Addr::new(192, 168, 1, 4);
        failed.probe.status = "/info failed: connection timed out".into();
        h.send(Msg::Bluetooth(bluetooth::Snapshot {
            scan_status: "Scanning Bluetooth LE".into(),
            records: vec![beacon, failed],
            ..bluetooth::Snapshot::default()
        }));
        h.send(Msg::Code("private-pairing-code".into()));
        h.send(Msg::ShowDiscovery);
        let details = h.windows().into_iter().find(|w| *w != main).unwrap();
        assert!(h.texts(details).iter().any(|t| t.contains("0.0.0.0:7000")));
        assert!(h.texts(details).iter().any(|t| t.contains("connection timed out")));
        h.send(Msg::InspectBeacon("/org/bluez/hci0/dev_example".into()));
        assert!(h.texts(details).iter().any(|t| t.contains("09 06 03 00 00 00 00 00")));
        h.send(Msg::CopyDiscovery);
        let report = h.platform.clipboard.as_deref().unwrap();
        let json: serde_json::Value = serde_json::from_str(report).unwrap();
        assert_eq!(json["bluetooth"]["records"].as_array().unwrap().len(), 2);
        assert!(!report.contains("private-pairing-code"));
        if let Some(dir) = std::env::var_os("AIRPLAY_TEST_SCREENSHOTS") {
            h.save_png(details, std::path::PathBuf::from(dir).join("discovery-details.png"), 1.0);
        }
        h.request_close(details);
        assert_eq!(h.windows(), vec![main]);
        assert!(!h.platform.quit);
    }

    #[test]
    fn tracked_signal_is_pinned_while_other_signals_reorder_and_disappear() {
        let mut h = app();
        let main = h.window();
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let mut target = bluetooth::Record {
            path: "target".into(),
            address: "00:11:22:33:44:55".into(),
            rssi_dbm: Some(-45),
            last_report_unix_ms: Some(timestamp),
            report_count: 1,
            ..bluetooth::Record::default()
        };
        let other = bluetooth::Record {
            path: "other".into(),
            address: "00:11:22:33:44:66".into(),
            rssi_dbm: Some(-50),
            ..bluetooth::Record::default()
        };
        h.send(Msg::Bluetooth(bluetooth::Snapshot {
            records: vec![other.clone(), target.clone()],
            ..bluetooth::Snapshot::default()
        }));
        h.send(Msg::ShowDiscovery);
        let window = h.windows().into_iter().find(|w| *w != main).unwrap();
        h.send(Msg::TrackBeacon("target".into()));
        assert!(h.find(window, "Tracking Beacon").is_some());
        target.rssi_dbm = Some(-75);
        target.report_count = 2;
        target.last_report_unix_ms = Some(timestamp + 1000);
        h.send(Msg::Bluetooth(bluetooth::Snapshot {
            records: vec![other.clone(), target],
            ..bluetooth::Snapshot::default()
        }));
        assert!(h.find(window, "-75 dBm").is_some(), "signal must stay in the fixed header");
        assert!(!h.texts(window).iter().any(|t| t.contains("00:11:22:33:44:66")));
        if let Some(dir) = std::env::var_os("AIRPLAY_TEST_SCREENSHOTS") {
            h.save_png(window, std::path::PathBuf::from(dir).join("tracking.png"), 1.0);
        }
        h.send(Msg::Bluetooth(bluetooth::Snapshot { records: vec![other], ..bluetooth::Snapshot::default() }));
        assert!(h.texts(window).iter().any(|t| t.contains("Beacon absent")));
        assert!(h.find(window, "-75 dBm").is_some());
        h.send(Msg::StopTracking);
        assert!(h.texts(window).iter().any(|t| t.contains("00:11:22:33:44:66")));
    }

    #[test]
    fn screenshots() {
        let Some(dir) = std::env::var_os("AIRPLAY_TEST_SCREENSHOTS") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = app();
        let w = h.window();
        let device = Device {
            name: "Example TV".into(),
            ip: "192.0.2.42".into(),
            port: 7000,
            model: "AirPlay display".into(),
            ..Device::default()
        };
        h.send(Msg::Backend(Event::Snapshot(
            Status { state: "idle".into(), ..Status::default() },
            vec![device.clone()],
        )));
        h.save_png(w, dir.join("picker.png"), 1.0);
        h.send(Msg::Connect(device));
        h.send(Msg::Backend(Event::Snapshot(Status { state: "pin_required".into(), ..Status::default() }, vec![])));
        h.save_png(w, dir.join("pairing.png"), 1.0);
        h.send(Msg::Backend(Event::Snapshot(
            Status { state: "streaming".into(), has_audio: true, ..Status::default() },
            vec![],
        )));
        h.save_png(w, dir.join("mirroring.png"), 1.0);
    }
}
