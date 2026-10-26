//! Which recorders can record here, found once a session, in the
//! background as the shell starts, so a recording starts at once with one
//! that works rather than seconds into ones that can't (gpu-screen-recorder
//! takes seconds to give up on software OpenGL, eight under load in a VM,
//! while the indicator counts for nothing):
//!
//! - gpu-screen-recorder needs hardware OpenGL, an H.264 encoder (the
//!   GPU's, or x264 to fall back on) and the displays through KMS:
//!   `gpu-screen-recorder --info` says all three, and which displays.
//! - wf-recorder's VA-API encoding needs an H.264 encoder on the render
//!   node it opens: `vainfo` says (without it, VA-API is tried and may
//!   make way for x264, as before).
//!
//! A recorder found able that then can't record is struck off for the
//! session, so the next recording starts with the one that did.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What was found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The displays gpu-screen-recorder can capture (empty: it can't
    /// record here).
    pub gsr_displays: Vec<String>,
    /// Whether the render node encodes H.264 through VA-API (`None`: no
    /// way to tell).
    pub vaapi: Option<bool>,
}

/// The render node wf-recorder encodes on (its default).
pub const RENDER_NODE: &str = "/dev/dri/renderD128";

/// How long a probe may take: gpu-screen-recorder giving up on software
/// OpenGL under load took eight seconds.
const PATIENCE: Duration = Duration::from_secs(30);

/// The session's probe: started once, read by recordings.
#[derive(Clone, Default)]
pub struct Probes {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    started: bool,
    found: Option<Found>,
}

impl Probes {
    /// Probes on a thread of its own, unless it has been already.
    pub fn start(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.started {
                return;
            }
            inner.started = true;
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new().name("recorder-probe".into()).spawn(move || {
            let t = Instant::now();
            let found = probe(super::on_path);
            log::info!("recorders: {found:?}, found in {:?}", t.elapsed());
            me.inner.lock().unwrap().found = Some(found);
        });
        if spawned.is_err() {
            // Nothing found: everything is tried, as before probes.
            self.inner.lock().unwrap().found = Some(Found { gsr_displays: Vec::new(), vaapi: None });
        }
    }

    /// What was found, once it has been.
    pub fn found(&self) -> Option<Found> {
        self.inner.lock().unwrap().found.clone()
    }

    /// A recorder that was found able but couldn't record: not again this
    /// session.
    pub fn strike(&self, recorder: &str) {
        let mut inner = self.inner.lock().unwrap();
        let Some(found) = inner.found.as_mut() else { return };
        match recorder {
            super::GSR => found.gsr_displays.clear(),
            super::WF_VAAPI => found.vaapi = Some(false),
            _ => return,
        }
        log::info!("recorders: {recorder} couldn't record; not trying it again");
    }
}

/// Runs the probes (`have`: whether a program is installed).
pub fn probe(have: impl Fn(&str) -> bool) -> Found {
    let gsr_displays = if have("gpu-screen-recorder") {
        match output_of("gpu-screen-recorder", &["--info"]) {
            Some(info) => gsr_displays(&info),
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let vaapi = if !have("wf-recorder") || !std::path::Path::new(RENDER_NODE).exists() {
        Some(false)
    } else if have("vainfo") {
        Some(output_of("vainfo", &["--display", "drm", "--device", RENDER_NODE]).is_some_and(|o| encodes_h264(&o)))
    } else {
        None
    };
    Found { gsr_displays, vaapi }
}

/// A program's output, if it succeeded in time.
fn output_of(prog: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // No desktop portal: gpu-screen-recorder would open a screencast
        // session to see whether it may use one.
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/aizome-probe")
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + PATIENCE;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                log::warn!("recorders: {prog} took too long; taking it as unable");
                // Our own unreaped child.
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader.join().unwrap_or_default();
    status.filter(|s| s.success()).map(|_| out)
}

/// The displays `gpu-screen-recorder --info` can capture, if it can
/// encode H.264 too (on the GPU, or with x264 to fall back on): its
/// capture options' `NAME|WxH` lines (cameras' start with `/`).
pub fn gsr_displays(info: &str) -> Vec<String> {
    let mut section = "";
    let (mut h264, mut displays) = (false, Vec::new());
    for line in info.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix("section=") {
            section = s;
            continue;
        }
        match section {
            "video_codecs" if line == "h264" || line == "h264_software" => h264 = true,
            "capture_options" => {
                if let Some((name, size)) = line.split_once('|')
                    && !name.starts_with('/')
                    && size.contains('x')
                {
                    displays.push(name.to_owned());
                }
            }
            _ => {}
        }
    }
    if h264 { displays } else { Vec::new() }
}

/// Whether `vainfo` lists an H.264 encoding entry point (full or low
/// power).
pub fn encodes_h264(vainfo: &str) -> bool {
    vainfo.lines().any(|l| l.contains("VAProfileH264") && l.contains("VAEntrypointEncSlice"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "section=system_info\ndisplay_server|wayland\nsupports_app_audio|yes\nis_steam_deck|no\n\
        gsr_version|6.1.3\nsection=gpu_info\nvendor|intel\ncard_path|/dev/dri/card0\nsection=video_codecs\nh264\n\
        h264_software\nhevc\nsection=image_formats\njpeg\npng\nsection=containers\nmp4\nmkv\nsection=capture_options\n\
        eDP-1|2880x1800\nDP-2|2560x1440\nregion\n/dev/video0|1280x720@30hz|yuyv\nportal\n";

    #[test]
    fn gpu_screen_recorder_says_what_it_can_capture() {
        assert_eq!(gsr_displays(INFO), ["eDP-1", "DP-2"]);
        // No H.264 at all, or no displays: nothing.
        assert!(gsr_displays(&INFO.replace("\nh264\n", "\n").replace("h264_software\n", "")).is_empty());
        let no_displays = INFO.replace("eDP-1|2880x1800\nDP-2|2560x1440\nregion\n", "");
        assert!(gsr_displays(&no_displays).is_empty());
        assert!(gsr_displays("").is_empty());
    }

    #[test]
    fn vainfo_says_whether_h264_encodes() {
        let lnl = "vainfo: Driver version: Intel iHD driver\nvainfo: Supported profile and entrypoints\n\
            \x20     VAProfileH264Main               :\tVAEntrypointVLD\n\
            \x20     VAProfileH264Main               :\tVAEntrypointEncSliceLP\n";
        assert!(encodes_h264(lnl));
        let decode_only = "      VAProfileH264Main               :\tVAEntrypointVLD\n\
            \x20     VAProfileHEVCMain               :\tVAEntrypointEncSlice\n";
        assert!(!encodes_h264(decode_only));
    }

    #[test]
    fn probing_without_recorders_finds_nothing() {
        assert_eq!(probe(|_| false), Found { gsr_displays: Vec::new(), vaapi: Some(false) });
    }

    #[test]
    fn a_recorder_that_couldnt_record_is_struck_off() {
        let p = Probes::default();
        p.inner.lock().unwrap().found = Some(Found { gsr_displays: vec!["eDP-1".into()], vaapi: Some(true) });
        p.strike(super::super::GSR);
        p.strike(super::super::WF_VAAPI);
        assert_eq!(p.found(), Some(Found { gsr_displays: Vec::new(), vaapi: Some(false) }));
    }
}
