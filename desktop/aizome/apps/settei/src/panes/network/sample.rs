//! The Network pane's sample: a laptop with Wi-Fi in a busy building, an
//! unplugged Ethernet port and two VPNs, for `--sample`, tests and golden
//! images. Actions change the sample as NetworkManager would, so the pane
//! can be driven without touching the machine.

use aizome_sys::network::{
    AccessPoint, ActiveConnection, ActiveState, Band, ConnectionKind, Device, DeviceKind, DeviceState, IpConfig,
    Metered, NetworkState, NmState, Profile, Proxy, SavedConnection, Security, WifiNetwork,
};

const WLAN: &str = "/org/freedesktop/NetworkManager/Devices/3";
const ETH: &str = "/org/freedesktop/NetworkManager/Devices/2";

fn network(ssid: &str, strength: u8, security: Security, bands: &[Band], uuid: Option<&str>) -> WifiNetwork {
    WifiNetwork {
        ssid: ssid.into(),
        ssid_bytes: ssid.as_bytes().to_vec(),
        strength,
        security,
        bands: bands.to_vec(),
        best_access_point: format!("/ap/{ssid}"),
        access_points: vec![format!("/ap/{ssid}")],
        device: WLAN.into(),
        known: uuid.is_some(),
        connection: uuid.map(str::to_owned),
        connected: false,
        connecting: false,
    }
}

fn saved(
    uuid: &str,
    id: &str,
    kind: ConnectionKind,
    ssid: Option<&str>,
    key_mgmt: Option<&str>,
    days_ago: u64,
) -> SavedConnection {
    let now = super::logic::now_secs();
    SavedConnection {
        path: format!("/settings/{uuid}"),
        uuid: uuid.into(),
        id: id.into(),
        kind,
        autoconnect: true,
        timestamp: if days_ago == u64::MAX { 0 } else { now.saturating_sub(days_ago * 86_400) },
        ssid: ssid.map(str::to_owned),
        key_mgmt: key_mgmt.map(str::to_owned),
        hidden: false,
        wifi_mode: ssid.map(|_| "infrastructure".to_owned()),
        metered: Metered::Unknown,
        interface_name: None,
        vpn_service: None,
        proxy: Proxy::default(),
        active: false,
        unsaved: false,
        filename: String::new(),
    }
}

/// The sample's state, with "Kohaku" joined.
pub fn state() -> NetworkState {
    let mut networks = vec![
        network("Kohaku", 86, Security::Wpa2Wpa3Personal, &[Band::TwoPointFourGhz, Band::FiveGhz], Some("c-kohaku")),
        network("Aizome Studio", 64, Security::Wpa2Personal, &[Band::FiveGhz], Some("c-studio")),
        network("Hanada Café", 71, Security::Open, &[Band::TwoPointFourGhz], None),
        network("Tōkyō Guest", 52, Security::Wpa3Personal, &[Band::FiveGhz, Band::SixGhz], None),
        network("Shitamachi Corp", 44, Security::Enterprise, &[Band::FiveGhz], None),
        network("Neighbour 2G", 18, Security::Wpa2Personal, &[Band::TwoPointFourGhz], None),
        network("Old Router", 30, Security::Wep, &[Band::TwoPointFourGhz], None),
    ];
    networks[0].connected = true;
    let mut connections = vec![
        saved("c-kohaku", "Kohaku", ConnectionKind::Wifi, Some("Kohaku"), Some("sae"), 0),
        saved("c-studio", "Aizome Studio", ConnectionKind::Wifi, Some("Aizome Studio"), Some("wpa-psk"), 3),
        saved("c-airport", "Haneda Free Wi-Fi", ConnectionKind::Wifi, Some("Haneda Free Wi-Fi"), None, 45),
        saved("c-hotel", "Hotel Sakura", ConnectionKind::Wifi, Some("Hotel Sakura"), Some("wpa-psk"), 200),
        saved("c-office-vpn", "Office", ConnectionKind::WireGuard, None, None, 1),
        saved("c-home-vpn", "Home (OpenVPN)", ConnectionKind::Vpn, None, None, 30),
        saved("c-eth", "Wired connection 1", ConnectionKind::Ethernet, None, None, 12),
    ];
    connections[0].active = true;
    connections[3].autoconnect = false;
    connections[5].vpn_service = Some("org.freedesktop.NetworkManager.openvpn".into());
    let ip4 = IpConfig {
        addresses: vec!["192.168.10.42/24".into()],
        gateway: Some("192.168.10.1".into()),
        dns: vec!["192.168.10.1".into()],
        domains: vec!["lan".into()],
    };
    let ip6 = IpConfig {
        addresses: vec!["fd00:10::42/64".into(), "fe80::5a2c:80ff:fe13:42/64".into()],
        gateway: Some("fe80::1".into()),
        dns: Vec::new(),
        domains: Vec::new(),
    };
    let wlan = Device {
        path: WLAN.into(),
        interface: "wlp1s0".into(),
        kind: DeviceKind::Wifi,
        state: DeviceState::Activated,
        managed: true,
        autoconnect: true,
        driver: "iwlwifi".into(),
        hw_address: "5A:2C:80:13:00:42".into(),
        perm_hw_address: "A8:7E:EA:4C:19:D0".into(),
        bitrate_kbps: Some(866_000),
        active_connection: Some("/active/1".into()),
        active_access_point: Some("/ap/Kohaku".into()),
        ip4: ip4.clone(),
        ip6: ip6.clone(),
        mtu: 1500,
        ..Device::default()
    };
    let eth = Device {
        path: ETH.into(),
        interface: "enp2s0".into(),
        kind: DeviceKind::Ethernet,
        state: DeviceState::Unavailable,
        managed: true,
        autoconnect: true,
        driver: "r8169".into(),
        hw_address: "00:E0:4C:68:01:9A".into(),
        carrier: Some(false),
        mtu: 1500,
        ..Device::default()
    };
    let access_points = networks
        .iter()
        .map(|n| AccessPoint {
            path: n.best_access_point.clone(),
            device: WLAN.into(),
            ssid: n.ssid.clone(),
            ssid_bytes: n.ssid_bytes.clone(),
            bssid: "00:00:00:00:00:00".into(),
            frequency: if n.bands.contains(&Band::FiveGhz) { 5180 } else { 2437 },
            band: n.bands.last().copied().unwrap_or(Band::TwoPointFourGhz),
            strength: n.strength,
            security: n.security,
            max_bitrate_kbps: 866_000,
            infrastructure: true,
        })
        .collect();
    NetworkState {
        available: true,
        version: "1.58.1".into(),
        state: NmState::ConnectedGlobal,
        connectivity: aizome_sys::network::Connectivity::Full,
        networking_enabled: true,
        wifi_enabled: true,
        wifi_hardware_enabled: true,
        wwan_enabled: false,
        wwan_hardware_enabled: false,
        airplane_mode: false,
        radios: Vec::new(),
        metered: Metered::GuessNo,
        primary_connection: Some("/active/1".into()),
        hostname: "kohaku-laptop".into(),
        can_modify: true,
        devices: vec![eth, wlan],
        access_points,
        networks,
        connections,
        active: vec![ActiveConnection {
            path: "/active/1".into(),
            connection: "/settings/c-kohaku".into(),
            uuid: "c-kohaku".into(),
            id: "Kohaku".into(),
            kind: "802-11-wireless".into(),
            state: ActiveState::Activated,
            devices: vec![WLAN.into()],
            default4: true,
            default6: true,
            vpn: false,
            specific_object: Some("/ap/Kohaku".into()),
            ip4,
            ip6,
        }],
    }
}

