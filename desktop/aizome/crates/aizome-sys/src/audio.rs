//! Sound: output and input devices, their volume and mute, the defaults, and
//! each application's streams.
//!
//! # Why PulseAudio's API
//!
//! PipeWire is the sound server; this module speaks to it through
//! **pipewire-pulse**, PipeWire's implementation of the PulseAudio
//! protocol, with `libpulse` (the `libpulse-binding` crate). The choice,
//! against the alternatives:
//!
//! - **Latency.** A volume key must apply within a few milliseconds. One
//!   long-lived connection does: a change is a message on a socket that is
//!   already open, applied by the server in well under a millisecond. Running
//!   `wpctl set-volume` per key press costs a process start and a fresh
//!   connection to PipeWire and WirePlumber (5–10 ms here), and `pw-dump`
//!   reparses every object on each change.
//! - **Completeness and stability.** The PulseAudio API has everything the
//!   shell and Settings need — sinks and sources with descriptions, ports,
//!   the defaults, per-application streams (sink inputs and source outputs)
//!   with their own volume, mute and device, and change events — and it has
//!   been stable for fifteen years. pipewire-pulse translates it faithfully,
//!   including WirePlumber's per-route volumes and the default-device
//!   metadata, which raw `libpipewire` clients must handle themselves.
//! - **Build cost.** `libpulse-binding` is hand-written FFI (no bindgen) to
//!   a small C library; the `pipewire` crate needs bindgen and libclang.
//!
//! The cost is a runtime dependency on pipewire-pulse
//! (`services.pipewire.pulse.enable = true` on NixOS), which every desktop
//! already runs for applications that use the PulseAudio API.
//!
//! # How it runs
//!
//! [`Audio`] runs libpulse's standard main loop on a thread of its own. The
//! UI's commands reach it through a channel and an eventfd the main loop
//! watches, so they run within microseconds of being sent. Subscribed
//! events (a device added, a volume changed elsewhere) refresh the affected
//! object, and the snapshot is republished. Volume changes are also applied
//! to the snapshot at once, so the OSD can draw the new level before the
//! server confirms it. When the server goes away the service reconnects
//! when it comes back.
//!
//! Volumes are fractions: 1.0 is 100% (PulseAudio's "normal"), and the
//! highest settable is [`MAX_VOLUME`], 150%.

use crate::error::{Error, Result};
use crate::service::{Done, Model, Stop, Task, Wake, lock, service_handle};
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{CardInfo, SinkInfo, SinkInputInfo, SourceInfo, SourceOutputInfo};
use pulse::context::subscribe::{Facility, InterestMaskSet, Operation};
use pulse::context::{Context, FlagSet, State};
use pulse::mainloop::api::Mainloop as _;
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::proplist::Proplist;
use pulse::volume::{ChannelVolumes, Volume};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::rc::{Rc, Weak};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The loudest volume that can be set: 150%.
pub const MAX_VOLUME: f32 = 1.5;

/// What the sound server reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioState {
    /// Connected to the sound server.
    pub available: bool,
    /// The server's name and version, as "PulseAudio (on PipeWire 1.6.9)
    /// 15.0.0".
    pub server: String,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
    /// Outputs, by description.
    pub sinks: Vec<AudioDevice>,
    /// Inputs, by description, without the monitors of outputs.
    pub sources: Vec<AudioDevice>,
    /// Applications playing (sink inputs).
    pub playback: Vec<AudioStream>,
    /// Applications recording (source outputs), without level meters.
    pub recording: Vec<AudioStream>,
    /// Sound cards with their profiles (a Bluetooth headset's high-quality
    /// playback or headset mode, HDMI and analog output).
    pub cards: Vec<Card>,
}

/// A sound card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Card {
    pub index: u32,
    pub name: String,
    /// Its name for people.
    pub description: String,
    pub icon_name: String,
    /// `pci`, `usb`, `bluetooth`…
    pub bus: String,
    pub active_profile: Option<String>,
    pub profiles: Vec<CardProfile>,
}

/// A way a card can be used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CardProfile {
    pub name: String,
    pub description: String,
    pub priority: u32,
    /// Usable now (a jack plugged in, the headset connected).
    pub available: bool,
    /// How many outputs and inputs it gives.
    pub sinks: u32,
    pub sources: u32,
}

impl AudioState {
    /// The default output.
    pub fn default_output(&self) -> Option<&AudioDevice> {
        let name = self.default_sink.as_deref()?;
        self.sinks.iter().find(|s| s.name == name)
    }

    /// The default input.
    pub fn default_input(&self) -> Option<&AudioDevice> {
        let name = self.default_source.as_deref()?;
        self.sources.iter().find(|s| s.name == name)
    }

    pub fn sink(&self, index: u32) -> Option<&AudioDevice> {
        self.sinks.iter().find(|s| s.index == index)
    }

    pub fn source(&self, index: u32) -> Option<&AudioDevice> {
        self.sources.iter().find(|s| s.index == index)
    }

    /// Whether any application is recording (for a microphone-in-use
    /// indicator).
    pub fn microphone_in_use(&self) -> bool {
        self.recording.iter().any(|s| !s.corked)
    }
}

/// An output or input device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioDevice {
    /// The server's index, to act on it.
    pub index: u32,
    /// The stable name (`alsa_output.pci-0000_00_1f.3…`).
    pub name: String,
    /// The name to show ("Speakers", "WH-1000XM4").
    pub description: String,
    /// Loudest channel, as a fraction (1.0 = 100%).
    pub volume: f32,
    /// Per-channel volumes, as fractions.
    pub channel_volumes: Vec<f32>,
    /// Left–right balance: −1.0 all left, 0.0 centred, 1.0 all right (0.0
    /// for a device without left and right channels).
    pub balance: f32,
    pub muted: bool,
    pub is_default: bool,
    /// The device's own icon name (`audio-card-analog-pci`,
    /// `audio-headset-bluetooth`).
    pub icon_name: String,
    /// `internal`, `speaker`, `headphone`, `headset`, `hdmi`…
    pub form_factor: String,
    /// `pci`, `usb`, `bluetooth`…
    pub bus: String,
    pub ports: Vec<Port>,
    pub active_port: Option<String>,
    /// Running, idle or suspended.
    pub state: DeviceState,
    /// A monitor of an output (sources only).
    pub is_monitor: bool,
}

/// A port of a device (speakers, headphones jack, HDMI).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Port {
    pub name: String,
    pub description: String,
    pub priority: u32,
    /// Something is plugged in (`Some(true)`), nothing is (`Some(false)`),
    /// or the hardware can't tell (`None`).
    pub available: Option<bool>,
}

