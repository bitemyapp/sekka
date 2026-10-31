//! The command palette (Strip's, in Aizome's look): ❖Space opens it on the
//! focused output with every command of the focused app's menus, the
//! windows, files, apps, settings and actions, ranked as you type.

pub mod model;
pub mod view;

use std::path::PathBuf;
use std::sync::Arc;

use aizome_ipc::Request;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, KeyEvent, NamedKey};
use aizome_kit::{KeyboardInteractivity, Layer, LayerOptions, MenuStyle};
use aizome_search::Scope;
use aizome_search::files::FileIndex;

use crate::actions::{Action, SessionVerb};
use crate::menus::model::{Entry, Menu as ShellMenu, MenuIcon};
use crate::shell::{Msg, Shell, Surface, layer};
use model::{Item, Ranking, Section, SettingEntry};
use view::{Detail, PaletteMsg, PaletteView};

/// The palette while it's open.
#[derive(Clone, Debug)]
pub struct PaletteState {
    pub window: WindowKey,
    pub query: String,
    pub scope: Option<Scope>,
    pub selected: usize,
    pub scroll: f64,
    /// Every row but files', made when it opened.
    pub items: Vec<Item>,
    /// Files found for `files_for`.
    pub files: Vec<Item>,
    pub files_for: String,
    pub sections: Vec<Section>,
    /// "in Editor · damage.rs".
    pub context: String,
    pub app_name: Option<String>,
    pub app_id: String,
    pub search: u64,
    /// Recently used files (and whether each is a folder), read on a thread
    /// when it opened, and their frecency boosts.
    pub recent: Arc<Vec<(PathBuf, bool)>>,
    pub boosts: Arc<std::collections::HashMap<String, i64>>,
}

/// The palette's window between openings (from the warming two seconds
/// after start on): hidden, and drawn ahead as the palette would open
/// now, so that opening it shows a frame drawn already and draws only
/// what changed since (`Cx::prepare_window`). Drawn again a second after
/// what it opens with changes (the focused app, its menus, the windows,
/// apps, settings, what was picked, the theme, the output's scale), at
/// idle. Its pictures hold one surface's pixels, 5.6 MB at 2×.
#[derive(Default)]
pub struct Ahead {
    /// The window is kept and drawn ahead (from the first warming on).
    pub on: bool,
    /// The hidden window, and what it was last drawn with (while the
    /// palette is closed).
    pub spare: Option<PaletteState>,
    /// The output the window shows on (a layer surface stays on the output
    /// it's made for: on another, another window is made).
    pub output: u32,
    /// The scale it's drawn ahead at: none until it is again (once hidden).
    pub scale: Option<f64>,
    /// What the palette opens with ([`Shell::palette_inputs`]) when it was
    /// last drawn, or since asked to draw again for.
    pub inputs: u64,
    /// Counts the asks to draw it again: only the last one's timer does.
    pub asked: u64,
    /// Counts the changes to what it opens with that
    /// [`Shell::palette_inputs`] doesn't look into: apps' menus, picks.
    pub bumped: u64,
}

/// How long after what the palette opens with changes it's drawn ahead
/// again: changes come in bursts (a window opening, focus, a title).
pub const AHEAD_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// Where the palette shows: over everything on `output`, taking the
/// keyboard.
fn palette_layer(output: u32) -> LayerOptions {
    LayerOptions::new(Layer::Overlay, "aizome-palette").keyboard(KeyboardInteractivity::Exclusive).output(output)
}

/// The file index, read or built on a thread.
#[derive(Clone)]
pub struct Index(pub Arc<FileIndex>);

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Index({} entries)", self.0.len())
    }
}

/// What the palette hears from its elements and threads.
#[derive(Clone, Debug)]
pub enum PaletteEvent {
    Ui(PaletteMsg),
    /// ↑ ↓ (by rows), Tab and ⇧Tab, ❖K.
    Move(isize),
    Cycle(bool),
    Actions,
    /// Files found for search `seq`.
    Files(u64, Vec<Item>),
    Index(Index),
    /// Time to see whether the index is stale.
    Refresh,
    /// Recently used files, read on a thread.
    Recent(Vec<(PathBuf, bool)>),
    Settings(Vec<SettingEntry>),
    /// The history as written after recording a pick into its file.
    Frecency(Box<aizome_search::frecency::Frecency>),
}

