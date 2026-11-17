//! Settings changes wake the reader; unchanged settings need no timer.
//!
//! The watcher follows the settings file by the kernel's file notifications
//! ([`Notified`]) or, where those can't be had, by looking at it every
//! interval ([`Polling`]). Placing the notifications is where it can go
//! wrong, because another program can change the folders meanwhile; the
//! steps of a placing, and what their types rule out, are in [`placing`],
//! over the answers the kernel gave, in [`kernel`].
//!
//! These modules are public so that their doctests can show what doesn't
//! compile. Nothing outside this crate can start a placing: the rest of
//! Aizome uses [`Watcher`] alone.

use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::{fs::MetadataExt, net::UnixStream};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

pub mod kernel;
pub mod placing;

use kernel::Unavailable;
use placing::Notifications;

/// Watches the settings file and calls back when it changes.
///
/// Linux file notifications cover writes, atomic replacement, deletion,
/// and settings directories created after startup. The thread blocks
/// until an event or shutdown; dropping the watcher stops and joins it.
/// If notifications cannot be installed, a logged polling fallback keeps
/// settings updates working.
pub struct Watcher {
    stop: Option<UnixStream>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn spawn(on_change: impl FnMut() + Send + 'static) -> Watcher {
        Self::spawn_for(crate::Settings::path(), Duration::from_secs(1), on_change)
    }

    /// Watch `path`. `interval` is used only if file notifications fail;
    /// it adds no delay or periodic wakeups while notifications work.
    pub fn spawn_for(path: PathBuf, interval: Duration, on_change: impl FnMut() + Send + 'static) -> Watcher {
        // Resolve relative paths before handing them to another thread.
        let path = std::path::absolute(path).expect("resolve the settings path");
        let (read, write) = UnixStream::pair().expect("settings watcher stop socket");
        let interval = interval.max(Duration::from_millis(1));
        let mut following =
            Following::new(Watched { path, interval, stop: read, last: None, on_change: Box::new(on_change) });
        // Place before returning, so an immediate save cannot be missed.
        following.with_typed(|state| match state {
            FollowingAny::Unplaced(unplaced) => (unplaced.place().into(), ()),
            placed @ (FollowingAny::Notified(_) | FollowingAny::Polling(_)) => (placed, ()),
        });
        let thread = std::thread::Builder::new()
            .name("aizome-config-watch".into())
            .spawn(move || while let Woken::GoOn = following.with_typed(woken) {})
            .expect("spawn the settings watcher");
        Watcher { stop: Some(write), thread: Some(thread) }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What the watcher's thread works on, whichever way it follows the file.
struct Watched {
    path: PathBuf,
    /// How often the file is looked at while it is polled.
    interval: Duration,
    /// Readable once the watcher is dropped.
    stop: UnixStream,
    /// The file as it was when last looked at.
    last: Option<Stamp>,
    on_change: Box<dyn FnMut() + Send>,
}

aizome_typestate::machine! {
    /// How the settings file is followed.
    pub struct Following over Watched {
        stored FollowingState;
        /// The way it is followed now, typed.
        typed FollowingAny;
        states {
            /// Before the first placing.
            Unplaced,
            /// By file notifications: every watch in them was asked for
            /// twice ([`placing::Kept`]).
            Notified(Notifications),
            /// By looking every interval, and trying to place notifications
            /// again each time. It takes the reason they can't be had, which
            /// a folder that merely went is not:
            ///
            /// ```compile_fail,E0308
            /// use aizome_config::watch::kernel::Placed;
            /// use aizome_config::watch::{Notified, Polling};
            /// // The settings folder went: a change, to place again for.
            /// fn give_up<'s>(notified: Notified<'s>, answer: Placed<'_>) -> Polling<'s> {
            ///     notified.poll(answer)
            /// }
            /// ```
            ///
            /// ```
            /// use aizome_config::watch::kernel::Unavailable;
            /// use aizome_config::watch::{Notified, Polling};
            /// fn give_up<'s>(notified: Notified<'s>, why: Unavailable) -> Polling<'s> {
            ///     notified.poll(why)
            /// }
            /// ```
            Polling(Unavailable),
        }
        branches {
            Followed = Notified | Polling;
        }
        // The watcher's thread is the only user: `Watcher` is the way in.
        access(private);
        new(private);
    }
}

/// Whether the watcher's thread goes on.
enum Woken {
    GoOn,
    Stop,
}

/// One turn of the watcher's thread: waits as the way the file is followed
/// says, and follows it from then as that turn left it.
fn woken<'s>(state: FollowingAny<'s>) -> (FollowingAny<'s>, Woken) {
    match state {
        FollowingAny::Unplaced(unplaced) => (unplaced.place().into(), Woken::GoOn),
        FollowingAny::Notified(notified) => notified.wake(),
        FollowingAny::Polling(polling) => polling.wake(),
    }
}

impl Watched {
    /// Places notifications again, then looks at the file. In that order:
    /// a rename may have replaced its inode or its folder, and what changes
    /// once the watch is placed is reported, so nothing falls between the
    /// placing and the look.
    fn place_again(&mut self) -> Result<Notifications, Unavailable> {
        let placed = Notifications::place(&self.path);
        let now = stamp(&self.path);
        if now != self.last {
            self.last = now;
            (self.on_change)();
        }
        placed
    }

