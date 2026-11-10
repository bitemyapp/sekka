//! Apps that ask the session bus to keep the screen awake (VLC, Firefox,
//! the desktop portal's Inhibit: `aizome_sys::screensaver`). The
//! compositor decides idleness and hears only Wayland's idle inhibitors,
//! so while any such app holds, the shell holds one for them: the screen
//! neither locks nor turns off, as with a Wayland video player or Keep
//! Awake.
//!
//! The inhibitor is held from a thread of its own, on its own connection
//! to the compositor ([`aizome_awake::IdleInhibitor`]). It goes when the
//! last app lets go or leaves the bus, and with the shell if the shell
//! goes: a restarted shell starts with no holds, and a player takes its
//! own again the next time it starts playing.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use aizome_awake::IdleInhibitor;
use aizome_sys::screensaver::ScreenSaverState;

/// What the apps on the session bus want of the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    /// Nobody holds: it locks and turns off as the settings say.
    #[default]
    MayIdle,
    /// At least one app holds it awake.
    Awake,
}

impl From<&ScreenSaverState> for Screen {
    fn from(state: &ScreenSaverState) -> Screen {
        if state.held() { Screen::Awake } else { Screen::MayIdle }
    }
}

/// While held, how often the thread reads what the compositor sent (and
/// tries again for an inhibitor it couldn't take).
const LOOK_IN: Duration = Duration::from_secs(1);

/// The compositor's idle inhibitor, held while apps want the screen awake.
#[derive(Default)]
pub struct IdleHold {
    wanted: Screen,
    /// The holding thread, started the first time an app holds.
    thread: Option<mpsc::Sender<Screen>>,
}

impl IdleHold {
    /// What the apps want now; the thread hears only changes.
    pub fn want(&mut self, wanted: Screen) {
        if wanted == self.wanted {
            return;
        }
        self.wanted = wanted;
        let thread = self.thread.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            let spawned = std::thread::Builder::new().name("idle-hold".into()).spawn(move || hold(&rx));
            if let Err(e) = spawned {
                log::warn!("keeping the screen awake for apps: {e}");
            }
            tx
        });
        let _ = thread.send(wanted);
    }
}

/// Holds the inhibitor while the last word was [`Screen::Awake`].
fn hold(rx: &mpsc::Receiver<Screen>) {
    let mut wanted = Screen::MayIdle;
    let mut held: Option<IdleInhibitor> = None;
    let mut warned = false;
    loop {
        // With nothing held there's nothing to do until an app asks.
        let next = match wanted {
            Screen::MayIdle => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Screen::Awake => rx.recv_timeout(LOOK_IN),
        };
        match next {
            Ok(w) => wanted = w,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        held = match (held, wanted) {
            // Dropped: let go.
            (_, Screen::MayIdle) => None,
            (Some(mut inhibitor), Screen::Awake) => match inhibitor.wait(Duration::ZERO) {
                Ok(()) => Some(inhibitor),
                Err(e) => {
                    log::warn!("keeping the screen awake for apps: {e}");
                    None
                }
            },
            (None, Screen::Awake) => match IdleInhibitor::new() {
                Ok(inhibitor) => {
                    warned = false;
                    Some(inhibitor)
                }
                Err(e) => {
                    if !warned {
                        log::warn!("keeping the screen awake for apps: {e}");
                        warned = true;
                    }
                    None
                }
            },
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_sys::screensaver::Holds;

    #[test]
    fn the_screen_stays_awake_while_any_app_holds() {
        let mut holds = Holds::default();
        assert_eq!(Screen::from(&holds.state()), Screen::MayIdle);
        let vlc = aizome_sys::screensaver::Peer::new(":1.7").unwrap();
        let firefox = aizome_sys::screensaver::Peer::new(":1.9").unwrap();
        let first = holds.take(vlc.clone(), "vlc", "Playing some media.").unwrap();
        holds.take(firefox.clone(), "firefox", "Playing video").unwrap();
        assert_eq!(Screen::from(&holds.state()), Screen::Awake);
        holds.release(&vlc, first.cookie);
        assert_eq!(Screen::from(&holds.state()), Screen::Awake, "another app still holds");
        holds.peer_left(firefox);
        assert_eq!(Screen::from(&holds.state()), Screen::MayIdle);
    }

    #[test]
    fn nothing_starts_until_an_app_holds() {
        let mut hold = IdleHold::default();
        hold.want(Screen::MayIdle);
        assert!(hold.thread.is_none());
    }
}
