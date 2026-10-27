//! The calendars on disk: one `.ics` file per calendar in a folder
//! (`$XDG_DATA_HOME/aizome/koyomi/`), read at start, read again when
//! another program changes them, written whole and atomically.
//!
//! Removing a calendar moves its file into `.trash/` in the folder, so
//! it can be had back.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::model::{Calendar, Dye, Event};
use crate::settings::write_atomic;

/// What a file looked like when last read or written: its modification
/// time and length.
pub type Stamp = Option<(SystemTime, u64)>;

pub fn stamp(path: &Path) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// The calendars in a folder.
#[derive(Clone, Debug, Default)]
pub struct Store {
    pub dir: PathBuf,
    pub calendars: Vec<Calendar>,
    /// Each calendar's file as Koyomi last knew it.
    stamps: Vec<Stamp>,
    /// Files that couldn't be read, with why.
    pub problems: Vec<String>,
}

impl Store {
    /// The calendars in `dir` (none if it's missing), sorted by name.
    pub fn open(dir: impl Into<PathBuf>) -> Store {
        let mut s = Store { dir: dir.into(), ..Store::default() };
        s.load();
        s
    }

    fn files(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut v: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ics")) && p.is_file())
            .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
            .collect();
        v.sort();
        v
    }

    fn load(&mut self) {
        self.calendars.clear();
        self.stamps.clear();
        self.problems.clear();
        for path in self.files() {
            let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let st = stamp(&path);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let (cal, dropped) = Calendar::parse(&id, &text);
                    if dropped > 0 {
                        self.problems
                            .push(format!("{}: {dropped} events without a start were skipped", path.display()));
                    }
                    self.calendars.push(cal);
                    self.stamps.push(st);
                }
                Err(e) => self.problems.push(format!("{}: {e}", path.display())),
            }
        }
        self.sort();
    }

    fn sort(&mut self) {
        let mut pairs: Vec<(Calendar, Stamp)> = self.calendars.drain(..).zip(self.stamps.drain(..)).collect();
        pairs.sort_by(|a, b| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()).then(a.0.id.cmp(&b.0.id)));
        for (c, s) in pairs {
            self.calendars.push(c);
            self.stamps.push(s);
        }
    }

    pub fn path_of(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.ics"))
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.calendars.iter().position(|c| c.id == id)
    }

    /// Whether files came, went or changed since Koyomi last read or
    /// wrote them.
    pub fn changed_on_disk(&self) -> bool {
        let files = self.files();
        if files.len() != self.calendars.len() {
            return true;
        }
        files.iter().any(|p| {
            let id = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match self.index_of(&id) {
                Some(i) => stamp(p) != self.stamps[i],
                None => true,
            }
        })
    }

    /// Read everything again (after `changed_on_disk`).
    pub fn reload(&mut self) {
        self.load();
    }

    /// Write calendar `i` now.
    pub fn save(&mut self, i: usize) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path_of(&self.calendars[i].id);
        write_atomic(&path, self.calendars[i].to_ics().as_bytes())?;
        self.stamps[i] = stamp(&path);
        Ok(())
    }

    /// What a save of calendar `i` writes, to write elsewhere (a
    /// background thread): the path and the text.
    pub fn snapshot(&self, i: usize) -> (PathBuf, Calendar) {
        (self.path_of(&self.calendars[i].id), self.calendars[i].clone())
    }

    /// Note that calendar `id` was written (by a background save) and
    /// its file now looks like `st`.
    pub fn saved(&mut self, id: &str, st: Stamp) {
        if let Some(i) = self.index_of(id) {
            self.stamps[i] = st;
        }
    }

    /// A new, empty calendar called `name`: its index.
    pub fn create(&mut self, name: &str, color: Dye) -> std::io::Result<usize> {
        let id = self.fresh_id(name);
        let cal = Calendar::new(id.clone(), name, color);
        self.calendars.push(cal);
        self.stamps.push(None);
        self.sort();
        let i = self.index_of(&id).expect("just added");
        self.save(i)?;
        Ok(i)
    }

    /// An id for a calendar called `name` that no file has.
    pub fn fresh_id(&self, name: &str) -> String {
        let mut slug: String = name
            .chars()
            .flat_map(|c| if c.is_alphanumeric() { c.to_lowercase().collect::<Vec<char>>() } else { vec!['-'] })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        if slug.is_empty() {
            slug = "calendar".into();
        }
        let mut id = slug.clone();
        let mut n = 2;
        while self.index_of(&id).is_some() || self.path_of(&id).exists() {
            id = format!("{slug}-{n}");
            n += 1;
        }
        id
    }

    /// Take calendar `i` away: its file goes to `.trash/`.
    pub fn remove(&mut self, i: usize) -> std::io::Result<Calendar> {
        let cal = self.calendars.remove(i);
        self.stamps.remove(i);
        let path = self.path_of(&cal.id);
        if path.exists() {
            let trash = self.dir.join(".trash");
            std::fs::create_dir_all(&trash)?;
            let stamp = jiff::Timestamp::now().as_second();
            std::fs::rename(&path, trash.join(format!("{}-{stamp}.ics", cal.id)))?;
        }
        Ok(cal)
    }

    /// Put back a calendar taken away (an undo): written again.
    pub fn restore(&mut self, cal: Calendar) -> std::io::Result<usize> {
        let id = cal.id.clone();
        self.calendars.push(cal);
        self.stamps.push(None);
        self.sort();
        let i = self.index_of(&id).expect("just added");
        self.save(i)?;
        Ok(i)
    }

    /// The first calendars Koyomi makes in an empty folder.
    pub fn ensure_some(&mut self) -> std::io::Result<()> {
        if self.calendars.is_empty() && self.files().is_empty() {
            self.create("Home", Dye::Hanada)?;
            self.create("Work", Dye::Kohaku)?;
        }
        Ok(())
    }
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub added: usize,
    pub updated: usize,
    /// Events already there in the same or a newer version.
    pub unchanged: usize,
    /// Events without a start.
    pub skipped: usize,
}

