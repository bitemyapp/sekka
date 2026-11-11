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
use super::guard;
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
    /// Folders of console BIOS images offered to the core: linked into
    /// `system_dir` beside what's there ([`offer_bios`]).
    pub bios: Vec<PathBuf>,
    /// Images from a `bios` folder by the games, and where each goes in
    /// `system_dir` ([`super::bios`]).
    pub found_bios: Vec<(PathBuf, PathBuf)>,
    pub save_dir: PathBuf,
    pub players: usize,
    /// Core settings to use instead of the core's defaults.
    pub prefer: Vec<(String, String)>,
    /// Lines the picture is shown in, for the enlargement.
    pub screen_lines: u32,
    /// Play without sound.
    pub silent: bool,
    /// Where a zipped game is unpacked for its core, and the extensions
    /// of the system's games, to know which file in the zip is the game.
    pub unpack_dir: PathBuf,
    pub extensions: &'static [&'static str],
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
    /// It stopped by itself, its core having thrown (`guard`): what it
    /// said. Where it was isn't kept.
    Stopped(String),
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

    /// Give up on a game still starting: it's told to end without keeping
    /// where it was left, and nobody waits for it, since a core stuck
    /// loading may never come back. Its thread ends by itself if it does;
    /// it's handed back to see when.
    pub fn abandon(mut self) -> Option<std::thread::JoinHandle<()>> {
        self.live.store(false, Ordering::Release);
        let _ = self.tx.send(Ctl::Stop { keep: false });
        self.thread.take()
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

/// Link each file in the folders `offered` into `system` (the core's
/// system folder): a file of its name already there stays (a grown-up's
/// own console's BIOS), a link to elsewhere is pointed here, and links to
/// what's gone (one offered before) go.
pub fn offer_bios(offered: &[PathBuf], system: &Path) {
    if let Ok(entries) = std::fs::read_dir(system) {
        for e in entries.flatten() {
            let is_link = e.file_type().is_ok_and(|t| t.is_symlink());
            if is_link && !e.path().exists() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    for dir in offered {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let (from, to) = (e.path(), system.join(e.file_name()));
            match std::fs::symlink_metadata(&to) {
                Ok(m) if !m.file_type().is_symlink() => continue,
                Ok(_) if std::fs::read_link(&to).is_ok_and(|t| t == from) => continue,
                Ok(_) => {
                    let _ = std::fs::remove_file(&to);
                }
                Err(_) => {}
            }
            if let Err(err) = std::os::unix::fs::symlink(&from, &to) {
                log::warn!("chanoma: offering {} to the cores: {err}", from.display());
            }
        }
    }
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
        match guard::state(api.serialize, buf.as_mut_ptr().cast(), size) {
            Ok(saved) => saved.then_some(buf),
            Err(why) => {
                log::warn!("chanoma: the core couldn't keep where the game was: {why}");
                None
            }
        }
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
    mut launch: Launch,
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
    offer_bios(&launch.bios, &launch.system_dir);
    super::bios::offer(&launch.found_bios, &launch.system_dir);
    // A zipped game is unpacked first, as RetroArch does: a core takes a
    // file or its bytes, never a zip. The last few stay unpacked.
    if launch.game.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        match crate::zip::unpack(&launch.game, launch.extensions, &launch.unpack_dir, 8) {
            Ok(game) => launch.game = game,
            Err(e) => return events(Event::Failed(format!("{}: {e}", launch.game.display()))),
        }
    }
    // What the compositor takes, known since the window showed: a core
    // drawing with OpenGL draws on its GPU.
    let caps = sink.as_ref().and_then(|_| aizome_kit::sidestep_ui::video::caps(Duration::from_millis(500)));
    let mut host = Box::new(Host::new(Screen::new(sink), pads, live.clone()));
    host.caps = caps;
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
        crate::watch::stopped();
        events(Event::Failed(msg));
    };

    // SAFETY: the core's entry points in libretro's order.
    let mut info: ffi::retro_system_info = unsafe { std::mem::zeroed() };
    unsafe { Host::connect_environment(&api) };
    // SAFETY: as above.
    if let Err(why) = unsafe { guard::call(api.init) } {
        return fail(format!("The game's player couldn't start: {why}"));
    }
    unsafe {
        Host::connect(&api);
        (api.get_system_info)(&mut info);
    }
    let core_name = info_string(info.library_name).unwrap_or_else(|| "a game core".into());
    // Should the core crash, Chanoma says what it was playing as it starts
    // again (`watch`).
    crate::watch::playing(&core_name, &crate::library::title_of(&launch.game));
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
    match unsafe { guard::load(api.load_game, &game) } {
        Ok(true) => {}
        Ok(false) => {
            let _ = unsafe { guard::call(api.deinit) };
            return fail(format!("{core_name} couldn't start this game"));
        }
        Err(why) => {
            let _ = unsafe { guard::call(api.deinit) };
            return fail(format!("{core_name} couldn't start this game: {why}"));
        }
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
        if let Some(hw) = &mut h.hw {
            hw.gl.configure(g.max_width.max(g.base_width), g.max_height.max(g.base_height));
        }
    }
    // A core drawing with OpenGL sets its things up in the framebuffer
    // just made, as libretro has it: after the game's loaded.
    // A core that throws (`guard`) is done with: its game ends, saying
    // what it said, with nothing more kept.
    let mut threw: Option<String> = None;
    // SAFETY: not inside a core call; then the core's own function.
    if let Some(reset) = unsafe { &*host_ptr }.hw.as_ref().and_then(|hw| hw.reset)
        && let Err(why) = unsafe { guard::call(reset) }
    {
        threw = Some(why);
    }
    load_save_ram(&api, &launch.save_ram);
    let mut resumed = false;
    if threw.is_none()
        && launch.resume
        && let Ok(bytes) = std::fs::read(&launch.state)
    {
        // Mupen64Plus-Next creates its emulation coroutine in init, but
        // only starts it in retro_run. Until then retro_unserialize
        // returns false (its `initializing` flag). Finish that startup
        // before restoring; discard the startup frame's audio.
        if core_name == "Mupen64Plus-Next" {
            // SAFETY: as in the frame loop below, with no borrowed host.
            threw = unsafe { guard::call(api.run) }.err();
            unsafe { &mut *host_ptr }.sound.clear();
        }
        // SAFETY: the state's bytes, read whole; unserialize's pointer is
        // a const one, called alike.
        let unserialize: unsafe extern "C" fn(*mut std::ffi::c_void, usize) -> bool =
            unsafe { std::mem::transmute(api.unserialize) };
        if threw.is_none() {
            match unsafe { guard::state(unserialize, bytes.as_ptr().cast_mut().cast(), bytes.len()) } {
                Ok(r) => resumed = r,
                Err(why) => threw = Some(why),
            }
        }
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
    while threw.is_none() {
        // What the program asks.
        let ctl = if paused { rx.recv_timeout(Duration::from_millis(250)).ok() } else { rx.try_recv().ok() };
        if let Some(ctl) = ctl {
            match ctl {
                Ctl::Pause(on) => {
                    paused = on;
                    speaker.set_active(!on);
                    live.store(!on, Ordering::Release);
                    if on {
                        // SAFETY: not inside a core call.
                        unsafe { &mut *host_ptr }.still();
                    }
                    next = Instant::now();
                }
                // SAFETY: as above.
                Ctl::Reset => threw = unsafe { guard::call(api.reset) }.err(),
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
        if let Err(why) = unsafe { guard::call(api.run) } {
            threw = Some(why);
            break;
        }
        // SAFETY: not inside a core call.
        let h = unsafe { &mut *host_ptr };
        if h.av_changed {
            h.av_changed = false;
            let g = h.av.geometry;
            let k = video::factor(g.base_height.max(1), launch.screen_lines);
            h.screen.configure(g.max_width.max(g.base_width), g.max_height.max(g.base_height), k);
            if let Some(hw) = &mut h.hw {
                hw.gl.configure(g.max_width.max(g.base_width), g.max_height.max(g.base_height));
            }
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
    // SAFETY: not inside a core call.
    unsafe { &mut *host_ptr }.still();
    // A core that threw may have left its memory half done: the saves
    // kept before stay.
    if threw.is_none()
        && let Some(bytes) = save_ram(&api)
        && Some(&bytes) != last_save.as_ref()
    {
        let _ = crate::store::write_bytes(&launch.save_ram, &bytes);
    }
    let mut kept = false;
    if keep
        && threw.is_none()
        && let Some(bytes) = state(&api)
    {
        kept = crate::store::write_bytes(&launch.state, &bytes).is_ok();
        // SAFETY: not inside a core call.
        let h = unsafe { &*host_ptr };
        if let Some((w, hgt, rgba)) = h.last_rgba() {
            let _ = save_picture(&launch.picture, w, hgt, &rgba, aspect(&h.av));
        }
    }
    // SAFETY: as above, in libretro's order: a core's OpenGL things go
    // while its context is there, which goes after the core.
    unsafe {
        let destroy = {
            let h = &*host_ptr;
            h.screen.clear();
            h.hw.as_ref().and_then(|hw| hw.destroy)
        };
        for f in destroy.into_iter().chain([api.unload_game, api.deinit]) {
            if let Err(why) = guard::call(f) {
                log::warn!("chanoma: {core_name} threw as it was put away: {why}");
            }
        }
        (*host_ptr).hw = None;
        // What it set going as it was put away.
        (*host_ptr).still();
    }
    Host::unregister();
    drop(host);
    crate::watch::stopped();
    match threw {
        Some(why) => {
            log::warn!("chanoma: {core_name} threw, and its game ended: {why}");
            events(Event::Stopped(format!("{core_name} stopped: {why}")));
        }
        None => events(Event::Ended { kept }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bios_offered_is_linked_in_beside_a_grown_ups_own() {
        let root = std::env::temp_dir().join(format!("chanoma-bios-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (offered, system) = (root.join("offered"), root.join("system"));
        std::fs::create_dir_all(&offered).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(offered.join("openbios.bin"), b"free").unwrap();
        std::fs::write(offered.join("scph5501.bin"), b"offered").unwrap();
        // A grown-up's own, a link to one offered before and since gone,
        // and a link to elsewhere.
        std::fs::write(system.join("scph5501.bin"), b"their own").unwrap();
        std::os::unix::fs::symlink(root.join("gone.bin"), system.join("gone.bin")).unwrap();
        std::os::unix::fs::symlink(system.join("scph5501.bin"), system.join("openbios.bin")).unwrap();
        offer_bios(std::slice::from_ref(&offered), &system);
        assert_eq!(std::fs::read(system.join("openbios.bin")).unwrap(), b"free");
        assert_eq!(std::fs::read_link(system.join("openbios.bin")).unwrap(), offered.join("openbios.bin"));
        assert_eq!(std::fs::read(system.join("scph5501.bin")).unwrap(), b"their own");
        assert!(std::fs::symlink_metadata(system.join("gone.bin")).is_err());
        // Again: nothing changes.
        offer_bios(std::slice::from_ref(&offered), &system);
        assert_eq!(std::fs::read(system.join("openbios.bin")).unwrap(), b"free");
        let _ = std::fs::remove_dir_all(&root);
    }
}
