//! Keyboards one by one: who each is (the name and IDs it gives), so that
//! each can have keys of its own (`[[input.keyboards]]`: modifier swaps,
//! Caps Lock, Compose). The seat has one keymap, as Wayland has, so it
//! becomes the typing keyboard's before its key goes anywhere; Caps Lock,
//! Num Lock and the layout in use carry over.
//!
//! Only keyboards that type count (a space bar and the letters): not a
//! power button, nor the media keys a keyboard's second device sends, nor
//! another device with a few keys, which leave the keymap as it is.

use std::collections::{BTreeMap, HashSet};

use aizome_ipc::{Event, KeyboardBus, KeyboardInfo};
use smithay::input::SeatHandler;
use smithay::input::keyboard::{KeyboardTarget, XkbConfig};
use smithay::utils::SERIAL_COUNTER;
use tracing::{info, warn};

use crate::state::State;

/// A keyboard that types, as its device gives itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyboard {
    pub name: String,
    /// Vendor and product, `05ac:029c`.
    pub id: String,
    pub bus: KeyboardBus,
}

impl Keyboard {
    pub fn new(name: &str, vendor: u32, product: u32, bus: KeyboardBus) -> Keyboard {
        Keyboard { name: name.to_owned(), id: format!("{vendor:04x}:{product:04x}"), bus }
    }

    fn identity(&self) -> (String, String) {
        (self.name.clone(), self.id.to_ascii_lowercase())
    }

    fn info(&self, last: bool) -> KeyboardInfo {
        KeyboardInfo { name: self.name.clone(), id: self.id.clone(), bus: self.bus, last }
    }
}

/// Whether a device with these keys (evdev codes) types: a space bar and
/// the letters A to Z.
pub fn types(has_key: impl Fn(u32) -> bool) -> bool {
    const KEY_SPACE: u32 = 57;
    // Q to P, A to L, Z to M.
    const LETTERS: [std::ops::RangeInclusive<u32>; 3] = [16..=25, 30..=38, 44..=50];
    has_key(KEY_SPACE) && LETTERS.into_iter().flatten().all(has_key)
}

/// How an input device is connected, from its `id/bustype` in sysfs. QEMU's
/// USB keyboard (vendor 0627) is a VM's, not one plugged in.
pub fn bus(bustype: Option<u16>, vendor: u32) -> KeyboardBus {
    match bustype {
        _ if vendor == 0x0627 => KeyboardBus::Other,
        Some(0x03) => KeyboardBus::Usb,
        Some(0x05) => KeyboardBus::Bluetooth,
        // i8042, ADB, I²C, host, SPI.
        Some(0x11 | 0x17 | 0x18 | 0x19 | 0x1c) => KeyboardBus::BuiltIn,
        _ => KeyboardBus::Other,
    }
}

/// What a keymap is compiled from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeymapSpec {
    pub layout: String,
    pub variant: String,
    pub model: String,
    pub options: String,
}

impl KeymapSpec {
    pub fn xkb(&self) -> XkbConfig<'_> {
        XkbConfig {
            rules: "",
            model: &self.model,
            layout: &self.layout,
            variant: &self.variant,
            options: if self.options.is_empty() { None } else { Some(self.options.clone()) },
        }
    }
}

#[derive(Debug, Default)]
pub struct Keyboards {
    /// By device: libinput's sysname (`event5`), or `synthetic:NAME`.
    devices: BTreeMap<String, Keyboard>,
    /// Who has appeared since the compositor started.
    met: HashSet<(String, String)>,
    /// The device that typed last, and the one the key on its way came
    /// from (none for a key from IPC or another device).
    last: Option<String>,
    sending: Option<String>,
    /// What the seat's keymap was compiled from.
    pub active: Option<KeymapSpec>,
    /// The tag of a [`Request::CatchKey`](aizome_ipc::Request::CatchKey)
    /// waiting for a key, and the IPC connection that asked: it waits only
    /// while that stays open.
    pub catch: Option<(String, u64)>,
}

impl Keyboards {
    /// None yet, with the keymap compiled from `active`.
    pub fn new(active: KeymapSpec) -> Keyboards {
        Keyboards { active: Some(active), ..Default::default() }
    }

    /// The keyboard the key on its way came from.
    pub fn sending(&self) -> Option<KeyboardInfo> {
        self.sending.as_ref().and_then(|d| self.devices.get(d)).map(|k| k.info(true))
    }

    /// Each keyboard once (one with several devices is one), in name order.
    pub fn info(&self) -> Vec<KeyboardInfo> {
        let last = self.last.as_ref().and_then(|d| self.devices.get(d)).map(Keyboard::identity);
        let mut seen = HashSet::new();
        let mut v: Vec<KeyboardInfo> = self
            .devices
            .values()
            .filter(|k| seen.insert(k.identity()))
            .map(|k| k.info(last.as_ref() == Some(&k.identity())))
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        v
    }
}

impl State {
    /// The keymap for a keyboard (or, with none, any other): the settings'
    /// layouts, with its own keys' options.
    pub fn keymap_for(&self, k: Option<&Keyboard>) -> KeymapSpec {
        let input = &self.az.settings.input;
        let kb = &input.keyboard;
        KeymapSpec {
            layout: kb.layouts.join(","),
            variant: kb.variant.clone(),
            model: kb.model.clone(),
            options: match k {
                Some(k) => input.options_for(&k.name, &k.id),
                None => kb.options.clone(),
            },
        }
    }