/// The system actions the palette offers (with Stop Recording instead
/// of the Record ones while a recording goes).
pub fn actions(day: bool, dnd: bool, recording: bool, awake: bool) -> Vec<Item> {
    let a = |t: &str, g: Icon, action: Action, kw: &[&str]| {
        Item::new(Scope::Action, t, g, action)
            .keywords(kw.iter().map(|s| s.to_string()).collect())
            .key(format!("act:{t}"))
    };
    let mut out = vec![
        a("Lock", Icon::Lock, Action::Session(SessionVerb::Lock), &["screen", "away"]),
        a("Sleep", Icon::Moon, Action::Session(SessionVerb::Sleep), &["suspend"]),
        a("Restart…", Icon::RotateCw, Action::Session(SessionVerb::Restart), &["reboot"]),
        a("Shut Down…", Icon::Power, Action::Session(SessionVerb::ShutDown), &["power off", "turn off"]),
        a("Log Out…", Icon::LogOut, Action::Session(SessionVerb::LogOut), &["sign out", "quit session"]),
        if day {
            a("Switch to Night", Icon::Moon, Action::SetTheme { day: false }, &["dark", "theme"])
        } else {
            a("Switch to Day", Icon::Sun, Action::SetTheme { day: true }, &["light", "theme"])
        },
        a(
            if dnd { "Turn Off Do Not Disturb" } else { "Do Not Disturb" },
            Icon::BellOff,
            Action::ToggleDoNotDisturb,
            &["notifications", "quiet", "focus"],
        ),
        a(
            if awake { "Turn Off Keep Awake" } else { "Keep Awake" },
            Icon::Coffee,
            Action::ToggleKeepAwake,
            &["stay awake", "sleep", "caffeine", "lid", "suspend", "inhibit", "no sleep", "amphetamine"],
        ),
        a("Screenshot", Icon::Camera, Action::Screenshot, &["capture", "screen"]),
        a(
            "Reload Aizome",
            Icon::RotateCw,
            Action::ReloadAizome,
            &["restart shell", "reload compositor", "refresh", "update", "recover"],
        ),
        a("New Desk", Icon::Grid2x2, Action::NewDesk, &["workspace"]),
        a("Empty Trash", Icon::Trash, Action::EmptyTrash, &["delete"]),
        a("Show Desktop", Icon::Monitor, Action::ShowDesktop, &["minimise all", "hide windows"]),
    ];
    out.extend(recording_actions(recording));
    out
}

/// Record Screen and Record Area, each without saying (the settings' audio)
/// and with desktop audio, the microphone or both; or Stop Recording.
pub fn recording_actions(recording: bool) -> Vec<Item> {
    use crate::recording::{Ask, Audio, Target, ask_label};
    let kw = |more: &[&str]| -> Vec<String> {
        ["video", "screencast", "capture", "recording"].iter().chain(more).map(|s| s.to_string()).collect()
    };
    if recording {
        let stop = Item::new(Scope::Action, "Stop Recording", Icon::Square, Action::StopRecording)
            .keywords(kw(&["end", "finish", "save"]))
            .shortcut(Some(crate::menus::model::sc("⌥", "Print")))
            .key("act:Stop Recording");
        return vec![stop];
    }
    let audios = [
        (None, &[][..]),
        (Some(Audio { desktop: true, microphone: false }), &["sound", "system audio"][..]),
        (Some(Audio { desktop: false, microphone: true }), &["mic", "voice"][..]),
        (Some(Audio { desktop: true, microphone: true }), &["sound", "mic", "voice"][..]),
    ];
    let mut out = Vec::new();
    for (target, glyph) in [(Target::Screen, Icon::Video), (Target::Pick, Icon::Scan)] {
        for (audio, more) in audios {
            let ask = Ask { target: target.clone(), audio };
            let title = ask_label(&ask);
            let mut item = Item::new(Scope::Action, title.clone(), glyph, Action::Record(ask)).keywords(kw(more));
            if target == Target::Pick && audio.is_none() {
                // ⌥Print: pick an area, or click for a display.
                item = item.shortcut(Some(crate::menus::model::sc("⌥", "Print")));
            }
            out.push(item.key(format!("act:{title}")));
        }
    }
    out
}

/// A shortcut as keycaps: each modifier, then the key.
pub fn keycaps(s: &aizome_shell_proto::Shortcut) -> Vec<String> {
    let mut k = Vec::new();
    for (on, g) in [(s.ctrl, "⌃"), (s.alt, "⌥"), (s.shift, "⇧"), (s.command, "❖")] {
        if on {
            k.push(g.to_owned());
        }
    }
    k.push(s.key.clone());
    k
}

/// What a key in the palette does (the keys the query field leaves).
pub fn key_event(k: &KeyEvent) -> Option<PaletteEvent> {
    let m = k.modifiers;
    Some(match &k.key {
        Key::Named(NamedKey::Escape) => PaletteEvent::Ui(PaletteMsg::Close),
        Key::Named(NamedKey::Enter) => PaletteEvent::Ui(PaletteMsg::Run { keep_open: m.super_key() }),
        Key::Named(NamedKey::ArrowUp) => PaletteEvent::Move(-1),
        Key::Named(NamedKey::ArrowDown) => PaletteEvent::Move(1),
        Key::Named(NamedKey::PageUp) => PaletteEvent::Move(-6),
        Key::Named(NamedKey::PageDown) => PaletteEvent::Move(6),
        Key::Named(NamedKey::Tab) => PaletteEvent::Cycle(!m.shift()),
        Key::Character(c) if m.super_key() && c.eq_ignore_ascii_case("k") => PaletteEvent::Actions,
        _ => return None,
    })
}

impl Shell {
    /// Whether the palette finds `scope` (`[palette] scopes`; an answer to
    /// arithmetic always).
    pub fn palette_finds(&self, scope: Scope) -> bool {
        match scope {
            Scope::Command => self.palette_cfg.finds("commands"),
            Scope::Window => self.palette_cfg.finds("windows"),
            Scope::File => self.palette_cfg.finds("files"),
            Scope::App => self.palette_cfg.finds("apps"),
            Scope::Setting => self.palette_cfg.finds("settings"),
            Scope::Action => self.palette_cfg.finds("actions"),
            Scope::Answer => true,
        }
    }

    /// The scopes Tab goes through, after All: those the palette finds.
    pub fn palette_scopes(&self) -> Vec<Scope> {
        Scope::CYCLE.iter().copied().filter(|s| self.palette_finds(*s)).collect()
    }

