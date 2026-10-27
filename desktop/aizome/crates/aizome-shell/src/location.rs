//! Where the computer is, for sunrise and sunset: while something follows
//! the sun (Night Light's sunset schedule, Auto following sunset and
//! sunrise), the shell asks GeoClue, as `org.aizome.Shell`, at a city's
//! accuracy, and keeps what it says in `settings.toml`'s `[location]`
//! (to 0.01°, with the time zone it was found in), then asks the
//! compositor to apply it. Every part reckons sunrise and sunset from
//! there (`aizome_config::location`); without an answer, from the time
//! zone's city, as it did before asking.
//!
//! It asks once a session, and again after six hours (a laptop moves).

use std::time::{Duration, Instant};

use aizome_config::{Location, Settings};
use aizome_kit::prelude::*;

use crate::shell::{Msg, Shell};

/// The desktop id GeoClue knows the shell by (allowed in Aizome's NixOS
/// module).
pub const DESKTOP_ID: &str = "org.aizome.Shell";
/// How long GeoClue may take (it may ask the network).
pub const WAIT: Duration = Duration::from_secs(30);
/// How long a position holds before it's asked for again.
pub const AGAIN: Duration = Duration::from_secs(6 * 3600);

/// Whether a position found should be written over the one kept: it moved
/// by more than the rounding, or was found in another time zone.
pub fn changed(kept: &Location, found: &Location) -> bool {
    let near = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < 0.005,
        (None, None) => true,
        _ => false,
    };
    !near(kept.latitude, found.latitude) || !near(kept.longitude, found.longitude) || kept.zone != found.zone
}

/// `settings` with the position found kept (what else is in `[location]`
/// stays).
pub fn keep(settings: &mut Settings, found: &Location) {
    let extra = std::mem::take(&mut settings.location.extra);
    settings.location = Location { extra, ..found.clone() };
}

impl Shell {
    /// Ask GeoClue where the computer is, on a thread, if something
    /// follows the sun and it hasn't been asked lately.
    pub fn locate_if_wanted(&mut self, _cx: &mut Cx<Msg>) {
        if !self.opts.live || self.locating || !self.settings.wants_location() {
            return;
        }
        if self.located_at.is_some_and(|at| at.elapsed() < AGAIN) {
            return;
        }
        let Some(tx) = self.sender() else { return };
        self.locating = true;
        let spawned = std::thread::Builder::new().name("aizome-locate".into()).spawn(move || {
            let found = aizome_sys::location::locate(DESKTOP_ID, WAIT)
                .map(|l| (l.latitude, l.longitude))
                .map_err(|e| e.to_string());
            tx.send(Msg::Located(found));
        });
        if spawned.is_err() {
            self.locating = false;
        }
    }

    /// GeoClue answered: keep a new position, and have the compositor
    /// apply it.
    pub fn located(&mut self, found: Result<(f64, f64), String>) {
        self.locating = false;
        self.located_at = Some(Instant::now());
        let (latitude, longitude) = match found {
            Ok(p) => p,
            Err(e) => {
                log::info!("where the computer is: {e}; sunrise and sunset follow the time zone's city");
                return;
            }
        };
        let found = Location::found(latitude, longitude, aizome_config::system_timezone());
        if !changed(&self.settings.location, &found) {
            return;
        }
        // Read again, so nobody else's edit is lost.
        let mut fresh = Settings::load_or_default();
        keep(&mut fresh, &found);
        if let Err(e) = fresh.save() {
            log::warn!("keeping where the computer is: {e}");
            return;
        }
        self.settings = fresh;
        self.ipc.send(aizome_ipc::Request::ReloadConfig);
        log::info!("the computer is near {:.2}, {:.2}: sunrise and sunset are reckoned there", latitude, longitude);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_is_kept_when_it_moved_or_the_zone_changed() {
        let paris = Location::found(48.8534, 2.3488, Some("Europe/Paris".into()));
        assert!(changed(&Location::default(), &paris));
        assert!(!changed(&paris, &Location::found(48.851, 2.352, Some("Europe/Paris".into()))));
        assert!(changed(&paris, &Location::found(45.76, 4.84, Some("Europe/Paris".into()))), "Lyon");
        assert!(changed(&paris, &Location::found(48.85, 2.35, Some("Europe/Brussels".into()))));
        let mut s = Settings::from_toml("[location]\nnote = \"kept\"\nlatitude = 1.0\n").unwrap();
        keep(&mut s, &paris);
        let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
        assert_eq!(back.location.position_in(Some("Europe/Paris")), Some((48.85, 2.35)));
        assert_eq!(back.location.extra.get("note").and_then(|v| v.as_str()), Some("kept"));
    }
}
