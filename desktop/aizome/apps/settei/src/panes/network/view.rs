//! The Network pane's pages and sheets.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_sys::network::{ConnectionKind, NetworkState, Profile, SavedConnection, Security, WifiNetwork};

use super::logic::{self, Dns, Mac};
use super::{EAP_INNER, EAP_METHODS, FORM_SECURITY, Field, JoinForm, Msg, Network, Page, Sheet};
use crate::form::{self, Row};
use crate::pane::{Tile, ViewCx};
use crate::panes::hw::{signal, spinner};
use crate::widgets::{PressStyle, press, tile};

/// The pane's page now.
pub fn page(n: &Network, vcx: &ViewCx) -> Element<Msg> {
    let state = n.state();
    if n.is_live() && !state.available {
        return form::page(vec![
            form::heading("NetworkManager isn't running", "Networks are joined and set up through NetworkManager."),
            form::note(
                "On NixOS it's turned on with networking.networkmanager.enable = true; your account needs the networkmanager group (or polkit's permission) to change connections.",
            ),
        ]);
    }
    match n.page_now() {
        Page::Main => main(n, vcx, &state),
        Page::Saved => saved(vcx, &state),
        Page::Details(uuid) => details(n, vcx, &state, uuid),
    }
}

/// A row that's one pressable line in a card.
fn line<Msg: Clone + Send + 'static>(content: impl Into<Element<Msg>>, msg: Option<Msg>, name: &str) -> Row<Msg> {
    let content = container(content).min_height(24.0).align_y(Align::Center).width(Fill);
    form::whole(
        press(content)
            .style(PressStyle::Plain)
            .padding([8.0, 12.0])
            .on_press_maybe(msg)
            .named(name.to_owned())
            .width(Fill),
    )
}

/// A row's leading glyph, in the room a form row's tile takes, so
/// labels line up down a card.
fn lead<Msg: 'static>(glyph: impl Into<Element<Msg>>) -> Element<Msg> {
    container(glyph).width(24.0).center().into()
}

/// A name and a line under it.
fn two_lines<Msg: 'static>(title: impl Into<String>, sub: Option<String>, strong: bool) -> Element<Msg> {
    let mut t = text(title.into()).role(Role::Text);
    if strong {
        t = t.weight(600.0);
    }
    let mut col = column![t].spacing(2.0).width(Fill);
    if let Some(s) = sub.filter(|s| !s.is_empty()) {
        col = col.push(text(s).size(11.0).role(Role::Text3));
    }
    col.into()
}

fn lock<Msg: 'static>(security: Security) -> Element<Msg> {
    let glyph = if security == Security::Wep { Icon::TriangleAlert } else { Icon::Lock };
    icon(glyph).size(12.0).role(Role::Text3).tooltip(logic::security_words(security)).into()
}

/// A section with a caption that carries something at its right (a scan
/// spinner, a count).
fn section_with_tail(vcx: &ViewCx, title: &str, tail: Element<Msg>, rows: Vec<Row<Msg>>) -> Element<Msg> {
    column![
        row![form::caption(title), spacer(), tail].align(Align::Center).width(Fill).height(16.0),
        vcx.section("", rows),
    ]
    .spacing(6.0)
    .width(Fill)
    .into()
}

