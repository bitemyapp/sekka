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

use aizome_chanoma::app::play::FilmItem;
use aizome_chanoma::app::state::Subtitles as StateSubtitles;
use aizome_chanoma::app::{Input, MenuKind, Modal, Screen};
use aizome_chanoma::clock::ManualClock;
use aizome_chanoma::pad::{Button, Dir, Family, PadEvent, PadInfo};
use aizome_chanoma::retro::ffi::*;
use aizome_chanoma::rules::{PinHash, Settings};
use aizome_chanoma::store::Dirs;
use aizome_chanoma::{Chanoma, Config, Msg};
use aizome_kit::layout::Point;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_kit::{Size, WindowKey};
use aizome_media::CueBody;

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
    let root = cores.parent().unwrap().to_path_buf();
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
        // Beside the cores: a home and a folder drives appear in, for the
        // folder picker.
        home: root.join("home"),
        mounts: vec![root.join("media")],
        mpris: false,
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
    let w = h.window();
    shot_of(h, w, name);
}

fn shot_of(h: &mut Harness<Chanoma>, w: WindowKey, name: &str) {
    if let Some(dir) = std::env::var_os("CHANOMA_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
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

/// One of the engine's test media (a film with subtitles beside it, say).
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/aizome-media/tests/fixtures").join(name)
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
    // Escape alone opens it too, and a click beside it puts it away.
    let game_menu = |h: &Harness<Chanoma>| matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Game, .. }));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(game_menu(&h), "Escape opens the menu");
    h.click(w, Point::new(40.0, 540.0));
    assert!(h.program().modal.is_none(), "a click beside the menu");
    // The pointer moving brings the bar down; its Home is the menu.
    h.move_to(w, Point::new(960.0, 540.0));
    assert!(h.program().bar_until.is_some(), "the bar comes with the pointer");
    shot(&mut h, "08-game-bar");
    h.click(w, Point::new(200.0, 76.0));
    assert!(game_menu(&h), "the bar's Home");
    assert!(h.program().bar_until.is_none(), "the menu in its place");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    // A click on the game: the menu.
    h.click(w, Point::new(960.0, 700.0));
    assert!(game_menu(&h), "a click on the picture");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().modal.is_none());
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
fn a_film_shows_its_subtitles_and_keeps_the_choice_for_next_time() {
    // The engine's two-second test card, with a SubRip file beside it.
    let videos = std::env::temp_dir().join(format!("chanoma-test-film-{}", std::process::id())).join("Videos");
    let (dirs, cores) = household("film", |s| s.video_folders = vec![videos.clone()]);
    std::fs::create_dir_all(&videos).unwrap();
    for f in ["clip.mp4", "clip.srt"] {
        std::fs::copy(fixture(f), videos.join(f)).unwrap();
    }
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    let open_film = |h: &mut Harness<Chanoma>| {
        let row = h.program().home_view().shelves.iter().position(|s| s.title == "Films and shows").expect("films");
        while h.program().focus.row < row {
            input(h, Input::Dir(Dir::Down));
        }
        input(h, Input::Ok);
        until(h, |a| a.screen == Screen::Film && a.film.as_ref().is_some_and(|f| f.info.is_some()));
    };
    open_film(&mut h);

    // Its subtitles, found beside it, show as it plays; paused there.
    until(&mut h, |a| a.film_subtitles().is_some());
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.film.as_ref().is_some_and(|f| !f.playing));
    let subs = h.program().film_subtitles().expect("still showing, paused");
    assert!(matches!(&subs.cues[0].body, CueBody::Text { text, italic: true, .. } if text == "First line"));
    shot(&mut h, "16-film-subtitles");

    // The film's menu offers them (one sound track: no language to choose).
    input(&mut h, Input::Back);
    assert!(matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Film, .. })));
    let (items, words) = h.program().film_menu();
    assert_eq!(items, [FilmItem::Keep, FilmItem::Subtitles, FilmItem::Stop]);
    assert_eq!(words[1], "Subtitles: Track 1");
    shot(&mut h, "17-film-menu");
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    assert!(matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Subtitles, .. })));
    assert_eq!(h.program().modal_focus, 1, "on the track showing");
    shot(&mut h, "18-film-subtitles-menu");
    // Back goes back to the film's menu; there again, Off.
    input(&mut h, Input::Back);
    assert!(matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Film, .. })));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    input(&mut h, Input::Dir(Dir::Up));
    input(&mut h, Input::Ok);
    assert!(h.program().modal.is_none());
    until(&mut h, |a| a.film.as_ref().is_some_and(|f| f.subtitles.is_none()) && a.film_subtitles().is_none());
    let kept = h.program().recent.languages.get(&videos.join("clip.mp4").to_string_lossy().into_owned()).cloned();
    assert_eq!(kept.and_then(|l| l.subtitles), Some(StateSubtitles::Off), "kept for the film");

    // The desktop's media controls play and pause it, and stop it.
    h.send(Msg::Mpris(aizome_mpris::Action::Toggle));
    until(&mut h, |a| a.film.as_ref().is_some_and(|f| f.playing));
    h.send(Msg::Mpris(aizome_mpris::Action::Stop));
    assert_eq!(h.program().screen, Screen::Home);

    // Next time, it plays without them.
    open_film(&mut h);
    until(&mut h, |a| a.film.as_ref().is_some_and(|f| f.playing && f.subtitles.is_none()));
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1200) {
        h.settle(Duration::from_millis(20), |_| false);
        assert!(h.program().film_subtitles().is_none(), "subtitles off, as chosen");
    }
}

