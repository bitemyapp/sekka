//! Network: Wi-Fi (the switch, the network in use, known and other networks
//! in range, joining with a password, an organization's account or by a
//! hidden network's name), airplane mode, wired connections, VPNs and
//! WireGuard (on and off, importing, removing), a hotspot, the saved Wi-Fi
//! networks, and each connection's own page: its addresses, how it joins,
//! its private hardware address, password, IPv4, DNS, IPv6 and proxy, and
//! forgetting it.
//!
//! Ported from Tatami's Network (`desktop/tatami/network`: window.rs,
//! details.rs, join.rs), keeping its care: one join at a time; a saved
//! password that's refused asks for the password again rather than
//! failing; a new network whose password is wrong leaves nothing behind;
//! WEP isn't offered; a change that fails is undone on the page (it's
//! rebuilt from what NetworkManager kept); changes to the connection in
//! use apply at once where NetworkManager can, and say so where they wait
//! for the next join. It speaks NetworkManager over D-Bus through
//! `aizome-sys` instead of nmcli.

mod logic;
mod sample;
mod view;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{FileDialog, TimerHandle};
use aizome_sys::network::{
    Enterprise, Ipv4Method, Metered, Network as Nm, NetworkState, Profile, Security, WifiNetwork,
};
use aizome_sys::{Error, Task, Wake};

use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::system::Cell;

pub use logic::{Dns, Mac};

/// How often the pane asks for a scan while it shows (Tatami's 30 s), and
/// how long a scan's spinner shows.
const SCAN_EVERY: Duration = Duration::from_secs(30);
const SCAN_SHOWS: Duration = Duration::from_secs(6);

/// NetworkManager, live or the sample.
pub enum Net {
    None,
    Live(Nm),
    Sample(Cell<NetworkState>),
}

impl Net {
    pub fn snapshot(&self) -> Arc<NetworkState> {
        match self {
            Net::None => Arc::new(NetworkState::default()),
            Net::Live(n) => n.snapshot(),
            Net::Sample(c) => c.get(),
        }
    }

    fn unavailable<T>() -> Task<T> {
        Task::ready(Err(Error::Unavailable("NetworkManager".into())))
    }

    fn set_wifi(&self, on: bool) -> Task<()> {
        match self {
            Net::Live(n) => n.set_wifi_enabled(on),
            Net::Sample(c) => c.update(|s| sample::set_wifi(s, on)),
            Net::None => Net::unavailable(),
        }
    }

    fn set_airplane(&self, on: bool) -> Task<()> {
        match self {
            Net::Live(n) => n.set_airplane_mode(on),
            Net::Sample(c) => c.update(|s| {
                s.airplane_mode = on;
                sample::set_wifi(s, !on);
            }),
            Net::None => Net::unavailable(),
        }
    }

    fn scan(&self) -> Task<()> {
        match self {
            Net::Live(n) => n.scan(),
            _ => Task::ready(Ok(())),
        }
    }

    fn connect(&self, network: &WifiNetwork, password: Option<&str>) -> Task<()> {
        match self {
            Net::Live(n) => n.connect(network, password),
            Net::Sample(c) => {
                let mut result = Ok(());
                let ssid = network.ssid.clone();
                let task = c.update(|s| result = sample::join(s, &ssid, password));
                match result {
                    Ok(()) => task,
                    Err(e) => c.refuse(e),
                }
            }
            Net::None => Net::unavailable(),
        }
    }

    fn connect_enterprise(&self, network: &WifiNetwork, e: Enterprise) -> Task<()> {
        match self {
            Net::Live(n) => n.connect_enterprise(network, e),
            _ => self.connect(network, Some(&e.password)),
        }
    }

    fn connect_hidden(&self, ssid: &str, security: Security, password: Option<&str>) -> Task<()> {
        match self {
            Net::Live(n) => n.connect_hidden(ssid, security, password),
            Net::Sample(c) => c.refuse(Error::Failed(format!("No network called “{ssid}” answered"))),
            Net::None => Net::unavailable(),
        }
    }

    fn connect_hidden_enterprise(&self, ssid: &str, e: Enterprise) -> Task<()> {
        match self {
            Net::Live(n) => n.connect_hidden_enterprise(ssid, e),
            _ => self.connect_hidden(ssid, Security::Enterprise, Some("")),
        }
    }

    fn activate(&self, uuid: &str) -> Task<()> {
        match self {
            Net::Live(n) => n.activate(uuid),
            Net::Sample(c) => {
                let uuid = uuid.to_owned();
                c.update(move |s| sample::set_vpn(s, &uuid, true))
            }
            Net::None => Net::unavailable(),
        }
    }

    fn deactivate(&self, uuid: &str) -> Task<()> {
        match self {
            Net::Live(n) => n.deactivate(uuid),
            Net::Sample(c) => {
                let uuid = uuid.to_owned();
                c.update(move |s| {
                    sample::set_vpn(s, &uuid, false);
                    for n in &mut s.networks {
                        if n.connection.as_deref() == Some(uuid.as_str()) {
                            n.connected = false;
                        }
                    }
                })
            }
            Net::None => Net::unavailable(),
        }
    }

    fn forget(&self, uuid: &str) -> Task<()> {
        match self {
            Net::Live(n) => n.forget(uuid),
            Net::Sample(c) => {
                let uuid = uuid.to_owned();
                c.update(move |s| sample::forget(s, &uuid))
            }
            Net::None => Net::unavailable(),
        }
    }

    fn set_autoconnect(&self, uuid: &str, on: bool) -> Task<()> {
        match self {
            Net::Live(n) => n.set_autoconnect(uuid, on),
            Net::Sample(c) => {
                let uuid = uuid.to_owned();
                c.update(move |s| {
                    if let Some(x) = s.connections.iter_mut().find(|x| x.uuid == uuid) {
                        x.autoconnect = on;
                    }
                })
            }
            Net::None => Net::unavailable(),
        }
    }

