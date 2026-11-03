//! Audio through PipeWire: a playback stream on a thread of its own,
//! taking the ring in its process callback on PipeWire's real-time
//! thread.
//!
//! The stream asks the graph for a long cycle (`node.latency`): with
//! nothing else playing, PipeWire then wakes the device, its own daemon
//! and this process a few times a second for music rather than fifty,
//! which is most of what playing audio costs when the screen is off.
//! Long cycles cost nothing in responsiveness here: pausing drops what
//! the graph holds and steps the ring back (see `audio`), and seeking
//! flushes; only volume changes wait for the next cycle. The stream says
//! its role (Movie or Music), its application and what's playing, so
//! the desktop's volume controls show it; its volume is a stream volume
//! PipeWire applies.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use pipewire as pw;
use pw::spa;

use crate::audio::{Output, Ring, amplitude, fill};
use crate::av::ChannelLayout;
use crate::clock::Clock;

mod routing;
use routing::{Routes, Routing};

pub enum OutputEvent {
    Devices(Vec<AudioDevice>),
    Routed(Vec<AudioDevice>),
    Volume(f32, bool),
    Lost,
}

#[derive(Clone)]
struct Controls {
    master: f32,
    channels: Vec<f32>,
    muted: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self { master: 1.0, channels: vec![1.0], muted: false }
    }
}

/// An output advertised by PipeWire. Names survive reconnects; numeric
/// registry IDs do not, so preferences and target.object use the name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

/// What the stream is for: how long its cycles may be, and its role.
#[derive(Clone, Debug)]
pub struct StreamKind {
    /// Seconds a graph cycle may be (the stream's `node.latency`).
    pub latency: f64,
    /// `media.role`: "Movie" or "Music".
    pub role: &'static str,
    pub app_name: String,
    pub app_id: String,
}

enum Cmd {
    Open { ring: Arc<Ring>, clock: Arc<Clock>, media: String, kind: StreamKind, device: Option<String>, reply: mpsc::Sender<Result<(), String>> },
    Playing(bool, mpsc::Sender<()>),
    Flush,
    Volume(f32, bool),
    Close(mpsc::Sender<()>),
    Quit,
}

/// PipeWire playback. Connecting fails where there's no PipeWire.
pub struct PipeWireOutput {
    tx: pw::channel::Sender<Cmd>,
    thread: Option<std::thread::JoinHandle<()>>,
    kind: StreamKind,
    describe: Arc<Mutex<String>>,
    device: Option<String>,
    lost: Arc<AtomicBool>,
}

impl PipeWireOutput {
    /// Connect to PipeWire (on a thread of its own): an error where it
    /// can't.
    pub fn connect(kind: StreamKind) -> Result<PipeWireOutput, String> {
        Self::connect_with_events(kind, |_| {})
    }

    pub fn connect_with_events(
        kind: StreamKind,
        events: impl Fn(OutputEvent) + Send + Sync + 'static,
    ) -> Result<PipeWireOutput, String> {
        let (tx, rx) = pw::channel::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let describe = Arc::new(Mutex::new(String::from("PipeWire")));
        let d = describe.clone();
        let lost = Arc::new(AtomicBool::new(false));
        let l = lost.clone();
        let thread = std::thread::Builder::new()
            .name("aizome-media-audio".into())
            .spawn(move || run(rx, ready_tx, d, l, Arc::new(events)))
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(PipeWireOutput { tx, thread: Some(thread), kind, describe, device: None, lost }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("PipeWire's thread stopped".into()),
        }
    }
}

impl Output for PipeWireOutput {
    fn take_loss(&mut self) -> bool {
        self.lost.swap(false, Ordering::AcqRel)
    }

    fn set_device(&mut self, device: Option<&str>) {
        self.device = device.map(str::to_owned);
    }

    fn set_stream(&mut self, latency: f64, role: &'static str) {
        self.kind.latency = latency;
        self.kind.role = role;
    }

    fn open(&mut self, ring: Arc<Ring>, clock: Arc<Clock>, media: &str) -> Result<(), String> {
        let (reply, rx) = mpsc::channel();
        let cmd = Cmd::Open { ring, clock, media: media.to_owned(), kind: self.kind.clone(), device: self.device.clone(), reply };
        if self.tx.send(cmd).is_err() {
            return Err("PipeWire's thread stopped".into());
        }
        rx.recv().unwrap_or_else(|_| Err("PipeWire's thread stopped".into()))
    }

