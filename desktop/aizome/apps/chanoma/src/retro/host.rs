//! What a core calls back into: its questions about the frontend
//! (`environment`), its frames, its sound and its questions about the
//! controllers.
//!
//! libretro's callbacks carry no context, so the running core's host is
//! kept in one place for the process: there is one game at a time.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_uint, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use super::ffi::*;
use super::video::Screen;
use crate::pad::{Axis, Button, Shared};

static HOST: AtomicPtr<Host> = AtomicPtr::new(std::ptr::null_mut());

/// A core's setting: its value and the values it takes.
#[derive(Clone, Debug)]
pub struct Opt {
    pub value: CString,
    pub values: Vec<String>,
    pub label: String,
}

/// A core's settings.
#[derive(Debug, Default)]
pub struct Options {
    pub map: BTreeMap<String, Opt>,
    /// Changed since the core last asked.
    pub dirty: bool,
    /// Values to use instead of a core's defaults (Chanoma's choices).
    pub prefer: BTreeMap<String, String>,
}

fn string(p: *const c_char) -> Option<String> {
    // SAFETY: cores give NUL-terminated strings or null.
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

impl Options {
    fn define(&mut self, key: String, label: String, values: Vec<String>, default: Option<String>) {
        let mut value = default.filter(|d| values.contains(d)).or_else(|| values.first().cloned()).unwrap_or_default();
        if let Some(p) = self.prefer.get(&key)
            && values.contains(p)
        {
            value = p.clone();
        }
        // A value the core gave before (a second definition) stays.
        if let Some(old) = self.map.get(&key) {
            let old = old.value.to_string_lossy().into_owned();
            if values.contains(&old) {
                value = old;
            }
        }
        let value = CString::new(value).unwrap_or_default();
        self.map.insert(key, Opt { value, values, label });
        self.dirty = true;
    }

    /// Libretro's first form: `"Description; first|second|third"`.
    ///
    /// # Safety
    ///
    /// `vars` is an array ended by a null key.
    pub unsafe fn define_v0(&mut self, vars: *const retro_variable) {
        let mut v = vars;
        // SAFETY: walking the array to its null key.
        while let Some(key) = string(unsafe { (*v).key }) {
            let spec = string(unsafe { (*v).value }).unwrap_or_default();
            let (label, values) = spec.split_once("; ").unwrap_or(("", &spec));
            let values: Vec<String> = values.split('|').map(str::to_owned).collect();
            self.define(key, label.to_owned(), values, None);
            v = unsafe { v.add(1) };
        }
    }

    /// # Safety
    ///
    /// `defs` is an array ended by a null key.
    pub unsafe fn define_v1(&mut self, defs: *const retro_core_option_definition) {
        let mut d = defs;
        // SAFETY: walking the array to its null key.
        while let Some(key) = string(unsafe { (*d).key }) {
            let def = unsafe { &*d };
            let values = def.values.iter().map_while(|v| string(v.value)).collect();
            self.define(key, string(def.desc).unwrap_or_default(), values, string(def.default_value));
            d = unsafe { d.add(1) };
        }
    }

    /// # Safety
    ///
    /// `opts` holds an array of definitions ended by a null key.
    pub unsafe fn define_v2(&mut self, opts: *const retro_core_options_v2) {
        if opts.is_null() {
            return;
        }
        // SAFETY: as promised.
        let mut d = unsafe { (*opts).definitions };
        if d.is_null() {
            return;
        }
        while let Some(key) = string(unsafe { (*d).key }) {
            let def = unsafe { &*d };
            let values = def.values.iter().map_while(|v| string(v.value)).collect();
            self.define(key, string(def.desc).unwrap_or_default(), values, string(def.default_value));
            d = unsafe { d.add(1) };
        }
    }

    pub fn get(&self, key: &str) -> Option<&CStr> {
        self.map.get(key).map(|o| o.value.as_c_str())
    }

    pub fn set(&mut self, key: &str, value: &str) -> bool {
        match self.map.get_mut(key) {
            Some(o) if o.values.iter().any(|v| v == value) || o.values.is_empty() => {
                o.value = CString::new(value).unwrap_or_default();
                self.dirty = true;
                true
            }
            _ => false,
        }
    }
}

/// What the running core may ask of the frontend.
pub struct Host {
    pub system_dir: CString,
    pub save_dir: CString,
    pub core_path: CString,
    pub username: CString,
    pub options: Options,
    pub av: retro_system_av_info,
    /// The core changed its timing or picture size.
    pub av_changed: bool,
    pub screen: Screen,
    /// This frame's sound, interleaved.
    pub sound: Vec<i16>,
    pub pads: Arc<Shared>,
    /// Controllers' presses reach the game (not under a menu).
    pub live: Arc<AtomicBool>,
    pub players: usize,
    pub rumble: Option<super::Rumble>,
    rumble_state: [[u16; 2]; 4],
    /// The core asked to end.
    pub shutdown: bool,
    pub messages: Vec<String>,
}

impl Host {
    pub fn new(screen: Screen, pads: Arc<Shared>, live: Arc<AtomicBool>) -> Host {
        Host {
            system_dir: CString::default(),
            save_dir: CString::default(),
            core_path: CString::default(),
            username: CString::new("Player").unwrap_or_default(),
            options: Options::default(),
            av: retro_system_av_info::default(),
            av_changed: false,
            screen,
            sound: Vec::with_capacity(4096),
            pads,
            live,
            players: 2,
            rumble: None,
            rumble_state: [[0; 2]; 4],
            shutdown: false,
            messages: Vec::new(),
        }
    }

    /// Make this the running core's host.
    ///
    /// # Safety
    ///
    /// `host` stays where it is, alive, until [`Host::unregister`]; no
    /// reference to it is held across a call into the core.
    pub unsafe fn register(host: *mut Host) {
        HOST.store(host, Ordering::Release);
    }

    pub fn unregister() {
        HOST.store(std::ptr::null_mut(), Ordering::Release);
    }

    /// Set the core's callbacks to this module's.
    ///
    /// # Safety
    ///
    /// `api` is a loaded core's.
    pub unsafe fn connect(api: &Api) {
        // SAFETY: the core's setters take these function types.
        unsafe {
            (api.set_environment)(environment);
            (api.set_video_refresh)(video_refresh);
            (api.set_audio_sample)(audio_sample);
            (api.set_audio_sample_batch)(audio_sample_batch);
            (api.set_input_poll)(input_poll);
            (api.set_input_state)(input_state);
        }
    }

    fn joypad(&self, port: usize) -> u32 {
        if port >= self.players || !self.live.load(Ordering::Acquire) {
            return 0;
        }
        self.pads.buttons(port)
    }

    /// Our buttons as libretro's joypad bits.
    fn retro_bits(b: u32) -> i16 {
        const ORDER: [(c_uint, Button); 16] = [
            (RETRO_DEVICE_ID_JOYPAD_B, Button::South),
            (RETRO_DEVICE_ID_JOYPAD_Y, Button::West),
            (RETRO_DEVICE_ID_JOYPAD_SELECT, Button::Select),
            (RETRO_DEVICE_ID_JOYPAD_START, Button::Start),
            (RETRO_DEVICE_ID_JOYPAD_UP, Button::Up),
            (RETRO_DEVICE_ID_JOYPAD_DOWN, Button::Down),
            (RETRO_DEVICE_ID_JOYPAD_LEFT, Button::Left),
            (RETRO_DEVICE_ID_JOYPAD_RIGHT, Button::Right),
            (RETRO_DEVICE_ID_JOYPAD_A, Button::East),
            (RETRO_DEVICE_ID_JOYPAD_X, Button::North),
            (RETRO_DEVICE_ID_JOYPAD_L, Button::L1),
            (RETRO_DEVICE_ID_JOYPAD_R, Button::R1),
            (RETRO_DEVICE_ID_JOYPAD_L2, Button::L2),
            (RETRO_DEVICE_ID_JOYPAD_R2, Button::R2),
            (RETRO_DEVICE_ID_JOYPAD_L3, Button::L3),
            (RETRO_DEVICE_ID_JOYPAD_R3, Button::R3),
        ];
        let mut out = 0u16;
        for (id, btn) in ORDER {
            if b & btn.bit() != 0 {
                out |= 1 << id;
            }
        }
        out as i16
    }

    fn input(&self, port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
        let port = port as usize;
        match device & RETRO_DEVICE_MASK {
            RETRO_DEVICE_JOYPAD => {
                let bits = Self::retro_bits(self.joypad(port));
                if id == RETRO_DEVICE_ID_JOYPAD_MASK { bits } else { i16::from(id < 16 && bits & (1 << id) != 0) }
            }
            RETRO_DEVICE_ANALOG => {
                if port >= self.players || !self.live.load(Ordering::Acquire) {
                    return 0;
                }
                let axis = match (index, id) {
                    (RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_X) => Axis::LeftX,
                    (RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_Y) => Axis::LeftY,
                    (RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_ID_ANALOG_X) => Axis::RightX,
                    (RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_ID_ANALOG_Y) => Axis::RightY,
                    (RETRO_DEVICE_INDEX_ANALOG_BUTTON, RETRO_DEVICE_ID_JOYPAD_L2) => Axis::L2,
                    (RETRO_DEVICE_INDEX_ANALOG_BUTTON, RETRO_DEVICE_ID_JOYPAD_R2) => Axis::R2,
                    (RETRO_DEVICE_INDEX_ANALOG_BUTTON, id) => {
                        let bits = Self::retro_bits(self.joypad(port));
                        return if id < 16 && bits & (1 << id) != 0 { 0x7fff } else { 0 };
                    }
                    _ => return 0,
                };
                self.pads.axis(port, axis)
            }
            _ => 0,
        }
    }

    /// Answer the core's question `cmd`: whether it was understood.
    ///
    /// # Safety
    ///
    /// `data` is what libretro says `cmd` comes with.
    unsafe fn environment(&mut self, cmd: c_uint, data: *mut c_void) -> bool {
        // SAFETY (each arm): `data` points to what libretro defines for
        // its command.
        unsafe {
            match cmd {
                RETRO_ENVIRONMENT_GET_CAN_DUPE => put(data, true),
                RETRO_ENVIRONMENT_GET_OVERSCAN => put(data, false),
                RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => {
                    let f = *(data as *const c_uint);
                    if f > RETRO_PIXEL_FORMAT_RGB565 {
                        return false;
                    }
                    self.screen.set_format(f);
                    true
                }
                RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY | RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY => {
                    put(data, self.system_dir.as_ptr())
                }
                RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => put(data, self.save_dir.as_ptr()),
                RETRO_ENVIRONMENT_GET_LIBRETRO_PATH => put(data, self.core_path.as_ptr()),
                RETRO_ENVIRONMENT_GET_USERNAME => put(data, self.username.as_ptr()),
                RETRO_ENVIRONMENT_GET_LANGUAGE => put(data, RETRO_LANGUAGE_ENGLISH),
                RETRO_ENVIRONMENT_GET_VARIABLE => {
                    let var = &mut *(data as *mut retro_variable);
                    let Some(key) = string(var.key) else { return false };
                    match self.options.get(&key) {
                        Some(v) => {
                            var.value = v.as_ptr();
                            true
                        }
                        None => {
                            var.value = std::ptr::null();
                            false
                        }
                    }
                }
                RETRO_ENVIRONMENT_SET_VARIABLES => {
                    self.options.define_v0(data as *const retro_variable);
                    true
                }
                RETRO_ENVIRONMENT_SET_VARIABLE => {
                    if data.is_null() {
                        return true;
                    }
                    let var = &*(data as *const retro_variable);
                    match (string(var.key), string(var.value)) {
                        (Some(k), Some(v)) => self.options.set(&k, &v),
                        _ => false,
                    }
                }
                RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
                    let dirty = std::mem::take(&mut self.options.dirty);
                    put(data, dirty)
                }
                RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION => put(data, 2 as c_uint),
                RETRO_ENVIRONMENT_SET_CORE_OPTIONS => {
                    self.options.define_v1(data as *const retro_core_option_definition);
                    true
                }
                RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL => {
                    let intl = &*(data as *const retro_core_options_intl);
                    self.options.define_v1(intl.us);
                    true
                }
                RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 => {
                    self.options.define_v2(data as *const retro_core_options_v2);
                    true
                }
                RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL => {
                    let intl = &*(data as *const retro_core_options_v2_intl);
                    self.options.define_v2(intl.us);
                    true
                }
                RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY
                | RETRO_ENVIRONMENT_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK => true,
                RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO => {
                    self.av = *(data as *const retro_system_av_info);
                    self.av_changed = true;
                    true
                }
                RETRO_ENVIRONMENT_SET_GEOMETRY => {
                    self.av.geometry = *(data as *const retro_game_geometry);
                    self.av_changed = true;
                    true
                }
                RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE => {
                    put(data, retro_rumble_interface { set_rumble_state: rumble })
                }
                RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES => {
                    put(data, (1u64 << RETRO_DEVICE_JOYPAD) | (1u64 << RETRO_DEVICE_ANALOG))
                }
                RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => true,
                RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS => put(data, self.players.min(4) as c_uint),
                RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE => {
                    if !data.is_null() {
                        *(data as *mut i32) = 3;
                    }
                    true
                }
                RETRO_ENVIRONMENT_GET_FASTFORWARDING => put(data, false),
                RETRO_ENVIRONMENT_GET_TARGET_REFRESH_RATE => put(data, 60.0f32),
                RETRO_ENVIRONMENT_GET_JIT_CAPABLE => put(data, true),
                RETRO_ENVIRONMENT_SHUTDOWN => {
                    self.shutdown = true;
                    true
                }
                RETRO_ENVIRONMENT_SET_MESSAGE => {
                    if let Some(m) = data.as_ref().and_then(|_| string((*(data as *const retro_message)).msg)) {
                        log::info!("chanoma: core says {m}");
                        self.messages.push(m);
                    }
                    true
                }
                // Told and taken note of; nothing for Chanoma to do.
                RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL
                | RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS
                | RETRO_ENVIRONMENT_SET_CONTROLLER_INFO
                | RETRO_ENVIRONMENT_SET_MEMORY_MAPS
                | RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO
                | RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS
                | RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS
                | RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME
                | RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE
                | RETRO_ENVIRONMENT_SET_MINIMUM_AUDIO_LATENCY => true,
                // Hardware rendering (OpenGL, Vulkan cores), logging (a C
                // varargs function), and the rest: not offered.
                _ => false,
            }
        }
    }

    fn set_rumble(&mut self, port: usize, effect: c_uint, strength: u16) -> bool {
        let Some(state) = self.rumble_state.get_mut(port) else { return false };
        let i = usize::from(effect == RETRO_RUMBLE_WEAK);
        if state[i] == strength {
            return true;
        }
        state[i] = strength;
        let [strong, weak] = *state;
        if let Some(r) = &self.rumble {
            r(port, strong, weak);
        }
        true
    }
}