/// Whether a device is in use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceState {
    Running,
    Idle,
    Suspended,
    #[default]
    Unknown,
}

/// An application's playback or recording stream.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioStream {
    pub index: u32,
    /// The application's name ("Firefox").
    pub application: String,
    /// Its desktop/app id, when it says (`firefox`, `org.gnome.Rhythmbox3`).
    pub app_id: String,
    pub icon_name: String,
    /// The executable (`firefox`).
    pub binary: String,
    pub pid: Option<u32>,
    /// What it is playing ("Big Buck Bunny - YouTube").
    pub media_name: String,
    /// `music`, `video`, `event`, `phone`…
    pub role: String,
    pub volume: f32,
    pub muted: bool,
    /// The device's index.
    pub device: u32,
    /// Paused.
    pub corked: bool,
    /// Its volume can be changed.
    pub volume_writable: bool,
}

/// Which kind of object a command is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    /// An output (sink).
    Sink,
    /// An input (source).
    Source,
    /// An application's playback (sink input).
    Playback,
    /// An application's recording (source output).
    Recording,
}

/// What the UI asks of the audio thread.
enum Command {
    Volume(Kind, u32, f32, Done<()>),
    /// Mute (`Some`) or toggle (`None`); the result is the new state.
    Mute(Kind, u32, Option<bool>, Done<bool>),
    /// Make the named sink or source the default.
    Default(Kind, String, Done<()>),
    Port(Kind, u32, String, Done<()>),
    /// Move a playback or recording stream to another device.
    Move(Kind, u32, u32, Done<()>),
    /// Change the default output's volume by a step (unmuting when
    /// raising); the result is the new volume.
    Step(f32, Done<f32>),
    /// Switch a card to a profile.
    CardProfile(u32, String, Done<()>),
    /// Toggle the default output's (Sink) or input's (Source) mute.
    ToggleDefault(Kind, Done<bool>),
    /// Set an output's or input's left–right balance.
    Balance(Kind, u32, f32, Done<()>),
}

/// The sound server.
pub struct Audio {
    commands: Mutex<mpsc::Sender<Command>>,
    eventfd: Option<Arc<OwnedFd>>,
    model: Arc<Model<AudioState>>,
    stop: Stop,
}

service_handle!(Audio, AudioState);

impl Audio {
    /// Connect to the PulseAudio server (pipewire-pulse) of this session,
    /// and keep reconnecting while it is away.
    pub fn new(wake: Wake) -> Audio {
        Audio::with_server(None, wake)
    }

    /// Connect to the server at `server` (a PulseAudio server string such
    /// as `unix:/run/user/1000/pulse/native`), or the session's.
    pub fn with_server(server: Option<String>, wake: Wake) -> Audio {
        let model = Arc::new(Model::new(AudioState::default(), wake));
        let (tx, rx) = mpsc::channel();
        let stop = Stop::default();
        // SAFETY: eventfd returns a new descriptor or -1.
        let raw = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        let eventfd = if raw >= 0 {
            // SAFETY: a descriptor we just created and own.
            Some(Arc::new(unsafe { OwnedFd::from_raw_fd(raw) }))
        } else {
            log::warn!("aizome-sys: no eventfd for the audio thread");
            None
        };
        if let Some(fd) = eventfd.clone() {
            let thread_model = model.clone();
            let thread_stop = stop.clone();
            let spawned = std::thread::Builder::new()
                .name("aizome-sys audio".into())
                .spawn(move || run(server, thread_model, rx, fd, thread_stop));
            if spawned.is_err() {
                log::warn!("aizome-sys: could not start the audio thread");
            }
        }
        Audio { commands: Mutex::new(tx), eventfd, model, stop }
    }

    /// Connected to the sound server.
    pub fn available(&self) -> bool {
        self.model.get().available
    }

    /// Wake the audio thread.
    fn poke(&self) {
        if let Some(fd) = &self.eventfd {
            let one: u64 = 1;
            // SAFETY: writing 8 bytes from a valid u64 to our eventfd.
            unsafe { libc::write(fd.as_raw_fd(), (&one as *const u64).cast(), 8) };
        }
    }

