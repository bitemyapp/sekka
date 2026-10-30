//! What the Displays pane asks of the compositor (`aizome-ipc`): the
//! outputs, changing them for this session (`output-config`), and their
//! changes as they happen (`outputs-changed`). In the sample, a compositor
//! of its own with two displays that takes the same requests in memory.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aizome_ipc::{Event, Mode as IpcMode, OutputInfo, Request, Response, Transform};

/// The compositor, or the sample's.
#[derive(Clone)]
pub enum Compositor {
    Live,
    Sample(Arc<Mutex<SampleOutputs>>),
}

/// The sample's displays: those on (as the compositor lists them) and
/// those turned off (which it doesn't).
#[derive(Clone, Debug, Default)]
pub struct SampleOutputs {
    pub on: Vec<OutputInfo>,
    pub off: Vec<OutputInfo>,
    /// Requests taken, for tests.
    pub requests: Vec<Request>,
}

fn mode(w: i32, h: i32, hz: i32, preferred: bool) -> IpcMode {
    IpcMode { width: w, height: h, refresh: hz, preferred }
}

/// The sample: a laptop's 2880 × 1800 panel at 2×, and a 27-inch 4K
/// display at 2× to its right.
pub fn sample_outputs() -> SampleOutputs {
    let laptop = OutputInfo {
        name: "eDP-1".into(),
        make: "Samsung Display Corp.".into(),
        model: "ATNA40HQ10-0".into(),
        serial: "0x0000003F".into(),
        enabled: true,
        current_mode: Some(mode(2880, 1800, 120_000, false)),
        modes: vec![
            mode(2880, 1800, 120_000, false),
            mode(2880, 1800, 60_000, true),
            mode(2560, 1600, 60_000, false),
            mode(1920, 1200, 60_000, false),
            mode(1680, 1050, 60_000, false),
            mode(1280, 800, 60_000, false),
        ],
        scale: 2.0,
        transform: Transform::Normal,
        logical: aizome_ipc::Rect::new(0, 180, 1440, 900),
        usable: aizome_ipc::Rect::new(0, 204, 1440, 876),
        physical_size_mm: (300, 190),
        powered: true,
        ..OutputInfo::default()
    };
    let monitor = OutputInfo {
        name: "DP-2".into(),
        make: "ASUSTek COMPUTER INC".into(),
        model: "PG27UCDM".into(),
        serial: "T5LMAV009458".into(),
        enabled: true,
        current_mode: Some(mode(3840, 2160, 144_000, false)),
        modes: vec![
            mode(3840, 2160, 240_000, false),
            mode(3840, 2160, 144_000, false),
            mode(3840, 2160, 119_880, false),
            mode(3840, 2160, 60_000, true),
            mode(2560, 1440, 144_000, false),
            mode(1920, 1080, 60_000, false),
        ],
        scale: 2.0,
        transform: Transform::Normal,
        logical: aizome_ipc::Rect::new(1440, 0, 1920, 1080),
        usable: aizome_ipc::Rect::new(1440, 24, 1920, 1056),
        physical_size_mm: (597, 336),
        powered: true,
        ..OutputInfo::default()
    };
    SampleOutputs { on: vec![laptop, monitor], off: Vec::new(), requests: Vec::new() }
}

