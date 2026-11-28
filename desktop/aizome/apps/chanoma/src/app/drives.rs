//! Drives plugged in, opened as Chanoma starts and as they come
//! ([`aizome_sys::disks::Automount`]): a television's films and games are
//! often on a USB disk, and nobody opens it in a file manager first.
//!
//! Opening runs off the interface thread; a drive opened brings its
//! folders back (the library is read again, and the picker's places). One
//! that couldn't be opened is said once, in a note
//! at the top: not again each time Chanoma starts, nor while it stays
//! broken, but again if it opens one day and later fails. A drive open in
//! another account's folder, where this one can't read it, is said the same
//! way. The settings' Folders keep saying which drives couldn't be opened,
//! and why, with a way to try again.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use aizome_kit::MsgSender;
use aizome_kit::prelude::*;
use aizome_sys::disks::{Automount, Disks, DisksState, Outcome};
use serde::{Deserialize, Serialize};

use super::{Chanoma, Msg};
use crate::store::Dirs;
use crate::ui;

/// A drive that couldn't be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unopened {
    pub block: String,
    pub name: String,
    /// What it would have been mounted as (its label, else its UUID).
    pub label: String,
    /// What knows it again (its UUID, else its label, else its name).
    pub key: String,
    pub why: String,
}

/// The drives said not to open, each once until it opens.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warned {
    pub drives: BTreeSet<String>,
}

#[derive(Default)]
pub struct Drives {
    pub disks: Option<Disks>,
    pub automount: Automount,
    pub unopened: Vec<Unopened>,
    pub warned: Warned,
    /// Drives found open where this account can't read them, by block.
    pub elsewhere: HashSet<String>,
    /// UDisks's drives looked at once (those to open are being opened).
    pub looked: bool,
    path: std::path::PathBuf,
}

/// Drives open in another account's folder (`/run/media/USER` is that
/// account's alone), as when a grown-up's desktop opened one before the TV
/// account logged in: said as drives that couldn't be opened, each once
/// while it stays open there.
pub fn elsewhere(state: &DisksState, told: &mut HashSet<String>) -> Vec<Outcome> {
    let volumes = state.volumes();
    told.retain(|b| volumes.iter().any(|v| &v.block == b && v.mount_point.is_some()));
    volumes
        .into_iter()
        .filter_map(|v| {
            let at = v.mount_point.clone()?;
            // Asks only of the folders above the drive, never the drive.
            let denied = matches!(std::fs::metadata(&at), Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied);
            if !denied || !told.insert(v.block.clone()) {
                return None;
            }
            let (label, uuid) =
                state.block(&v.block).map(|b| (b.id_label.clone(), b.id_uuid.clone())).unwrap_or_default();
            let why = format!("It's open in another account ({at}): eject it there, or unplug it and plug it back in");
            Some(Outcome { volume: v, label, uuid, result: Err(why) })
        })
        .collect()
}

/// How a folder's drive is, for the settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriveIs {
    Opening(String),
    Unopened(String),
}

impl Drives {
    pub fn load(dirs: &Dirs) -> Drives {
        let path = dirs.state.join("drives.json");
        Drives { warned: crate::store::read(&path), path, ..Drives::default() }
    }

    /// Follow UDisks: what's plugged in now is looked at as soon as it's
    /// read, and what comes later as it comes.
    pub fn watch(&mut self, tx: MsgSender<Msg>) {
        self.disks = Some(Disks::new(aizome_sys::Wake::new(move || {
            tx.send(Msg::Drives);
        })));
    }

    fn save(&self) {
        if let Err(e) = crate::store::write(&self.path, &self.warned) {
            log::warn!("chanoma: {}: {e}", self.path.display());
        }
    }

    /// The drive `folder` is on, if it's being opened or couldn't be,
    /// going by where it would be mounted (UDisks names the folder after
    /// the label).
    pub fn of_folder(&self, folder: &Path, mounts: &[std::path::PathBuf]) -> Option<DriveIs> {
        let on = |label: &str| !label.is_empty() && mounts.iter().any(|m| folder.starts_with(m.join(label)));
        if let Some(o) =
            self.automount.opening().iter().find(|o| on(if o.label.is_empty() { &o.uuid } else { &o.label }))
        {
            return Some(DriveIs::Opening(o.volume.name.clone()));
        }
        self.unopened.iter().find(|u| on(&u.label)).map(|u| DriveIs::Unopened(u.name.clone()))
    }
}