fn main(n: &Network, vcx: &ViewCx, state: &NetworkState) -> Element<Msg> {
    let mut sections: Vec<Element<Msg>> = Vec::new();
    let has_wifi = logic::has_wifi(state);
    let on = has_wifi && state.wifi_enabled && !state.airplane_mode;

    // Wi-Fi and airplane mode, with the network in use under the switch.
    let mut top: Vec<Row<Msg>> = Vec::new();
    if has_wifi {
        let can = state.wifi_hardware_enabled && !state.airplane_mode;
        top.push(
            form::row("Wi-Fi", toggle(state.wifi_enabled && !state.airplane_mode).on_toggle(Msg::Wifi).enabled(can))
                .leading(tile(if on { Icon::Wifi } else { Icon::WifiOff }, Tile::Hanada, 24.0))
                .detail(logic::wifi_subtitle(state))
                .entry("wifi"),
        );
        if on && let Some(current) = state.current_wifi() {
            top.push(current_row(state, current));
        }
    }
    top.push(
        form::row("Airplane Mode", toggle(state.airplane_mode).on_toggle(Msg::Airplane))
            .leading(tile(Icon::Plane, Tile::Kohaku, 24.0))
            .detail(if state.airplane_mode {
                "Every radio is off".to_owned()
            } else {
                "Turns off Wi-Fi, Bluetooth and mobile radios".to_owned()
            })
            .entry("airplane"),
    );
    let top = vcx.section("", top);
    // Wi-Fi off: what search finds in the network lists shows the switch
    // that brings them.
    sections.push(if on {
        top
    } else {
        ["networks", "known", "join-other"].iter().fold(top, |el, e| vcx.anchor(e, el))
    });

    if on {
        let (known, others) = logic::in_range(state);
        if !known.is_empty() {
            let rows = known.iter().map(|w| network_row(n, state, w)).collect::<Vec<_>>();
            sections.push(marked_section(vcx, "Known Networks", rows, "known"));
        }
        let mut rows: Vec<Row<Msg>> = others.iter().map(|w| network_row(n, state, w)).collect();
        if state.networks.is_empty() {
            rows.push(line(
                row![lead(spinner(14.0)), text("Looking for networks…").role(Role::Text3)].spacing(12.0),
                None,
                "Looking for networks",
            ));
        }
        rows.push(
            line(
                row![
                    lead(icon(Icon::CirclePlus).size(16.0).role(Role::Text2)),
                    two_lines(
                        "Other Network…",
                        Some("Join a hidden network by its name, or with an account".into()),
                        false
                    ),
                ]
                .spacing(12.0)
                .align(Align::Center),
                Some(Msg::JoinOther),
                "Other Network",
            )
            .entry("join-other"),
        );
        let tail: Element<Msg> = if n.is_scanning() {
            spinner(14.0)
        } else {
            icon_button(Icon::RefreshCw)
                .plain()
                .compact()
                .on_press(Msg::Scan)
                .tooltip("Look for networks")
                .label("Look for networks")
                .into()
        };
        let mut other = if rows.first().is_some_and(|r| !r.has_entry()) {
            section_with_tail(vcx, "Other Networks", tail, mark_first(rows, "networks"))
        } else {
            // Only "Other Network…": it keeps its own entry, and the
            // section answers for the list.
            vcx.anchor("networks", section_with_tail(vcx, "Other Networks", tail, rows))
        };
        if known.is_empty() {
            other = vcx.anchor("known", other);
        }
        sections.push(other);
    }

    // Wired.
    let wired = logic::wired(state);
    if !wired.is_empty() {
        let rows = wired
            .iter()
            .map(|w| {
                let mut r = row![
                    lead(icon(Icon::EthernetPort).size(18.0).role(if w.connected { Role::Text } else { Role::Text3 })),
                    two_lines(w.title.clone(), Some(w.status.clone()), w.connected),
                ]
                .spacing(12.0)
                .align(Align::Center)
                .width(Fill);
                if let Some(uuid) = &w.uuid {
                    r = r.push(details_button(uuid));
                }
                line(r, None, &w.title)
            })
            .collect();
        sections.push(marked_section(vcx, "Wired", rows, "wired"));
    }

    // VPN.
    let mut rows: Vec<Row<Msg>> = state.vpns().map(|c| vpn_row(n, state, c)).collect();
    rows.push(
        form::row("Add VPN", button("From File…").on_press(Msg::ImportVpn))
            .leading(lead(icon(Icon::FilePlus).size(16.0).role(Role::Text2)))
            .help("A WireGuard .conf or OpenVPN .ovpn file from your provider")
            .entry("add-vpn"),
    );
    sections.push(marked_section(vcx, "VPN", rows, "vpn"));

    // Hotspot.
    if has_wifi && logic::hotspot_possible(state) {
        let active = state.hotspot().is_some();
        let detail = match logic::hotspot_name(state) {
            Some(name) => format!("Sharing as “{name}”"),
            None => "Share this computer's connection over Wi-Fi".to_owned(),
        };
        sections.push(vcx.section(
            "",
            vec![
                form::row("Hotspot", toggle(active).on_toggle(Msg::Hotspot).enabled(state.wifi_enabled))
                    .leading(tile(Icon::RadioTower, Tile::Violet, 24.0))
                    .detail(detail)
                    .entry("hotspot"),
            ],
        ));
    }

    // Saved networks.
    let count = state.saved_wifi().count();
    if has_wifi && count > 0 {
        let words = if count == 1 { "1 network".to_owned() } else { format!("{count} networks") };
        sections.push(vcx.section(
            "",
            vec![
                    line(
                        row![
                            lead(icon(Icon::Bookmark).size(16.0).role(Role::Text2)),
                            two_lines("Saved Networks", Some(words), false),
                            icon(Icon::ChevronRight).size(14.0).role(Role::Text3)
                        ]
                        .spacing(12.0)
                        .align(Align::Center)
                        .width(Fill),
                        Some(Msg::Open(Page::Saved)),
                        "Saved Networks",
                    )
                    .entry("saved"),
                ],
        ));
    }
    form::page(sections)
}

