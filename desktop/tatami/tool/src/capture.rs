// SPDX-License-Identifier: MIT OR Apache-2.0
//! Trigger › Capture, after Omarchy's omarchy-capture-* commands: screenshots
//! (freeze the screen, select with slurp, save, copy, and offer editing in
//! Satty from the notification), screen recordings with gpu-screen-recorder,
//! text (OCR with Tesseract) and QR codes (zbar) from a region, and colors.
use crate::{
    hypr::{self, Rect},
    util,
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime},
};

/// A directory from user-dirs.dirs (`KEY="$HOME/Pictures"` form), else
/// ~/fallback.
fn user_dir(user_dirs: &str, home: &str, key: &str, fallback: &str) -> PathBuf {
    user_dirs
        .lines()
        .filter_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
        .map(|value| value.trim_matches('"').replace("$HOME", home))
        .find(|value| value.starts_with('/'))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(fallback))
}

/// XDG_PICTURES_DIR, else ~/Pictures.
pub fn pictures_dir(user_dirs: &str, home: &str) -> PathBuf {
    user_dir(user_dirs, home, "XDG_PICTURES_DIR", "Pictures")
}

/// XDG_VIDEOS_DIR, else ~/Videos.
pub fn videos_dir(user_dirs: &str, home: &str) -> PathBuf {
    user_dir(user_dirs, home, "XDG_VIDEOS_DIR", "Videos")
}

fn user_dirs() -> (String, String) {
    let home = util::home();
    let dirs = fs::read_to_string(format!("{home}/.config/user-dirs.dirs")).unwrap_or_default();
    (dirs, home)
}

/// Now in local time, as seconds for `timestamp`: file names use the clock
/// the bar shows, as Omarchy's do.
fn now() -> u64 {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // SAFETY: libc::tm is plain data, valid when zeroed.
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    let time = seconds as libc::time_t;
    // SAFETY: localtime_r reads `time` and writes only into `local`.
    let offset = if unsafe { libc::localtime_r(&time, &mut local) }.is_null() {
        0
    } else {
        local.tm_gmtoff
    };
    seconds.saturating_add_signed(offset)
}

/// A click (a selection under 20 square pixels) means "the window or monitor
/// under the pointer", not a 2-pixel screenshot.
pub fn smart_selection(selection: Rect, candidates: &[Rect]) -> Rect {
    if selection.w * selection.h >= 20 {
        return selection;
    }
    // Windows are listed after monitors; prefer the most specific match.
    candidates
        .iter()
        .rev()
        .find(|rect| rect.contains(selection.x, selection.y))
        .copied()
        .unwrap_or(selection)
}

/// Timestamp for file names from seconds since the epoch (shifted to local
/// time by `now`), without external date tools.
pub fn timestamp(seconds: u64) -> String {
    let days = seconds / 86400;
    let (hour, minute, second) = (seconds % 86400 / 3600, seconds % 3600 / 60, seconds % 60);
    // Civil-from-days (Howard Hinnant), valid for all dates after 1970.
    let z = days as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}_{hour:02}-{minute:02}-{second:02}")
}

pub fn screenshot(mode: &str) {
    // Pressing the key again while selecting cancels the selection.
    let selecting = util::pids_named(&["slurp"]);
    if !selecting.is_empty() {
        util::terminate(&selecting);
        return;
    }
    // From the menu: keep it out of the frozen frame.
    crate::menu::before_capture();
    let (dirs, home) = user_dirs();
    let dir = pictures_dir(&dirs, &home);
    if fs::create_dir_all(&dir).is_err() {
        util::notify(
            "\u{f030}",
            "Screenshot failed",
            &format!("Cannot create {}", dir.display()),
        );
        return;
    }
    let monitors = hypr::json("monitors").unwrap_or_default();
    let clients = hypr::json("clients").unwrap_or_default();
    let candidates = hypr::workspace_rects(&monitors, &clients);
    let selection = if mode == "fullscreen" {
        hypr::focused_monitor_rect(&monitors)
    } else {
        // Freeze the screen so the selection matches what is captured.
        let freeze = Freeze::start();
        let picked = match mode {
            "region" => util::output("slurp", &[]),
            _ => {
                let input: String = candidates.iter().map(|r| r.slurp() + "\n").collect();
                let args: &[&str] = if mode == "windows" { &["-r"] } else { &[] };
                util::filter("slurp", args, input.as_bytes())
            }
        };
        let rect = picked.ok().and_then(|text| Rect::parse(&text)).map(|rect| {
            if mode == "region" {
                rect
            } else {
                smart_selection(rect, &candidates)
            }
        });
        let shot = rect.and_then(|rect| capture(&dir, rect));
        drop(freeze);
        if let Some(path) = shot {
            announce(&path);
        }
        return;
    };
    if let Some(path) = selection.and_then(|rect| capture(&dir, rect)) {
        announce(&path);
    }
}