    /// Open the palette (or close it if open) on the focused output.
    pub fn toggle_palette(&mut self, scope: Option<Scope>, query: String, cx: &mut Cx<Msg>) {
        if self.palette.is_some() {
            self.close_palette(cx);
            return;
        }
        self.open_palette(scope, query, cx);
    }

    pub fn open_palette(&mut self, scope: Option<Scope>, query: String, cx: &mut Cx<Msg>) {
        // A scope the settings leave out opens All.
        let scope = scope.filter(|s| self.palette_finds(*s));
        // What it learnt, as its file has it now (Settings may have
        // forgotten it).
        self.refresh_frecency();
        if let Some(p) = &mut self.palette {
            p.scope = scope;
            p.query = query;
            self.rank_palette(cx);
            return;
        }
        let Some(output) = self.focused_screen().map(|s| s.output.id) else { return };
        // The window kept hidden and drawn ahead, if there is one: it shows
        // what it drew, and its first frame draws only what differs from
        // that (nothing, as a rule).
        let window = match self.spare_palette_on(output, cx) {
            Some(spare) => {
                self.palette_ahead.spare = None;
                self.palette_ahead.scale = None;
                cx.show_window(spare);
                spare
            }
            None => {
                let size = view::size();
                let window = cx.open_window(layer(palette_layer(output), size.width, size.height, "Palette"));
                self.surfaces.insert(window, Surface::Palette);
                self.palette_ahead.output = output;
                window
            }
        };
        self.palette = Some(self.palette_state(window, scope, query));
        if self.opts.live {
            cx.spawn(|| {
                let list = aizome_search::recent::load(aizome_search::recent::default_path())
                    .into_iter()
                    .take(200)
                    .map(|r| {
                        let d = r.path.is_dir();
                        (r.path, d)
                    })
                    .collect();
                Msg::Palette(PaletteEvent::Recent(list))
            });
        }
        if self.file_index.is_none() && self.opts.live {
            self.index_soon(true, cx);
        }
        self.rank_palette(cx);
        cx.focus("palette-query");
    }

    pub fn close_palette(&mut self, cx: &mut Cx<Msg>) {
        self.file_index = None;
        let Some(p) = self.palette.take() else { return };
        if self.palette_ahead.on {
            // Kept, hidden, and drawn ahead again soon as it will open.
            cx.hide_window(p.window);
            self.palette_ahead.spare = Some(p);
            self.palette_ahead.scale = None;
            self.palette_ahead_soon(cx);
        } else {
            cx.close_window(p.window);
            self.surfaces.remove(&p.window);
        }
    }

    /// What the palette shows in `window` when it opens now in `scope`
    /// with `query`, before ranking: every row but files', and where it
    /// opens from.
    fn palette_state(&self, window: WindowKey, scope: Option<Scope>, query: String) -> PaletteState {
        let (app_name, app_id, context) = match self.wm.focused_window() {
            Some(w) => {
                let name = self.focused_menus().0;
                let ctx = if w.title.is_empty() { format!("in {name}") } else { format!("in {name} · {}", w.title) };
                (Some(name), w.app_id.clone(), ctx)
            }
            None => (None, String::new(), "in Aizome".into()),
        };
        PaletteState {
            window,
            query,
            scope,
            selected: 0,
            scroll: 0.0,
            items: self.palette_items(),
            files: Vec::new(),
            files_for: "\u{0}".into(),
            sections: Vec::new(),
            context,
            app_name,
            app_id,
            search: 0,
            recent: Arc::default(),
            boosts: Arc::default(),
        }
    }

