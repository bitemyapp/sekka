//! A game, played on a thread of its own: the core runs a frame at a time
//! at its own rate, frames go to the window's video sink, sound to
//! PipeWire, and the controllers are read from [`Shared`] as the core asks.
//!
//! - **Saves are kept**: the game's own battery-backed save is read before
//!   it starts and written whenever it changes (checked every half
//!   minute) and when it ends.
//! - **Where it was left**: leaving a game saves its whole state and a
//!   picture of its last frame, and the next start can carry on from there.
//! - **Paused**, the core doesn't run and the sound stops; the controllers
//!   reach it again only when it carries on.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aizome_kit::sidestep_ui::video::VideoSink;

use super::audio::{self, Resampler, Speaker};
use super::ffi::{self, Api};
use super::host::Host;
use super::video::{self, Screen};
use crate::pad::Shared;

/// Where the core comes from.
pub enum CoreSource {
    /// A library to load.
    Library(PathBuf),
    /// Entry points already in the process (tests).
    Api(Api),
}

/// What to play and where its things are kept.
pub struct Launch {
    pub core: CoreSource,
    pub game: PathBuf,
    /// The game's battery-backed save.
    pub save_ram: PathBuf,
    /// Its state where it was left.
    pub state: PathBuf,
    /// A picture of where it was left.
    pub picture: PathBuf,
    /// Carry on from the state left.
    pub resume: bool,
    pub system_dir: PathBuf,
    pub save_dir: PathBuf,
    pub players: usize,
    /// Core settings to use instead of the core's defaults.
    pub prefer: Vec<(String, String)>,
    /// Lines the picture is shown in, for the enlargement.
    pub screen_lines: u32,
    /// Play without sound.
    pub silent: bool,
}

/// What the game says.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// It runs: its name, its picture's shape (width over height) and
    /// whether it carried on from where it was left.
    Started { core: String, aspect: f64, resumed: bool },
    /// It couldn't start.
    Failed(String),
    /// The picture's shape changed.
    Aspect(f64),
    /// It ended: whether where it was left was kept.
    Ended { kept: bool },
}

enum Ctl {
    Pause(bool),
    Reset,
    Volume(f32),
    Stop { keep: bool },
}

/// A game being played.
pub struct Session {
    tx: mpsc::Sender<Ctl>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Presses reach the game.
    pub live: Arc<AtomicBool>,
}

impl Session {
    pub fn start(
        launch: Launch,
        sink: Option<VideoSink>,
        pads: Arc<Shared>,
        rumble: Option<super::Rumble>,
        events: impl Fn(Event) + Send + 'static,
    ) -> Session {
        let (tx, rx) = mpsc::channel();
        let live = Arc::new(AtomicBool::new(true));
        let l = live.clone();
        let thread = std::thread::Builder::new()
            .name("chanoma-game".into())
            .spawn(move || run(launch, sink, pads, l, rumble, rx, Box::new(events)))
            .ok();
        Session { tx, thread, live }
    }

    /// Stop the game where it is (a menu over it), or carry on.
    pub fn pause(&self, on: bool) {
        self.live.store(!on, Ordering::Release);
        let _ = self.tx.send(Ctl::Pause(on));
    }

    pub fn reset(&self) {
        let _ = self.tx.send(Ctl::Reset);
    }

    pub fn set_volume(&self, v: f32) {
        let _ = self.tx.send(Ctl::Volume(v));
    }