fn capture(dir: &std::path::Path, rect: Rect) -> Option<PathBuf> {
    let path = dir.join(format!("screenshot-{}.png", timestamp(now())));
    let target = path.to_str()?;
    util::run("grim", &["-g", &rect.slurp(), target]).then_some(path)
}

fn announce(path: &std::path::Path) {
    if let Ok(bytes) = fs::read(path) {
        let _ = util::filter("wl-copy", &["--type", "image/png"], &bytes);
    }
    // The notification waits for a click; let it outlive this command.
    if let Some(path) = path.to_str()
        && let Some(me) = util::me()
    {
        util::spawn(&me, &["screenshot-notify", path]);
    }
}

/// Offer editing from the notification (or Super+Alt+, which invokes it).
pub fn notify_edit(path: &str) {
    let action = util::output(
        "notify-send",
        &[
            "Screenshot saved to clipboard and file",
            "Edit with Super + Alt + , (or click this)",
            "-t",
            "10000",
            "-i",
            path,
            "-A",
            "default=edit",
        ],
    )
    .unwrap_or_default();
    if action.trim() == "default" {
        util::run(
            "satty",
            &[
                "--filename",
                path,
                "--output-filename",
                path,
                "--actions-on-enter",
                "save-to-clipboard",
                "--save-after-copy",
                "--copy-command",
                "wl-copy",
            ],
        );
    }
}

/// The screen frozen while a region is chosen (`hyprpicker -r -z`), until
/// dropped. hyprpicker keeps each monitor's frame in a file in its runtime
/// directory (32 MB at 4K) and removes the files only when it ends by
/// itself. Killed, it left them in /run/user, until that was full and the
/// next screen copy into such a file crashed Hyprland (SIGBUS: Hyprland
/// writes to clients' memory unguarded). So it runs with a runtime directory
/// of its own, removed when the freeze ends, whatever hyprpicker did.
struct Freeze {
    child: Option<std::process::Child>,
    dir: PathBuf,
}

impl Freeze {
    fn start() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        sweep_frames(&runtime);
        let dir = PathBuf::from(&runtime).join(format!("tatami-freeze-{}", std::process::id()));
        // Absolute, so it still finds the compositor from the other directory.
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let socket = if display.starts_with('/') {
            display
        } else {
            format!("{runtime}/{display}")
        };
        // Its frames are the screen's contents: the user's alone.
        let made =
            runtime.starts_with('/') && fs::DirBuilder::new().mode(0o700).create(&dir).is_ok();
        let child = made
            .then(|| {
                Command::new("hyprpicker")
                    .args(["-r", "-z"])
                    .env("XDG_RUNTIME_DIR", &dir)
                    .env("WAYLAND_DISPLAY", &socket)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .ok()
            })
            .flatten();
        thread::sleep(Duration::from_millis(100));
        Self { child, dir }
    }
}

impl Drop for Freeze {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            // SIGTERM lets it close its surfaces; SIGKILL if it does not.
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            for _ in 0..20 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Frames freezes left before: hyprpicker's own files, from when it was
/// killed, and the directories of helpers that did not finish. Only while no
/// hyprpicker runs, as the color picker keeps its frames there too.
fn sweep_frames(runtime: &str) {
    if !runtime.starts_with('/') || !util::pids_named(&["hyprpicker"]).is_empty() {
        return;
    }
    for entry in fs::read_dir(runtime).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if leaked_frame(name, |pid| {
            std::path::Path::new(&format!("/proc/{pid}")).exists()
        }) {
            let path = entry.path();
            let _ = fs::remove_file(&path).or_else(|_| fs::remove_dir_all(&path));
        }
    }
}

/// A frame file or freeze directory in the runtime directory that nothing
/// uses: hyprpicker's (".hyprpicker_XXXXXX"), or a helper's that has ended.
pub fn leaked_frame(name: &str, alive: impl Fn(u32) -> bool) -> bool {
    if name.starts_with(".hyprpicker_") {
        return true;
    }
    name.strip_prefix("tatami-freeze-")
        .and_then(|pid| pid.parse().ok())
        .is_some_and(|pid| !alive(pid))
}