    /// Every row the palette can show but files.
    pub fn palette_items(&self) -> Vec<Item> {
        let mut out = Vec::new();
        let app_id = self.wm.focused_window().map(|w| w.app_id.clone()).unwrap_or_else(|| "aizome".into());
        // Commands: every item of the focused app's menus.
        let (_name, menus) = self.focused_menus();
        for m in &menus {
            for f in m.flatten() {
                if !f.entry.enabled || f.entry.action == Action::None {
                    continue;
                }
                let mut trail = vec![m.title.clone()];
                trail.extend(f.trail.iter().cloned());
                out.push(self.command_item(&app_id, &trail, &f.entry));
            }
        }
        // The Window menu's verbs, for apps whose menus lack them.
        if self.wm.focused_window().is_some() {
            let verbs = crate::menus::build::window_verbs(self.wm.focused_window(), &self.wm.desks);
            for f in verbs.flatten() {
                if !f.entry.enabled || out.iter().any(|i| i.scope == Scope::Command && i.title == f.entry.label) {
                    continue;
                }
                let mut trail = vec!["Window".to_owned()];
                trail.extend(f.trail.iter().cloned());
                out.push(self.command_item(&app_id, &trail, &f.entry));
            }
        }
        // Windows.
        for w in self.wm.in_open_order() {
            let (name, glyph, _) = self.app_look(&w.app_id);
            let where_ = if w.minimised || w.hidden {
                "hidden".to_owned()
            } else {
                w.desk.map(|d| format!("desk {d}")).unwrap_or_else(|| "every desk".into())
            };
            let title = if w.title.is_empty() { name.clone() } else { w.title.clone() };
            out.push(
                Item::new(Scope::Window, title, glyph, Action::Wm(Request::Focus { id: w.id }))
                    .trail(format!("{name} · {where_}"))
                    .keywords(vec![name])
                    .key(format!("win:{}", w.app_id)),
            );
        }
        // Apps.
        if let Some(index) = &self.apps {
            for a in index.apps() {
                let glyph = crate::apps::catalog::glyph_for(a.app_id())
                    .map(|g| g.0)
                    .unwrap_or_else(|| crate::apps::catalog::glyph_by_category(&a.categories));
                let mut kw: Vec<String> = a.keywords.clone();
                kw.extend(a.generic_name.clone());
                let key = format!("app:{}", a.app_id());
                let mut item =
                    Item::new(Scope::App, a.name.clone(), glyph, Action::FocusApp { app_id: a.app_id().to_owned() })
                        .trail(a.generic_name.clone().unwrap_or_default())
                        .keywords(kw)
                        .key(key.clone());
                item.boost = self.frecency.boost(&key);
                item.mark = self.app_toolkit(a).and_then(|t| Some((t.mark(self.marks)?, t.name())));
                out.push(item);
            }
        }
        // Settings.
        let list = if self.settings_list.is_empty() { model::builtin_settings() } else { self.settings_list.clone() };
        for s in &list {
            let mut item = model::setting_item(s);
            item.boost = item.key.as_deref().map_or(0, |k| self.frecency.boost(k));
            out.push(item);
        }
        // Actions.
        for mut a in actions(
            self.theme_day,
            self.notes_cfg.do_not_disturb,
            self.recording.is_some(),
            self.awake != aizome_awake::Status::Off,
        ) {
            a.boost = a.key.as_deref().map_or(0, |k| self.frecency.boost(k));
            out.push(a);
        }
        // Only what the settings say the palette finds.
        out.retain(|i| self.palette_finds(i.scope));
        out
    }

    fn command_item(&self, app_id: &str, trail: &[String], e: &Entry) -> Item {
        let key = format!("cmd:{app_id}:{}/{}", trail.join("/"), e.label);
        // The item's own glyph, else its app's (Aizome's command key is ❖,
        // so never ⌘), else a chevron.
        let glyph = match &e.icon {
            Some(MenuIcon::Glyph(g)) => *g,
            _ => crate::apps::catalog::glyph_for(app_id).map(|g| g.0).unwrap_or(Icon::ChevronRight),
        };
        let mut item = Item::new(Scope::Command, e.label.clone(), glyph, e.action.clone())
            .trail(trail.join(" › "))
            .shortcut(e.shortcut.clone())
            .key(key.clone());
        item.boost = self.frecency.boost(&key);
        item
    }

    /// Rank again for the query and scope; search files if they're wanted.
    pub fn rank_palette(&mut self, cx: &mut Cx<Msg>) {
        let Some(mut p) = self.palette.take() else { return };
        self.rank(&mut p, cx);
        self.palette = Some(p);
    }

    /// Rank `p` for its query and scope; search files if they're wanted.
    fn rank(&self, p: &mut PaletteState, cx: &mut Cx<Msg>) {
        let per_section = self.palette_cfg.per_section as usize;
        let home = aizome_config::home_dir();
        let boosts = p.boosts.clone();
        let index = self.file_index.clone();
        let files_on = self.palette_finds(Scope::File);
        let wants_files = files_on && (p.scope.is_none() || p.scope == Some(Scope::File));
        let q = p.query.trim().to_owned();
        if wants_files && p.files_for != q {
            if q.is_empty() {
                // Recent files first when nothing's typed.
                p.files = p.recent.iter().take(20).map(|(path, folder)| Item::file(path, *folder, &home)).collect();
                p.files_for = q.clone();
            } else if let Some(index) = index {
                p.search += 1;
                let seq = p.search;
                let query = q.clone();
                cx.spawn(move || {
                    let boost =
                        |path: &std::path::Path| boosts.get(&format!("file:{}", path.display())).copied().unwrap_or(0);
                    let hits = index.search(&query, 40, &boost);
                    let items = hits.iter().map(|h| Item::file(&h.path, h.folder, &home)).collect();
                    Msg::Palette(PaletteEvent::Files(seq, items))
                });
            }
        }
        let files: Vec<Item> = if wants_files { p.files.clone() } else { Vec::new() };
        let r = Ranking { query: &p.query, scope: p.scope, per_section, app_name: p.app_name.as_deref() };
        p.sections = model::sections(&r, &p.items, &files);
        let n = model::flat(&p.sections).len();
        if p.selected >= n {
            p.selected = n.saturating_sub(1);
        }
        p.scroll = view::scroll_for(&p.sections, p.selected, p.scroll);
    }

    /// Frecency boosts of recently used files, for searches on a thread
    /// (the store stays here).
    fn file_boosts(&self, recent: &[(PathBuf, bool)]) -> std::collections::HashMap<String, i64> {
        recent
            .iter()
            .take(200)
            .map(|(path, _)| {
                let k = format!("file:{}", path.display());
                let b = self.frecency.boost(&k).max(8);
                (k, b)
            })
            .collect()
    }

    pub fn palette_view(&self) -> Option<PaletteView> {
        self.palette.as_ref().map(|p| self.palette_view_of(p))
    }

    /// The palette that `window` shows: the open one, or the one kept
    /// hidden and drawn ahead.
    pub fn palette_in(&self, window: WindowKey) -> Option<&PaletteState> {
        let open = self.palette.as_ref().filter(|p| p.window == window);
        open.or(self.palette_ahead.spare.as_ref().filter(|p| p.window == window))
    }

