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
use aizome_chanoma::devtools::{Field, Focused};
use aizome_chanoma::pad::{Button, Dir, Family, PadEvent, PadInfo};
use aizome_chanoma::retro::ffi::*;
use aizome_chanoma::rules::{PinHash, Settings};
use aizome_chanoma::store::Dirs;
use aizome_chanoma::ui::keyboard::{Key, Shift};
use aizome_chanoma::ui::pinpad::button_of;
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
/// Between `retro_init` and `retro_deinit`: a callback given outside that
/// is lost, as cores that keep their callbacks in what `retro_init` makes
/// (Mesen) lose them, or crash.
static INITED: AtomicBool = AtomicBool::new(false);
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
    if INITED.load(Ordering::SeqCst) {
        *VIDEO.lock().unwrap() = Some(cb);
    }
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
    INITED.store(true, Ordering::SeqCst);
    *VIDEO.lock().unwrap() = None;
    FRAMES.store(0, Ordering::SeqCst);
    X.store(0, Ordering::SeqCst);
}
unsafe extern "C" fn deinit() {
    INITED.store(false, Ordering::SeqCst);
}
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
        metadata: None,
        art_url: None,
        database: None,
        fullscreen: false,
        scan: true,
        core_override: Some(test_core()),
        signals: false,
        account: ("ann".into(), "Ann".into()),
        authenticator: std::sync::Arc::new(aizome_sys::pam::Fixed("hunter2".into())),
        h24: true,
        // Beside the cores: a home and a folder drives appear in, for the
        // folder picker.
        home: root.join("home"),
        mounts: vec![root.join("media")],
        mpris: false,
        drives: false,
        tv: false,
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
        buttons: aizome_chanoma::pad::Button::all_bits(),
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

fn press_on(h: &mut Harness<Chanoma>, id: u32, b: Button) {
    for pressed in [true, false] {
        h.send(Msg::Pad(PadEvent::Button { id, slot: id as usize - 1, button: b, pressed }));
    }
}

/// Press a PIN on the controller: each digit is the legend's button in
/// that place (1 is Up).
fn type_pin(h: &mut Harness<Chanoma>, pin: &str) {
    for d in pin.bytes() {
        press(h, button_of(d - b'0').expect("a digit"));
    }
}

/// The presses typed on the PIN pad so far.
fn typed(h: &Harness<Chanoma>) -> usize {
    match &h.program().modal {
        Some(Modal::Pin { pin, .. }) => pin.typed.len(),
        _ => panic!("the pad isn't there"),
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
    // Five presses of the legend's buttons, a slip taken back with Delete
    // (B), then Done (A).
    type_pin(&mut h, "1357");
    press(&mut h, Button::Up);
    assert_eq!(typed(&h), 5);
    press(&mut h, Button::East);
    assert_eq!(typed(&h), 4);
    shot(&mut h, "02-choose-pin");
    // The stick, Start and Select aren't a PIN's: nothing's typed.
    h.send(Msg::Pad(PadEvent::Nav { id: 1, slot: 0, dir: Dir::Left, repeat: false }));
    for b in [Button::Start, Button::Select] {
        press(&mut h, b);
    }
    assert_eq!(typed(&h), 4);
    // The right trigger is 0.
    press(&mut h, Button::R2);
    assert_eq!(typed(&h), 5);
    press(&mut h, Button::South);
    // Again on the keyboard (the arrows, Page Up and Down, Tab, Space),
    // but another PIN: they don't match.
    for i in [Input::Dir(Dir::Up), Input::Prev, Input::Next, Input::Alt, Input::Space] {
        input(&mut h, i);
    }
    input(&mut h, Input::Ok);
    let Some(Modal::Pin { pin, .. }) = &h.program().modal else { panic!("the pad went") };
    assert_eq!(pin.error.as_deref(), Some("They didn't match. Try again"));
    type_pin(&mut h, "13570");
    press(&mut h, Button::South);
    type_pin(&mut h, "13570");
    press(&mut h, Button::South);
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
    assert!(saved.setup_done && saved.pin.as_ref().is_some_and(|p| p.verify("13570")));
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
    shot(&mut h, "05-pin-keyboard");
    // Typed on the keyboard by the legend's numbers, 9 and 0 among them,
    // and Backspace takes them back.
    input(&mut h, Input::Digit(9));
    input(&mut h, Input::Digit(0));
    assert_eq!(typed(&h), 2);
    input(&mut h, Input::Erase);
    input(&mut h, Input::Erase);
    assert_eq!(typed(&h), 0);
    for d in PIN.bytes() {
        input(&mut h, Input::Digit(d - b'0'));
    }
    until(&mut h, |a| a.is_grownup());
    assert!(h.program().modal.is_none());
    let shelves = h.program().home_view().shelves;
    assert!(shelves[0].tiles.len() > 5, "every service shows to a grown-up");
    assert!(shelves[0].tiles.iter().any(|t| t.sub == "Grown-ups only"));
    shot(&mut h, "06-home-grownup");
}

#[test]
fn a_pin_is_the_same_presses_on_any_controller() {
    // The PIN was made on the Xbox controller (Down, Right, RB, X); a
    // Nintendo one, its yes and no the other way round, presses it too.
    let (dirs, cores) = household("pin-pads", |_| {});
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    let switch = PadInfo {
        id: 2,
        slot: 1,
        name: "Nintendo Switch Pro Controller".into(),
        family: Family::Nintendo,
        bluetooth: true,
        guid: String::new(),
        battery: None,
        source: aizome_chanoma::pad::mapping::Source::Database,
        rumble: false,
        buttons: aizome_chanoma::pad::Button::all_bits(),
    };
    h.send(Msg::Pad(PadEvent::Connected(switch)));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    // A slip: B, the bottom button, is no on a Nintendo controller, so it
    // takes the press back (A, the right one, would be Done).
    press_on(&mut h, 2, Button::Down);
    press_on(&mut h, 2, Button::Up);
    press_on(&mut h, 2, Button::South);
    assert_eq!(typed(&h), 1);
    assert_eq!(h.program().family, Family::Nintendo, "the legend shows the controller in hand");
    shot(&mut h, "05-pin-nintendo");
    for b in [Button::Right, Button::R1] {
        press_on(&mut h, 2, b);
    }
    // The Xbox controller's X is where the Nintendo one's Y is: the last.
    press_on(&mut h, 1, Button::West);
    until(&mut h, |a| a.is_grownup());
    assert!(h.program().modal.is_none());
}

#[test]
fn a_pad_without_triggers_presses_nine_and_zero_as_chords() {
    // Shaped as Nintendo's Super pad: no triggers, so 9 is Select with
    // the left shoulder and 0 Select with the right, and the legend says
    // so; its other buttons press as any controller's do.
    let (dirs, cores) = household("pin-snes", |s| {
        s.pin = Some(PinHash::new("9015", 10));
        s.pin_length = Some(4);
    });
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    let snes = [
        Button::Up,
        Button::Down,
        Button::Left,
        Button::Right,
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::L1,
        Button::R1,
        Button::Select,
        Button::Start,
    ];
    let pad = PadInfo {
        id: 3,
        slot: 2,
        name: "8BitDo SN30 Modkit".into(),
        family: Family::Nintendo,
        bluetooth: true,
        guid: String::new(),
        battery: None,
        source: aizome_chanoma::pad::mapping::Source::Database,
        rumble: false,
        buttons: snes.iter().fold(0, |b, x| b | x.bit()),
    };
    h.send(Msg::Pad(PadEvent::Connected(pad)));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    let held = |h: &mut Harness<Chanoma>, b: Button, pressed: bool| {
        h.send(Msg::Pad(PadEvent::Button { id: 3, slot: 2, button: b, pressed }));
    };
    // The legend's 9 and 0 are chords for this one.
    press_on(&mut h, 3, Button::Up);
    let Some(aizome_chanoma::ui::Overlay::Pin(p)) = h.program().overlay() else { panic!("no pad") };
    use aizome_chanoma::ui::pinpad::{Press, presses};
    assert_eq!(presses(p.have)[8], Some(Press::Chord(Button::Select, Button::L1)));
    press_on(&mut h, 3, Button::South);
    assert_eq!(typed(&h), 0, "B on a Nintendo pad takes the press back");
    held(&mut h, Button::Select, true);
    press_on(&mut h, 3, Button::L1);
    press_on(&mut h, 3, Button::R1);
    held(&mut h, Button::Select, false);
    assert_eq!(typed(&h), 2);
    shot(&mut h, "05-pin-chords");
    // Let go of Select, the shoulder is itself again: 1, then 5.
    press_on(&mut h, 3, Button::Up);
    press_on(&mut h, 3, Button::L1);
    until(&mut h, |a| a.is_grownup());
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
        type_pin(&mut h, "1111");
        until(&mut h, |a| matches!(&a.modal, Some(Modal::Pin { pin, .. }) if !pin.checking));
    }
    let now = h.program().now_secs();
    assert_eq!(h.program().lockout.wait(now), Some(30));
    shot(&mut h, "07-pin-locked");
    // While it waits, presses do nothing.
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
    // Kept to the service in kids' mode: a link elsewhere stays, said so.
    assert!(h.program().page.as_ref().is_some_and(|p| p.browser.is_kept()));
    h.send(Msg::Browser(aizome_chanoma::browser::Event::Outside("www.example.com".into())));
    assert_eq!(h.program().toast.as_ref().map(|t| t.0.as_str()), Some("That's outside PBS KIDS Video"));
    assert_eq!(h.program().screen, Screen::Watching);
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

    // Again from Keep playing, in a browser that can't be kept to the
    // service: it's ended, and the screen says why.
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Watching);
    h.send(Msg::Browser(aizome_chanoma::browser::Event::Unkept));
    assert!(h.program().page.is_none());
    assert!(matches!(&h.program().screen, Screen::Problem(t, _) if t == "PBS KIDS Video didn't open"));
}

