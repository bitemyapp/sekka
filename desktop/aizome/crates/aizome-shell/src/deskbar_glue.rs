//! The Deskbar's part of the shell: its model from the window model, the
//! notifications and the Shelf; its surface; what its rows do.

use aizome_ipc::Request;
use aizome_kit::prelude::*;
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions, Margins, MenuStyle};

use crate::actions::Action;
use crate::apps::catalog;
use crate::deskbar::{AppRow, DeskbarModel, DeskbarMsg, NoteCard, ShelfRow, WindowRow};
use crate::look::Hue;
use crate::menus::model::{Entry, Menu as ShellMenu};
use crate::shelf::ShelfItem;
use crate::shell::{Msg, Shell, Surface, at_point, layer};
use crate::widgets::drawn::DragPhase;

impl Shell {
    /// An app's name, glyph and colour, as the Deskbar shows it.
    pub fn app_look(&self, app_id: &str) -> (String, Icon, Hue) {
        let hello = self.registry.apps().find(|a| a.app_id == app_id).map(|a| a.name.clone());
        let name = catalog::display_name(self.apps.as_deref(), hello.as_deref(), app_id);
        let (glyph, hue) = catalog::glyph_for(app_id).unwrap_or_else(|| {
            let cats = self
                .apps
                .as_deref()
                .and_then(|i| i.match_window(app_id))
                .map(|a| a.categories.clone())
                .unwrap_or_default();
            (catalog::glyph_by_category(&cats), Hue::Text2)
        });
        (name, glyph, hue)
    }

    /// Whether an app's windows show folded under it.
    pub fn is_folded(&self, app_id: &str, windows: usize, focused: bool) -> bool {
        match self.folded.get(app_id) {
            Some(f) => *f,
            None => windows >= 3 && !focused,
        }
    }

    pub fn deskbar_model(&self) -> DeskbarModel {
        let mut apps: Vec<AppRow> = self
            .wm
            .deskbar(&self.deskbar_cfg.pinned)
            .into_iter()
            .map(|a| {
                let (name, glyph, hue) = self.app_look(&a.app_id);
                let folded = self.is_folded(&a.app_id, a.windows.len(), a.focused);
                AppRow {
                    image: self.icons.get(&a.app_id).cloned().flatten().map(crate::look::Pic),
                    name,
                    glyph,
                    hue,
                    folded,
                    focused: a.focused,
                    windows: a
                        .windows
                        .into_iter()
                        .map(|w| WindowRow {
                            id: w.id,
                            title: w.title,
                            focused: w.focused,
                            note: w.note,
                            urgent: w.urgent,
                        })
                        .collect(),
                    unread: 0,
                    cards: Vec::new(),
                    app_id: a.app_id,
                }
            })
            .collect();
        // Unread notifications under their app (apps not running get a row).
        for app in self.notes.apps_with_unread() {
            let unread = self.notes.unread(&app);
            if !apps.iter().any(|a| a.app_id == app) {
                let (name, glyph, hue) = self.app_look(&app);
                let name = unread.first().map(|n| n.app_name.clone()).filter(|n| !n.is_empty()).unwrap_or(name);
                apps.push(AppRow {
                    app_id: app.clone(),
                    name,
                    glyph,
                    hue,
                    image: None,
                    windows: vec![],
                    folded: true,
                    focused: false,
                    unread: 0,
                    cards: vec![],
                });
            }
            let row = apps.iter_mut().find(|a| a.app_id == app).expect("added");
            row.unread = unread.len();
            let all = self.notes_open.contains(&app);
            row.cards = unread
                .iter()
                .take(if all { 8 } else { 1 })
                .map(|n| NoteCard {
                    id: n.id,
                    summary: n.summary.clone(),
                    body: n.body.clone(),
                    buttons: n.buttons().cloned().collect(),
                })
                .collect();
        }
        let shelf = self
            .shelf
            .items
            .iter()
            .map(|i| {
                let (glyph, hue) = i.glyph();
                ShelfRow {
                    label: i.label(),
                    glyph,
                    hue,
                    dim: matches!(i, ShelfItem::Text { .. }),
                    uri: i.as_uri(&self.shelf.dir),
                    drag_text: i.as_text(&self.shelf.dir),
                }
            })
            .collect();
        DeskbarModel { apps, shelf, hide_shelf: !self.deskbar_cfg.show_shelf }
    }