/// Write `value` where `data` points: false for a null pointer.
///
/// # Safety
///
/// `data` is null or points to a `T`.
unsafe fn put<T>(data: *mut c_void, value: T) -> bool {
    if data.is_null() {
        return false;
    }
    // SAFETY: as promised.
    unsafe { (data as *mut T).write(value) };
    true
}

/// The running core's host, for a callback.
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    let p = HOST.load(Ordering::Acquire);
    // SAFETY: registered by the session, which holds no reference across
    // the core's calls (whose callbacks these are) and unregisters it
    // before it goes.
    (!p.is_null()).then(|| f(unsafe { &mut *p }))
}

unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    // SAFETY: the core passes what its command defines.
    with_host(|h| unsafe { h.environment(cmd, data) }).unwrap_or(false)
}

unsafe extern "C" fn video_refresh(data: *const c_void, width: c_uint, height: c_uint, pitch: usize) {
    // SAFETY: libretro's frame, as the callback defines it.
    with_host(|h| unsafe { h.screen.frame(data.cast(), width, height, pitch) });
}

unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    with_host(|h| h.sound.extend_from_slice(&[left, right]));
}

unsafe extern "C" fn audio_sample_batch(data: *const i16, frames: usize) -> usize {
    if data.is_null() {
        return 0;
    }
    // SAFETY: `frames` interleaved stereo frames.
    let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
    with_host(|h| h.sound.extend_from_slice(samples));
    frames
}

