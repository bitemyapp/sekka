//! Torn-off palettes: a menu at the pointer (or the Toolchest) dragged by
//! its title strip stays as a palette — a layer surface placed with
//! margins, moved by dragging its strip, with a close box and a pin.
//! Palettes are per app and show only while their app has the focus,
//! unless pinned; they're remembered across sessions.

use std::path::PathBuf;

use aizome_kit::element::Element;
use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::text::TextSpec;
use aizome_kit::theme::FontSpec;
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions, Margins, menu_view};
use serde::{Deserialize, Serialize};

use crate::menus::build;
use crate::menus::model::{Kind, Menu as ShellMenu};
use crate::shell::{Msg, Shell, Surface, layer};
use crate::widgets::drawn::{DragPhase, drawn};

/// The Toolchest's palette, which shows whatever has the focus.
pub const TOOLCHEST: &str = "toolchest";
/// Aizome's own menus (no window focused).
pub const DESKTOP: &str = "aizome";

/// A palette, as remembered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TearOff {
    /// Whose menus: an app id, [`TOOLCHEST`] or [`DESKTOP`].
    pub app: String,
    /// The menus' titles down to it ([] for the app's whole menu).
    pub path: Vec<String>,
    /// Its output (connector) and top left there.
    pub output: String,
    pub x: f64,
    pub y: f64,
    pub pinned: bool,
}

/// A palette's surface while it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub window: WindowKey,
    pub size: Size,
    /// Where a strip drag started, in the window, and where the palette
    /// was then.
    pub grab: Option<(Point, Point)>,
}

/// What a palette's strip and buttons say.
#[derive(Clone, Debug, PartialEq)]
pub enum TearMsg {
    Close(usize),
    Pin(usize, bool),
    Drag(usize, DragPhase),
}

pub fn store_path() -> PathBuf {
    aizome_config::state_dir().join("palettes.json")
}

pub fn load(path: &std::path::Path) -> Vec<TearOff> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(path: &std::path::Path, list: &[TearOff]) {
    if let Ok(b) = serde_json::to_vec_pretty(list) {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(path, b);
    }
}

/// A palette's size for a menu: as the kit's menu lays one out (its strip,
/// rows of 24pt, separators of 9, its edges and shadow).
pub fn size(menu: &ShellMenu, measure: &mut dyn FnMut(&str, &TextSpec) -> f64, theme: &aizome_kit::Theme) -> Size {
    let m = &theme.metrics;
    let label = TextSpec::new(FontSpec::ui(m.body), theme.text());
    let shortcut = TextSpec::new(FontSpec::mono(m.shortcut), theme.text3());
    let mut strip_font = FontSpec::ui(12.5 * m.factor);
    strip_font.weight = aizome_theme::typeface::WEIGHT_BOLD;
    strip_font.condensed = true;
    let strip = TextSpec::new(strip_font, theme.on_menu_title());
    let mut widest = measure(&menu.title, &strip) + 56.0;
    let mut h = 1.0 + 22.0 + 4.0;
    let mut gutter = 0.0;
    for e in &menu.items {
        let (rh, w) = match e.kind {
            Kind::Action => {
                if e.checked || e.icon.is_some() {
                    gutter = 18.0;
                }
                let sw = e.shortcut.as_ref().map_or(0.0, |s| measure(&s.label(), &shortcut) + 24.0);
                (m.menu_row, measure(&e.label, &label) + sw)
            }
            Kind::Submenu => (m.menu_row, measure(&e.label, &label) + 16.0 + 12.0),
            Kind::Separator => (9.0, 0.0),
            Kind::Header => (22.0, 0.0),
        };
        widest = widest.max(w);
        h += rh;
    }
    h += 4.0 + 1.0;
    let width = (widest + 20.0 + gutter + 2.0).ceil().max(180.0);
    Size::new(width + m.shadow, (h + m.shadow).ceil())
}

/// A palette: the kit's menu drawn in place, under a hold for its strip.
pub fn view(
    index: usize,
    menu: aizome_kit::Menu<Msg>,
    title: &str,
    pinned: bool,
    size: Size,
    mark: bool,
) -> Element<Msg> {
    let mut menu = menu_view(menu);
    if mark {
        menu = menu.mark();
    }
    let menu = menu
        .title(title.to_owned())
        .on_close(Msg::Tear(TearMsg::Close(index)))
        .pin(pinned, move |p| Msg::Tear(TearMsg::Pin(index, p)));
    // The strip between the close box and the pin moves the palette.
    let shadow = aizome_theme::metrics::SHADOW;
    let hold = drawn((size.width - shadow - 54.0).max(10.0), 22.0, "hold", |_| {})
        .on_drag(move |d| Msg::Tear(TearMsg::Drag(index, d)))
        .cursor(aizome_kit::sidestep_ui::Cursor::Move);
    aizome_kit::stack(vec![
        Element::from(menu).width(size.width).height(size.height),
        aizome_kit::container(hold).padding([1.0, 0.0, 0.0, 26.0]).into(),
    ])
    .into()
}

