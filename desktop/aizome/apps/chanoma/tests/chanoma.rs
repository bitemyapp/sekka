//! Chanoma in the kit's headless harness: the first setup, Home, the PIN
//! and its lockout, time running out, and a game played start to finish
//! through a libretro core written here (its frames reach the window's
//! video, the controller reaches it, it pauses under the menu and carries
//! on from where it was left).
//!
//! With `CHANOMA_SHOTS=<dir>`, each screen is also saved there as a PNG
//! at 1920 × 1080, to look at.

use std::ffi::{c_uint, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aizome_chanoma::app::{Input, MenuKind, Modal, Screen};
use aizome_chanoma::clock::ManualClock;
use aizome_chanoma::pad::{Button, Dir, Family, PadEvent, PadInfo};
use aizome_chanoma::retro::ffi::*;
use aizome_chanoma::rules::{PinHash, Settings};
use aizome_chanoma::store::Dirs;
use aizome_chanoma::{Chanoma, Config, Msg};
use aizome_kit::Size;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};

// A libretro core: a 160 × 120 picture, blue (green while B is held) with
// a white square that moves right while Right is held; it counts frames,
// keeps the count in its state, and writes it to its save RAM on Start.

static CORE: Mutex<()> = Mutex::new(());
static ENV: Mutex<Option<retro_environment_t>> = Mutex::new(None);
static VIDEO: Mutex<Option<retro_video_refresh_t>> = Mutex::new(None);
static BATCH: Mutex<Option<retro_audio_sample_batch_t>> = Mutex::new(None);
static INPUT: Mutex<Option<retro_input_state_t>> = Mutex::new(None);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static X: AtomicU32 = AtomicU32::new(0);
static HELD_B: AtomicBool = AtomicBool::new(false);
static LOADED: AtomicBool = AtomicBool::new(false);
static mut SRAM: [u8; 32] = [0; 32];
static OPTION: Mutex<String> = Mutex::new(String::new());