    /// A keyboard that types appeared: subscribers hear of it unless it
    /// came before (plugged in again, or back from another VT).
    pub fn keyboard_added(&mut self, device: String, k: Keyboard) {
        let new = self.az.keyboards.met.insert(k.identity());
        info!(device, name = k.name, id = k.id, ?k.bus, new, "keyboard");
        let event = new.then(|| Event::KeyboardAdded { keyboard: k.info(false) });
        self.keyboard_removed(&device);
        self.az.keyboards.devices.insert(device, k);
        if let Some(e) = event {
            self.az.ipc.emit(&e);
        }
    }

    /// A device went. libinput names the next one alike (`event5`), which
    /// may be another keyboard: its first key mustn't pass for this one's.
    pub fn keyboard_removed(&mut self, device: &str) {
        let ks = &mut self.az.keyboards;
        ks.devices.remove(device);
        for d in [&mut ks.last, &mut ks.sending] {
            if d.as_deref() == Some(device) {
                *d = None;
            }
        }
    }

    /// A key from `device` (none: from IPC) is on its way: the keymap
    /// becomes its keyboard's, if it isn't. Other devices leave it be.
    pub fn key_from(&mut self, device: Option<&str>) {
        let ks = &mut self.az.keyboards;
        ks.sending = device.filter(|d| ks.devices.contains_key(*d)).map(str::to_owned);
        if ks.sending.is_none() || ks.sending == ks.last {
            return;
        }
        ks.last = ks.sending.clone();
        self.apply_keymap();
    }

    /// Compiles the keymap the keyboard that typed last needs (after a
    /// change of keyboard or of the settings), if it isn't the one in force.
    pub fn apply_keymap(&mut self) {
        let ks = &self.az.keyboards;
        let spec = self.keymap_for(ks.last.as_ref().and_then(|d| ks.devices.get(d)));
        if self.az.keyboards.active.as_ref() == Some(&spec) {
            return;
        }
        let Some(kb) = self.az.seat.get_keyboard() else { return };
        let before = kb.modifier_state();
        let layout = kb.with_xkb_state(self, |c| c.xkb().lock().unwrap().active_layout());
        match kb.set_xkb_config(self, spec.xkb()) {
            Ok(()) => {
                // A new keymap starts unlocked, in the first layout, and the
                // focus has heard so; it hears the locks again before the
                // key on its way (whose modifiers would come after it).
                // The locks first: smithay's set_modifier_state puts the
                // seat back in the first layout, which set_layout then
                // undoes (keeping the locks).
                let mut now = kb.modifier_state();
                let locks = (now.caps_lock, now.num_lock) != (before.caps_lock, before.num_lock);
                if locks {
                    now.caps_lock = before.caps_lock;
                    now.num_lock = before.num_lock;
                    kb.set_modifier_state(now);
                }
                kb.with_xkb_state(self, |mut c| c.set_layout(layout));
                if locks {
                    let seat = self.az.seat.clone();
                    self.led_state_changed(&seat, kb.led_state());
                    if let Some(focus) = kb.current_focus() {
                        focus.modifiers(&seat, self, kb.modifier_state(), SERIAL_COUNTER.next_serial());
                    }
                }
            }
            Err(e) => warn!("keyboard layout {}, options {}: {e:?}", spec.layout, spec.options),
        }
        // Failed or not, it isn't compiled again until something changes.
        self.az.keyboards.active = Some(spec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buses_say_what_was_plugged_in() {
        assert_eq!(bus(Some(0x03), 0x05ac), KeyboardBus::Usb);
        assert_eq!(bus(Some(0x05), 0x004c), KeyboardBus::Bluetooth);
        assert_eq!(bus(Some(0x11), 0x0001), KeyboardBus::BuiltIn);
        assert_eq!(bus(Some(0x06), 0x0000), KeyboardBus::Other, "virtio");
        assert_eq!(bus(Some(0x03), 0x0627), KeyboardBus::Other, "QEMU's USB keyboard");
        assert_eq!(bus(None, 0x046d), KeyboardBus::Other);
        assert_eq!(Keyboard::new("K", 0x5ac, 0x29c, KeyboardBus::Usb).id, "05ac:029c");
    }

    #[test]
    fn only_a_keyboard_with_a_space_bar_and_letters_types() {
        assert!(types(|k| k < 128), "a keyboard");
        assert!(!types(|k| k == 57 || k == 28), "a space bar and Return alone");
        assert!(!types(|k| k == 116), "a power button");
        assert!(!types(|k| k != 57), "no space bar");
        assert!(!types(|k| k != 50), "no M");
    }

    #[test]
    fn a_keyboard_with_several_devices_is_listed_once() {
        let mut ks = Keyboards::default();
        let k = Keyboard::new("Logitech K120", 0x046d, 0xc31c, KeyboardBus::Usb);
        ks.devices.insert("event3".into(), k.clone());
        ks.devices.insert("event4".into(), k);
        ks.devices.insert("event9".into(), Keyboard::new("AT Translated Set 2 keyboard", 1, 1, KeyboardBus::BuiltIn));
        ks.last = Some("event4".into());
        let info = ks.info();
        assert_eq!(info.len(), 2);
        assert_eq!(info[0].name, "AT Translated Set 2 keyboard");
        assert!(!info[0].last);
        assert!(info[1].last);
    }
}