unsafe extern "C" fn input_poll() {}

unsafe extern "C" fn input_state(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
    with_host(|h| h.input(port, device, index, id)).unwrap_or(0)
}

unsafe extern "C" fn rumble(port: c_uint, effect: c_uint, strength: u16) -> bool {
    with_host(|h| h.set_rumble(port as usize, effect, strength)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_take_defaults_and_chanomas_preferences() {
        let mut o = Options::default();
        o.prefer.insert("core_region".into(), "NTSC".into());
        let keys = [c"core_region", c"core_speed", c"core_odd"];
        let specs = [c"Region; Auto|NTSC|PAL", c"Speed; 100%|200%", c"Nothing to split"];
        let vars = [
            retro_variable { key: keys[0].as_ptr(), value: specs[0].as_ptr() },
            retro_variable { key: keys[1].as_ptr(), value: specs[1].as_ptr() },
            retro_variable { key: keys[2].as_ptr(), value: specs[2].as_ptr() },
            retro_variable { key: std::ptr::null(), value: std::ptr::null() },
        ];
        // SAFETY: an array ended by a null key.
        unsafe { o.define_v0(vars.as_ptr()) };
        assert_eq!(o.get("core_region"), Some(c"NTSC"));
        assert_eq!(o.get("core_speed"), Some(c"100%"));
        assert_eq!(o.map["core_speed"].label, "Speed");
        assert_eq!(o.get("core_odd"), Some(c"Nothing to split"));
        assert!(o.set("core_speed", "200%"));
        assert!(!o.set("core_speed", "300%"));
        assert_eq!(o.get("core_speed"), Some(c"200%"));
    }

    #[test]
    fn buttons_reach_libretro_by_position() {
        let bits = Host::retro_bits(Button::South.bit() | Button::East.bit() | Button::Start.bit());
        assert_eq!(
            bits as u16,
            (1 << RETRO_DEVICE_ID_JOYPAD_B) | (1 << RETRO_DEVICE_ID_JOYPAD_A) | (1 << RETRO_DEVICE_ID_JOYPAD_START)
        );
    }
}
