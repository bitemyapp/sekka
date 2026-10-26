//! Keep Awake: while it's on, the computer doesn't go to sleep.
//!
//! A user service (`aizome-keep-awake.service`, running `aizome-awake
//! hold`) holds two things for as long as it runs:
//!
//! - logind's **block** inhibitors for `sleep`, `idle` and
//!   `handle-lid-switch`, so neither suspending, hibernating, logind's own
//!   idle action nor closing the lid puts the computer to sleep;
//! - the compositor's **idle inhibitor** (`zwp_idle_inhibit_manager_v1`),
//!   so Aizome doesn't lock the screen or turn the displays off when the
//!   computer is left alone.
//!
//! Whether it's on is a file, `$XDG_STATE_HOME/aizome/keep-awake`: it
//! survives logging out, and the service starts at the next login when
//! it's there. "On" and "protected" are kept apart everywhere: the menu
//! bar shows Keep Awake as working only when logind lists the service's
//! inhibitor, never because the setting says so.
//!
//! An administrator can still suspend past an inhibitor on purpose
//! (`systemctl suspend -i`), and nothing here stops a flat battery,
//! firmware or thermal shutdown.

use std::path::PathBuf;

/// The service that holds the locks.
pub const UNIT: &str = "aizome-keep-awake.service";
/// Who logind says holds them.
pub const WHO: &str = "Aizome Keep Awake";
/// What they block.
pub const WHAT: &str = "sleep:idle:handle-lid-switch";
/// Why, as `systemd-inhibit --list` and other desktops' dialogs show it.
pub const WHY: &str = "Keep Awake is on; turn it off from Aizome's menu bar to let the computer sleep";
const REQUIRED: [&str; 3] = ["sleep", "idle", "handle-lid-switch"];

/// One of logind's inhibitors (`ListInhibitors`' `a(ssssuu)`).
pub type Inhibitor = (String, String, String, String, u32, u32);

/// Where the setting is kept.
pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into())).join(".local/state"));
    base.join("aizome")
}

/// The file whose presence means Keep Awake is on.
pub fn setting() -> PathBuf {
    state_dir().join("keep-awake")
}

/// Keep Awake is on (asked for), whether or not it's working.
pub fn requested() -> bool {
    setting().is_file()
}

/// Whether logind lists Keep Awake's block inhibitor, for all of sleep,
/// idle and the lid switch, held by `uid` (and by `pid`, when given).
pub fn protected_by(inhibitors: &[Inhibitor], uid: u32, pid: Option<u32>) -> bool {
    inhibitors.iter().any(|(what, who, _why, mode, u, p)| {
        who == WHO
            && mode == "block"
            && *u == uid
            && pid.is_none_or(|pid| *p == pid)
            && REQUIRED.iter().all(|r| what.split(':').any(|w| w == *r))
    })
}

/// What the menu bar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Off: the computer sleeps as usual.
    #[default]
    Off,
    /// On, and logind lists the locks.
    On,
    /// On, but the locks aren't there (the service failed, or logind
    /// restarted and it's taking them again): sleep may happen.
    Unprotected,
}

impl Status {
    pub fn of(requested: bool, protected: bool) -> Status {
        match (requested, protected) {
            (_, true) => Status::On,
            (true, false) => Status::Unprotected,
            (false, false) => Status::Off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(what: &str, who: &str, mode: &str, uid: u32, pid: u32) -> Inhibitor {
        (what.into(), who.into(), "why".into(), mode.into(), uid, pid)
    }

    #[test]
    fn protection_needs_all_three_block_locks_held_by_the_owner() {
        let ok = [row("idle:handle-lid-switch:sleep", WHO, "block", 1000, 42)];
        assert!(protected_by(&ok, 1000, Some(42)));
        assert!(protected_by(&ok, 1000, None));
        for bad in [
            row("sleep:idle", WHO, "block", 1000, 42),
            row("sleep:idle:handle-lid-switch", WHO, "delay", 1000, 42),
            row("sleep:idle:handle-lid-switch", WHO, "block", 1001, 42),
            row("sleep:idle:handle-lid-switch", WHO, "block", 1000, 43),
            row("sleep:idle:handle-lid-switch", "Tatami Keep Awake", "block", 1000, 42),
            row("sleep:idler:handle-lid-switch", WHO, "block", 1000, 42),
        ] {
            assert!(!protected_by(&[bad], 1000, Some(42)));
        }
        assert!(!protected_by(&[], 1000, None));
    }

    #[test]
    fn on_is_never_confused_with_protected() {
        assert_eq!(Status::of(false, false), Status::Off);
        assert_eq!(Status::of(true, true), Status::On);
        assert_eq!(Status::of(true, false), Status::Unprotected);
        // Held while the setting is off (turning off, or a second login's
        // service): the computer is kept awake, so say so.
        assert_eq!(Status::of(false, true), Status::On);
    }
}
