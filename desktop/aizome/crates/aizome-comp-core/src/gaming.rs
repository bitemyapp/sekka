//! Conservative fullscreen presentation policy, shared by DRM outputs.

use aizome_config::VrrMode;
use smithay::output::Output;

use crate::state::{Aizome, Mapped};

/// The top visible window on this output must cover it in fullscreen.
/// A fullscreen game on another display must not change this one's timing.
pub fn fullscreen_on<'a>(az: &'a Aizome, output: &Output) -> Option<&'a Mapped> {
    let rect = az.output_rect(output);
    let window = az
        .view
        .stacking
        .iter()
        .rev()
        .filter_map(|id| az.windows.get(id))
        .find(|w| w.view.shown && w.view.rect.intersects(rect))?;
    (window.view.fullscreen && window.view.rect.intersection(rect) == Some(rect)).then_some(window)
}

pub fn vrr_wanted(mode: VrrMode, fullscreen: bool, blocked: bool, mirrored: bool) -> bool {
    !blocked
        && !mirrored
        && match mode {
            VrrMode::Off => false,
            VrrMode::Fullscreen => fullscreen,
            VrrMode::Always => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_sync_follows_policy_and_retires_for_overlays_and_mirrors() {
        assert!(!vrr_wanted(VrrMode::Off, true, false, false));
        assert!(!vrr_wanted(VrrMode::Fullscreen, false, false, false));
        assert!(vrr_wanted(VrrMode::Fullscreen, true, false, false));
        assert!(vrr_wanted(VrrMode::Always, false, false, false));
        for mode in [VrrMode::Off, VrrMode::Fullscreen, VrrMode::Always] {
            assert!(!vrr_wanted(mode, true, true, false));
            assert!(!vrr_wanted(mode, true, false, true));
        }
    }
}