    fn send<T: Send + 'static>(&self, command: impl FnOnce(Done<T>) -> Command) -> Task<T> {
        let (task, done) = Task::pending(self.model.wake());
        if !self.model.get().available {
            done.finish(Err(Error::Unavailable("the sound server".into())));
            return task;
        }
        // If the thread is gone, the dropped Done reports Cancelled.
        if lock(&self.commands).send(command(done)).is_ok() {
            self.poke();
        }
        task
    }

    /// Set an output's volume (0.0–[`MAX_VOLUME`]), keeping its balance.
    pub fn set_sink_volume(&self, index: u32, volume: f32) -> Task<()> {
        self.send(|done| Command::Volume(Kind::Sink, index, clamp(volume), done))
    }

    /// Mute or unmute an output; the result is the new state.
    pub fn set_sink_mute(&self, index: u32, muted: bool) -> Task<bool> {
        self.send(|done| Command::Mute(Kind::Sink, index, Some(muted), done))
    }

    pub fn set_source_volume(&self, index: u32, volume: f32) -> Task<()> {
        self.send(|done| Command::Volume(Kind::Source, index, clamp(volume), done))
    }

    pub fn set_source_mute(&self, index: u32, muted: bool) -> Task<bool> {
        self.send(|done| Command::Mute(Kind::Source, index, Some(muted), done))
    }

    /// Set an output's left–right balance (−1.0 all left … 1.0 all right),
    /// keeping its volume.
    pub fn set_sink_balance(&self, index: u32, balance: f32) -> Task<()> {
        self.send(|done| Command::Balance(Kind::Sink, index, clamp_balance(balance), done))
    }

    /// Set an input's left–right balance.
    pub fn set_source_balance(&self, index: u32, balance: f32) -> Task<()> {
        self.send(|done| Command::Balance(Kind::Source, index, clamp_balance(balance), done))
    }

    /// Make an output the default (by its name).
    pub fn set_default_sink(&self, name: &str) -> Task<()> {
        let name = name.to_owned();
        self.send(|done| Command::Default(Kind::Sink, name, done))
    }

    /// Make an input the default (by its name).
    pub fn set_default_source(&self, name: &str) -> Task<()> {
        let name = name.to_owned();
        self.send(|done| Command::Default(Kind::Source, name, done))
    }

    /// Switch an output to another port (speakers, headphones).
    pub fn set_sink_port(&self, index: u32, port: &str) -> Task<()> {
        let port = port.to_owned();
        self.send(|done| Command::Port(Kind::Sink, index, port, done))
    }

    pub fn set_source_port(&self, index: u32, port: &str) -> Task<()> {
        let port = port.to_owned();
        self.send(|done| Command::Port(Kind::Source, index, port, done))
    }

    /// Set an application's playback volume.
    pub fn set_playback_volume(&self, index: u32, volume: f32) -> Task<()> {
        self.send(|done| Command::Volume(Kind::Playback, index, clamp(volume), done))
    }

    pub fn set_playback_mute(&self, index: u32, muted: bool) -> Task<bool> {
        self.send(|done| Command::Mute(Kind::Playback, index, Some(muted), done))
    }

    /// Move an application's playback to another output.
    pub fn move_playback(&self, index: u32, sink: u32) -> Task<()> {
        self.send(|done| Command::Move(Kind::Playback, index, sink, done))
    }

    pub fn set_recording_volume(&self, index: u32, volume: f32) -> Task<()> {
        self.send(|done| Command::Volume(Kind::Recording, index, clamp(volume), done))
    }

    pub fn set_recording_mute(&self, index: u32, muted: bool) -> Task<bool> {
        self.send(|done| Command::Mute(Kind::Recording, index, Some(muted), done))
    }

    /// Move an application's recording to another input.
    pub fn move_recording(&self, index: u32, source: u32) -> Task<()> {
        self.send(|done| Command::Move(Kind::Recording, index, source, done))
    }

    /// Switch a card to one of its profiles (a Bluetooth headset between
    /// high-quality playback and headset mode).
    pub fn set_card_profile(&self, card: u32, profile: &str) -> Task<()> {
        let profile = profile.to_owned();
        self.send(|done| Command::CardProfile(card, profile, done))
    }

    /// The volume keys: change the default output's volume by `step`
    /// (`0.05` up, `-0.05` down), landing on multiples of the step, within
    /// 0–[`MAX_VOLUME`]; raising unmutes. The snapshot shows the new volume
    /// at once; the task's result is it.
    pub fn step_volume(&self, step: f32) -> Task<f32> {
        self.send(|done| Command::Step(step, done))
    }

    /// The mute key: mute or unmute the default output; the result is the
    /// new state.
    pub fn toggle_mute(&self) -> Task<bool> {
        self.send(|done| Command::ToggleDefault(Kind::Sink, done))
    }

    /// The microphone mute key.
    pub fn toggle_microphone_mute(&self) -> Task<bool> {
        self.send(|done| Command::ToggleDefault(Kind::Source, done))
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop.stop();
        self.poke();
    }
}

fn clamp_balance(balance: f32) -> f32 {
    if balance.is_finite() { balance.clamp(-1.0, 1.0) } else { 0.0 }
}

/// A device's balance, 0.0 where it has no left and right.
fn balance_of(volumes: &ChannelVolumes, map: &pulse::channelmap::Map) -> f32 {
    if map.can_balance() { volumes.get_balance(map) } else { 0.0 }
}

fn clamp(volume: f32) -> f32 {
    if volume.is_finite() { volume.clamp(0.0, MAX_VOLUME) } else { 0.0 }
}

/// A fraction for a PulseAudio volume.
pub(crate) fn fraction(volume: Volume) -> f32 {
    volume.0 as f32 / Volume::NORMAL.0 as f32
}

/// A PulseAudio volume for a fraction.
pub(crate) fn pa_volume(fraction: f32) -> Volume {
    Volume((clamp(fraction) * Volume::NORMAL.0 as f32).round() as u32)
}

/// `volumes` with their loudest channel at `fraction`, the balance kept.
pub(crate) fn scaled(volumes: &ChannelVolumes, fraction: f32) -> ChannelVolumes {
    let mut volumes = *volumes;
    if volumes.len() == 0 {
        volumes.set(2, pa_volume(fraction));
    } else if volumes.max().is_muted() {
        let channels = volumes.len();
        volumes.set(channels, pa_volume(fraction));
    } else {
        volumes.scale(pa_volume(fraction));
    }
    volumes
}

/// The default output's volume after a volume key: a step up or down,
/// landing on a multiple of the step (so 38% goes up to 45%, not 43%).
pub(crate) fn stepped(current: f32, step: f32) -> f32 {
    let size = step.abs();
    if size <= f32::EPSILON || !step.is_finite() {
        return clamp(current);
    }
    let steps = current / size;
    // A level just off the grid (37.9%) counts as on it.
    let near = steps.round();
    let base = if (steps - near).abs() < 0.02 {
        near
    } else if step > 0.0 {
        steps.floor()
    } else {
        steps.ceil()
    };
    let target = (base + step.signum()) * size;
    clamp((target * 1000.0).round() / 1000.0)
}

/// What can be turned up and muted.
trait Levels {
    fn apply_volumes(&mut self, volumes: &ChannelVolumes);
    fn is_muted(&self) -> bool;
    fn set_muted(&mut self, muted: bool);
}

impl Levels for AudioDevice {
    fn apply_volumes(&mut self, volumes: &ChannelVolumes) {
        self.volume = fraction(volumes.max());
        self.channel_volumes = volumes.get().iter().map(|v| fraction(*v)).collect();
    }
    fn is_muted(&self) -> bool {
        self.muted
    }
    fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }
}

impl Levels for AudioStream {
    fn apply_volumes(&mut self, volumes: &ChannelVolumes) {
        self.volume = fraction(volumes.max());
    }
    fn is_muted(&self) -> bool {
        self.muted
    }
    fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }
}

/// A level we set and the server hasn't reported yet. Until it does (or a
/// second passes), reports of the old level — answers to questions asked
/// before the change, or events still on their way — don't undo it, so a
/// quick second key press steps from the level the first one set.
struct Expect {
    volumes: Option<ChannelVolumes>,
    muted: Option<bool>,
    since: Instant,
}

const EXPECT_FOR: Duration = Duration::from_secs(1);

fn same_volumes(a: &ChannelVolumes, b: &ChannelVolumes) -> bool {
    a.len() == b.len() && a.get().iter().zip(b.get()).all(|(x, y)| x.0.abs_diff(y.0) <= 64)
}

type Entries<T> = BTreeMap<u32, (T, ChannelVolumes)>;
type Expects = HashMap<(Kind, u32), Expect>;