unsafe extern "C" fn set_environment(cb: retro_environment_t) {
    *ENV.lock().unwrap() = Some(cb);
    let key = c"test_speed";
    let spec = c"Speed; slow|fast";
    let vars = [
        retro_variable { key: key.as_ptr(), value: spec.as_ptr() },
        retro_variable { key: std::ptr::null(), value: std::ptr::null() },
    ];
    unsafe { cb(RETRO_ENVIRONMENT_SET_VARIABLES, vars.as_ptr() as *mut c_void) };
}
unsafe extern "C" fn set_video(cb: retro_video_refresh_t) {
    *VIDEO.lock().unwrap() = Some(cb);
}
unsafe extern "C" fn set_sample(_: retro_audio_sample_t) {}
unsafe extern "C" fn set_batch(cb: retro_audio_sample_batch_t) {
    *BATCH.lock().unwrap() = Some(cb);
}
unsafe extern "C" fn set_poll(_: retro_input_poll_t) {}
unsafe extern "C" fn set_input(cb: retro_input_state_t) {
    *INPUT.lock().unwrap() = Some(cb);
}
unsafe extern "C" fn init() {
    FRAMES.store(0, Ordering::SeqCst);
    X.store(0, Ordering::SeqCst);
}
unsafe extern "C" fn deinit() {}
unsafe extern "C" fn api_version() -> c_uint {
    RETRO_API_VERSION
}
unsafe extern "C" fn system_info(info: *mut retro_system_info) {
    unsafe {
        (*info).library_name = c"Test Core".as_ptr();
        (*info).library_version = c"1".as_ptr();
        (*info).valid_extensions = c"sfc".as_ptr();
        (*info).need_fullpath = false;
        (*info).block_extract = false;
    }
}
unsafe extern "C" fn av_info(info: *mut retro_system_av_info) {
    unsafe {
        *info = retro_system_av_info {
            geometry: retro_game_geometry {
                base_width: 160,
                base_height: 120,
                max_width: 160,
                max_height: 120,
                aspect_ratio: 4.0 / 3.0,
            },
            timing: retro_system_timing { fps: 60.0, sample_rate: 32_000.0 },
        };
    }
}
unsafe extern "C" fn port_device(_: c_uint, _: c_uint) {}
unsafe extern "C" fn reset() {
    FRAMES.store(0, Ordering::SeqCst);
    X.store(0, Ordering::SeqCst);
}
unsafe extern "C" fn run() {
    let input = INPUT.lock().unwrap().unwrap();
    let env = ENV.lock().unwrap().unwrap();
    let mut var = retro_variable { key: c"test_speed".as_ptr(), value: std::ptr::null() };
    if unsafe { env(RETRO_ENVIRONMENT_GET_VARIABLE, (&mut var as *mut retro_variable).cast()) } && !var.value.is_null()
    {
        *OPTION.lock().unwrap() = unsafe { std::ffi::CStr::from_ptr(var.value) }.to_string_lossy().into_owned();
    }
    let bits = unsafe { input(0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_MASK) } as u16;
    let b = bits & (1 << RETRO_DEVICE_ID_JOYPAD_B) != 0;
    HELD_B.store(b, Ordering::SeqCst);
    if bits & (1 << RETRO_DEVICE_ID_JOYPAD_RIGHT) != 0 {
        X.store((X.load(Ordering::SeqCst) + 1) % 150, Ordering::SeqCst);
    }
    let n = FRAMES.fetch_add(1, Ordering::SeqCst) + 1;
    if bits & (1 << RETRO_DEVICE_ID_JOYPAD_START) != 0 {
        let sram = std::ptr::addr_of_mut!(SRAM).cast::<u8>();
        unsafe { std::ptr::copy_nonoverlapping(n.to_le_bytes().as_ptr(), sram, 8) };
    }
    let x = X.load(Ordering::SeqCst) as usize;
    let mut px = vec![if b { 0x0030_a050u32 } else { 0x0020_3080 }; 160 * 120];
    for y in 50..60 {
        for xx in x..x + 10 {
            px[y * 160 + xx] = 0x00ff_ffff;
        }
    }
    let video = VIDEO.lock().unwrap().unwrap();
    unsafe { video(px.as_ptr().cast(), 160, 120, 160 * 4) };
    let sound = vec![0i16; 533 * 2];
    let batch = BATCH.lock().unwrap().unwrap();
    unsafe { batch(sound.as_ptr(), 533) };
}
unsafe extern "C" fn serialize_size() -> usize {
    16
}
unsafe extern "C" fn serialize(data: *mut c_void, size: usize) -> bool {
    if size < 16 {
        return false;
    }
    let mut s = FRAMES.load(Ordering::SeqCst).to_le_bytes().to_vec();
    s.extend_from_slice(&u64::from(X.load(Ordering::SeqCst)).to_le_bytes());
    unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), data.cast::<u8>(), 16) };
    true
}
unsafe extern "C" fn unserialize(data: *const c_void, size: usize) -> bool {
    if size < 16 {
        return false;
    }
    let s = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), 16) };
    FRAMES.store(u64::from_le_bytes(s[..8].try_into().unwrap()), Ordering::SeqCst);
    X.store(u64::from_le_bytes(s[8..].try_into().unwrap()) as u32, Ordering::SeqCst);
    true
}
unsafe extern "C" fn load_game(game: *const retro_game_info) -> bool {
    let env = ENV.lock().unwrap().unwrap();
    let mut format = RETRO_PIXEL_FORMAT_XRGB8888;
    unsafe { env(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, (&mut format as *mut c_uint).cast()) };
    let g = unsafe { &*game };
    let ok =
        !g.data.is_null() && unsafe { std::slice::from_raw_parts(g.data.cast::<u8>(), g.size) }.starts_with(b"CHANOMA");
    LOADED.store(ok, Ordering::SeqCst);
    ok
}
unsafe extern "C" fn unload_game() {
    LOADED.store(false, Ordering::SeqCst);
}
unsafe extern "C" fn memory_data(id: c_uint) -> *mut c_void {
    if id == RETRO_MEMORY_SAVE_RAM { std::ptr::addr_of_mut!(SRAM).cast() } else { std::ptr::null_mut() }
}
unsafe extern "C" fn memory_size(id: c_uint) -> usize {
    if id == RETRO_MEMORY_SAVE_RAM { 32 } else { 0 }
}

fn test_core() -> Api {
    Api {
        set_environment,
        set_video_refresh: set_video,
        set_audio_sample: set_sample,
        set_audio_sample_batch: set_batch,
        set_input_poll: set_poll,
        set_input_state: set_input,
        init,
        deinit,
        api_version,
        get_system_info: system_info,
        get_system_av_info: av_info,
        set_controller_port_device: port_device,
        reset,
        run,
        serialize_size,
        serialize,
        unserialize,
        load_game,
        unload_game,
        get_memory_data: memory_data,
        get_memory_size: memory_size,
    }
}

// The harness.