    /// What palette `p` shows.
    pub fn palette_view_of(&self, p: &PaletteState) -> PaletteView {
        let rows = model::flat(&p.sections);
        let detail = rows.get(p.selected).map(|r| self.detail_of(&r.item, p));
        PaletteView {
            query: p.query.clone(),
            scope: p.scope,
            scopes: self.palette_scopes(),
            context: p.context.clone(),
            sections: p.sections.clone(),
            selected: p.selected,
            detail,
        }
    }

    fn detail_of(&self, i: &Item, p: &PaletteState) -> Detail {
        let caption = match i.scope {
            Scope::Command => "COMMAND",
            Scope::Window => "WINDOW",
            Scope::File if i.title.ends_with('/') => "FOLDER",
            Scope::File => "FILE",
            Scope::App => "APP",
            Scope::Setting => "SETTING",
            Scope::Action => "ACTION",
            Scope::Answer => "ANSWER",
        };
        let mut d = Detail {
            caption: caption.into(),
            title: i.title.clone(),
            path: i.trail.clone(),
            toolkit: i.mark.as_ref().map(|(m, name)| (m.clone(), (*name).to_owned())),
            ..Detail::default()
        };
        match i.scope {
            Scope::Command => {
                d.path = format!("{} › {}", p.app_name.clone().unwrap_or_else(|| "Aizome".into()), i.trail);
                if let Some(s) = &i.shortcut {
                    d.keys = keycaps(s);
                    let count = i
                        .key
                        .as_deref()
                        .map(|k| {
                            model::coach_count(self.frecency.score(&format!("menu:{}", k.trim_start_matches("cmd:"))))
                        })
                        .unwrap_or(0);
                    d.coach = model::coach(count, true);
                }
                d.actions = vec![
                    ("Run".into(), "↩".into()),
                    ("Run, keep palette open".into(), "❖↩".into()),
                    ("More actions".into(), "❖K".into()),
                ];
            }
            Scope::Answer => d.actions = vec![("Copy the result".into(), "↩".into())],
            Scope::Window => d.actions = vec![("Focus".into(), "↩".into()), ("More actions".into(), "❖K".into())],
            _ => d.actions = vec![("Open".into(), "↩".into()), ("More actions".into(), "❖K".into())],
        }
        d
    }

    /// What the palette heard.
    pub fn palette_event(&mut self, e: PaletteEvent, cx: &mut Cx<Msg>) {
        match e {
            PaletteEvent::Index(Index(i)) => {
                // Only the palette needs it: dropped when it closes.
                if self.palette.is_some() {
                    self.file_index = Some(i);
                    if let Some(p) = &mut self.palette {
                        p.files_for = "\u{0}".into();
                    }
                    self.rank_palette(cx);
                }
            }
            PaletteEvent::Frecency(store) => {
                self.frecency = *store;
                // Picks rank what the palette opens with.
                self.palette_ahead.bumped += 1;
            }
            PaletteEvent::Recent(list) => {
                let boosts = self.file_boosts(&list);
                if let Some(p) = &mut self.palette {
                    p.recent = Arc::new(list);
                    p.boosts = Arc::new(boosts);
                    p.files_for = "\u{0}".into();
                    self.rank_palette(cx);
                }
            }
            PaletteEvent::Refresh => {
                if self.opts.live && self.palette.is_none() {
                    self.index_soon(false, cx);
                }
            }
            PaletteEvent::Settings(list) => self.settings_list = list,
            PaletteEvent::Files(seq, items) => {
                if let Some(p) = &mut self.palette
                    && p.search == seq
                {
                    p.files = items;
                    p.files_for = p.query.trim().to_owned();
                    self.rank_palette(cx);
                }
            }
            PaletteEvent::Move(by) => self.palette_move(by),
            PaletteEvent::Cycle(forward) => self.palette_cycle(forward, cx),
            PaletteEvent::Actions => self.palette_actions(cx),
            PaletteEvent::Ui(m) => self.palette_msg(m, cx),
        }
    }

    pub fn palette_msg(&mut self, m: PaletteMsg, cx: &mut Cx<Msg>) {
        let Some(p) = &mut self.palette else { return };
        match m {
            PaletteMsg::Query(q) => {
                p.query = q;
                p.selected = 0;
                p.scroll = 0.0;
                self.rank_palette(cx);
            }
            PaletteMsg::Scope(s) => {
                p.scope = s;
                p.selected = 0;
                p.scroll = 0.0;
                self.rank_palette(cx);
            }
            PaletteMsg::Click(i) => {
                if p.selected == i {
                    self.run_selected(false, cx);
                } else {
                    p.selected = i;
                    p.scroll = view::scroll_for(&p.sections, i, p.scroll);
                }
            }
            PaletteMsg::Run { keep_open } => self.run_selected(keep_open, cx),
            PaletteMsg::Close => self.close_palette(cx),
        }
    }

    /// ↑ and ↓.
    pub fn palette_move(&mut self, by: isize) {
        let Some(p) = &mut self.palette else { return };
        let n = model::flat(&p.sections).len();
        if n == 0 {
            return;
        }
        let at = p.selected as isize + by;
        p.selected =
            if by.abs() > 1 { at.clamp(0, n as isize - 1) as usize } else { at.rem_euclid(n as isize) as usize };
        p.scroll = view::scroll_for(&p.sections, p.selected, p.scroll);
    }

