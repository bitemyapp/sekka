//! Sounds the shell makes: the alert (the system bell, at `[sound]`'s
//! alert sound and volume), the sound a notification asks for (at the alert
//! volume), and feedback when the volume keys or the Sound panel change
//! the volume (at the new volume). Named sounds come from the freedesktop
//! sound theme where it's installed; files play through PipeWire with
//! `pw-play` (else `paplay`, else `aplay`, which can't set a volume), each
//! in a process of its own, reaped on a thread.
//!
//! `pw-play` asks for short graph cycles ([`LATENCY`]), and the whole graph
//! runs at those while a sound plays, so with nothing on long cycles a
//! sound starts within one. Over Utsushi's music it still waits for the
//! sound card to play out the long cycles it has already queued, one to
//! two of them (170 to 340 ms at 8192 frames): PipeWire doesn't drop them.
//! Left to its default (100 ms), `pw-play` would take whatever cycle up to
//! that the graph allows: with Aizome's long-cycle cap, 4096 frames.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use aizome_config::SoundSettings;
use aizome_kit::prelude::*;

use crate::shell::{Msg, Shell};

/// The theme's sound for volume feedback (the alert where it's missing).
pub const FEEDBACK: &str = "audio-volume-change";
/// How long after a volume change its feedback plays: long enough for the
/// sound server to have taken the new volume.
pub const FEEDBACK_AFTER: Duration = Duration::from_millis(60);
/// The graph cycle `pw-play` asks for: 20 ms, which PipeWire runs as its
/// usual 1024 frames at 48 kHz, against music's 8192. While a sound plays,
/// whatever else is playing runs at that too.
pub const LATENCY: &str = "20ms";

/// Where the sound theme's files may be: `$XDG_DATA_HOME`,
/// `$XDG_DATA_DIRS`, NixOS's system profile, `/usr/share`.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(h));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    if let Some(d) = std::env::var_os("XDG_DATA_DIRS") {
        dirs.extend(std::env::split_paths(&d));
    }
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    dirs.push(PathBuf::from("/usr/share"));
    dirs
}

/// The theme's file for a sound name (`message-new-instant`, `bell`).
pub fn theme_file(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    for dir in data_dirs() {
        for ext in ["oga", "ogg", "wav"] {
            let p = dir.join("sounds/freedesktop/stereo").join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The alert's name: the one chosen in Sound, else `bell`.
pub fn alert_name(cfg: &SoundSettings) -> &str {
    let name = cfg.alert_sound.trim();
    if name.is_empty() { "bell" } else { name }
}

/// The alert's file: the one chosen, else the theme's bell.
pub fn alert_file(cfg: &SoundSettings) -> Option<PathBuf> {
    theme_file(alert_name(cfg)).or_else(|| theme_file("bell"))
}

/// A player's command line for a file at a volume (0–1).
pub fn command(player: &str, file: &std::path::Path, volume: f64) -> Vec<String> {
    let volume = volume.clamp(0.0, 1.0);
    let file = file.to_string_lossy().into_owned();
    match player {
        "pw-play" => {
            vec!["pw-play".into(), "--volume".into(), format!("{volume:.2}"), "--latency".into(), LATENCY.into(), file]
        }
        // PulseAudio's scale: 65536 is 100%.
        "paplay" => vec!["paplay".into(), format!("--volume={}", (volume * 65536.0).round() as u32), file],
        other => vec![other.into(), file],
    }
}

/// Play a sound file at a volume (0–1, of the output's), without waiting.
pub fn play(file: PathBuf, volume: f64) {
    let _ = std::thread::Builder::new().name("aizome-sound".into()).spawn(move || {
        for player in ["pw-play", "paplay", "aplay"] {
            let argv = command(player, &file, volume);
            match Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(_) => return,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    log::warn!("{player} {}: {e}", file.display());
                    return;
                }
            }
        }
        log::debug!("no pw-play, paplay or aplay for {}", file.display());
    });
}

impl Shell {
    /// Play `file` at `volume`; `what` is what tests see it as
    /// (`last_sound`: `bell at 0.70`). Silent at volume 0.
    pub fn play_sound(&mut self, what: &str, file: Option<PathBuf>, volume: f64) {
        if volume <= 0.0 {
            return;
        }
        self.last_sound = Some(format!("{what} at {volume:.2}"));
        if self.opts.live
            && let Some(f) = file
        {
            play(f, volume);
        }
    }

    /// The alert sound, at the alert volume.
    pub fn alert_sound(&mut self) {
        let cfg = self.sound_cfg.clone();
        self.play_sound(alert_name(&cfg), alert_file(&cfg), cfg.alert_level());
    }

    /// The volume changed (a key, the Sound panel): with feedback on, play
    /// it at the new volume, a moment later, once the sound server has it;
    /// nothing while the output is muted.
    pub fn volume_feedback(&mut self, muted: bool, cx: &mut Cx<Msg>) {
        if !self.sound_cfg.volume_feedback || muted {
            return;
        }
        cx.after(FEEDBACK_AFTER, Msg::VolumeFeedback);
    }

    /// The feedback's moment: the output's volume is applied to it, so the
    /// stream plays at full.
    pub fn play_feedback(&mut self) {
        let muted = self.sys.audio.as_ref().and_then(|a| a.snapshot().default_output().map(|d| d.muted));
        if muted == Some(true) {
            return;
        }
        let file = theme_file(FEEDBACK).or_else(|| alert_file(&self.sound_cfg));
        self.play_sound(FEEDBACK, file, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_are_told_the_volume() {
        let f = std::path::Path::new("/s/bell.oga");
        assert_eq!(command("pw-play", f, 0.7), ["pw-play", "--volume", "0.70", "--latency", "20ms", "/s/bell.oga"]);
        assert_eq!(command("paplay", f, 0.5), ["paplay", "--volume=32768", "/s/bell.oga"]);
        assert_eq!(command("pw-play", f, 3.0)[2], "1.00");
        assert_eq!(command("aplay", f, 0.5), ["aplay", "/s/bell.oga"]);
    }

    #[test]
    fn the_alert_is_the_one_chosen_else_the_bell() {
        let mut cfg = SoundSettings::default();
        assert_eq!(alert_name(&cfg), "bell");
        cfg.alert_sound = "message".into();
        assert_eq!(alert_name(&cfg), "message");
        cfg.alert_sound = " ".into();
        assert_eq!(alert_name(&cfg), "bell");
    }
}