impl Chanoma {
    /// UDisks said something: open what's new, and take in what opened.
    pub fn drives_changed(&mut self, cx: &mut Cx<Msg>) {
        let Some(disks) = &self.drives.disks else { return };
        let mut outcomes = self.drives.automount.update(disks);
        outcomes.extend(elsewhere(&disks.snapshot(), &mut self.drives.elsewhere));
        self.drives.looked |= disks.available();
        self.opened(outcomes, cx);
        self.splash_check();
    }

    /// Drives that finished opening: their folders come back, and those
    /// that couldn't be opened are said (each once).
    pub fn opened(&mut self, outcomes: Vec<Outcome>, cx: &mut Cx<Msg>) {
        let mut any = false;
        let mut said = Vec::new();
        let mut changed = false;
        for o in outcomes {
            let key =
                [&o.uuid, &o.label, &o.volume.name].into_iter().find(|k| !k.is_empty()).cloned().unwrap_or_default();
            self.drives.unopened.retain(|u| u.block != o.volume.block && u.key != key);
            match o.result {
                Ok(at) => {
                    log::info!("chanoma: opened the drive {} at {at}", o.volume.name);
                    any = true;
                    changed |= self.drives.warned.drives.remove(&key);
                }
                Err(why) => {
                    log::warn!("chanoma: couldn't open the drive {} ({}): {why}", o.volume.name, o.volume.block);
                    if self.drives.warned.drives.insert(key.clone()) {
                        said.push(o.volume.name.clone());
                        changed = true;
                    }
                    let label = if o.label.is_empty() { o.uuid } else { o.label };
                    self.drives.unopened.push(Unopened { block: o.volume.block, name: o.volume.name, label, key, why });
                }
            }
        }
        if changed {
            self.drives.save();
        }
        if !said.is_empty() {
            log::info!("chanoma: said {} couldn't be opened", said.join(", "));
        }
        match said.as_slice() {
            [] => {}
            [name] => self.toast_note(ui::Note::Drive, format!("Couldn't open the drive {name}"), cx),
            more => self.toast_note(ui::Note::Drive, format!("Couldn't open {} drives", more.len()), cx),
        }
        if any {
            self.rescan(cx);
            // The picker's places now rather than at its next second.
            self.picker_tick(cx);
        }
    }

    /// Try a drive that couldn't be opened again (from the settings).
    pub fn try_drive_again(&mut self, i: usize, cx: &mut Cx<Msg>) {
        if i >= self.drives.unopened.len() {
            return;
        }
        let u = self.drives.unopened.remove(i);
        self.drives.automount.try_again(&u.block);
        self.drives.elsewhere.remove(&u.block);
        self.drives_changed(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_sys::disks::{BlockDevice, Drive, Filesystem};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_drive_open_in_another_accounts_folder_is_said_once_while_it_stays_there() {
        // Another account's /run/media/USER: this one may not look in.
        let theirs = std::env::temp_dir().join(format!("chanoma-elsewhere-{}", std::process::id()));
        let at = theirs.join("Extreme Pro");
        std::fs::create_dir_all(&at).unwrap();
        std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o000)).unwrap();
        let state = |mounted: Option<&Path>| DisksState {
            available: true,
            drives: vec![Drive { path: "usb".into(), removable: true, ..Default::default() }],
            blocks: vec![BlockDevice {
                path: "sda1".into(),
                drive: Some("usb".into()),
                id_label: "Extreme Pro".into(),
                id_uuid: "648D-C135".into(),
                hint_auto: true,
                filesystem: Some(Filesystem {
                    mount_points: mounted.map(|m| m.display().to_string()).into_iter().collect(),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        if std::fs::metadata(&at).is_ok() {
            // Root looks in every folder: nothing to try here.
            std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::remove_dir_all(&theirs).unwrap();
            return;
        }
        let mut told = HashSet::new();
        let found = elsewhere(&state(Some(&at)), &mut told);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].label.as_str(), found[0].uuid.as_str()), ("Extreme Pro", "648D-C135"));
        assert!(found[0].result.as_ref().unwrap_err().contains("open in another account"));
        assert!(elsewhere(&state(Some(&at)), &mut told).is_empty(), "said once");
        // Unmounted there (ejected, or unplugged): forgotten, so said again
        // if it comes back the same way.
        assert!(elsewhere(&state(None), &mut told).is_empty());
        assert_eq!(elsewhere(&state(Some(&at)), &mut told).len(), 1);
        // Readable (this account's own): nothing to say.
        std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(elsewhere(&state(Some(&at)), &mut HashSet::new()).is_empty());
        std::fs::remove_dir_all(&theirs).unwrap();
    }
}
