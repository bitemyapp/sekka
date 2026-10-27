// SPDX-License-Identifier: MIT OR Apache-2.0
//! What `pactl --format=json` reports, as the window shows it: the outputs
//! and inputs (without the monitors of outputs), the applications playing
//! and recording (without virtual-device plumbing and level meters), and
//! the sound cards with their profiles. Also how the window speaks of them,
//! after Aizome's Sound settings: a device by the name people say, with its
//! bus, port and use, and a glyph for each.
use serde_json::Value;

/// The loudest volume the window sets, in percent. Its sliders stop at
/// 100% unless a device or stream is already louder.
pub const MAX_VOLUME: f64 = 150.0;
/// PulseAudio's volume for 100%.
const NORMAL: f64 = 65536.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Output,
    Input,
    Playback,
    Recording,
}
impl Kind {
    pub fn noun(self) -> &'static str {
        match self {
            Self::Output => "sink",
            Self::Input => "source",
            Self::Playback => "sink-input",
            Self::Recording => "source-output",
        }
    }
    pub fn list(self) -> &'static str {
        match self {
            Self::Output => "sinks",
            Self::Input => "sources",
            Self::Playback => "sink-inputs",
            Self::Recording => "source-outputs",
        }
    }
    pub fn device(self) -> bool {
        matches!(self, Self::Output | Self::Input)
    }
    /// The devices a stream of this kind plays to or records from.
    pub fn devices(self) -> Kind {
        match self {
            Self::Output | Self::Playback => Self::Output,
            Self::Input | Self::Recording => Self::Input,
        }
    }
}

/// Which side a channel is on, for the balance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub name: String,
    pub label: String,
    /// pactl's type for it: "Speaker", "Headphones", "Headset", "HDMI", "Mic"…
    pub kind: String,
    /// Whether something is plugged in; None when the hardware can't tell.
    pub available: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub label: String,
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub id: u32,
    pub name: String,
    /// A device's description, or a stream's application.
    pub label: String,
    /// What a stream plays or records: "Call with Aiko", a video's title.
    pub media: String,
    /// "pci", "usb", "bluetooth"…
    pub bus: String,
    /// "internal", "headphone", "headset", "hdmi"…
    pub form: String,
    /// The device's or application's own icon name.
    pub icon: String,
    /// What a stream is for: "music", "video", "phone", "event"…
    pub role: String,
    /// An output playing (its state is RUNNING).
    pub running: bool,
    /// A stream paused.
    pub corked: bool,
    /// The loudest channel, in percent.
    pub volume: f64,
    /// From -1.0, all left, to 1.0, all right.
    pub balance: f64,
    /// Each channel's side, in the channel map's order.
    pub sides: Vec<Side>,
    pub muted: bool,
    pub writable: bool,
    /// The device a stream plays to or records from.
    pub target: Option<u32>,
    /// Its PipeWire node and serial (the serial is pactl's index), for
    /// routing streams through PipeWire's metadata (audio.rs).
    pub object: Option<u32>,
    pub serial: Option<u64>,
    /// A stream that may be moved to another device: not one that asked
    /// not to be (`node.dont-reconnect`), which pipewire-pulse refuses to move.
    pub movable: bool,
    pub ports: Vec<Port>,
    pub port: String,
}
impl Node {
    /// The port in use.
    pub fn active_port(&self) -> Option<&Port> {
        self.ports.iter().find(|port| port.name == self.port)
    }
    /// The ports one can switch to: those plugged in or that can't tell,
    /// and the one in use.
    pub fn usable_ports(&self) -> Vec<&Port> {
        self.ports
            .iter()
            .filter(|port| port.available != Some(false) || port.name == self.port)
            .collect()
    }
    /// It has channels on both sides, so a balance.
    pub fn stereo(&self) -> bool {
        self.sides.contains(&Side::Left) && self.sides.contains(&Side::Right)
    }
    fn hdmi(&self) -> bool {
        matches!(self.form.as_str(), "hdmi" | "tv")
            || self.name.contains("hdmi")
            || self.label.contains("HDMI")
            || self.label.contains("DisplayPort")
            || self.active_port().is_some_and(|port| port.kind == "HDMI")
    }
    fn port_is(&self, kind: &str) -> bool {
        self.active_port().is_some_and(|port| port.kind == kind)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub name: String,
    pub label: String,
    pub bus: String,
    pub form: String,
    /// The card's profiles, the most preferred first and Off last.
    pub profiles: Vec<Profile>,
    pub active: String,
}
impl Card {
    /// The profiles one can choose: those available, and the one in use.
    pub fn usable_profiles(&self) -> Vec<&Profile> {
        self.profiles
            .iter()
            .filter(|profile| profile.available || profile.name == self.active)
            .collect()
    }
    /// A graphics card's sound: its every output profile is HDMI (as
    /// PipeWire names DisplayPort's too).
    pub fn hdmi(&self) -> bool {
        let mut outputs = self
            .profiles
            .iter()
            .filter(|profile| profile.name != "off" && profile.name != "pro-audio")
            .peekable();
        outputs.peek().is_some() && outputs.all(|profile| profile.name.contains("hdmi"))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub nodes: Vec<Node>,
    pub cards: Vec<Card>,
    pub output: String,
    pub input: String,
}
impl Snapshot {
    pub fn node(&self, kind: Kind, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.kind == kind && n.id == id)
    }
    pub fn devices(&self, kind: Kind) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |n| n.kind == kind)
    }
    /// The applications: playing, then recording.
    pub fn streams(&self) -> impl Iterator<Item = &Node> {
        self.devices(Kind::Playback)
            .chain(self.devices(Kind::Recording))
    }
    /// The default output's or input's name.
    pub fn default_name(&self, kind: Kind) -> &str {
        match kind.devices() {
            Kind::Output => &self.output,
            _ => &self.input,
        }
    }
    /// The default output or input, when it is one listed.
    pub fn current(&self, kind: Kind) -> Option<&Node> {
        let name = self.default_name(kind);
        self.devices(kind).find(|n| n.name == name)
    }
    /// Whether a device is in use: an output playing, an input that an
    /// application records from. An input runs while the window's own level
    /// meter reads it, so its state alone would always say so.
    pub fn in_use(&self, device: &Node) -> bool {
        match device.kind {
            Kind::Output => device.running,
            Kind::Input => self
                .devices(Kind::Recording)
                .any(|s| s.target == Some(device.id) && !s.corked),
            _ => false,
        }
    }
}

