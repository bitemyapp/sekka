// SPDX-License-Identifier: GPL-3.0-or-later
//! What Finish Installing Sekka's window shows, apart from AppKit: its
//! screens, and when it may close.
use crate::finish::Phase;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Checking,
    Broken(String),
    WrongRecovery,
    NotOneTrue,
    Ready { error: Option<String> },
    Working(Phase),
    Done,
}

impl Screen {
    /// Whether `bputil`, `kmutil` or `bless` may be running. Each answers
    /// to the app's pseudo-terminal, and one stopped part way can leave the
    /// startup policy or the boot object half changed, so while a step runs
    /// the window stays open and the app doesn't quit.
    pub fn busy(&self) -> bool {
        matches!(self, Screen::Working(_))
    }
}

/// Said when someone tries to quit or close the window while a step runs.
pub const BUSY: &str = "Wait for this step to finish";
pub const BUSY_DETAIL: &str = "macOS is changing how your Mac starts up, and stopping it part way isn't safe. Finish Installing Sekka can quit when this step is done.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_running_step_keeps_the_app_open() {
        assert!(Screen::Working(Phase::Allowing).busy());
        assert!(Screen::Working(Phase::Selecting).busy());
        for screen in [
            Screen::Checking,
            Screen::Broken("x".into()),
            Screen::WrongRecovery,
            Screen::NotOneTrue,
            Screen::Ready { error: None },
            Screen::Done,
        ] {
            assert!(!screen.busy(), "{screen:?}");
        }
    }
}
