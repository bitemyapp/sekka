//! Which of a device's codes is which button, by position: the bottom face
//! button is `South` whatever it's labelled.
//!
//! Drivers disagree. `hid-playstation` and `hid-nintendo` follow the
//! kernel's gamepad layout (`BTN_NORTH` is the top button), while `xpad`
//! and Microsoft's Bluetooth controllers report X, the left button, as
//! `BTN_X`, which is the same code as `BTN_NORTH`. Other devices (`hid-generic`)
//! report whatever their descriptor says. So, in order:
//!
//! 1. a layout a grown-up set up in Chanoma for this model;
//! 2. SDL's community database (`gamecontrollerdb.txt`), which names
//!    buttons by SDL's numbering of the device's codes;
//! 3. the kernel's layout, with Microsoft's and `xpad`'s left and top
//!    buttons swapped back.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::evdev::*;
use super::{Axis, Button};

/// Where a mapping came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    SetUp,
    Database,
    #[default]
    Kernel,
}

/// A device input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Input {
    Key(u16),
    /// An axis; `half` takes only its positive (1) or negative (-1) side;
    /// `invert` flips it.
    Abs {
        code: u16,
        half: i8,
        invert: bool,
    },
    /// A hat axis pair's direction: up 1, right 2, down 4, left 8.
    Hat {
        hat: u8,
        mask: u8,
    },
}

/// What an input becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Output {
    Button(Button),
    Axis(Axis),
    /// One side of a stick (a stick made of buttons).
    HalfAxis(Axis, i8),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub binds: Vec<(Input, Output)>,
    pub source: Source,
}

impl Mapping {
    pub fn output_of_key(&self, code: u16) -> impl Iterator<Item = Output> + '_ {
        self.binds.iter().filter(move |(i, _)| *i == Input::Key(code)).map(|(_, o)| *o)
    }

    pub fn has(&self, out: Output) -> bool {
        self.binds.iter().any(|(_, o)| *o == out)
    }
}