    fn polling(&self, why: &Unavailable) {
        log::warn!("settings: polling {} because file notifications failed: {why}", self.path.display());
    }
}

impl<'s> Unplaced<'s> {
    /// The first placing. What the file looks like is noted after it, so a
    /// save made between the two is seen by one of them.
    fn place(self) -> Followed<'s> {
        let placed = Notifications::place(&self.ctx.path);
        self.ctx.last = stamp(&self.ctx.path);
        match placed {
            Ok(notifications) => Followed::Notified(Notified::enter(self.ctx, notifications)),
            Err(why) => {
                self.ctx.polling(&why);
                Followed::Polling(Polling::enter(self.ctx, why))
            }
        }
    }
}

impl<'s> Notified<'s> {
    /// Waits for a report or for the watcher to be dropped. A report about
    /// a watched path places the notifications again: a newly made folder
    /// lets the watch sit closer, and a moved one has taken its watch away.
    fn wake(self) -> (FollowingAny<'s>, Woken) {
        match wait(&self.ctx.stop, Some(&self.data), None) {
            Ok(Ready::Stop) => (self.into(), Woken::Stop),
            Ok(Ready::Change) => {
                match self.data.changed() {
                    Ok(false) => return (self.into(), Woken::GoOn),
                    Ok(true) => {}
                    Err(error) => log::warn!("settings notifications: {error}"),
                }
                match self.ctx.place_again() {
                    Ok(notifications) => (Notified::enter(self.ctx, notifications).into(), Woken::GoOn),
                    Err(why) => {
                        self.ctx.polling(&why);
                        (self.poll(why).into(), Woken::GoOn)
                    }
                }
            }
            Err(error) => {
                log::warn!("settings watcher stopped: {error}");
                (self.into(), Woken::Stop)
            }
        }
    }

    /// Notifications can't be had any more: the file is polled from now.
    pub fn poll(self, why: Unavailable) -> Polling<'s> {
        Polling::enter(self.ctx, why)
    }
}

impl<'s> Polling<'s> {
    /// Waits an interval, or for the watcher to be dropped, then looks at
    /// the file and tries notifications again.
    fn wake(self) -> (FollowingAny<'s>, Woken) {
        match wait(&self.ctx.stop, None, Some(self.ctx.interval)) {
            Ok(Ready::Stop) => (self.into(), Woken::Stop),
            Ok(Ready::Change) => match self.ctx.place_again() {
                Ok(notifications) => (Notified::enter(self.ctx, notifications).into(), Woken::GoOn),
                // Still unavailable: said once, when polling began.
                Err(why) => (Polling::enter(self.ctx, why).into(), Woken::GoOn),
            },
            Err(error) => {
                log::warn!("settings watcher stopped: {error}");
                (self.into(), Woken::Stop)
            }
        }
    }
}

/// Inode identity detects atomic replacements even when size and mtime
/// are preserved. ctime also detects an in-place write with restored mtime.
type Stamp = (u64, u64, u64, i64, i64, i64, i64);

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec()))
}

enum Ready {
    Stop,
    Change,
}

/// Waits for the stop socket, for `watch` to have something to read, or for
/// `timeout`: the last two both mean it is time to look.
fn wait(stop: &UnixStream, watch: Option<&Notifications>, timeout: Option<Duration>) -> io::Result<Ready> {
    let mut fds = [
        libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: watch.map_or(-1, |w| w.as_raw_fd()), events: libc::POLLIN, revents: 0 },
    ];
    let timeout = timeout.map_or(-1, |d| d.as_millis().min(i32::MAX as u128) as i32);
    loop {
        // SAFETY: both entries are valid pollfd values for this call.
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout) } >= 0 {
            return Ok(if fds[0].revents != 0 { Ready::Stop } else { Ready::Change });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