pub fn string(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}

/// pactl says "available", "not available" or "availability unknown" for a
/// port; older versions "yes", "no" or "unknown".
fn availability(v: &Value) -> Option<bool> {
    match v.as_str()? {
        "available" | "yes" => Some(true),
        "not available" | "no" => Some(false),
        _ => None,
    }
}

fn ports(v: &Value) -> Vec<Port> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let name = string(v, "name");
            let label = string(v, "description");
            Port {
                label: if label.is_empty() {
                    name.clone()
                } else {
                    label
                },
                name,
                kind: string(v, "type"),
                available: availability(&v["availability"]),
            }
        })
        .collect()
}

/// A card's profiles, from pactl's object of them by name. pactl 17 says
/// whether each is available as a boolean.
fn profiles(v: &Value) -> Vec<Profile> {
    let mut profiles: Vec<(u64, Profile)> = v
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, v)| {
            let label = string(v, "description");
            let available = v["available"]
                .as_bool()
                .or_else(|| availability(&v["available"]))
                .unwrap_or(true);
            let priority = v["priority"].as_u64().unwrap_or(0);
            let profile = Profile {
                label: if label.is_empty() {
                    name.clone()
                } else {
                    label
                },
                name: name.clone(),
                available,
            };
            (priority, profile)
        })
        .collect();
    profiles.sort_by(|(a, x), (b, y)| {
        (x.name == "off")
            .cmp(&(y.name == "off"))
            .then(b.cmp(a))
            .then(x.label.cmp(&y.label))
    });
    profiles.into_iter().map(|(_, profile)| profile).collect()
}