    fn set_playing(&mut self, playing: bool) {
        let (reply, rx) = mpsc::channel();
        if self.tx.send(Cmd::Playing(playing, reply)).is_ok() {
            let _ = rx.recv();
        }
    }

    fn flush(&mut self) {
        let _ = self.tx.send(Cmd::Flush);
    }

    fn set_volume(&mut self, volume: f32, muted: bool) {
        let _ = self.tx.send(Cmd::Volume(volume, muted));
    }

    fn close(&mut self) {
        let (reply, rx) = mpsc::channel();
        if self.tx.send(Cmd::Close(reply)).is_ok() {
            let _ = rx.recv();
        }
    }

    fn describe(&self) -> String {
        self.describe.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Drop for PipeWireOutput {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The stream as it plays, and what its callback reads.
struct Playing {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
    ring: Arc<Ring>,
    clock: Arc<Clock>,
    channels: u32,
    active: bool,
}

#[derive(Clone)]
struct StreamContext {
    controls: Rc<RefCell<Controls>>,
    events: Arc<dyn Fn(OutputEvent) + Send + Sync>,
    routing: Rc<RefCell<Routing>>,
    report: Rc<dyn Fn()>,
    loss: Rc<dyn Fn()>,
}

struct WatchedDevice {
    _device: Rc<pw::device::Device>,
    _params: pw::device::DeviceListener,
    _done: pw::core::Listener,
}

fn run(
    rx: pw::channel::Receiver<Cmd>,
    ready: mpsc::Sender<Result<(), String>>,
    describe: Arc<Mutex<String>>,
    lost: Arc<AtomicBool>,
    events: Arc<dyn Fn(OutputEvent) + Send + Sync>,
) {
    pw::init();
    let setup = || -> Result<(pw::main_loop::MainLoopRc, pw::core::CoreRc), pw::Error> {
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        Ok((mainloop, core))
    };
    let (mainloop, core) = match setup() {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(format!("PipeWire: {e}")));
            return;
        }
    };
    let registry = match core.get_registry_rc() {
        Ok(r) => r,
        Err(e) => {
            let _ = ready.send(Err(format!("PipeWire outputs: {e}")));
            return;
        }
    };
    let current: Rc<RefCell<Option<Playing>>> = Default::default();
    let routing = Rc::new(RefCell::new(Routing::default()));
    let controls = Rc::new(RefCell::new(Controls::default()));
    let preference = Rc::new(RefCell::new(None::<String>));
    let format = Rc::new(RefCell::new(String::from("PipeWire")));
    let report: Rc<dyn Fn()> = {
        let routing = routing.clone();
        let describe = describe.clone();
        let events = events.clone();
        let preference = preference.clone();
        let format = format.clone();
        let last = RefCell::new(None);
        Rc::new(move || {
            let routing = routing.borrow();
            let actual = routing.devices();
            let label = if actual.is_empty() {
                "unavailable".into()
            } else {
                actual.iter().map(|d| d.description.as_str()).collect::<Vec<_>>().join(", ")
            };
            let preference = preference.borrow();
            let preferred = match preference.as_ref() {
                None => "System Default",
                Some(name) => routing
                    .sinks
                    .values()
                    .find(|d| &d.name == name)
                    .map_or("Saved output (unavailable)", |d| d.description.as_str()),
            };
            let description = format!("{} · current output: {label} · preferred output: {preferred}", format.borrow());
            if last.borrow().as_ref() != Some(&description) {
                *last.borrow_mut() = Some(description.clone());
                *describe.lock().unwrap_or_else(|e| e.into_inner()) = description;
                events(OutputEvent::Routed(actual));
            }
        })
    };
    let loss: Rc<dyn Fn()> = {
        let current = current.clone();
        let lost = lost.clone();
        let events = events.clone();
        Rc::new(move || {
            if let Some(p) = current.borrow_mut().as_mut() {
                // Stop in this loop, before policy can link a fallback.
                // The controller clears want_play and updates video/UI.
                set_playing(p, false);
                if !lost.swap(true, Ordering::AcqRel) {
                    events(OutputEvent::Lost);
                }
            }
        })
    };
    let cards: Rc<RefCell<std::collections::BTreeMap<u32, WatchedDevice>>> = Default::default();
    let added = routing.clone();
    let removed = routing.clone();
    let changed = events.clone();
    let removed_events = events.clone();
    let add_report = report.clone();
    let remove_report = report.clone();
    let remove_loss = loss.clone();
    let add_loss = loss.clone();
    let add_cards = cards.clone();
    let remove_cards = cards.clone();
    let reg = registry.clone();
    let card_core = core.clone();
    let _registry_listener = registry
        .add_listener_local()
        .global(move |object| {
            let Some(props) = object.props else { return };
            if object.type_ == pw::types::ObjectType::Device {
                let Ok(device) = reg.bind::<pw::device::Device, _>(object) else { return };
                let device = Rc::new(device);
                let weak = Rc::downgrade(&device);
                let routes = Rc::new(RefCell::new(Routes::default()));
                let pending = Rc::new(std::cell::Cell::new(None));
                let r = routes.clone();
                let p = pending.clone();
                let c = card_core.clone();
                let listener = device
                    .add_listener_local()
                    .param(move |_, kind, _, _, pod| {
                        if let Some(pod) = pod {
                            r.borrow_mut().param(kind, pod);
                            // Read availability afresh when policy changes
                            // the active route. A round trip alone would not
                            // cover an availability event delivered later.
                            if kind == spa::param::ParamType::Route
                                && let Some(device) = weak.upgrade()
                            {
                                device.enum_params(0, Some(spa::param::ParamType::EnumRoute), 0, u32::MAX);
                            }
                            p.set(c.sync(0).ok());
                        }
                    })
                    .register();
                let graph = added.clone();
                let loss = add_loss.clone();
                let id = object.id;
                let done = card_core
                    .add_listener_local()
                    .done(move |_, seq| {
                        if pending.get() != Some(seq) {
                            return;
                        }
                        pending.set(None);
                        let unavailable = routes.borrow_mut().commit();
                        let graph = graph.borrow();
                        let affected = graph.actual.iter().any(|sink| {
                            graph
                                .cards
                                .get(sink)
                                .is_some_and(|(card, device)| *card == id && unavailable.contains(device))
                        });
                        drop(graph);
                        if affected {
                            loss();
                        }
                    })
                    .register();
                device.subscribe_params(&[spa::param::ParamType::EnumRoute, spa::param::ParamType::Route]);
                add_cards
                    .borrow_mut()
                    .insert(object.id, WatchedDevice { _device: device, _params: listener, _done: done });
                return;
            }
            if object.type_ == pw::types::ObjectType::Link {
                if let (Some(from), Some(to)) = (
                    props.get("link.output.node").and_then(|v| v.parse().ok()),
                    props.get("link.input.node").and_then(|v| v.parse().ok()),
                ) {
                    let mut graph = added.borrow_mut();
                    graph.links.insert(object.id, (from, to));
                    graph.linked();
                    drop(graph);
                    add_report();
                }
                return;
            }
            if object.type_ != pw::types::ObjectType::Node {
                return;
            }
            if props.get("media.class") != Some("Audio/Sink") {
                return;
            }
            let Some(name) = props.get("node.name") else { return };
            let description = props.get("node.description").or_else(|| props.get("node.nick")).unwrap_or(name);
            let mut graph = added.borrow_mut();
            graph.sinks.insert(object.id, AudioDevice { name: name.into(), description: description.into() });
            if let (Some(card), Some(device)) = (
                props.get("device.id").and_then(|v| v.parse().ok()),
                props.get("card.profile.device").and_then(|v| v.parse().ok()),
            ) {
                graph.cards.insert(object.id, (card, device));
            }
            graph.linked();
            publish_devices(&graph.sinks, &changed);
            drop(graph);
            add_report();
        })
        .global_remove(move |id| {
            remove_cards.borrow_mut().remove(&id);
            let mut graph = removed.borrow_mut();
            let was_sink = graph.sinks.contains_key(&id);
            let affected = graph.remove(id);
            if was_sink {
                publish_devices(&graph.sinks, &removed_events);
            }
            drop(graph);
            if affected {
                remove_loss();
            }
            remove_report();
        })
        .register();
    let core_loss = loss.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |id, _, _, _| {
            if id == pw::core::PW_ID_CORE {
                core_loss();
            }
        })
        .register();
    let _ = ready.send(Ok(()));
    let ml = mainloop.clone();
    let cur = current.clone();
    let context =
        StreamContext { controls: controls.clone(), events, routing: routing.clone(), report: report.clone(), loss };
    let _attached = rx.attach(mainloop.loop_(), move |cmd| match cmd {
        Cmd::Open { ring, clock, media, kind, device, reply } => {
            routing.borrow_mut().reset();
            *preference.borrow_mut() = device.clone();
            let f = ring.format;
            *format.borrow_mut() = format!(
                "PipeWire, {} Hz, {} channel{}, {} ms cycles",
                f.rate,
                f.channels,
                if f.channels == 1 { "" } else { "s" },
                (kind.latency * 1000.0).round()
            );
            close(&cur);
            let r = open(&core, ring, clock, &media, &kind, device.as_deref(), &context);
            match r {
                Ok(p) => {
                    *describe.lock().unwrap_or_else(|e| e.into_inner()) = format!(
                        "PipeWire · connecting · preferred output: {}",
                        device.as_deref().unwrap_or("System Default")
                    );
                    *cur.borrow_mut() = Some(p);
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            }
        }
        Cmd::Playing(on, reply) => {
            if let Some(p) = cur.borrow_mut().as_mut() {
                set_playing(p, on && !lost.load(Ordering::Acquire));
            }
            let _ = reply.send(());
        }
        Cmd::Flush => {
            if let Some(p) = cur.borrow().as_ref() {
                let _ = p.stream.flush(false);
            }
        }
        Cmd::Volume(v, muted) => {
            *controls.borrow_mut() = Controls { master: 1.0, channels: vec![amplitude(v)], muted };
            if let Some(p) = cur.borrow_mut().as_mut() {
                apply_volume(&p.stream, p.channels, &controls.borrow());
            }
        }
        Cmd::Close(reply) => {
            routing.borrow_mut().reset();
            close(&cur);
            report();
            let _ = reply.send(());
        }
        Cmd::Quit => {
            close(&cur);
            ml.quit();
        }
    });
    mainloop.run();
}

fn publish_devices(
    outputs: &std::collections::BTreeMap<u32, AudioDevice>,
    changed: &Arc<dyn Fn(OutputEvent) + Send + Sync>,
) {
    let mut list: Vec<_> = outputs.values().cloned().collect();
    list.sort_by(|a, b| a.description.cmp(&b.description).then_with(|| a.name.cmp(&b.name)));
    changed(OutputEvent::Devices(list));
}

fn close(cur: &std::rc::Rc<std::cell::RefCell<Option<Playing>>>) {
    if let Some(p) = cur.borrow_mut().take() {
        let _ = p.stream.set_active(false);
        let _ = p.stream.disconnect();
    }
}

/// Channel positions for `n` channels, as PipeWire names them: the layout
/// the player's resampler mixes them to ([`ChannelLayout::mixed`], FFmpeg's
/// usual one for `n`: 2.1 for three, 4.0 for four, 5.1(back) for six),
/// channel by channel, so each is placed where FFmpeg put it.
fn positions(n: u32) -> [u32; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize] {
    use crate::av::ff::AVChannel::*;
    use spa::sys::*;
    let mut p = [SPA_AUDIO_CHANNEL_UNKNOWN; SPA_AUDIO_MAX_CHANNELS as usize];
    let n = n.clamp(1, SPA_AUDIO_MAX_CHANNELS);
    let layout = ChannelLayout::mixed(n as i32);
    for (i, slot) in p.iter_mut().enumerate().take(n as usize) {
        *slot = match layout.channel(i as u32) {
            // FFmpeg's mono is a centre channel; PipeWire's is MONO.
            AV_CHAN_FRONT_CENTER if n == 1 => SPA_AUDIO_CHANNEL_MONO,
            AV_CHAN_FRONT_LEFT => SPA_AUDIO_CHANNEL_FL,
            AV_CHAN_FRONT_RIGHT => SPA_AUDIO_CHANNEL_FR,
            AV_CHAN_FRONT_CENTER => SPA_AUDIO_CHANNEL_FC,
            AV_CHAN_LOW_FREQUENCY => SPA_AUDIO_CHANNEL_LFE,
            AV_CHAN_BACK_LEFT => SPA_AUDIO_CHANNEL_RL,
            AV_CHAN_BACK_RIGHT => SPA_AUDIO_CHANNEL_RR,
            AV_CHAN_FRONT_LEFT_OF_CENTER => SPA_AUDIO_CHANNEL_FLC,
            AV_CHAN_FRONT_RIGHT_OF_CENTER => SPA_AUDIO_CHANNEL_FRC,
            AV_CHAN_BACK_CENTER => SPA_AUDIO_CHANNEL_RC,
            AV_CHAN_SIDE_LEFT => SPA_AUDIO_CHANNEL_SL,
            AV_CHAN_SIDE_RIGHT => SPA_AUDIO_CHANNEL_SR,
            AV_CHAN_TOP_CENTER => SPA_AUDIO_CHANNEL_TC,
            AV_CHAN_TOP_FRONT_LEFT => SPA_AUDIO_CHANNEL_TFL,
            AV_CHAN_TOP_FRONT_CENTER => SPA_AUDIO_CHANNEL_TFC,
            AV_CHAN_TOP_FRONT_RIGHT => SPA_AUDIO_CHANNEL_TFR,
            AV_CHAN_TOP_BACK_LEFT => SPA_AUDIO_CHANNEL_TRL,
            AV_CHAN_TOP_BACK_CENTER => SPA_AUDIO_CHANNEL_TRC,
            AV_CHAN_TOP_BACK_RIGHT => SPA_AUDIO_CHANNEL_TRR,
            AV_CHAN_WIDE_LEFT => SPA_AUDIO_CHANNEL_FLW,
            AV_CHAN_WIDE_RIGHT => SPA_AUDIO_CHANNEL_FRW,
            AV_CHAN_LOW_FREQUENCY_2 => SPA_AUDIO_CHANNEL_LFE2,
            _ => SPA_AUDIO_CHANNEL_UNKNOWN,
        };
    }
    p
}

/// Frames in a cycle of `secs` at `rate`, as a power of two (the graph's
/// quanta are).
pub fn quantum(secs: f64, rate: u32) -> u32 {
    let frames = (secs * f64::from(rate)).max(64.0);
    (frames.log2().round().exp2() as u32).clamp(64, 8192)
}

fn open(
    core: &pw::core::CoreRc,
    ring: Arc<Ring>,
    clock: Arc<Clock>,
    media: &str,
    kind: &StreamKind,
    device: Option<&str>,
    context: &StreamContext,
) -> Result<Playing, String> {
    let StreamContext { controls, events, routing, report, loss } = context.clone();
    let fmt = ring.format;
    let channels = fmt.channels.clamp(1, 8);
    let latency = format!("{}/{}", quantum(kind.latency, fmt.rate), fmt.rate);
    let mut props = pw::properties::properties! {
        "media.type" => "Audio",
        "media.category" => "Playback",
        "media.role" => kind.role,
        "media.name" => media,
        "application.name" => kind.app_name.as_str(),
        "application.id" => kind.app_id.as_str(),
        "application.icon-name" => kind.app_id.as_str(),
        "node.name" => kind.app_name.to_lowercase().as_str(),
        // Utsushi owns this preference. A remembered external route must
        // not override choosing System Default when this stream reopens.
        "state.restore-target" => "false",
        // The player restores its own volume/mute too. WirePlumber's
        // default/restored volumes otherwise overwrite them on reconnect.
        "state.restore-props" => "false",
        "node.latency" => latency.as_str(),
        "audio.channels" => channels.to_string().as_str(),
    };
    if let Some(name) = device {
        props.insert("target.object", name);
    }
    let stream = pw::stream::StreamRc::new(core.clone(), &kind.app_name, props).map_err(|e| e.to_string())?;
    let r = ring.clone();
    let c = clock.clone();
    let stride = (channels as usize) * std::mem::size_of::<f32>();
    let ready_volume = controls.clone();
    let volume_initialized = Rc::new(std::cell::Cell::new(false));
    let initialized = volume_initialized.clone();
    let listener = stream
        .add_local_listener::<()>()
        .state_changed(move |stream, _, _, state| {
            if matches!(state, pw::stream::StreamState::Error(_)) {
                loss();
                return;
            }
            // Commands can arrive before the new stream's controls exist.
            // Reapply only at the first connection, including a stream
            // opened paused. Later pause/resume transitions must preserve
            // volume and mute changes made through the desktop mixer.
            if matches!(state, pw::stream::StreamState::Paused | pw::stream::StreamState::Streaming)
                && !volume_initialized.replace(true)
            {
                let volume = ready_volume.borrow().clone();
                apply_volume(stream, channels, &volume);
                let mut graph = routing.borrow_mut();
                graph.stream = Some(stream.node_id());
                graph.linked();
                drop(graph);
                report();
            }
        })
        .param_changed(move |_, _, id, pod| {
            if id != spa::sys::SPA_PARAM_Props || !initialized.get() {
                return;
            }
            let Some(pod) = pod else { return };
            use spa::pod::Value;
            let Ok((_, Value::Object(object))) =
                spa::pod::deserialize::PodDeserializer::deserialize_any_from(pod.as_bytes())
            else {
                return;
            };
            let mut controls = controls.borrow_mut();
            for p in object.properties {
                match p.value {
                    Value::Float(master)
                        if p.key == spa::sys::SPA_PROP_volume && master.is_finite() && master >= 0.0 =>
                    {
                        controls.master = master;
                    }
                    Value::Bool(muted) if p.key == spa::sys::SPA_PROP_mute => controls.muted = muted,
                    Value::ValueArray(spa::pod::ValueArray::Float(channels))
                        if p.key == spa::sys::SPA_PROP_channelVolumes
                            && !channels.is_empty()
                            && channels.iter().all(|v| v.is_finite() && *v >= 0.0) =>
                    {
                        controls.channels = channels
                    }
                    _ => (),
                }
            }
            events(OutputEvent::Volume(
                (controls.master * controls.channels.iter().copied().fold(0.0, f32::max)).cbrt(),
                controls.muted,
            ));
        })
        .process(move |stream, _| {
            // SAFETY: a buffer dequeued is ours until queued; its first
            // data's memory is mapped (MAP_BUFFERS), maxsize bytes.
            unsafe {
                let b = stream.dequeue_raw_buffer();
                if b.is_null() {
                    return;
                }
                let buf = (*b).buffer;
                let d = &mut *(*buf).datas;
                if d.data.is_null() {
                    stream.queue_raw_buffer(b);
                    return;
                }
                let room = d.maxsize as usize / stride;
                let want = if (*b).requested > 0 { ((*b).requested as usize).min(room) } else { room };
                let out = std::slice::from_raw_parts_mut(d.data as *mut f32, want * channels as usize);
                let (latency, now) = match stream.time() {
                    Ok(t) => {
                        let rate = t.rate();
                        let delay = if rate.denom > 0 {
                            t.delay().max(0) as f64 * f64::from(rate.num) / f64::from(rate.denom)
                        } else {
                            0.0
                        };
                        let frames = delay * f64::from(fmt.rate) + (t.buffered() + t.queued()) as f64;
                        (frames as u64, t.now().max(0) as u64)
                    }
                    Err(_) => (0, crate::clock::monotonic_now()),
                };
                fill(&r, &c, out, latency, now);
                let chunk = &mut *d.chunk;
                chunk.offset = 0;
                chunk.stride = stride as i32;
                chunk.size = (want * stride) as u32;
                (*b).size = want as u64;
                stream.queue_raw_buffer(b);
            }
        })
        .register()
        .map_err(|e| e.to_string())?;
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(fmt.rate);
    info.set_channels(channels);
    info.set_position(positions(channels));
    let bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .map_err(|e| format!("{e:?}"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&bytes).ok_or("an audio format PipeWire won't read")?];
    stream
        .connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS
                | pw::stream::StreamFlags::INACTIVE,
            &mut params,
        )
        .map_err(|e| e.to_string())?;
    Ok(Playing { stream, _listener: listener, ring, clock, channels, active: false })
}

