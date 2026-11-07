//! The part of libretro's C interface (`libretro.h`, API version 1) a
//! frontend for cores that draw in software needs.

#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_uint, c_void};

pub const RETRO_API_VERSION: c_uint = 1;

pub const RETRO_DEVICE_NONE: c_uint = 0;
pub const RETRO_DEVICE_JOYPAD: c_uint = 1;
pub const RETRO_DEVICE_MOUSE: c_uint = 2;
pub const RETRO_DEVICE_KEYBOARD: c_uint = 3;
pub const RETRO_DEVICE_LIGHTGUN: c_uint = 4;
pub const RETRO_DEVICE_ANALOG: c_uint = 5;
pub const RETRO_DEVICE_POINTER: c_uint = 6;
pub const RETRO_DEVICE_MASK: c_uint = 0xff;

pub const RETRO_DEVICE_ID_JOYPAD_B: c_uint = 0;
pub const RETRO_DEVICE_ID_JOYPAD_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_JOYPAD_SELECT: c_uint = 2;
pub const RETRO_DEVICE_ID_JOYPAD_START: c_uint = 3;
pub const RETRO_DEVICE_ID_JOYPAD_UP: c_uint = 4;
pub const RETRO_DEVICE_ID_JOYPAD_DOWN: c_uint = 5;
pub const RETRO_DEVICE_ID_JOYPAD_LEFT: c_uint = 6;
pub const RETRO_DEVICE_ID_JOYPAD_RIGHT: c_uint = 7;
pub const RETRO_DEVICE_ID_JOYPAD_A: c_uint = 8;
pub const RETRO_DEVICE_ID_JOYPAD_X: c_uint = 9;
pub const RETRO_DEVICE_ID_JOYPAD_L: c_uint = 10;
pub const RETRO_DEVICE_ID_JOYPAD_R: c_uint = 11;
pub const RETRO_DEVICE_ID_JOYPAD_L2: c_uint = 12;
pub const RETRO_DEVICE_ID_JOYPAD_R2: c_uint = 13;
pub const RETRO_DEVICE_ID_JOYPAD_L3: c_uint = 14;
pub const RETRO_DEVICE_ID_JOYPAD_R3: c_uint = 15;
/// With `GET_INPUT_BITMASKS`: all the buttons at once.
pub const RETRO_DEVICE_ID_JOYPAD_MASK: c_uint = 256;

pub const RETRO_DEVICE_INDEX_ANALOG_LEFT: c_uint = 0;
pub const RETRO_DEVICE_INDEX_ANALOG_RIGHT: c_uint = 1;
pub const RETRO_DEVICE_INDEX_ANALOG_BUTTON: c_uint = 2;
pub const RETRO_DEVICE_ID_ANALOG_X: c_uint = 0;
pub const RETRO_DEVICE_ID_ANALOG_Y: c_uint = 1;

pub const RETRO_MEMORY_SAVE_RAM: c_uint = 0;
pub const RETRO_MEMORY_RTC: c_uint = 1;
pub const RETRO_MEMORY_SYSTEM_RAM: c_uint = 2;

