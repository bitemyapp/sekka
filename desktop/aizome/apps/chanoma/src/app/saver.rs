//! The screensaver: left untouched for [`AFTER`] off a game or a film,
//! Chanoma goes dark but for the time and its mark, small, moved to
//! another place every minute so nothing stays put on a television. Any
//! button, key, click or move brings it back as it was, and does nothing
//! else; a controller switched on brings it back and is counted as ever.
//!
//! The compositor can't see a controller: Chanoma reads it itself
//! (`crate::pad`), so its presses are told to the compositor as input
//! (aizome-ipc's `activity`, at most every [`TELL_EVERY`]). On the desktop
//! the session's own idleness (locking, the displays going off) then
//! counts them as it counts keys. On the TV account, whose session is
//! Chanoma alone, Chanoma holds the display awake for as long as it runs
//! (aizome-ipc's `keep-awake`): a television that went dark would look
//! broken, and only a keyboard or a mouse could wake it.

use std::time::{Duration, Instant};

use aizome_kit::prelude::*;

use super::{Chanoma, Msg};
use crate::pad::PadEvent;

/// Untouched this long, the screensaver comes.
pub const AFTER: Duration = Duration::from_secs(5 * 60);
/// The compositor is told of a controller's presses this often at most.
const TELL_EVERY: Duration = Duration::from_secs(10);

impl Chanoma {
    /// Someone pressed, clicked or moved something (`msg`): the
    /// screensaver goes, and if it showed, the press does nothing else
    /// (the answer: whether `msg` is done with).
    pub(super) fn input_seen(&mut self, msg: &Msg, _cx: &mut Cx<Msg>) -> bool {
        let pad = match msg {
            Msg::Pad(PadEvent::Connected(_) | PadEvent::Nav { .. } | PadEvent::Button { pressed: true, .. }) => true,
            Msg::Input(_) | Msg::Click(_) | Msg::Moved { .. } => false,
            _ => return false,
        };
        self.last_input = Instant::now();
        if pad {
            self.tell_compositor();
        }
        if self.saver.take().is_some() {
            log::info!("chanoma: screensaver off");
            // A controller switched on is still one to count.
            return !matches!(msg, Msg::Pad(PadEvent::Connected(_)));
        }
        false
    }

    /// Each second: the screensaver comes once Chanoma has been left
    /// alone long enough, but never over a game, a film or a page.
    pub fn saver_tick(&mut self) {
        if self.saver.is_some() && !self.saver_allowed() {
            self.saver = None;
        }
        if self.saver.is_none() && self.saver_allowed() && self.last_input.elapsed() >= self.config.saver_after {
            log::info!("chanoma: screensaver on");
            self.saver = Some(Instant::now());
        }
    }

    fn saver_allowed(&self) -> bool {
        !self.playing() && self.splash.is_none() && self.setup.is_none()
    }

    /// What the screensaver shows, while it shows.
    pub fn saver_view(&self) -> Option<crate::ui::Saver> {
        let since = self.saver?;
        Some(crate::ui::Saver {
            clock: self.local.time_label(self.config.h24),
            place: (since.elapsed().as_secs() / 60) as u32,
        })
    }

    /// A controller's presses, for the compositor's idleness.
    fn tell_compositor(&mut self) {
        if !self.config.compositor {
            return;
        }
        let now = Instant::now();
        if self.activity_told.is_some_and(|t| now.duration_since(t) < TELL_EVERY) {
            return;
        }
        self.activity_told = Some(now);
        let spawned = std::thread::Builder::new().name("chanoma activity".into()).spawn(|| {
            if let Ok(mut c) = aizome_ipc::Client::connect() {
                let _ = c.request(&aizome_ipc::Request::Activity);
            }
        });
        if let Err(e) = spawned {
            log::warn!("chanoma: telling the compositor of a controller: {e}");
        }
    }

    /// On the TV account: the display held awake for as long as Chanoma
    /// runs (the hold lasts as long as its connection).
    pub(super) fn keep_awake(&self) {
        if !(self.config.tv && self.config.compositor) {
            return;
        }
        let spawned = std::thread::Builder::new().name("chanoma keep-awake".into()).spawn(|| {
            let mut c = match aizome_ipc::Client::connect() {
                Ok(c) => c,
                Err(e) => return log::warn!("chanoma: no compositor to keep the display awake: {e}"),
            };
            match c.request(&aizome_ipc::Request::KeepAwake) {
                Ok(aizome_ipc::Response::Ok) => {
                    log::info!("chanoma: the display kept awake, for the screensaver");
                    loop {
                        std::thread::park();
                    }
                }
                other => log::warn!("chanoma: the compositor wouldn't keep the display awake: {other:?}"),
            }
        });
        if let Err(e) = spawned {
            log::warn!("chanoma: keeping the display awake: {e}");
        }
    }
}