fn apply_volume(stream: &pw::stream::Stream, channels: u32, controls: &Controls) {
    use spa::pod::{Object, Property, Value, ValueArray};
    // Send Props directly: an inactive stream may not have advertised
    // its controls yet. This also avoids pipewire-rs 0.10.1's variadic
    // set_control wrapper, which omits the required terminating 0u32.
    let props = Value::Object(Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Props,
        id: spa::sys::SPA_PARAM_Props,
        properties: vec![
            Property::new(spa::sys::SPA_PROP_volume, Value::Float(controls.master)),
            Property::new(
                spa::sys::SPA_PROP_channelVolumes,
                Value::ValueArray(ValueArray::Float(if controls.channels.len() == channels as usize {
                    controls.channels.clone()
                } else {
                    vec![controls.channels.iter().copied().fold(0.0, f32::max); channels as usize]
                })),
            ),
            Property::new(spa::sys::SPA_PROP_mute, Value::Bool(controls.muted)),
        ],
    });
    let Ok((bytes, _)) = spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &props) else {
        return;
    };
    let bytes = bytes.into_inner();
    let Some(pod) = spa::pod::Pod::from_bytes(&bytes) else { return };
    // SAFETY: called on the stream's main loop; the serialized pod lives
    // throughout the synchronous call and contains only owned values.
    unsafe {
        pw::sys::pw_stream_set_param(stream.as_raw_ptr(), spa::sys::SPA_PARAM_Props, pod.as_raw_ptr());
    }
}

