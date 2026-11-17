//! One placing of the watch, as steps that can only be taken in order.
//!
//! The watch is placed from the settings file upwards: its folder, or the
//! nearest folder above it that exists, then every folder above that.
//! Another program can change those folders meanwhile, and what each step
//! takes is what keeps a change from going unseen:
//!
//! - [`Chain::found`] takes a [`Nearest`]: the placing stops above the
//!   settings folder only on an absence seen through the watch above it.
//! - [`Chain::confirm`] exists only once the chain is [`Whole`]: every watch
//!   is placed before any is asked for again, and only watches asked for
//!   twice are [`Kept`].
//! - A step that fails says whether to place [`Step::Again`] or that
//!   notifications are [`Unavailable`]; a folder that went is the first.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};

use aizome_typestate::{Confirmed, Reviewed};

use super::kernel::{
    ABOVE, Asked, Below, Budget, CHANGES, FOLDER, Inotify, Landed, MOVED, Nearest, Placed, Placement, Step,
    Unavailable, name,
};

/// The watches asked for in one placing, and how far it has got: one of
/// [`ToNearest`], [`ToRoot`] and [`Whole`].
#[derive(Debug)]
pub struct Chain<'p, Stage> {
    /// Each watch, with the name to listen for in its folder, if any.
    asked: Vec<(Reviewed<Asked<'p>, Placement>, Option<OsString>)>,
    stage: Stage,
}

/// Looking for the nearest folder to the settings file that exists.
#[derive(Debug)]
pub struct ToNearest;

/// The nearest folder is watched; those above it come next.
#[derive(Debug)]
pub struct ToRoot<'p> {
    nearest: &'p Path,
}

/// Every folder is watched. Nothing has been asked for twice yet.
#[derive(Debug)]
pub struct Whole;

/// The watches of a finished placing, each asked for twice.
#[derive(Debug, Default)]
pub struct Kept {
    watches: Vec<(Confirmed<Landed, Placement>, Option<OsString>)>,
}

