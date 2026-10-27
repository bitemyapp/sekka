//! Screen recordings, as Tatami makes them: a display or an area, with
//! what the computer plays and the microphone if asked, into
//! `~/Videos/Recordings` (`XDG_VIDEOS_DIR`).
//!
//! The recorder is gpu-screen-recorder: it reads the display's scan-out
//! buffers through KMS (its `gsr-kms-server`, which NixOS gives
//! CAP_SYS_ADMIN), converts and encodes them on the GPU (VA-API, NVENC)
//! and draws the pointer from its plane, so the compositor and the CPU do
//! nothing per frame. Where it can't run (no hardware OpenGL, no capture
//! rights) wf-recorder takes over through the compositor's wlr-screencopy:
//! with a VA-API encoder the frames stay in GPU memory (dmabuf), else
//! they're read back and encoded with x264.
//!
//! Which of these can record here is found once a session, as the shell
//! starts ([`probe`]), so a recording starts with that one; another is
//! tried only if it then fails.
//!
//! This module decides what to run; [`run`] runs it and watches it; the
//! shell's part (indicator, palette, menu, notification) is in
//! `recording_glue`.

pub mod probe;
pub mod run;

use std::path::{Path, PathBuf};
use std::time::Duration;

use aizome_config::{Recorder, RecordingFormat, RecordingQuality, RecordingSettings};
use aizome_ipc::{OutputInfo, Rect};

/// What a recording takes in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The display with the focused window (or the main one).
    Screen,
    /// The person picks with the screenshot picker: an area, a display
    /// (a click) or a window (Space).
    Pick,
    /// A display, by its connector's name.
    Output(String),
    /// An area of the global layout, in points.
    Area(Rect),
}

/// What to hear: what the computer plays (the default output's monitor)
/// and the microphone (the default input).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Audio {
    pub desktop: bool,
    pub microphone: bool,
}

/// Recording asked for: a target, and audio (`None`: the settings').
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub target: Target,
    pub audio: Option<Audio>,
}

/// What the recorder captures, resolved against the outputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capture {
    /// All of a display.
    Output(String),
    /// Part of one display: in the global layout (points).
    Area { output: String, rect: Rect },
}

impl Capture {
    pub fn output(&self) -> &str {
        match self {
            Capture::Output(o) | Capture::Area { output: o, .. } => o,
        }
    }
}

/// A recording, decided: what, with which audio, into which file, how.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub capture: Capture,
    pub audio: Audio,
    pub path: PathBuf,
    pub settings: RecordingSettings,
}

/// An area picked, as a capture: the whole display when it covers one
/// (a click, or a drag over all of it), else the area within the display
/// holding its centre.
pub fn capture_for(area: Rect, output: Option<&str>, outputs: &[OutputInfo]) -> Option<Capture> {
    let centre = (area.x + area.w / 2, area.y + area.h / 2);
    let inside = |r: &Rect| centre.0 >= r.x && centre.0 < r.x + r.w && centre.1 >= r.y && centre.1 < r.y + r.h;
    let o = output
        .and_then(|n| outputs.iter().find(|o| o.name == n))
        .or_else(|| outputs.iter().find(|o| o.enabled && inside(&o.logical)))?;
    let l = o.logical;
    if area == l {
        return Some(Capture::Output(o.name.clone()));
    }
    // Within that display.
    let x0 = area.x.clamp(l.x, l.x + l.w);
    let y0 = area.y.clamp(l.y, l.y + l.h);
    let x1 = (area.x + area.w).clamp(l.x, l.x + l.w);
    let y1 = (area.y + area.h).clamp(l.y, l.y + l.h);
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return None;
    }
    let rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
    if rect == l {
        return Some(Capture::Output(o.name.clone()));
    }
    Some(Capture::Area { output: o.name.clone(), rect })
}