fn set_playing(p: &mut Playing, on: bool) {
    if on == p.active {
        return;
    }
    p.active = on;
    if on {
        p.ring.halted.store(false, std::sync::atomic::Ordering::Release);
        let _ = p.stream.set_active(true);
        return;
    }
    // Stop, then step back to what was being heard: what the graph held
    // goes, and plays again from the ring.
    let heard = p.clock.now();
    p.ring.halted.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = p.stream.set_active(false);
    while p.ring.busy.load(std::sync::atomic::Ordering::SeqCst) {
        std::hint::spin_loop();
    }
    let _ = p.stream.flush(false);
    let to = p.ring.index_of(heard).map(|i| p.ring.rewind_to(i));
    let pos = to.and_then(|i| p.ring.pts_of(i, true)).unwrap_or(heard);
    p.clock.stop_at(pos);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_are_powers_of_two_within_the_graphs_limits() {
        assert_eq!(quantum(0.170, 48_000), 8192);
        assert_eq!(quantum(0.085, 48_000), 4096);
        assert_eq!(quantum(0.040, 48_000), 2048);
        assert_eq!(quantum(10.0, 48_000), 8192);
        assert_eq!(quantum(0.0, 48_000), 64);
    }

    #[test]
    fn channels_are_placed_where_ffmpeg_puts_them() {
        use spa::sys::*;
        const FL: u32 = SPA_AUDIO_CHANNEL_FL;
        const FR: u32 = SPA_AUDIO_CHANNEL_FR;
        const FC: u32 = SPA_AUDIO_CHANNEL_FC;
        const LFE: u32 = SPA_AUDIO_CHANNEL_LFE;
        const RL: u32 = SPA_AUDIO_CHANNEL_RL;
        const RR: u32 = SPA_AUDIO_CHANNEL_RR;
        const RC: u32 = SPA_AUDIO_CHANNEL_RC;
        const SL: u32 = SPA_AUDIO_CHANNEL_SL;
        const SR: u32 = SPA_AUDIO_CHANNEL_SR;
        // FFmpeg's default layouts, which the resampler mixes to.
        let want: [&[u32]; 8] = [
            &[SPA_AUDIO_CHANNEL_MONO],
            &[FL, FR],
            &[FL, FR, LFE],                     // 2.1
            &[FL, FR, FC, RC],                  // 4.0
            &[FL, FR, FC, RL, RR],              // 5.0(back)
            &[FL, FR, FC, LFE, RL, RR],         // 5.1(back)
            &[FL, FR, FC, LFE, RC, SL, SR],     // 6.1
            &[FL, FR, FC, LFE, RL, RR, SL, SR], // 7.1
        ];
        for (n, want) in (1..).zip(want) {
            let p = positions(n);
            assert_eq!(&p[..n as usize], want, "{n} channels");
            assert!(p[n as usize..].iter().all(|&c| c == SPA_AUDIO_CHANNEL_UNKNOWN), "{n} channels");
        }
    }
}
