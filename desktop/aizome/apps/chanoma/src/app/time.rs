//! Keeping time: once a second, today's minutes count up while something
//! plays for kids, warnings come at five minutes and at one, and when the
//! time's up (or it's bedtime) what plays is put away, kept where it was
//! left, and the closed screen says so.

use std::time::{Duration, Instant};

use aizome_kit::Cx;

use super::{Chanoma, Msg, Screen};
use crate::rules::{Gate, gate};
use crate::ui::screens::{Art, Message};

/// How long a toast shows.
const TOAST: Duration = Duration::from_secs(5);

impl Chanoma {
    /// Something plays (what counts toward today's time).
    pub fn playing(&self) -> bool {
        matches!(self.screen, Screen::Playing | Screen::Watching | Screen::Film | Screen::Starting(_))
    }

    pub fn tick(&mut self, cx: &mut Cx<Msg>) {
        self.picker_tick(cx);
        let now = Instant::now();
        let elapsed = self.last_tick.map_or(0.0, |t| now.duration_since(t).as_secs_f64().min(5.0));
        self.last_tick = Some(now);
        self.local = self.config.clock.local();
        if self.usage.roll(&self.local) {
            self.warned = 0;
            self.dirty_usage = true;
        }
        if self.playing() && !self.is_grownup() {
            self.counted += elapsed;
            let whole = self.counted.floor();
            if whole >= 1.0 {
                self.usage.used += whole as u64;
                self.counted -= whole;
                self.dirty_usage = true;
            }
        }
        // Kept every half minute while it counts, so a power cut loses little.
        if self.dirty_usage && self.usage_saved.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(30)) {
            self.save_usage();
        }
        // Grown-up mode locks itself, untouched, but not under a film.
        if let Some(t) = self.grownup
            && !self.playing()
            && now.duration_since(t) >= Duration::from_secs(u64::from(self.settings.relock_minutes) * 60)
            && self.settings.setup_done
        {
            self.lock(cx);
        }
        if self.toast.as_ref().is_some_and(|(_, _, at)| now.duration_since(*at) >= TOAST) {
            self.toast = None;
            self.sync_hud(cx);
        }
        self.bar_tick(cx);
        self.film_tick();
        self.setup_tick(cx);
        self.check_gate(cx);
    }

    /// Work out the gate, warn, and close up when it shuts.
    pub fn check_gate(&mut self, cx: &mut Cx<Msg>) {
        let now = self.now_secs();
        let g = gate(&self.settings.limits, &self.usage, &self.local, now);
        self.gate = g;
        if self.is_grownup() || !self.settings.setup_done {
            return;
        }
        match g {
            Gate::Open { left: Some(left) } if self.playing() => {
                if left <= 60 && self.warned < 2 {
                    self.warned = 2;
                    self.toast("One minute left", cx);
                } else if left <= 300 && self.warned < 1 {
                    self.warned = 1;
                    self.toast("Five minutes left", cx);
                }
            }
            Gate::Open { .. } => {}
            Gate::TimeUp | Gate::Bedtime { .. } => {
                // A warning has said its piece.
                if self.toast.as_ref().is_some_and(|t| t.1 == crate::ui::Note::Time) {
                    self.toast = None;
                }
                if self.playing() {
                    self.end_activity(cx);
                }
                if !matches!(self.screen, Screen::Closed | Screen::Welcome | Screen::GrownUps) && self.modal.is_none() {
                    self.go(Screen::Closed, cx);
                }
            }
        }
        if self.screen == Screen::Closed && g.open() {
            self.go(Screen::Home, cx);
        }
    }

    pub fn closed_message(&self) -> Message {
        match self.gate {
            Gate::Bedtime { until } => Message::new(
                Art::Moon,
                "It's bedtime",
                format!(
                    "Sleep well. Everything will be here in the morning, from {}.",
                    crate::clock::minute_label(until, self.config.h24)
                ),
            ),
            _ => Message::new(
                Art::Hourglass,
                "That's all for today",
                "Your games are kept just where you left them, for tomorrow.",
            ),
        }
        .actions(&["Ask a grown-up"])
    }
}
