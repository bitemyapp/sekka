//! The system services the systray, its panels, the OSD and the lock screen
//! follow, from `aizome-sys`: each one wakes the shell's loop when its
//! state changes, and the shell reads snapshots.

use aizome_config::MenubarSettings;
use aizome_sys::Wake;
use aizome_sys::audio::Audio;
use aizome_sys::bluetooth::Bluetooth;
use aizome_sys::mpris::Mpris;
use aizome_sys::network::Network;
use aizome_sys::power::Backlight;
use aizome_sys::power::Logind;
use aizome_sys::power::PowerProfiles;
use aizome_sys::power::Upower;
use aizome_sys::statusnotifier::StatusNotifier;

use crate::menubar::{NetGlyph, Tray};
use crate::systray::panels::{Media, MediaVerb};

/// The services, when the shell runs for real (none in tests).
#[derive(Default)]
pub struct Sys {
    pub network: Option<Network>,
    pub audio: Option<Audio>,
    pub bluetooth: Option<Bluetooth>,
    pub upower: Option<Upower>,
    pub backlight: Option<Backlight>,
    pub profiles: Option<PowerProfiles>,
    pub logind: Option<Logind>,
    pub disks: Option<aizome_sys::disks::Disks>,
    /// Media players on the session bus, followed as they change, for the
    /// sound panel and the media keys.
    pub mpris: Option<Mpris>,
    /// Apps' status items: the shell serves the watcher they register
    /// with.
    pub status: Option<StatusNotifier>,
}

impl Sys {
    /// Start following every service; `wake` is called on each change.
    pub fn start(wake: impl Fn() + Send + Sync + Clone + 'static) -> Sys {
        let w = || Wake::new(wake.clone());
        Sys {
            network: Some(Network::new(w())),
            audio: Some(Audio::new(w())),
            bluetooth: Some(Bluetooth::new(w())),
            upower: Some(Upower::new(w())),
            backlight: Some(Backlight::new(w())),
            profiles: Some(PowerProfiles::new(w())),
            logind: Some(Logind::new(w())),
            disks: Some(aizome_sys::disks::Disks::new(w())),
            mpris: Some(Mpris::new(w())),
            status: Some(StatusNotifier::new(w())),
        }
    }

    /// What the sound panel shows of the player playing (or the one that
    /// played last): its track, or its name while it plays something
    /// untitled.
    pub fn media(&self) -> Option<Media> {
        let s = self.mpris.as_ref()?.snapshot();
        let p = s.active_player()?;
        let titled = !p.metadata.title.is_empty();
        (titled || p.is_playing()).then(|| Media {
            title: if titled { p.metadata.title.clone() } else { p.identity.clone() },
            artist: p.metadata.artist(),
            playing: p.is_playing(),
        })
    }

    /// ⏮ ⏯ ⏭ on that player.
    pub fn media_control(&self, verb: MediaVerb) {
        let Some(m) = &self.mpris else { return };
        let _ = match verb {
            MediaVerb::Previous => m.media_previous(),
            MediaVerb::PlayPause => m.media_play_pause(),
            MediaVerb::Next => m.media_next(),
        };
    }

    /// The processes playing sound now (streams that aren't paused),
    /// sorted.
    pub fn playing_pids(&self) -> Vec<u32> {
        let Some(a) = self.audio.as_ref().filter(|a| a.available()) else { return Vec::new() };
        let mut pids: Vec<u32> = a.snapshot().playback.iter().filter(|s| !s.corked).filter_map(|s| s.pid).collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// What the systray shows now.
    pub fn tray(&self, cfg: &MenubarSettings) -> Tray {
        let mut t = Tray {
            cpu: cfg.shows("cpu"),
            memory: cfg.shows("memory"),
            battery_glyph_only: !cfg.battery_percent,
            ..Tray::default()
        };
        if cfg.shows("network")
            && let Some(n) = &self.network
            && n.available()
        {
            let s = n.snapshot();
            t.network = Some(net_glyph(&s));
        }
        if cfg.shows("sound")
            && let Some(a) = &self.audio
            && a.available()
        {
            let s = a.snapshot();
            if let Some(out) = s.default_output() {
                t.sound = Some((out.volume, out.muted));
            }
        }
        if cfg.shows("bluetooth")
            && let Some(b) = &self.bluetooth
            && b.available()
        {
            let s = b.snapshot();
            if s.default_adapter().is_some() {
                t.bluetooth = Some((s.powered(), s.connected()));
            }
        }
        if cfg.shows("display")
            && let Some(b) = &self.backlight
            && b.available()
        {
            let s = b.snapshot();
            if let Some(d) = s.primary() {
                t.display = Some(d.fraction());
            }
        }
        if cfg.shows("battery")
            && let Some(u) = &self.upower
            && u.available()
        {
            let s = u.snapshot();
            if let Some(d) = s.display.as_ref().filter(|d| d.is_present) {
                use aizome_sys::power::BatteryState;
                let charging = matches!(d.state, BatteryState::Charging | BatteryState::FullyCharged)
                    || (!s.on_battery && s.plugged_in());
                t.battery = Some((d.percentage as f32, charging));
            }
        }
        t
    }
}

/// The network widget's glyph for NetworkManager's state.
pub fn net_glyph(s: &aizome_sys::network::NetworkState) -> NetGlyph {
    if s.airplane_mode {
        return NetGlyph::Off;
    }
    if let Some(p) = s.primary() {
        if p.vpn {
            return NetGlyph::Vpn;
        }
        if p.kind.contains("wireless") || p.kind.contains("wifi") {
            return NetGlyph::Wifi(s.current_wifi().map_or(4, |w| w.bars()));
        }
        return NetGlyph::Wired;
    }
    if let Some(w) = s.current_wifi() {
        return NetGlyph::Wifi(w.bars());
    }
    if !s.wifi_enabled && s.wired_devices().next().is_none() {
        return NetGlyph::Off;
    }
    NetGlyph::Disconnected
}