/// Where recordings go: the setting's folder, else `Recordings` in the
/// Videos folder (`$XDG_VIDEOS_DIR`, else `user-dirs.dirs`, else
/// `~/Videos`).
pub fn folder(settings: &RecordingSettings, env_videos: Option<&str>, user_dirs: Option<&str>, home: &Path) -> PathBuf {
    if let Some(f) = settings.folder.as_deref().filter(|f| !f.trim().is_empty()) {
        return aizome_config::expand_home(f);
    }
    let videos = env_videos
        .filter(|v| !v.is_empty())
        .map(|v| PathBuf::from(v.replace("$HOME", &home.to_string_lossy())))
        .or_else(|| {
            let line = user_dirs?.lines().find(|l| l.trim_start().starts_with("XDG_VIDEOS_DIR="))?;
            let value = line.trim_start()["XDG_VIDEOS_DIR=".len()..].trim().trim_matches('"');
            Some(PathBuf::from(value.replace("$HOME", &home.to_string_lossy())))
        })
        .unwrap_or_else(|| home.join("Videos"));
    videos.join("Recordings")
}

/// Tatami's name for a recording: `screenrecording-2026-10-18_14-05-09.mp4`
/// (local time), with ` (2)` and on when taken.
pub fn file_in(dir: &Path, now: crate::clock::Local, format: RecordingFormat) -> PathBuf {
    let stem = format!(
        "screenrecording-{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        now.year, now.month, now.day, now.hour, now.minute, now.second
    );
    let ext = format.extension();
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// gpu-screen-recorder's quality names.
fn gsr_quality(q: RecordingQuality) -> &'static str {
    match q {
        RecordingQuality::Medium => "medium",
        RecordingQuality::High => "high",
        RecordingQuality::VeryHigh => "very_high",
        RecordingQuality::Ultra => "ultra",
    }
}

/// gpu-screen-recorder's command line: KMS capture of a display (by name)
/// or of an area (`WxH+X+Y` in the global layout's points, which it maps
/// to the display holding the area's centre), H.264 on the GPU, the CPU
/// only where the GPU has no encoder.
pub fn gsr_argv(plan: &Plan) -> Vec<String> {
    let s = &plan.settings;
    let window = match &plan.capture {
        Capture::Output(name) => name.clone(),
        Capture::Area { rect: r, .. } => format!("{}x{}+{}+{}", r.w, r.h, r.x, r.y),
    };
    let mut argv: Vec<String> = vec!["gpu-screen-recorder".into(), "-w".into(), window];
    let fps = s.framerate.clamp(1, 240).to_string();
    let format = s.format.extension();
    for (k, v) in [
        ("-c", format),
        ("-k", "h264"),
        ("-f", fps.as_str()),
        // Constant frame rate, as video editors want.
        ("-fm", "cfr"),
        ("-q", gsr_quality(s.quality)),
        ("-cursor", if s.cursor { "yes" } else { "no" }),
        ("-fallback-cpu-encoding", "yes"),
        // No per-second frame-rate line on stderr.
        ("-v", "no"),
    ] {
        argv.push(k.into());
        argv.push(v.into());
    }
    let sources: Vec<&str> = [(plan.audio.desktop, "default_output"), (plan.audio.microphone, "default_input")]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, s)| s)
        .collect();
    if !sources.is_empty() {
        // Both mixed into one track.
        argv.extend(["-a".into(), sources.join("|")]);
        let codec = match s.format {
            RecordingFormat::Mp4 => "aac",
            RecordingFormat::Mkv => "opus",
        };
        argv.extend(["-ac".into(), codec.into()]);
    }
    argv.extend(["-o".into(), plan.path.to_string_lossy().into_owned()]);
    argv
}

/// wf-recorder's command line, through the compositor's screencopy. With
/// `vaapi` the frames go to the GPU's encoder as dmabufs and never reach
/// the CPU; without, they're read back and x264 encodes them, at its
/// fastest presets so a large display keeps up. It takes one audio source:
/// what the computer plays before the microphone.
pub fn wf_argv(plan: &Plan, vaapi: bool) -> Vec<String> {
    let s = &plan.settings;
    let mut argv: Vec<String> = vec!["wf-recorder".into(), "-y".into()];
    match &plan.capture {
        Capture::Output(name) => argv.extend(["-o".into(), name.clone()]),
        Capture::Area { output, rect: r } => {
            argv.extend(["-o".into(), output.clone(), "-g".into(), format!("{},{} {}x{}", r.x, r.y, r.w, r.h)])
        }
    }
    argv.extend(["-r".into(), s.framerate.clamp(1, 240).to_string()]);
    let (qp, crf) = match s.quality {
        RecordingQuality::Medium => (28, 28),
        RecordingQuality::High => (24, 24),
        RecordingQuality::VeryHigh => (21, 21),
        RecordingQuality::Ultra => (18, 17),
    };
    if vaapi {
        argv.extend(["-c".into(), "h264_vaapi".into(), "-p".into(), format!("qp={qp}")]);
    } else {
        argv.extend(["-c".into(), "libx264".into(), "-p".into(), "preset=superfast".into()]);
        argv.extend(["-p".into(), format!("crf={crf}")]);
        argv.extend(["-x".into(), "yuv420p".into()]);
    }
    let source = if plan.audio.desktop {
        Some("@DEFAULT_MONITOR@")
    } else if plan.audio.microphone {
        Some("@DEFAULT_SOURCE@")
    } else {
        None
    };
    if let Some(src) = source {
        argv.push(format!("--audio={src}"));
        let codec = match s.format {
            RecordingFormat::Mp4 => "aac",
            RecordingFormat::Mkv => "libopus",
        };
        argv.extend(["-C".into(), codec.into()]);
    }
    argv.extend(["-f".into(), plan.path.to_string_lossy().into_owned()]);
    argv
}