/// The first row of a section answers to its search entry (unless it's
/// an entry of its own).
fn mark_first(mut rows: Vec<Row<Msg>>, entry: &str) -> Vec<Row<Msg>> {
    if rows.first().is_some_and(|r| !r.has_entry()) {
        let first = rows.remove(0);
        rows.insert(0, first.entry(entry));
    }
    rows
}

/// A section that answers to a search entry: by its first row, or, when
/// that row is an entry of its own ("Add VPN" with no VPNs yet), as a
/// whole, so both are found.
fn marked_section(vcx: &ViewCx, title: &str, rows: Vec<Row<Msg>>, entry: &str) -> Element<Msg> {
    if rows.first().is_some_and(|r| !r.has_entry()) {
        vcx.section(title, mark_first(rows, entry))
    } else {
        vcx.anchor(entry, vcx.section(title, rows))
    }
}

fn details_button(uuid: &str) -> Element<Msg> {
    icon_button(Icon::Info)
        .plain()
        .compact()
        .on_press(Msg::Open(Page::Details(uuid.to_owned())))
        .tooltip("Details")
        .label("Details")
        .into()
}

fn current_row(state: &NetworkState, w: &WifiNetwork) -> Row<Msg> {
    let details = state.wifi_device().and_then(|d| state.details(&d.path));
    let mut words = vec!["Connected".to_owned()];
    if let Some(a) = details.as_ref().and_then(|d| d.ipv4.address()) {
        words.push(a.to_owned());
    }
    if let Some(f) = details.as_ref().and_then(|d| d.frequency_mhz) {
        words.push(logic::band_words(f));
    }
    let mut r = row![lead(signal(w.bars(), 18.0, true)), two_lines(w.ssid.clone(), Some(words.join(" · ")), true)]
        .spacing(12.0)
        .align(Align::Center)
        .width(Fill);
    if w.security.needs_password() || w.security == Security::EnhancedOpen {
        r = r.push(lock(w.security));
    }
    if let Some(uuid) = &w.connection {
        r = r.push(button("Details…").compact().on_press(Msg::Open(Page::Details(uuid.clone()))));
    }
    line(r, None, &w.ssid).entry("current")
}

fn network_row(n: &Network, state: &NetworkState, w: &WifiNetwork) -> Row<Msg> {
    let joining = n.joining_now() == Some(w.ssid.as_str()) || w.connecting;
    let sub = if w.known {
        state.saved_for(&w.ssid).filter(|c| !c.autoconnect).map(|_| "Automatic joining off".to_owned())
    } else if w.security == Security::Enterprise {
        Some("Sign in with an account".to_owned())
    } else if w.security == Security::Wep {
        Some("WEP, not secure".to_owned())
    } else {
        None
    };
    let mut r = row![lead(signal(w.bars(), 18.0, false)), two_lines(w.ssid.clone(), sub, false)]
        .spacing(12.0)
        .align(Align::Center)
        .width(Fill);
    if joining {
        r = r.push(spinner(14.0));
    } else if w.security != Security::Open {
        r = r.push(lock(w.security));
    }
    if let Some(uuid) = &w.connection {
        r = r.push(details_button(uuid));
    }
    let msg = (!joining).then(|| Msg::Join(w.ssid.clone()));
    line(r, msg, &w.ssid)
}