    fn set_metered(&self, uuid: &str, m: Metered) -> Task<()> {
        match self {
            Net::Live(n) => n.set_metered(uuid, m),
            Net::Sample(c) => {
                let uuid = uuid.to_owned();
                c.update(move |s| {
                    if let Some(x) = s.connections.iter_mut().find(|x| x.uuid == uuid) {
                        x.metered = m;
                    }
                })
            }
            Net::None => Net::unavailable(),
        }
    }

    fn import_vpn(&self, file: &std::path::Path) -> Task<String> {
        match self {
            Net::Live(n) => n.import_vpn(file, None),
            Net::Sample(_) => Task::ready(Err(Error::Failed("The sample doesn't import files".into()))),
            Net::None => Net::unavailable(),
        }
    }

    fn start_hotspot(&self, name: &str, password: &str) -> Task<()> {
        match self {
            Net::Live(n) => n.start_hotspot(name, password, None),
            Net::Sample(c) => c.refuse(Error::Failed("The sample has no hotspot".into())),
            Net::None => Net::unavailable(),
        }
    }

    fn stop_hotspot(&self) -> Task<()> {
        match self {
            Net::Live(n) => n.stop_hotspot(),
            _ => Task::ready(Ok(())),
        }
    }
}

/// Where in the pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    /// The saved Wi-Fi networks.
    Saved,
    /// A connection's own page, by UUID.
    Details(String),
}

/// What a sheet asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Sheet {
    /// A network's password, to join it.
    Password {
        ssid: String,
        security: Security,
        title: String,
        message: String,
        password: String,
        show: bool,
        error: Option<String>,
    },
    /// A hidden network by name, or an organization's network.
    Form(JoinForm),
    /// Share this connection.
    Hotspot { name: String, password: String, error: Option<String> },
}

/// Security choices when joining by name.
pub const FORM_SECURITY: [(&str, Option<Security>); 5] = [
    ("None", Some(Security::Open)),
    ("WPA/WPA2 Personal", Some(Security::Wpa2Personal)),
    ("WPA2/WPA3 Personal", Some(Security::Wpa2Wpa3Personal)),
    ("WPA3 Personal", Some(Security::Wpa3Personal)),
    ("Enterprise (802.1X)", None),
];
pub const FORM_ENTERPRISE: usize = 4;
pub const EAP_METHODS: [(&str, &str); 2] = [("PEAP", "peap"), ("TTLS", "ttls")];
pub const EAP_INNER: [(&str, &str); 2] = [("MSCHAPv2", "mschapv2"), ("PAP", "pap")];

/// The form for joining by name, or an enterprise network.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinForm {
    /// Joined by name (a hidden network): the name and security show.
    pub hidden: bool,
    pub name: String,
    pub security: usize,
    pub method: usize,
    pub inner: usize,
    pub username: String,
    pub password: String,
    pub validate: bool,
    pub domain: String,
    pub anonymous: String,
    /// Why the last try didn't join.
    pub error: Option<String>,
}

impl JoinForm {
    fn new(name: Option<String>, hidden: bool) -> JoinForm {
        JoinForm {
            hidden,
            name: name.unwrap_or_default(),
            security: if hidden { 1 } else { FORM_ENTERPRISE },
            method: 0,
            inner: 0,
            username: String::new(),
            password: String::new(),
            validate: true,
            domain: String::new(),
            anonymous: String::new(),
            error: None,
        }
    }

    pub fn enterprise(&self) -> bool {
        self.security == FORM_ENTERPRISE
    }

    pub fn personal(&self) -> Option<Security> {
        FORM_SECURITY.get(self.security).and_then(|(_, s)| *s).filter(|s| s.needs_password())
    }

    /// Whether Join can be pressed: a name, and what the security asks for.
    pub fn ready(&self) -> bool {
        !self.name.trim().is_empty()
            && self.name.len() <= 32
            && match (self.enterprise(), self.personal()) {
                (true, _) => !self.username.trim().is_empty() && !self.password.is_empty(),
                (false, Some(s)) => logic::password_ok(s, &self.password),
                (false, None) => true,
            }
    }

    fn credentials(&self) -> Enterprise {
        Enterprise {
            method: EAP_METHODS[self.method.min(1)].1.into(),
            phase2: EAP_INNER[self.inner.min(1)].1.into(),
            identity: self.username.trim().into(),
            anonymous_identity: self.anonymous.trim().into(),
            password: self.password.clone(),
            domain: self.domain.trim().into(),
            validate_server: self.validate,
        }
    }
}

/// A field of the join form.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Name(String),
    Security(usize),
    Method(usize),
    Inner(usize),
    Username(String),
    Password(String),
    Validate(bool),
    Domain(String),
    Anonymous(String),
}

/// What the details page's fields hold while being typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    /// The page's connection (fields reset when it changes).
    pub uuid: String,
    pub address: String,
    pub router: String,
    pub dns: String,
    /// Manual IPv4 chosen, not applied yet (it takes effect with an
    /// address).
    pub manual: bool,
    /// Custom DNS chosen, not applied yet.
    pub custom: bool,
    pub address_error: Option<String>,
    pub dns_error: Option<String>,
}