impl Shell {
    /// The menu a palette shows now.
    pub fn tear_menu(&self, t: &TearOff) -> Option<(String, ShellMenu)> {
        if t.app == TOOLCHEST {
            let found = descend(self.toolchest_menu(), &t.path)?;
            let title = if t.path.is_empty() { "Toolchest".to_owned() } else { found.title.clone() };
            return Some((title, found));
        }
        let (name, menus) = if t.app == DESKTOP {
            ("Aizome".to_owned(), build::desktop(&self.wm.desks))
        } else {
            let conn = self.registry.apps().filter(|a| a.app_id == t.app).last();
            let name = crate::apps::catalog::display_name(self.apps.as_deref(), conn.map(|c| c.name.as_str()), &t.app);
            match conn {
                Some(c) if !c.menus.is_empty() => (name, build::app_menus(&c.menus, c.pid)),
                _ => {
                    let w = self.wm.windows.values().find(|w| w.app_id == t.app);
                    (name.clone(), build::fallback(&name, w, &self.wm.desks))
                }
            }
        };
        let hide = crate::actions::Action::Wm(aizome_ipc::Request::HideApp { app: crate::shell::app_sel(&t.app) });
        let quit = crate::actions::Action::QuitApp { app_id: t.app.clone() };
        let pointer = build::pointer_menu(&name, &menus, build::services(None), hide, quit);
        let Some(first) = t.path.first() else { return Some((name, pointer)) };
        // One of the app's menus (or a submenu down it); else one the menu
        // at the pointer has of its own (Services).
        let found = match menus.into_iter().find(|m| m.title == *first) {
            Some(m) => descend(m, &t.path[1..])?,
            None => descend(pointer, &t.path)?,
        };
        Some((found.title.clone(), found))
    }

    /// Whether a palette shows now: pinned, the Toolchest's, or its app has
    /// the focus.
    pub fn tear_visible(&self, t: &TearOff) -> bool {
        if t.pinned || t.app == TOOLCHEST {
            return true;
        }
        match self.wm.focused_window() {
            Some(w) => w.app_id == t.app,
            None => t.app == DESKTOP,
        }
    }

    /// Tear a menu off into a palette at a point of an output.
    pub fn tear_off(&mut self, app: String, path: Vec<String>, output: String, at: Point, cx: &mut Cx<Msg>) {
        self.tearoffs.retain(|t| !(t.app == app && t.path == path));
        self.tearoffs.push(TearOff {
            app,
            path,
            output,
            x: at.x.max(0.0),
            y: at.y.max(crate::menubar::HEIGHT),
            pinned: false,
        });
        self.save_tearoffs();
        self.sync_tearoffs(cx);
    }

    pub fn save_tearoffs(&self) {
        if self.opts.live {
            save(&store_path(), &self.tearoffs);
        }
    }

    /// Open the palettes that should show and close the rest; size them
    /// to their menus.
    pub fn sync_tearoffs(&mut self, cx: &mut Cx<Msg>) {
        let theme = cx.theme().clone();
        for i in 0..self.tearoffs.len() {
            let t = self.tearoffs[i].clone();
            let want = self.tear_visible(&t) && self.lock.phase == crate::lock::Phase::Unlocked;
            let menu = if want { self.tear_menu(&t) } else { None };
            match (menu, self.tear_shown.get(&i).cloned()) {
                (Some((_, menu)), None) => {
                    let Some(screen) = self.screen_named(&t.output).or(self.primary()).cloned() else { continue };
                    let text = &mut self.text;
                    let mut measure = |s: &str, spec: &TextSpec| text.layout(&theme, s, spec, None).size().width;
                    let size = size(&menu, &mut measure, &theme);
                    let opts = LayerOptions::new(Layer::Top, "aizome-menu")
                        .anchors(Anchors::TOP | Anchors::LEFT)
                        .exclusive_zone(-1)
                        .margins(Margins::new(t.y as i32, 0, 0, t.x as i32))
                        .keyboard(KeyboardInteractivity::OnDemand)
                        .output(screen.output.id);
                    let w = cx.open_window(layer(opts, size.width, size.height, "Palette (torn off)"));
                    self.surfaces.insert(w, Surface::TearOff(i));
                    self.tear_shown.insert(i, Shown { window: w, size, grab: None });
                }
                (Some((_, menu)), Some(shown)) => {
                    let text = &mut self.text;
                    let mut measure = |s: &str, spec: &TextSpec| text.layout(&theme, s, spec, None).size().width;
                    let size = size(&menu, &mut measure, &theme);
                    if size != shown.size {
                        cx.set_layer_size(shown.window, size);
                        if let Some(s) = self.tear_shown.get_mut(&i) {
                            s.size = size;
                        }
                    }
                }
                (None, Some(shown)) => {
                    cx.close_window(shown.window);
                    self.surfaces.remove(&shown.window);
                    self.tear_shown.remove(&i);
                }
                (None, None) => {}
            }
        }
    }

