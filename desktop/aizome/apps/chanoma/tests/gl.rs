//! A core that draws with OpenGL, written here, played through a whole
//! session: its context made on the GPU (EGL, no window) as it loads, set
//! up once its framebuffer is there, its frames copied the right way up
//! (red over blue) and kept as the picture of where it was left, the GL
//! state it left (a scissor) as it left it, and its things let go before
//! it's unloaded. Needs a GPU, or Mesa's software one, through libglvnd.
//!
//! `cargo test -p aizome-chanoma --test gl -- --ignored --nocapture`

use std::ffi::{c_int, c_uint, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use aizome_chanoma::pad::Shared;
use aizome_chanoma::retro::ffi::*;
use aizome_chanoma::retro::{CoreSource, Event, Launch, Session};

static ENV: Mutex<Option<retro_environment_t>> = Mutex::new(None);
static VIDEO: Mutex<Option<retro_video_refresh_t>> = Mutex::new(None);
static mut HW: retro_hw_render_callback = retro_hw_render_callback {
    context_type: RETRO_HW_CONTEXT_OPENGL_CORE,
    context_reset: Some(context_reset),
    get_current_framebuffer: None,
    get_proc_address: None,
    depth: true,
    stencil: true,
    bottom_left_origin: true,
    version_major: 3,
    version_minor: 3,
    cache_context: false,
    context_destroy: Some(context_destroy),
    debug_context: false,
};
static RESET: AtomicBool = AtomicBool::new(false);
static DESTROYED: AtomicBool = AtomicBool::new(false);
/// `context_destroy` came before `retro_unload_game`.
static IN_ORDER: AtomicBool = AtomicBool::new(false);
/// The scissor it left on was on at its next frame.
static SCISSOR_KEPT: AtomicBool = AtomicBool::new(true);
static FRAMES: AtomicU64 = AtomicU64::new(0);

/// GL's functions, as the core got them.
static GL: [AtomicUsize; 7] = [const { AtomicUsize::new(0) }; 7];
const NAMES: [&std::ffi::CStr; 7] =
    [c"glBindFramebuffer", c"glViewport", c"glClearColor", c"glClear", c"glEnable", c"glScissor", c"glIsEnabled"];
const GL_FRAMEBUFFER: c_uint = 0x8d40;
const GL_COLOR_BUFFER_BIT: c_uint = 0x4000;
const GL_SCISSOR_TEST: c_uint = 0x0c11;

fn gl<T: Copy>(i: usize) -> T {
    let p = GL[i].load(Ordering::SeqCst);
    assert_ne!(p, 0, "{:?} wasn't there", NAMES[i]);
    // SAFETY: the function's address, of the type the caller names.
    unsafe { std::mem::transmute_copy::<usize, T>(&p) }
}

unsafe extern "C" fn context_reset() {
    let get = unsafe { HW.get_proc_address }.expect("given get_proc_address");
    for (i, name) in NAMES.iter().enumerate() {
        GL[i].store(unsafe { get(name.as_ptr()) } as usize, Ordering::SeqCst);
    }
    RESET.store(true, Ordering::SeqCst);
}
unsafe extern "C" fn context_destroy() {
    DESTROYED.store(true, Ordering::SeqCst);
}

unsafe extern "C" fn set_environment(cb: retro_environment_t) {
    *ENV.lock().unwrap() = Some(cb);
}
unsafe extern "C" fn set_video(cb: retro_video_refresh_t) {
    *VIDEO.lock().unwrap() = Some(cb);
}
unsafe extern "C" fn set_sample(_: retro_audio_sample_t) {}
unsafe extern "C" fn set_batch(_: retro_audio_sample_batch_t) {}
unsafe extern "C" fn set_poll(_: retro_input_poll_t) {}
unsafe extern "C" fn set_input(_: retro_input_state_t) {}
unsafe extern "C" fn init() {}
unsafe extern "C" fn deinit() {}
unsafe extern "C" fn api_version() -> c_uint {
    RETRO_API_VERSION
}
unsafe extern "C" fn system_info(info: *mut retro_system_info) {
    unsafe {
        (*info).library_name = c"Test GL Core".as_ptr();
        (*info).library_version = c"1".as_ptr();
        (*info).valid_extensions = c"gdi".as_ptr();
        (*info).need_fullpath = true;
        (*info).block_extract = false;
    }
}
unsafe extern "C" fn av_info(info: *mut retro_system_av_info) {
    unsafe {
        *info = retro_system_av_info {
            geometry: retro_game_geometry {
                base_width: 320,
                base_height: 240,
                max_width: 640,
                max_height: 480,
                aspect_ratio: 4.0 / 3.0,
            },
            timing: retro_system_timing { fps: 60.0, sample_rate: 44_100.0 },
        };
    }
}
unsafe extern "C" fn port_device(_: c_uint, _: c_uint) {}
unsafe extern "C" fn reset() {}
unsafe extern "C" fn run() {
    // The scissor left on at the last frame is still on: the frontend's
    // copy put it back.
    if FRAMES.load(Ordering::SeqCst) > 0 {
        let is_enabled: unsafe extern "C" fn(c_uint) -> u8 = gl(6);
        if unsafe { is_enabled(GL_SCISSOR_TEST) } == 0 {
            SCISSOR_KEPT.store(false, Ordering::SeqCst);
        }
    }
    let fbo = unsafe { HW.get_current_framebuffer }.expect("given get_current_framebuffer");
    let bind: unsafe extern "C" fn(c_uint, c_uint) = gl(0);
    let viewport: unsafe extern "C" fn(c_int, c_int, c_int, c_int) = gl(1);
    let clear_color: unsafe extern "C" fn(f32, f32, f32, f32) = gl(2);
    let clear: unsafe extern "C" fn(c_uint) = gl(3);
    let enable: unsafe extern "C" fn(c_uint) = gl(4);
    let scissor: unsafe extern "C" fn(c_int, c_int, c_int, c_int) = gl(5);
    let disable: unsafe extern "C" fn(c_uint) = {
        // glDisable isn't asked for: glEnable's partner, by its own name.
        let get = unsafe { HW.get_proc_address }.unwrap();
        let p = unsafe { get(c"glDisable".as_ptr()) };
        unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(c_uint)>(p) }
    };
    unsafe {
        bind(GL_FRAMEBUFFER, fbo() as c_uint);
        viewport(0, 0, 320, 240);
        disable(GL_SCISSOR_TEST);
        // Blue below; red above (GL's rows count up from the bottom).
        clear_color(0.0, 0.0, 1.0, 1.0);
        clear(GL_COLOR_BUFFER_BIT);
        enable(GL_SCISSOR_TEST);
        scissor(0, 120, 320, 120);
        clear_color(1.0, 0.0, 0.0, 1.0);
        clear(GL_COLOR_BUFFER_BIT);
    }
    FRAMES.fetch_add(1, Ordering::SeqCst);
    let video = VIDEO.lock().unwrap().unwrap();
    unsafe { video(RETRO_HW_FRAME_BUFFER_VALID, 320, 240, 0) };
}
unsafe extern "C" fn serialize_size() -> usize {
    8
}
unsafe extern "C" fn serialize(data: *mut c_void, size: usize) -> bool {
    let n = FRAMES.load(Ordering::SeqCst).to_le_bytes();
    unsafe { std::ptr::copy_nonoverlapping(n.as_ptr(), data.cast::<u8>(), size.min(8)) };
    true
}
unsafe extern "C" fn unserialize(_: *const c_void, _: usize) -> bool {
    true
}
unsafe extern "C" fn load_game(_: *const retro_game_info) -> bool {
    let env = ENV.lock().unwrap().unwrap();
    let mut preferred: c_uint = 0;
    unsafe { env(RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER, (&mut preferred as *mut c_uint).cast()) };
    assert_eq!(preferred, RETRO_HW_CONTEXT_OPENGL_CORE);
    unsafe { env(RETRO_ENVIRONMENT_SET_HW_RENDER, std::ptr::addr_of_mut!(HW).cast()) }
}
unsafe extern "C" fn unload_game() {
    IN_ORDER.store(DESTROYED.load(Ordering::SeqCst), Ordering::SeqCst);
}
unsafe extern "C" fn memory_data(_: c_uint) -> *mut c_void {
    std::ptr::null_mut()
}
unsafe extern "C" fn memory_size(_: c_uint) -> usize {
    0
}