// A profile read for a details page is the large one; messages are few.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Msg {
    Wifi(bool),
    Airplane(bool),
    Scan,
    ScanDone,
    /// A network's row: join it (or open the one in use).
    Join(String),
    JoinOther,
    Joined {
        ssid: String,
        retry: bool,
        result: Result<(), Error>,
    },
    Open(Page),
    Back,
    Vpn(String, bool),
    VpnDone(String, bool, Result<(), Error>),
    ImportVpn,
    ImportPicked(Option<Vec<PathBuf>>),
    Imported(String, Result<String, Error>),
    Hotspot(bool),
    // The details page.
    Loaded(String, Result<Profile, Error>),
    Connect(String),
    Disconnect(String),
    Autoconnect(String, bool),
    Metered(String, bool),
    PrivateAddress(String, Mac),
    ChangePassword(String),
    Ipv4(String, usize),
    AddressText(String),
    RouterText(String),
    ApplyAddress(String),
    DnsChoice(String, Dns),
    DnsText(String),
    ApplyDns(String),
    Ipv6(String, String),
    Forget(String),
    ForgetNow(String),
    Forgotten(String, String, Result<(), Error>),
    /// A change to a connection finished: say why it failed, read it again,
    /// apply it to the connection in use when asked.
    Changed {
        uuid: String,
        what: String,
        reapply: bool,
        result: Result<(), Error>,
    },
    Reapplied(String, Result<bool, Error>),
    /// An action finished: say why it failed.
    Done(String, Result<(), Error>),
    // Sheets.
    SheetPassword(String),
    ShowPassword(bool),
    SheetField(Field),
    HotspotName(String),
    HotspotPassword(String),
    SheetSubmit,
    SheetCancel,
}

pub struct Network {
    net: Net,
    page: Page,
    /// The details page was opened from Saved networks (Back goes there).
    from_saved: bool,
    sheet: Option<Sheet>,
    /// The network being joined (one at a time).
    joining: Option<String>,
    /// The sheet a join in progress was asked for in, with what was typed
    /// in it: it opens again, as it was, if the join fails.
    tried: Option<(String, Sheet)>,
    /// A sheet waiting for the one open to close (a join that ended while
    /// another was being typed in).
    queued: Option<Sheet>,
    /// Connections being brought up or down.
    busy: HashSet<String>,
    scanning: bool,
    scan_timer: Option<TimerHandle>,
    /// Profiles read for details pages, by UUID.
    profiles: HashMap<String, Result<Profile, String>>,
    /// The sample's profiles, as changed.
    sample_profiles: HashMap<String, Profile>,
    edit: Edit,
}

impl Default for Network {
    fn default() -> Network {
        Network {
            net: Net::None,
            page: Page::Main,
            from_saved: false,
            sheet: None,
            joining: None,
            tried: None,
            queued: None,
            busy: HashSet::new(),
            scanning: false,
            scan_timer: None,
            profiles: HashMap::new(),
            sample_profiles: HashMap::new(),
            edit: Edit::default(),
        }
    }
}

impl Network {
    pub fn state(&self) -> Arc<NetworkState> {
        self.net.snapshot()
    }

    fn start(&mut self, cx: &mut PaneCx<Msg>) {
        if !matches!(self.net, Net::None) {
            return;
        }
        let wake: Wake = cx.wake();
        self.net = if cx.sample() {
            Net::Sample(Cell::new(
                crate::panes::hw::live_snapshot(
                    || Nm::new(aizome_sys::Wake::none()),
                    |n| n.available(),
                    |n| n.snapshot(),
                )
                .unwrap_or_else(sample::state),
                wake,
            ))
        } else {
            Net::Live(Nm::new(wake))
        };
    }

    fn scan(&mut self, cx: &mut PaneCx<Msg>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        let task = self.net.scan();
        cx.wait(task, |_| Msg::Done(String::new(), Ok(())));
        cx.after(SCAN_SHOWS, Msg::ScanDone);
    }

    fn act(&mut self, cx: &mut PaneCx<Msg>, what: &str, task: Task<()>) {
        let what = what.to_owned();
        cx.wait(task, move |r| Msg::Done(what, r));
    }

    fn change(&mut self, cx: &mut PaneCx<Msg>, uuid: &str, what: &str, reapply: bool, task: Task<()>) {
        let (uuid, what) = (uuid.to_owned(), what.to_owned());
        cx.wait(task, move |result| Msg::Changed { uuid, what, reapply, result });
    }

    /// Read a connection's profile for its page.
    fn load(&mut self, uuid: &str, cx: &mut PaneCx<Msg>) {
        match &self.net {
            Net::Live(n) => {
                let task = n.profile(uuid);
                let uuid = uuid.to_owned();
                cx.wait(task, move |r| Msg::Loaded(uuid, r));
            }
            Net::Sample(c) => {
                let p = self.sample_profiles.get(uuid).cloned().or_else(|| sample::profile(&c.get(), uuid));
                let r = p.ok_or_else(|| Error::NotFound(uuid.to_owned()));
                self.loaded(uuid.to_owned(), r);
            }
            Net::None => {}
        }
    }

    fn loaded(&mut self, uuid: String, result: Result<Profile, Error>) {
        let fresh = result.map_err(|e| logic::reason(&e));
        if let Ok(p) = &fresh
            && (self.edit.uuid != uuid || (!self.edit.manual && !self.edit.custom))
        {
            self.edit = Edit {
                uuid: uuid.clone(),
                address: p.ipv4_addresses.first().cloned().unwrap_or_default(),
                router: p.ipv4_gateway.clone().unwrap_or_default(),
                dns: if Dns::of(p.ipv4_ignore_auto_dns, &p.dns()) == Dns::Custom {
                    p.dns().join(" ")
                } else {
                    String::new()
                },
                ..Edit::default()
            };
        }
        self.profiles.insert(uuid, fresh);
    }

    fn profile(&self, uuid: &str) -> Option<&Profile> {
        self.profiles.get(uuid).and_then(|r| r.as_ref().ok())
    }

    /// A change to a sample profile, as NetworkManager would keep it.
    fn sample_edit(&mut self, uuid: &str, f: impl FnOnce(&mut Profile)) -> Task<()> {
        let state = self.state();
        if let Some(mut p) = self.sample_profiles.get(uuid).cloned().or_else(|| sample::profile(&state, uuid)) {
            f(&mut p);
            self.sample_profiles.insert(uuid.to_owned(), p);
        }
        Task::ready(Ok(()))
    }

    fn open_sheet(&mut self, sheet: Sheet, cx: &mut PaneCx<Msg>) {
        self.sheet = Some(sheet);
        cx.open_sheet();
    }