fn hold(h: &mut Harness<Chanoma>, b: Button, pressed: bool) {
    h.send(Msg::Pad(PadEvent::Button { id: 1, slot: 0, button: b, pressed }));
}

fn typing(h: &Harness<Chanoma>) -> Option<&aizome_chanoma::app::typing::Typing> {
    h.program().page.as_ref().and_then(|p| p.typing.as_ref())
}

fn focused(h: &mut Harness<Chanoma>, f: Focused) {
    h.send(Msg::Browser(aizome_chanoma::browser::Event::Focused(f)));
}

fn search_box(text: &str) -> Focused {
    Focused::Field(Field {
        label: "Search".into(),
        text: text.into(),
        length: text.chars().count(),
        caret: text.chars().count(),
        secret: false,
    })
}

#[test]
fn a_box_chosen_on_a_page_brings_chanomas_keyboard_but_not_on_youtube() {
    let browser = std::env::temp_dir().join(format!("chanoma-test-typing-{}", std::process::id())).join("browser");
    let (dirs, cores) = household("typing", |s| {
        s.services = vec!["pbs-kids".into(), "youtube".into()];
        s.browser = Some(browser.display().to_string());
    });
    std::fs::write(&browser, "#!/bin/sh\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&browser, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    input(&mut h, Input::Ok);
    h.send(Msg::Browser(aizome_chanoma::browser::Event::Shown(7)));
    assert!(h.program().page.as_ref().is_some_and(|p| p.keyboard));

    // A click (A) on the page lands on a search box, the page says: the
    // keyboard comes up, the sticks Chanoma's.
    press(&mut h, Button::South);
    focused(&mut h, search_box(""));
    assert!(typing(&h).is_some(), "the keyboard over the page");
    let hud = h.program().hud.expect("the overlay");
    h.resize(hud, Size::new(1920.0, 1080.0));
    assert!(!h.program().hud_menu, "the page keeps the keyboard's typing");
    // The arrows (or the stick) between the keys; LT for capitals.
    h.send(Msg::Pad(PadEvent::Nav { id: 1, slot: 0, dir: Dir::Right, repeat: false }));
    assert_eq!(typing(&h).unwrap().keys.key(), Key::Char('w'));
    press(&mut h, Button::L2);
    assert_eq!(typing(&h).unwrap().keys.shift, Shift::Once);
    press(&mut h, Button::South);
    assert_eq!(typing(&h).unwrap().keys.shift, Shift::Off, "a capital, then small letters");
    focused(&mut h, search_box("W"));
    assert_eq!(typing(&h).unwrap().keys.field.as_ref().map(|f| f.text.as_str()), Some("W"));
    shot_of(&mut h, hud, "13-page-keyboard");
    // A key clicked with a mouse types too.
    let k = typing(&h).unwrap().keys.clone();
    h.click(hud, k.rect(2, 0).center());
    assert_eq!(typing(&h).unwrap().keys.key(), Key::Char('a'));
    // B puts it away.
    press(&mut h, Button::East);
    assert!(typing(&h).is_none());

    // Select (alone) brings it up by hand; with no box yet it stays, till
    // a box has had the keyboard and lets it go (the search sent).
    press(&mut h, Button::Select);
    assert!(typing(&h).is_some_and(|t| t.by_hand));
    focused(&mut h, Focused::Elsewhere);
    assert!(typing(&h).is_some());
    focused(&mut h, search_box("cats"));
    focused(&mut h, Focused::Elsewhere);
    assert!(typing(&h).is_none(), "the box let it go");
    // Select and Start held together are Home: neither the keyboard nor
    // Enter.
    hold(&mut h, Button::Select, true);
    hold(&mut h, Button::Start, true);
    hold(&mut h, Button::Start, false);
    hold(&mut h, Button::Select, false);
    assert!(typing(&h).is_none());
    // Start alone over the keyboard is Enter, and it goes.
    press(&mut h, Button::Select);
    press(&mut h, Button::Start);
    assert!(typing(&h).is_none(), "Enter sends it");

    // Moved to a box with RB, as a keyboard's Tab: A brings the keyboard
    // rather than Return.
    press(&mut h, Button::R1);
    focused(&mut h, search_box(""));
    assert!(typing(&h).is_none());
    press(&mut h, Button::South);
    assert!(typing(&h).is_some(), "A on the box moved to");
    // Home: the page's menu, the keyboard put away.
    press(&mut h, Button::Guide);
    assert!(matches!(h.program().modal, Some(Modal::Menu { kind: MenuKind::Page, .. })));
    assert!(typing(&h).is_none());
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Home);

    // YouTube's television interface has a keyboard of its own: Chanoma's
    // doesn't come.
    let shelves = h.program().home_view().shelves;
    let (row, col) = shelves
        .iter()
        .enumerate()
        .find_map(|(r, s)| s.tiles.iter().position(|t| t.label == "YouTube").map(|c| (r, c)))
        .expect("YouTube on Home");
    h.send(Msg::Click(aizome_chanoma::ui::Target::Tile(row, col)));
    assert_eq!(h.program().page.as_ref().map(|p| p.name.as_str()), Some("YouTube"));
    h.send(Msg::Browser(aizome_chanoma::browser::Event::Shown(8)));
    press(&mut h, Button::South);
    focused(&mut h, search_box(""));
    press(&mut h, Button::Select);
    assert!(typing(&h).is_none());
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

/// The settings' row labelled `label`, moved to with the arrows.
fn to_row(h: &mut Harness<Chanoma>, label: &str) {
    let page = h.program().grownups_page();
    let Some(target) = page.rows.iter().position(|r| !r.heading && r.label == label) else {
        panic!("no row {label:?} in {:?}", page.rows.iter().map(|r| &r.label).collect::<Vec<_>>());
    };
    for _ in 0..60 {
        let row = h.program().grownups.row;
        if row == target {
            return;
        }
        input(h, Input::Dir(if row < target { Dir::Down } else { Dir::Up }));
    }
    panic!("couldn't reach {label:?}");
}

/// The games that show, by key.
fn shown(h: &Harness<Chanoma>) -> Vec<String> {
    let mut v: Vec<String> = h.program().shown_games().map(|g| g.key.clone()).collect();
    v.sort();
    v
}

#[test]
fn a_big_collection_is_managed_a_system_at_a_time_and_found_on_a_drive() {
    let (dirs, cores) = household("collection", |_| {});
    let root = cores.parent().unwrap().to_path_buf();
    // A set as No-Intro names it: regions, a revision, a beta, a BIOS.
    for f in [
        "snes/Kirby (USA).sfc",
        "snes/Kirby (Europe).sfc",
        "snes/Kirby (Japan) (Rev 1).sfc",
        "snes/Kirby (USA) (Beta).sfc",
        "snes/[BIOS] Super Famicom (Japan).sfc",
        "gb/Tetris (World).gb",
    ] {
        std::fs::write(root.join("Games").join(f).tap_parent(), b"x").unwrap();
    }
    std::fs::write(cores.join("gambatte_libretro.so"), b"").unwrap();
    // A drive plugged in, with a collection on it and a backup beside it.
    let stick = root.join("media/Stick");
    for f in ["roms/snes/Zelda (USA).sfc", "roms/snes/Zelda (Europe).sfc", "backup/firmware.bin"] {
        std::fs::write(stick.join(f).tap_parent(), b"x").unwrap();
    }
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    // One of each game, no beta, no BIOS.
    assert_eq!(shown(&h), ["gb/gb/Tetris (World).gb", "snes/snes/Kirby (USA).sfc", "snes/snes/Test Quest (USA).sfc"]);

    // In: the PIN, Settings, Games.
    let shelves = h.program().home_view().shelves.len();
    for _ in 1..shelves {
        input(&mut h, Input::Dir(Dir::Down));
    }
    input(&mut h, Input::Ok);
    type_pin(&mut h, PIN);
    until(&mut h, |a| a.is_grownup());
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::GrownUps);
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Right));

    // A whole system off, with one switch.
    to_row(&mut h, "Systems");
    input(&mut h, Input::Ok);
    assert_eq!(h.program().grownups_page().title, "Games › Systems");
    to_row(&mut h, "Game Boy");
    input(&mut h, Input::Ok);
    assert!(!shown(&h).iter().any(|k| k.starts_with("gb/")));
    shot(&mut h, "16-systems");
    input(&mut h, Input::Back);
    assert_eq!(h.program().grownups_page().rows[h.program().grownups.row].label, "Systems");

    // Every region, then the beta too; then one of each again.
    to_row(&mut h, "One of each game");
    input(&mut h, Input::Ok);
    assert_eq!(shown(&h).iter().filter(|k| k.contains("Kirby")).count(), 3);
    to_row(&mut h, "Hide demos, betas and prototypes");
    input(&mut h, Input::Ok);
    assert_eq!(shown(&h).iter().filter(|k| k.contains("Kirby")).count(), 4);
    assert!(!shown(&h).iter().any(|k| k.contains("BIOS")), "a BIOS is never a game");
    input(&mut h, Input::Ok);
    to_row(&mut h, "One of each game");
    input(&mut h, Input::Ok);

    // A system's games for kids: all, none, and down the list a letter at
    // a time.
    to_row(&mut h, "Super Nintendo");
    input(&mut h, Input::Ok);
    to_row(&mut h, "Let kids play all 2");
    input(&mut h, Input::Ok);
    let allowed = |h: &Harness<Chanoma>, key: &str| h.program().settings.game_allowed(key);
    assert!(allowed(&h, "snes/snes/Kirby (USA).sfc") && allowed(&h, "snes/snes/Test Quest (USA).sfc"));
    shot(&mut h, "17-system-games");
    to_row(&mut h, "Keep all 2 from kids");
    input(&mut h, Input::Ok);
    assert!(!allowed(&h, "snes/snes/Kirby (USA).sfc") && !allowed(&h, "snes/snes/Test Quest (USA).sfc"));
    to_row(&mut h, "Kirby");
    input(&mut h, Input::Next);
    assert_eq!(h.program().grownups_page().rows[h.program().grownups.row].label, "Test Quest");
    input(&mut h, Input::Prev);
    assert_eq!(h.program().grownups_page().rows[h.program().grownups.row].label, "Kirby");
    input(&mut h, Input::Back);

    // Over to Folders; every drive searched for games: its SNES folder,
    // counted where it's named, and not the backup's stray .bin; switched
    // on, its games join.
    to_row(&mut h, "Your folders");
    input(&mut h, Input::Ok);
    to_row(&mut h, "Search every drive for games…");
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.finding.as_ref().is_some_and(|f| !f.running));
    let page = h.program().grownups_page();
    assert_eq!(page.title, "Folders › Games on your drives");
    let stick_row = page.rows.iter().position(|r| r.heading && r.label == "Stick").expect("the drive");
    let snes = &page.rows[stick_row + 1];
    assert_eq!((snes.label.as_str(), snes.detail.as_str()), ("Super Nintendo", "2 games in roms › snes"));
    assert_eq!(page.rows.iter().filter(|r| !r.heading).count(), 1, "{:?}", page.rows);
    shot(&mut h, "18-found-on-drives");
    to_row(&mut h, "Super Nintendo");
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.shown_games().any(|g| g.title == "Zelda"));
    assert_eq!(h.program().shown_games().filter(|g| g.title == "Zelda").count(), 1, "one of each");
}

