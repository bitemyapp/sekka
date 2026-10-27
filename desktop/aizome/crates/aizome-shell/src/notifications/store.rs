//! Notifications as the shell keeps them: posted, replaced, shown in the
//! bar's line for a moment, then filed under their app in the Deskbar
//! until read or dismissed. Critical ones stay as a card under the bar
//! until acted on.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How urgent a notification is (the spec's `urgency` hint).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl Urgency {
    pub fn from_u8(v: u8) -> Urgency {
        match v {
            0 => Urgency::Low,
            2 => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

/// An image a notification carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NoteImage {
    File(PathBuf),
    /// Straight RGBA pixels (from `image-data`).
    Pixels {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

/// The most pixels a notification's image keeps on its longer side: a
/// thumbnail 64pt on a 2× screen, as much as anything shows of it. Apps
/// send whole screenshots as `image-data`; kept whole, one is megabytes
/// the shell would write out (`carry.rs`) whenever notifications change.
pub const IMAGE_PX: u32 = 128;

impl NoteImage {
    /// The image no larger than `max` pixels on its longer side, its
    /// pixels averaged down (weighted by their alpha) when it's larger.
    pub fn fit(self, max: u32) -> NoteImage {
        let NoteImage::Pixels { width, height, rgba } = self else { return self };
        let long = width.max(height);
        if long <= max || max == 0 || rgba.len() != width as usize * height as usize * 4 {
            return NoteImage::Pixels { width, height, rgba };
        }
        let scale = |n: u32| ((u64::from(n) * u64::from(max)).div_ceil(u64::from(long)) as u32).max(1);
        let (w, h) = (scale(width), scale(height));
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            let (y0, y1) = (y * height / h, ((y + 1) * height).div_ceil(h).min(height));
            for x in 0..w {
                let (x0, x1) = (x * width / w, ((x + 1) * width).div_ceil(w).min(width));
                let mut sum = [0u64; 4];
                for sy in y0..y1 {
                    let row = sy as usize * width as usize;
                    for sx in x0..x1 {
                        let p = &rgba[(row + sx as usize) * 4..][..4];
                        let a = u64::from(p[3]);
                        for c in 0..3 {
                            sum[c] += u64::from(p[c]) * a;
                        }
                        sum[3] += a;
                    }
                }
                let n = u64::from((y1 - y0) * (x1 - x0)).max(1);
                let a = sum[3];
                let colour = |c: usize| (sum[c] + a / 2).checked_div(a).unwrap_or(0) as u8;
                out.extend_from_slice(&[colour(0), colour(1), colour(2), ((a + n / 2) / n) as u8]);
            }
        }
        NoteImage::Pixels { width: w, height: h, rgba: out }
    }
}

/// A sound a notification asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum NoteSound {
    File(PathBuf),
    /// A name in the freedesktop sound theme (`message-new-instant`).
    Named(String),
}

/// What an app asked to show (`Notify`'s arguments).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Incoming {
    pub app_name: String,
    /// Replace the notification with this id (0: a new one).
    pub replaces_id: u32,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    /// Pairs of action key and label.
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    /// The `desktop-entry` hint: the app's desktop entry id.
    pub desktop_entry: Option<String>,
    pub image: Option<NoteImage>,
    pub category: Option<String>,
    /// Milliseconds; -1 the server's choice, 0 never.
    pub expire_timeout: i32,
    pub transient: bool,
    pub resident: bool,
    /// The sender's process, when known.
    pub pid: Option<u32>,
    /// The sound it asks for (`sound-file`, `sound-name`; none with
    /// `suppress-sound`).
    pub sound: Option<NoteSound>,
}

/// A notification the shell holds (carried across a restart, `carry.rs`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    /// The Deskbar app it's filed under (an app id), or its app name.
    pub filed_under: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub image: Option<NoteImage>,
    pub category: Option<String>,
    pub transient: bool,
    pub resident: bool,
    /// Seconds since the epoch it came (or was last replaced).
    pub at: u64,
    pub read: bool,
}

impl Notification {
    /// The default action's key, if it has one.
    pub fn default_action(&self) -> Option<&str> {
        self.actions.iter().find(|(k, _)| k == "default").map(|(k, _)| k.as_str())
    }

    /// Actions shown as buttons (all but `default`).
    pub fn buttons(&self) -> impl Iterator<Item = &(String, String)> {
        self.actions.iter().filter(|(k, _)| k != "default")
    }

    /// The body's first line, for the bar.
    pub fn first_line(&self) -> &str {
        self.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()
    }
}

/// Why a notification closed (the spec's `NotificationClosed` reasons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
    Undefined = 4,
}

/// What posting did, for the shell to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Posted {
    pub id: u32,
    pub replaced: bool,
    /// Show it in the bar's line now.
    pub banner: bool,
    /// Keep it as a card under the bar until acted on.
    pub card: bool,
}