/// Where a placing has got to. A test changes the folders at that moment,
/// as another program might.
#[cfg_attr(not(test), expect(dead_code, reason = "only tests look at where a placing has got to"))]
pub(super) enum Moment<'a, 'p> {
    /// This folder is about to be asked for. A test that moves it now names
    /// where it went, and the watch lands on it there.
    Watching(&'p Path, &'a mut Option<PathBuf>),
    /// This folder was found missing; the one above it is tried next.
    Missing(&'p Path),
    /// The nearest folder is watched; those above it are watched next.
    Nearest(&'p Path),
}

impl<'p> Chain<'p, ToNearest> {
    pub(super) fn new() -> Self {
        Chain { asked: Vec::new(), stage: ToNearest }
    }

    /// Watches the nearest folder to `file` that exists, from its own
    /// upwards.
    pub(super) fn descend(
        mut self,
        inotify: &Inotify,
        file: &'p Path,
        placings: &mut Budget,
        at: &mut dyn FnMut(Moment<'_, 'p>),
    ) -> Result<Chain<'p, ToRoot<'p>>, Step> {
        'down: loop {
            for folder in file.ancestors().skip(1) {
                let mut landing = None;
                at(Moment::Watching(folder, &mut landing));
                match inotify.watch_landing(folder, landing.as_deref(), FOLDER) {
                    Placed::Watching(watch) => match Nearest::below(watch, file) {
                        Below::Nothing(nearest) => {
                            at(Moment::Nearest(nearest.folder()));
                            return Ok(self.found(nearest));
                        }
                        // A folder found missing was made before the watch
                        // on this one was placed. What is written inside it
                        // would go unseen from here: go back down to it,
                        // keeping this watch for its name.
                        Below::Folder(watch, made) => {
                            self.asked.push((watch, Some(name(made))));
                            placings.spend()?;
                            continue 'down;
                        }
                    },
                    Placed::Gone => at(Moment::Missing(folder)),
                    Placed::Unavailable(why) => return Err(Step::Unavailable(why)),
                }
            }
            // Not even the root was there to watch.
            return Err(Step::Again);
        }
    }

    /// The nearest folder that exists is watched: on to those above it.
    /// A watch alone doesn't say so. A folder found missing earlier can
    /// have been made since, unreported, so this takes the look below the
    /// watch ([`Nearest`]):
    ///
    /// ```compile_fail,E0308
    /// use aizome_config::watch::kernel::{Asked, Placement};
    /// use aizome_config::watch::placing::{Chain, ToNearest, ToRoot};
    /// use aizome_typestate::Reviewed;
    /// // Stops at a watched folder without looking below it.
    /// fn stop<'p>(chain: Chain<'p, ToNearest>, watch: Reviewed<Asked<'p>, Placement>) -> Chain<'p, ToRoot<'p>> {
    ///     chain.found(watch)
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_config::watch::kernel::Nearest;
    /// use aizome_config::watch::placing::{Chain, ToNearest, ToRoot};
    /// fn stop<'p>(chain: Chain<'p, ToNearest>, nearest: Nearest<'p>) -> Chain<'p, ToRoot<'p>> {
    ///     chain.found(nearest)
    /// }
    /// ```
    pub fn found(mut self, nearest: Nearest<'p>) -> Chain<'p, ToRoot<'p>> {
        let folder = nearest.folder();
        let (watch, child) = nearest.into_watch();
        self.asked.push((watch, Some(child)));
        Chain { asked: self.asked, stage: ToRoot { nearest: folder } }
    }
}

impl<'p> Chain<'p, ToRoot<'p>> {
    /// Watches every folder above the nearest for being moved or removed,
    /// which takes the settings path with it. These watches don't observe
    /// what is inside them.
    ///
    /// A folder above that has gone is a change, not a failure of
    /// notifications, and polling wouldn't see the next one for a whole
    /// interval: the placing is given up to be made again.
    pub fn above(mut self, inotify: &Inotify) -> Result<Chain<'p, Whole>, Step> {
        for folder in self.stage.nearest.ancestors().skip(1) {
            match inotify.watch(folder, ABOVE) {
                Placed::Watching(watch) => self.asked.push((watch, None)),
                Placed::Gone => return Err(Step::Again),
                Placed::Unavailable(why) => return Err(Step::Unavailable(why)),
            }
        }
        Ok(Chain { asked: self.asked, stage: Whole })
    }
}

impl Chain<'_, Whole> {
    /// Asks for every watch again, now that all are placed
    /// ([`Inotify::again`]). One that came back different is on something
    /// that has left the settings path, and the placing is given up to be
    /// made again.
    ///
    /// Only a whole chain can be confirmed. Asked for again while folders
    /// above were still unwatched, a watch could be confirmed and the folder
    /// above it then swapped for another before its own watch landed:
    ///
    /// ```compile_fail,E0599
    /// use aizome_config::watch::kernel::Inotify;
    /// use aizome_config::watch::placing::{Chain, ToRoot};
    /// fn hurry(chain: Chain<'_, ToRoot<'_>>, inotify: &Inotify) {
    ///     let _ = chain.confirm(inotify);
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_config::watch::kernel::{Inotify, Step};
    /// use aizome_config::watch::placing::{Chain, Kept, ToRoot};
    /// fn in_order(chain: Chain<'_, ToRoot<'_>>, inotify: &Inotify) -> Result<Kept, Step> {
    ///     chain.above(inotify)?.confirm(inotify)
    /// }
    /// ```
    pub fn confirm(self, inotify: &Inotify) -> Result<Kept, Step> {
        let mut kept = Kept::default();
        for (asked, child) in self.asked {
            kept.watches.push((inotify.again(asked)?, child));
        }
        Ok(kept)
    }
}

impl Kept {
    /// Whether a report on watch `wd`, about `name` in it or about the
    /// watched file itself (no name), is one a watch here listens for. A
    /// name filters what happens inside a folder; every watch also hears
    /// about its own file.
    fn hears(&self, wd: i32, name: &[u8]) -> bool {
        use std::os::unix::ffi::OsStrExt;
        self.watches.iter().any(|(watch, child)| {
            watch.target().wd() == wd && (name.is_empty() || child.as_ref().is_some_and(|c| name == c.as_bytes()))
        })
    }
}