/// Each channel's volume (in pactl's channel map order) and side.
fn channels(v: &Value) -> Vec<(f64, Side)> {
    let volumes = &v["volume"];
    let side = |channel: &str| {
        if channel.contains("left") {
            Side::Left
        } else if channel.contains("right") {
            Side::Right
        } else {
            Side::Other
        }
    };
    let map = v["channel_map"].as_str().unwrap_or_default();
    let mut channels: Vec<(f64, Side)> = map
        .split(',')
        .filter_map(|channel| {
            let value = volumes[channel]["value"].as_f64()?;
            Some((value / NORMAL * 100.0, side(channel)))
        })
        .collect();
    // Without a channel map, the volumes alone.
    if channels.is_empty() {
        channels = volumes
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(channel, v)| Some((v["value"].as_f64()? / NORMAL * 100.0, side(channel))))
            .collect();
    }
    channels
}

/// PulseAudio's balance of channel volumes (pa_cvolume_get_balance).
fn balance(channels: &[(f64, Side)]) -> f64 {
    let average = |wanted: Side| {
        let side: Vec<f64> = channels
            .iter()
            .filter(|(_, side)| *side == wanted)
            .map(|(volume, _)| *volume)
            .collect();
        (!side.is_empty()).then(|| side.iter().sum::<f64>() / side.len() as f64)
    };
    match (average(Side::Left), average(Side::Right)) {
        (Some(left), Some(right)) if left > right => -1.0 + right / left,
        (Some(left), Some(right)) if right > left => 1.0 - left / right,
        _ => 0.0,
    }
}

/// The channels' volumes in PulseAudio's units for a volume (the loudest
/// channel, in percent) and a balance, as pa_cvolume_set_balance spreads
/// them: the side away from the balance is turned down.
pub fn channel_volumes(volume: f64, balance: f64, sides: &[Side]) -> Vec<u32> {
    let full = volume.clamp(0.0, MAX_VOLUME) / 100.0 * NORMAL;
    let balance = balance.clamp(-1.0, 1.0);
    let left = if balance > 0.0 {
        full * (1.0 - balance)
    } else {
        full
    };
    let right = if balance < 0.0 {
        full * (1.0 + balance)
    } else {
        full
    };
    sides
        .iter()
        .map(|side| match side {
            Side::Left => left,
            Side::Right => right,
            Side::Other => full,
        })
        .map(|volume| volume.round() as u32)
        .collect()
}

