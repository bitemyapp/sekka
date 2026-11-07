//! Starting: Chanoma's name on the screen from its first frame until Home
//! has its games, so a television never shows nothing at all and then a
//! Home that fills in. It's up while the first look at the folders is
//! read (from the files index, a moment for thousands of games) and while
//! a drive that holds one of the folders is being opened ("Opening the
//! drive Extreme Pro"): their games come once it's open. Not longer than
//! [`SPLASH_MOST`], whatever keeps it; a drive UDisks hasn't said anything
//! of yet is waited for [`DRIVES_WAIT`] at most. Buttons do nothing on
//! it.

use std::time::{Duration, Instant};

use aizome_kit::prelude::*;

use super::drives::DriveIs;
use super::{Chanoma, Msg, Screen};
use crate::ui::screens::{Art, Message};

/// The longest the name shows, whatever it waits for.
pub const SPLASH_MOST: Duration = Duration::from_secs(10);
/// How long a folder that isn't there waits for UDisks to say whether its
/// drive is plugged in.
pub const DRIVES_WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct Splash {
    /// Chanoma's start, for the journal.
    pub since: Instant,
    /// A look at the folders asked for and not answered yet.
    pub scan_due: bool,
    /// UDisks has had [`DRIVES_WAIT`] to say.
    pub drives_waited: bool,
}

impl Chanoma {
    /// At the start, onto Home with folders to read: the name, and the
    /// timers that end it.
    pub fn start_splash(&mut self, cx: &mut Cx<Msg>) {
        if !self.config.scan || self.screen != Screen::Home || self.settings.folders.is_empty() {
            return;
        }
        self.splash = Some(Splash { since: Instant::now(), scan_due: true, drives_waited: false });
        cx.after(SPLASH_MOST, Msg::SplashOver);
        cx.after(DRIVES_WAIT, Msg::DrivesWaited);
    }

    /// Whether Home can show now: its games read, no folder's drive being
    /// opened.
    pub fn splash_check(&mut self) {
        let Some(s) = &self.splash else { return };
        if s.scan_due || self.drive_awaited(s.drives_waited).is_some() {
            return;
        }
        log::info!(target: "aizome_chanoma::start", "chanoma: Home, with {} games, {:.0?} after starting", self.library.games.len(), s.since.elapsed());
        self.splash = None;
    }

    pub fn splash_over(&mut self) {
        if let Some(s) = self.splash.take() {
            log::warn!(
                "chanoma: Home shown {:.0?} after starting, its games or a drive still awaited",
                s.since.elapsed()
            );
        }
    }

    /// A folder whose drive is being opened (its name), or (`""`) one
    /// that isn't there while UDisks hasn't said of the drives yet.
    fn drive_awaited(&self, waited: bool) -> Option<String> {
        let mounts = &self.config.mounts;
        // Not watching drives (`--no-drives`) is having looked.
        let looked = waited || self.drives.looked || self.drives.disks.is_none();
        self.settings.folders.iter().find_map(|f| match self.drives.of_folder(f, mounts) {
            Some(DriveIs::Opening(name)) => Some(name),
            _ if !looked && self.library.held.iter().any(|h| h.folder == *f && !h.here) => Some(String::new()),
            _ => None,
        })
    }

    /// The screen while starting.
    pub fn splash_message(&self) -> Message {
        let body = match self.splash.as_ref().and_then(|s| self.drive_awaited(s.drives_waited)) {
            Some(name) if !name.is_empty() => format!("Opening the drive {name}"),
            _ => "Getting your games ready".to_owned(),
        };
        Message::new(Art::Mark, "Chanoma", body)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::library::{Held, Library};

    fn app(here: bool) -> Chanoma {
        let dirs =
            crate::store::Dirs::under(&std::env::temp_dir().join(format!("chanoma-splash-{}", std::process::id())));
        let mut app = Chanoma::new(crate::Config { dirs, scan: false, pads: false, ..crate::Config::standard() });
        let folder = PathBuf::from("/run/media/ann/Stick/roms");
        app.settings.folders = vec![folder.clone()];
        app.library = Arc::new(Library { held: vec![Held { folder, here, ..Held::default() }], ..Library::default() });
        // Watching drives, UDisks not heard from.
        app.drives.disks = Some(aizome_sys::disks::Disks::with_bus(None, aizome_sys::Wake::new(|| {})));
        app.splash = Some(Splash { since: Instant::now(), scan_due: true, drives_waited: false });
        app
    }

    #[test]
    fn it_waits_for_the_games_then_a_drive_udisks_may_be_opening() {
        let mut a = app(false);
        a.splash_check();
        assert!(a.splash.is_some(), "the look at the folders not answered");
        a.splash.as_mut().unwrap().scan_due = false;
        a.splash_check();
        assert!(a.splash.is_some(), "the folder's drive not said of yet");
        a.drives.looked = true;
        a.splash_check();
        assert!(a.splash.is_none(), "UDisks has looked and isn't opening it: not plugged in");

        let mut a = app(false);
        a.splash.as_mut().unwrap().scan_due = false;
        a.splash.as_mut().unwrap().drives_waited = true;
        a.splash_check();
        assert!(a.splash.is_none(), "UDisks had its time");

        let mut a = app(true);
        a.splash.as_mut().unwrap().scan_due = false;
        a.splash_check();
        assert!(a.splash.is_none(), "the folder's there");
    }
}
