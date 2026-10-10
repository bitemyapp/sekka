// SPDX-License-Identifier: MIT OR Apache-2.0
//! A watch on the interface thread, for finding what makes the window
//! stutter. With `YUKIMI_STALLS` set (to a number of milliseconds, 50 when
//! empty or not a number), every time the interface thread is busy for
//! longer than that, standard error says for how long and what it was doing.
//!
//! The interface thread marks a heartbeat every few milliseconds; another
//! thread notices when the heartbeat stops and, when it starts again, how
//! long it stopped for.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gtk::glib;

static THRESHOLD: OnceLock<Option<Duration>> = OnceLock::new();
static DOING: Mutex<&'static str> = Mutex::new("waiting");

fn threshold() -> Option<Duration> {
    *THRESHOLD.get_or_init(|| {
        let value = std::env::var("YUKIMI_STALLS").ok()?;
        Some(Duration::from_millis(value.trim().parse().unwrap_or(50)))
    })
}

/// Say what the interface thread is doing until the returned guard goes,
/// for the report of a stall that happens meanwhile.
pub fn doing(what: &'static str) -> Doing {
    let before = threshold().map(|_| std::mem::replace(&mut *DOING.lock().unwrap_or_else(|p| p.into_inner()), what));
    Doing(before)
}

pub struct Doing(Option<&'static str>);

impl Drop for Doing {
    fn drop(&mut self) {
        if let Some(before) = self.0 {
            *DOING.lock().unwrap_or_else(|p| p.into_inner()) = before;
        }
    }
}

/// Start watching, if asked to.
pub fn watch() {
    let Some(threshold) = threshold() else {
        return;
    };
    let start = Instant::now();
    let beat = std::sync::Arc::new(AtomicU64::new(0));
    {
        let beat = beat.clone();
        glib::timeout_add_local(Duration::from_millis(5), move || {
            beat.store(start.elapsed().as_millis() as u64, Ordering::Relaxed);
            glib::ControlFlow::Continue
        });
    }
    std::thread::spawn(move || {
        let mut stalled: Option<(u64, &'static str)> = None;
        loop {
            std::thread::sleep(Duration::from_millis(5));
            let now = start.elapsed().as_millis() as u64;
            let last = beat.load(Ordering::Relaxed);
            match stalled {
                None if now.saturating_sub(last) >= threshold.as_millis() as u64 => {
                    stalled = Some((last, *DOING.lock().unwrap_or_else(|p| p.into_inner())));
                }
                Some((since, what)) if last > since => {
                    eprintln!("yukimi: the interface was busy for {} ms ({what})", last - since);
                    stalled = None;
                }
                _ => {}
            }
        }
    });
}