pub const RETRO_ENVIRONMENT_EXPERIMENTAL: c_uint = 0x10000;
pub const RETRO_ENVIRONMENT_SET_ROTATION: c_uint = 1;
pub const RETRO_ENVIRONMENT_GET_OVERSCAN: c_uint = 2;
pub const RETRO_ENVIRONMENT_GET_CAN_DUPE: c_uint = 3;
pub const RETRO_ENVIRONMENT_SET_MESSAGE: c_uint = 6;
pub const RETRO_ENVIRONMENT_SHUTDOWN: c_uint = 7;
pub const RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL: c_uint = 8;
pub const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
pub const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub const RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK: c_uint = 12;
pub const RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE: c_uint = 13;
pub const RETRO_ENVIRONMENT_SET_HW_RENDER: c_uint = 14;
pub const RETRO_ENVIRONMENT_GET_VARIABLE: c_uint = 15;
pub const RETRO_ENVIRONMENT_SET_VARIABLES: c_uint = 16;
pub const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
pub const RETRO_ENVIRONMENT_GET_LIBRETRO_PATH: c_uint = 19;
pub const RETRO_ENVIRONMENT_SET_FRAME_TIME_CALLBACK: c_uint = 21;
pub const RETRO_ENVIRONMENT_SET_AUDIO_CALLBACK: c_uint = 22;
pub const RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE: c_uint = 23;
pub const RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES: c_uint = 24;
pub const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
pub const RETRO_ENVIRONMENT_GET_PERF_INTERFACE: c_uint = 28;
pub const RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY: c_uint = 30;
pub const RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: c_uint = 31;
pub const RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO: c_uint = 32;
pub const RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO: c_uint = 34;
pub const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;
pub const RETRO_ENVIRONMENT_SET_MEMORY_MAPS: c_uint = 36 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
pub const RETRO_ENVIRONMENT_GET_USERNAME: c_uint = 38;
pub const RETRO_ENVIRONMENT_GET_LANGUAGE: c_uint = 39;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS: c_uint = 42 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS: c_uint = 44;
pub const RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE: c_uint = 47 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_FASTFORWARDING: c_uint = 49 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_TARGET_REFRESH_RATE: c_uint = 50 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: c_uint = 51 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS: c_uint = 53;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL: c_uint = 54;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY: c_uint = 55;
pub const RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER: c_uint = 56;
pub const RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION: c_uint = 57;
pub const RETRO_ENVIRONMENT_GET_MESSAGE_INTERFACE_VERSION: c_uint = 59;
pub const RETRO_ENVIRONMENT_SET_MESSAGE_EXT: c_uint = 60;
pub const RETRO_ENVIRONMENT_GET_INPUT_MAX_USERS: c_uint = 61;
pub const RETRO_ENVIRONMENT_SET_AUDIO_BUFFER_STATUS_CALLBACK: c_uint = 62;
pub const RETRO_ENVIRONMENT_SET_MINIMUM_AUDIO_LATENCY: c_uint = 63;
pub const RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE: c_uint = 65;
pub const RETRO_ENVIRONMENT_GET_GAME_INFO_EXT: c_uint = 66;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2: c_uint = 67;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL: c_uint = 68;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK: c_uint = 69;
pub const RETRO_ENVIRONMENT_SET_VARIABLE: c_uint = 70;
pub const RETRO_ENVIRONMENT_GET_THROTTLE_STATE: c_uint = 71 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_SAVESTATE_CONTEXT: c_uint = 72 | RETRO_ENVIRONMENT_EXPERIMENTAL;
pub const RETRO_ENVIRONMENT_GET_JIT_CAPABLE: c_uint = 74;

pub const RETRO_PIXEL_FORMAT_0RGB1555: c_uint = 0;
pub const RETRO_PIXEL_FORMAT_XRGB8888: c_uint = 1;
pub const RETRO_PIXEL_FORMAT_RGB565: c_uint = 2;

pub const RETRO_LANGUAGE_ENGLISH: c_uint = 0;

pub const RETRO_RUMBLE_STRONG: c_uint = 0;
pub const RETRO_RUMBLE_WEAK: c_uint = 1;

pub const RETRO_NUM_CORE_OPTION_VALUES_MAX: usize = 128;