impl SampleOutputs {
    /// Take a request as the compositor would.
    fn apply(&mut self, req: &Request) -> Result<(), String> {
        self.requests.push(req.clone());
        if let Request::MirrorOutput { name, source } = req {
            // As the compositor mirrors: the source's place and size.
            let place = match source {
                Some(s) => Some(self.on.iter().find(|o| o.name == *s).ok_or_else(|| format!("no output {s}"))?.logical),
                None => None,
            };
            let o = self.on.iter_mut().find(|o| o.name == *name).ok_or_else(|| format!("no output {name}"))?;
            o.mirror = source.clone();
            if let Some(r) = place {
                o.logical = r;
                o.usable = r;
            }
            return Ok(());
        }
        if let Request::OutputVrr { name, mode } = req {
            let o = self.on.iter_mut().find(|o| o.name == *name).ok_or_else(|| format!("no output {name}"))?;
            let vrr = aizome_config::VrrMode::parse(mode).ok_or("invalid VRR policy")?;
            if !o.vrr_supported && vrr != aizome_config::VrrMode::Off {
                return Err("adaptive sync unavailable".into());
            }
            o.vrr = mode.clone();
            return Ok(());
        }
        let Request::OutputConfig { name, mode, scale, position, transform, enabled } = req else {
            return Err("the sample compositor takes only output-config and mirror-output".into());
        };
        if *enabled == Some(false) {
            let i = self.on.iter().position(|o| o.name == *name).ok_or_else(|| format!("no output {name}"))?;
            let o = self.on.remove(i);
            self.off.push(o);
            return Ok(());
        }
        if *enabled == Some(true)
            && let Some(i) = self.off.iter().position(|o| o.name == *name)
        {
            let o = self.off.remove(i);
            self.on.push(o);
        }
        let o = self.on.iter_mut().find(|o| o.name == *name).ok_or_else(|| format!("no output {name}"))?;
        if let Some(m) = mode {
            let (w, h, r) = aizome_config::parse_mode(m).ok_or_else(|| format!("bad mode {m}"))?;
            let found = o
                .modes
                .iter()
                .find(|x| x.width == w && x.height == h && r.is_none_or(|r| (x.refresh - r).abs() < 10))
                .cloned();
            o.current_mode = Some(found.ok_or_else(|| format!("{name} has no mode {m}"))?);
        }
        if let Some(s) = scale {
            if !(0.5..=4.0).contains(s) {
                return Err(format!("scale {s} is out of range"));
            }
            o.scale = *s;
        }
        if let Some(t) = transform {
            o.transform = *t;
        }
        if let Some([x, y]) = position {
            o.logical.x = *x;
            o.logical.y = *y;
        }
        let m = o.current_mode.clone().unwrap_or_default();
        let (w, h) =
            super::model::logical_size(&super::model::Mode::new(m.width, m.height, m.refresh), o.scale, o.transform);
        o.logical.w = w;
        o.logical.h = h;
        o.usable = o.logical;
        Ok(())
    }
}

impl Compositor {
    /// The outputs the compositor lists (those on).
    pub fn outputs(&self) -> Result<Vec<OutputInfo>, String> {
        match self {
            Compositor::Sample(s) => Ok(s.lock().unwrap_or_else(|e| e.into_inner()).on.clone()),
            Compositor::Live => {
                let mut c = aizome_ipc::Client::connect().map_err(no_compositor)?;
                let _ = c.stream().set_read_timeout(Some(Duration::from_secs(3)));
                match c.call(&Request::Outputs).map_err(|e| e.to_string())? {
                    Response::Outputs { outputs } => Ok(outputs),
                    other => Err(format!("the compositor answered {other:?}")),
                }
            }
        }
    }

    /// Send the requests, in order, then read the outputs back.
    pub fn apply(&self, requests: &[Request]) -> Result<Vec<OutputInfo>, String> {
        match self {
            Compositor::Sample(s) => {
                let mut s = s.lock().unwrap_or_else(|e| e.into_inner());
                for r in requests {
                    s.apply(r)?;
                }
                Ok(s.on.clone())
            }
            Compositor::Live => {
                let mut c = aizome_ipc::Client::connect().map_err(no_compositor)?;
                let _ = c.stream().set_read_timeout(Some(Duration::from_secs(10)));
                for r in requests {
                    c.call(r).map_err(|e| e.to_string())?;
                }
                match c.call(&Request::Outputs).map_err(|e| e.to_string())? {
                    Response::Outputs { outputs } => Ok(outputs),
                    other => Err(format!("the compositor answered {other:?}")),
                }
            }
        }
    }

    /// Follow the compositor's `outputs-changed` until `stop`, calling
    /// `changed` with each list; reconnects when the compositor restarts.
    pub fn watch(&self, stop: Arc<AtomicBool>, changed: impl Fn(Vec<OutputInfo>) -> bool + Send + 'static) {
        if !matches!(self, Compositor::Live) {
            return;
        }
        let spawned = std::thread::Builder::new().name("settei-outputs".into()).spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let events = aizome_ipc::Client::connect().and_then(|c| c.events());
                if let Ok(events) = events {
                    // Wake now and then to see whether to stop.
                    let _ = events.stream().set_read_timeout(Some(Duration::from_secs(1)));
                    for e in events {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        match e {
                            Ok(Event::OutputsChanged { outputs }) => {
                                if !changed(outputs) {
                                    return;
                                }
                            }
                            Ok(_) => {}
                            Err(e)
                                if matches!(
                                    e.kind(),
                                    std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::TimedOut
                                        | std::io::ErrorKind::InvalidData
                                ) => {}
                            Err(_) => break,
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
        if let Err(e) = spawned {
            log::warn!("no thread to follow the outputs: {e}");
        }
    }
}

fn no_compositor(e: std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "Aizome's compositor isn't running here (no AIZOME_SOCKET)".into(),
        std::io::ErrorKind::ConnectionRefused => "Aizome's compositor isn't answering".into(),
        _ => format!("Couldn't reach the compositor: {e}"),
    }
}