const PIN: &str = "2468";

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("chanoma-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A household set up with the PIN, a game folder holding one game, and a
/// stand-in core for it.
fn household(name: &str, settings: impl FnOnce(&mut Settings)) -> (Dirs, PathBuf) {
    let root = temp(name);
    let dirs = Dirs::under(&root);
    let games = root.join("Games");
    std::fs::create_dir_all(games.join("snes")).unwrap();
    std::fs::write(games.join("snes/Test Quest (USA).sfc"), b"CHANOMA test game").unwrap();
    let cores = root.join("cores");
    std::fs::create_dir_all(&cores).unwrap();
    std::fs::write(cores.join("snes9x_libretro.so"), b"").unwrap();
    let mut s = Settings {
        pin: Some(PinHash::new(PIN, 10)),
        pin_length: Some(4),
        game_folders: vec![games],
        setup_done: true,
        ..Settings::default()
    };
    s.games_allowed.insert("snes/snes/Test Quest (USA).sfc".into());
    settings(&mut s);
    aizome_chanoma::store::write(&dirs.settings(), &s).unwrap();
    (dirs, cores)
}

fn config(dirs: Dirs, cores: PathBuf, clock: ManualClock) -> Config {
    Config {
        dirs,
        clock: Box::new(clock),
        pads: false,
        grab: false,
        silent: true,
        pin_rounds: 10,
        core_dirs: vec![cores],
        database: None,
        fullscreen: false,
        scan: true,
        core_override: Some(test_core()),
        signals: false,
        h24: true,
    }
}

fn harness(config: Config) -> Harness<Chanoma> {
    let mut h = Harness::new(Chanoma::new(config));
    let w = h.window();
    h.resize(w, Size::new(1920.0, 1080.0));
    let pad = PadInfo {
        id: 1,
        slot: 0,
        name: "Xbox Wireless Controller".into(),
        family: Family::Xbox,
        bluetooth: true,
        guid: String::new(),
        battery: Some((80, false)),
        source: aizome_chanoma::pad::mapping::Source::Database,
        rumble: true,
    };
    h.send(Msg::Pad(PadEvent::Connected(pad)));
    until(&mut h, |a| a.scanned);
    h
}

/// Take the program's messages until `f` holds (threads answer in real
/// time: generous on a loaded machine).
fn until(h: &mut Harness<Chanoma>, f: impl Fn(&Chanoma) -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f(h.program()) {
        assert!(Instant::now() < end, "timed out on {:?}", h.program().screen);
        h.settle(Duration::from_millis(20), |a| f(a));
    }
}

fn input(h: &mut Harness<Chanoma>, i: Input) {
    h.send(Msg::Input(i));
}

fn press(h: &mut Harness<Chanoma>, b: Button) {
    for pressed in [true, false] {
        h.send(Msg::Pad(PadEvent::Button { id: 1, slot: 0, button: b, pressed }));
    }
}

/// Move the PIN pad's focus to key `target` with the arrows.
fn pad_to(h: &mut Harness<Chanoma>, target: usize) {
    for _ in 0..8 {
        let f = h.program().modal_focus;
        let dir = if f / 3 > target / 3 {
            Dir::Up
        } else if f / 3 < target / 3 {
            Dir::Down
        } else if f % 3 > target % 3 {
            Dir::Left
        } else if f % 3 < target % 3 {
            Dir::Right
        } else {
            return;
        };
        input(h, Input::Dir(dir));
    }
    panic!("couldn't reach key {target}");
}

fn type_pin(h: &mut Harness<Chanoma>, pin: &str) {
    for d in pin.bytes() {
        input(h, Input::Digit(d - b'0'));
    }
}

fn shot(h: &mut Harness<Chanoma>, name: &str) {
    if let Some(dir) = std::env::var_os("CHANOMA_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let w = h.window();
        // In Night, and in Day and High contrast beside it: Aizome's apps
        // must look finished in each.
        for (suffix, theme) in
            [("-day", ThemeChoice::Day), ("-hc", ThemeChoice::HighContrast), ("", ThemeChoice::Night)]
        {
            h.set_appearance(Appearance { theme, ..Appearance::default() });
            h.save_png(w, dir.join(format!("{name}{suffix}.png")), 1.0);
        }
    }
}

fn core_lock() -> MutexGuard<'static, ()> {
    CORE.lock().unwrap_or_else(|e| e.into_inner())
}

fn exists(p: &Path) -> bool {
    p.is_file()
}