#[test]
fn a_page_always_has_a_way_home_by_keyboard_and_mouse() {
    // A stand-in for the browser: it stays until it's stopped and shows no
    // window (there's no compositor here), so the page showing is told by
    // hand, as the compositor's word on the Home key is.
    let browser = std::env::temp_dir().join(format!("chanoma-test-page-{}", std::process::id())).join("browser");
    let (dirs, cores) = household("page", |s| {
        s.services = vec!["pbs-kids".into()];
        s.browser = Some(browser.display().to_string());
    });
    std::fs::write(&browser, "#!/bin/sh\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&browser, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    let page_menu = |h: &Harness<Chanoma>| matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Page, .. }));
    // PBS KIDS, the first tile.
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Watching);
    shot(&mut h, "10-page-opening");

    // Chanoma's own screen, till the page shows: its Home hint is a
    // button for the mouse, and Escape is the menu too.
    let w = h.window();
    h.click(w, Point::new(110.0, 1016.0));
    assert!(page_menu(&h), "the Home hint, clicked");
    h.click(w, Point::new(40.0, 540.0));
    assert!(h.program().modal.is_none(), "a click beside the menu");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(page_menu(&h), "Escape");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().modal.is_none());

    // The page shows: how to come back is said over it, and a strip along
    // the top waits for the pointer.
    h.send(Msg::Browser(aizome_chanoma::browser::Event::Shown(7)));
    assert_eq!(h.program().toast.as_ref().map(|t| t.0.as_str()), Some("Press Home on the controller to come back"));
    let edge = h.program().edge.expect("the strip along the top");
    h.resize(edge, Size::new(1920.0, 2.0));
    let over = |h: &mut Harness<Chanoma>| {
        let hud = h.program().hud.expect("the overlay");
        h.resize(hud, Size::new(1920.0, 1080.0));
        hud
    };
    // The pointer pushed up to the top brings the bar down; its Home is
    // the menu, which has the keyboard.
    h.move_to(edge, Point::new(900.0, 1.0));
    assert!(h.program().bar_until.is_some(), "the bar");
    let hud = over(&mut h);
    shot_of(&mut h, hud, "11-page-bar");
    h.click(hud, Point::new(200.0, 76.0));
    assert!(page_menu(&h), "the bar's Home");
    assert!(h.program().hud_menu, "the menu has the keys");
    let hud = over(&mut h);
    h.named(hud, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().modal.is_none(), "Escape in the menu over the page");
    assert!(!h.program().hud_menu, "and the page has the keys again");
    // Down from the bar to the page, the bar goes.
    h.move_to(edge, Point::new(900.0, 1.0));
    let hud = over(&mut h);
    h.move_to(hud, Point::new(900.0, 600.0));
    assert!(h.program().bar_until.is_none(), "the pointer back on the page");

    // Home on the keyboard over the page, which the compositor keeps for
    // Chanoma: the menu; Down and Return go home.
    h.send(Msg::Browser(aizome_chanoma::browser::Event::Home));
    assert!(page_menu(&h), "Home on the keyboard");
    let hud = over(&mut h);
    shot_of(&mut h, hud, "12-page-menu");
    h.named(hud, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(hud, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().screen, Screen::Home);
    let a = h.program();
    assert!(a.page.is_none() && a.edge.is_none() && a.hud.is_none(), "the page and what was over it, gone");
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

#[test]
fn a_games_folder_is_chosen_with_the_controller_alone() {
    let (dirs, cores) = household("picker", |_| {});
    let root = cores.parent().unwrap().to_path_buf();
    let home = root.join("home");
    std::fs::create_dir_all(home.join("ROMs/Super Nintendo")).unwrap();
    std::fs::write(home.join("ROMs/Super Nintendo/Pocket Quest (USA).sfc"), b"CHANOMA test game").unwrap();
    std::fs::create_dir_all(home.join("ROMs/.cache")).unwrap();
    std::fs::create_dir_all(home.join("Videos")).unwrap();
    let stick = root.join("media/STICK");
    std::fs::create_dir_all(stick.join("snes")).unwrap();
    std::fs::write(stick.join("snes/Stick Game.sfc"), b"CHANOMA test game").unwrap();
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));

    // In: the grown-ups' door, the PIN, then Settings.
    let shelves = h.program().home_view().shelves.len();
    for _ in 1..shelves {
        input(&mut h, Input::Dir(Dir::Down));
    }
    input(&mut h, Input::Ok);
    type_pin(&mut h, PIN);
    until(&mut h, |a| a.is_grownup());
    let f = h.program().focus;
    assert_eq!(h.program().home_view().shelves[f.row].tiles[f.col].label, "Settings");
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::GrownUps);
    // Games, into its rows, down to adding a folder.
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Right));
    let focused = |a: &Chanoma| a.grownups_page().rows[a.grownups.row].label.clone();
    for _ in 0..4 {
        if focused(h.program()) == "Add a games folder…" {
            break;
        }
        input(&mut h, Input::Dir(Dir::Down));
    }
    assert_eq!(focused(h.program()), "Add a games folder…");
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Folders);
    let read = |a: &Chanoma| a.picker.as_ref().is_some_and(|p| p.listing.is_some() && p.found.is_some());
    until(&mut h, read);

    // Home: the places, and its folders with the first one ready.
    let view = h.program().picker_view();
    let places: Vec<&str> = view.places.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(places, ["Home", "ROMs", "Videos", "STICK", "This computer"]);
    assert_eq!(view.crumbs, ["Home"]);
    let aizome_chanoma::ui::picker::Body::Folders(rows) = &view.body else { panic!("{:?}", view.body) };
    assert_eq!(rows.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["ROMs", "Videos"]);
    assert_eq!(rows[0].1, "1 folder");
    assert_eq!(h.program().focus, aizome_chanoma::ui::Focus { row: 1, col: 1 });
    shot(&mut h, "14-folders-home");

    // Into ROMs: what Chanoma would find there, counted as it reads it.
    input(&mut h, Input::Ok);
    until(&mut h, read);
    let view = h.program().picker_view();
    assert_eq!(view.crumbs, ["Home", "ROMs"]);
    assert_eq!(view.found, Some(("1 game for 1 system".to_owned(), true)));
    let aizome_chanoma::ui::picker::Body::Folders(rows) = &view.body else { panic!("{:?}", view.body) };
    assert_eq!(rows, &[("Super Nintendo".to_owned(), "1 game".to_owned())], "the hidden folder isn't shown");
    shot(&mut h, "15-folders-roms");

    // Back goes up, the focus on the folder left.
    input(&mut h, Input::Back);
    until(&mut h, read);
    let p = h.program().picker.as_ref().unwrap();
    assert_eq!((p.dir.clone(), p.row), (home.clone(), 1));

    // Left to the places; down to the stick, which shows as the focus
    // reaches it; and the top button uses it.
    input(&mut h, Input::Dir(Dir::Left));
    for _ in 0..3 {
        input(&mut h, Input::Dir(Dir::Down));
    }
    until(&mut h, read);
    let p = h.program().picker.as_ref().unwrap();
    assert_eq!(p.dir, stick);
    assert_eq!(h.program().picker_view().found, Some(("1 game for 1 system".to_owned(), true)));
    press(&mut h, Button::North);
    assert_eq!(h.program().screen, Screen::GrownUps);
    assert!(h.program().settings.game_folders.contains(&stick));
    assert_eq!(focused(h.program()), "Add a games folder…", "back where it was, not on the folder added");
    until(&mut h, |a| a.library.games.iter().any(|g| g.title == "Stick Game"));

    // Opened again and put away: nothing changes.
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Folders);
    input(&mut h, Input::Dir(Dir::Left));
    input(&mut h, Input::Back);
    assert_eq!(h.program().screen, Screen::GrownUps);
    assert_eq!(h.program().settings.game_folders.len(), 2);
}