    /// Open a sheet, or, while another is open (something being typed),
    /// once that one closes.
    fn open_sheet_after(&mut self, sheet: Sheet, cx: &mut PaneCx<Msg>) {
        if self.sheet.is_some() {
            self.queued = Some(sheet);
        } else {
            self.open_sheet(sheet, cx);
            Self::focus_sheet(self.sheet.as_ref(), cx);
        }
    }

    fn focus_sheet(sheet: Option<&Sheet>, cx: &mut PaneCx<Msg>) {
        match sheet {
            Some(Sheet::Password { .. }) => cx.focus("network-password"),
            Some(Sheet::Form(f)) if f.hidden => cx.focus("network-name"),
            Some(Sheet::Form(_)) => cx.focus("network-username"),
            _ => {}
        }
    }

    fn close_sheet(&mut self, cx: &mut PaneCx<Msg>) {
        self.sheet = None;
        cx.close_sheet();
        if let Some(next) = self.queued.take() {
            self.open_sheet(next, cx);
            Self::focus_sheet(self.sheet.as_ref(), cx);
        }
    }

    /// Say in the open sheet why it can't join yet, keeping what's typed.
    fn sheet_error(&mut self, e: String) {
        match &mut self.sheet {
            Some(Sheet::Password { error, .. }) | Some(Sheet::Hotspot { error, .. }) => *error = Some(e),
            Some(Sheet::Form(f)) => f.error = Some(e),
            None => {}
        }
    }

    fn ask_password(&mut self, ssid: &str, security: Security, title: String, message: String, cx: &mut PaneCx<Msg>) {
        self.open_sheet(
            Sheet::Password {
                ssid: ssid.to_owned(),
                security,
                title,
                message,
                password: String::new(),
                show: false,
                error: None,
            },
            cx,
        );
        cx.focus("network-password");
    }

    /// Join on a thread, the network's row showing a spinner meanwhile.
    fn run_join(&mut self, ssid: &str, retry: bool, task: Task<()>, cx: &mut PaneCx<Msg>) {
        self.joining = Some(ssid.to_owned());
        let ssid = ssid.to_owned();
        cx.wait(task, move |result| Msg::Joined { ssid, retry, result });
    }

    /// Whether another network is being joined (said in the status strip).
    fn busy_joining(&self, cx: &mut PaneCx<Msg>) -> bool {
        if let Some(j) = &self.joining {
            cx.say(format!("Still joining {j}"));
            return true;
        }
        false
    }

    fn join(&mut self, ssid: &str, cx: &mut PaneCx<Msg>) {
        let state = self.state();
        let Some(network) = state.network(ssid).cloned() else {
            // Not in range: join it by name, as a hidden one.
            if !self.busy_joining(cx) {
                self.open_sheet(Sheet::Form(JoinForm::new(Some(ssid.to_owned()), true)), cx);
            }
            return;
        };
        if network.connected {
            if let Some(uuid) = &network.connection {
                self.open(Page::Details(uuid.clone()), cx);
            }
            return;
        }
        if self.busy_joining(cx) {
            return;
        }
        if network.known {
            let task = self.net.connect(&network, None);
            let retry = network.security.needs_password();
            self.run_join(ssid, retry, task, cx);
            return;
        }
        match network.security {
            Security::Open | Security::EnhancedOpen => {
                let task = self.net.connect(&network, None);
                self.run_join(ssid, false, task, cx);
            }
            Security::Enterprise => self.open_sheet(Sheet::Form(JoinForm::new(Some(ssid.to_owned()), false)), cx),
            Security::Wep => cx.failed(&format!("{ssid} isn't offered"), "it uses WEP, which anyone nearby can break"),
            security => self.ask_password(
                ssid,
                security,
                format!("Join “{ssid}”"),
                format!("Enter the password for “{ssid}”."),
                cx,
            ),
        }
    }

    fn open(&mut self, page: Page, cx: &mut PaneCx<Msg>) {
        if let Page::Details(uuid) = &page {
            self.from_saved = self.page == Page::Saved;
            let uuid = uuid.clone();
            if self.edit.uuid != uuid {
                self.edit = Edit { uuid: uuid.clone(), ..Edit::default() };
            }
            self.load(&uuid, cx);
        }
        self.page = page;
    }

    fn back(&mut self) -> bool {
        match &self.page {
            Page::Main => false,
            Page::Details(_) if self.from_saved => {
                self.page = Page::Saved;
                self.from_saved = false;
                true
            }
            _ => {
                self.page = Page::Main;
                true
            }
        }
    }

    fn submit(&mut self, cx: &mut PaneCx<Msg>) {
        let Some(sheet) = self.sheet.clone() else { return };
        match &sheet {
            Sheet::Password { ssid, security, password, .. } => {
                let (ssid, security, password) = (ssid.clone(), *security, password.clone());
                if !logic::password_ok(security, &password) {
                    let min = logic::minimum_password(security);
                    if let Some(Sheet::Password { error, .. }) = &mut self.sheet {
                        *error = Some(format!("This network's password has at least {min} characters"));
                    }
                    return;
                }
                // Another join first: the sheet stays, with what's typed.
                if let Some(j) = self.joining.clone() {
                    self.sheet_error(format!("Still joining {j}: join this one once that's done"));
                    return;
                }
                let state = self.state();
                let Some(network) = state.network(&ssid).cloned() else {
                    self.sheet_error(format!("{ssid} is out of range"));
                    return;
                };
                self.close_sheet(cx);
                let task = self.net.connect(&network, Some(&password));
                self.tried = Some((ssid.clone(), sheet.clone()));
                self.run_join(&ssid, false, task, cx);
            }
            Sheet::Form(form) => {
                if !form.ready() {
                    return;
                }
                if let Some(j) = self.joining.clone() {
                    self.sheet_error(format!("Still joining {j}: join this one once that's done"));
                    return;
                }
                self.close_sheet(cx);
                let ssid = form.name.trim().to_owned();
                self.tried = Some((ssid.clone(), sheet.clone()));
                let in_range = self.state().network(&ssid).cloned();
                let task = match (form.enterprise(), form.hidden, in_range) {
                    (true, false, Some(n)) => self.net.connect_enterprise(&n, form.credentials()),
                    (true, _, _) => self.net.connect_hidden_enterprise(&ssid, form.credentials()),
                    (false, _, _) => {
                        let security = FORM_SECURITY[form.security].1.unwrap_or(Security::Open);
                        let pw = (security.needs_password()).then_some(form.password.as_str());
                        self.net.connect_hidden(&ssid, security, pw)
                    }
                };
                self.run_join(&ssid, false, task, cx);
            }
            Sheet::Hotspot { name, password, .. } => {
                let (name, password) = (name.clone(), password.clone());
                let error = if name.trim().is_empty() || name.len() > 32 {
                    Some("A network name has 1 to 32 characters")
                } else if !(8..=63).contains(&password.chars().count()) {
                    Some("A hotspot password has 8 to 63 characters")
                } else {
                    None
                };
                if let Some(e) = error {
                    if let Some(Sheet::Hotspot { error, .. }) = &mut self.sheet {
                        *error = Some(e.to_owned());
                    }
                    return;
                }
                self.close_sheet(cx);
                let task = self.net.start_hotspot(name.trim(), &password);
                self.act(cx, "Couldn't start the hotspot", task);
            }
        }
    }

