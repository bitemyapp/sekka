//! Night Light: warmer colours at night, through the displays' gamma
//! ramps (the DRM CRTC's GAMMA_LUT, under GLES and pixman alike), on the
//! settings' schedule, with this session's changes over IPC on top.
//! Outputs without gamma (headless, winit, some virtual GPUs) can't show
//! it, and the IPC says which can.

use std::time::Duration;

use aizome_config::NightLight;
use aizome_ipc::{Event, NightLightState};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::state::State;

/// This session's Night Light: changes made over IPC, and what the
/// displays show.
#[derive(Default)]
pub struct Night {
    pub enabled: Option<bool>,
    pub temperature: Option<u32>,
    /// Kelvin; 0 before the first update.
    pub current: u32,
    started: bool,
}

/// Gamma ramps for a temperature: `size` entries each of red, green, blue.
pub fn ramps(kelvin: u32, size: usize) -> (Vec<u16>, Vec<u16>, Vec<u16>) {
    let k = aizome_config::temperature_rgb(kelvin);
    let ramp = |m: f64| -> Vec<u16> {
        (0..size)
            .map(|i| {
                let v = if size > 1 { i as f64 / (size - 1) as f64 } else { 1.0 };
                (v * m * 65535.0).round().clamp(0.0, 65535.0) as u16
            })
            .collect()
    };
    (ramp(k[0]), ramp(k[1]), ramp(k[2]))
}

impl State {
    /// The settings' Night Light with this session's changes.
    pub fn night_settings(&self) -> NightLight {
        let mut n = self.az.settings.night_light.clone();
        if let Some(e) = self.az.night.enabled {
            n.enabled = e;
        }
        if let Some(t) = self.az.night.temperature {
            n.temperature = t;
        }
        n
    }

    pub fn night_state(&self) -> NightLightState {
        let n = self.night_settings();
        let now = aizome_config::LocalNow::now();
        // Sunset and sunrise where the computer is (or its time zone's
        // city), looked up again each minute: the time zone may change.
        let place = self.az.settings.place();
        let current = n.temperature_at(place.as_ref(), now);
        let schedule = match n.schedule {
            aizome_config::NightSchedule::Sunset => "sunset",
            aizome_config::NightSchedule::Manual => "manual",
            aizome_config::NightSchedule::Always => "always",
        };
        let (from, to) = match n.span(place.as_ref(), now.day_of_year, now.utc_offset) {
            aizome_config::Span::Between { start, end } => {
                (Some(aizome_config::hm(start)), Some(aizome_config::hm(end)))
            }
            _ => (None, None),
        };
        let place = place.filter(|_| n.schedule == aizome_config::NightSchedule::Sunset).map(|p| match p.source {
            aizome_config::PlaceSource::Settings => "settings".to_owned(),
            aizome_config::PlaceSource::Located => "located".to_owned(),
            aizome_config::PlaceSource::TimeZone(zone) => zone,
        });
        NightLightState {
            from,
            to,
            place,
            enabled: n.enabled,
            temperature: n.temperature.clamp(1900, aizome_config::compositor::NEUTRAL_K),
            schedule: schedule.into(),
            active: current != aizome_config::compositor::NEUTRAL_K,
            current,
            outputs: self
                .az
                .outputs
                .iter()
                .filter(|o| self.az.out.get(*o).is_some_and(|s| s.gamma))
                .map(|o| o.name())
                .collect(),
        }
    }

    /// Shows the temperature the schedule says now on every display that
    /// can; `force` re-applies it (outputs added, the session back).
    pub fn update_night_light(&mut self, force: bool) {
        if !self.az.night.started {
            self.az.night.started = true;
            // Every minute, for the schedule, and for Auto's theme on the
            // tabs and frames (which turns at sunrise and sunset, or 07:00
            // and 19:00).
            let _ =
                self.az.lh.insert_source(Timer::from_duration(Duration::from_secs(60)), |_, _, state: &mut State| {
                    state.update_night_light(false);
                    if state.az.settings.appearance.theme == aizome_config::ThemeChoice::Auto {
                        let palette = crate::config::palette_for(&state.az.settings);
                        state.az.set_palette(palette);
                    }
                    TimeoutAction::ToDuration(Duration::from_secs(60))
                });
        }
        let state = self.night_state();
        if state.current == self.az.night.current && !force {
            return;
        }
        let first = self.az.night.current == 0;
        self.az.night.current = state.current;
        // Nothing to undo on displays that were never warmed.
        if first && state.current == aizome_config::compositor::NEUTRAL_K && !force {
            return;
        }
        self.backend.set_temperature(&mut self.az, state.current);
        let state = self.night_state();
        self.az.ipc.emit(&Event::NightLightChanged { state });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_ramps_are_identity_and_warm_ones_dim_blue() {
        let (r, g, b) = ramps(6500, 256);
        assert_eq!(r[255], 65535);
        assert_eq!(g[128], b[128]);
        assert_eq!(r[0], 0);
        let (r, g, b) = ramps(3000, 1024);
        assert_eq!(r[1023], 65535);
        assert!(g[1023] < 60000 && b[1023] < g[1023], "{} {}", g[1023], b[1023]);
        assert!(b.windows(2).all(|w| w[0] <= w[1]));
    }
}