/// The recorders' names, as attempts and the indicator's state give them.
pub const GSR: &str = "gpu-screen-recorder";
pub const WF_VAAPI: &str = "wf-recorder (VA-API)";
pub const WF: &str = "wf-recorder";

/// What to try, in order: gpu-screen-recorder, then wf-recorder on the
/// GPU's encoder, then wf-recorder with x264 (as the setting allows, and
/// as `have` says is installed). With what the probe `found`, only what
/// can record here, unless nothing can (then everything, for its error).
pub fn attempts(plan: &Plan, have: impl Fn(&str) -> bool, found: Option<&probe::Found>) -> Vec<run::Attempt> {
    let gsr = matches!(plan.settings.recorder, Recorder::Auto | Recorder::GpuScreenRecorder);
    let wf = matches!(plan.settings.recorder, Recorder::Auto | Recorder::WfRecorder);
    let mut all = Vec::new();
    if gsr && have(GSR) {
        all.push(run::Attempt { recorder: GSR, argv: gsr_argv(plan) });
    }
    if wf && have(WF) {
        all.push(run::Attempt { recorder: WF_VAAPI, argv: wf_argv(plan, true) });
        all.push(run::Attempt { recorder: WF, argv: wf_argv(plan, false) });
    }
    let Some(found) = found else { return all };
    let able = |a: &run::Attempt| match a.recorder {
        GSR => found.gsr_displays.iter().any(|d| d == plan.capture.output()),
        WF_VAAPI => found.vaapi != Some(false),
        _ => true,
    };
    let some: Vec<run::Attempt> = all.iter().filter(|a| able(a)).cloned().collect();
    if some.is_empty() { all } else { some }
}

/// Whether a program is on `PATH`.
pub fn on_path(prog: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| {
            let f = d.join(prog);
            std::fs::metadata(&f).is_ok_and(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.is_file() && m.permissions().mode() & 0o111 != 0
            })
        })
    })
}

/// The indicator's time: `0:42`, `12:05`, `1:02:09`.
pub fn elapsed_label(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The palette's and the Capture menu's names for a recording.
pub fn ask_label(ask: &Ask) -> String {
    let what = match ask.target {
        Target::Pick | Target::Area(_) => "Record Area",
        Target::Screen | Target::Output(_) => "Record Screen",
    };
    match ask.audio {
        None => what.to_owned(),
        Some(Audio { desktop: false, microphone: false }) => format!("{what} without Audio"),
        Some(Audio { desktop: true, microphone: false }) => format!("{what} with Desktop Audio"),
        Some(Audio { desktop: false, microphone: true }) => format!("{what} with Microphone"),
        Some(Audio { desktop: true, microphone: true }) => format!("{what} with Desktop Audio and Microphone"),
    }
}

/// Where a recording is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The picker is up.
    Picking,
    /// A recorder is starting (or making way for the next one).
    Starting,
    Recording,
    /// Asked to stop: the recorder is finishing the file.
    Stopping,
}

/// A recording, from asking for it to its file.
#[derive(Debug)]
pub struct Live {
    pub ask: Ask,
    pub phase: Phase,
    pub path: Option<PathBuf>,
    /// The recorder running.
    pub recorder: Option<String>,
    /// Since when it has been running (the indicator's clock).
    pub since: Option<std::time::Instant>,
    pub handle: Option<run::Handle>,
    /// Which recording the thread's messages are about.
    pub serial: u64,
    /// The indicator's tick in hand.
    pub tick: u64,
}

