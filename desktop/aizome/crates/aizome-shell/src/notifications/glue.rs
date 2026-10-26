//! The notifications' part of the shell: what a click on a card or the
//! bar's line does.

use crate::shell::{Msg, Shell};
use aizome_kit::Cx;

impl Shell {
    /// A notification's card or line was clicked: its default action, and
    /// it's read.
    pub fn open_note(&mut self, id: u32, _cx: &mut Cx<Msg>) {
        let Some(n) = self.notes.get(id).cloned() else { return };
        if n.default_action().is_some() {
            self.note_action(id, "default");
        } else {
            self.notes.close(id);
            self.note_closed(id, crate::notifications::store::CloseReason::Dismissed);
        }
    }

    pub fn dismiss_note(&mut self, id: u32) {
        if self.notes.close(id) {
            self.note_closed(id, crate::notifications::store::CloseReason::Dismissed);
        }
    }

    /// Invoke one of a notification's actions; it closes unless resident.
    pub fn note_action(&mut self, id: u32, key: &str) {
        let Some(n) = self.notes.get(id).cloned() else { return };
        if let Some(a) = self.note_actions.remove(&(id, key.to_owned())) {
            self.pending_actions.push(a);
        }
        self.note_invoked(id, key);
        if !n.resident {
            self.notes.close(id);
            self.note_closed(id, crate::notifications::store::CloseReason::Dismissed);
        }
    }

    /// Tell the app an action was invoked (the D-Bus signal).
    pub fn note_invoked(&mut self, id: u32, key: &str) {
        if let Some(s) = &self.note_server {
            s.action_invoked(id, key);
        }
    }

    /// Tell the app a notification closed.
    pub fn note_closed(&mut self, id: u32, reason: crate::notifications::store::CloseReason) {
        if let Some(s) = &self.note_server {
            s.closed(id, reason);
        }
    }
}

impl Shell {
    /// What the notifications server heard.
    pub fn take_note(&mut self, e: crate::notifications::server::NoteEvent, cx: &mut Cx<Msg>) {
        use crate::notifications::server::NoteEvent;
        match e {
            NoteEvent::Notify(id, n) => {
                let filed = self.file_under(&n);
                let names = [n.app_name.as_str(), filed.as_str(), n.desktop_entry.as_deref().unwrap_or("")];
                let cfg = &self.notes_cfg;
                // A blocked app's are dropped: the app hears it closed.
                if aizome_config::names_app(&cfg.blocked_apps, &names) {
                    self.note_closed(id, crate::notifications::store::CloseReason::Dismissed);
                    return;
                }
                let now = crate::clock::Local::now();
                let muted = aizome_config::names_app(&cfg.muted_apps, &names);
                let silent = aizome_config::names_app(&cfg.silent_apps, &names);
                let quiet = cfg.quiet_at(now.minutes()) || muted || self.locked();
                let sound = n.sound.clone().filter(|_| !quiet && !silent);
                let at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let posted = self.notes.post_as(id, *n, filed, quiet, at);
                if posted.banner {
                    self.show_banner(id, cx);
                }
                if let Some(s) = sound {
                    self.make_sound(&s);
                }
            }
            NoteEvent::Close(id) => {
                if self.notes.close(id) {
                    self.note_closed(id, crate::notifications::store::CloseReason::Closed);
                }
                self.banner_queue.retain(|b| *b != id);
                if self.banner == Some(id) {
                    self.banner_done(id, cx);
                }
            }
            // The shell's update loop handles these.
            NoteEvent::NameAcquired | NoteEvent::NameLost => {}
        }
    }

    /// Make a notification's sound, at the alert volume (in tests: note
    /// it).
    pub fn make_sound(&mut self, s: &crate::notifications::store::NoteSound) {
        use crate::notifications::store::NoteSound;
        let file = match s {
            NoteSound::File(p) => Some(p.clone()),
            NoteSound::Named(name) => crate::sound::theme_file(name),
        };
        let volume = self.sound_cfg.alert_level();
        self.play_sound(&format!("{s:?}"), file, volume);
    }

    /// Whether the screen is locked (notifications are only filed then).
    pub fn locked(&self) -> bool {
        self.wm.locked || self.lock.phase != crate::lock::Phase::Unlocked
    }