/// Freeze the screen and select a region; None when cancelled.
fn frozen_region() -> Option<Rect> {
    let _freeze = Freeze::start();
    util::output("slurp", &[])
        .ok()
        .and_then(|text| Rect::parse(&text))
}

/// A region as a PNG in the runtime directory, removed when dropped.
struct Grab(PathBuf);

impl Grab {
    fn region(rect: Rect) -> Option<Grab> {
        let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
        let grab =
            Grab(PathBuf::from(runtime).join(format!("tatami-grab-{}.png", std::process::id())));
        util::run("grim", &["-g", &rect.slurp(), grab.0.to_str()?]).then_some(grab)
    }
}

impl Drop for Grab {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Trigger › Capture › Text and Super+Ctrl+Print: the text in a region, in
/// the clipboard.
pub fn text() {
    crate::menu::before_capture();
    let Some(grab) = frozen_region().and_then(Grab::region) else {
        return;
    };
    let text = grab.0.to_str().and_then(|path| {
        util::output(
            "tesseract",
            &[
                path,
                "stdout",
                "--oem",
                "1",
                "--psm",
                "6",
                "-l",
                "eng",
                "--dpi",
                "300",
                "-c",
                "preserve_interword_spaces=1",
            ],
        )
        .ok()
    });
    match text.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => {
            let _ = util::filter("wl-copy", &[], text.as_bytes());
            util::notify("\u{f0d11}", "Copied text from selection to clipboard", "");
        }
        _ => util::notify(
            "\u{f0d11}",
            "No text found",
            "Select a region containing text",
        ),
    }
}

/// Trigger › Capture › QR Code: the contents of a QR code in a region, in
/// the clipboard (marked sensitive: they are often Wi-Fi passwords).
pub fn qr() {
    crate::menu::before_capture();
    let Some(grab) = frozen_region().and_then(Grab::region) else {
        return;
    };
    let code = grab.0.to_str().and_then(|path| {
        util::output(
            "zbarimg",
            &["-q", "--raw", "-Sdisable", "-Sqrcode.enable", path],
        )
        .ok()
    });
    match code.as_deref().map(str::trim) {
        Some(code) if !code.is_empty() => {
            let _ = util::filter("wl-copy", &["--sensitive"], code.as_bytes());
            util::notify("\u{f0432}", "QR code copied to clipboard", "");
        }
        _ => util::notify(
            "\u{f0432}",
            "No QR code found",
            "Select a region containing a QR code",
        ),
    }
}

/// The recorders' process names, as /proc truncates them: gpu-screen-recorder,
/// and wf-recorder for machines without hardware OpenGL, which
/// gpu-screen-recorder refuses (virtual machines, missing drivers).
const RECORDERS: [&str; 2] = ["gpu-screen-reco", "wf-recorder"];
pub const RECORDING_SIGNAL: i32 = 8;

pub fn recording() -> bool {
    !util::pids_named(&RECORDERS).is_empty()
}

/// What to record: a whole monitor (selected by clicking it, or dragging over
/// all of it) or a region.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Monitor(String),
    Region(Rect),
}

pub fn record_target(selection: Rect, monitors: &serde_json::Value) -> Target {
    monitors
        .as_array()
        .into_iter()
        .flatten()
        .find(|monitor| hypr::monitor_rect(monitor) == Some(selection))
        .and_then(|monitor| monitor["name"].as_str())
        .filter(|name| hypr::safe_name(name))
        .map(|name| Target::Monitor(name.to_owned()))
        .unwrap_or(Target::Region(selection))
}

/// gpu-screen-recorder's arguments, as omarchy-capture-screenrecording
/// passes them.
pub fn gsr_args(target: &Target, file: &str, audio: Option<&str>) -> Vec<String> {
    let window = match target {
        Target::Monitor(name) => name.clone(),
        Target::Region(r) => format!("{}x{}+{}+{}", r.w, r.h, r.x, r.y),
    };
    let mut args: Vec<String> = [
        "-w",
        &window,
        "-k",
        "auto",
        "-f",
        "60",
        "-fm",
        "cfr",
        "-fallback-cpu-encoding",
        "yes",
        "-o",
        file,
    ]
    .map(str::to_owned)
    .to_vec();
    if let Some(audio) = audio {
        args.extend(["-a", audio, "-ac", "aac"].map(str::to_owned));
    }
    args
}