    /// Tab and ⇧Tab.
    pub fn palette_cycle(&mut self, forward: bool, cx: &mut Cx<Msg>) {
        let scopes = self.palette_scopes();
        let Some(p) = &mut self.palette else { return };
        p.scope = model::cycle(&scopes, p.scope, forward);
        p.selected = 0;
        p.scroll = 0.0;
        self.rank_palette(cx);
    }

    fn run_selected(&mut self, keep_open: bool, cx: &mut Cx<Msg>) {
        let Some(p) = &self.palette else { return };
        let Some(item) = model::flat(&p.sections).get(p.selected).map(|r| r.item.clone()) else { return };
        if let Some(k) = &item.key {
            let mut keys = vec![k.clone()];
            if item.scope == Scope::Command && item.shortcut.is_some() {
                keys.push(format!("menu:{}", k.trim_start_matches("cmd:")));
            }
            for k in &keys {
                self.frecency.visit(k);
            }
            self.record_frecency(keys);
        }
        if !keep_open {
            self.close_palette(cx);
        }
        self.run(item.action, cx);
        if keep_open {
            // The app may have changed its menus: rows again.
            let items = self.palette_items();
            if let Some(p) = &mut self.palette {
                p.items = items;
            }
            self.rank_palette(cx);
        }
    }

    /// ❖K: the selected row's actions, as a menu over it.
    pub fn palette_actions(&mut self, cx: &mut Cx<Msg>) {
        let Some(p) = &self.palette else { return };
        let Some(item) = model::flat(&p.sections).get(p.selected).map(|r| r.item.clone()) else { return };
        let Some(rect) = view::row_rect(&p.sections, p.selected, p.scroll) else { return };
        let window = p.window;
        let entries = match (&item.scope, &item.action) {
            (Scope::File, Action::Open { path }) => {
                let p = path.clone();
                let mut open_with: Vec<Entry> = Vec::new();
                if let Some(index) = &self.apps {
                    for a in index.apps().into_iter().filter(|a| !a.mime_types.is_empty()).take(12) {
                        let argv = a
                            .exec
                            .as_deref()
                            .and_then(|e| aizome_sys::apps::Exec::parse(e).ok())
                            .map(|e| {
                                e.expand(&[p.to_string_lossy().into_owned()], &aizome_sys::apps::ExecContext::default())
                            })
                            .unwrap_or_default();
                        if !argv.is_empty() {
                            open_with.push(Entry::action(a.name.clone(), Action::Spawn { argv }));
                        }
                    }
                }
                vec![
                    Entry::action("Open", Action::Open { path: p.clone() }),
                    Entry::submenu("Open With", open_with),
                    Entry::action("Show in Hako", Action::Reveal { path: p.clone() }),
                    Entry::action("Copy Path", Action::Copy { text: p.to_string_lossy().into_owned() }),
                    Entry::separator(),
                    Entry::action("Move to Trash", Action::MoveToTrash { path: p.clone() }),
                ]
            }
            (Scope::Window, Action::Wm(Request::Focus { id })) => vec![
                Entry::action("Focus", crate::actions::wm::focus(*id)),
                Entry::action("Close", crate::actions::wm::close(Some(*id))),
                Entry::submenu(
                    "Move to Desk",
                    crate::menus::build::desk_items(&self.wm.desks, |d| crate::actions::wm::move_to_desk(Some(*id), d)),
                ),
            ],
            (Scope::App, Action::FocusApp { app_id }) => {
                let running = self.wm.app_window_to_activate(app_id, self.apps.as_deref()).is_some();
                vec![
                    Entry::action("Open", Action::FocusApp { app_id: app_id.clone() }),
                    Entry::action("New Window", Action::NewWindow { app_id: app_id.clone() }),
                    Entry::action("Quit", Action::QuitApp { app_id: app_id.clone() }).enabled(running),
                ]
            }
            (Scope::Command, a) => {
                let mut v = vec![Entry::action("Run", a.clone())];
                if let Some(s) = &item.shortcut {
                    v.push(Entry::action("Copy Shortcut", Action::Copy { text: s.label() }));
                }
                v.push(Entry::action("Show in Menu", Action::ShowInMenu { trail: item.trail.clone() }));
                v
            }
            (_, a) => vec![Entry::action("Run", a.clone())],
        };
        let menu = ShellMenu::new(item.title.clone(), entries);
        let anchor = aizome_kit::Rect::new(rect.x1 - 200.0, rect.y0, rect.x1, rect.y1);
        self.show_menu(window, anchor, &menu, MenuStyle::below(), cx);
    }

    /// Record picks into the history's file, on a thread, and hold what
    /// it holds then. The file is the history: one forgotten (Settings
    /// removes it) isn't written back from what the shell remembered.
    pub fn record_frecency(&self, keys: Vec<String>) {
        if !self.opts.live {
            return;
        }
        let path = aizome_search::frecency::Frecency::default_path();
        let tx = self.sender();
        std::thread::spawn(move || match aizome_search::frecency::Frecency::record(&path, &keys) {
            Ok(store) => {
                if let Some(tx) = tx {
                    tx.send(Msg::Palette(PaletteEvent::Frecency(Box::new(store))));
                }
            }
            Err(e) => log::warn!("recording the palette's pick in {}: {e}", path.display()),
        });
    }