/// Store a report from the server, keeping levels we set and it hasn't
/// caught up with.
fn put<T: Levels>(map: &mut Entries<T>, expects: &mut Expects, key: (Kind, u32), mut entry: (T, ChannelVolumes)) {
    if let Some(expect) = expects.get_mut(&key) {
        let fresh = expect.since.elapsed() < EXPECT_FOR;
        if let Some(volumes) = expect.volumes {
            if !fresh || same_volumes(&entry.1, &volumes) {
                expect.volumes = None;
            } else {
                entry.1 = volumes;
                entry.0.apply_volumes(&volumes);
            }
        }
        if let Some(muted) = expect.muted {
            if !fresh || entry.0.is_muted() == muted {
                expect.muted = None;
            } else {
                entry.0.set_muted(muted);
            }
        }
        if expect.volumes.is_none() && expect.muted.is_none() {
            expects.remove(&key);
        }
    }
    map.insert(key.1, entry);
}

fn expect(expects: &mut Expects, key: (Kind, u32)) -> &mut Expect {
    let expect = expects.entry(key).or_insert_with(|| Expect { volumes: None, muted: None, since: Instant::now() });
    expect.since = Instant::now();
    expect
}

/// Set a level at once, remembering it until the server reports it.
fn set_volume_in<T: Levels>(
    map: &mut Entries<T>,
    expects: &mut Expects,
    key: (Kind, u32),
    level: f32,
) -> Option<ChannelVolumes> {
    let (item, volumes) = map.get_mut(&key.1)?;
    let new = scaled(volumes, level);
    *volumes = new;
    item.apply_volumes(&new);
    expect(expects, key).volumes = Some(new);
    Some(new)
}

fn set_mute_in<T: Levels>(
    map: &mut Entries<T>,
    expects: &mut Expects,
    key: (Kind, u32),
    muted: Option<bool>,
) -> Option<bool> {
    let (item, _) = map.get_mut(&key.1)?;
    let muted = muted.unwrap_or(!item.is_muted());
    item.set_muted(muted);
    expect(expects, key).muted = Some(muted);
    Some(muted)
}

/// What the audio thread knows, by index.
#[derive(Default)]
struct Live {
    server: String,
    default_sink: Option<String>,
    default_source: Option<String>,
    sinks: Entries<AudioDevice>,
    sources: Entries<AudioDevice>,
    playback: Entries<AudioStream>,
    recording: Entries<AudioStream>,
    cards: BTreeMap<u32, Card>,
    expects: Expects,
    dirty: bool,
}

impl Live {
    fn state(&self) -> AudioState {
        let mut sinks: Vec<AudioDevice> = self.sinks.values().map(|(d, _)| d.clone()).collect();
        let mut sources: Vec<AudioDevice> =
            self.sources.values().filter(|(d, _)| !d.is_monitor).map(|(d, _)| d.clone()).collect();
        for sink in &mut sinks {
            sink.is_default = self.default_sink.as_deref() == Some(sink.name.as_str());
        }
        for source in &mut sources {
            source.is_default = self.default_source.as_deref() == Some(source.name.as_str());
        }
        sinks.sort_by(|a, b| a.description.cmp(&b.description).then(a.index.cmp(&b.index)));
        sources.sort_by(|a, b| a.description.cmp(&b.description).then(a.index.cmp(&b.index)));
        AudioState {
            available: true,
            server: self.server.clone(),
            default_sink: self.default_sink.clone(),
            default_source: self.default_source.clone(),
            sinks,
            sources,
            playback: self.playback.values().map(|(s, _)| s.clone()).collect(),
            recording: self.recording.values().map(|(s, _)| s.clone()).collect(),
            cards: self.cards.values().cloned().collect(),
        }
    }

    fn put_device(&mut self, kind: Kind, entry: (AudioDevice, ChannelVolumes)) {
        let key = (kind, entry.0.index);
        match kind {
            Kind::Source => put(&mut self.sources, &mut self.expects, key, entry),
            _ => put(&mut self.sinks, &mut self.expects, key, entry),
        }
        self.dirty = true;
    }

    fn put_stream(&mut self, kind: Kind, entry: (AudioStream, ChannelVolumes)) {
        let key = (kind, entry.0.index);
        match kind {
            Kind::Recording => put(&mut self.recording, &mut self.expects, key, entry),
            _ => put(&mut self.playback, &mut self.expects, key, entry),
        }
        self.dirty = true;
    }

    fn remove(&mut self, kind: Kind, index: u32) {
        match kind {
            Kind::Sink => self.sinks.remove(&index).map(drop),
            Kind::Source => self.sources.remove(&index).map(drop),
            Kind::Playback => self.playback.remove(&index).map(drop),
            Kind::Recording => self.recording.remove(&index).map(drop),
        };
        self.expects.remove(&(kind, index));
        self.dirty = true;
    }

    fn set_volume(&mut self, kind: Kind, index: u32, level: f32) -> Option<ChannelVolumes> {
        let key = (kind, index);
        let new = match kind {
            Kind::Sink => set_volume_in(&mut self.sinks, &mut self.expects, key, level),
            Kind::Source => set_volume_in(&mut self.sources, &mut self.expects, key, level),
            Kind::Playback => set_volume_in(&mut self.playback, &mut self.expects, key, level),
            Kind::Recording => set_volume_in(&mut self.recording, &mut self.expects, key, level),
        };
        self.dirty |= new.is_some();
        new
    }

    fn set_mute(&mut self, kind: Kind, index: u32, muted: Option<bool>) -> Option<bool> {
        let key = (kind, index);
        let new = match kind {
            Kind::Sink => set_mute_in(&mut self.sinks, &mut self.expects, key, muted),
            Kind::Source => set_mute_in(&mut self.sources, &mut self.expects, key, muted),
            Kind::Playback => set_mute_in(&mut self.playback, &mut self.expects, key, muted),
            Kind::Recording => set_mute_in(&mut self.recording, &mut self.expects, key, muted),
        };
        self.dirty |= new.is_some();
        new
    }

    /// Set a device's volumes at once (a balance change), remembering them
    /// until the server reports them.
    fn set_volumes(&mut self, kind: Kind, index: u32, volumes: ChannelVolumes, balance: f32) {
        let map = match kind {
            Kind::Source => &mut self.sources,
            _ => &mut self.sinks,
        };
        if let Some((device, stored)) = map.get_mut(&index) {
            *stored = volumes;
            device.apply_volumes(&volumes);
            device.balance = balance;
            expect(&mut self.expects, (kind, index)).volumes = Some(volumes);
            self.dirty = true;
        }
    }

    fn volume_of(&self, kind: Kind, index: u32) -> Option<f32> {
        match kind {
            Kind::Sink => self.sinks.get(&index).map(|(d, _)| d.volume),
            Kind::Source => self.sources.get(&index).map(|(d, _)| d.volume),
            Kind::Playback => self.playback.get(&index).map(|(s, _)| s.volume),
            Kind::Recording => self.recording.get(&index).map(|(s, _)| s.volume),
        }
    }