    /// Open, size or close the Deskbar to match the settings and its
    /// content.
    pub fn sync_deskbar(&mut self, cx: &mut Cx<Msg>) {
        let want = !self.deskbar_cfg.collapsed && !self.screens.is_empty();
        match (want, self.deskbar) {
            (false, Some(k)) => {
                cx.close_window(k);
                self.surfaces.remove(&k);
                self.deskbar = None;
                self.deskbar_size = None;
            }
            (true, None) => {
                let Some(output) = self.primary().map(|s| s.output.id) else { return };
                let model = self.deskbar_model();
                let size = crate::deskbar::size(&model);
                let opts = LayerOptions::new(Layer::Top, "aizome-deskbar")
                    .anchors(Anchors::TOP | Anchors::RIGHT)
                    // Placed by its margin from the output's top, whatever others'
                    // zones (the bar's) say.
                    .exclusive_zone(-1)
                    .margins(Margins::new(crate::menubar::HEIGHT as i32, 0, 0, 0))
                    .keyboard(KeyboardInteractivity::OnDemand)
                    .output(output);
                let k = cx.open_window(layer(opts, size.width, size.height, "Deskbar"));
                cx.set_input_region(k, Some(crate::deskbar::input_region(&model)));
                self.surfaces.insert(k, Surface::Deskbar);
                self.deskbar = Some(k);
                self.deskbar_size = Some(size);
            }
            (true, Some(k)) => {
                let model = self.deskbar_model();
                let size = crate::deskbar::size(&model);
                if self.deskbar_size != Some(size) {
                    cx.set_layer_size(k, size);
                    cx.set_input_region(k, Some(crate::deskbar::input_region(&model)));
                    self.deskbar_size = Some(size);
                }
            }
            (false, None) => {}
        }
    }

    pub fn deskbar_msg(&mut self, m: DeskbarMsg, cx: &mut Cx<Msg>) {
        match m {
            DeskbarMsg::FocusApp(app_id) => {
                let hidden = self.wm.app_windows_by_recency(&app_id).iter().all(|w| w.hidden || w.minimised);
                match self.wm.app_windows_by_recency(&app_id).first() {
                    Some(w) if hidden => {
                        let id = w.id;
                        self.ipc.send(Request::ShowApp { app: crate::shell::app_sel(&app_id) });
                        self.ipc.send(Request::Focus { id });
                    }
                    Some(w) => self.ipc.send(Request::Focus { id: w.id }),
                    None => self.run(Action::Launch { app_id }, cx),
                }
            }
            DeskbarMsg::AppMenu(app_id, at) => {
                let Some(k) = self.deskbar else { return };
                let running = self.wm.is_running(&app_id);
                let pinned = self.deskbar_cfg.pinned.contains(&app_id);
                let menu = ShellMenu::new(
                    self.app_look(&app_id).0,
                    vec![
                        Entry::action("New Window", Action::NewWindow { app_id: app_id.clone() }),
                        Entry::action("Hide", Action::Wm(Request::HideApp { app: crate::shell::app_sel(&app_id) }))
                            .enabled(running),
                        Entry::action("Quit", Action::QuitApp { app_id: app_id.clone() }).enabled(running),
                        Entry::separator(),
                        Entry::action("Keep in Deskbar", Action::Pin { app_id: app_id.clone(), on: !pinned })
                            .checked(pinned),
                    ],
                );
                self.show_menu(k, at_point(at), &menu, MenuStyle::at_point(), cx);
            }
            DeskbarMsg::Fold(app_id) => {
                let model_folded =
                    self.deskbar_model().apps.iter().find(|a| a.app_id == app_id).map(|a| a.folded).unwrap_or(false);
                self.folded.insert(app_id, !model_folded);
            }
            DeskbarMsg::FocusWindow(id) => self.ipc.send(Request::Focus { id }),
            DeskbarMsg::CloseWindow(id) => self.ipc.send(Request::Close { id: Some(id) }),
            DeskbarMsg::WindowDrag(id, phase) => {
                if let DragPhase::End(p) = phase
                    && let Some(desk) = self.desk_at_deskbar_point(p)
                {
                    self.ipc.send(Request::MoveToDesk {
                        id: Some(id),
                        desk: aizome_ipc::DeskSel::Index(desk),
                        follow: false,
                    });
                }
            }
            DeskbarMsg::OpenNote(id) => self.open_note(id, cx),
            DeskbarMsg::DismissNote(id) => self.dismiss_note(id),
            DeskbarMsg::NoteAction(id, key) => self.note_action(id, &key),
            DeskbarMsg::ShowNotes(app) => {
                if let Some(i) = self.notes_open.iter().position(|a| *a == app) {
                    self.notes_open.remove(i);
                    self.notes.mark_read(&app);
                } else {
                    self.notes_open.push(app);
                }
            }
            DeskbarMsg::ShelfOpen(i) => {
                if let Some(item) = self.shelf.items.get(i).cloned() {
                    self.open_shelf_item(&item, cx);
                }
            }
            DeskbarMsg::ShelfMenu(i, at) => {
                let Some(k) = self.deskbar else { return };
                let Some(item) = self.shelf.items.get(i) else { return };
                let text = item.as_text(&self.shelf.dir);
                let open = match item.target(&self.shelf.dir) {
                    Some(path) => Action::Open { path },
                    None => match item {
                        ShelfItem::Link { url } => Action::OpenUrl { url: url.clone() },
                        _ => Action::None,
                    },
                };
                let menu = ShellMenu::new(
                    item.label(),
                    vec![
                        Entry::action("Open", open.clone()).enabled(open != Action::None),
                        Entry::action("Copy", Action::Copy { text }),
                        Entry::separator(),
                        Entry::action("Remove", Action::ShelfRemove { index: i }),
                    ],
                );
                self.show_menu(k, at_point(at), &menu, MenuStyle::at_point(), cx);
            }
            DeskbarMsg::Park(urls, text) => {
                for u in &urls {
                    if let Some(item) = ShelfItem::from_uri(u) {
                        self.shelf.park(item);
                    }
                }
                if urls.is_empty()
                    && let Some(t) = text
                {
                    self.shelf.park(ShelfItem::from_text(&t));
                }
                self.save_shelf();
            }
        }
    }