#[repr(C)]
pub struct retro_system_info {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct retro_game_geometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct retro_system_timing {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct retro_system_av_info {
    pub geometry: retro_game_geometry,
    pub timing: retro_system_timing,
}

#[repr(C)]
pub struct retro_game_info {
    pub path: *const c_char,
    pub data: *const c_void,
    pub size: usize,
    pub meta: *const c_char,
}

#[repr(C)]
pub struct retro_variable {
    pub key: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct retro_core_option_value {
    pub value: *const c_char,
    pub label: *const c_char,
}

#[repr(C)]
pub struct retro_core_option_definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
    pub values: [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

#[repr(C)]
pub struct retro_core_options_intl {
    pub us: *const retro_core_option_definition,
    pub local: *const retro_core_option_definition,
}

#[repr(C)]
pub struct retro_core_option_v2_definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub desc_categorized: *const c_char,
    pub info: *const c_char,
    pub info_categorized: *const c_char,
    pub category_key: *const c_char,
    pub values: [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

#[repr(C)]
pub struct retro_core_options_v2 {
    pub categories: *const c_void,
    pub definitions: *const retro_core_option_v2_definition,
}

#[repr(C)]
pub struct retro_core_options_v2_intl {
    pub us: *const retro_core_options_v2,
    pub local: *const retro_core_options_v2,
}

#[repr(C)]
pub struct retro_rumble_interface {
    pub set_rumble_state: unsafe extern "C" fn(port: c_uint, effect: c_uint, strength: u16) -> bool,
}

#[repr(C)]
pub struct retro_message {
    pub msg: *const c_char,
    pub frames: c_uint,
}

pub type retro_environment_t = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
pub type retro_video_refresh_t = unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
pub type retro_audio_sample_t = unsafe extern "C" fn(left: i16, right: i16);
pub type retro_audio_sample_batch_t = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
pub type retro_input_poll_t = unsafe extern "C" fn();
pub type retro_input_state_t = unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;

/// A core's entry points.
#[derive(Clone, Copy)]
pub struct Api {
    pub set_environment: unsafe extern "C" fn(retro_environment_t),
    pub set_video_refresh: unsafe extern "C" fn(retro_video_refresh_t),
    pub set_audio_sample: unsafe extern "C" fn(retro_audio_sample_t),
    pub set_audio_sample_batch: unsafe extern "C" fn(retro_audio_sample_batch_t),
    pub set_input_poll: unsafe extern "C" fn(retro_input_poll_t),
    pub set_input_state: unsafe extern "C" fn(retro_input_state_t),
    pub init: unsafe extern "C" fn(),
    pub deinit: unsafe extern "C" fn(),
    pub api_version: unsafe extern "C" fn() -> c_uint,
    pub get_system_info: unsafe extern "C" fn(*mut retro_system_info),
    pub get_system_av_info: unsafe extern "C" fn(*mut retro_system_av_info),
    pub set_controller_port_device: unsafe extern "C" fn(port: c_uint, device: c_uint),
    pub reset: unsafe extern "C" fn(),
    pub run: unsafe extern "C" fn(),
    pub serialize_size: unsafe extern "C" fn() -> usize,
    pub serialize: unsafe extern "C" fn(data: *mut c_void, size: usize) -> bool,
    pub unserialize: unsafe extern "C" fn(data: *const c_void, size: usize) -> bool,
    pub load_game: unsafe extern "C" fn(game: *const retro_game_info) -> bool,
    pub unload_game: unsafe extern "C" fn(),
    pub get_memory_data: unsafe extern "C" fn(id: c_uint) -> *mut c_void,
    pub get_memory_size: unsafe extern "C" fn(id: c_uint) -> usize,
}

impl Api {
    /// The entry points of a core library.
    ///
    /// # Safety
    ///
    /// `lib` must be a libretro core: its symbols are taken to have
    /// libretro's types.
    pub unsafe fn of(lib: &libloading::Library) -> Result<Api, String> {
        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: a libretro core exports these with these types.
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
            };
        }
        Ok(Api {
            set_environment: sym!("retro_set_environment"),
            set_video_refresh: sym!("retro_set_video_refresh"),
            set_audio_sample: sym!("retro_set_audio_sample"),
            set_audio_sample_batch: sym!("retro_set_audio_sample_batch"),
            set_input_poll: sym!("retro_set_input_poll"),
            set_input_state: sym!("retro_set_input_state"),
            init: sym!("retro_init"),
            deinit: sym!("retro_deinit"),
            api_version: sym!("retro_api_version"),
            get_system_info: sym!("retro_get_system_info"),
            get_system_av_info: sym!("retro_get_system_av_info"),
            set_controller_port_device: sym!("retro_set_controller_port_device"),
            reset: sym!("retro_reset"),
            run: sym!("retro_run"),
            serialize_size: sym!("retro_serialize_size"),
            serialize: sym!("retro_serialize"),
            unserialize: sym!("retro_unserialize"),
            load_game: sym!("retro_load_game"),
            unload_game: sym!("retro_unload_game"),
            get_memory_data: sym!("retro_get_memory_data"),
            get_memory_size: sym!("retro_get_memory_size"),
        })
    }
}