    fn default_index(&self, kind: Kind) -> Option<u32> {
        let (name, map) = match kind {
            Kind::Source => (self.default_source.as_deref()?, &self.sources),
            _ => (self.default_sink.as_deref()?, &self.sinks),
        };
        map.iter().find(|(_, (d, _))| d.name == name).map(|(i, _)| *i)
    }
}

fn prop(list: &Proplist, key: &str) -> String {
    list.get_str(key).unwrap_or_default()
}

fn text(value: &Option<std::borrow::Cow<'_, str>>) -> String {
    value.as_deref().unwrap_or_default().to_owned()
}

fn port_available(available: pulse::def::PortAvailable) -> Option<bool> {
    match available {
        pulse::def::PortAvailable::Yes => Some(true),
        pulse::def::PortAvailable::No => Some(false),
        _ => None,
    }
}

fn sink_device(info: &SinkInfo<'_>) -> (AudioDevice, ChannelVolumes) {
    let device = AudioDevice {
        index: info.index,
        name: text(&info.name),
        description: text(&info.description),
        volume: fraction(info.volume.max()),
        channel_volumes: info.volume.get().iter().map(|v| fraction(*v)).collect(),
        balance: balance_of(&info.volume, &info.channel_map),
        muted: info.mute,
        is_default: false,
        icon_name: prop(&info.proplist, "device.icon_name"),
        form_factor: prop(&info.proplist, "device.form_factor"),
        bus: prop(&info.proplist, "device.bus"),
        ports: info
            .ports
            .iter()
            .map(|p| Port {
                name: text(&p.name),
                description: text(&p.description),
                priority: p.priority,
                available: port_available(p.available),
            })
            .collect(),
        active_port: info.active_port.as_ref().and_then(|p| p.name.as_deref().map(str::to_owned)),
        state: match info.state {
            pulse::def::SinkState::Running => DeviceState::Running,
            pulse::def::SinkState::Idle => DeviceState::Idle,
            pulse::def::SinkState::Suspended => DeviceState::Suspended,
            _ => DeviceState::Unknown,
        },
        is_monitor: false,
    };
    (device, info.volume)
}

fn source_device(info: &SourceInfo<'_>) -> (AudioDevice, ChannelVolumes) {
    let device = AudioDevice {
        index: info.index,
        name: text(&info.name),
        description: text(&info.description),
        volume: fraction(info.volume.max()),
        channel_volumes: info.volume.get().iter().map(|v| fraction(*v)).collect(),
        balance: balance_of(&info.volume, &info.channel_map),
        muted: info.mute,
        is_default: false,
        icon_name: prop(&info.proplist, "device.icon_name"),
        form_factor: prop(&info.proplist, "device.form_factor"),
        bus: prop(&info.proplist, "device.bus"),
        ports: info
            .ports
            .iter()
            .map(|p| Port {
                name: text(&p.name),
                description: text(&p.description),
                priority: p.priority,
                available: port_available(p.available),
            })
            .collect(),
        active_port: info.active_port.as_ref().and_then(|p| p.name.as_deref().map(str::to_owned)),
        state: match info.state {
            pulse::def::SourceState::Running => DeviceState::Running,
            pulse::def::SourceState::Idle => DeviceState::Idle,
            pulse::def::SourceState::Suspended => DeviceState::Suspended,
            _ => DeviceState::Unknown,
        },
        is_monitor: info.monitor_of_sink.is_some(),
    };
    (device, info.volume)
}

#[allow(clippy::too_many_arguments)]
fn stream(
    index: u32,
    name: &Option<std::borrow::Cow<'_, str>>,
    proplist: &Proplist,
    volume: &ChannelVolumes,
    muted: bool,
    device: u32,
    corked: bool,
    volume_writable: bool,
) -> (AudioStream, ChannelVolumes) {
    let application = Some(prop(proplist, "application.name")).filter(|n| !n.is_empty()).unwrap_or_else(|| text(name));
    let app_id = ["pipewire.access.portal.app_id", "application.id", "application.process.binary"]
        .iter()
        .map(|key| prop(proplist, key))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    let stream = AudioStream {
        index,
        application,
        app_id,
        icon_name: prop(proplist, "application.icon_name"),
        binary: prop(proplist, "application.process.binary"),
        pid: prop(proplist, "application.process.id").parse().ok(),
        media_name: Some(prop(proplist, "media.name")).filter(|n| !n.is_empty()).unwrap_or_else(|| text(name)),
        role: prop(proplist, "media.role"),
        volume: fraction(volume.max()),
        muted,
        device,
        corked,
        volume_writable,
    };
    (stream, *volume)
}

fn card(info: &CardInfo<'_>) -> Card {
    let profile = |p: &pulse::context::introspect::CardProfileInfo<'_>| CardProfile {
        name: text(&p.name),
        description: text(&p.description),
        priority: p.priority,
        available: p.available,
        sinks: p.n_sinks,
        sources: p.n_sources,
    };
    Card {
        index: info.index,
        name: text(&info.name),
        description: Some(prop(&info.proplist, "device.description"))
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| text(&info.name)),
        icon_name: prop(&info.proplist, "device.icon_name"),
        bus: prop(&info.proplist, "device.bus"),
        active_profile: info.active_profile.as_ref().and_then(|p| p.name.as_deref().map(str::to_owned)),
        profiles: info.profiles.iter().map(profile).collect(),
    }
}

fn playback_stream(info: &SinkInputInfo<'_>) -> (AudioStream, ChannelVolumes) {
    stream(
        info.index,
        &info.name,
        &info.proplist,
        &info.volume,
        info.mute,
        info.sink,
        info.corked,
        info.volume_writable,
    )
}

/// Recording streams that are only level meters (pavucontrol's, the
/// shell's own) are left out.
fn recording_stream(info: &SourceOutputInfo<'_>) -> Option<(AudioStream, ChannelVolumes)> {
    let peak = info.proplist.get_str("resample.method").is_some_and(|m| m == "peaks")
        || info.resample_method.as_deref() == Some("peaks")
        || info.name.as_deref().is_some_and(|n| n.contains("Peak detect"));
    (!peak).then(|| {
        stream(
            info.index,
            &info.name,
            &info.proplist,
            &info.volume,
            info.mute,
            info.source,
            info.corked,
            info.volume_writable,
        )
    })
}

type Ctx = Rc<RefCell<Context>>;