fn vpn_row(n: &Network, state: &NetworkState, c: &SavedConnection) -> Row<Msg> {
    let words = logic::vpn_state(state, c);
    let up = matches!(words, "Connected" | "Connecting…");
    let busy = n.is_busy(&c.uuid) || words.ends_with('…');
    let mut r = row![
        lead(icon(if up { Icon::ShieldCheck } else { Icon::Shield }).size(18.0).role(if up {
            Role::Success
        } else {
            Role::Text3
        })),
        two_lines(c.id.clone(), Some(format!("{} · {words}", logic::vpn_kind(c))), up),
    ]
    .spacing(12.0)
    .align(Align::Center)
    .width(Fill);
    if busy {
        r = r.push(spinner(14.0));
    }
    let uuid = c.uuid.clone();
    r = r.push(toggle(up).on_toggle(move |on| Msg::Vpn(uuid.clone(), on)).enabled(!busy));
    r = r.push(details_button(&c.uuid));
    line(r, None, &c.id)
}

/// The top of a sub-page: back, and its heading.
fn back_heading(back: &str, title: &str, sub: &str) -> Element<Msg> {
    column![
        button(back.to_owned()).plain().compact().icon(Icon::ChevronLeft).on_press(Msg::Back),
        form::heading(title.to_owned(), sub.to_owned()),
    ]
    .spacing(8.0)
    .into()
}

fn saved(vcx: &ViewCx, state: &NetworkState) -> Element<Msg> {
    let now = logic::now_secs();
    let mut list: Vec<&SavedConnection> = state.saved_wifi().collect();
    list.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| a.id.cmp(&b.id)));
    let rows: Vec<Row<Msg>> = list
        .iter()
        .map(|c| {
            let in_range = c.ssid.as_deref().and_then(|s| state.network(s));
            let glyph: Element<Msg> = match in_range {
                Some(w) => signal(w.bars(), 16.0, c.active),
                None => icon(Icon::Wifi).size(16.0).role(Role::Faint).into(),
            };
            line(
                row![
                    lead(glyph),
                    two_lines(c.id.clone(), Some(logic::saved_words(c, now)), c.active),
                    icon(Icon::ChevronRight).size(14.0).role(Role::Text3)
                ]
                .spacing(12.0)
                .align(Align::Center)
                .width(Fill),
                Some(Msg::Open(Page::Details(c.uuid.clone()))),
                &c.id,
            )
        })
        .collect();
    let mut sections = vec![back_heading(
        "Network",
        "Saved Networks",
        "Networks this computer has joined. Open one to change how it joins, or to forget it.",
    )];
    if rows.is_empty() {
        sections.push(form::note("No saved networks."));
    } else {
        sections.push(vcx.section("", rows));
    }
    form::page(sections)
}

fn status_words(state: &NetworkState, c: &SavedConnection) -> &'static str {
    match state.active_connection(&c.uuid).map(|a| a.state) {
        Some(aizome_sys::network::ActiveState::Activating) => "Connecting…",
        Some(aizome_sys::network::ActiveState::Activated) => "Connected",
        Some(aizome_sys::network::ActiveState::Deactivating) => "Disconnecting…",
        _ if c.active => "Connected",
        _ => "Not connected",
    }
}

fn kind_words(c: &SavedConnection) -> String {
    match c.kind {
        ConnectionKind::Wifi => "Wi-Fi".into(),
        ConnectionKind::Ethernet => "Ethernet".into(),
        ConnectionKind::Vpn | ConnectionKind::WireGuard => logic::vpn_kind(c),
        ConnectionKind::Bluetooth => "Bluetooth".into(),
        ConnectionKind::Mobile => "Mobile broadband".into(),
        _ => "Connection".into(),
    }
}