pub fn nodes(kind: Kind, v: Value) -> Result<Vec<Node>, String> {
    let values = v
        .as_array()
        .ok_or("The audio service returned an invalid device list")?;
    let mut nodes = vec![];
    for v in values {
        let props = &v["properties"];
        // Virtual-device plumbing is not an application. Moving a remap
        // source's own stream could even route it back into itself.
        if !kind.device() && v["client"].is_null() && string(props, "node.virtual") == "true" {
            continue;
        }
        // Level meters (this window's, pavucontrol's) only observe audio;
        // they are not recording apps.
        if kind == Kind::Recording
            && (string(v, "resample_method") == "peaks"
                || string(props, "resample.method") == "peaks"
                || string(props, "media.name") == "Peak detect")
        {
            continue;
        }
        // A source monitoring speaker output is not a microphone.
        if kind == Kind::Input
            && (v["monitor_of_sink"]
                .as_u64()
                .is_some_and(|n| n != u32::MAX as u64)
                || v["monitor_of_sink"]
                    .as_str()
                    .is_some_and(|n| n != "4294967295" && n != "n/a")
                || string(props, "device.class") == "monitor"
                || string(v, "name").ends_with(".monitor"))
        {
            continue;
        }
        let id = v["index"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("Audio object has no valid index")?;
        let name = string(v, "name");
        let mut label = if kind.device() {
            string(v, "description")
        } else {
            string(props, "application.name")
        };
        if label.is_empty() {
            label = string(props, "application.process.binary");
        }
        if label.is_empty() {
            label = if name.is_empty() {
                format!("Audio stream {id}")
            } else {
                name.clone()
            };
        }
        let channels = channels(v);
        nodes.push(Node {
            kind,
            id,
            name,
            label,
            media: string(props, "media.name"),
            bus: string(props, "device.bus"),
            form: string(props, "device.form_factor"),
            icon: string(
                props,
                if kind.device() {
                    "device.icon_name"
                } else {
                    "application.icon_name"
                },
            ),
            role: string(props, "media.role"),
            running: string(v, "state") == "RUNNING",
            corked: v["corked"].as_bool().unwrap_or(false),
            volume: channels
                .iter()
                .map(|(volume, _)| *volume)
                .fold(0.0, f64::max),
            balance: balance(&channels),
            sides: channels.iter().map(|(_, side)| *side).collect(),
            muted: v["mute"].as_bool().unwrap_or(false),
            writable: v["volume_writable"].as_bool().unwrap_or(true),
            object: string(props, "object.id").parse().ok(),
            serial: string(props, "object.serial").parse().ok(),
            movable: string(props, "node.dont-reconnect") != "true",
            target: v[if kind == Kind::Playback {
                "sink"
            } else {
                "source"
            }]
            .as_u64()
            .map(|n| n as u32),
            ports: ports(&v["ports"]),
            port: string(v, "active_port"),
        });
    }
    nodes.sort_by(|a, b| a.label.cmp(&b.label).then(a.id.cmp(&b.id)));
    Ok(nodes)
}

pub fn cards(v: Value) -> Result<Vec<Card>, String> {
    Ok(v.as_array()
        .ok_or("The audio service returned an invalid card list")?
        .iter()
        .map(|v| {
            let name = string(v, "name");
            let props = &v["properties"];
            let label = string(props, "device.description");
            Card {
                label: if label.is_empty() {
                    name.clone()
                } else {
                    label
                },
                name,
                bus: string(props, "device.bus"),
                form: string(props, "device.form_factor"),
                profiles: profiles(&v["profiles"]),
                active: string(v, "active_profile"),
            }
        })
        .collect())
}

/// Words that say how many channels, at the end of a description.
const LAYOUTS: [&str; 8] = [
    "Analog Stereo",
    "Digital Stereo",
    "Surround 4.0",
    "Surround 5.1",
    "Surround 7.1",
    "Multichannel",
    "Stereo",
    "Mono",
];

/// A device's name as people say it, as Aizome's Sound settings has it: a
/// display's own name where HDMI gives one ("… (HDMI) [PG27UCDM]" is
/// "PG27UCDM"), the sound card's name taken off where its other devices
/// share it ("USB Audio Front Headphones" is "Front Headphones"), and the
/// channel layout dropped.
pub fn device_name(device: &Node, all: &[Node]) -> String {
    let description = device.label.trim();
    if let (Some(open), Some(close)) = (description.find('['), description.rfind(']'))
        && open < close
        && !description[open + 1..close].trim().is_empty()
    {
        return description[open + 1..close].trim().to_owned();
    }
    let mut name = description.to_owned();
    for layout in LAYOUTS {
        for ending in [format!(" ({layout})"), format!(" {layout}")] {
            if name.len() > ending.len() && name.ends_with(&ending) {
                name.truncate(name.len() - ending.len());
            }
        }
    }
    // The words every device of its card begins with (its card: the
    // devices sharing at least two first words with it).
    let words: Vec<&str> = name.split(' ').collect();
    let common = |other: &Node| {
        words
            .iter()
            .zip(other.label.split(' '))
            .take_while(|(a, b)| **a == *b)
            .count()
    };
    let shared = all
        .iter()
        .filter(|other| other.kind.device())
        .filter(|other| other.kind != device.kind || other.id != device.id)
        .map(common)
        .filter(|n| *n >= 2)
        .min()
        .unwrap_or(0);
    if shared >= 2 && shared < words.len() {
        name = words[shared..].join(" ");
    }
    for ending in [" Output", " Input"] {
        if name.len() > ending.len() && name.ends_with(ending) {
            name.truncate(name.len() - ending.len());
        }
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => description.to_owned(),
    }
}

/// "Bluetooth", "USB", "HDMI", "Built-in", or nothing for a virtual device.
pub fn bus_words(bus: &str, form: &str, hdmi: bool) -> &'static str {
    match bus {
        "bluetooth" => "Bluetooth",
        "usb" => "USB",
        _ if hdmi => "HDMI",
        "pci" if form.is_empty() || form == "internal" => "Built-in",
        _ => "",
    }
}