/// wf-recorder's: one audio source at most, desktop audio before the
/// microphone.
pub fn wf_args(target: &Target, file: &str, desktop: bool, microphone: bool) -> Vec<String> {
    let mut args: Vec<String> = match target {
        Target::Monitor(name) => vec!["-o".into(), name.clone()],
        Target::Region(r) => vec!["-g".into(), r.slurp()],
    };
    if desktop {
        args.push("--audio=@DEFAULT_MONITOR@".into());
    } else if microphone {
        args.push("--audio=@DEFAULT_SOURCE@".into());
    }
    args.extend(["-f".into(), file.to_owned()]);
    args
}

/// Wait up to `tries` × 200 ms for a recorder to be running and stay so.
fn started(tries: u32) -> bool {
    for _ in 0..tries {
        thread::sleep(Duration::from_millis(200));
        if !recording() {
            return false;
        }
    }
    true
}

/// gpu-screen-recorder's audio sources: desktop audio, the microphone, both.
pub fn audio_sources(desktop: bool, microphone: bool) -> Option<String> {
    let sources: Vec<&str> = [(desktop, "default_output"), (microphone, "default_input")]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, source)| *source)
        .collect();
    (!sources.is_empty()).then(|| sources.join("|"))
}

/// Trigger › Capture › Screenrecord, Alt+Print and the bar's recording
/// indicator: stop the recording in progress, or select what to record and
/// start one. `--stop` only stops; `--menu` offers the audio choices instead
/// of starting.
pub fn screenrecord(args: &[String]) {
    let running = util::pids_named(&RECORDERS);
    if !running.is_empty() {
        stop_recording(&running);
        return;
    }
    if args.iter().any(|arg| arg == "--stop") {
        return;
    }
    // The indicator and Alt+Print when idle: choose the audio first.
    if args.iter().any(|arg| arg == "--menu") {
        crate::menu::show("screenrecord");
        return;
    }
    crate::menu::before_capture();
    let (dirs, home) = user_dirs();
    let dir = videos_dir(&dirs, &home);
    if fs::create_dir_all(&dir).is_err() {
        util::notify(
            "\u{f0ec2}",
            "Screen recording failed",
            &format!("Cannot create {}", dir.display()),
        );
        return;
    }
    let monitors = hypr::json("monitors").unwrap_or_default();
    let clients = hypr::json("clients").unwrap_or_default();
    let candidates = hypr::workspace_rects(&monitors, &clients);
    let input: String = candidates.iter().map(|r| r.slurp() + "\n").collect();
    let Some(selection) = util::filter("slurp", &[], input.as_bytes())
        .ok()
        .and_then(|text| Rect::parse(&text))
        .map(|rect| smart_selection(rect, &candidates))
    else {
        return;
    };
    let target = record_target(selection, &monitors);
    let path = dir.join(format!("screenrecording-{}.mp4", timestamp(now())));
    let Some(file) = path.to_str() else {
        return;
    };
    let desktop = args.iter().any(|arg| arg == "--desktop-audio");
    let microphone = args.iter().any(|arg| arg == "--microphone");
    let audio = audio_sources(desktop, microphone);
    let gsr = gsr_args(&target, file, audio.as_deref());
    util::spawn(
        "gpu-screen-recorder",
        &gsr.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    // gpu-screen-recorder exits at once without hardware OpenGL.
    if !started(15) {
        let wf = wf_args(&target, file, desktop, microphone);
        util::spawn(
            "wf-recorder",
            &wf.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        started(5);
    }
    util::signal_waybar(RECORDING_SIGNAL);
    if !recording() {
        util::notify("\u{f0ec2}", "Screen recording failed to start", "");
    }
}

/// SIGINT, so the recorder finishes the file, then tell where it is.
fn stop_recording(pids: &[i32]) {
    for pid in pids {
        // SAFETY: kill(2) only sends a signal to the given process.
        unsafe {
            libc::kill(*pid, libc::SIGINT);
        }
    }
    for _ in 0..50 {
        if !recording() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    util::signal_waybar(RECORDING_SIGNAL);
    if recording() {
        util::terminate(&util::pids_named(&RECORDERS));
        util::notify(
            "\u{f0ec2}",
            "Screen recording error",
            "The recorder had to be stopped. The video may be incomplete.",
        );
    } else {
        let (dirs, home) = user_dirs();
        util::notify(
            "\u{f0ec2}",
            "Screen recording saved",
            &videos_dir(&dirs, &home).display().to_string(),
        );
    }
}

pub fn color_picker() {
    let running = util::pids_named(&["hyprpicker"]);
    if running.is_empty() {
        util::spawn("hyprpicker", &["-a"]);
    } else {
        util::terminate(&running);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_left_by_freezes_are_found() {
        let alive = |pid: u32| pid == 42;
        assert!(leaked_frame(".hyprpicker_ZVzUkA", alive));
        assert!(leaked_frame("tatami-freeze-7", alive));
        // A freeze still running, and everything else in the runtime directory.
        assert!(!leaked_frame("tatami-freeze-42", alive));
        for name in [
            "wayland-1",
            "tatami",
            "tatami-grab-7.png",
            "tatami-freeze-x",
            "hypr",
        ] {
            assert!(!leaked_frame(name, alive), "{name}");
        }
    }
    #[test]
    fn pictures_directory_from_user_dirs() {
        assert_eq!(
            pictures_dir("XDG_PICTURES_DIR=\"$HOME/Bilder\"\n", "/home/a"),
            PathBuf::from("/home/a/Bilder")
        );
        assert_eq!(
            pictures_dir("", "/home/a"),
            PathBuf::from("/home/a/Pictures")
        );
        assert_eq!(
            pictures_dir("XDG_PICTURES_DIR=\"relative\"", "/home/a"),
            PathBuf::from("/home/a/Pictures")
        );
    }
    #[test]
    fn videos_directory_from_user_dirs() {
        assert_eq!(
            videos_dir(
                "XDG_PICTURES_DIR=\"$HOME/P\"\nXDG_VIDEOS_DIR=\"$HOME/Filme\"\n",
                "/home/a"
            ),
            PathBuf::from("/home/a/Filme")
        );
        assert_eq!(videos_dir("", "/home/a"), PathBuf::from("/home/a/Videos"));
    }
    #[test]
    fn recordings_take_a_whole_monitor_by_name_or_a_region() {
        let monitors = serde_json::json!([
            {"name": "eDP-1", "x": 0, "y": 0, "width": 2880, "height": 1800, "scale": 2.0, "transform": 0}
        ]);
        let whole = Rect {
            x: 0,
            y: 0,
            w: 1440,
            h: 900,
        };
        let monitor = record_target(whole, &monitors);
        assert_eq!(monitor, Target::Monitor("eDP-1".into()));
        let part = Rect {
            x: 10,
            y: 20,
            w: 300,
            h: 200,
        };
        let region = record_target(part, &monitors);
        assert_eq!(region, Target::Region(part));
        let gsr = gsr_args(&region, "/v/a.mp4", Some("default_output"));
        assert_eq!(gsr[..2], ["-w", "300x200+10+20"]);
        assert_eq!(gsr[gsr.len() - 4..], ["-a", "default_output", "-ac", "aac"]);
        assert_eq!(gsr_args(&monitor, "/v/a.mp4", None)[1], "eDP-1");
        assert_eq!(
            wf_args(&region, "/v/a.mp4", true, true),
            [
                "-g",
                "10,20 300x200",
                "--audio=@DEFAULT_MONITOR@",
                "-f",
                "/v/a.mp4"
            ]
        );
        assert_eq!(
            wf_args(&monitor, "/v/a.mp4", false, true),
            ["-o", "eDP-1", "--audio=@DEFAULT_SOURCE@", "-f", "/v/a.mp4"]
        );
    }
    #[test]
    fn recording_audio_sources() {
        assert_eq!(audio_sources(false, false), None);
        assert_eq!(
            audio_sources(true, false).as_deref(),
            Some("default_output")
        );
        assert_eq!(
            audio_sources(true, true).as_deref(),
            Some("default_output|default_input")
        );
    }
    #[test]
    fn clicks_select_the_window_under_the_pointer() {
        let monitor = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let window = Rect {
            x: 100,
            y: 100,
            w: 800,
            h: 600,
        };
        let click = Rect {
            x: 200,
            y: 200,
            w: 1,
            h: 1,
        };
        assert_eq!(smart_selection(click, &[monitor, window]), window);
        let outside = Rect {
            x: 1500,
            y: 900,
            w: 2,
            h: 2,
        };
        assert_eq!(smart_selection(outside, &[monitor, window]), monitor);
        let drag = Rect {
            x: 5,
            y: 5,
            w: 300,
            h: 200,
        };
        assert_eq!(smart_selection(drag, &[monitor, window]), drag);
    }
    #[test]
    fn timestamps_are_civil_dates() {
        assert_eq!(timestamp(0), "1970-01-01_00-00-00");
        assert_eq!(timestamp(1_791_158_400), "2026-10-05_00-00-00");
        assert_eq!(timestamp(951_782_400 + 3661), "2000-02-29_01-01-01");
    }
}