fn details(n: &Network, vcx: &ViewCx, state: &NetworkState, uuid: &str) -> Element<Msg> {
    let back = if n.came_from_saved() { "Saved Networks" } else { "Network" };
    let Some(c) = state.connection(uuid) else {
        return form::page(vec![back_heading(back, "Connection", ""), form::note("This connection was removed.")]);
    };
    let words = status_words(state, c);
    let wifi = c.kind == ConnectionKind::Wifi;
    let vpn = c.kind.is_vpn();
    let mut sections: Vec<Element<Msg>> = vec![back_heading(back, &c.id, &format!("{} · {words}", kind_words(c)))];

    // Status: up or down, and its addresses while up.
    let up = matches!(words, "Connected" | "Connecting…");
    let in_range = c.ssid.as_deref().is_none_or(|s| state.wifi_enabled && state.network(s).is_some());
    let busy = n.is_busy(uuid) || words.ends_with('…') || c.ssid.as_deref().is_some_and(|s| n.joining_now() == Some(s));
    let action: Element<Msg> = if busy {
        spinner(16.0)
    } else if up {
        button("Disconnect").on_press(Msg::Disconnect(uuid.to_owned())).into()
    } else {
        let label = if wifi { "Join" } else { "Connect" };
        let b = button(label).primary().on_press_maybe(in_range.then(|| Msg::Connect(uuid.to_owned())));
        let b = if in_range { b } else { b.enabled(false) };
        let e: Element<Msg> = b.into();
        if in_range { e } else { e.tooltip("Not in range") }
    };
    let mut status: Vec<Row<Msg>> = vec![form::row("Status", action).detail(words)];
    let device = state.active_connection(uuid).and_then(|a| a.devices.first().cloned());
    if let Some(d) = device.as_deref().and_then(|p| state.details(p)) {
        let mut ips: Vec<String> = d.ipv4.addresses.clone();
        ips.extend(d.ipv6.addresses.iter().filter(|a| !a.starts_with("fe80")).cloned());
        if !ips.is_empty() {
            status.push(form::value_mono("IP address", ips.join("\n")));
        }
        if let Some(g) = &d.ipv4.gateway {
            status.push(form::value_mono("Router", g.clone()));
        }
        let mut dns = d.ipv4.dns.clone();
        dns.extend(d.ipv6.dns.iter().cloned());
        if !dns.is_empty() {
            status.push(form::value_mono("DNS servers", dns.join("\n")));
        }
        if !vpn && !d.hardware_address.is_empty() {
            status.push(form::value_mono("Hardware address", d.hardware_address.clone()));
        }
        match (d.speed_mbps.filter(|s| *s > 0), d.frequency_mhz) {
            (Some(s), Some(f)) => {
                status.push(form::value("Link", format!("{} · {}", logic::speed_words(s), logic::band_words(f))))
            }
            (Some(s), None) => status.push(form::value("Link speed", logic::speed_words(s))),
            _ => {}
        }
    }
    sections.push(vcx.section("", status));

    let profile = match n.loaded_profile(uuid) {
        None => {
            sections.push(row![spinner(14.0), text("Reading its settings…").role(Role::Text3)].spacing(8.0).into());
            return form::page(sections);
        }
        Some(Err(e)) => {
            sections.push(form::note(format!("Couldn't read its settings: {e}")));
            return form::page(sections);
        }
        Some(Ok(p)) => p,
    };
    let edit = n.edit_now();

    // Joining.
    let mut joining: Vec<Row<Msg>> = Vec::new();
    let u = uuid.to_owned();
    joining.push(
        form::row(
            if wifi { "Join automatically" } else { "Connect automatically" },
            toggle(profile.autoconnect).on_toggle(move |on| Msg::Autoconnect(u.clone(), on)),
        )
        .entry("auto-join"),
    );
    if profile.has_ip() {
        let u = uuid.to_owned();
        joining.push(
            form::row(
                "Metered connection",
                toggle(profile.metered == aizome_sys::network::Metered::Yes)
                    .on_toggle(move |on| Msg::Metered(u.clone(), on)),
            )
            .help("Apps may wait to download large updates on it")
            .entry("metered"),
        );
    }
    if wifi {
        let current = Mac::of(&profile.assigned_mac);
        let mut choices: Vec<Mac> = Mac::CHOICES.to_vec();
        if matches!(current, Mac::Other(_)) {
            choices.push(current.clone());
        }
        let labels: Vec<String> = choices.iter().map(Mac::label).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let selected = choices.iter().position(|m| *m == current).unwrap_or(0);
        let u = uuid.to_owned();
        joining.push(
            form::row("Private address", form::choice(&labels, selected, move |i| Msg::PrivateAddress(u.clone(), choices.get(i).cloned().unwrap_or(Mac::Default))))
                .help("A private hardware address keeps networks from following this computer. Applies the next time it joins.")
                .entry("private-address"),
        );
        if matches!(profile.key_mgmt.as_deref(), Some("wpa-psk" | "sae" | "wpa-eap")) {
            let help = match &profile.eap_identity {
                Some(id) => format!("Signed in as {id}; kept by this computer"),
                None => "Kept by this computer".to_owned(),
            };
            joining.push(
                form::row("Password", button("Change…").on_press(Msg::ChangePassword(uuid.to_owned()))).help(help),
            );
        }
    }
    sections.push(vcx.section("Joining", joining));

    if profile.has_ip() {
        sections.push(ipv4_section(vcx, uuid, profile, edit));
    }
    if c.kind != ConnectionKind::Unknown {
        sections.push(dns_section(vcx, uuid, profile, edit));
    }
    if profile.has_ip() {
        sections.push(ipv6_section(vcx, uuid, profile));
    }
    if profile.proxy.automatic || profile.proxy.pac_url.is_some() {
        let words = match &profile.proxy.pac_url {
            Some(url) => format!("Automatic, from {url}"),
            None => "Automatic (WPAD)".to_owned(),
        };
        sections.push(vcx.section(
            "Proxy",
            vec![
                    form::value("Proxy", words)
                        .help_maybe(profile.proxy.browser_only.then(|| "For web browsers only".to_owned()))
                        .entry("proxy"),
                ],
        ));
    }

    if wifi || vpn {
        let (label, help) = if wifi {
            ("Forget This Network…", "This computer stops joining it, and its password is removed.")
        } else {
            ("Remove This VPN…", "Its configuration is removed from this computer.")
        };
        sections.push(
            vcx.section("", vec![form::row(help, button(label).destructive().on_press(Msg::Forget(uuid.to_owned())))]),
        );
    }
    form::page(sections)
}