/// A sample connection's profile, made from its saved connection.
pub fn profile(state: &NetworkState, uuid: &str) -> Option<Profile> {
    let c = state.connection(uuid)?;
    Some(Profile {
        id: c.id.clone(),
        uuid: c.uuid.clone(),
        kind: c.kind.clone(),
        autoconnect: c.autoconnect,
        metered: c.metered,
        interface_name: c.interface_name.clone(),
        ipv4_method: "auto".into(),
        ipv6_method: "auto".into(),
        ssid: c.ssid.clone(),
        key_mgmt: c.key_mgmt.clone(),
        hidden: c.hidden,
        vpn_service: c.vpn_service.clone(),
        assigned_mac: if c.kind == ConnectionKind::Wifi { "stable".into() } else { String::new() },
        ..Profile::default()
    })
}

/// Join a network in the sample: the password "wrong" is refused; the
/// network becomes the one in use, saved if it wasn't.
pub fn join(state: &mut NetworkState, ssid: &str, password: Option<&str>) -> Result<(), aizome_sys::Error> {
    if password == Some("wrong") {
        return Err(aizome_sys::Error::Rejected("The password was not accepted".into()));
    }
    let Some(i) = state.networks.iter().position(|n| n.ssid == ssid) else {
        return Err(aizome_sys::Error::Failed("The network is out of range".into()));
    };
    let uuid =
        state.networks[i].connection.clone().unwrap_or_else(|| format!("c-{}", ssid.to_lowercase().replace(' ', "-")));
    if state.connection(&uuid).is_none() {
        let security = state.networks[i].security;
        state.connections.insert(0, saved(&uuid, ssid, ConnectionKind::Wifi, Some(ssid), security.key_mgmt(), 0));
    }
    for n in &mut state.networks {
        n.connected = n.ssid == ssid;
        if n.connected {
            n.known = true;
            n.connection = Some(uuid.clone());
        }
    }
    for c in &mut state.connections {
        if c.kind == ConnectionKind::Wifi {
            c.active = c.uuid == uuid;
        }
    }
    if let Some(a) = state.active.iter_mut().find(|a| a.kind == "802-11-wireless") {
        a.uuid = uuid.clone();
        a.id = ssid.to_owned();
    }
    Ok(())
}

/// Turn Wi-Fi off or on in the sample (networks go while it's off).
pub fn set_wifi(state: &mut NetworkState, on: bool) {
    state.wifi_enabled = on;
    if on {
        let fresh = self::state();
        state.networks = fresh.networks;
        for n in &mut state.networks {
            n.connected = false;
        }
    } else {
        state.networks.clear();
        for c in &mut state.connections {
            if c.kind == ConnectionKind::Wifi {
                c.active = false;
            }
        }
    }
}

/// Bring a VPN up or down in the sample.
pub fn set_vpn(state: &mut NetworkState, uuid: &str, on: bool) {
    let Some(c) = state.connections.iter_mut().find(|c| c.uuid == uuid) else { return };
    c.active = on;
    let (id, kind) = (c.id.clone(), c.kind.clone());
    state.active.retain(|a| a.uuid != uuid);
    if on {
        state.active.push(ActiveConnection {
            path: format!("/active/{uuid}"),
            uuid: uuid.into(),
            id,
            kind: if kind == ConnectionKind::WireGuard { "wireguard".into() } else { "vpn".into() },
            state: ActiveState::Activated,
            vpn: true,
            ..ActiveConnection::default()
        });
    }
}

/// Forget a connection in the sample.
pub fn forget(state: &mut NetworkState, uuid: &str) {
    state.connections.retain(|c| c.uuid != uuid);
    for n in &mut state.networks {
        if n.connection.as_deref() == Some(uuid) {
            n.connection = None;
            n.known = false;
            n.connected = false;
        }
    }
}