    /// Read the history again if its file changed (forgotten, or written
    /// by another shell) since it was read.
    pub fn refresh_frecency(&mut self) {
        if self.opts.live && self.frecency.is_stale() {
            self.frecency = aizome_search::frecency::Frecency::load(aizome_search::frecency::Frecency::default_path());
        }
    }

    /// Read (or build) the file index and Settei's settings list, on
    /// threads.
    pub fn start_palette_threads(&mut self, cx: &mut Cx<Msg>) {
        // The index is rebuilt when stale, now and every ten minutes, and
        // held in memory only while the palette is open.
        self.index_soon(false, cx);
        let period = std::time::Duration::from_secs(self.palette_cfg.reindex_minutes.max(1) as u64 * 60);
        cx.every(period, Msg::Palette(PaletteEvent::Refresh));
        self.settings_list_soon(cx);
    }

    /// Read the file index (rebuilding it first if it's stale), on a
    /// thread; `keep` hands it to the palette.
    pub fn index_soon(&mut self, keep: bool, cx: &mut Cx<Msg>) {
        let roots: Vec<PathBuf> = self.palette_cfg.roots.iter().map(|r| aizome_config::expand_home(r)).collect();
        let skip: Vec<PathBuf> = self.palette_cfg.exclude.iter().map(|r| aizome_config::expand_home(r)).collect();
        let max_age = std::time::Duration::from_secs(self.palette_cfg.reindex_minutes.max(1) as u64 * 60);
        // What the index was built over: an index of other roots, or one
        // that went into a folder skipped since, is built again.
        let over = index_key(&roots, &skip);
        cx.spawn(move || {
            let cache = FileIndex::default_cache_path();
            let key_file = cache.with_extension("key");
            let same = std::fs::read_to_string(&key_file).is_ok_and(|k| k == over);
            let index = match FileIndex::load(&cache) {
                Some(i) if same && i.age().is_some_and(|a| a < max_age) => i,
                _ => {
                    let limits = aizome_search::files::Limits { skip, ..Default::default() };
                    let i = FileIndex::build(&roots, &limits);
                    if let Some(d) = cache.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let _ = i.save(&cache);
                    let _ = std::fs::write(&key_file, &over);
                    i
                }
            };
            if keep { Msg::Palette(PaletteEvent::Index(Index(Arc::new(index)))) } else { Msg::Nothing }
        });
    }

    fn settings_list_soon(&mut self, cx: &mut Cx<Msg>) {
        cx.spawn(|| {
            let cache = model::settings_cache_path();
            let list = std::process::Command::new("aizome-settei")
                .args(["--list-settings", "--json"])
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    let text = String::from_utf8_lossy(&o.stdout).into_owned();
                    let list = model::parse_settings(&text)?;
                    if let Some(d) = cache.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let _ = std::fs::write(&cache, &text);
                    Some(list)
                })
                .or_else(|| std::fs::read_to_string(&cache).ok().and_then(|t| model::parse_settings(&t)))
                .unwrap_or_default();
            Msg::Palette(PaletteEvent::Settings(list))
        });
    }
}

/// What a file index was built over (its roots, and the folders it
/// skipped), kept beside it to tell when it's for other settings.
pub fn index_key(roots: &[PathBuf], skip: &[PathBuf]) -> String {
    let mut s = String::from("roots\n");
    for r in roots {
        s.push_str(&r.to_string_lossy());
        s.push('\n');
    }
    s.push_str("skip\n");
    for r in skip {
        s.push_str(&r.to_string_lossy());
        s.push('\n');
    }
    s
}

impl Shell {
    /// Draw the palette ahead as it would open now (the empty query, All,
    /// the query field focused) in its hidden window, made hidden the
    /// first time (or for another output), at the focused output's scale;
    /// nothing if it's drawn so already.
    pub fn prepare_palette(&mut self, cx: &mut Cx<Msg>) {
        if self.palette.is_some() || !self.palette_ahead.on {
            return;
        }
        let Some((output, scale)) = self.focused_screen().map(|s| (s.output.id, s.output.scale)) else { return };
        self.refresh_frecency();
        let spare = self.spare_palette_on(output, cx);
        let window = match spare {
            Some(spare) => spare,
            None => {
                let size = view::size();
                let w = cx.open_window(layer(palette_layer(output), size.width, size.height, "Palette").hidden());
                self.surfaces.insert(w, Surface::Palette);
                self.palette_ahead.output = output;
                self.palette_ahead.scale = None;
                w
            }
        };
        let mut state = self.palette_state(window, None, String::new());
        self.rank(&mut state, cx);
        self.palette_ahead.inputs = self.palette_inputs();
        let drawn = self.palette_ahead.scale == Some(scale)
            && self
                .palette_ahead
                .spare
                .as_ref()
                .is_some_and(|s| self.palette_view_of(s) == self.palette_view_of(&state));
        self.palette_ahead.spare = Some(state);
        if drawn {
            return;
        }
        self.palette_ahead.scale = cx.prepare_window(window, scale).then_some(scale);
        // As it opens: with the query field focused (it draws as focused
        // once the palette has the keyboard).
        cx.focus("palette-query");
        log::debug!("the palette drawn ahead at {scale}× for output {output}");
    }