/// The audio thread: connect, follow, run commands; reconnect after the
/// server leaves.
fn run(
    server: Option<String>,
    model: Arc<Model<AudioState>>,
    commands: mpsc::Receiver<Command>,
    eventfd: Arc<OwnedFd>,
    stop: Stop,
) {
    let commands = Rc::new(commands);
    while !stop.stopped() {
        match session(server.as_deref(), &model, &commands, &eventfd, &stop) {
            Ok(()) => {}
            Err(error) => log::debug!("aizome-sys: sound server: {error}"),
        }
        model.set_if_changed(AudioState::default());
        // Fail queued commands rather than leaving them waiting.
        while let Ok(command) = commands.try_recv() {
            drop(command);
        }
        // Wait a little before reconnecting (pipewire-pulse restarting).
        for _ in 0..10 {
            if stop.stopped() {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

fn session(
    server: Option<&str>,
    model: &Arc<Model<AudioState>>,
    commands: &Rc<mpsc::Receiver<Command>>,
    eventfd: &Arc<OwnedFd>,
    stop: &Stop,
) -> Result<()> {
    let mut mainloop = Mainloop::new().ok_or_else(|| Error::Failed("no PulseAudio main loop".into()))?;
    let mut proplist = Proplist::new().ok_or_else(|| Error::Failed("no proplist".into()))?;
    let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_NAME, "Aizome");
    let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_ID, "org.sekka.Aizome");
    let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_ICON_NAME, "multimedia-volume-control");
    let context = Context::new_with_proplist(&mainloop, "Aizome", &proplist)
        .ok_or_else(|| Error::Failed("no PulseAudio context".into()))?;
    let context: Ctx = Rc::new(RefCell::new(context));
    context
        .borrow_mut()
        .connect(server, FlagSet::NOAUTOSPAWN, None)
        .map_err(|e| Error::Unavailable(format!("the sound server: {e}")))?;

    // Wait until connected.
    loop {
        if stop.stopped() {
            return Ok(());
        }
        match mainloop.iterate(true) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                return Err(Error::Failed("the main loop stopped".into()));
            }
        }
        match context.borrow().get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => {
                return Err(Error::Unavailable("the sound server".into()));
            }
            _ => {}
        }
    }

    let live = Rc::new(RefCell::new(Live::default()));
    load_all(&context, &live);
    subscribe(&context, &live);

    // Commands: the eventfd wakes the main loop.
    let weak = Rc::downgrade(&context);
    let io_live = live.clone();
    let io_commands = commands.clone();
    let fd = eventfd.as_raw_fd();
    let _io = mainloop
        .new_io_event(
            fd,
            pulse::mainloop::events::io::FlagSet::INPUT,
            Box::new(move |_, fd, _| {
                let mut count: u64 = 0;
                // SAFETY: reading 8 bytes into a valid u64 from our eventfd.
                unsafe { libc::read(fd, (&mut count as *mut u64).cast(), 8) };
                while let Ok(command) = io_commands.try_recv() {
                    execute(&weak, &io_live, command);
                }
            }),
        )
        .ok_or_else(|| Error::Failed("could not watch the command queue".into()))?;
    // Commands sent before the io event existed.
    while let Ok(command) = commands.try_recv() {
        execute(&Rc::downgrade(&context), &live, command);
    }

    loop {
        if stop.stopped() {
            return Ok(());
        }
        match mainloop.iterate(true) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                return Err(Error::Failed("the main loop stopped".into()));
            }
        }
        if matches!(context.borrow().get_state(), State::Failed | State::Terminated) {
            return Err(Error::Unavailable("the sound server left".into()));
        }
        let mut live = live.borrow_mut();
        if live.dirty {
            live.dirty = false;
            model.set_if_changed(live.state());
        }
    }
}

/// Read everything once.
fn load_all(context: &Ctx, live: &Rc<RefCell<Live>>) {
    let introspect = context.borrow().introspect();
    let l = live.clone();
    introspect.get_server_info(move |info| {
        let mut live = l.borrow_mut();
        live.server = format!("{} {}", text(&info.server_name), text(&info.server_version));
        live.default_sink = info.default_sink_name.as_deref().map(str::to_owned);
        live.default_source = info.default_source_name.as_deref().map(str::to_owned);
        live.dirty = true;
    });
    let l = live.clone();
    introspect.get_sink_info_list(move |result| {
        if let ListResult::Item(info) = result {
            l.borrow_mut().put_device(Kind::Sink, sink_device(info));
        }
    });
    let l = live.clone();
    introspect.get_source_info_list(move |result| {
        if let ListResult::Item(info) = result {
            l.borrow_mut().put_device(Kind::Source, source_device(info));
        }
    });
    let l = live.clone();
    introspect.get_sink_input_info_list(move |result| {
        if let ListResult::Item(info) = result {
            l.borrow_mut().put_stream(Kind::Playback, playback_stream(info));
        }
    });
    let l = live.clone();
    introspect.get_card_info_list(move |result| {
        if let ListResult::Item(info) = result {
            let mut live = l.borrow_mut();
            live.cards.insert(info.index, card(info));
            live.dirty = true;
        }
    });
    let l = live.clone();
    introspect.get_source_output_info_list(move |result| {
        if let ListResult::Item(info) = result
            && let Some(stream) = recording_stream(info)
        {
            l.borrow_mut().put_stream(Kind::Recording, stream);
        }
    });
}