#[test]
fn the_first_setup_chooses_a_pin_then_hands_over_to_the_kids() {
    let root = temp("first");
    let dirs = Dirs::under(&root);
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs.clone(), root.join("cores"), clock));
    assert_eq!(h.program().screen, Screen::Welcome);
    shot(&mut h, "01-welcome");
    input(&mut h, Input::Ok);
    assert!(matches!(h.program().modal, Some(Modal::Pin { .. })));
    shot(&mut h, "02-choose-pin");
    type_pin(&mut h, "13579");
    input(&mut h, Input::Ok);
    // The controller alone: the arrows to each key, then OK. Another PIN
    // than the first: they don't match.
    for key in [0, 2, 4, 6, 7, 11] {
        pad_to(&mut h, key);
        input(&mut h, Input::Ok);
    }
    let Some(Modal::Pin { pin, .. }) = &h.program().modal else { panic!("the pad went") };
    assert_eq!(pin.error.as_deref(), Some("They didn't match. Try again"));
    type_pin(&mut h, "13579");
    input(&mut h, Input::Ok);
    type_pin(&mut h, "13579");
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.screen == Screen::GrownUps);
    assert!(h.program().is_grownup());
    assert_eq!(h.program().settings.pin_length, Some(5));
    shot(&mut h, "03-settings-time");
    // Weekday time: one step right is half an hour.
    input(&mut h, Input::Ok);
    input(&mut h, Input::Dir(Dir::Right));
    assert_eq!(h.program().settings.limits.daily[0], Some(30));
    assert_eq!(h.program().settings.limits.daily[4], Some(30));
    assert_eq!(h.program().settings.limits.daily[5], None);
    // Back, Back: out, and the kids have it.
    input(&mut h, Input::Back);
    input(&mut h, Input::Back);
    assert_eq!(h.program().screen, Screen::Home);
    assert!(!h.program().is_grownup());
    let saved: Settings = aizome_chanoma::store::read(&dirs.settings());
    assert!(saved.setup_done && saved.pin.as_ref().is_some_and(|p| p.verify("13579")));
}

#[test]
fn home_shows_what_kids_may_have_and_grown_ups_see_the_rest() {
    let (dirs, cores) = household("home", |s| {
        s.services = vec!["pbs-kids".into(), "netflix".into()];
    });
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    assert_eq!(h.program().screen, Screen::Home);
    let titles: Vec<String> = h.program().home_view().shelves.iter().map(|s| s.title.clone()).collect();
    assert_eq!(titles, ["Watch", "Games", "More"]);
    let watch: Vec<String> = h.program().home_view().shelves[0].tiles.iter().map(|t| t.label.clone()).collect();
    assert_eq!(watch, ["PBS KIDS", "Netflix"]);
    shot(&mut h, "04-home-kids");
    // The grown-ups' door asks for the PIN.
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    assert!(matches!(h.program().modal, Some(Modal::Pin { .. })));
    shot(&mut h, "05-pin");
    type_pin(&mut h, PIN);
    until(&mut h, |a| a.is_grownup());
    assert!(h.program().modal.is_none());
    let shelves = h.program().home_view().shelves;
    assert!(shelves[0].tiles.len() > 5, "every service shows to a grown-up");
    assert!(shelves[0].tiles.iter().any(|t| t.sub == "Grown-ups only"));
    shot(&mut h, "06-home-grownup");
}

#[test]
fn wrong_pins_wait_longer_each_time() {
    let (dirs, cores) = household("pins", |_| {});
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs.clone(), cores, clock));
    assert_eq!(h.program().screen, Screen::Home);
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    for _ in 0..3 {
        type_pin(&mut h, "0000");
        until(&mut h, |a| matches!(&a.modal, Some(Modal::Pin { pin, .. }) if !pin.checking));
    }
    let now = h.program().now_secs();
    assert_eq!(h.program().lockout.wait(now), Some(30));
    shot(&mut h, "07-pin-locked");
    // While it waits, digits do nothing.
    type_pin(&mut h, PIN);
    assert!(!h.program().is_grownup());
    // The wait survives a restart.
    let saved: aizome_chanoma::rules::Lockout = aizome_chanoma::store::read(&dirs.state.join("lockout.json"));
    assert_eq!(saved.failures, 3);
}