    fn details_update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        let live = matches!(self.net, Net::Live(_));
        match msg {
            Msg::Connect(uuid) => {
                let state = self.state();
                let Some(c) = state.connection(&uuid).cloned() else { return };
                match c.ssid.as_deref() {
                    Some(ssid) => self.join(ssid, cx),
                    None => {
                        self.busy.insert(uuid.clone());
                        let task = self.net.activate(&uuid);
                        cx.wait(task, move |r| Msg::VpnDone(uuid, true, r));
                    }
                }
            }
            Msg::Disconnect(uuid) => {
                self.busy.insert(uuid.clone());
                let task = self.net.deactivate(&uuid);
                cx.wait(task, move |r| Msg::VpnDone(uuid, false, r));
            }
            Msg::Autoconnect(uuid, on) => {
                let task = self.net.set_autoconnect(&uuid, on);
                if !live {
                    self.sample_edit(&uuid, |p| p.autoconnect = on);
                }
                self.change(cx, &uuid, "joining", false, task);
            }
            Msg::Metered(uuid, on) => {
                let m = if on { Metered::Yes } else { Metered::No };
                let task = self.net.set_metered(&uuid, m);
                if !live {
                    self.sample_edit(&uuid, |p| p.metered = m);
                }
                self.change(cx, &uuid, "metering", false, task);
            }
            Msg::PrivateAddress(uuid, mac) => {
                let task = match &self.net {
                    Net::Live(n) => n.set_private_address(&uuid, mac.value()),
                    _ => self.sample_edit(&uuid, |p| p.assigned_mac = mac.value().to_owned()),
                };
                self.change(cx, &uuid, "the private address", false, task);
                if self.state().connection(&uuid).is_some_and(|c| c.active) {
                    cx.say("The private address applies the next time it joins");
                }
            }
            Msg::ChangePassword(uuid) => {
                if self.busy_joining(cx) {
                    return;
                }
                let state = self.state();
                let Some(c) = state.connection(&uuid) else { return };
                let ssid = c.ssid.clone().unwrap_or_else(|| c.id.clone());
                let Some(n) = state.network(&ssid) else {
                    cx.failed(&format!("{ssid} is not in range"), "its password is changed by joining it");
                    return;
                };
                let security = n.security;
                self.ask_password(
                    &ssid,
                    security,
                    format!("New password for “{ssid}”"),
                    "Joining with it replaces the saved password.".into(),
                    cx,
                );
            }
            Msg::Ipv4(uuid, choice) => {
                if choice == 1 {
                    // Manual takes effect with an address.
                    self.edit.manual = true;
                    cx.focus("network-address");
                    return;
                }
                self.edit.manual = false;
                self.edit.address_error = None;
                let task = match &self.net {
                    Net::Live(n) => n.set_ipv4(&uuid, Ipv4Method::Automatic),
                    _ => self.sample_edit(&uuid, |p| {
                        p.ipv4_method = "auto".into();
                        p.ipv4_addresses.clear();
                        p.ipv4_gateway = None;
                    }),
                };
                self.change(cx, &uuid, "IPv4", true, task);
            }
            Msg::AddressText(t) => {
                self.edit.address = t;
                self.edit.address_error = None;
            }
            Msg::RouterText(t) => {
                self.edit.router = t;
                self.edit.address_error = None;
            }
            Msg::ApplyAddress(uuid) => {
                let Some((address, prefix)) = logic::address_with_prefix(&self.edit.address) else {
                    self.edit.address_error =
                        Some("Write the address with its prefix length, as 192.168.1.20/24".into());
                    cx.focus("network-address");
                    return;
                };
                let router = self.edit.router.trim();
                let gateway = if router.is_empty() {
                    None
                } else {
                    match logic::ipv4(router) {
                        Some(g) => Some(g),
                        None => {
                            self.edit.address_error = Some("The router is an IPv4 address, as 192.168.1.1".into());
                            cx.focus("network-router");
                            return;
                        }
                    }
                };
                if gateway.is_none() {
                    // Without a router, nothing beyond this network is reached.
                    cx.say("Without a router, only this network is reached");
                }
                self.edit.manual = false;
                let task = match &self.net {
                    Net::Live(n) => {
                        n.set_ipv4(&uuid, Ipv4Method::Manual { addresses: vec![(address, prefix)], gateway })
                    }
                    _ => self.sample_edit(&uuid, |p| {
                        p.ipv4_method = "manual".into();
                        p.ipv4_addresses = vec![format!("{address}/{prefix}")];
                        p.ipv4_gateway = gateway.map(|g| g.to_string());
                    }),
                };
                self.change(cx, &uuid, "the IP address", true, task);
            }
            Msg::DnsChoice(uuid, dns) => {
                self.edit.dns_error = None;
                if dns == Dns::Custom {
                    // Custom takes effect with its servers.
                    self.edit.custom = true;
                    cx.focus("network-dns");
                    return;
                }
                self.edit.custom = false;
                self.set_dns(&uuid, dns.addresses(), dns == Dns::Automatic, cx);
            }
            Msg::DnsText(t) => {
                self.edit.dns = t;
                self.edit.dns_error = None;
            }
            Msg::ApplyDns(uuid) => match logic::custom_servers(&self.edit.dns) {
                Some(servers) => {
                    self.edit.custom = false;
                    self.set_dns(&uuid, servers, false, cx);
                }
                None => {
                    self.edit.dns_error = Some("DNS servers are addresses, as 9.9.9.9 or 2620:fe::fe".into());
                    cx.focus("network-dns");
                }
            },
            Msg::Ipv6(uuid, method) => {
                let task = match &self.net {
                    Net::Live(n) => n.set_ipv6_method(&uuid, &method),
                    _ => self.sample_edit(&uuid, |p| p.ipv6_method = method.clone()),
                };
                self.change(cx, &uuid, "IPv6", true, task);
            }
            Msg::Forget(uuid) => {
                let state = self.state();
                let Some(c) = state.connection(&uuid) else { return };
                let wifi = c.ssid.is_some();
                let confirm = if wifi {
                    Confirm::new(
                        format!("Forget “{}”?", c.id),
                        "This computer stops joining it, and its password is removed. You can join it again later.",
                        "Forget",
                        Msg::ForgetNow(uuid),
                    )
                } else {
                    Confirm::new(
                        format!("Remove “{}”?", c.id),
                        "Its configuration is removed from this computer.",
                        "Remove",
                        Msg::ForgetNow(uuid),
                    )
                };
                cx.confirm(confirm.destructive());
            }
            Msg::ForgetNow(uuid) => {
                let name = self.state().connection(&uuid).map(|c| c.id.clone()).unwrap_or_default();
                let task = self.net.forget(&uuid);
                cx.wait(task, move |r| Msg::Forgotten(uuid, name, r));
            }
            _ => {}
        }
    }

    fn set_dns(&mut self, uuid: &str, servers: Vec<std::net::IpAddr>, automatic: bool, cx: &mut PaneCx<Msg>) {
        // Where IPv6 is off, IPv6 servers have nowhere to go.
        let v6_off = self.profile(uuid).is_some_and(|p| matches!(p.ipv6_method.as_str(), "disabled" | "ignore"));
        let servers: Vec<std::net::IpAddr> = servers.into_iter().filter(|s| !(v6_off && s.is_ipv6())).collect();
        let task = match &self.net {
            Net::Live(n) => n.set_dns(uuid, &servers, automatic),
            _ => {
                let list: Vec<String> = servers.iter().map(|s| s.to_string()).collect();
                self.sample_edit(uuid, |p| {
                    p.ipv4_dns = list.iter().filter(|s| !s.contains(':')).cloned().collect();
                    p.ipv6_dns = list.iter().filter(|s| s.contains(':')).cloned().collect();
                    p.ipv4_ignore_auto_dns = !automatic && !list.is_empty();
                })
            }
        };
        self.change(cx, uuid, "DNS", true, task);
    }
}