/// Follow changes: each event re-reads the object it names.
fn subscribe(context: &Ctx, live: &Rc<RefCell<Live>>) {
    let weak = Rc::downgrade(context);
    let l = live.clone();
    context.borrow_mut().set_subscribe_callback(Some(Box::new(move |facility, operation, index| {
        let Some(context) = weak.upgrade() else { return };
        let introspect = context.borrow().introspect();
        let kind = match facility {
            Some(Facility::Sink) => Kind::Sink,
            Some(Facility::Source) => Kind::Source,
            Some(Facility::SinkInput) => Kind::Playback,
            Some(Facility::SourceOutput) => Kind::Recording,
            Some(Facility::Card) => {
                if operation == Some(Operation::Removed) {
                    let mut live = l.borrow_mut();
                    live.cards.remove(&index);
                    live.dirty = true;
                } else {
                    let l = l.clone();
                    introspect.get_card_info_by_index(index, move |result| {
                        if let ListResult::Item(info) = result {
                            let mut live = l.borrow_mut();
                            live.cards.insert(info.index, card(info));
                            live.dirty = true;
                        }
                    });
                }
                return;
            }
            Some(Facility::Server) => {
                let l = l.clone();
                introspect.get_server_info(move |info| {
                    let mut live = l.borrow_mut();
                    live.default_sink = info.default_sink_name.as_deref().map(str::to_owned);
                    live.default_source = info.default_source_name.as_deref().map(str::to_owned);
                    live.dirty = true;
                });
                return;
            }
            _ => return,
        };
        if operation == Some(Operation::Removed) {
            l.borrow_mut().remove(kind, index);
            return;
        }
        let l = l.clone();
        match kind {
            Kind::Sink => {
                introspect.get_sink_info_by_index(index, move |result| {
                    if let ListResult::Item(info) = result {
                        l.borrow_mut().put_device(Kind::Sink, sink_device(info));
                    }
                });
            }
            Kind::Source => {
                introspect.get_source_info_by_index(index, move |result| {
                    if let ListResult::Item(info) = result {
                        l.borrow_mut().put_device(Kind::Source, source_device(info));
                    }
                });
            }
            Kind::Playback => {
                introspect.get_sink_input_info(index, move |result| {
                    if let ListResult::Item(info) = result {
                        l.borrow_mut().put_stream(Kind::Playback, playback_stream(info));
                    }
                });
            }
            Kind::Recording => {
                introspect.get_source_output_info(index, move |result| {
                    if let ListResult::Item(info) = result
                        && let Some(stream) = recording_stream(info)
                    {
                        l.borrow_mut().put_stream(Kind::Recording, stream);
                    }
                });
            }
        }
    })));
    let mask = InterestMaskSet::SINK
        | InterestMaskSet::SOURCE
        | InterestMaskSet::SINK_INPUT
        | InterestMaskSet::SOURCE_OUTPUT
        | InterestMaskSet::SERVER
        | InterestMaskSet::CARD;
    context.borrow_mut().subscribe(mask, |_| {});
}

/// A callback for an operation's success that finishes `done` with `value`.
fn finish<T: 'static>(weak: &Weak<RefCell<Context>>, done: Done<T>, value: T) -> Box<dyn FnMut(bool)> {
    let weak = weak.clone();
    let mut pending = Some((done, value));
    Box::new(move |ok| {
        if let Some((done, value)) = pending.take() {
            done.finish(if ok { Ok(value) } else { Err(failure(&weak)) });
        }
    })
}

fn failure(weak: &Weak<RefCell<Context>>) -> Error {
    let message = weak
        .upgrade()
        .and_then(|c| c.try_borrow().ok().map(|c| c.errno().to_string().unwrap_or_default()))
        .unwrap_or_default();
    match message.as_str() {
        "No such entity" => Error::NotFound(message),
        "Access denied" => Error::NotAuthorized(message),
        "" => Error::Failed("The sound server refused".into()),
        _ => Error::Failed(message),
    }
}

fn send_volume(
    introspect: &mut pulse::context::introspect::Introspector,
    kind: Kind,
    index: u32,
    volumes: &ChannelVolumes,
    callback: Box<dyn FnMut(bool)>,
) {
    let callback = Some(callback);
    match kind {
        Kind::Sink => drop(introspect.set_sink_volume_by_index(index, volumes, callback)),
        Kind::Source => drop(introspect.set_source_volume_by_index(index, volumes, callback)),
        Kind::Playback => drop(introspect.set_sink_input_volume(index, volumes, callback)),
        Kind::Recording => drop(introspect.set_source_output_volume(index, volumes, callback)),
    }
}

fn send_mute(
    introspect: &mut pulse::context::introspect::Introspector,
    kind: Kind,
    index: u32,
    muted: bool,
    callback: Option<Box<dyn FnMut(bool)>>,
) {
    match kind {
        Kind::Sink => drop(introspect.set_sink_mute_by_index(index, muted, callback)),
        Kind::Source => drop(introspect.set_source_mute_by_index(index, muted, callback)),
        Kind::Playback => drop(introspect.set_sink_input_mute(index, muted, callback)),
        Kind::Recording => drop(introspect.set_source_output_mute(index, muted, callback)),
    }
}

fn what(kind: Kind) -> &'static str {
    match kind {
        Kind::Sink => "output",
        Kind::Source => "input",
        Kind::Playback | Kind::Recording => "stream",
    }
}