#[test]
fn a_game_plays_pauses_and_carries_on_where_it_was_left() {
    let _core = core_lock();
    let (dirs, cores) = household("game", |_| {});
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs.clone(), cores, clock));
    // Games is the second shelf.
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.screen == Screen::Playing);
    assert!(LOADED.load(Ordering::SeqCst));
    assert_eq!(*OPTION.lock().unwrap(), "slow", "the core's option, at its default");
    let shared = h.program().shared.clone();
    shared.set(0, true, Button::South.bit() | Button::Right.bit(), [0.0; 6]);
    let start = Instant::now();
    while !(HELD_B.load(Ordering::SeqCst) && X.load(Ordering::SeqCst) > 5) {
        assert!(start.elapsed() < Duration::from_secs(10), "the controller didn't reach the game");
        std::thread::sleep(Duration::from_millis(10));
    }
    // Frames reach the window's video.
    let sinks: Vec<u64> = h.platform.video_sinks.values().map(|(_, d)| d.shown()).collect();
    assert!(sinks.iter().any(|n| *n > 5), "frames shown: {sinks:?}");
    let (_, detached) = h.platform.video_sinks.values().next().unwrap();
    assert_eq!(detached.last_size(), Some((160 * 6, 120 * 6)), "enlarged as far as it goes for 1080 lines");
    shared.set(0, true, 0, [0.0; 6]);
    // A keyboard reaches the game's window too (its stage moved when the
    // picture came): F1 is Home, Escape closes the menu again.
    let w = h.window();
    h.named(w, NamedKey::F(1), Modifiers::NONE);
    assert!(matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Game, .. })), "F1 opens the menu");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().modal.is_none(), "Escape closes it");
    // Home pauses it under the menu.
    press(&mut h, Button::Guide);
    assert!(matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Game, .. })));
    std::thread::sleep(Duration::from_millis(100));
    let paused_at = FRAMES.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(FRAMES.load(Ordering::SeqCst), paused_at, "paused");
    shot(&mut h, "08-game-menu");
    // Stop playing: kept where it was left.
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Home);
    let state = dirs.states().join("snes~snes~Test Quest (USA).sfc.state");
    assert!(exists(&state), "{}", state.display());
    assert!(exists(&dirs.states().join("snes~snes~Test Quest (USA).sfc.png")));
    let left_at = FRAMES.load(Ordering::SeqCst);
    // It leads Home now, and carries on from there.
    let home = h.program().home_view();
    assert_eq!(home.shelves[0].title, "Keep playing");
    assert_eq!(h.program().focus.row, 0, "Home starts at the top again");
    shot(&mut h, "09-home-keep-playing");
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.screen == Screen::Playing);
    let start = Instant::now();
    while FRAMES.load(Ordering::SeqCst) < left_at {
        assert!(start.elapsed() < Duration::from_secs(10), "it didn't carry on");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(FRAMES.load(Ordering::SeqCst) >= left_at);
    press(&mut h, Button::Guide);
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
}

#[test]
fn when_time_runs_out_the_game_is_kept_and_a_grown_up_can_give_more() {
    let _core = core_lock();
    // Saturday: an hour.
    let (dirs, cores) = household("time", |s| {
        s.limits.daily[5] = Some(60);
    });
    let clock = ManualClock::at(2026, 10, 31, 15, 0);
    let mut h = harness(config(dirs.clone(), cores, clock));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.screen == Screen::Playing);
    // Most of the hour gone: a warning.
    h.program_mut().usage.used = 56 * 60;
    h.send(Msg::Tick);
    assert_eq!(h.program().toast.as_ref().map(|t| t.0.as_str()), Some("Five minutes left"));
    shot(&mut h, "10-five-minutes");
    h.program_mut().usage.used = 60 * 60;
    h.send(Msg::Tick);
    assert_eq!(h.program().screen, Screen::Closed);
    assert!(h.program().game.is_none());
    assert!(exists(&dirs.states().join("snes~snes~Test Quest (USA).sfc.state")), "kept where it was left");
    shot(&mut h, "11-times-up");
    // Kids can't start anything now.
    input(&mut h, Input::Home);
    assert_eq!(h.program().screen, Screen::Closed);
    // A grown-up gives a quarter of an hour.
    input(&mut h, Input::Ok);
    type_pin(&mut h, PIN);
    until(&mut h, |a| matches!(a.modal, Some(Modal::Menu { kind: MenuKind::Time, .. })));
    shot(&mut h, "12-more-time");
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Home);
    assert!(!h.program().is_grownup(), "back to the kids");
    assert!(matches!(h.program().gate, aizome_chanoma::rules::Gate::Open { left: Some(900) }));
}

#[test]
fn bedtime_closes_up_for_the_night() {
    let (dirs, cores) = household("bedtime", |s| {
        s.limits.bedtime = Some(aizome_chanoma::rules::Bedtime { from: 19 * 60 + 30, until: 7 * 60 });
    });
    let clock = ManualClock::at(2026, 10, 31, 20, 15);
    let mut h = harness(config(dirs, cores, clock));
    assert_eq!(h.program().screen, Screen::Closed);
    shot(&mut h, "13-bedtime");
}