/// A file's folder made, for writing it.
trait TapParent {
    fn tap_parent(self) -> Self;
}

impl TapParent for PathBuf {
    fn tap_parent(self) -> PathBuf {
        std::fs::create_dir_all(self.parent().unwrap()).unwrap();
        self
    }
}

/// A zip of `files`, stored, as a ROM set's are (deflated ones are tested
/// beside the reader).
fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let (mut out, mut dir) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let crc = crc32fast::hash(data);
        let (n, len) = (name.len() as u16, data.len() as u32);
        let offset = out.len() as u32;
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend(crc.to_le_bytes());
        out.extend(len.to_le_bytes());
        out.extend(len.to_le_bytes());
        out.extend(n.to_le_bytes());
        out.extend([0, 0]);
        out.extend(name.as_bytes());
        out.extend(*data);
        dir.extend(0x0201_4b50u32.to_le_bytes());
        dir.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        dir.extend(crc.to_le_bytes());
        dir.extend(len.to_le_bytes());
        dir.extend(len.to_le_bytes());
        dir.extend(n.to_le_bytes());
        dir.extend([0; 12]);
        dir.extend(offset.to_le_bytes());
        dir.extend(name.as_bytes());
    }
    let (at, count) = (out.len() as u32, files.len() as u16);
    out.extend(&dir);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0; 4]);
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend((dir.len() as u32).to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend([0, 0]);
    out
}