/// What the device row says under its name: its bus, the port in use where
/// it has a choice, and whether it plays or records, or nothing is plugged
/// in.
pub fn device_detail(device: &Node, s: &Snapshot, name: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let bus = bus_words(&device.bus, &device.form, device.hdmi());
    if !bus.is_empty() {
        parts.push(bus.to_owned());
    }
    if let Some(port) = device.active_port() {
        let words = port
            .label
            .trim_end_matches(" Output")
            .trim_end_matches(" Input");
        // Only where there is a choice, and not to say the name or the bus
        // again ("HDMI · HDMI / DisplayPort").
        if device.usable_ports().len() > 1
            && !name.contains(words)
            && (bus.is_empty() || !words.starts_with(bus))
        {
            parts.push(words.to_owned());
        }
        if port.available == Some(false) {
            parts.push("Unplugged".into());
        }
    }
    if s.in_use(device) {
        parts.push(
            if device.kind == Kind::Output {
                "Playing"
            } else {
                "In use"
            }
            .into(),
        );
    }
    parts.join(" · ")
}

/// What an application's row says under its name.
pub fn stream_detail(stream: &Node) -> String {
    let media = (!stream.media.is_empty() && stream.media != stream.label).then_some(&stream.media);
    match (stream.corked, media) {
        (true, None) => "Paused".into(),
        (true, Some(media)) => format!("Paused · {media}"),
        (false, None) => String::new(),
        (false, Some(media)) => media.clone(),
    }
}

/// The glyph for a device or stream, from what it says it is.
pub fn glyph(n: &Node) -> &'static str {
    let headset = n.form == "headset" || n.icon.contains("headset") || n.port_is("Headset");
    match n.kind {
        Kind::Input if headset => "audio-headset-symbolic",
        Kind::Input | Kind::Recording => "audio-input-microphone-symbolic",
        Kind::Output
            if n.form == "headphone" || n.icon.contains("headphone") || n.port_is("Headphones") =>
        {
            "audio-headphones-symbolic"
        }
        Kind::Output if headset => "audio-headset-symbolic",
        Kind::Output if n.bus == "bluetooth" => "audio-headphones-symbolic",
        Kind::Output if n.hdmi() => "video-display-symbolic",
        Kind::Output if n.icon.contains("speaker") || n.port_is("Speaker") => {
            "audio-speakers-symbolic"
        }
        Kind::Output if n.bus == "usb" => "audio-card-symbolic",
        Kind::Output => "audio-speakers-symbolic",
        Kind::Playback => match n.role.as_str() {
            "music" => "audio-x-generic-symbolic",
            "video" => "video-x-generic-symbolic",
            "event" | "notification" => "preferences-system-notifications-symbolic",
            "phone" => "call-start-symbolic",
            _ => "applications-multimedia-symbolic",
        },
    }
}