fn api() -> Api {
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

#[test]
#[ignore = "needs a GPU through EGL"]
fn an_opengl_core_draws_and_its_frames_come_the_right_way_up() {
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Info).try_init();
    let dir = std::env::temp_dir().join(format!("chanoma-gl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("game.gdi"), b"1\n").unwrap();
    let launch = Launch {
        core: CoreSource::Api(api()),
        game: dir.join("game.gdi"),
        save_ram: dir.join("game.srm"),
        state: dir.join("game.state"),
        picture: dir.join("game.png"),
        resume: false,
        system_dir: dir.join("system"),
        bios: Vec::new(),
        found_bios: Vec::new(),
        save_dir: dir.join("saves"),
        players: 1,
        prefer: Vec::new(),
        screen_lines: 1080,
        silent: true,
        unpack_dir: dir.join("unzipped"),
        extensions: &[],
    };
    let (tx, rx) = mpsc::channel();
    let session = Session::start(launch, None, Arc::new(Shared::default()), None, move |e| {
        let _ = tx.send(e);
    });
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Event::Started { core, .. }) => assert_eq!(core, "Test GL Core"),
        other => panic!("no OpenGL context here? {other:?}"),
    }
    assert!(RESET.load(Ordering::SeqCst), "context_reset after the game loaded");
    std::thread::sleep(Duration::from_millis(500));
    session.stop(true);
    assert!(matches!(rx.recv_timeout(Duration::from_secs(5)), Ok(Event::Ended { kept: true })));
    assert!(FRAMES.load(Ordering::SeqCst) > 5, "it ran");
    assert!(SCISSOR_KEPT.load(Ordering::SeqCst), "the core's scissor was put back");
    assert!(IN_ORDER.load(Ordering::SeqCst), "context_destroy, then retro_unload_game");
    // The picture: red at the top, blue at the bottom.
    let img = image::open(dir.join("game.png")).unwrap().to_rgba8();
    let (w, h) = img.dimensions();
    let top = img.get_pixel(w / 2, h / 10).0;
    let bottom = img.get_pixel(w / 2, h - h / 10).0;
    println!("{w}x{h}: top {top:?}, bottom {bottom:?}");
    assert_eq!((top[0], top[2]), (255, 0), "red at the top");
    assert_eq!((bottom[0], bottom[2]), (0, 255), "blue at the bottom");
    let _ = std::fs::remove_dir_all(&dir);
}