/// A RetroArch database of games: each a name, a year, a genre, how many
/// play, and a CRC.
fn database(games: &[(&str, u16, &str, u8, u32)]) -> Vec<u8> {
    let mut out = b"RARCHDB\0".to_vec();
    out.extend([0; 8]);
    let text = |out: &mut Vec<u8>, s: &[u8], tag: u8| {
        out.extend([tag, s.len() as u8]);
        out.extend(s);
    };
    for (name, year, genre, users, crc) in games {
        out.push(0x85);
        text(&mut out, b"name", 0xd9);
        text(&mut out, name.as_bytes(), 0xd9);
        text(&mut out, b"releaseyear", 0xd9);
        out.push(0xcd);
        out.extend(year.to_be_bytes());
        text(&mut out, b"genre", 0xd9);
        text(&mut out, genre.as_bytes(), 0xd9);
        text(&mut out, b"users", 0xd9);
        out.push(*users);
        text(&mut out, b"crc", 0xd9);
        text(&mut out, &crc.to_be_bytes(), 0xc4);
    }
    out.push(0xc0);
    out
}

#[test]
fn games_say_what_the_databases_know_and_two_controllers_bring_their_games() {
    let _core = core_lock();
    // Beside Test Quest: a zipped game, as sets come, and a file renamed
    // out of the set's naming, found by its CRC.
    let (dirs, cores) = household("known", |s| {
        s.games_allowed.insert("snes/snes/Contra III - The Alien Wars (USA).zip".into());
        s.games_allowed.insert("snes/snes/mystery.sfc".into());
    });
    let root = cores.parent().unwrap().to_path_buf();
    let snes = root.join("Games/snes");
    let contra: &[u8] = b"CHANOMA contra";
    std::fs::write(
        snes.join("Contra III - The Alien Wars (USA).zip"),
        zip_of(&[("readme.txt", b"hi"), ("Contra III - The Alien Wars (USA).sfc", contra)]),
    )
    .unwrap();
    let mario: &[u8] = b"CHANOMA mario";
    std::fs::write(snes.join("mystery.sfc"), mario).unwrap();
    let metadata = root.join("metadata");
    std::fs::create_dir_all(&metadata).unwrap();
    std::fs::write(
        metadata.join("Nintendo - Super Nintendo Entertainment System.rdb"),
        database(&[
            ("Test Quest (USA)", 1994, "Action", 1, 1),
            ("Contra III - The Alien Wars (USA)", 1992, "Shooter", 2, crc32fast::hash(contra)),
            ("Super Mario World (USA)", 1990, "Platform", 2, crc32fast::hash(mario)),
        ]),
    )
    .unwrap();
    let mut config = config(dirs.clone(), cores, ManualClock::at(2026, 11, 2, 17, 0));
    config.metadata = Some(metadata);
    let mut h = harness(config);
    let info = |h: &Harness<Chanoma>, title: &str| {
        let g = h.program().library.games.iter().find(|g| g.title == title).unwrap_or_else(|| panic!("{title}"));
        g.info.as_ref().map(aizome_chanoma::metadata::Info::summary).unwrap_or_default()
    };
    // By name, zipped or not; renamed, by what's in it.
    assert_eq!(info(&h, "Test Quest"), "1994 · Action · 1 player");
    assert_eq!(info(&h, "Contra III - The Alien Wars"), "1992 · Shooter · 2 players");
    assert_eq!(info(&h, "mystery"), "1990 · Platform · 2 players");
    let titles = |h: &Harness<Chanoma>| -> Vec<String> {
        h.program().home_view().shelves.into_iter().map(|s| s.title).collect::<Vec<_>>()
    };
    assert!(!titles(&h).iter().any(|t| t.starts_with("For ")), "{:?}", titles(&h));
    // On Games, a second controller: the games for two come before it,
    // and the focus stays on Games.
    input(&mut h, Input::Dir(Dir::Down));
    let on = |h: &Harness<Chanoma>| titles(h)[h.program().focus.row].clone();
    assert_eq!(on(&h), "Games");
    let second = PadInfo {
        id: 2,
        slot: 1,
        name: "Pro Controller".into(),
        family: Family::Nintendo,
        bluetooth: true,
        guid: String::new(),
        battery: Some((60, false)),
        source: aizome_chanoma::pad::mapping::Source::Database,
        rumble: true,
        buttons: aizome_chanoma::pad::Button::all_bits(),
    };
    h.send(Msg::Pad(PadEvent::Connected(second)));
    assert_eq!(on(&h), "Games", "{:?}", titles(&h));
    let home = h.program().home_view();
    let together = home.shelves.iter().find(|s| s.title == "For 2 players").expect("the games for two");
    let names: Vec<&str> = together.tiles.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(names, ["Contra III - The Alien Wars", "mystery"]);
    assert_eq!(together.tiles[0].more, "1992 · Shooter · 2 players");
    input(&mut h, Input::Dir(Dir::Up));
    shot(&mut h, "19-for-two-players");
    // The zipped game plays: unpacked for the core, which gets its bytes.
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.screen == Screen::Playing);
    let start = Instant::now();
    while !LOADED.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10), "the zipped game didn't load");
        std::thread::sleep(Duration::from_millis(10));
    }
    let unpacked = dirs.unzipped().join(format!("{:08x}", crc32fast::hash(contra)));
    assert_eq!(std::fs::read(unpacked.join("Contra III - The Alien Wars (USA).sfc")).unwrap(), contra);
    press(&mut h, Button::Guide);
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Home);
    // The second controller gone: so is the shelf, and the focus stays.
    h.send(Msg::Pad(PadEvent::Disconnected { id: 2, slot: 1 }));
    assert!(!titles(&h).iter().any(|t| t.starts_with("For ")), "{:?}", titles(&h));
}