    pub fn tear_view(&self, i: usize) -> Element<Msg> {
        let (Some(t), Some(shown)) = (self.tearoffs.get(i), self.tear_shown.get(&i)) else {
            return aizome_kit::space(0.0);
        };
        let Some((title, menu)) = self.tear_menu(t) else { return aizome_kit::space(0.0) };
        let kit = build::to_kit(&menu, &Msg::Run);
        view(i, kit, &title, t.pinned, shown.size, t.app == TOOLCHEST && t.path.is_empty())
    }

    pub fn tear_msg(&mut self, m: TearMsg, cx: &mut Cx<Msg>) {
        match m {
            TearMsg::Close(i) => {
                if i < self.tearoffs.len() {
                    // Close every surface; indices shift.
                    for (_, s) in self.tear_shown.drain() {
                        cx.close_window(s.window);
                        self.surfaces.remove(&s.window);
                    }
                    self.tearoffs.remove(i);
                    self.save_tearoffs();
                    self.sync_tearoffs(cx);
                }
            }
            TearMsg::Pin(i, on) => {
                if let Some(t) = self.tearoffs.get_mut(i) {
                    t.pinned = on;
                    self.save_tearoffs();
                }
            }
            TearMsg::Drag(i, phase) => {
                let Some(t) = self.tearoffs.get(i) else { return };
                let at = Point::new(t.x, t.y);
                // The palette stays on its output, whole and below the bar
                // (a layer surface pushed past the output's edge would be
                // configured with a negative size).
                let output = self.screen_named(&t.output).or(self.primary()).map(|s| s.output.frame);
                let Some(shown) = self.tear_shown.get_mut(&i) else { return };
                match phase {
                    DragPhase::Start(p) => shown.grab = Some((p, at)),
                    DragPhase::Move(p) | DragPhase::End(p) => {
                        let Some((g, from)) = shown.grab else { return };
                        let (w, size) = (shown.window, shown.size);
                        // The pointer's place comes relative to where the
                        // palette was when the button went down (the press
                        // keeps its surface's place through the drag), so the
                        // palette goes where it was plus how far the pointer
                        // has come, not that again on every move.
                        let (max_x, max_y) = output.map_or((f64::MAX, f64::MAX), |o| {
                            ((o.width() - size.width).max(0.0), (o.height() - size.height).max(crate::menubar::HEIGHT))
                        });
                        if let Some(t) = self.tearoffs.get_mut(i) {
                            t.x = (from.x + p.x - g.x).clamp(0.0, max_x).round();
                            t.y = (from.y + p.y - g.y).clamp(crate::menubar::HEIGHT, max_y).round();
                            cx.set_layer_margins(w, Margins::new(t.y as i32, 0, 0, t.x as i32));
                        }
                        if let DragPhase::End(_) = phase {
                            if let Some(s) = self.tear_shown.get_mut(&i) {
                                s.grab = None;
                            }
                            self.save_tearoffs();
                        }
                    }
                }
            }
        }
    }
}

/// A palette's frame, to check its look apart from the kit's menu.
pub fn strip_rect(size: Size) -> Rect {
    Rect::new(1.0, 1.0, size.width - aizome_theme::metrics::SHADOW - 1.0, 23.0)
}

/// The submenu of `menu` down the titles `path` (`menu` itself for none).
pub fn descend(menu: ShellMenu, path: &[String]) -> Option<ShellMenu> {
    let mut found = menu;
    for title in path {
        let i = found.items.iter().position(|e| e.kind == Kind::Submenu && e.label == *title)?;
        found = found.at(&[i])?;
    }
    Some(found)
}

/// The titles down to the submenu at `path` (by index) of `menu`, as a
/// palette remembers its place. The menu at the pointer calls the app
/// menu Info; it's remembered by its own title, `app_menu`.
pub fn titles_at(menu: &ShellMenu, path: &[usize], app_menu: Option<&str>) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut items = &menu.items;
    for (n, &i) in path.iter().enumerate() {
        let e = items.get(i).filter(|e| e.kind == Kind::Submenu)?;
        let label = match app_menu {
            Some(a) if n == 0 && e.label == "Info" => a.to_owned(),
            _ => e.label.clone(),
        };
        out.push(label);
        items = &e.items;
    }
    Some(out)
}

/// What tearing off a submenu of `root` sends: a palette of `app`'s,
/// `prefix` the titles down to `root`, placed where the submenu was
/// (`root`'s top left on `output` is `at`).
pub fn tear_sub(
    app: String,
    root: ShellMenu,
    prefix: Vec<String>,
    app_menu: Option<String>,
    output: String,
    at: Point,
) -> impl Fn(Vec<usize>, aizome_kit::Vec2) -> Msg + 'static {
    move |path, off| {
        let mut titles = prefix.clone();
        titles.extend(titles_at(&root, &path, app_menu.as_deref()).unwrap_or_default());
        Msg::TearOff {
            app: app.clone(),
            path: titles,
            output: output.clone(),
            at: Point::new(at.x + off.x, at.y + off.y),
        }
    }
}
