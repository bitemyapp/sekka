//! Drives plugged in, opened as Chanoma starts and as they come
//! ([`aizome_sys::disks::Automount`]): a television's films and games are
//! often on a USB disk, and nobody opens it in a file manager first.
//!
//! Opening runs off the interface thread; a drive opened brings its
//! folders back (the library is read again, and the picker's places). One
//! that couldn't be opened is said once, in a note
//! at the top: not again each time Chanoma starts, nor while it stays
//! broken, but again if it opens one day and later fails. The settings'
//! Folders keep saying which drives couldn't be opened, and why, with a
//! way to try again.

use std::collections::BTreeSet;
use std::path::Path;

use aizome_kit::MsgSender;
use aizome_kit::prelude::*;
use aizome_sys::disks::{Automount, Disks, Outcome};
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
    path: std::path::PathBuf,
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
        let outcomes = self.drives.automount.update(disks);
        self.opened(outcomes, cx);
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
        self.drives_changed(cx);
    }
}