#[test]
fn a_folder_is_added_with_the_controller_alone() {
    use aizome_chanoma::ui::picker::{Body, Choice};
    let (dirs, cores) = household("picker", |_| {});
    let root = cores.parent().unwrap().to_path_buf();
    let home = root.join("home");
    std::fs::create_dir_all(home.join("ROMs/Super Nintendo")).unwrap();
    std::fs::write(home.join("ROMs/Super Nintendo/Pocket Quest (USA).sfc"), b"CHANOMA test game").unwrap();
    std::fs::create_dir_all(home.join("ROMs/.cache")).unwrap();
    std::fs::create_dir_all(home.join("Videos")).unwrap();
    // A drive with a collection (a game's clip beside it, and a film) and
    // a backup.
    let stick = root.join("media/STICK");
    for f in [
        "roms/snes/Stick Game.sfc",
        "roms/snes/videos/Stick Game.mp4",
        "roms/gba/Pocket.gba",
        "roms/Holiday.mp4",
        "backup/old/notes.txt",
    ] {
        std::fs::write(stick.join(f).tap_parent(), b"CHANOMA test game").unwrap();
    }
    // Its pictures, as Batocera's scraper keeps them and its list says.
    image::RgbaImage::new(2, 2).save(stick.join("roms/snes/images/Stick Game-thumb.png").tap_parent()).unwrap();
    std::fs::write(
        stick.join("roms/snes/gamelist.xml"),
        "<gameList><game><path>./Stick Game.sfc</path><thumbnail>./images/Stick Game-thumb.png</thumbnail></game></gameList>",
    )
    .unwrap();
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
    // Folders, into its rows: the household's folder, then adding one.
    for _ in 0..4 {
        input(&mut h, Input::Dir(Dir::Down));
    }
    input(&mut h, Input::Dir(Dir::Right));
    let focused = |a: &Chanoma| a.grownups_page().rows[a.grownups.row].label.clone();
    assert_eq!(focused(h.program()), "Games");
    to_row(&mut h, "Add a folder…");
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Folders);

    // It opens on what looks like a collection, found from two levels of
    // home and the drive: the stick's roms first (more systems).
    until(&mut h, |a| a.picker.as_ref().is_some_and(|p| p.suggested.is_some()));
    let view = h.program().picker_view();
    let places: Vec<&str> = view.places.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(places, ["Suggested", "Home", "ROMs", "Videos", "STICK", "This computer"]);
    let Body::Suggested(rows) = &view.body else { panic!("{:?}", view.body) };
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, w)| (n.as_str(), w.as_str())).collect();
    assert_eq!(
        rows,
        [
            ("roms", "On STICK · Folders for 2 systems: Game Boy Advance, Super NES · 1 video"),
            ("ROMs", "In your home folder · A folder for Super NES"),
        ]
    );
    assert_eq!(h.program().focus, aizome_chanoma::ui::Focus { row: 0, col: 1 });
    shot(&mut h, "14-add-a-folder");

    // One press adds it, back where it was opened; then it's looked
    // through: its games and its film, not its game's clip.
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::GrownUps);
    assert!(h.program().settings.folders.contains(&stick.join("roms")));
    assert_eq!(focused(h.program()), "Add a folder…", "back where it was, not on the folder added");
    until(&mut h, |a| a.library.held.iter().any(|x| x.folder == stick.join("roms") && x.films == 1));
    let stick_game = h.program().library.games.iter().find(|g| g.title == "Stick Game").cloned().expect("its game");
    assert!(
        stick_game.art.as_ref().is_some_and(|a| a.ends_with("images/Stick Game-thumb.png")),
        "{:?}",
        stick_game.art
    );
    assert!(!h.program().library.shelves.iter().any(|s| s.title == "Stick Game"), "a game's clip isn't a film");
    let page = h.program().grownups_page();
    let row = page.rows.iter().find(|r| r.label == "roms").expect("the folder's row");
    assert_eq!(row.detail, "On STICK · 1 game for 1 system, and 1 without a game player here · 1 film");
    shot(&mut h, "15-folders");

    // Again: the stick's roms isn't suggested now. Over to the places,
    // home, and into ROMs: what's in each folder, from one listing.
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.picker.as_ref().is_some_and(|p| p.suggested.is_some()));
    let Body::Suggested(rows) = h.program().picker_view().body else { panic!() };
    assert_eq!(rows.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["ROMs"]);
    let read = |a: &Chanoma| a.picker.as_ref().is_some_and(|p| p.listing.is_some() && p.inside.is_some());
    input(&mut h, Input::Dir(Dir::Left));
    input(&mut h, Input::Dir(Dir::Down));
    until(&mut h, read);
    let view = h.program().picker_view();
    assert_eq!(view.crumbs, ["Home"]);
    let Body::Folders(rows) = &view.body else { panic!("{:?}", view.body) };
    assert_eq!(rows.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["ROMs", "Videos"]);
    assert_eq!(rows[0].1, "A folder for Super NES");
    input(&mut h, Input::Dir(Dir::Right));
    assert_eq!(h.program().focus, aizome_chanoma::ui::Focus { row: 1, col: 1 });
    input(&mut h, Input::Ok);
    until(&mut h, read);
    let view = h.program().picker_view();
    assert_eq!(view.crumbs, ["Home", "ROMs"]);
    assert_eq!(
        (view.choice.clone(), view.found.clone()),
        (Choice::Add, Some(("A folder for Super NES".to_owned(), true)))
    );
    let Body::Folders(rows) = &view.body else { panic!("{:?}", view.body) };
    assert_eq!(rows, &[("Super Nintendo".to_owned(), "1 game".to_owned())], "the hidden folder isn't shown");
    shot(&mut h, "14b-add-a-folder-browsing");

    // Back goes up, the focus on the folder left.
    input(&mut h, Input::Back);
    until(&mut h, read);
    let p = h.program().picker.as_ref().unwrap();
    assert_eq!((p.dir.clone(), p.row), (home.clone(), 1));

    // The stick: its roms is one of the folders, and its snes part of it,
    // so neither is added again.
    input(&mut h, Input::Dir(Dir::Left));
    for _ in 0..3 {
        input(&mut h, Input::Dir(Dir::Down));
    }
    until(&mut h, read);
    let view = h.program().picker_view();
    assert_eq!(view.crumbs, ["STICK"]);
    let Body::Folders(rows) = &view.body else { panic!("{:?}", view.body) };
    assert!(rows[1].1.starts_with("Added · "), "{rows:?}");
    input(&mut h, Input::Dir(Dir::Right));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    until(&mut h, read);
    assert_eq!(h.program().picker_view().choice, Choice::Already);
    press(&mut h, Button::North);
    assert_eq!(h.program().screen, Screen::Folders, "nothing to add: the picker stays");
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    until(&mut h, read);
    assert_eq!(h.program().picker_view().crumbs, ["STICK", "roms", "snes"]);
    assert_eq!(h.program().picker_view().choice, Choice::Inside("roms".into()));

    // Put away: nothing more added.
    input(&mut h, Input::Dir(Dir::Left));
    input(&mut h, Input::Back);
    assert_eq!(h.program().screen, Screen::GrownUps);
    assert_eq!(h.program().settings.folders.len(), 2);

    // A folder's page: removing it asks first, keeping it ready to press.
    to_row(&mut h, "roms");
    input(&mut h, Input::Ok);
    assert_eq!(h.program().grownups_page().title, "Folders › roms");
    to_row(&mut h, "Remove roms");
    input(&mut h, Input::Ok);
    let Some(Modal::Menu { menu, kind: MenuKind::Remove(_) }) = &h.program().modal else { panic!("no question") };
    assert_eq!(
        (menu.title.as_str(), menu.items.clone()),
        ("Remove roms?", vec!["Keep it".to_owned(), "Remove it from Chanoma".to_owned()])
    );
    shot(&mut h, "15b-remove-a-folder");
    input(&mut h, Input::Ok);
    assert!(h.program().modal.is_none());
    assert_eq!(h.program().settings.folders.len(), 2, "kept");
    input(&mut h, Input::Ok);
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    assert_eq!(h.program().settings.folders.len(), 1, "removed");
    assert!(!h.program().settings.folders.contains(&stick.join("roms")));
}