/// The tag of the shell's `pick-area` requests.
pub const PICK_TAG: &str = "record";

#[cfg(test)]
mod tests {
    use super::*;

    fn out(name: &str, x: i32, w: i32, h: i32) -> OutputInfo {
        OutputInfo { name: name.into(), enabled: true, logical: Rect::new(x, 0, w, h), ..Default::default() }
    }

    fn plan(capture: Capture, audio: Audio) -> Plan {
        Plan { capture, audio, path: PathBuf::from("/v/Recordings/a.mp4"), settings: RecordingSettings::default() }
    }

    #[test]
    fn picks_become_displays_or_areas() {
        let outputs = [out("eDP-1", 0, 1440, 900), out("DP-1", 1440, 1920, 1080)];
        // A click (the whole display) and a drag over all of it.
        assert_eq!(
            capture_for(Rect::new(0, 0, 1440, 900), Some("eDP-1"), &outputs),
            Some(Capture::Output("eDP-1".into()))
        );
        assert_eq!(capture_for(Rect::new(1440, 0, 1920, 1080), None, &outputs), Some(Capture::Output("DP-1".into())));
        // An area, and one straddling two displays: cut to the one with its
        // centre.
        let a = Rect::new(100, 120, 400, 300);
        assert_eq!(capture_for(a, None, &outputs), Some(Capture::Area { output: "eDP-1".into(), rect: a }));
        assert_eq!(
            capture_for(Rect::new(1300, 100, 600, 200), None, &outputs),
            Some(Capture::Area { output: "DP-1".into(), rect: Rect::new(1440, 100, 460, 200) })
        );
        assert_eq!(capture_for(Rect::new(5000, 5000, 10, 10), None, &outputs), None);
    }

    #[test]
    fn recordings_go_to_the_videos_folder() {
        let home = Path::new("/home/a");
        let s = RecordingSettings::default();
        assert_eq!(folder(&s, None, None, home), PathBuf::from("/home/a/Videos/Recordings"));
        assert_eq!(folder(&s, Some("/media/films"), None, home), PathBuf::from("/media/films/Recordings"));
        let dirs = "XDG_PICTURES_DIR=\"$HOME/P\"\nXDG_VIDEOS_DIR=\"$HOME/Filme\"\n";
        assert_eq!(folder(&s, None, Some(dirs), home), PathBuf::from("/home/a/Filme/Recordings"));
        let s = RecordingSettings { folder: Some("/tmp/caps".into()), ..s };
        assert_eq!(folder(&s, Some("/x"), Some(dirs), home), PathBuf::from("/tmp/caps"));
    }

