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

/// Failed frames with adaptive sync on, for one output. A driver may pass
/// the test commit and reject the real one, but a frame can also fail for
/// other reasons (a busy page flip, a session switch). So a failed VRR frame
/// is retried at fixed refresh, and VRR is to blame only when that retry
/// succeeds. [`VrrFailures::LIMIT`] such retries in a row, with no good VRR
/// frame between them, mark it unsupported until the output's mode changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VrrFailures {
    /// VRR is held off for a fixed-refresh retry.
    retrying: bool,
    blamed: u8,
}

impl VrrFailures {
    pub const LIMIT: u8 = 3;

    /// Whether VRR must stay off for now.
    pub fn holding_off(&self) -> bool {
        self.retrying
    }

    /// A frame failed. True when it had VRR on: hold it off and retry.
    pub fn failed(&mut self, vrr: bool) -> bool {
        // When the fixed-refresh retry fails too, VRR wasn't the cause.
        self.retrying = vrr;
        vrr
    }

    /// A frame reached the display. True, once, when VRR should be given up.
    pub fn presented(&mut self, vrr: bool) -> bool {
        // Any frame that reaches the display ends a hold-off. One with VRR
        // still on (the driver refused to switch it off for the retry)
        // shows VRR wasn't the cause.
        let retried = std::mem::take(&mut self.retrying);
        if vrr {
            self.blamed = 0;
        } else if retried {
            self.blamed = self.blamed.saturating_add(1);
            return self.blamed == Self::LIMIT;
        }
        false
    }

    /// The driver refused to switch VRR. True when it should be given up.
    pub fn refused(&mut self) -> bool {
        self.blamed = self.blamed.saturating_add(1);
        self.blamed >= Self::LIMIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_sync_is_given_up_only_when_it_alone_fails_repeatedly() {
        // A transient failure that a later VRR frame survives.
        let mut f = VrrFailures::default();
        assert!(f.failed(true) && f.holding_off());
        assert!(!f.presented(false) && !f.holding_off());
        assert!(!f.presented(true));
        for _ in 0..VrrFailures::LIMIT {
            assert!(f.failed(true));
            assert!(!f.presented(false));
            assert!(!f.presented(true));
        }
        // Failing at fixed refresh too isn't VRR's fault.
        for _ in 0..VrrFailures::LIMIT + 1 {
            assert!(f.failed(true));
            assert!(!f.failed(false) && !f.holding_off());
        }
        assert!(!f.presented(false));
        // Only VRR frames fail, every time.
        for n in 1..=VrrFailures::LIMIT {
            assert!(f.failed(true));
            assert_eq!(f.presented(false), n == VrrFailures::LIMIT);
        }
        assert!(!f.presented(false));
        let mut f = VrrFailures::default();
        assert!(!f.refused() && !f.refused() && f.refused());
    }

    #[test]
    fn a_frame_shown_with_adaptive_sync_on_ends_the_hold_off() {
        // A VRR frame failed, the driver refused to switch VRR off for the
        // retry, and the next frame, still with VRR, reached the display.
        let mut f = VrrFailures::default();
        assert!(f.failed(true) && f.holding_off());
        assert!(!f.refused());
        assert!(!f.presented(true));
        assert!(!f.holding_off());
        assert_eq!(f, VrrFailures::default());
    }

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