/// From Home, the grown-ups' door and the PIN, to Settings.
fn into_settings(h: &mut Harness<Chanoma>) {
    let shelves = h.program().home_view().shelves.len();
    for _ in 1..shelves {
        input(h, Input::Dir(Dir::Down));
    }
    input(h, Input::Ok);
    type_pin(h, PIN);
    until(h, |a| a.is_grownup());
    input(h, Input::Ok);
    assert_eq!(h.program().screen, Screen::GrownUps);
}

#[test]
fn a_forgotten_pin_is_chosen_again_with_the_accounts_password() {
    let (dirs, cores) = household("forgot", |_| {});
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    // The grown-ups' door: the PIN pad offers *Forgot it?* on Start.
    let shelves = h.program().home_view().shelves.len();
    for _ in 1..shelves {
        input(&mut h, Input::Dir(Dir::Down));
    }
    input(&mut h, Input::Ok);
    assert!(matches!(h.program().modal, Some(Modal::Pin { .. })));
    press(&mut h, Button::Start);
    assert_eq!(h.program().screen, Screen::Reset);
    let view = h.program().reset_view();
    assert_eq!(view.title, "Forgot the PIN?");
    assert!(view.lines[0].contains("Ann (ann)"), "{:?}", view.lines);
    let k = h.program().reset_keyboard().expect("the keyboard");
    assert!(k.field.as_ref().is_some_and(|f| f.secret), "dots, not letters");

    // A wrong password: said so, and counted with wrong PINs.
    for c in "nope".chars() {
        input(&mut h, Input::Typed(c));
    }
    press(&mut h, Button::Start);
    until(&mut h, |a| a.reset.as_ref().is_some_and(|r| !r.checking));
    assert_eq!(h.program().reset_view().error.as_deref(), Some("That's not the password"));
    assert_eq!(h.program().reset.as_ref().unwrap().password, "", "not kept");
    // Typed on a real keyboard, a slip taken back with X.
    input(&mut h, Input::Typed('h'));
    input(&mut h, Input::Typed('x'));
    press(&mut h, Button::West);
    for c in "unter2".chars() {
        input(&mut h, Input::Typed(c));
    }
    assert_eq!(h.program().reset.as_ref().unwrap().password, "hunter2");
    shot(&mut h, "22-forgot-the-pin");
    press(&mut h, Button::Start);

    // Right: grown-up mode, and a new PIN to choose.
    until(&mut h, |a| matches!(&a.modal, Some(Modal::Pin { pin, .. }) if pin.new));
    assert!(h.program().is_grownup());
    for _ in 0..2 {
        type_pin(&mut h, "1357");
        press(&mut h, Button::South);
    }
    until(&mut h, |a| a.settings.pin.as_ref().is_some_and(|p| p.verify("1357")));
    assert_eq!(h.program().screen, Screen::Home);
}

