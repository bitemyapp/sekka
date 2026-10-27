//! Sounds: the freedesktop sound theme's files where they're installed,
//! played with `pw-play` (PipeWire) or `paplay`, and a tone of Tokei's own
//! where there's no theme.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// The sounds an alarm can make: the theme's name, and what Tokei calls
/// it.
pub const SOUNDS: [(&str, &str); 6] = [
    ("alarm-clock-elapsed", "Alarm Clock"),
    ("phone-incoming-call", "Ringing"),
    ("bell", "Bell"),
    ("complete", "Chime"),
    ("message", "Message"),
    ("tone", "Tone"),
];

pub fn sound_name(id: &str) -> &'static str {
    SOUNDS.iter().find(|(i, _)| *i == id).map_or("Alarm Clock", |(_, n)| n)
}

/// Where data lives: `$XDG_DATA_HOME`, `$XDG_DATA_DIRS`, NixOS's system
/// profile.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(h));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share"));
    }
    if let Some(d) = std::env::var_os("XDG_DATA_DIRS") {
        dirs.extend(std::env::split_paths(&d));
    }
    dirs.extend(["/run/current-system/sw/share", "/usr/local/share", "/usr/share"].map(PathBuf::from));
    dirs
}

/// The sound theme's file for `id`, if one is installed.
pub fn sound_file(id: &str) -> Option<PathBuf> {
    if id == "tone" {
        return None;
    }
    for dir in data_dirs() {
        for ext in ["oga", "ogg", "wav"] {
            let p = dir.join("sounds/freedesktop/stereo").join(format!("{id}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// A program on `$PATH`, or in NixOS's profiles (a timer's service may
/// run with a short `$PATH`).
fn on_path(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(user) = std::env::var_os("USER") {
        dirs.push(PathBuf::from("/etc/profiles/per-user").join(user).join("bin"));
    }
    dirs.extend(["/run/current-system/sw/bin", "/usr/bin"].map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// Tokei's own tone: two short beeps at 880 Hz, a 16-bit mono WAV.
pub fn tone_wav() -> Vec<u8> {
    let rate = 22_050u32;
    let mut samples: Vec<i16> = Vec::new();
    let beep = |samples: &mut Vec<i16>, ms: u32| {
        let n = rate * ms / 1000;
        for i in 0..n {
            let t = i as f32 / rate as f32;
            // A soft start and end, so it doesn't click.
            let edge = (i.min(n - i) as f32 / (rate as f32 * 0.01)).min(1.0);
            let v = (t * 880.0 * std::f32::consts::TAU).sin() * 0.45 * edge;
            samples.push((v * i16::MAX as f32) as i16);
        }
    };
    let silence = |samples: &mut Vec<i16>, ms: u32| samples.extend(std::iter::repeat_n(0, (rate * ms / 1000) as usize));
    beep(&mut samples, 140);
    silence(&mut samples, 90);
    beep(&mut samples, 140);
    silence(&mut samples, 500);
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

/// The tone, written once to the runtime directory.
fn tone_file() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let p = dir.join("aizome-tokei-tone.wav");
    if !p.is_file() {
        std::fs::write(&p, tone_wav()).ok()?;
    }
    Some(p)
}

/// Plays sounds.
pub trait Player {
    /// Start playing `id` once; whether it started.
    fn start(&mut self, id: &str) -> bool;
    /// Whether the last sound is still playing.
    fn playing(&mut self) -> bool;
    fn stop(&mut self);
}

/// The system's player: `pw-play`, else `paplay`, else `aplay`.
#[derive(Default)]
pub struct System {
    child: Option<Child>,
}

impl Player for System {
    fn start(&mut self, id: &str) -> bool {
        self.stop();
        let Some(file) = sound_file(id).or_else(tone_file) else { return false };
        let Some(player) = ["pw-play", "paplay", "aplay"].iter().find_map(|p| on_path(p)) else {
            log::warn!("no pw-play, paplay or aplay to play {}", file.display());
            return false;
        };
        match Command::new(player).arg(&file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(c) => {
                self.child = Some(c);
                true
            }
            Err(e) => {
                log::warn!("playing {}: {e}", file.display());
                false
            }
        }
    }

    fn playing(&mut self) -> bool {
        match &mut self.child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for System {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A player that plays nothing and counts: for tests.
#[derive(Default)]
pub struct Silent {
    pub started: Vec<String>,
    pub stopped: usize,
}

impl Player for Silent {
    fn start(&mut self, id: &str) -> bool {
        self.started.push(id.to_owned());
        true
    }

    fn playing(&mut self) -> bool {
        false
    }

    fn stop(&mut self) {
        self.stopped += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tone_is_a_wav() {
        let w = tone_wav();
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        let len = u32::from_le_bytes([w[40], w[41], w[42], w[43]]) as usize;
        assert_eq!(w.len(), 44 + len);
        assert!(w.len() > 22_050, "most of a second");
    }

    #[test]
    fn every_sound_has_a_name() {
        assert_eq!(sound_name("bell"), "Bell");
        assert_eq!(sound_name("nonsense"), "Alarm Clock");
        assert!(sound_file("tone").is_none());
    }
}
