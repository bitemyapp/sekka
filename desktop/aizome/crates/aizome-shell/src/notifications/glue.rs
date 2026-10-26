//! The notifications' part of the shell: what a click on a card or the
//! bar's line does.

use crate::shell::{Msg, Shell};
use aizome_kit::Cx;

impl Shell {
    /// A notification's card or line was clicked: its default action, and
    /// it's read.
    pub fn open_note(&mut self, id: u32, cx: &mut Cx<Msg>) {
        let Some(n) = self.notes.get(id).cloned() else { return };
        let focused = self.focus_app_window(&n.filed_under);
        if n.default_action().is_some() {
            self.note_action(id, "default");
        } else {
            // Without a default action, the notification itself is the
            // user's request to open its app. Never treat an arbitrary
            // sender name as a command to run.
            if !focused && self.apps.as_deref().is_some_and(|i| i.match_window_identity(&n.filed_under).is_some()) {
                self.run(crate::actions::Action::FocusApp { app_id: n.filed_under }, cx);
            }
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

    /// Prefer an explicit desktop identity, then the sender's process.
    /// Name-only notifications may use an unambiguous declared name, but
    /// display heuristics (such as reverse-DNS suffixes) aren't identity.
    pub fn file_under(&self, n: &crate::notifications::store::Incoming) -> String {
        let index = self.apps.as_deref();
        let canonical = |id: &str| {
            index.and_then(|i| i.match_window_identity(id)).map_or_else(|| id.to_owned(), |a| a.app_id().to_owned())
        };
        let file_for = |id: &str| {
            let entry = index.and_then(|i| i.match_window_identity(id));
            let window = self.wm.most_recent_window_matching(|w| match (index, entry) {
                (Some(index), Some(entry)) => index.match_window_identity(&w.app_id).is_some_and(|a| a.id == entry.id),
                _ => crate::apps::same_app_id(&w.app_id, id),
            });
            // The Deskbar groups windows by their actual compositor ID.
            // Use that row when present, retaining the installed identity
            // when there is no running window to receive the notification.
            window.map(|w| w.app_id.clone()).or_else(|| entry.map(|a| a.app_id().to_owned()))
        };
        let hint = n.desktop_entry.as_deref().map(str::trim).filter(|id| !id.is_empty());
        if let Some(filed) = hint.and_then(file_for) {
            return filed;
        }
        if let Some(pid) = n.pid.and_then(|pid| i32::try_from(pid).ok())
            && let Some(w) = self.wm.most_recent_window_matching(|w| w.pid == Some(pid))
        {
            return w.app_id.clone();
        }
        // Keep an unavailable app's declared identity for later activation;
        // never replace it with a different app that happens to look alike.
        if let Some(hint) = hint {
            return hint.strip_suffix(".desktop").unwrap_or(hint).to_owned();
        }
        if let Some(filed) = file_for(&n.app_name) {
            return filed;
        }
        // Some senders, including the shell CLI, only supply a human name.
        // Match names supplied by desktop entries or registered apps, never
        // names synthesized for display from a window ID.
        let name = n.app_name.to_lowercase();
        if !name.is_empty() {
            let mut matches: Vec<String> = index
                .into_iter()
                .flat_map(|i| i.all())
                .filter(|a| a.name.to_lowercase() == name)
                .map(|a| a.app_id().to_owned())
                .chain(self.registry.apps().filter(|a| a.name.to_lowercase() == name).map(|a| canonical(&a.app_id)))
                .collect();
            matches.sort();
            matches.dedup();
            if matches.len() == 1 {
                let app = matches.pop().unwrap();
                return file_for(&app).unwrap_or(app);
            }
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

#[cfg(test)]
mod activation_tests {
    use super::*;
    use crate::ipc::Ipc;
    use crate::notifications::{cards::CardMsg, store::Incoming};
    use crate::shell::Options;
    use aizome_ipc::{Event, Request, WindowInfo};
    use aizome_kit::testing::Harness;

    #[test]
    fn clicking_a_notification_raises_its_apps_recent_window() {
        for default_action in [false, true] {
            let mut h = Harness::new(Shell::new(Options::default()));
            let (ipc, requests) = Ipc::recording();
            let s = h.program_mut();
            s.ipc = ipc;
            s.wm.reset(crate::wm::Snapshot {
                current_desk: 1,
                windows: vec![
                    WindowInfo { id: 10, app_id: "mail".into(), desk: Some(2), ..Default::default() },
                    WindowInfo { id: 11, app_id: "mail".into(), desk: Some(2), minimised: true, ..Default::default() },
                ],
                ..Default::default()
            });
            s.wm.apply(&Event::WindowFocused { id: Some(11) });
            s.notes.post_as(
                7,
                Incoming {
                    app_name: "Mail".into(),
                    actions: if default_action { vec![("default".into(), "Open".into())] } else { vec![] },
                    ..Default::default()
                },
                "mail".into(),
                false,
                0,
            );
            h.send(Msg::Card(CardMsg::Open(7)));
            assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: 11 });
            assert!(h.program().notes.get(7).is_none());
        }
    }

    #[test]
    fn dismissing_a_notification_does_not_activate_its_app() {
        let mut h = Harness::new(Shell::new(Options::default()));
        let (ipc, requests) = Ipc::recording();
        h.program_mut().ipc = ipc;
        h.program_mut().notes.post_as(7, Incoming::default(), "mail".into(), false, 0);
        h.send(Msg::Card(CardMsg::Dismiss(7)));
        assert!(requests.try_recv().is_err());
        assert!(h.program().notes.get(7).is_none());
    }

    #[test]
    fn notification_activation_prefers_the_current_desk_over_a_more_recent_remote_window() {
        let mut h = Harness::new(Shell::new(Options::default()));
        let (ipc, requests) = Ipc::recording();
        let s = h.program_mut();
        s.ipc = ipc;
        s.wm.reset(crate::wm::Snapshot {
            current_desk: 1,
            windows: vec![
                WindowInfo { id: 10, app_id: "mail".into(), desk: Some(1), minimised: true, ..Default::default() },
                WindowInfo { id: 11, app_id: "mail".into(), desk: Some(2), ..Default::default() },
            ],
            focused: Some(11),
            ..Default::default()
        });
        s.notes.post_as(7, Incoming::default(), "mail".into(), false, 0);
        h.send(Msg::Card(CardMsg::Open(7)));
        assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: 10 });
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn opening_a_resident_notification_focuses_and_invokes_without_dismissing() {
        let mut h = Harness::new(Shell::new(Options::default()));
        let (ipc, requests) = Ipc::recording();
        let s = h.program_mut();
        s.ipc = ipc;
        s.wm.reset(crate::wm::Snapshot {
            windows: vec![WindowInfo {
                id: 11,
                app_id: "mail".into(),
                desk: Some(2),
                minimised: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        s.notes.post_as(
            7,
            Incoming {
                app_name: "Mail".into(),
                actions: vec![("default".into(), "Open".into())],
                resident: true,
                ..Default::default()
            },
            "mail".into(),
            false,
            0,
        );
        let action = (7, "default".into());
        s.note_actions.insert(action.clone(), crate::actions::Action::None);
        h.send(Msg::Card(CardMsg::Open(7)));
        assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: 11 });
        assert!(requests.try_recv().is_err());
        assert!(!h.program().note_actions.contains_key(&action), "the default action was invoked");
        assert!(h.program().notes.get(7).unwrap().resident, "invoking an action keeps a resident notification");

        h.send(Msg::Card(CardMsg::Dismiss(7)));
        assert!(requests.try_recv().is_err());
        assert!(h.program().notes.get(7).is_none(), "explicit dismissal still removes it");
    }

    #[test]
    fn desktop_hint_does_not_file_under_a_competing_reverse_dns_suffix() {
        let mut h = Harness::new(Shell::new(Options::default()));
        let (ipc, requests) = Ipc::recording();
        let s = h.program_mut();
        s.ipc = ipc;
        s.apps = Some(crate::apps::test_index(&[("com.spotify.Client", "")]));
        s.wm.reset(crate::wm::Snapshot {
            windows: vec![
                WindowInfo { id: 10, app_id: "org.other.Client".into(), ..Default::default() },
                WindowInfo { id: 11, app_id: "com.spotify.Client".into(), ..Default::default() },
            ],
            ..Default::default()
        });
        let incoming = Incoming { desktop_entry: Some("com.spotify.Client.desktop".into()), ..Default::default() };
        let filed = s.file_under(&incoming);
        assert_eq!(filed, "com.spotify.Client");
        s.notes.post_as(7, incoming, filed, false, 0);
        h.send(Msg::Card(CardMsg::Open(7)));
        assert_eq!(requests.try_recv().unwrap().0, Request::Focus { id: 11 });
    }

    #[test]
    fn filing_prefers_declared_identity_then_pid_and_preserves_unknown_hints() {
        let mut s = Shell::new(Options::default());
        s.apps = Some(crate::apps::test_index(&[
            ("com.spotify.Client", ""),
            ("org.editor.Suite", "StartupWMClass=EditorWindow"),
            ("org.closed.Editor", "StartupWMClass=ClosedEditor"),
        ]));
        s.wm.reset(crate::wm::Snapshot {
            windows: vec![
                WindowInfo { id: 10, app_id: "com.spotify.Client".into(), pid: Some(100), ..Default::default() },
                WindowInfo { id: 11, app_id: "EditorWindow".into(), pid: Some(200), ..Default::default() },
            ],
            ..Default::default()
        });
        for hint in ["org.editor.Suite.desktop", "EditorWindow"] {
            assert_eq!(
                s.file_under(&Incoming { desktop_entry: Some(hint.into()), pid: Some(100), ..Default::default() }),
                "EditorWindow",
                "a known desktop identity wins over the sender process"
            );
        }
        for hint in [None, Some("unknown-helper.desktop".into())] {
            assert_eq!(
                s.file_under(&Incoming { desktop_entry: hint, pid: Some(200), ..Default::default() }),
                "EditorWindow",
                "the sender process supplies a known window identity"
            );
        }
        assert_eq!(
            s.file_under(&Incoming { desktop_entry: Some("ClosedEditor".into()), ..Default::default() }),
            "org.closed.Editor",
            "an app without a window retains its canonical desktop identity"
        );
        let incoming = Incoming { desktop_entry: Some("org.editor.Suite".into()), ..Default::default() };
        let filed = s.file_under(&incoming);
        s.notes.post_as(7, incoming, filed, false, 0);
        let rows = s.deskbar_model().apps;
        assert_eq!(rows.iter().find(|row| row.app_id == "EditorWindow").unwrap().unread, 1);
        assert!(!rows.iter().any(|row| row.app_id == "org.editor.Suite"));
        assert_eq!(
            s.file_under(&Incoming {
                desktop_entry: Some("org.other.Client.desktop".into()),
                app_name: "com.spotify.Client".into(),
                ..Default::default()
            }),
            "org.other.Client"
        );
    }

    #[test]
    fn name_only_notifications_require_an_unambiguous_declared_name() {
        let mut s = Shell::new(Options::default());
        s.apps = Some(crate::apps::test_index(&[
            ("com.spotify.Client", "Name=Client"),
            ("org.other.Client", "Name=Client"),
        ]));
        s.wm.reset(crate::wm::Snapshot {
            windows: vec![
                WindowInfo { id: 10, app_id: "com.spotify.Client".into(), ..Default::default() },
                WindowInfo { id: 11, app_id: "org.other.Client".into(), ..Default::default() },
                WindowInfo { id: 12, app_id: "org.aizome.Settei".into(), ..Default::default() },
            ],
            ..Default::default()
        });
        s.registry.take(
            1,
            &aizome_shell_proto::AppMessage::Hello {
                app_id: "org.aizome.Settei".into(),
                pid: 123,
                name: "Settei".into(),
            },
        );
        assert_eq!(s.file_under(&Incoming { app_name: "Client".into(), ..Default::default() }), "Client");
        assert_eq!(s.file_under(&Incoming { app_name: "Settei".into(), ..Default::default() }), "org.aizome.Settei");
    }
}
