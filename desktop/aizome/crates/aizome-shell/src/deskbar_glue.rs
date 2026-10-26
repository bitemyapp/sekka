//! The Deskbar's part of the shell: its model from the window model, the
//! notifications and the Shelf; its surface; what its rows do.

use aizome_ipc::{Request, WindowId};
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

/// Transient visibility never writes the user's persistent fold preference.
#[derive(Default)]
pub(crate) struct Visibility {
    pub revealed: bool,
    pub inside: bool,
    pub control_inside: bool,
    pub menu: Option<u64>,
    pub dragging: Option<DragTarget>,
    /// The platform owns this drag until drop or cancellation, even if its
    /// Shelf row disappears or stops receiving pointer events.
    pub native_drag: Option<DragId>,
    pub generation: u64,
}

pub(crate) enum DragTarget {
    App(String),
    Window(WindowId),
}

impl Shell {
    pub fn deskbar_visible(&self) -> bool {
        !self.deskbar_cfg.collapsed && (!self.deskbar_cfg.auto_hide || self.deskbar_visibility.revealed)
    }

    pub(crate) fn deskbar_hover(&mut self, control: bool, inside: bool, cx: &mut Cx<Msg>) {
        let v = &mut self.deskbar_visibility;
        if control {
            v.control_inside = inside;
        } else {
            v.inside = inside;
        }
        v.generation = v.generation.wrapping_add(1);
        if self.deskbar_cfg.auto_hide && !self.deskbar_cfg.collapsed {
            if inside {
                v.revealed = true;
            } else {
                self.arm_deskbar_hide(cx);
            }
        }
    }

    pub(crate) fn arm_deskbar_hide(&mut self, cx: &mut Cx<Msg>) {
        let v = &mut self.deskbar_visibility;
        if self.deskbar_cfg.auto_hide && v.revealed && !v.inside && !v.control_inside && v.menu.is_none() {
            v.generation = v.generation.wrapping_add(1);
            cx.after(std::time::Duration::from_millis(350), Msg::Deskbar(DeskbarMsg::AutoHide(v.generation)));
        }
    }

