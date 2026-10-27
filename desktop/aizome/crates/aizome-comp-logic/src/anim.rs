//! Motion: Desks sliding and tabs sliding into a group. Motion moves
//! layers (render elements) and never asks clients to redraw; the effects
//! tier sets its length (80ms in Lean, none in Austere or with reduced
//! motion).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::wm::{DeskId, WinId};

#[derive(Clone, Debug)]
pub struct DeskSlide {
    /// The display it slides on.
    pub output: String,
    pub from: DeskId,
    pub to: DeskId,
    /// +1 when the new Desk comes in from the right.
    pub dir: f64,
    pub start: Instant,
    pub dur: Duration,
}

impl DeskSlide {
    /// 0.0 at the start, 1.0 when done, eased out.
    pub fn progress(&self) -> f64 {
        ease_out(self.start.elapsed().as_secs_f64() / self.dur.as_secs_f64().max(1e-6))
    }
    pub fn done(&self) -> bool {
        self.start.elapsed() >= self.dur
    }
}

/// A tab moving from where it was to where it now belongs.
#[derive(Clone, Copy, Debug)]
pub struct Slide {
    pub dx: f64,
    pub dy: f64,
    pub start: Instant,
    pub dur: Duration,
}

impl Slide {
    /// The offset still to cover.
    pub fn offset(&self) -> (f64, f64) {
        let t = ease_out(self.start.elapsed().as_secs_f64() / self.dur.as_secs_f64().max(1e-6));
        (self.dx * (1.0 - t), self.dy * (1.0 - t))
    }
    pub fn done(&self) -> bool {
        self.start.elapsed() >= self.dur
    }
}

#[derive(Debug, Default)]
pub struct Animations {
    /// Desks sliding, a display each.
    pub desks: Vec<DeskSlide>,
    /// Tabs sliding into place, by window.
    pub tabs: HashMap<WinId, Slide>,
    /// Overlays fading or growing in (switcher, overview), by name.
    pub overlays: HashMap<&'static str, Slide>,
}

impl Animations {
    pub fn active(&self) -> bool {
        !self.desks.is_empty() || !self.tabs.is_empty() || !self.overlays.is_empty()
    }

    /// Drops finished animations; returns whether any ran.
    pub fn tick(&mut self) -> bool {
        let was = self.active();
        self.desks.retain(|d| !d.done());
        self.tabs.retain(|_, s| !s.done());
        self.overlays.retain(|_, s| !s.done());
        was
    }

    pub fn tab_offset(&self, win: WinId) -> (f64, f64) {
        self.tabs.get(&win).map(|s| s.offset()).unwrap_or((0.0, 0.0))
    }

    /// Whether a window is drawn only because a Desk is sliding away.
    pub fn is_hidden_by_animation(&self, _win: WinId) -> bool {
        false
    }

    /// A display's Desk slides from one to another (one sliding there
    /// already gives way).
    pub fn start_desk(&mut self, output: &str, from: DeskId, to: DeskId, dir: f64, ms: u32) {
        self.desks.retain(|d| d.output != output);
        if ms == 0 {
            return;
        }
        let dur = Duration::from_millis(ms as u64);
        self.desks.push(DeskSlide { output: output.into(), from, to, dir, start: Instant::now(), dur });
    }

    /// The slide on a display, if one runs.
    pub fn desk_on(&self, output: &str) -> Option<&DeskSlide> {
        self.desks.iter().find(|d| d.output == output)
    }

    pub fn start_tab(&mut self, win: WinId, dx: f64, dy: f64, ms: u32) {
        if ms == 0 || (dx.abs() < 0.5 && dy.abs() < 0.5) {
            self.tabs.remove(&win);
            return;
        }
        self.tabs.insert(win, Slide { dx, dy, start: Instant::now(), dur: Duration::from_millis(ms as u64) });
    }

    pub fn start_overlay(&mut self, name: &'static str, ms: u32) {
        if ms == 0 {
            self.overlays.remove(name);
            return;
        }
        self.overlays
            .insert(name, Slide { dx: 1.0, dy: 0.0, start: Instant::now(), dur: Duration::from_millis(ms as u64) });
    }

    /// An overlay's entrance, 0.0 → 1.0 (1.0 when not animating).
    pub fn overlay_progress(&self, name: &str) -> f64 {
        self.overlays.get(name).map(|s| 1.0 - s.offset().0).unwrap_or(1.0)
    }
}

/// Cubic ease-out, clamped.
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn austere_means_no_motion() {
        let mut a = Animations::default();
        a.start_desk("HEADLESS-1", 1, 2, 1.0, 0);
        a.start_tab(1, 40.0, 0.0, 0);
        assert!(!a.active());
    }

    #[test]
    fn slides_finish() {
        let mut a = Animations::default();
        a.start_tab(1, 40.0, 0.0, 1);
        std::thread::sleep(Duration::from_millis(3));
        assert_eq!(a.tab_offset(1), (0.0, 0.0));
        a.tick();
        assert!(!a.active());
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
    }
}