/// File notifications for the settings file: its watches, each asked for
/// twice, and the kernel's queue of what they report.
#[derive(Debug)]
pub struct Notifications {
    inotify: Inotify,
    kept: Kept,
}

impl AsRawFd for Notifications {
    fn as_raw_fd(&self) -> RawFd {
        self.inotify.as_raw_fd()
    }
}

impl Notifications {
    /// Reads what has been reported: whether any of it was about a watched
    /// path.
    pub(super) fn changed(&self) -> io::Result<bool> {
        self.inotify.reported(|wd, name| self.kept.hears(wd, name))
    }

    /// Places the watch for the settings file at `path`.
    pub(super) fn place(path: &Path) -> Result<Notifications, Unavailable> {
        Self::place_seeing(path, &mut |_| {})
    }

    /// [`Notifications::place`], telling `at` where it has got to.
    pub(super) fn place_seeing(path: &Path, at: &mut dyn FnMut(Moment<'_, '_>)) -> Result<Notifications, Unavailable> {
        let inotify = Inotify::new()?;
        let mut kept = Kept::default();
        let mut symlinks = Budget::symlinks();
        let mut pending = vec![path.to_owned()];
        let mut visited = HashSet::new();
        while let Some(path) = pending.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            symlinks.spend()?;
            let mut placings = Budget::placings();
            let folders = loop {
                let placing = Chain::new()
                    .descend(&inotify, &path, &mut placings, at)
                    .and_then(|chain| chain.above(&inotify))
                    .and_then(|chain| chain.confirm(&inotify));
                match placing {
                    Ok(folders) => break folders,
                    Err(Step::Again) => placings.spend()?,
                    Err(Step::Unavailable(why)) => return Err(why),
                }
            };
            kept.watches.extend(folders.watches);
            // Watch both sides of symlinks, including directory symlinks:
            // the link can be retargeted and its target deleted/recreated.
            for component in path.ancestors() {
                if let Ok(target) = std::fs::read_link(component) {
                    pending.push(component.to_owned());
                    let mut resolved = component.parent().unwrap().join(target);
                    let suffix = path.strip_prefix(component).unwrap();
                    if !suffix.as_os_str().is_empty() {
                        resolved.push(suffix);
                    }
                    pending.push(resolved);
                }
            }
            // Also observe writes made through a different hard link. A file
            // replaced as this watch landed needs no second placing: its
            // folder is watched already, and has reported the replacement.
            let itself = if path.is_dir() { MOVED } else { CHANGES };
            match inotify.watch(&path, itself) {
                Placed::Watching(asked) => match inotify.again(asked) {
                    Ok(watch) => kept.watches.push((watch, None)),
                    Err(Step::Again) => {}
                    Err(Step::Unavailable(why)) => return Err(why),
                },
                Placed::Gone => {}
                Placed::Unavailable(why) => return Err(why),
            }
        }
        Ok(Notifications { inotify, kept })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    /// A scratch folder, removed when dropped.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("aizome-settings-watch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Whether `watch` reports a change within five seconds.
    fn reports(watch: &Notifications) -> bool {
        let (stop, _held) = UnixStream::pair().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            crate::watch::wait(&stop, Some(watch), Some(left)).unwrap();
            if watch.changed().unwrap() {
                return true;
            }
        }
    }

    #[test]
    fn a_folder_made_before_the_watch_above_it_is_still_watched() {
        let dir = Temp::new("made-meanwhile");
        let folder = dir.0.join("config/aizome");
        let path = folder.join("settings.toml");
        // The settings folder is found missing and is made, with the one
        // above it, before the watch tries that one: nothing reports either.
        let mut found_missing = Vec::new();
        let watch = Notifications::place_seeing(&path, &mut |at| {
            if let Moment::Missing(missing) = at {
                if found_missing.is_empty() {
                    std::fs::create_dir_all(&folder).unwrap();
                }
                found_missing.push(missing.to_owned());
            }
        })
        .unwrap();
        assert_eq!(found_missing, [folder.as_path()], "found missing once, then watched");
        std::fs::write(&path, "written into the folder made meanwhile").unwrap();
        assert!(reports(&watch), "a file written into that folder is reported");
    }

    #[test]
    fn folders_made_while_the_watch_climbs_two_levels_are_still_watched() {
        let dir = Temp::new("made-two-levels");
        let above = dir.0.join("config");
        let folder = above.join("aizome");
        let path = folder.join("settings.toml");
        // Both folders are found missing; both are made before the watch
        // is placed on the scratch folder, which reports neither.
        let mut found_missing = Vec::new();
        let watch = Notifications::place_seeing(&path, &mut |at| {
            if let Moment::Missing(missing) = at {
                found_missing.push(missing.to_owned());
                if found_missing.len() == 2 {
                    std::fs::create_dir_all(&folder).unwrap();
                }
            }
        })
        .unwrap();
        assert_eq!(found_missing, [folder.as_path(), above.as_path()]);
        std::fs::write(&path, "written two levels down").unwrap();
        assert!(reports(&watch), "a file written into the new folders is reported");
    }

    #[test]
    fn a_folder_above_moved_away_while_the_watch_is_placed_is_not_a_failure() {
        let dir = Temp::new("moved-meanwhile");
        let above = dir.0.join("config");
        let folder = above.join("aizome");
        let path = folder.join("settings.toml");
        std::fs::create_dir_all(&folder).unwrap();
        // The settings folder is watched; before the folders above it are,
        // the one above is renamed away. The settings path is simply absent
        // now, as it is after any rename: notifications haven't failed.
        let mut moved = false;
        let watch = Notifications::place_seeing(&path, &mut |at| {
            if let Moment::Nearest(nearest) = at
                && !moved
            {
                assert_eq!(nearest, folder);
                std::fs::rename(&above, dir.0.join("old-config")).unwrap();
                moved = true;
            }
        })
        .expect("a path that changed is watched, not polled");
        assert!(moved);
        // The watch is on what is there now, so the folders' return is seen.
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&path, "written after the folders came back").unwrap();
        assert!(reports(&watch), "the settings folder's return is reported");
    }

    #[test]
    fn a_folder_moved_after_it_was_found_and_before_it_was_watched_is_noticed() {
        let dir = Temp::new("moved-as-watched");
        let folder = dir.0.join("config/aizome");
        let path = folder.join("settings.toml");
        let previous = dir.0.join("previous");
        std::fs::create_dir_all(&folder).unwrap();
        // The settings folder is found, moved away, and only then watched:
        // the watch lands on the moved folder, and its move is not reported
        // because it has already happened.
        let mut moved = false;
        let watch = Notifications::place_seeing(&path, &mut |at| {
            if let Moment::Watching(found, landing) = at
                && found == folder
                && !moved
            {
                std::fs::rename(&folder, &previous).unwrap();
                *landing = Some(previous.clone());
                moved = true;
            }
        })
        .unwrap();
        assert!(moved);
        // The watch was placed again on what is at the path, so a new
        // settings folder there, and the file written into it, are seen.
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(&path, "written into the new folder").unwrap();
        assert!(reports(&watch), "the new settings folder is reported");
    }

    #[test]
    fn folders_that_never_hold_still_end_in_polling_not_in_a_loop() {
        let dir = Temp::new("never-still");
        let above = dir.0.join("config");
        let folder = above.join("aizome");
        let path = folder.join("settings.toml");
        std::fs::create_dir_all(&folder).unwrap();
        // Every time the settings folder is watched, the folder above it is
        // swapped for a new one before its own watch is placed.
        let mut swaps = 0;
        let placed = Notifications::place_seeing(&path, &mut |at| {
            if let Moment::Nearest(_) = at {
                swaps += 1;
                std::fs::rename(&above, dir.0.join(format!("old-{swaps}"))).unwrap();
                std::fs::create_dir_all(&folder).unwrap();
            }
        });
        let why = placed.expect_err("sixteen placings were given up").to_string();
        assert!(why.contains("kept changing"), "{why}");
        assert_eq!(swaps, 17, "the first placing and sixteen more");
    }
}