impl Pane for Network {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "network"
    }

    fn title(&self) -> &'static str {
        "Network"
    }

    fn icon(&self) -> Icon {
        Icon::Wifi
    }

    fn tile(&self) -> Tile {
        Tile::Hanada
    }

    fn group(&self) -> Group {
        Group::Connections
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["wifi", "wi-fi", "wireless", "internet", "ethernet", "wired", "vpn", "wireguard", "lan", "ip"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("wifi", "Wi-Fi").keywords(&["wireless", "wlan", "turn off"]),
            SettingEntry::new("airplane", "Airplane Mode").keywords(&["flight", "radios", "rfkill"]),
            SettingEntry::new("networks", "Other networks").keywords(&["join", "scan", "ssid", "nearby"]),
            SettingEntry::new("join-other", "Join a hidden network").keywords(&[
                "other",
                "hidden",
                "ssid",
                "by name",
                "enterprise",
                "802.1x",
                "eduroam",
            ]),
            SettingEntry::new("known", "Known networks").keywords(&["saved", "remembered"]),
            SettingEntry::new("saved", "Saved networks").keywords(&["forget", "remove", "history", "remembered"]),
            SettingEntry::new("wired", "Ethernet").keywords(&["wired", "cable", "lan"]),
            SettingEntry::new("vpn", "VPN").keywords(&["wireguard", "openvpn", "tunnel"]),
            SettingEntry::new("add-vpn", "Add a VPN").keywords(&["import", "wireguard", "conf", "ovpn"]),
            SettingEntry::new("hotspot", "Hotspot").keywords(&["share", "tethering", "access point"]),
            SettingEntry::new("auto-join", "Join automatically")
                .under("Details")
                .keywords(&["autoconnect", "auto-join"]),
            SettingEntry::new("metered", "Metered connection").under("Details").keywords(&["data", "limited", "cost"]),
            SettingEntry::new("private-address", "Private address").under("Details").keywords(&[
                "mac",
                "random",
                "hardware address",
            ]),
            SettingEntry::new("ipv4", "Configure IPv4").under("Details").keywords(&[
                "ip address",
                "static",
                "manual",
                "dhcp",
                "router",
                "gateway",
            ]),
            SettingEntry::new("dns", "DNS servers").under("Details").keywords(&[
                "cloudflare",
                "google",
                "quad9",
                "resolver",
                "nameserver",
            ]),
            SettingEntry::new("ipv6", "Configure IPv6").under("Details"),
            SettingEntry::new("proxy", "Proxy").under("Details").keywords(&["pac"]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        self.start(cx);
        if let Page::Details(uuid) = self.page.clone() {
            self.load(&uuid, cx);
        }
        self.scan(cx);
        if self.scan_timer.is_none() {
            self.scan_timer = Some(cx.every(SCAN_EVERY, Msg::Scan));
        }
    }

    fn hidden(&mut self, cx: &mut PaneCx<Msg>) {
        if let Some(t) = self.scan_timer.take() {
            cx.cancel(t);
        }
    }

    fn focus_entry(&mut self, entry: &str, cx: &mut PaneCx<Msg>) {
        self.start(cx);
        let details = ["auto-join", "metered", "private-address", "ipv4", "dns", "ipv6", "proxy"];
        if details.contains(&entry) {
            // The connection in use, or the first saved one.
            let state = self.state();
            let uuid =
                state.primary().map(|a| a.uuid.clone()).or_else(|| state.connections.first().map(|c| c.uuid.clone()));
            if let Some(uuid) = uuid {
                self.open(Page::Details(uuid), cx);
            }
        } else if entry == "saved" {
            self.page = Page::Saved;
        } else {
            self.page = Page::Main;
        }
    }

    fn back(&mut self, _cx: &mut PaneCx<Msg>) -> bool {
        self.back()
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Wifi(on) => {
                let task = self.net.set_wifi(on);
                self.act(cx, if on { "Couldn't turn Wi-Fi on" } else { "Couldn't turn Wi-Fi off" }, task);
                if on {
                    self.scanning = false;
                    self.scan(cx);
                }
            }
            Msg::Airplane(on) => {
                let task = self.net.set_airplane(on);
                self.act(cx, "Couldn't change airplane mode", task);
            }
            Msg::Scan => self.scan(cx),
            Msg::ScanDone => self.scanning = false,
            Msg::Join(ssid) => self.join(&ssid, cx),
            Msg::JoinOther => {
                if !self.busy_joining(cx) {
                    self.open_sheet(Sheet::Form(JoinForm::new(None, true)), cx);
                    cx.focus("network-name");
                }
            }
            Msg::Joined { ssid, retry, result } => {
                self.joining = None;
                let tried = self.tried.take().filter(|(s, _)| *s == ssid).map(|(_, sheet)| sheet);
                match result {
                    Ok(()) => cx.say(format!("Connected to {ssid}")),
                    // What was typed is asked for again, as it was, with
                    // why it didn't join.
                    Err(e) if tried.is_some() => {
                        let why = logic::reason(&e);
                        let mut sheet = tried.expect("tried");
                        match &mut sheet {
                            Sheet::Password { error, .. } => *error = Some(format!("Couldn't join: {why}")),
                            Sheet::Form(f) => f.error = Some(format!("Couldn't join: {why}")),
                            Sheet::Hotspot { .. } => {}
                        }
                        self.open_sheet_after(sheet, cx);
                    }
                    Err(Error::Rejected(_)) if retry => {
                        // A refused saved password: ask for it again.
                        let security =
                            self.state().network(&ssid).map(|n| n.security).unwrap_or(Security::Wpa2Personal);
                        self.open_sheet_after(
                            Sheet::Password {
                                ssid: ssid.clone(),
                                security,
                                title: format!("Password for “{ssid}”"),
                                message: format!("“{ssid}” didn't accept the saved password. Enter it again to join."),
                                password: String::new(),
                                show: false,
                                error: None,
                            },
                            cx,
                        );
                    }
                    Err(e) => cx.failed(&format!("Couldn't join {ssid}"), logic::reason(&e)),
                }
            }
            Msg::Open(page) => self.open(page, cx),
            Msg::Back => {
                self.back();
            }
            Msg::Vpn(uuid, on) => {
                self.busy.insert(uuid.clone());
                let task = if on { self.net.activate(&uuid) } else { self.net.deactivate(&uuid) };
                cx.wait(task, move |r| Msg::VpnDone(uuid, on, r));
            }
            Msg::VpnDone(uuid, on, result) => {
                self.busy.remove(&uuid);
                if let Err(e) = result {
                    let name = self.state().connection(&uuid).map(|c| c.id.clone()).unwrap_or_default();
                    let verb = if on { "connect" } else { "disconnect" };
                    cx.failed(&format!("Couldn't {verb} {name}"), logic::reason(&e));
                }
            }
            Msg::ImportVpn => {
                let dialog = FileDialog::new()
                    .title("Add a VPN from its configuration file")
                    .filter("VPN configurations", &["conf", "ovpn"]);
                cx.open_panel(dialog, Msg::ImportPicked);
            }
            Msg::ImportPicked(paths) => {
                if let Some(path) = paths.and_then(|p| p.into_iter().next()) {
                    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let task = self.net.import_vpn(&path);
                    cx.wait(task, move |r| Msg::Imported(name, r));
                }
            }
            Msg::Imported(name, result) => match result {
                Ok(_) => cx.say(format!("Added {name}")),
                Err(e) => cx.failed(&format!("Couldn't add {name}"), logic::reason(&e)),
            },
            Msg::Hotspot(on) => {
                if on {
                    let name = format!("{} Hotspot", self.state().hostname.split('.').next().unwrap_or("Aizome"));
                    self.open_sheet(Sheet::Hotspot { name, password: String::new(), error: None }, cx);
                    cx.focus("network-hotspot-password");
                } else {
                    let task = self.net.stop_hotspot();
                    self.act(cx, "Couldn't stop the hotspot", task);
                }
            }
            Msg::Loaded(uuid, result) => self.loaded(uuid, result),
            Msg::Changed { uuid, what, reapply, result } => {
                match result {
                    Err(e) => {
                        cx.failed(&format!("Couldn't change {what}"), logic::reason(&e));
                        // The page shows what NetworkManager kept.
                        self.edit.manual = false;
                        self.edit.custom = false;
                    }
                    Ok(()) if reapply => {
                        if let Net::Live(n) = &self.net {
                            let task = n.reapply(&uuid);
                            let u = uuid.clone();
                            cx.wait(task, move |r| Msg::Reapplied(u, r));
                        }
                    }
                    Ok(()) => {}
                }
                self.load(&uuid, cx);
            }
            Msg::Reapplied(uuid, result) => {
                let active = self.state().connection(&uuid).is_some_and(|c| c.active);
                match result {
                    Ok(false) if active => cx.say("The change applies the next time it connects"),
                    Err(e) => cx.failed("Couldn't apply the change now", logic::reason(&e)),
                    _ => {}
                }
            }
            Msg::Forgotten(uuid, name, result) => match result {
                Ok(()) => {
                    self.profiles.remove(&uuid);
                    if self.page == Page::Details(uuid) {
                        self.back();
                    }
                    let verb = if name.is_empty() { "Forgot the network".to_owned() } else { format!("Forgot {name}") };
                    cx.say(verb);
                }
                Err(e) => cx.failed(&format!("Couldn't forget {name}"), logic::reason(&e)),
            },
            Msg::Done(what, result) => {
                if let Err(e) = result
                    && !what.is_empty()
                {
                    cx.failed(&what, logic::reason(&e));
                }
            }
            Msg::SheetPassword(p) => {
                if let Some(Sheet::Password { password, error, .. }) = &mut self.sheet {
                    *password = p;
                    *error = None;
                }
            }
            Msg::ShowPassword(on) => {
                if let Some(Sheet::Password { show, .. }) = &mut self.sheet {
                    *show = on;
                }
            }
            Msg::SheetField(f) => {
                if let Some(Sheet::Form(form)) = &mut self.sheet {
                    form.error = None;
                    match f {
                        Field::Name(v) => form.name = v,
                        Field::Security(v) => form.security = v.min(FORM_SECURITY.len() - 1),
                        Field::Method(v) => form.method = v,
                        Field::Inner(v) => form.inner = v,
                        Field::Username(v) => form.username = v,
                        Field::Password(v) => form.password = v,
                        Field::Validate(v) => form.validate = v,
                        Field::Domain(v) => form.domain = v,
                        Field::Anonymous(v) => form.anonymous = v,
                    }
                }
            }
            Msg::HotspotName(v) => {
                if let Some(Sheet::Hotspot { name, error, .. }) = &mut self.sheet {
                    *name = v;
                    *error = None;
                }
            }
            Msg::HotspotPassword(v) => {
                if let Some(Sheet::Hotspot { password, error, .. }) = &mut self.sheet {
                    *password = v;
                    *error = None;
                }
            }
            Msg::SheetSubmit => self.submit(cx),
            Msg::SheetCancel => self.close_sheet(cx),
            other => self.details_update(other, cx),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        view::page(self, vcx)
    }

    fn sheet(&self, vcx: &ViewCx) -> Option<Element<Msg>> {
        self.sheet.as_ref().map(|s| view::sheet(self, s, vcx))
    }
}

/// What views read of the pane.
impl Network {
    pub(crate) fn page_now(&self) -> &Page {
        &self.page
    }

    pub(crate) fn joining_now(&self) -> Option<&str> {
        self.joining.as_deref()
    }

    pub(crate) fn is_busy(&self, uuid: &str) -> bool {
        self.busy.contains(uuid)
    }

    pub(crate) fn is_scanning(&self) -> bool {
        self.scanning
    }

    pub(crate) fn loaded_profile(&self, uuid: &str) -> Option<&Result<Profile, String>> {
        self.profiles.get(uuid)
    }

    pub(crate) fn edit_now(&self) -> &Edit {
        &self.edit
    }

    pub(crate) fn is_live(&self) -> bool {
        matches!(self.net, Net::Live(_))
    }
}

impl Network {
    pub(crate) fn came_from_saved(&self) -> bool {
        self.from_saved
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aizome_kit::testing::Harness;

    use super::Msg;
    use crate::app::{AppMsg, Settei, Start};

    fn open(name: &str) -> Harness<Settei> {
        let dir = std::env::temp_dir().join(format!("settei-network-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = Harness::new(Settei::sample(dir.join("settings.toml"), Start::parse("network")));
        settle(&mut h, |h| has(h, "Kohaku"));
        h
    }

    fn settle(h: &mut Harness<Settei>, until: impl Fn(&Harness<Settei>) -> bool) {
        for _ in 0..400 {
            if until(h) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.send(AppMsg::Nothing);
        }
    }

    fn has(h: &Harness<Settei>, s: &str) -> bool {
        h.texts(h.window()).iter().any(|t| t == s)
    }

    fn send(h: &mut Harness<Settei>, m: Msg) {
        h.send(AppMsg::Pane("network", Arc::new(m)));
    }

    fn found(h: &Harness<Settei>, entry: &str) -> bool {
        h.find_id(h.window(), &format!("entry:network.{entry}")).is_some()
    }

    #[test]
    fn search_finds_add_vpn_and_other_network_in_every_state() {
        let mut h = open("anchors");
        for e in ["wifi", "networks", "join-other", "known", "vpn", "add-vpn"] {
            assert!(found(&h, e), "{e} with Wi-Fi on");
        }
        // No VPNs yet: Add VPN is the section's only row, and both answer.
        for uuid in ["c-office-vpn", "c-home-vpn"] {
            send(&mut h, Msg::ForgetNow(uuid.into()));
        }
        settle(&mut h, |h| !has(h, "Home (OpenVPN)"));
        assert!(!has(&h, "Home (OpenVPN)"));
        assert!(found(&h, "vpn") && found(&h, "add-vpn"), "VPN and Add VPN without VPNs");
        // Wi-Fi off: what the network lists hold shows the switch.
        send(&mut h, Msg::Wifi(false));
        settle(&mut h, |h| !has(h, "Other Network…"));
        for e in ["networks", "join-other", "known"] {
            assert!(found(&h, e), "{e} with Wi-Fi off");
        }
    }
}
