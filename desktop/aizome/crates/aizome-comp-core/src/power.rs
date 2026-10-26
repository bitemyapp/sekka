//! Display power: every display off for idleness (`idle.rs`) or over IPC,
//! and a laptop's own panel off while its lid is closed. Keep Awake blocks
//! logind's lid handling, so without this a closed lid would leave the
//! panel lit behind it all night; opening the lid lights it again. A
//! display that's off is dark (on udev its CRTC is disabled, DPMS off) but
//! keeps its place in the layout, and its next frame turns it back on.

use smithay::output::Output;
use tracing::info;

use crate::state::{Aizome, State};

#[derive(Debug, Default)]
pub struct Power {
    /// Every display off: for idleness, or asked over IPC.
    pub monitors_off: bool,
    /// The laptop's lid is closed (libinput's lid switch).
    pub lid_closed: bool,
}

/// A laptop's own panel, by its connector's name (`eDP-1`, `LVDS-1`,
/// `DSI-1`).
pub fn is_laptop_panel(name: &str) -> bool {
    ["eDP-", "LVDS-", "DSI-"].iter().any(|p| name.starts_with(p))
}

impl Aizome {
    /// Whether a display should be on. A panel another display mirrors
    /// stays on with the lid closed: its frames drive the mirror's.
    pub fn wants_power(&self, output: &Output) -> bool {
        let behind_lid = self.power.lid_closed
            && is_laptop_panel(&output.name())
            && !self.mirrors.iter().any(|(_, source)| source == output);
        !self.power.monitors_off && !behind_lid
    }
}

impl State {
    /// Turns every display off or on (idleness, IPC). A panel behind a
    /// closed lid stays off.
    pub fn set_monitors_power(&mut self, on: bool) {
        self.az.power.monitors_off = !on;
        self.apply_power();
    }

    /// The lid closed or opened. Opening it is coming back to the
    /// computer, as input is: displays off for idleness come on with it.
    pub fn lid_switched(&mut self, closed: bool) {
        if self.az.power.lid_closed == closed {
            return;
        }
        info!(closed, "lid");
        self.az.power.lid_closed = closed;
        if !closed {
            self.note_activity();
        }
        self.apply_power();
    }

    /// Turns each display off or on as it should be now: after a change
    /// to the power, and when outputs come, go or start mirroring.
    pub fn apply_power(&mut self) {
        self.backend.apply_power(&mut self.az);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laptop_panels_by_connector() {
        for name in ["eDP-1", "eDP-2", "LVDS-1", "DSI-1"] {
            assert!(is_laptop_panel(name), "{name}");
        }
        for name in ["HDMI-A-1", "DP-1", "DP-10", "Virtual-1", "HEADLESS-1", "DVI-D-1", "edp-1"] {
            assert!(!is_laptop_panel(name), "{name}");
        }
    }
}