#[test]
fn films_and_shows_are_kept_from_kids_one_by_one() {
    let videos = temp("films-kids-videos");
    for f in ["Bluey/Bluey S01E01.mkv", "Bluey/Bluey S01E02.mkv", "Totoro (1988).mp4"] {
        std::fs::write(videos.join(f).tap_parent(), b"x").unwrap();
    }
    let (dirs, cores) = household("films-kids", |s| s.video_folders = vec![videos.clone()]);
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    let titles = |a: &Chanoma| a.watchable().iter().map(|i| a.library.shelves[*i].title.clone()).collect::<Vec<_>>();
    assert_eq!(titles(h.program()), ["Bluey", "Totoro"], "new ones show at once");

    // Films and shows: Totoro kept from kids.
    into_settings(&mut h);
    for _ in 0..3 {
        input(&mut h, Input::Dir(Dir::Down));
    }
    input(&mut h, Input::Dir(Dir::Right));
    to_row(&mut h, "Totoro");
    input(&mut h, Input::Ok);
    let page = h.program().grownups_page();
    let row = page.rows.iter().find(|r| r.label == "Totoro").unwrap();
    assert_eq!(
        (row.value.clone(), row.detail.as_str()),
        (aizome_chanoma::ui::settings::Value::Switch(false), "A film")
    );
    let bluey = page.rows.iter().find(|r| r.label == "Bluey").unwrap();
    assert_eq!(bluey.detail, "A show of 2 episodes");
    // New ones wait for a grown-up from now on.
    to_row(&mut h, "New films and shows");
    input(&mut h, Input::Dir(Dir::Right));
    assert_eq!(h.program().settings.new_films, aizome_chanoma::rules::NewGames::Ask);
    std::fs::write(videos.join("Up (2009).mp4"), b"x").unwrap();
    to_row(&mut h, "Look again");
    input(&mut h, Input::Ok);
    until(&mut h, |a| a.library.shelves.iter().any(|s| s.title == "Up"));
    let page = h.program().grownups_page();
    let up = page.rows.iter().find(|r| r.label == "Up").unwrap();
    assert_eq!(up.badge.as_deref(), Some("New"));
    shot(&mut h, "23-films-for-kids");

    // The kids: Bluey, and nothing else; on Home too.
    h.program_mut().grownup = None;
    assert_eq!(titles(h.program()), ["Bluey"]);
    let home = h.program().home_view();
    let films = home.shelves.iter().find(|s| s.title == "Films and shows").expect("the shelf");
    assert_eq!(films.tiles.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Bluey"]);
}

#[test]
fn search_finds_what_names_start_with_and_only_what_kids_may_open() {
    let (dirs, cores) = household("search", |_| {});
    let root = cores.parent().unwrap().to_path_buf();
    std::fs::write(root.join("Games/snes/Kirby Super Star (USA).sfc"), b"x").unwrap();
    let videos = root.join("Videos");
    std::fs::write(videos.join("Kirby Right Back at Ya (2001).mp4").tap_parent(), b"x").unwrap();
    let settings = dirs.settings();
    let mut s: Settings = aizome_chanoma::store::read(&settings);
    s.video_folders = vec![videos];
    aizome_chanoma::store::write(&settings, &s).unwrap();
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut h = harness(config(dirs, cores, clock));
    until(&mut h, |a| a.library.shelves.len() == 1);
    let hits = |a: &Chanoma| a.search_view().tiles.iter().map(|t| t.label.clone()).collect::<Vec<_>>();

    // A kid types on a keyboard at Home: search, with what's typed.
    input(&mut h, Input::Typed('k'));
    assert_eq!(h.program().screen, Screen::Search);
    input(&mut h, Input::Typed('i'));
    assert_eq!(hits(h.program()), ["Kirby Right Back at Ya"], "Kirby Super Star isn't theirs");
    // Back puts the keyboard away; Back again goes home.
    input(&mut h, Input::Back);
    assert!(h.program().search.as_ref().is_some_and(|s| s.keys.is_none()));
    input(&mut h, Input::Back);
    assert_eq!(h.program().screen, Screen::Home);

    // A grown-up, with the controller: Y opens it, the keyboard's keys
    // type (from its first letter, q), X takes one back.
    h.program_mut().grownup = Some(Instant::now());
    press(&mut h, Button::North);
    assert_eq!(h.program().screen, Screen::Search);
    press(&mut h, Button::South);
    assert_eq!(h.program().search.as_ref().unwrap().query, "q");
    press(&mut h, Button::West);
    for c in "super st".chars() {
        input(&mut h, Input::Typed(c));
    }
    assert_eq!(hits(h.program()), ["Kirby Super Star"]);
    input(&mut h, Input::Erase);
    for _ in 0.."super s".len() {
        input(&mut h, Input::Erase);
    }
    for c in "snes test".chars() {
        input(&mut h, Input::Typed(c));
    }
    assert_eq!(hits(h.program()), ["Test Quest"], "by the system's name too");
    shot(&mut h, "21-search");
    // Start shows the results alone; up from them, typing again.
    press(&mut h, Button::Start);
    assert!(h.program().search.as_ref().is_some_and(|s| s.keys.is_none()));
    shot(&mut h, "21b-search-results");
    input(&mut h, Input::Dir(Dir::Up));
    assert!(h.program().search.as_ref().is_some_and(|s| s.keys.is_some()));
}

#[test]
fn the_tv_accounts_kids_may_log_out_and_a_forgotten_pin_is_a_grown_ups_to_forget() {
    // The TV account's Chanoma: kids' mode as ever, but leaving is
    // logging out, which kids may do; and its own password can't reset
    // the PIN (it may be none, or the PIN a kid logs in with).
    let (dirs, cores) = household("tv", |_| {});
    let clock = ManualClock::at(2026, 10, 31, 17, 0);
    let mut cfg = config(dirs, cores, clock);
    cfg.tv = true;
    let mut h = harness(cfg);
    assert_eq!(h.program().screen, Screen::Home);
    assert!(!h.program().is_grownup());
    let more: Vec<String> =
        h.program().home_view().shelves.last().unwrap().tiles.iter().map(|t| t.label.clone()).collect();
    assert_eq!(more, ["Grown-ups", "Log Out"]);
    shot(&mut h, "30-tv-kids");
    // The grown-ups' PIN, forgotten: no sign-in here, but who can help.
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Dir(Dir::Down));
    input(&mut h, Input::Ok);
    assert!(matches!(h.program().modal, Some(Modal::Pin { .. })));
    input(&mut h, Input::Menu);
    assert!(h.program().reset.is_none(), "no signing in with the TV account's password");
    assert!(
        matches!(&h.program().screen, Screen::Problem(t, b) if t == "Forgot the PIN?" && b.contains("Users & Groups"))
    );
    input(&mut h, Input::Ok);
    assert_eq!(h.program().screen, Screen::Home);
    // Log Out: Chanoma ends, and with it the TV account's session.
    input(&mut h, Input::Dir(Dir::Right));
    input(&mut h, Input::Ok);
    assert!(h.platform.quit, "left");
}

