//! Power: batteries, power profiles, the session and the machine's power
//! state, and brightness. Four services, each optional:
//!
//! - [`Upower`] (UPower, `services.upower.enable`): batteries with percent,
//!   state, time to empty or full, energy rate and health; line power;
//!   peripherals' batteries; the lid; keyboard backlights.
//! - [`PowerProfiles`] (power-profiles-daemon,
//!   `services.power-profiles-daemon.enable`, or tuned's PPD API): the
//!   profiles, the active one, holds.
//! - [`Logind`] (systemd-logind, always there on NixOS): suspend,
//!   hibernate, reboot and power off with their `Can…` answers, lock and
//!   unlock requests for this session, `PrepareForSleep` with a delay
//!   inhibitor so the shell locks before sleep, the lid, the idle hint,
//!   inhibitors.
//! - [`Backlight`] (sysfs and logind): screen brightness, set through
//!   logind's `Session.SetBrightness` without root.

mod backlight;
mod logind;
mod profiles;
mod upower;

pub use backlight::{Backlight, BacklightDevice, BacklightState, MIN_STEP_FRACTION, stepped as stepped_brightness};
pub(crate) use logind::session_id;
pub use logind::{Can, Inhibition, Inhibitor, Logind, LogindState, SessionEvent};
pub use profiles::{PowerProfiles, Profile, ProfileHold, ProfilesState};
pub use upower::{
    BatteryState, HistoryPoint, KeyboardBacklight, PowerDevice, PowerKind, Upower, UpowerState, WarningLevel,
};