/// Every notification the shell holds, oldest first.
#[derive(Clone, Debug, Default)]
pub struct Store {
    pub list: Vec<Notification>,
    next_id: u32,
}

impl Store {
    /// Take a notification in. `filed_under` is where the Deskbar files it;
    /// `quiet` is Do Not Disturb (or the app is muted): filed without the
    /// bar's line. Critical ones show anyway.
    pub fn post(&mut self, n: Incoming, filed_under: String, quiet: bool, now: u64) -> Posted {
        let id = if n.replaces_id != 0 && self.list.iter().any(|x| x.id == n.replaces_id) {
            n.replaces_id
        } else {
            self.fresh_id()
        };
        self.post_as(id, n, filed_under, quiet, now)
    }

    /// The last id given.
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// Starts from notifications a previous shell held.
    pub fn restore(&mut self, list: Vec<Notification>, next_id: u32) {
        self.next_id = list.iter().map(|n| n.id).max().unwrap_or(0).max(next_id);
        self.list = list;
    }

    /// Takes in notifications a previous shell got while this one started
    /// (those it doesn't have), keeping the order they came in.
    pub fn merge(&mut self, more: Vec<Notification>) -> usize {
        let mut added = 0;
        for n in more {
            if !self.list.iter().any(|x| x.id == n.id) {
                self.next_id = self.next_id.max(n.id);
                self.list.push(n);
                added += 1;
            }
        }
        self.list.sort_by_key(|n| (n.at, n.id));
        added
    }

    /// An id no notification has.
    pub fn fresh_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        while self.list.iter().any(|x| x.id == self.next_id) {
            self.next_id = self.next_id.wrapping_add(1).max(1);
        }
        self.next_id
    }

    /// Take a notification in under `id` (the server gave it), replacing
    /// the one with that id if there is one.
    pub fn post_as(&mut self, id: u32, n: Incoming, filed_under: String, quiet: bool, now: u64) -> Posted {
        let replaced = self.list.iter().any(|x| x.id == id);
        let note = Notification {
            id,
            app_name: n.app_name,
            app_icon: n.app_icon,
            filed_under,
            summary: n.summary,
            body: strip_markup(&n.body),
            actions: n.actions,
            urgency: n.urgency,
            image: n.image,
            category: n.category,
            transient: n.transient,
            resident: n.resident,
            at: now,
            read: false,
        };
        let critical = note.urgency == Urgency::Critical;
        if replaced {
            if let Some(slot) = self.list.iter_mut().find(|x| x.id == id) {
                *slot = note;
            }
        } else {
            self.list.push(note);
        }
        // Keep the list from growing without end: the oldest read ones go.
        while self.list.len() > 200 {
            match self.list.iter().position(|x| x.read).or(Some(0)) {
                Some(i) => {
                    self.list.remove(i);
                }
                None => break,
            }
        }
        self.next_id = self.next_id.max(id);
        Posted { id, replaced, banner: critical || !quiet, card: critical }
    }

    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.list.iter().find(|n| n.id == id)
    }

    /// Take a notification away: whether it was there.
    pub fn close(&mut self, id: u32) -> bool {
        let before = self.list.len();
        self.list.retain(|n| n.id != id);
        self.list.len() != before
    }

    /// A transient notification goes once its line has shown.
    pub fn banner_done(&mut self, id: u32) -> bool {
        if self.get(id).is_some_and(|n| n.transient && n.urgency != Urgency::Critical) {
            return self.close(id);
        }
        false
    }

    /// Unread notifications filed under `app`, newest first.
    pub fn unread(&self, app: &str) -> Vec<&Notification> {
        self.list.iter().rev().filter(|n| !n.read && n.filed_under == app).collect()
    }

    /// Every app with unread notifications, by the newest's age.
    pub fn apps_with_unread(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in self.list.iter().rev() {
            if !n.read && !out.contains(&n.filed_under) {
                out.push(n.filed_under.clone());
            }
        }
        out
    }

    /// Unread notifications that came at `at` or since, newest first.
    pub fn unread_since(&self, at: u64) -> Vec<&Notification> {
        self.list.iter().rev().filter(|n| !n.read && n.at >= at).collect()
    }

    pub fn unread_count(&self) -> usize {
        self.list.iter().filter(|n| !n.read).count()
    }

    /// Mark `app`'s notifications read (its list was opened).
    pub fn mark_read(&mut self, app: &str) {
        for n in &mut self.list {
            if n.filed_under == app {
                n.read = true;
            }
        }
    }

    /// The critical ones not yet acted on, oldest first: the cards under
    /// the bar.
    pub fn cards(&self) -> Vec<&Notification> {
        self.list.iter().filter(|n| n.urgency == Urgency::Critical && !n.read).collect()
    }
}