/// SDL's identifier for a device: bus, vendor, product and version,
/// little-endian, with the name's checksum left zero as the database's
/// Linux entries mostly have it.
pub fn guid(id: &InputId) -> String {
    let mut b = [0u8; 16];
    b[0..2].copy_from_slice(&id.bustype.to_le_bytes());
    b[4..6].copy_from_slice(&id.vendor.to_le_bytes());
    b[8..10].copy_from_slice(&id.product.to_le_bytes());
    b[12..14].copy_from_slice(&id.version.to_le_bytes());
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The guid without its version, for a looser match.
fn without_version(guid: &str) -> String {
    let mut g = guid.to_owned();
    if g.len() == 32 {
        g.replace_range(24..28, "0000");
    }
    g
}

/// Without SDL's name checksum (bytes 2–3), which newer entries carry.
fn without_crc(guid: &str) -> String {
    let mut g = guid.to_ascii_lowercase();
    if g.len() == 32 {
        g.replace_range(4..8, "0000");
    }
    g
}

/// SDL's database, Linux's entries, by guid.
#[derive(Clone, Debug, Default)]
pub struct Database {
    entries: HashMap<String, String>,
}

impl Database {
    pub fn parse(text: &str) -> Database {
        let mut entries = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let platform = line.split(',').find_map(|f| f.strip_prefix("platform:"));
            if platform.is_some_and(|p| p != "Linux") {
                continue;
            }
            let Some((guid, rest)) = line.split_once(',') else { continue };
            entries.insert(without_crc(guid), rest.to_owned());
        }
        Database { entries }
    }

    pub fn load(path: &std::path::Path) -> Database {
        std::fs::read_to_string(path).map(|t| Database::parse(&t)).unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry for `id`: its version too if one has it, else any version.
    pub fn find(&self, id: &InputId) -> Option<&str> {
        let g = guid(id);
        if let Some(e) = self.entries.get(&g) {
            return Some(e);
        }
        let loose = without_version(&g);
        self.entries.iter().find(|(k, _)| without_version(k) == loose).map(|(_, v)| v.as_str())
    }
}

/// SDL's numbering of a device's buttons, axes and hats.
struct Numbering {
    buttons: Vec<u16>,
    axes: Vec<u16>,
    hats: Vec<u8>,
}

fn numbering(caps: &Caps) -> Numbering {
    // Joystick and gamepad codes first, then the rest (SDL's Linux order).
    let buttons = (BTN_JOYSTICK..=KEY_MAX).chain(0..BTN_JOYSTICK).filter(|c| caps.keys.has(*c)).collect();
    let axes = (0..=ABS_MAX).filter(|c| !(ABS_HAT0X..=ABS_HAT3Y).contains(c) && caps.abs.has(*c)).collect();
    let hats = (0..4u8)
        .filter(|h| caps.abs.has(ABS_HAT0X + u16::from(*h) * 2) || caps.abs.has(ABS_HAT0Y + u16::from(*h) * 2))
        .collect();
    Numbering { buttons, axes, hats }
}

fn target(name: &str) -> Option<Output> {
    let (sign, name) = match name.as_bytes().first() {
        Some(b'+') => (1, &name[1..]),
        Some(b'-') => (-1, &name[1..]),
        _ => (0, name),
    };
    let axis = |a| if sign == 0 { Output::Axis(a) } else { Output::HalfAxis(a, sign) };
    Some(match name {
        "a" => Output::Button(Button::South),
        "b" => Output::Button(Button::East),
        "x" => Output::Button(Button::West),
        "y" => Output::Button(Button::North),
        "back" => Output::Button(Button::Select),
        "start" => Output::Button(Button::Start),
        "guide" => Output::Button(Button::Guide),
        "leftshoulder" => Output::Button(Button::L1),
        "rightshoulder" => Output::Button(Button::R1),
        "leftstick" => Output::Button(Button::L3),
        "rightstick" => Output::Button(Button::R3),
        "dpup" => Output::Button(Button::Up),
        "dpdown" => Output::Button(Button::Down),
        "dpleft" => Output::Button(Button::Left),
        "dpright" => Output::Button(Button::Right),
        "lefttrigger" => axis(Axis::L2),
        "righttrigger" => axis(Axis::R2),
        "leftx" => axis(Axis::LeftX),
        "lefty" => axis(Axis::LeftY),
        "rightx" => axis(Axis::RightX),
        "righty" => axis(Axis::RightY),
        _ => return None,
    })
}

fn input(src: &str, n: &Numbering) -> Option<Input> {
    let (half, src) = match src.as_bytes().first() {
        Some(b'+') => (1, &src[1..]),
        Some(b'-') => (-1, &src[1..]),
        _ => (0, src),
    };
    let (src, invert) = match src.strip_suffix('~') {
        Some(s) => (s, true),
        None => (src, false),
    };
    let rest = src.get(1..)?;
    match src.as_bytes().first()? {
        b'b' => n.buttons.get(rest.parse::<usize>().ok()?).map(|c| Input::Key(*c)),
        b'a' => n.axes.get(rest.parse::<usize>().ok()?).map(|c| Input::Abs { code: *c, half, invert }),
        b'h' => {
            let (hat, mask) = rest.split_once('.')?;
            let hat = *n.hats.get(hat.parse::<usize>().ok()?)?;
            Some(Input::Hat { hat, mask: mask.parse().ok()? })
        }
        _ => None,
    }
}

/// A database entry (`name,a:b0,…`) for the device with `caps`.
pub fn from_database(entry: &str, caps: &Caps) -> Mapping {
    let n = numbering(caps);
    let mut binds = Vec::new();
    for field in entry.split(',').skip(1) {
        let Some((t, s)) = field.split_once(':') else { continue };
        if let (Some(out), Some(inp)) = (target(t), input(s, &n)) {
            // A trigger the database gives as a button is a full press.
            let out = match (out, inp) {
                (Output::Axis(a @ (Axis::L2 | Axis::R2)), Input::Key(_)) => Output::HalfAxis(a, 1),
                _ => out,
            };
            binds.push((inp, out));
        }
    }
    Mapping { binds, source: Source::Database }
}

/// The kernel's gamepad layout, with Microsoft's left and top buttons put
/// back where they are.
pub fn from_kernel(caps: &Caps) -> Mapping {
    let xbox = caps.driver == "xpad" || caps.id.vendor == 0x045e;
    let (north, west) = if xbox { (BTN_WEST, BTN_NORTH) } else { (BTN_NORTH, BTN_WEST) };
    let mut binds = Vec::new();
    let mut key = |code: u16, b: Button| {
        if caps.keys.has(code) {
            binds.push((Input::Key(code), Output::Button(b)));
        }
    };
    key(BTN_SOUTH, Button::South);
    key(BTN_EAST, Button::East);
    key(north, Button::North);
    key(west, Button::West);
    key(BTN_TL, Button::L1);
    key(BTN_TR, Button::R1);
    key(BTN_SELECT, Button::Select);
    key(BTN_START, Button::Start);
    key(BTN_MODE, Button::Guide);
    key(BTN_THUMBL, Button::L3);
    key(BTN_THUMBR, Button::R3);
    key(BTN_DPAD_UP, Button::Up);
    key(BTN_DPAD_DOWN, Button::Down);
    key(BTN_DPAD_LEFT, Button::Left);
    key(BTN_DPAD_RIGHT, Button::Right);
    let abs = |code: u16| Input::Abs { code, half: 0, invert: false };
    let has = |code: u16| caps.abs.has(code);
    if has(ABS_X) {
        binds.push((abs(ABS_X), Output::Axis(Axis::LeftX)));
    }
    if has(ABS_Y) {
        binds.push((abs(ABS_Y), Output::Axis(Axis::LeftY)));
    }
    // The right stick is RX/RY with triggers on Z/RZ; or, without RX/RY
    // (Microsoft's Bluetooth controllers), Z/RZ with triggers on
    // brake and gas.
    let (rx, ry, lt, rt) =
        if has(ABS_RX) && has(ABS_RY) { (ABS_RX, ABS_RY, ABS_Z, ABS_RZ) } else { (ABS_Z, ABS_RZ, ABS_BRAKE, ABS_GAS) };
    if has(rx) && has(ry) {
        binds.push((abs(rx), Output::Axis(Axis::RightX)));
        binds.push((abs(ry), Output::Axis(Axis::RightY)));
    }
    if has(lt) {
        binds.push((abs(lt), Output::Axis(Axis::L2)));
    } else if caps.keys.has(BTN_TL2) {
        binds.push((Input::Key(BTN_TL2), Output::HalfAxis(Axis::L2, 1)));
    }
    if has(rt) {
        binds.push((abs(rt), Output::Axis(Axis::R2)));
    } else if caps.keys.has(BTN_TR2) {
        binds.push((Input::Key(BTN_TR2), Output::HalfAxis(Axis::R2, 1)));
    }
    if has(ABS_HAT0X) || has(ABS_HAT0Y) {
        for (mask, b) in [(1, Button::Up), (2, Button::Right), (4, Button::Down), (8, Button::Left)] {
            binds.push((Input::Hat { hat: 0, mask }, Output::Button(b)));
        }
    }
    Mapping { binds, source: Source::Kernel }
}

/// The mapping for a device: one set up here, else the database's, else
/// the kernel's.
pub fn resolve(caps: &Caps, set_up: &HashMap<String, Mapping>, db: &Database) -> Mapping {
    if let Some(m) = set_up.get(&guid(&caps.id)) {
        return Mapping { source: Source::SetUp, ..m.clone() };
    }
    if let Some(entry) = db.find(&caps.id) {
        let m = from_database(entry, caps);
        // An entry naming codes the device doesn't have is for another
        // revision: the kernel's layout is the better guess.
        if m.has(Output::Button(Button::South)) && m.has(Output::Button(Button::East)) {
            return m;
        }
    }
    from_kernel(caps)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Microsoft's Xbox Wireless Controller over Bluetooth, as hid-generic
    /// and hid-microsoft give it: fifteen buttons from BTN_SOUTH, sticks on
    /// X/Y and Z/RZ, triggers on brake and gas.
    fn xbox_bluetooth() -> Caps {
        let keys: Vec<u16> = (BTN_SOUTH..=BTN_THUMBR).collect();
        Caps {
            name: "Xbox Wireless Controller".into(),
            id: InputId { bustype: BUS_BLUETOOTH, vendor: 0x045e, product: 0x0b13, version: 0x0509 },
            keys: Bits::with(&keys),
            abs: Bits::with(&[ABS_X, ABS_Y, ABS_Z, ABS_RZ, ABS_GAS, ABS_BRAKE, ABS_HAT0X, ABS_HAT0Y]),
            driver: "microsoft".into(),
            ..Caps::default()
        }
    }

    fn dualsense() -> Caps {
        Caps {
            name: "DualSense Wireless Controller".into(),
            id: InputId { bustype: BUS_BLUETOOTH, vendor: 0x054c, product: 0x0ce6, version: 0x8100 },
            keys: Bits::with(&[
                BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_TL, BTN_TR, BTN_TL2, BTN_TR2, BTN_SELECT, BTN_START,
                BTN_MODE, BTN_THUMBL, BTN_THUMBR,
            ]),
            abs: Bits::with(&[ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ, ABS_HAT0X, ABS_HAT0Y]),
            driver: "playstation".into(),
            ..Caps::default()
        }
    }

    fn bind(m: &Mapping, out: Output) -> Option<Input> {
        m.binds.iter().find(|(_, o)| *o == out).map(|(i, _)| *i)
    }

    #[test]
    fn guids_are_sdls() {
        let id = InputId { bustype: 3, vendor: 0x045e, product: 0x02e0, version: 0x0903 };
        assert_eq!(guid(&id), "030000005e040000e002000003090000");
    }

    #[test]
    fn the_database_numbers_buttons_and_axes_as_sdl_does() {
        let db = Database::parse(
            "# comment\n\
             050000005e040000130b000009050000,Xbox Series X Controller,a:b0,b:b1,back:b10,dpdown:h0.4,dpleft:h0.8,\
             dpright:h0.2,dpup:h0.1,guide:b12,leftshoulder:b6,leftstick:b13,lefttrigger:a5,leftx:a0,lefty:a1,\
             rightshoulder:b7,rightstick:b14,righttrigger:a4,rightx:a2,righty:a3,start:b11,x:b3,y:b4,platform:Linux,\n\
             030000005e040000130b000009050000,Not for us,a:b0,platform:Windows,\n",
        );
        assert_eq!(db.len(), 1);
        let caps = xbox_bluetooth();
        let m = resolve(&caps, &HashMap::new(), &db);
        assert_eq!(m.source, Source::Database);
        assert_eq!(bind(&m, Output::Button(Button::West)), Some(Input::Key(BTN_NORTH)), "X is BTN_X");
        assert_eq!(bind(&m, Output::Button(Button::North)), Some(Input::Key(BTN_WEST)), "Y is BTN_Y");
        assert_eq!(bind(&m, Output::Button(Button::Select)), Some(Input::Key(BTN_SELECT)));
        assert_eq!(bind(&m, Output::Button(Button::Guide)), Some(Input::Key(BTN_MODE)));
        let axis = |code| Some(Input::Abs { code, half: 0, invert: false });
        assert_eq!(bind(&m, Output::Axis(Axis::RightX)), axis(ABS_Z));
        assert_eq!(bind(&m, Output::Axis(Axis::L2)), axis(ABS_BRAKE));
        assert_eq!(bind(&m, Output::Axis(Axis::R2)), axis(ABS_GAS));
        assert_eq!(bind(&m, Output::Button(Button::Left)), Some(Input::Hat { hat: 0, mask: 8 }));
    }

    #[test]
    fn without_an_entry_the_kernels_layout_serves_both_families() {
        let db = Database::default();
        let xbox = resolve(&xbox_bluetooth(), &HashMap::new(), &db);
        assert_eq!(xbox.source, Source::Kernel);
        assert_eq!(bind(&xbox, Output::Button(Button::West)), Some(Input::Key(BTN_NORTH)));
        assert_eq!(bind(&xbox, Output::Axis(Axis::RightY)), Some(Input::Abs { code: ABS_RZ, half: 0, invert: false }));
        assert_eq!(bind(&xbox, Output::Axis(Axis::R2)), Some(Input::Abs { code: ABS_GAS, half: 0, invert: false }));
        let ps = resolve(&dualsense(), &HashMap::new(), &db);
        assert_eq!(bind(&ps, Output::Button(Button::North)), Some(Input::Key(BTN_NORTH)), "the triangle");
        assert_eq!(bind(&ps, Output::Button(Button::West)), Some(Input::Key(BTN_WEST)), "the square");
        assert_eq!(bind(&ps, Output::Axis(Axis::RightX)), Some(Input::Abs { code: ABS_RX, half: 0, invert: false }));
        assert_eq!(bind(&ps, Output::Axis(Axis::L2)), Some(Input::Abs { code: ABS_Z, half: 0, invert: false }));
    }

    #[test]
    fn a_layout_set_up_here_comes_first() {
        let caps = dualsense();
        let mine =
            Mapping { binds: vec![(Input::Key(BTN_EAST), Output::Button(Button::South))], source: Source::SetUp };
        let set_up = HashMap::from([(guid(&caps.id), mine)]);
        let m = resolve(&caps, &set_up, &Database::default());
        assert_eq!(m.source, Source::SetUp);
        assert_eq!(bind(&m, Output::Button(Button::South)), Some(Input::Key(BTN_EAST)));
    }

    #[test]
    fn half_axes_inversions_and_button_triggers_parse() {
        let caps = Caps {
            keys: Bits::with(&[BTN_SOUTH, BTN_EAST, BTN_TL2]),
            abs: Bits::with(&[ABS_X, ABS_Y]),
            ..Caps::default()
        };
        let m = from_database("Pad,a:b0,b:b1,lefttrigger:b2,dpup:-a1,lefty:a1~,+rightx:b1", &caps);
        assert_eq!(bind(&m, Output::HalfAxis(Axis::L2, 1)), Some(Input::Key(BTN_TL2)));
        assert_eq!(bind(&m, Output::Button(Button::Up)), Some(Input::Abs { code: ABS_Y, half: -1, invert: false }));
        assert_eq!(bind(&m, Output::Axis(Axis::LeftY)), Some(Input::Abs { code: ABS_Y, half: 0, invert: true }));
        assert_eq!(bind(&m, Output::HalfAxis(Axis::RightX, 1)), Some(Input::Key(BTN_EAST)));
    }
}