fn execute(weak: &Weak<RefCell<Context>>, live: &Rc<RefCell<Live>>, command: Command) {
    let Some(context) = weak.upgrade() else { return };
    let mut introspect = context.borrow().introspect();
    let not_found = |kind: Kind, index: u32| Error::NotFound(format!("{} {index}", what(kind)));
    match command {
        Command::Volume(kind, index, level, done) => {
            let new = live.borrow_mut().set_volume(kind, index, level);
            match new {
                Some(volumes) => send_volume(&mut introspect, kind, index, &volumes, finish(weak, done, ())),
                None => done.finish(Err(not_found(kind, index))),
            }
        }
        Command::Mute(kind, index, muted, done) => {
            let new = live.borrow_mut().set_mute(kind, index, muted);
            match new {
                Some(muted) => send_mute(&mut introspect, kind, index, muted, Some(finish(weak, done, muted))),
                None => done.finish(Err(not_found(kind, index))),
            }
        }
        Command::Default(kind, name, done) => {
            {
                let mut l = live.borrow_mut();
                let known = match kind {
                    Kind::Source => l.sources.values().any(|(d, _)| d.name == name),
                    _ => l.sinks.values().any(|(d, _)| d.name == name),
                };
                if !known {
                    return done.finish(Err(Error::NotFound(name)));
                }
                match kind {
                    Kind::Source => l.default_source = Some(name.clone()),
                    _ => l.default_sink = Some(name.clone()),
                }
                l.dirty = true;
            }
            let callback = finish(weak, done, ());
            let mut context = context.borrow_mut();
            match kind {
                Kind::Source => drop(context.set_default_source(&name, callback)),
                _ => drop(context.set_default_sink(&name, callback)),
            }
        }
        Command::Port(kind, index, port, done) => {
            let callback = Some(finish(weak, done, ()));
            match kind {
                Kind::Source => drop(introspect.set_source_port_by_index(index, &port, callback)),
                _ => drop(introspect.set_sink_port_by_index(index, &port, callback)),
            }
        }
        Command::Move(kind, index, device, done) => {
            let callback = Some(finish(weak, done, ()));
            match kind {
                Kind::Recording => drop(introspect.move_source_output_by_index(index, device, callback)),
                _ => drop(introspect.move_sink_input_by_index(index, device, callback)),
            }
        }
        Command::CardProfile(index, profile, done) => {
            if !live.borrow().cards.get(&index).is_some_and(|c| c.profiles.iter().any(|p| p.name == profile)) {
                return done.finish(Err(Error::NotFound(format!("card {index} profile {profile}"))));
            }
            drop(introspect.set_card_profile_by_index(index, &profile, Some(finish(weak, done, ()))));
        }
        Command::Step(step, done) => {
            let mut l = live.borrow_mut();
            let Some(index) = l.default_index(Kind::Sink) else {
                return done.finish(Err(Error::NotFound("no default output".into())));
            };
            let current = l.volume_of(Kind::Sink, index).unwrap_or(0.0);
            let Some(volumes) = l.set_volume(Kind::Sink, index, stepped(current, step)) else { return };
            let level = fraction(volumes.max());
            let unmute = step > 0.0 && l.sinks.get(&index).is_some_and(|(d, _)| d.muted);
            if unmute {
                l.set_mute(Kind::Sink, index, Some(false));
            }
            drop(l);
            if unmute {
                send_mute(&mut introspect, Kind::Sink, index, false, None);
            }
            send_volume(&mut introspect, Kind::Sink, index, &volumes, finish(weak, done, level));
        }
        Command::Balance(kind, index, balance, done) => {
            // The channel map comes with the device's info: read it, then
            // set the volumes.
            let (weak, live) = (weak.clone(), live.clone());
            let mut done = Some(done);
            let mut apply = move |volume: &ChannelVolumes, map: &pulse::channelmap::Map| {
                let Some(done) = done.take() else { return };
                if !map.can_balance() {
                    return done.finish(Err(Error::InvalidArgument("it has no left and right channels".into())));
                }
                let mut volumes = *volume;
                volumes.set_balance(map, balance);
                live.borrow_mut().set_volumes(kind, index, volumes, balance);
                let Some(context) = weak.upgrade() else { return };
                let mut introspect = context.borrow().introspect();
                send_volume(&mut introspect, kind, index, &volumes, finish(&weak, done, ()));
            };
            match kind {
                Kind::Source => drop(introspect.get_source_info_by_index(index, move |r| {
                    if let ListResult::Item(info) = r {
                        apply(&info.volume, &info.channel_map);
                    }
                })),
                _ => drop(introspect.get_sink_info_by_index(index, move |r| {
                    if let ListResult::Item(info) = r {
                        apply(&info.volume, &info.channel_map);
                    }
                })),
            }
        }
        Command::ToggleDefault(kind, done) => {
            let mut l = live.borrow_mut();
            let Some(index) = l.default_index(kind) else {
                return done.finish(Err(Error::NotFound(format!("no default {}", what(kind)))));
            };
            let Some(muted) = l.set_mute(kind, index, None) else { return };
            drop(l);
            send_mute(&mut introspect, kind, index, muted, Some(finish(weak, done, muted)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_convert_and_keep_balance() {
        assert_eq!(pa_volume(1.0), Volume::NORMAL);
        assert!((fraction(Volume::NORMAL) - 1.0).abs() < 1e-6);
        assert_eq!(pa_volume(9.0), pa_volume(MAX_VOLUME), "clamped to 150%");
        assert_eq!(pa_volume(f32::NAN), Volume::MUTED);

        let mut balanced = ChannelVolumes::default();
        balanced.set_len(2);
        balanced.get_mut()[0] = pa_volume(0.5);
        balanced.get_mut()[1] = pa_volume(0.25);
        let louder = scaled(&balanced, 1.0);
        assert!((fraction(louder.get()[0]) - 1.0).abs() < 0.01);
        assert!((fraction(louder.get()[1]) - 0.5).abs() < 0.01, "the balance is kept");

        // From silence every channel comes up together.
        let mut silent = ChannelVolumes::default();
        silent.set(2, Volume::MUTED);
        let up = scaled(&silent, 0.3);
        assert!(up.get().iter().all(|v| (fraction(*v) - 0.3).abs() < 0.01));
    }

    #[test]
    fn the_snapshot_marks_defaults_and_hides_monitors() {
        let mut live = Live::default();
        let device = |index, name: &str, monitor| {
            (
                AudioDevice {
                    index,
                    name: name.into(),
                    description: name.into(),
                    is_monitor: monitor,
                    ..Default::default()
                },
                ChannelVolumes::default(),
            )
        };
        live.sinks.insert(1, device(1, "speakers", false));
        live.sinks.insert(2, device(2, "hdmi", false));
        live.sources.insert(3, device(3, "speakers.monitor", true));
        live.sources.insert(4, device(4, "mic", false));
        live.default_sink = Some("speakers".into());
        live.default_source = Some("mic".into());
        let state = live.state();
        assert_eq!(state.default_output().unwrap().index, 1);
        assert!(state.sinks.iter().find(|s| s.index == 1).unwrap().is_default);
        assert_eq!(state.sources.len(), 1);
        assert_eq!(state.default_input().unwrap().name, "mic");
        assert_eq!(live.default_index(Kind::Sink), Some(1));
        assert_eq!(live.default_index(Kind::Source), Some(4));
    }

    #[test]
    fn volume_keys_land_on_the_grid() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(stepped(0.5, 0.05), 0.55));
        assert!(close(stepped(0.38, 0.05), 0.40));
        assert!(close(stepped(0.38, -0.05), 0.35));
        assert!(close(stepped(0.5499, 0.05), 0.60), "just under a step counts as on it");
        assert!(close(stepped(0.02, -0.05), 0.0));
        assert!(close(stepped(1.48, 0.05), MAX_VOLUME));
        assert!(close(stepped(0.5, 0.0), 0.5));
    }

    #[test]
    fn levels_we_set_survive_stale_reports() {
        let mut live = Live::default();
        let mut volumes = ChannelVolumes::default();
        volumes.set(2, pa_volume(0.5));
        let device = AudioDevice { index: 7, name: "speakers".into(), volume: 0.5, ..Default::default() };
        live.put_device(Kind::Sink, (device.clone(), volumes));
        live.set_volume(Kind::Sink, 7, 0.8).unwrap();
        live.set_mute(Kind::Sink, 7, Some(true)).unwrap();
        // A report from before the change doesn't undo it…
        live.put_device(Kind::Sink, (device.clone(), volumes));
        assert!((live.volume_of(Kind::Sink, 7).unwrap() - 0.8).abs() < 0.01);
        assert!(live.sinks[&7].0.muted);
        // …and once the server reports it, the server is followed again.
        let mut reported = ChannelVolumes::default();
        reported.set(2, pa_volume(0.8));
        let mut muted = device.clone();
        muted.muted = true;
        live.put_device(Kind::Sink, (muted, reported));
        assert!(live.expects.is_empty());
        live.put_device(Kind::Sink, (device, volumes));
        assert!((live.volume_of(Kind::Sink, 7).unwrap() - 0.5).abs() < 0.01);
    }
}