/// The body's text without its markup (`<b>`, `<i>`, `<u>`, `<a>`, `<img>`)
/// and with its entities read.
pub fn strip_markup(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        match rest[i..].find('>') {
            Some(j) => {
                let tag = &rest[i + 1..i + j];
                if tag.eq_ignore_ascii_case("br") || tag.eq_ignore_ascii_case("br/") {
                    out.push('\n');
                }
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_kept_as_thumbnails() {
        // A 2000 × 1000 screenshot: half opaque red, half see-through.
        let (w, h) = (2000u32, 1000u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(if x < w / 2 { &[200, 0, 0, 255] } else { &[0, 0, 255, 0] });
            }
        }
        let NoteImage::Pixels { width, height, rgba } = NoteImage::Pixels { width: w, height: h, rgba }.fit(IMAGE_PX)
        else {
            panic!("pixels stay pixels")
        };
        assert_eq!((width, height), (128, 64), "the longer side at most 128, in proportion");
        assert_eq!(rgba.len(), 128 * 64 * 4);
        assert_eq!(&rgba[..4], [200, 0, 0, 255]);
        // The see-through half's colour doesn't bleed in; at the seam, half.
        assert_eq!(&rgba[127 * 4..128 * 4], [0, 0, 0, 0]);
        let small = NoteImage::Pixels { width: 2, height: 1, rgba: vec![1, 2, 3, 4, 5, 6, 7, 8] };
        assert_eq!(small.clone().fit(IMAGE_PX), small, "a small one is left as it is");
        let file = NoteImage::File("/tmp/a.png".into());
        assert_eq!(file.clone().fit(IMAGE_PX), file);
    }

    fn note(app: &str, summary: &str) -> Incoming {
        Incoming {
            app_name: app.into(),
            summary: summary.into(),
            body: "line one\nline two".into(),
            ..Default::default()
        }
    }

    #[test]
    fn posting_files_under_the_app_and_shows_the_line() {
        let mut s = Store::default();
        let p = s.post(note("Mail", "Re: tiles"), "thunderbird".into(), false, 10);
        assert!(p.banner && !p.card && !p.replaced);
        assert_eq!(s.unread("thunderbird").len(), 1);
        assert_eq!(s.get(p.id).unwrap().first_line(), "line one");
        let q = s.post(note("Mail", "Second"), "thunderbird".into(), true, 11);
        assert!(!q.banner, "Do Not Disturb files it without the line");
        assert_eq!(s.unread("thunderbird")[0].summary, "Second");
        assert_eq!(s.apps_with_unread(), ["thunderbird"]);
        s.mark_read("thunderbird");
        assert_eq!(s.unread_count(), 0);
    }

    #[test]
    fn replacing_keeps_the_id() {
        let mut s = Store::default();
        let a = s.post(note("Build", "Compiling"), "foot".into(), false, 0);
        let b = s.post(Incoming { replaces_id: a.id, ..note("Build", "Done") }, "foot".into(), false, 1);
        assert!(b.replaced);
        assert_eq!(a.id, b.id);
        assert_eq!(s.list.len(), 1);
        assert_eq!(s.list[0].summary, "Done");
        // Replacing one that's gone makes a new one.
        s.close(a.id);
        let c = s.post(Incoming { replaces_id: a.id, ..note("Build", "Again") }, "foot".into(), false, 2);
        assert!(!c.replaced);
    }

    #[test]
    fn critical_ones_stay_as_cards_even_in_dnd() {
        let mut s = Store::default();
        let p =
            s.post(Incoming { urgency: Urgency::Critical, ..note("Power", "Battery low") }, "power".into(), true, 0);
        assert!(p.banner && p.card);
        assert_eq!(s.cards().len(), 1);
        assert!(!s.banner_done(p.id));
    }

    #[test]
    fn transient_ones_go_after_their_line() {
        let mut s = Store::default();
        let p = s.post(Incoming { transient: true, ..note("Vol", "50%") }, "x".into(), false, 0);
        assert!(s.banner_done(p.id));
        assert!(s.get(p.id).is_none());
    }

    #[test]
    fn markup_is_stripped() {
        assert_eq!(strip_markup("<b>Bold</b> &amp; <i>it</i><br>next"), "Bold & it\nnext");
        assert_eq!(strip_markup("a < b"), "a < b");
        assert_eq!(strip_markup("<a href=\"x\">link</a>"), "link");
    }

    #[test]
    fn actions_split_default_from_buttons() {
        let mut s = Store::default();
        let p = s.post(
            Incoming {
                actions: vec![("default".into(), "Open".into()), ("reply".into(), "Reply".into())],
                ..note("Mail", "x")
            },
            "m".into(),
            false,
            0,
        );
        let n = s.get(p.id).unwrap();
        assert_eq!(n.default_action(), Some("default"));
        assert_eq!(n.buttons().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["reply"]);
    }
}