fn ipv4_section(vcx: &ViewCx, uuid: &str, p: &Profile, edit: &super::Edit) -> Element<Msg> {
    let mut methods: Vec<(String, String)> =
        vec![("auto".into(), "Automatically (DHCP)".into()), ("manual".into(), "Manually".into())];
    if !["auto", "manual"].contains(&p.ipv4_method.as_str()) {
        methods.push((p.ipv4_method.clone(), format!("Other ({})", p.ipv4_method)));
    }
    let manual = edit.manual || p.ipv4_method == "manual";
    let selected = if edit.manual { 1 } else { methods.iter().position(|(m, _)| *m == p.ipv4_method).unwrap_or(0) };
    let labels: Vec<&str> = methods.iter().map(|(_, l)| l.as_str()).collect();
    let u = uuid.to_owned();
    let other = methods.len() > 2;
    let mut rows = vec![
        form::row(
            "Configure IPv4",
            form::choice(&labels, selected, move |i| {
                // An unknown method chosen again changes nothing.
                if other && i == 2 { Msg::Done(String::new(), Ok(())) } else { Msg::Ipv4(u.clone(), i) }
            }),
        )
        .entry("ipv4"),
    ];
    if manual {
        let u = uuid.to_owned();
        rows.push(form::row(
            "IP address",
            text_field(&edit.address)
                .placeholder("192.168.1.20/24")
                .mono()
                .on_change(Msg::AddressText)
                .on_submit(Msg::ApplyAddress(u.clone()))
                .id("network-address")
                .width(200.0),
        ));
        rows.push(
            form::row(
                "Router",
                row![
                    text_field(&edit.router)
                        .placeholder("192.168.1.1")
                        .mono()
                        .on_change(Msg::RouterText)
                        .on_submit(Msg::ApplyAddress(u.clone()))
                        .id("network-router")
                        .width(140.0),
                    button("Apply").on_press(Msg::ApplyAddress(u)),
                ]
                .spacing(8.0),
            )
            .help_maybe(edit.address_error.clone()),
        );
    }
    vcx.section("IPv4", rows)
}