    #[test]
    fn files_are_named_like_tatamis() {
        let dir = std::env::temp_dir().join(format!("aizome-rec-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = crate::clock::Local { year: 2026, month: 10, day: 5, hour: 9, minute: 3, second: 7, weekday: 1 };
        let p = file_in(&dir, t, RecordingFormat::Mp4);
        assert_eq!(p.file_name().unwrap(), "screenrecording-2026-10-05_09-03-07.mp4");
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(
            file_in(&dir, t, RecordingFormat::Mp4).file_name().unwrap(),
            "screenrecording-2026-10-05_09-03-07 (2).mp4"
        );
        assert_eq!(file_in(&dir, t, RecordingFormat::Mkv).extension().unwrap(), "mkv");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gpu_screen_recorder_takes_a_display_or_an_area_and_both_audio_sources() {
        let p = plan(Capture::Output("eDP-1".into()), Audio::default());
        let a = gsr_argv(&p);
        assert_eq!(a[..3], ["gpu-screen-recorder", "-w", "eDP-1"]);
        assert!(a.windows(2).any(|w| w == ["-f", "60"]));
        assert!(a.windows(2).any(|w| w == ["-cursor", "yes"]));
        assert!(!a.contains(&"-a".to_string()));
        assert_eq!(a[a.len() - 2..], ["-o", "/v/Recordings/a.mp4"]);
        let p = plan(
            Capture::Area { output: "DP-1".into(), rect: Rect::new(1500, 20, 640, 360) },
            Audio { desktop: true, microphone: true },
        );
        let a = gsr_argv(&p);
        assert_eq!(a[2], "640x360+1500+20");
        assert!(a.windows(2).any(|w| w == ["-a", "default_output|default_input"]));
        assert!(a.windows(2).any(|w| w == ["-ac", "aac"]));
    }

    #[test]
    fn wf_recorder_takes_one_audio_source_and_vaapi_or_x264() {
        let p = plan(
            Capture::Area { output: "eDP-1".into(), rect: Rect::new(10, 20, 300, 200) },
            Audio { desktop: true, microphone: true },
        );
        let a = wf_argv(&p, true);
        assert!(a.windows(4).any(|w| w == ["-o", "eDP-1", "-g", "10,20 300x200"]));
        assert!(a.windows(2).any(|w| w == ["-c", "h264_vaapi"]));
        assert!(a.contains(&"--audio=@DEFAULT_MONITOR@".to_string()));
        assert!(!a.iter().any(|s| s.contains("DEFAULT_SOURCE")));
        let p = plan(Capture::Output("eDP-1".into()), Audio { desktop: false, microphone: true });
        let a = wf_argv(&p, false);
        assert!(a.windows(2).any(|w| w == ["-c", "libx264"]));
        assert!(a.contains(&"--audio=@DEFAULT_SOURCE@".to_string()));
        assert!(a.windows(2).any(|w| w == ["-r", "60"]));
        assert_eq!(a[a.len() - 2..], ["-f", "/v/Recordings/a.mp4"]);
    }

    #[test]
    fn what_to_try_follows_the_setting_and_whats_installed() {
        let p = plan(Capture::Output("eDP-1".into()), Audio::default());
        let names = |p: &Plan, have: &dyn Fn(&str) -> bool| {
            attempts(p, have, None).into_iter().map(|a| a.recorder).collect::<Vec<_>>()
        };
        assert_eq!(names(&p, &|_| true), ["gpu-screen-recorder", "wf-recorder (VA-API)", "wf-recorder"]);
        assert_eq!(names(&p, &|n| n == "wf-recorder"), ["wf-recorder (VA-API)", "wf-recorder"]);
        let mut only_gsr = p.clone();
        only_gsr.settings.recorder = Recorder::GpuScreenRecorder;
        assert_eq!(names(&only_gsr, &|_| true), ["gpu-screen-recorder"]);
        assert!(names(&p, &|_| false).is_empty());
    }

    /// With the probe's findings, a recording starts with what can record
    /// here: no seconds lost to recorders that can't.
    #[test]
    fn what_to_try_follows_what_the_probe_found() {
        let p = plan(Capture::Output("eDP-1".into()), Audio::default());
        let names = |p: &Plan, found: probe::Found| {
            attempts(p, |_| true, Some(&found)).into_iter().map(|a| a.recorder).collect::<Vec<_>>()
        };
        let found = |displays: &[&str], vaapi| probe::Found {
            gsr_displays: displays.iter().map(|d| d.to_string()).collect(),
            vaapi,
        };
        assert_eq!(names(&p, found(&["eDP-1"], Some(true))), [GSR, WF_VAAPI, WF]);
        // A VM: no hardware OpenGL, no VA-API device.
        assert_eq!(names(&p, found(&[], Some(false))), [WF]);
        // gpu-screen-recorder can't capture this display (a nested session's).
        assert_eq!(names(&p, found(&["DP-1"], None)), [WF_VAAPI, WF]);
        // Asked for gpu-screen-recorder alone, which can't: tried anyway, for
        // its error.
        let mut only_gsr = p.clone();
        only_gsr.settings.recorder = Recorder::GpuScreenRecorder;
        assert_eq!(names(&only_gsr, found(&[], Some(false))), [GSR]);
    }

    #[test]
    fn labels() {
        assert_eq!(elapsed_label(Duration::from_secs(42)), "0:42");
        assert_eq!(elapsed_label(Duration::from_secs(725)), "12:05");
        assert_eq!(elapsed_label(Duration::from_secs(3729)), "1:02:09");
        let ask = Ask { target: Target::Pick, audio: Some(Audio { desktop: true, microphone: false }) };
        assert_eq!(ask_label(&ask), "Record Area with Desktop Audio");
        assert_eq!(ask_label(&Ask { target: Target::Screen, audio: None }), "Record Screen");
    }
}
