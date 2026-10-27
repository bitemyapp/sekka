//! What the shell carries across a restart (`aizomectl reload --shell`, a
//! new package after an update, or a crash and the compositor starting it
//! again): its notifications and their history, which apps' windows the
//! Deskbar folds, and which apps show every unread notification. Saved in
//! `$XDG_RUNTIME_DIR/aizome-shell-state.$WAYLAND_DISPLAY.json` whenever it
//! changes, read when the shell starts.
//!
//! A restart the compositor runs overlaps the two shells: the new one
//! takes the notifications' bus name (both hold it allowing replacement),
//! the old one writes its last state as it loses it, and the new one takes
//! in what came to the old one since it read the file
//! ([`wait_for_last`]).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::notifications::store::Notification;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Carried {
    /// The shell that wrote it.
    pub pid: u32,
    /// Written as that shell let go (it lost the bus name to a new shell,
    /// or stopped): nothing comes after.
    #[serde(rename = "final")]
    pub last: bool,
    pub notes: Vec<Notification>,
    /// The last notification id given.
    pub next_id: u32,
    pub folded: Vec<(String, bool)>,
    pub notes_open: Vec<String>,
}

/// Where it's kept: per session, in the runtime directory.
pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("AIZOME_SHELL_STATE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    aizome_ipc::runtime_dir().join(format!("aizome-shell-state.{display}.json"))
}

pub fn load() -> Option<Carried> {
    let text = std::fs::read_to_string(path()).ok()?;
    match serde_json::from_str::<Carried>(&text) {
        Ok(c) => Some(fitted(c)),
        Err(e) => {
            log::warn!("{}: {e}", path().display());
            None
        }
    }
}

/// Its notifications' images no larger than the shell keeps them: a shell
/// from before they were kept small may have kept one whole.
fn fitted(mut c: Carried) -> Carried {
    for n in &mut c.notes {
        n.image = n.image.take().map(|i| i.fit(crate::notifications::store::IMAGE_PX));
    }
    c
}

/// Writes it (whole, through a temporary file).
pub fn save(c: &Carried) {
    let p = path();
    let tmp = p.with_extension(format!("json.{}", std::process::id()));
    let r = serde_json::to_vec(c)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(&tmp, bytes))
        .and_then(|_| std::fs::rename(&tmp, &p));
    if let Err(e) = r {
        log::warn!("saving the shell's state to {}: {e}", p.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The shell this one replaces (the compositor says which), if any.
pub fn previous_shell() -> Option<u32> {
    std::env::var("AIZOME_SHELL_PREVIOUS_PID").ok()?.parse().ok()
}

/// Waits (on a thread) for the shell this one replaces to write its last
/// state, or to be gone, or eight seconds (the compositor stops it by
/// then): then what it wrote.
pub fn wait_for_last(previous: u32) -> Option<Carried> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(c) = load()
            && c.pid == previous
            && c.last
        {
            return Some(c);
        }
        let alive = std::path::Path::new(&format!("/proc/{previous}")).exists();
        if !alive || Instant::now() > deadline {
            return load().filter(|c| c.pid == previous);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A cheap mark of what would be carried, to save only on change.
pub fn mark(notes: &[Notification], folded: &[(String, bool)], open: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for n in notes {
        (n.id, n.read, n.at, &n.summary, &n.body, n.actions.len()).hash(&mut h);
    }
    folded.hash(&mut h);
    open.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_big_image_kept_whole_before_is_made_small_when_read() {
        let dir = std::env::temp_dir().join(format!("aizome-carry-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        let image = crate::notifications::store::NoteImage::Pixels {
            width: 1000,
            height: 1000,
            rgba: vec![255; 1000 * 1000 * 4],
        };
        let note = Notification { id: 9, summary: "a screenshot".into(), image: Some(image), ..Default::default() };
        let old = Carried { pid: 1, notes: vec![note], ..Default::default() };
        std::fs::write(&file, serde_json::to_vec(&old).unwrap()).unwrap();
        let c = fitted(serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap());
        let Some(crate::notifications::store::NoteImage::Pixels { width, height, .. }) = &c.notes[0].image else {
            panic!("an image")
        };
        assert_eq!((*width, *height), (128, 128));
        // What the shell then writes is small.
        assert!(serde_json::to_vec(&c).unwrap().len() < 300_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn it_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("aizome-carry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Safety: this test alone reads the variable.
        unsafe { std::env::set_var("AIZOME_SHELL_STATE", dir.join("state.json")) };
        let mut store = crate::notifications::store::Store::default();
        let p = store.post(
            crate::notifications::store::Incoming {
                app_name: "Mail".into(),
                summary: "Re: tiles".into(),
                ..Default::default()
            },
            "thunderbird".into(),
            false,
            7,
        );
        let c = Carried {
            pid: 42,
            last: true,
            notes: store.list.clone(),
            next_id: store.next_id(),
            folded: vec![("foot".into(), true)],
            notes_open: vec!["thunderbird".into()],
        };
        save(&c);
        assert_eq!(load(), Some(c.clone()));
        assert_eq!(wait_for_last(42), Some(c), "it was the last word of shell 42");
        let mut again = crate::notifications::store::Store::default();
        again.restore(store.list.clone(), store.next_id());
        assert_eq!(again.get(p.id).unwrap().summary, "Re: tiles");
        assert_ne!(again.fresh_id(), p.id, "ids go on from where they were");
        let _ = std::fs::remove_dir_all(dir);
    }
}