/// The glyph for a sound card.
pub fn card_glyph(card: &Card) -> &'static str {
    match (card.bus.as_str(), card.form.as_str()) {
        (_, "headset") => "audio-headset-symbolic",
        ("bluetooth", _) | (_, "headphone") => "audio-headphones-symbolic",
        _ => "audio-card-symbolic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn device(kind: Kind, id: u32, label: &str) -> Node {
        Node {
            kind,
            id,
            name: format!("dev{id}"),
            label: label.into(),
            media: String::new(),
            bus: String::new(),
            form: String::new(),
            icon: String::new(),
            role: String::new(),
            running: false,
            corked: false,
            volume: 100.0,
            balance: 0.0,
            sides: vec![],
            muted: false,
            writable: true,
            target: None,
            object: None,
            serial: None,
            movable: true,
            ports: vec![],
            port: String::new(),
        }
    }

    #[test]
    fn monitors_are_not_microphones_and_unknown_availability_is_usable() {
        let v = json!([
            {"index":1,"name":"speaker.monitor","monitor_of_sink":0},
            // pactl 17 reports a monitor's sink as null; its class says.
            {"index":3,"name":"hdmi-monitor","monitor_of_sink":null,"properties":{"device.class":"monitor"}},
            {"index":2,"name":"mic","description":"USB <Mic>","monitor_of_sink":4294967295u64,
             "channel_map":"front-left,front-right",
             "volume":{"front-left":{"value":32768},"front-right":{"value":65536}},
             "active_port":"mic",
             "ports":[{"name":"mic","description":"Microphone","type":"Mic","availability":"availability unknown"},
                      {"name":"line","description":"Line In","availability":"not available"}]}]);
        let n = nodes(Kind::Input, v).unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].volume, 100.0);
        assert_eq!(n[0].label, "USB <Mic>");
        assert_eq!(n[0].sides, [Side::Left, Side::Right]);
        assert!(
            (n[0].balance - 0.5).abs() < 1e-9,
            "the left is half the right"
        );
        assert_eq!(n[0].ports[0].available, None);
        assert_eq!(n[0].ports[1].available, Some(false));
        assert_eq!(n[0].usable_ports().len(), 1);
    }

    #[test]
    fn streams_keep_identity_routing_and_volume_capability() {
        let n = nodes(
            Kind::Recording,
            json!([{"index":7,"source":42,"corked":true,"volume_writable":false,
                    "properties":{"application.name":"Recorder","media.name":"Call","media.role":"phone",
                                  "object.id":"88","object.serial":"7","node.dont-reconnect":"true"}}]),
        )
        .unwrap();
        assert_eq!(n[0].id, 7);
        assert_eq!(n[0].target, Some(42));
        assert!(!n[0].writable);
        assert!(n[0].corked);
        assert_eq!(n[0].media, "Call");
        assert_eq!(
            (n[0].object, n[0].serial, n[0].movable),
            (Some(88), Some(7), false)
        );
        assert_eq!(stream_detail(&n[0]), "Paused · Call");
        assert!(nodes(Kind::Output, json!({})).is_err());
        assert!(nodes(Kind::Output, json!([{}])).is_err());
    }

    #[test]
    fn virtual_device_plumbing_and_level_meters_are_not_apps() {
        let streams = nodes(
            Kind::Recording,
            json!([
                {"index": 1, "client": null, "properties": {"node.virtual": "true"}},
                {"index": 2, "client": 4, "resample_method": "peaks"},
                {"index": 3, "client": 5, "properties": {"node.virtual": "true", "application.name": "Recorder"}},
                {"index": 4, "client": 6, "properties": {"resample.method": "peaks"}},
                {"index": 5, "client": 7, "properties": {"media.name": "Peak detect"}}
            ]),
        )
        .unwrap();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].label, "Recorder");
    }

    #[test]
    fn profiles_come_by_preference_with_off_last_and_unavailable_ones_kept_out() {
        // As pactl 17 prints them: available as a boolean.
        let c = cards(json!([{"name":"card","active_profile":"output:hdmi-stereo",
            "properties":{"device.description":"GB202 High Definition Audio Controller","device.bus":"pci"},
            "profiles":{
                "off":{"description":"Off","priority":0,"available":true},
                "output:hdmi-stereo":{"description":"Digital Stereo (HDMI) Output","priority":5900,"available":true},
                "output:hdmi-stereo-extra1":{"description":"Digital Stereo (HDMI 2) Output","priority":5700,"available":false},
                "pro-audio":{"description":"Pro Audio","priority":1,"available":true}}}]))
        .unwrap();
        let usable: Vec<&str> = c[0]
            .usable_profiles()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(usable, ["output:hdmi-stereo", "pro-audio", "off"]);
        assert_eq!(c[0].label, "GB202 High Definition Audio Controller");
        // Older pactl: "yes" and "no".
        let c = cards(json!([{"name":"bt","active_profile":"off","profiles":{
            "off":{"description":"Off","available":"yes"},
            "headset-head-unit":{"available":"unknown"},
            "a2dp-sink":{"available":"no"}}}]))
        .unwrap();
        assert_eq!(c[0].usable_profiles().len(), 2);
        assert_eq!(c[0].active, "off");
    }

    #[test]
    fn devices_go_by_the_names_people_say() {
        let all = [
            device(
                Kind::Output,
                64,
                "GB202 High Definition Audio Controller Digital Stereo (HDMI) [PG27UCDM]",
            ),
            device(Kind::Output, 65, "USB Audio S/PDIF Output"),
            device(Kind::Output, 66, "USB Audio Speakers"),
            device(Kind::Output, 67, "USB Audio Front Headphones"),
            device(Kind::Input, 68, "USB Audio Line Input"),
            device(Kind::Input, 69, "USB Audio Microphone"),
            device(Kind::Output, 70, "Built-in Audio Analog Stereo"),
            device(Kind::Output, 71, "WH-1000XM5"),
        ];
        let names: Vec<String> = all.iter().map(|d| device_name(d, &all)).collect();
        assert_eq!(
            names,
            [
                "PG27UCDM",
                "S/PDIF",
                "Speakers",
                "Front Headphones",
                "Line",
                "Microphone",
                "Built-in Audio",
                "WH-1000XM5"
            ]
        );
    }

    #[test]
    fn device_rows_say_bus_port_and_use() {
        let mut speakers = device(Kind::Output, 1, "Built-in Audio Analog Stereo");
        speakers.bus = "pci".into();
        speakers.running = true;
        speakers.ports = vec![
            Port {
                name: "speaker".into(),
                label: "Speakers".into(),
                kind: "Speaker".into(),
                available: None,
            },
            Port {
                name: "headphones".into(),
                label: "Headphones".into(),
                kind: "Headphones".into(),
                available: Some(true),
            },
        ];
        speakers.port = "speaker".into();
        let mut usb = device(Kind::Output, 2, "USB Audio Speakers");
        usb.bus = "usb".into();
        usb.ports = vec![Port {
            name: "out".into(),
            label: "Speakers".into(),
            kind: "Speaker".into(),
            available: Some(false),
        }];
        usb.port = "out".into();
        let mut hdmi = device(Kind::Output, 3, "HD Audio Digital Stereo (HDMI) [PG27UCDM]");
        hdmi.bus = "pci".into();
        hdmi.ports = vec![Port {
            name: "hdmi".into(),
            label: "HDMI / DisplayPort".into(),
            kind: "HDMI".into(),
            available: Some(true),
        }];
        hdmi.port = "hdmi".into();
        let mic = device(Kind::Input, 4, "Virtual Microphone");
        let mut call = device(Kind::Recording, 5, "Signal");
        call.target = Some(4);
        let s = Snapshot {
            nodes: vec![
                speakers.clone(),
                usb.clone(),
                hdmi.clone(),
                mic.clone(),
                call,
            ],
            ..Default::default()
        };
        assert_eq!(
            device_detail(&speakers, &s, "Built-in Audio"),
            "Built-in · Speakers · Playing"
        );
        assert_eq!(device_detail(&usb, &s, "Speakers"), "USB · Unplugged");
        assert_eq!(device_detail(&hdmi, &s, "PG27UCDM"), "HDMI");
        // A virtual device has no bus to name; an app records from it.
        assert_eq!(device_detail(&mic, &s, "Virtual Microphone"), "In use");
        assert_eq!(glyph(&speakers), "audio-speakers-symbolic");
        assert_eq!(glyph(&hdmi), "video-display-symbolic");
        speakers.port = "headphones".into();
        assert_eq!(glyph(&speakers), "audio-headphones-symbolic");
    }

    #[test]
    fn a_volume_and_balance_spread_over_the_channels() {
        let stereo = [Side::Left, Side::Right];
        assert_eq!(channel_volumes(50.0, 0.0, &stereo), [32768, 32768]);
        assert_eq!(channel_volumes(100.0, -0.5, &stereo), [65536, 32768]);
        assert_eq!(channel_volumes(100.0, 1.0, &stereo), [0, 65536]);
        // The center and LFE keep the volume.
        assert_eq!(
            channel_volumes(100.0, 0.5, &[Side::Left, Side::Right, Side::Other]),
            [32768, 65536, 65536]
        );
        assert_eq!(channel_volumes(400.0, 0.0, &[Side::Other]), [98304]);
        // pa_cvolume_get_balance, back again.
        let channels = [(100.0, Side::Left), (50.0, Side::Right)];
        assert!((balance(&channels) + 0.5).abs() < 1e-9);
    }
}