    fn deskbar_menu_style(&mut self) -> MenuStyle<Msg> {
        self.serial = self.serial.wrapping_add(1);
        self.deskbar_visibility.menu = Some(self.serial);
        MenuStyle::at_point().on_closed(Msg::Deskbar(DeskbarMsg::MenuClosed(self.serial)))
    }

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
        // Unlisted apps retain their normal pinned/launch order; explicit
        // ordering never manufactures a row for an app that has gone away.
        apps.sort_by_key(|a| self.deskbar_cfg.order.iter().position(|id| id == &a.app_id).unwrap_or(usize::MAX));
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
        DeskbarModel {
            apps,
            shelf,
            hide_shelf: !self.deskbar_cfg.show_shelf,
            max_height: self.primary().map(|s| (s.output.frame.height() - crate::menubar::HEIGHT).max(1.0)),
        }
    }

    /// Open, size or close the Deskbar to match the settings and its
    /// content.
    pub fn sync_deskbar(&mut self, cx: &mut Cx<Msg>) {
        // A row removed during a drag cannot deliver its release. Drop its
        // visibility hold when the source disappears from the current view.
        if let Some(drag) = &self.deskbar_visibility.dragging {
            let model = self.deskbar_model();
            let present = match drag {
                DragTarget::App(id) => model.apps.iter().any(|a| a.app_id == *id),
                DragTarget::Window(id) => model.apps.iter().any(|a| !a.folded && a.windows.iter().any(|w| w.id == *id)),
            };
            if !present {
                self.deskbar_visibility.dragging = None;
                self.deskbar_drag = None;
                self.arm_deskbar_hide(cx);
            }
        }
        if self.deskbar_transient != self.deskbar_cfg.auto_hide
            && let Some(k) = self.deskbar.take()
        {
            cx.close_window(k);
            self.surfaces.remove(&k);
            self.deskbar_size = None;
            self.deskbar_visibility.inside = false;
            self.deskbar_visibility.dragging = None;
            self.deskbar_visibility.native_drag = None;
            self.deskbar_visibility.menu = None;
            self.deskbar_drag = None;
        }
        let want = self.deskbar_visible() && !self.screens.is_empty();
        match (want, self.deskbar) {
            (false, Some(k)) => {
                cx.close_window(k);
                self.surfaces.remove(&k);
                self.deskbar = None;
                self.deskbar_size = None;
                self.deskbar_visibility.inside = false;
                self.deskbar_visibility.menu = None;
                self.deskbar_visibility.dragging = None;
                self.deskbar_visibility.native_drag = None;
                self.deskbar_drag = None;
            }
            (true, None) => {
                let Some(output) = self.primary().map(|s| s.output.id) else { return };
                let model = self.deskbar_model();
                let size = crate::deskbar::size(&model);
                // A transient reveal overlays apps; only a persistent Deskbar
                // participates in the compositor's automatic window layout.
                let namespace = if self.deskbar_cfg.auto_hide { "aizome-deskbar-transient" } else { "aizome-deskbar" };
                let opts = LayerOptions::new(Layer::Top, namespace)
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
                self.deskbar_transient = self.deskbar_cfg.auto_hide;
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
            DeskbarMsg::FocusApp(app_id) => self.run(Action::FocusApp { app_id }, cx),
            DeskbarMsg::AppMenu(app_id, at) => {
                let Some(k) = self.deskbar else { return };
                let running = self.wm.app_window_to_activate(&app_id, self.apps.as_deref()).is_some();
                let pinned = self.deskbar_cfg.pinned.contains(&app_id);
                let apps = self.deskbar_model().apps;
                let position = apps.iter().position(|a| a.app_id == app_id).unwrap_or(0);
                let menu = ShellMenu::new(
                    self.app_look(&app_id).0,
                    vec![
                        Entry::action("New Window", Action::NewWindow { app_id: app_id.clone() }),
                        Entry::action("Hide", Action::HideApp { app_id: app_id.clone() }).enabled(running),
                        Entry::action("Quit", Action::QuitApp { app_id: app_id.clone() }).enabled(running),
                        Entry::separator(),
                        Entry::action("Keep in Deskbar", Action::Pin { app_id: app_id.clone(), on: !pinned })
                            .checked(pinned),
                        Entry::separator(),
                        Entry::action("Move Up", Action::MoveDeskbarApp { app_id: app_id.clone(), down: false })
                            .enabled(position > 0),
                        Entry::action("Move Down", Action::MoveDeskbarApp { app_id: app_id.clone(), down: true })
                            .enabled(position + 1 < apps.len()),
                    ],
                );
                let style = self.deskbar_menu_style();
                self.show_menu(k, at_point(at), &menu, style, cx);
            }
            DeskbarMsg::AppDrag(app, center, phase) => match phase {
                DragPhase::Start(at) => {
                    self.deskbar_visibility.dragging = Some(DragTarget::App(app.clone()));
                    self.deskbar_drag = Some((app, at));
                }
                DragPhase::Move(_) => {}
                DragPhase::End(at) => {
                    self.deskbar_visibility.dragging = None;
                    if let Some((source, from)) = self.deskbar_drag.take()
                        && source == app
                        && (0.0..crate::deskbar::WIDTH).contains(&at.x)
                    {
                        let model = self.deskbar_model();
                        let before = model.insertion_at(center + at.y - from.y);
                        self.reorder_deskbar_app(&app, before);
                    }
                    self.arm_deskbar_hide(cx);
                }
            },
            DeskbarMsg::Fold(app_id) => {
                let model_folded =
                    self.deskbar_model().apps.iter().find(|a| a.app_id == app_id).map(|a| a.folded).unwrap_or(false);
                self.folded.insert(app_id, !model_folded);
            }
            DeskbarMsg::FocusWindow(id) => self.ipc.send(Request::Focus { id }),
            DeskbarMsg::CloseWindow(id) => self.ipc.send(Request::Close { id: Some(id) }),
            DeskbarMsg::WindowDrag(id, phase) => {
                if matches!(phase, DragPhase::Start(_)) {
                    self.deskbar_visibility.dragging = Some(DragTarget::Window(id));
                }
                if matches!(phase, DragPhase::End(_)) {
                    self.deskbar_visibility.dragging = None;
                    self.arm_deskbar_hide(cx);
                }
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
                let style = self.deskbar_menu_style();
                self.show_menu(k, at_point(at), &menu, style, cx);
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
            DeskbarMsg::AutoHide(generation) => {
                let v = &mut self.deskbar_visibility;
                if self.deskbar_cfg.auto_hide
                    && generation == v.generation
                    && !v.inside
                    && !v.control_inside
                    && v.menu.is_none()
                    && v.dragging.is_none()
                    && v.native_drag.is_none()
                {
                    v.revealed = false;
                }
            }
            DeskbarMsg::MenuClosed(serial) => {
                if self.deskbar_visibility.menu == Some(serial) {
                    self.deskbar_visibility.menu = None;
                    self.arm_deskbar_hide(cx);
                }
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

    pub fn move_deskbar_app(&mut self, app: &str, down: bool) {
        let apps = self.deskbar_model().apps;
        let Some(i) = apps.iter().position(|a| a.app_id == app) else { return };
        let before = if down { (i + 2).min(apps.len()) } else { i.saturating_sub(1) };
        self.reorder_deskbar_app(app, before);
    }

    fn reorder_deskbar_app(&mut self, app: &str, before: usize) {
        let mut ids: Vec<String> = self.deskbar_model().apps.into_iter().map(|a| a.app_id).collect();
        let Some(i) = ids.iter().position(|id| id == app) else { return };
        let before = before.min(ids.len());
        let at = before.saturating_sub(usize::from(i < before));
        if at == i {
            return;
        }
        let id = ids.remove(i);
        ids.insert(at, id);
        // Keep preferences for absent apps, ready for their next launch.
        for id in &self.deskbar_cfg.order {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        self.deskbar_cfg.order = ids;
        self.save_section("deskbar");
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
