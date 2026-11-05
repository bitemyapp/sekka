//! Common controllers, as their drivers describe them, through SDL's own
//! database (nixpkgs' `sdl_gamecontrollerdb`, named by
//! `AIZOME_GAMECONTROLLERDB`; without it the test says so and passes):
//! each has yes and no, and presses every one of a PIN's ten digits, by a
//! button of its own or, short of buttons, a chord.
//!
//!     AIZOME_GAMECONTROLLERDB=$(nix build --print-out-paths nixpkgs#sdl_gamecontrollerdb)/share/gamecontrollerdb.txt \
//!       cargo test -p aizome-sofa --test real_pads -- --nocapture

use std::collections::HashMap;

use aizome_gamepad::Button;
use aizome_gamepad::evdev::*;
use aizome_gamepad::mapping::{self, Database};
use aizome_sofa::pinpad::{Press, presses};

const BUS_USB: u16 = 0x03;

struct Pad {
    name: &'static str,
    id: InputId,
    keys: Vec<u16>,
    abs: Vec<(u16, i32, i32)>,
    driver: &'static str,
    /// Has a pair of triggers, so needs no chords.
    triggers: bool,
}

fn stick(code: u16) -> (u16, i32, i32) {
    (code, -32768, 32767)
}

fn hat(code: u16) -> (u16, i32, i32) {
    (code, -1, 1)
}

/// What the kernel's drivers give for each (`evtest`'s capabilities).
fn pads() -> Vec<Pad> {
    let face = [BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST];
    let sticks = [stick(ABS_X), stick(ABS_Y), stick(ABS_RX), stick(ABS_RY), hat(ABS_HAT0X), hat(ABS_HAT0Y)];
    let mut xpad_keys = face.to_vec();
    xpad_keys.extend([BTN_TL, BTN_TR, BTN_SELECT, BTN_START, BTN_MODE, BTN_THUMBL, BTN_THUMBR]);
    let mut xpad_abs = sticks.to_vec();
    xpad_abs.extend([(ABS_Z, 0, 1023), (ABS_RZ, 0, 1023)]);
    let mut sony_keys = face.to_vec();
    sony_keys.extend([BTN_TL, BTN_TR, BTN_TL2, BTN_TR2, BTN_SELECT, BTN_START, BTN_MODE, BTN_THUMBL, BTN_THUMBR]);
    let mut sony_abs = sticks.to_vec();
    sony_abs.extend([(ABS_Z, 0, 255), (ABS_RZ, 0, 255)]);
    let id = |bustype, vendor, product, version| InputId { bustype, vendor, product, version };
    vec![
        Pad {
            name: "Xbox Series X|S controller (USB, xpad)",
            id: id(BUS_USB, 0x045e, 0x0b12, 0x0510),
            keys: xpad_keys.clone(),
            abs: xpad_abs.clone(),
            driver: "xpad",
            triggers: true,
        },
        Pad {
            name: "Xbox 360 controller (xpad), as 8BitDo's pads in X-input mode",
            id: id(BUS_USB, 0x045e, 0x028e, 0x0114),
            keys: xpad_keys.clone(),
            abs: xpad_abs,
            driver: "xpad",
            triggers: true,
        },
        Pad {
            name: "DualSense (Bluetooth, hid-playstation)",
            id: id(BUS_BLUETOOTH, 0x054c, 0x0ce6, 0x8100),
            keys: sony_keys.clone(),
            abs: sony_abs.clone(),
            driver: "playstation",
            triggers: true,
        },
        Pad {
            name: "DualShock 4 (USB, hid-playstation)",
            id: id(BUS_USB, 0x054c, 0x09cc, 0x8111),
            keys: sony_keys,
            abs: sony_abs,
            driver: "playstation",
            triggers: true,
        },
        Pad {
            name: "Switch Pro Controller (Bluetooth, hid-nintendo)",
            id: id(BUS_BLUETOOTH, 0x057e, 0x2009, 0x8001),
            keys: [
                BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_WEST, BTN_TL, BTN_TR, BTN_TL2, BTN_TR2, BTN_SELECT, BTN_START,
                BTN_MODE, BTN_THUMBL, BTN_THUMBR, 0x135, /* BTN_Z: capture */
            ]
            .to_vec(),
            abs: [stick(ABS_X), stick(ABS_Y), stick(ABS_RX), stick(ABS_RY), hat(ABS_HAT0X), hat(ABS_HAT0Y)].to_vec(),
            driver: "nintendo",
            triggers: true,
        },
        Pad {
            name: "Retro SNES-style USB pad (DragonRise 0079:0011, hid-generic)",
            id: id(BUS_USB, 0x0079, 0x0011, 0x0110),
            keys: (BTN_JOYSTICK..BTN_JOYSTICK + 10).collect(),
            abs: vec![(ABS_X, 0, 255), (ABS_Y, 0, 255)],
            driver: "",
            triggers: false,
        },
        Pad {
            name: "Unknown digital pad (joystick codes, axis d-pad, no database entry)",
            id: id(BUS_USB, 0x1234, 0x5678, 0x0001),
            keys: (BTN_JOYSTICK..BTN_JOYSTICK + 8).collect(),
            abs: vec![(ABS_X, 0, 255), (ABS_Y, 0, 255)],
            driver: "",
            triggers: false,
        },
    ]
}

fn caps(p: &Pad) -> Caps {
    Caps {
        name: p.name.into(),
        id: p.id,
        keys: Bits::with(&p.keys),
        abs: Bits::with(&p.abs.iter().map(|a| a.0).collect::<Vec<_>>()),
        absinfo: p
            .abs
            .iter()
            .map(|&(c, minimum, maximum)| (c, AbsInfo { minimum, maximum, ..AbsInfo::default() }))
            .collect(),
        driver: p.driver.into(),
        ..Caps::default()
    }
}

#[test]
fn common_controllers_say_yes_and_no_and_press_every_digit() {
    let Some(path) = std::env::var_os("AIZOME_GAMECONTROLLERDB") else {
        eprintln!("AIZOME_GAMECONTROLLERDB isn't set: SDL's database isn't checked");
        return;
    };
    let db = Database::load(std::path::Path::new(&path));
    assert!(db.len() > 500, "SDL's database read: {} entries", db.len());
    for p in pads() {
        let c = caps(&p);
        let m = mapping::resolve(&c, &HashMap::new(), &db);
        let have = m.buttons();
        let p10 = presses(have);
        let chords = p10.iter().filter(|x| matches!(x, Some(Press::Chord(..)))).count();
        eprintln!("{}: {:?}, {} chords: {p10:?}", p.name, m.source, chords);
        assert!(have & Button::South.bit() != 0 && have & Button::East.bit() != 0, "{}: yes and no", p.name);
        assert!(p10.iter().all(Option::is_some), "{}: every digit, {p10:?}", p.name);
        if p.triggers {
            assert_eq!(chords, 0, "{}: a pad with triggers needs no chords", p.name);
        }
    }
}