fn dns_section(vcx: &ViewCx, uuid: &str, p: &Profile, edit: &super::Edit) -> Element<Msg> {
    let current = if edit.custom { Dns::Custom } else { Dns::of(p.ipv4_ignore_auto_dns, &p.dns()) };
    let labels: Vec<&str> = Dns::ALL.iter().map(|d| d.name()).collect();
    let selected = Dns::ALL.iter().position(|d| *d == current).unwrap_or(0);
    let u = uuid.to_owned();
    let help = match current {
        Dns::Automatic => Some("The network's own".to_owned()),
        Dns::Custom => None,
        d => Some(d.servers().join(" · ")),
    };
    let mut rows = vec![
        form::row(
            "DNS servers",
            form::choice(&labels, selected, move |i| Msg::DnsChoice(u.clone(), Dns::ALL[i.min(Dns::ALL.len() - 1)])),
        )
        .help_maybe(help)
        .entry("dns"),
    ];
    if current == Dns::Custom {
        let u = uuid.to_owned();
        rows.push(
            form::row(
                "Servers",
                row![
                    text_field(&edit.dns)
                        .placeholder("9.9.9.9 2620:fe::fe")
                        .mono()
                        .on_change(Msg::DnsText)
                        .on_submit(Msg::ApplyDns(u.clone()))
                        .id("network-dns")
                        .width(220.0),
                    button("Apply").on_press(Msg::ApplyDns(u)),
                ]
                .spacing(8.0),
            )
            .help(edit.dns_error.clone().unwrap_or_else(|| "Separated by spaces".to_owned())),
        );
    }
    vcx.section("DNS", rows)
}

fn ipv6_section(vcx: &ViewCx, uuid: &str, p: &Profile) -> Element<Msg> {
    let mut methods: Vec<(String, String)> =
        vec![("auto".into(), "Automatically".into()), ("disabled".into(), "Off".into())];
    if !["auto", "disabled"].contains(&p.ipv6_method.as_str()) {
        methods.push((p.ipv6_method.clone(), format!("Other ({})", p.ipv6_method)));
    }
    let selected = methods.iter().position(|(m, _)| *m == p.ipv6_method).unwrap_or(0);
    let labels: Vec<&str> = methods.iter().map(|(_, l)| l.as_str()).collect();
    let values: Vec<String> = methods.iter().map(|(m, _)| m.clone()).collect();
    let u = uuid.to_owned();
    let mut rows = vec![
        form::row(
            "Configure IPv6",
            form::choice(&labels, selected, move |i| {
                Msg::Ipv6(u.clone(), values.get(i).cloned().unwrap_or_else(|| "auto".into()))
            }),
        )
        .entry("ipv6"),
    ];
    if !p.ipv6_addresses.is_empty() {
        rows.push(form::value_mono("Fixed addresses", p.ipv6_addresses.join("\n")));
    }
    vcx.section("IPv6", rows)
}

// Sheets.

pub fn sheet(n: &Network, sheet: &Sheet, _vcx: &ViewCx) -> Element<Msg> {
    let _ = n;
    let body: Element<Msg> = match sheet {
        Sheet::Password { security, title, message, password, show, error, .. } => {
            let field = text_field(password).placeholder("Password").on_change(Msg::SheetPassword).on_submit(Msg::SheetSubmit).id("network-password").width(Fill);
            let field = if *show { field } else { field.password() };
            let ok = logic::password_ok(*security, password);
            column![
                text(title.clone()).size(15.0).weight(650.0),
                text(message.clone()).role(Role::Text2).wrap(),
                field,
                checkbox("Show password", *show).on_toggle(Msg::ShowPassword),
                error_line(error.clone().or_else(|| {
                    (!password.is_empty() && !ok).then(|| format!("At least {} characters", logic::minimum_password(*security)))
                })),
                buttons("Join", ok),
            ]
            .spacing(12.0)
            .into()
        }
        Sheet::Form(f) => form_sheet(f),
        Sheet::Hotspot { name, password, error } => column![
            text("Share This Connection").size(15.0).weight(650.0),
            text("Other devices join this network with its password. While it's on, this computer's Wi-Fi joins no other network.").role(Role::Text2).wrap(),
            labelled("Network name", text_field(name).on_change(Msg::HotspotName).id("network-hotspot-name").width(Fill)),
            labelled("Password", text_field(password).password().placeholder("8 to 63 characters").on_change(Msg::HotspotPassword).on_submit(Msg::SheetSubmit).id("network-hotspot-password").width(Fill)),
            error_line(error.clone()),
            buttons("Turn On", !name.trim().is_empty() && (8..=63).contains(&password.chars().count())),
        ]
        .spacing(12.0)
        .into(),
    };
    container(body).padding([20.0, 20.0, 16.0, 20.0]).width(420.0).into()
}

