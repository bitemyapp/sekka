//! Display power: every display off for idleness (`idle.rs`) or over IPC,
//! and a laptop's own panel off while its lid is closed. Keep Awake blocks
//! logind's lid handling, so without this a closed lid would leave the
//! panel lit behind it all night; opening the lid lights it again.
//! With another display in the layout, the closed panel leaves it, so
//! neither focus nor a Desk is stranded behind the lid. Its configuration
//! is restored when the lid opens or the last external display leaves.

use smithay::output::Output;
use tracing::{info, warn};

use crate::outputs::OutputChange;
use crate::state::{Aizome, State};

#[derive(Debug, Default)]
pub struct Power {
    /// Every display off: for idleness, or asked over IPC.
    pub monitors_off: bool,
    /// The laptop's lid is closed (libinput's lid switch).
    pub lid_closed: bool,
    /// Panels temporarily disabled by docking, with their live settings.
    pub lid_disabled: Vec<OutputChange>,
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
        self.apply_lid_layout();
        self.backend.apply_power(&mut self.az);
    }

    fn apply_lid_layout(&mut self) {
        let docked = self.az.outputs.iter().any(|o| !is_laptop_panel(&o.name()));
        if !self.az.power.lid_closed || !docked {
            for saved in std::mem::take(&mut self.az.power.lid_disabled) {
                // The connector may have gone away while the lid was closed.
                if self.az.known_output(&saved.name).is_some()
                    && let Err(e) = self.configure_output(&saved)
                {
                    warn!(output = saved.name, "restoring the laptop panel: {e}");
                }
            }
            return;
        }
        let panels: Vec<_> = self.az.outputs.iter().filter(|o| is_laptop_panel(&o.name())).cloned().collect();
        for panel in panels {
            // A mirror still needs its source's frames.
            if self.az.mirrors.iter().any(|(_, source)| source == &panel) {
                continue;
            }
            let saved = OutputChange {
                name: panel.name(),
                mode: panel.current_mode().map(|m| (m.size.w, m.size.h, Some(m.refresh))),
                scale: Some(panel.current_scale().fractional_scale()),
                position: Some(panel.current_location().into()),
                transform: Some(panel.current_transform()),
                enabled: Some(true),
            };
            let off = OutputChange { name: saved.name.clone(), enabled: Some(false), ..Default::default() };
            match self.configure_output(&off) {
                Ok(()) => self.az.power.lid_disabled.push(saved),
                Err(e) => warn!(output = off.name, "disabling the closed laptop panel: {e}"),
            }
        }
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