fn outcome(name: &str, result: Result<&str, &str>) -> aizome_sys::disks::Outcome {
    aizome_sys::disks::Outcome {
        volume: aizome_sys::disks::Volume {
            block: format!("/org/freedesktop/UDisks2/block_devices/{name}"),
            name: name.into(),
            ..Default::default()
        },
        label: name.into(),
        uuid: format!("{name}-UUID"),
        result: result.map(Into::into).map_err(Into::into),
    }
}

fn toast(h: &Harness<Chanoma>) -> Option<&str> {
    h.program().toast.as_ref().map(|t| t.0.as_str())
}

#[test]
fn a_drive_that_wont_open_is_said_once_and_its_folder_comes_back_when_it_opens() {
    // Drives plugged in are opened as Chanoma starts and as they come
    // (UDisks; here what it answers). One that couldn't be opened is said
    // once: not again while it stays broken, nor at the next start. The
    // settings say which drive and why, and its folder's state, and try it
    // again; opened, its folder's games come.
    let (dirs, cores) = household("drives", |_| {});
    let stick = cores.parent().unwrap().join("media").join("MOVIES");
    let mut s: Settings = aizome_chanoma::store::read(&dirs.settings());
    s.folders.push(stick.clone());
    aizome_chanoma::store::write(&dirs.settings(), &s).unwrap();
    let start = |dirs: &Dirs| harness(config(dirs.clone(), cores.clone(), ManualClock::at(2026, 11, 5, 17, 0)));
    let mut h = start(&dirs);
    let broken = || Msg::Opened(vec![outcome("MOVIES", Err("Error mounting /dev/sdb1: wrong fs type"))]);
    h.send(broken());
    assert_eq!(toast(&h), Some("Couldn't open the drive MOVIES"));
    h.program_mut().toast = None;
    h.send(broken());
    assert_eq!(toast(&h), None, "said once");

    into_settings(&mut h);
    for _ in 0..4 {
        input(&mut h, Input::Dir(Dir::Down));
    }
    input(&mut h, Input::Dir(Dir::Right));
    let page = h.program().grownups_page();
    let folder = page
        .rows
        .iter()
        .find(|r| r.detail.contains("couldn't be opened") && !r.detail.contains("Try again"))
        .expect("the folder");
    assert!(folder.detail.ends_with("Its drive, MOVIES, couldn't be opened"), "{}", folder.detail);
    let drive = page.rows.iter().find(|r| r.detail.contains("Try again")).expect("the drive");
    assert_eq!(drive.detail, "Error mounting /dev/sdb1: wrong fs type · Try again");
    shot(&mut h, "31-drive-unopened");
    // Tried again (here UDisks isn't there to answer): the row goes until
    // it answers.
    let i = page.rows.iter().position(|r| r.detail.contains("Try again")).unwrap();
    while h.program().grownups.row < i {
        input(&mut h, Input::Dir(Dir::Down));
    }
    assert_eq!(h.program().grownups.row, i);
    input(&mut h, Input::Ok);
    assert!(h.program().drives.unopened.is_empty());

    // The next start: still broken, not said again.
    drop(h);
    let mut h = start(&dirs);
    h.send(broken());
    assert_eq!(toast(&h), None, "said once, across starts");
    assert_eq!(h.program().drives.unopened.len(), 1);

    // Opened: its folder is read and its game comes; a later failure is
    // said again.
    std::fs::create_dir_all(stick.join("snes")).unwrap();
    std::fs::write(stick.join("snes/Stick Quest (USA).sfc"), b"CHANOMA test game").unwrap();
    h.send(Msg::Opened(vec![outcome("MOVIES", Ok(stick.to_str().unwrap()))]));
    assert!(h.program().drives.unopened.is_empty());
    until(&mut h, |a| a.library.games.iter().any(|g| g.title == "Stick Quest"));
    let held = h.program().library.held.iter().find(|x| x.folder == stick).cloned().expect("held");
    assert!(held.here);
    h.send(broken());
    assert_eq!(toast(&h), Some("Couldn't open the drive MOVIES"), "it opened since: said again");
}