    /// End it, keeping where it was left if `keep`; waits until it has.
    pub fn stop(mut self, keep: bool) {
        self.live.store(false, Ordering::Release);
        let _ = self.tx.send(Ctl::Stop { keep });
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.tx.send(Ctl::Stop { keep: true });
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn cstring(p: &Path) -> CString {
    CString::new(p.as_os_str().as_encoded_bytes()).unwrap_or_default()
}

/// The core's own save, read into its memory.
fn load_save_ram(api: &Api, path: &Path) {
    // SAFETY: the core's memory of the size it says, alive while loaded.
    unsafe {
        let size = (api.get_memory_size)(ffi::RETRO_MEMORY_SAVE_RAM);
        let data = (api.get_memory_data)(ffi::RETRO_MEMORY_SAVE_RAM);
        if size == 0 || data.is_null() {
            return;
        }
        if let Ok(bytes) = std::fs::read(path) {
            let n = bytes.len().min(size);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast::<u8>(), n);
        }
    }
}

/// The core's save as it is now.
fn save_ram(api: &Api) -> Option<Vec<u8>> {
    // SAFETY: as above.
    unsafe {
        let size = (api.get_memory_size)(ffi::RETRO_MEMORY_SAVE_RAM);
        let data = (api.get_memory_data)(ffi::RETRO_MEMORY_SAVE_RAM);
        (size > 0 && !data.is_null()).then(|| std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec())
    }
}

fn state(api: &Api) -> Option<Vec<u8>> {
    // SAFETY: a buffer of the size the core asks for.
    unsafe {
        let size = (api.serialize_size)();
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        (api.serialize)(buf.as_mut_ptr().cast(), size).then_some(buf)
    }
}

fn aspect(av: &ffi::retro_system_av_info) -> f64 {
    let g = &av.geometry;
    if g.aspect_ratio > 0.0 {
        f64::from(g.aspect_ratio)
    } else if g.base_height > 0 {
        f64::from(g.base_width) / f64::from(g.base_height)
    } else {
        4.0 / 3.0
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    launch: Launch,
    sink: Option<VideoSink>,
    pads: Arc<Shared>,
    live: Arc<AtomicBool>,
    rumble: Option<super::Rumble>,
    rx: mpsc::Receiver<Ctl>,
    events: Box<dyn Fn(Event) + Send>,
) {
    // The library stays loaded until the thread ends: some cores don't
    // like being unloaded and loaded again in one process.
    let (api, _lib, core_path) = match &launch.core {
        CoreSource::Api(api) => (*api, None, PathBuf::new()),
        // SAFETY: a core's initialisers run as it's loaded; Chanoma loads
        // only the cores it was given.
        CoreSource::Library(path) => match unsafe { libloading::Library::new(path) } {
            Ok(lib) => match unsafe { Api::of(&lib) } {
                Ok(api) => (api, Some(lib), path.clone()),
                Err(e) => return events(Event::Failed(format!("{} isn't a game core: {e}", path.display()))),
            },
            Err(e) => return events(Event::Failed(format!("{}: {e}", path.display()))),
        },
    };
    // SAFETY: calling the core's entry points as libretro defines them.
    if unsafe { (api.api_version)() } != ffi::RETRO_API_VERSION {
        return events(Event::Failed("The game core is for another version of libretro".into()));
    }
    for d in [&launch.system_dir, &launch.save_dir] {
        let _ = std::fs::create_dir_all(d);
    }
    let mut host = Box::new(Host::new(Screen::new(sink), pads, live.clone()));
    host.system_dir = cstring(&launch.system_dir);
    host.save_dir = cstring(&launch.save_dir);
    host.core_path = cstring(&core_path);
    host.players = launch.players.clamp(1, 4);
    host.rumble = rumble;
    host.options.prefer = launch.prefer.iter().cloned().collect();
    let host_ptr: *mut Host = &mut *host;
    // SAFETY: the box stays put until unregistered below; the code here
    // touches it only between calls into the core.
    unsafe { Host::register(host_ptr) };
    let fail = |msg: String| {
        Host::unregister();
        events(Event::Failed(msg));
    };

    // SAFETY: the core's entry points in libretro's order.
    let mut info: ffi::retro_system_info = unsafe { std::mem::zeroed() };
    unsafe {
        Host::connect(&api);
        (api.init)();
        (api.get_system_info)(&mut info);
    }
    let core_name = info_string(info.library_name).unwrap_or_else(|| "a game core".into());
    let game_path = cstring(&launch.game);
    // Cores that read the game themselves get its path; the rest its
    // bytes too, kept until it's unloaded.
    let data = if info.need_fullpath {
        None
    } else {
        match std::fs::read(&launch.game) {
            Ok(d) => Some(d),
            Err(e) => {
                // SAFETY: as above.
                unsafe { (api.deinit)() };
                return fail(format!("{}: {e}", launch.game.display()));
            }
        }
    };
    let game = ffi::retro_game_info {
        path: game_path.as_ptr(),
        data: data.as_ref().map_or(std::ptr::null(), |d| d.as_ptr().cast()),
        size: data.as_ref().map_or(0, Vec::len),
        meta: std::ptr::null(),
    };
    // SAFETY: as above; `game` and its bytes outlive the loaded game.
    if !unsafe { (api.load_game)(&game) } {
        unsafe { (api.deinit)() };
        return fail(format!("{core_name} couldn't start this game"));
    }
    let mut av = ffi::retro_system_av_info::default();
    // SAFETY: as above.
    unsafe {
        (api.get_system_av_info)(&mut av);
        for port in 0..host.players as u32 {
            (api.set_controller_port_device)(port, ffi::RETRO_DEVICE_JOYPAD);
        }
    }
    {
        // SAFETY: not inside a core call.
        let h = unsafe { &mut *host_ptr };
        h.av = av;
        let g = av.geometry;
        let k = video::factor(g.base_height.max(1), launch.screen_lines);
        h.screen.configure(g.max_width.max(g.base_width), g.max_height.max(g.base_height), k);
    }
    load_save_ram(&api, &launch.save_ram);
    let mut resumed = false;
    if launch.resume
        && let Ok(bytes) = std::fs::read(&launch.state)
    {
        // SAFETY: the state's bytes, read whole.
        resumed = unsafe { (api.unserialize)(bytes.as_ptr().cast(), bytes.len()) };
        if !resumed {
            log::warn!("chanoma: {} couldn't carry on from {}", core_name, launch.state.display());
        }
    }
    let speaker = if launch.silent {
        Speaker::silent()
    } else {
        Speaker::connect(&launch.game.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
            .unwrap_or_else(|e| {
                log::warn!("chanoma: no sound for the game: {e}");
                Speaker::silent()
            })
    };
    let mut resampler = Resampler::new(av.timing.sample_rate.max(1.0));
    events(Event::Started { core: core_name.clone(), aspect: aspect(&av), resumed });

    let mut frame_time = Duration::from_secs_f64(1.0 / av.timing.fps.clamp(1.0, 240.0));
    let mut next = Instant::now();
    let mut paused = false;
    let mut last_save = save_ram(&api);
    let mut save_check = Instant::now() + Duration::from_secs(30);
    let mut out = Vec::with_capacity(4096);
    let mut keep = true;
    loop {
        // What the program asks.
        let ctl = if paused { rx.recv_timeout(Duration::from_millis(250)).ok() } else { rx.try_recv().ok() };
        if let Some(ctl) = ctl {
            match ctl {
                Ctl::Pause(on) => {
                    paused = on;
                    speaker.set_active(!on);
                    live.store(!on, Ordering::Release);
                    next = Instant::now();
                }
                // SAFETY: as above.
                Ctl::Reset => unsafe { (api.reset)() },
                Ctl::Volume(v) => speaker.set_volume(v),
                Ctl::Stop { keep: k } => {
                    keep = k;
                    break;
                }
            }
            continue;
        }
        if paused {
            continue;
        }
        // SAFETY: as above; `host` isn't borrowed across the call.
        unsafe { (api.run)() };
        // SAFETY: not inside a core call.
        let h = unsafe { &mut *host_ptr };
        if h.av_changed {
            h.av_changed = false;
            let g = h.av.geometry;
            let k = video::factor(g.base_height.max(1), launch.screen_lines);
            h.screen.configure(g.max_width.max(g.base_width), g.max_height.max(g.base_height), k);
            resampler.set_rate(h.av.timing.sample_rate.max(1.0));
            frame_time = Duration::from_secs_f64(1.0 / h.av.timing.fps.clamp(1.0, 240.0));
            events(Event::Aspect(aspect(&h.av)));
        }
        out.clear();
        resampler.run(&h.sound, audio::lean(speaker.buffered()), &mut out);
        h.sound.clear();
        speaker.push(&out);
        if h.shutdown {
            break;
        }
        let now = Instant::now();
        if now >= save_check {
            save_check = now + Duration::from_secs(30);
            let s = save_ram(&api);
            if s != last_save {
                if let Some(bytes) = &s {
                    let _ = crate::store::write_bytes(&launch.save_ram, bytes);
                }
                last_save = s;
            }
        }
        // The clock sets the pace; a machine that fell behind (a stall)
        // starts counting again rather than racing to catch up.
        next += frame_time;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else if now - next > frame_time * 6 {
            next = now;
        }
    }

    speaker.set_active(false);
    if let Some(bytes) = save_ram(&api)
        && Some(&bytes) != last_save.as_ref()
    {
        let _ = crate::store::write_bytes(&launch.save_ram, &bytes);
    }
    let mut kept = false;
    if keep && let Some(bytes) = state(&api) {
        kept = crate::store::write_bytes(&launch.state, &bytes).is_ok();
        // SAFETY: not inside a core call.
        let h = unsafe { &*host_ptr };
        if let Some((w, hgt, rgba)) = h.screen.last_rgba() {
            let _ = save_picture(&launch.picture, w, hgt, &rgba, aspect(&h.av));
        }
    }
    // SAFETY: as above, in libretro's order.
    unsafe {
        let h = &*host_ptr;
        h.screen.clear();
        (api.unload_game)();
        (api.deinit)();
    }
    Host::unregister();
    drop(host);
    events(Event::Ended { kept });
}

fn info_string(p: *const std::ffi::c_char) -> Option<String> {
    // SAFETY: the core's NUL-terminated string, or null.
    (!p.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// The last frame as a PNG, stretched to the shape it's shown in.
fn save_picture(path: &Path, w: u32, h: u32, rgba: &[u8], aspect: f64) -> Result<(), String> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("a frame of the wrong size")?;
    // Twice the core's height, as wide as the shape says: big enough for a
    // tile, square pixels where the game's weren't.
    let th = h * 2;
    let tw = ((f64::from(th) * aspect).round() as u32).max(1);
    let img = image::imageops::resize(&img, tw, th, image::imageops::FilterType::Nearest);
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    crate::store::write_bytes(path, &bytes).map_err(|e| e.to_string())
}