fn labelled(label: &str, field: impl Into<Element<Msg>>) -> Element<Msg> {
    column![text(label.to_owned()).size(11.0).role(Role::Text3), field.into()].spacing(4.0).width(Fill).into()
}

fn error_line(e: Option<String>) -> Element<Msg> {
    match e {
        Some(e) => text(e).size(11.0).role(Role::Danger).wrap().into(),
        None => space(0.0),
    }
}

fn buttons(action: &str, ok: bool) -> Element<Msg> {
    row![
        spacer(),
        button("Cancel").cancel().on_press(Msg::SheetCancel),
        button(action.to_owned()).primary().default().on_press(Msg::SheetSubmit).enabled(ok),
    ]
    .spacing(8.0)
    .width(Fill)
    .into()
}

fn form_sheet(f: &JoinForm) -> Element<Msg> {
    let title = if f.hidden { "Join a Network by Name".to_owned() } else { format!("Join “{}”", f.name) };
    let message = if f.hidden {
        "Enter the network's name and how it's secured."
    } else {
        "This network signs you in with an account from your organization."
    };
    let mut col =
        column![text(title).size(15.0).weight(650.0), text(message).role(Role::Text2).wrap()].spacing(12.0).width(Fill);
    if f.hidden {
        let labels: Vec<&str> = FORM_SECURITY.iter().map(|(l, _)| *l).collect();
        col = col.push(labelled(
            "Network name",
            text_field(&f.name).on_change(|v| Msg::SheetField(Field::Name(v))).id("network-name").width(Fill),
        ));
        col = col.push(labelled(
            "Security",
            dropdown(labels.iter().map(|s| (*s).to_owned()), f.security)
                .on_select(|i| Msg::SheetField(Field::Security(i)))
                .width(Fill),
        ));
    }
    if f.enterprise() {
        let methods: Vec<String> = EAP_METHODS.iter().map(|(l, _)| (*l).to_owned()).collect();
        let inner: Vec<String> = EAP_INNER.iter().map(|(l, _)| (*l).to_owned()).collect();
        col = col.push(
            row![
                labelled(
                    "Method",
                    dropdown(methods, f.method).on_select(|i| Msg::SheetField(Field::Method(i))).width(Fill)
                )
                .width(Fill),
                labelled(
                    "Inner authentication",
                    dropdown(inner, f.inner).on_select(|i| Msg::SheetField(Field::Inner(i))).width(Fill)
                )
                .width(Fill),
            ]
            .spacing(12.0)
            .width(Fill),
        );
        col = col.push(labelled(
            "Username",
            text_field(&f.username)
                .on_change(|v| Msg::SheetField(Field::Username(v)))
                .id("network-username")
                .width(Fill),
        ));
        col = col.push(labelled(
            "Password",
            text_field(&f.password)
                .password()
                .on_change(|v| Msg::SheetField(Field::Password(v)))
                .on_submit(Msg::SheetSubmit)
                .width(Fill),
        ));
        col = col.push(
            checkbox("Check the server's certificate", f.validate).on_toggle(|v| Msg::SheetField(Field::Validate(v))),
        );
        if f.validate {
            col = col.push(labelled(
                "Server domain",
                text_field(&f.domain)
                    .placeholder("radius.example.org")
                    .on_change(|v| Msg::SheetField(Field::Domain(v)))
                    .width(Fill),
            ));
        } else {
            col = col.push(
                text("Without the check, a look-alike network could take your password.")
                    .size(11.0)
                    .role(Role::Warning)
                    .wrap(),
            );
        }
        col = col.push(labelled(
            "Anonymous identity (optional)",
            text_field(&f.anonymous).on_change(|v| Msg::SheetField(Field::Anonymous(v))).width(Fill),
        ));
    } else if let Some(s) = f.personal() {
        col = col.push(labelled(
            "Password",
            text_field(&f.password)
                .password()
                .placeholder(format!("At least {} characters", logic::minimum_password(s)))
                .on_change(|v| Msg::SheetField(Field::Password(v)))
                .on_submit(Msg::SheetSubmit)
                .width(Fill),
        ));
    }
    col = col.push(error_line(f.error.clone()));
    col = col.push(buttons("Join", f.ready()));
    col.into()
}