/// Merge `from`'s events into `into`: an event with the same UID (and
/// instance) is replaced if the import's is the same version or newer,
/// others are added. Zone definitions come along.
pub fn merge(into: &mut Calendar, from: &Calendar) -> Imported {
    let mut r = Imported::default();
    for e in &from.events {
        let same = |x: &Event| {
            x.uid == e.uid && x.recurrence_id.as_ref().map(|w| w.civil()) == e.recurrence_id.as_ref().map(|w| w.civil())
        };
        match into.events.iter().position(same) {
            Some(i) => {
                if e.sequence >= into.events[i].sequence && into.events[i] != *e {
                    into.events[i] = e.clone();
                    r.updated += 1;
                } else {
                    r.unchanged += 1;
                }
            }
            None => {
                into.events.push(e.clone());
                r.added += 1;
            }
        }
    }
    for t in &from.timezones {
        let id = t.text("TZID");
        if !into.timezones.iter().any(|x| x.text("TZID") == id) {
            into.timezones.push(t.clone());
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::When;
    use jiff::civil::date;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("koyomi-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn creates_saves_reloads_and_removes() {
        let dir = scratch("cycle");
        let mut s = Store::open(&dir);
        assert!(s.calendars.is_empty());
        s.ensure_some().unwrap();
        assert_eq!(s.calendars.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Home", "Work"]);
        assert!(!s.changed_on_disk());
        let i = s.index_of("work").unwrap();
        s.calendars[i].events.push(Event::new(
            "Ship it",
            When::Date(date(2026, 10, 20)),
            When::Date(date(2026, 10, 21)),
        ));
        s.save(i).unwrap();
        assert!(!s.changed_on_disk());
        // Another program writes a calendar.
        std::fs::write(dir.join("shared.ics"), "BEGIN:VCALENDAR\nX-WR-CALNAME:Shared\nEND:VCALENDAR\n").unwrap();
        assert!(s.changed_on_disk());
        s.reload();
        assert_eq!(s.calendars.len(), 3);
        assert_eq!(s.calendars[s.index_of("work").unwrap()].events[0].summary, "Ship it");
        assert_eq!(s.fresh_id("Work"), "work-2");
        assert_eq!(s.fresh_id("Ünïcode & Friends!"), "ünïcode-friends");
        let gone = s.remove(s.index_of("shared").unwrap()).unwrap();
        assert!(!dir.join("shared.ics").exists());
        assert_eq!(std::fs::read_dir(dir.join(".trash")).unwrap().count(), 1);
        s.restore(gone).unwrap();
        assert!(dir.join("shared.ics").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_merge_by_uid_and_version() {
        let mut into = Calendar::parse(
            "a",
            "BEGIN:VEVENT\nUID:1\nDTSTART;VALUE=DATE:20261017\nSUMMARY:Old\nSEQUENCE:1\nEND:VEVENT\n",
        )
        .0;
        let from = Calendar::parse(
            "b",
            "BEGIN:VEVENT\nUID:1\nDTSTART;VALUE=DATE:20261017\nSUMMARY:New\nSEQUENCE:2\nEND:VEVENT\nBEGIN:VEVENT\nUID:2\nDTSTART;VALUE=DATE:20261018\nSUMMARY:Other\nEND:VEVENT\n",
        )
        .0;
        let r = merge(&mut into, &from);
        assert_eq!(r, Imported { added: 1, updated: 1, unchanged: 0, skipped: 0 });
        assert_eq!(into.events[0].summary, "New");
        let again = merge(&mut into, &from);
        assert_eq!(again.unchanged, 2);
    }
}