    /// The Deskbar app a notification is filed under: its desktop entry's
    /// app if that's running (or known), else a running app of its name,
    /// else its name.
    pub fn file_under(&self, n: &crate::notifications::store::Incoming) -> String {
        let running: Vec<String> = self.wm.deskbar(&self.deskbar_cfg.pinned).into_iter().map(|a| a.app_id).collect();
        if let Some(entry) = &n.desktop_entry {
            let entry = entry.trim_end_matches(".desktop");
            if let Some(a) = running.iter().find(|a| {
                a.eq_ignore_ascii_case(entry)
                    || a.rsplit('.')
                        .next()
                        .is_some_and(|l| l.eq_ignore_ascii_case(entry.rsplit('.').next().unwrap_or(entry)))
            }) {
                return a.clone();
            }
            return entry.to_owned();
        }
        if let Some(pid) = n.pid
            && let Some(w) = self.wm.windows.values().find(|w| w.pid == Some(pid as i32))
        {
            return w.app_id.clone();
        }
        let name = n.app_name.to_lowercase();
        if let Some(a) = running.iter().find(|a| self.app_look(a).0.to_lowercase() == name || a.to_lowercase() == name)
        {
            return a.clone();
        }
        if n.app_name.is_empty() { "notifications".into() } else { n.app_name.clone() }
    }

    /// Show a notification in the bar's line for its own few seconds (a
    /// newer one takes the line; none waits in line, so the bar goes back
    /// to the app's menus five seconds after the last one came).
    pub fn show_banner(&mut self, id: u32, cx: &mut Cx<Msg>) {
        if let Some(old) = self.banner.filter(|b| *b != id) {
            // The one it replaces has had its moment.
            if self.notes.banner_done(old) {
                self.note_closed(old, crate::notifications::store::CloseReason::Expired);
            }
        }
        self.banner = Some(id);
        let secs = self.notes_cfg.banner_seconds.max(1) as u64;
        cx.after(std::time::Duration::from_secs(secs), Msg::BannerDone(id));
    }

    /// The line has shown long enough: the next one, if any.
    pub fn banner_done(&mut self, id: u32, cx: &mut Cx<Msg>) {
        if self.banner != Some(id) {
            return;
        }
        self.banner = None;
        let _ = cx;
        if self.notes.banner_done(id) {
            self.note_closed(id, crate::notifications::store::CloseReason::Expired);
        }
    }

    /// The bar's line for the notification showing.
    pub fn banner_view(&self) -> Option<crate::menubar::Banner> {
        let n = self.notes.get(self.banner?)?;
        let (_, glyph, hue) = self.app_look(&n.filed_under);
        let glyph =
            if crate::apps::catalog::glyph_for(&n.filed_under).is_some() { glyph } else { aizome_kit::Icon::Bell };
        Some(crate::menubar::Banner {
            id: n.id,
            glyph,
            hue,
            summary: n.summary.clone(),
            body: n.first_line().to_owned(),
        })
    }
}

impl Shell {
    /// An id for a notification of the shell's own: from the same counter
    /// as the ones apps send over D-Bus, so neither replaces the other.
    pub fn fresh_note_id(&mut self) -> u32 {
        match &self.note_server {
            Some(s) => s.fresh_id(),
            None => self.notes.fresh_id(),
        }
    }

    /// A notification of the shell's own, its actions the shell's.
    pub fn shell_note(
        &mut self,
        n: crate::notifications::store::Incoming,
        actions: Vec<(String, String, crate::actions::Action)>,
        cx: &mut Cx<Msg>,
    ) -> u32 {
        let id = self.fresh_note_id();
        let mut n = n;
        n.actions = actions.iter().map(|(k, l, _)| (k.clone(), l.clone())).collect();
        for (k, _, a) in actions {
            self.note_actions.insert((id, k), a);
        }
        self.take_note(crate::notifications::server::NoteEvent::Notify(id, Box::new(n)), cx);
        id
    }

    /// The compositor took a screenshot: a notification with its thumbnail
    /// and "Show in Hako".
    pub fn screenshot_taken(&mut self, path: std::path::PathBuf, cx: &mut Cx<Msg>) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let n = crate::notifications::store::Incoming {
            app_name: "Screenshot".into(),
            summary: "Screenshot saved".into(),
            body: name,
            image: Some(crate::notifications::store::NoteImage::File(path.clone())),
            category: Some("transfer.complete".into()),
            ..Default::default()
        };
        let reveal = crate::actions::Action::Reveal { path: path.clone() };
        self.shell_note(
            n,
            vec![
                ("default".into(), "Show in Hako".into(), reveal.clone()),
                ("show".into(), "Show in Hako".into(), reveal),
            ],
            cx,
        );
    }
}
