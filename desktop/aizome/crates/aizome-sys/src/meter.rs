//! A level meter for a sound input (or an output, through its monitor):
//! how loud it is, about 25 times a second, for Settings' input level and
//! anything else that shows one.
//!
//! [`Meter::start`] opens a connection of its own to the sound server
//! (pipewire-pulse) on a thread, and records from the device with
//! PulseAudio's peak detection: the server sends one value per interval,
//! the loudest sample in it, so the meter costs next to nothing. The
//! stream is named "Peak detect", which [`crate::audio`] leaves out of the
//! recording applications it lists. Dropping the meter stops it.

use crate::service::{Stop, Wake};
use libpulse_binding as pulse;
use pulse::context::{Context, FlagSet, State};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::proplist::Proplist;
use pulse::stream::{FlagSet as StreamFlags, PeekResult, Stream};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// How many levels a second the server sends.
pub const RATE: u32 = 25;

/// A level meter; see the module's documentation.
pub struct Meter {
    level: Arc<AtomicU32>,
    stop: Stop,
}

impl Meter {
    /// Follow `device`: a source's name, or a sink's name with `.monitor`
    /// for what an output plays. `wake` is called with each new level.
    pub fn start(device: &str, wake: Wake) -> Meter {
        Meter::start_on(None, device, wake)
    }

    /// The same on the server at `server` (a PulseAudio server string).
    pub fn start_on(server: Option<String>, device: &str, wake: Wake) -> Meter {
        let level = Arc::new(AtomicU32::new(0));
        let stop = Stop::default();
        let (l, s, device) = (level.clone(), stop.clone(), device.to_owned());
        let spawned = std::thread::Builder::new().name("aizome-sys meter".into()).spawn(move || {
            if let Err(e) = run(server.as_deref(), &device, &l, &wake, &s) {
                log::debug!("aizome-sys: level meter for {device}: {e}");
            }
        });
        if let Err(e) = spawned {
            log::warn!("aizome-sys: no thread for a level meter: {e}");
        }
        Meter { level, stop }
    }

    /// The latest level: the loudest sample of the last interval, 0.0–1.0
    /// (linear amplitude; see [`loudness`] for a meter's scale).
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

/// A peak as a meter shows it: decibels from −60 to 0 spread over 0.0–1.0.
pub fn loudness(peak: f32) -> f32 {
    if peak.is_nan() || peak <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * peak.log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn run(server: Option<&str>, device: &str, level: &Arc<AtomicU32>, wake: &Wake, stop: &Stop) -> Result<(), String> {
    let mut mainloop = Mainloop::new().ok_or("no main loop")?;
    let mut proplist = Proplist::new().ok_or("no proplist")?;
    let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_NAME, "Aizome level meter");
    let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_ID, "org.sekka.Aizome");
    let mut context = Context::new_with_proplist(&mainloop, "Aizome level meter", &proplist).ok_or("no context")?;
    context.connect(server, FlagSet::NOAUTOSPAWN, None).map_err(|e| format!("{e:?}"))?;
    loop {
        if stop.stopped() {
            return Ok(());
        }
        if let IterateResult::Quit(_) | IterateResult::Err(_) = mainloop.iterate(true) {
            return Err("the main loop stopped".into());
        }
        match context.get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => return Err("the sound server isn't there".into()),
            _ => {}
        }
    }
    let spec = pulse::sample::Spec { format: pulse::sample::Format::FLOAT32NE, channels: 1, rate: RATE };
    let stream = Stream::new(&mut context, "Peak detect", &spec, None).ok_or("no stream")?;
    let stream = Rc::new(RefCell::new(stream));
    {
        let s = stream.clone();
        let (level, wake) = (level.clone(), wake.clone());
        stream.borrow_mut().set_read_callback(Some(Box::new(move |_| {
            let mut s = s.borrow_mut();
            loop {
                match s.peek() {
                    Ok(PeekResult::Data(data)) => {
                        let peak =
                            data.as_chunks::<4>().0.iter().map(|b| f32::from_ne_bytes(*b).abs()).fold(0.0f32, f32::max);
                        level.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                        let _ = s.discard();
                        wake.wake();
                    }
                    Ok(PeekResult::Hole(_)) => {
                        let _ = s.discard();
                    }
                    Ok(PeekResult::Empty) | Err(_) => break,
                }
            }
        })));
    }
    let attr = pulse::def::BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        // One value at a time.
        fragsize: 4,
    };
    stream
        .borrow_mut()
        .connect_record(
            Some(device),
            Some(&attr),
            StreamFlags::PEAK_DETECT | StreamFlags::ADJUST_LATENCY | StreamFlags::DONT_MOVE,
        )
        .map_err(|e| format!("{e:?}"))?;
    // Iterate with a timeout of our own, so stopping is noticed.
    while !stop.stopped() {
        match mainloop.prepare(Some(pulse::time::MicroSeconds(250_000))) {
            Ok(()) => {}
            Err(e) => return Err(format!("{e:?}")),
        }
        if mainloop.poll().is_err() {
            return Err("polling failed".into());
        }
        if mainloop.dispatch().is_err() {
            return Err("dispatching failed".into());
        }
        if matches!(context.get_state(), State::Failed | State::Terminated) {
            return Err("the sound server left".into());
        }
    }
    let _ = stream.borrow_mut().disconnect();
    level.store(0f32.to_bits(), Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loudness_spreads_decibels() {
        assert_eq!(loudness(0.0), 0.0);
        assert_eq!(loudness(1.0), 1.0);
        assert!((loudness(0.1) - 2.0 / 3.0).abs() < 0.01, "−20 dB is two thirds up");
        assert_eq!(loudness(0.0001), 0.0, "below −60 dB is silence");
        assert_eq!(loudness(f32::NAN), 0.0);
    }
}