    /// The palette's hidden window, if there is one for `output`: one made
    /// for another output is closed (and its state dropped).
    fn spare_palette_on(&mut self, output: u32, cx: &mut Cx<Msg>) -> Option<WindowKey> {
        if self.palette_ahead.output != output
            && let Some(spare) = self.palette_ahead.spare.take()
        {
            cx.close_window(spare.window);
            self.surfaces.remove(&spare.window);
            self.palette_ahead.scale = None;
        }
        self.palette_ahead.spare.as_ref().map(|s| s.window)
    }

    /// Draw the palette ahead again [`AHEAD_AFTER`] from now, unless asked
    /// again meanwhile.
    pub fn palette_ahead_soon(&mut self, cx: &mut Cx<Msg>) {
        self.palette_ahead.asked += 1;
        cx.after(AHEAD_AFTER, Msg::PreparePalette(self.palette_ahead.asked));
    }

    /// After every update, while the palette is closed: if what it opens
    /// with changed, draw it ahead again soon.
    pub fn keep_palette_ahead(&mut self, cx: &mut Cx<Msg>) {
        if !self.palette_ahead.on || self.palette.is_some() {
            return;
        }
        let inputs = self.palette_inputs();
        if inputs != self.palette_ahead.inputs {
            self.palette_ahead.inputs = inputs;
            self.palette_ahead_soon(cx);
        }
    }

    /// What the palette opens with, hashed cheaply (it's asked after every
    /// update): the focused window and every window, apps' menus and picks
    /// (as counted), the apps, settings and actions, the palette's
    /// settings, and the focused output and its scale.
    pub fn palette_inputs(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.wm.focused_window().map(|w| (w.id, w.pid)).hash(&mut h);
        for w in self.wm.in_open_order() {
            (w.id, &w.app_id, &w.title, w.desk, w.minimised, w.hidden).hash(&mut h);
        }
        self.palette_ahead.bumped.hash(&mut h);
        self.apps.as_ref().map(|a| Arc::as_ptr(a) as usize).hash(&mut h);
        self.settings_list.len().hash(&mut h);
        (self.theme_day, self.notes_cfg.do_not_disturb, self.recording.is_some()).hash(&mut h);
        (self.palette_cfg.per_section, &self.palette_cfg.scopes).hash(&mut h);
        self.focused_screen().map(|s| (s.output.id, s.output.scale.to_bits())).hash(&mut h);
        h.finish()
    }

    /// The window memory the palette's surface takes at the largest scale
    /// of the outputs (it opens on the focused one): what's faulted in at
    /// idle, so its first opening draws into memory that's there rather
    /// than faulting in a new buffer (1446 × 966 pixels at 2×, 5.6 MB) in
    /// that frame.
    pub fn palette_buffer_bytes(&self) -> usize {
        let scale = self.screens.iter().map(|s| s.output.scale).fold(1.0, f64::max);
        crate::shell::surface_bytes(view::size(), scale)
    }

    /// At idle after starting: lay out what the palette sets, so its first
    /// opening finds the faces loaded and the shaping warm; and draw it
    /// offscreen on a thread of its own at each output's scale, so its
    /// glyphs are rasterized (into the glyph cache every thread shares)
    /// before the render thread first draws them. The palette sets text in
    /// some seventeen sizes and weights, each glyph rasterized at four
    /// quarter-pixel offsets: on its first opening, rasterizing them was
    /// most of the frame. The thread drawing them, if it started.
    pub fn warm_palette(&mut self, theme: &aizome_kit::Theme) -> Option<std::thread::JoinHandle<()>> {
        let started = std::time::Instant::now();
        let mut titles: Vec<String> = Vec::new();
        for i in self.palette_items().into_iter().take(80) {
            titles.push(i.title);
            if !i.trail.is_empty() {
                titles.push(i.trail);
            }
        }
        let layouts: Vec<aizome_kit::sidestep_ui::TextLayout> = view::warm_texts(theme, &titles)
            .into_iter()
            .map(|(text, spec)| self.text.layout(theme, &text, &spec, None))
            .collect();
        log::debug!("the palette's text laid out ahead in {:?}", started.elapsed());
        let mut scales: Vec<f64> = self.screens.iter().map(|s| s.output.scale).collect();
        scales.sort_by(f64::total_cmp);
        scales.dedup();
        let spawned = std::thread::Builder::new().name("aizome-palette-glyphs".into()).spawn(move || {
            let started = std::time::Instant::now();
            for &scale in &scales {
                rasterize_ahead(&layouts, scale);
            }
            log::debug!("the palette's glyphs rasterized ahead at {scales:?} in {:?}", started.elapsed());
        });
        spawned.map_err(|e| log::warn!("couldn't rasterize the palette's glyphs ahead: {e}")).ok()
    }
}

/// Draw `layouts` offscreen at `scale`, each at the four quarter-pixel
/// offsets a glyph is rasterized at, for the glyph cache threads share.
pub fn rasterize_ahead(layouts: &[aizome_kit::sidestep_ui::TextLayout], scale: f64) {
    use aizome_kit::sidestep_ui::{Appearance, offscreen};
    for layout in layouts {
        let size = layout.size();
        let room = aizome_kit::Size::new(size.width + 1.0, size.height.max(1.0));
        let _ = offscreen::render(room, scale, Appearance::DARK, |c| {
            for quarter in 0..4 {
                c.draw_text(layout, aizome_kit::Point::new(f64::from(quarter) * 0.25 / scale, 0.0));
            }
        });
    }
}