    pub fn save_shelf(&self) {
        if self.opts.live
            && let Err(e) = self.shelf.save()
        {
            log::warn!("the Shelf: {e}");
        }
    }

    fn open_shelf_item(&mut self, item: &ShelfItem, cx: &mut Cx<Msg>) {
        match item {
            ShelfItem::Link { url } => self.run(Action::OpenUrl { url: url.clone() }, cx),
            ShelfItem::Text { text } => self.run(Action::Copy { text: text.clone() }, cx),
            other => {
                if let Some(path) = other.target(&self.shelf.dir) {
                    self.run(Action::Open { path }, cx);
                }
            }
        }
    }

    /// The Desk in the bar's strip under a point of the Deskbar (a window
    /// dragged there).
    pub fn desk_at_deskbar_point(&self, p: Point) -> Option<u32> {
        let screen = self.primary()?;
        let w = screen.output.frame.width();
        let x = w - crate::deskbar::WIDTH - crate::deskbar::SHADOW + p.x;
        let y = crate::menubar::HEIGHT + p.y;
        if y < 0.0 || y >= crate::menubar::HEIGHT {
            return None;
        }
        self.desk_strip.iter().find(|(_, r)| r.x0 <= x && x < r.x1).map(|(i, _)| *i)
    }
}

impl Shell {
    /// Look up the icons of apps in the Deskbar the shell has no glyph
    /// for (once each, at 16pt on a 2× screen; SVGs drawn at that size).
    pub fn find_icons(&mut self) {
        let Some(lookup) = &self.icon_lookup else { return };
        let wanted: Vec<String> = self
            .wm
            .deskbar(&self.deskbar_cfg.pinned)
            .into_iter()
            .map(|a| a.app_id)
            .filter(|a| !self.icons.contains_key(a) && catalog::glyph_for(a).is_none())
            .collect();
        for app in wanted {
            let name = self
                .apps
                .as_deref()
                .and_then(|i| i.match_window(&app))
                .and_then(|e| e.icon.clone())
                .unwrap_or_else(|| app.clone());
            let image = lookup.lookup(&name, 16, 2).and_then(|p| crate::icons::read(&p, 16, 2));
            self.icons.insert(app, image);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_fold_by_default_from_three_windows() {
        let s = Shell::new(crate::shell::Options::default());
        assert!(s.is_folded("firefox", 3, false));
        assert!(!s.is_folded("firefox", 3, true));
        assert!(!s.is_folded("foot", 2, false));
    }
}
