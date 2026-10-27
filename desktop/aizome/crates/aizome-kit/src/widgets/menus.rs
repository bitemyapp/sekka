//! Menus as they draw: a menu (in a popup, or in a window as a torn-off
//! palette), the in-window menu bar (where there's no Aizome shell to
//! publish to), and tooltips. The shell's menus are these too.
//!
//! The look is the boards': `raised` rows 24pt tall with 10pt padding,
//! labels in Archivo 13, shortcuts right-aligned in IBM Plex Mono 11.5 in
//! `text3` (the modifier glyphs from Noto Sans Symbols 2), the hovered row
//! filled with `selection`, separators as an engraved `line`, a 1pt
//! `bevel_lo` edge with the top highlight, and the hard 3pt shadow. Menus
//! at the pointer and palettes have a `menu_title` strip with their name,
//! as the menus board draws them: a palette (torn off) has a close box and
//! a pin, a submenu at the pointer a pin to tear it off, and the menu at
//! the pointer itself only its name (its strip still tears it off).
//!
//! Behaviour: ↑↓ move, → opens a submenu, ← closes it (and past either
//! end a menu bar's menu goes to the title beside: [`MenuStyle::on_side`]),
//! Return or Space activates, Escape clears what's typed and then closes;
//! typing filters
//! the rows (and the submenus', flattened with their paths), or goes to
//! whoever showed the menu ([`MenuStyle::on_type`]); moving toward an open
//! submenu across other rows doesn't close it.

use std::marker::PhantomData;
use std::time::Duration;

use sidestep_ui::{Key, NamedKey, PointerButton};

use crate::draw;
use crate::element::{Common, Element, MsgFn, MsgMap, RawElement};
use crate::engine::KitMsg;
use crate::hash::PaintHasher;
use crate::icons::Icon;
use crate::layout::{Align, Constraints, Length, Point, Rect, Size, Vec2};
use crate::menu::{ItemSpec, Menu, MenuSpec};
use crate::platform::{Placement, WinId};
use crate::program::AppInfo;
use crate::text::TextSpec;
use crate::theme::{FontSpec, Theme};
use crate::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget};

const PAD: f64 = 10.0;
const GUTTER: f64 = 18.0;
const ARROW: f64 = 16.0;
const STRIP: f64 = 22.0;
/// How long the pointer may cross other rows on its way to a submenu.
const AIM: Duration = Duration::from_millis(220);

/// Where a menu opens against the rectangle it's anchored to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuPlacement {
    /// Below it, left edges lined up (a menu bar's title, a pop-up button).
    #[default]
    Below,
    /// Its top left at the rectangle's top left (a context menu: anchor a
    /// 1 × 1 rectangle at the pointer).
    AtPoint,
    /// To its right, tops lined up (a submenu).
    Beside,
    /// Above it.
    Above,
}

impl MenuPlacement {
    pub(crate) fn placement(self) -> Placement {
        match self {
            MenuPlacement::Below => Placement::Below,
            MenuPlacement::AtPoint => Placement::AtPoint,
            MenuPlacement::Beside => Placement::Beside,
            MenuPlacement::Above => Placement::Above,
        }
    }
}

/// How a menu shows: where, with a title strip or not, and what its strip
/// does.
pub struct MenuStyle<Msg> {
    pub(crate) placement: MenuPlacement,
    pub(crate) look: MenuLook,
    _m: PhantomData<fn() -> Msg>,
}

impl<Msg> Default for MenuStyle<Msg> {
    fn default() -> Self {
        MenuStyle { placement: MenuPlacement::Below, look: MenuLook::default(), _m: PhantomData }
    }
}

impl<Msg> MenuStyle<Msg> {
    /// Below the anchor (the default).
    pub fn below() -> Self {
        MenuStyle::default()
    }

    /// At the anchor's top left, as a menu at the pointer.
    pub fn at_point() -> Self {
        MenuStyle { placement: MenuPlacement::AtPoint, ..MenuStyle::default() }
    }

    pub fn placement(mut self, p: MenuPlacement) -> Self {
        self.placement = p;
        self
    }

    /// A `menu_title` strip with `title`, as menus at the pointer and
    /// their submenus have.
    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.look.title = Some(title.into());
        self
    }

    pub fn min_width(mut self, w: f64) -> Self {
        self.look.min_width = w;
        self
    }

    /// Aizome's mark in the title strip, before the title (the Toolchest).
    pub fn mark(mut self) -> Self {
        self.look.mark = true;
        self
    }

    /// Opened from the keyboard: its first row highlighted, as a menu a
    /// key opens has (F10 to a menu bar).
    pub fn from_keyboard(mut self) -> Self {
        self.look.keyboard = true;
        self
    }
}

impl<Msg: Clone + Send + 'static> MenuStyle<Msg> {
    /// Sent when the title strip is dragged (or the pin pressed) to tear
    /// the menu off into a palette; the menu closes.
    pub fn on_tear_off(mut self, msg: Msg) -> Self {
        self.look.tear_off = Some(MsgFn::new(msg));
        self
    }

    /// Sent once when the menu closes, however it closes: an item chosen,
    /// Escape, a press outside it, the compositor dismissing it, another
    /// menu shown in its place, [`Cx::close_menus`](crate::Cx::close_menus)
    /// or its window closing. A menu bar keeps its title highlighted until
    /// then.
    pub fn on_closed(mut self, msg: Msg) -> Self {
        self.look.closed = Some(MsgFn::new(msg));
        self
    }

    /// Typing in the menu (or in a submenu of it) sends what was typed,
    /// and the menus close, instead of filtering the rows: a launcher's
    /// menu handing over to a search (the Toolchest to the palette).
    pub fn on_type(mut self, f: impl Fn(String) -> Msg + 'static) -> Self {
        self.look.typed = Some(MsgMap::new(f));
        self
    }

    /// As [`MenuStyle::on_type`], but the menus stay open, sending every
    /// key typed on, until the program closes them or a surface that takes
    /// the keyboard shows (Aizome's compositor closes them then). Keys
    /// typed quickly after the first, while the search is still opening,
    /// reach it rather than the window under the menu.
    pub fn on_type_until_closed(mut self, f: impl Fn(String) -> Msg + 'static) -> Self {
        self.look.typed = Some(MsgMap::new(f));
        self.look.typed_stays = true;
        self
    }

    /// ← in the menu, and → on a row without a submenu (in it or in a
    /// submenu of it), send `f(-1)` or `f(1)`: a menu bar moving to the
    /// title beside. (← in a submenu still closes it.)
    pub fn on_side(mut self, f: impl Fn(i32) -> Msg + 'static) -> Self {
        self.look.side = Some(MsgMap::new(f));
        self
    }

    /// Submenus tear off too: each gets a title strip with its name, and
    /// dragging it (or pressing its pin) sends `f(path, offset)` and
    /// closes the menus. `path` is the submenu's place from this menu (its
    /// row's index, and its submenus' down to it); `offset` is where it
    /// opened from this menu's top left, as near as the kit knows (beside
    /// its row), to put the palette where the submenu was.
    pub fn on_tear_off_sub(mut self, f: impl Fn(Vec<usize>, Vec2) -> Msg + 'static) -> Self {
        self.look.tear_sub =
            Some(TearSub { path: Vec::new(), offset: Vec2::ZERO, map: MsgMap::new(move |(p, o)| f(p, o)) });
        self
    }
}

/// Tearing submenus off: the path and place so far, and what to send.
#[derive(Clone)]
pub(crate) struct TearSub {
    path: Vec<usize>,
    offset: Vec2,
    map: MsgMap<(Vec<usize>, Vec2)>,
}

/// What a menu looks like and does besides its items (type-erased).
#[derive(Clone, Default)]
pub(crate) struct MenuLook {
    pub title: Option<String>,
    pub min_width: f64,
    pub tear_off: Option<MsgFn>,
    pub on_close: Option<MsgFn>,
    pub pin: Option<(bool, MsgMap<bool>)>,
    /// Aizome's mark before the title (the Toolchest's).
    pub mark: bool,
    /// Sent when the menu closes (the menu's own, not its submenus').
    pub closed: Option<MsgFn>,
    /// What's typed goes here instead of filtering (submenus' too).
    pub typed: Option<MsgMap<String>>,
    /// The menus stay open after handing typing over.
    pub typed_stays: bool,
    /// ← and → past the menu: to the title beside (submenus' → too).
    pub side: Option<MsgMap<i32>>,
    /// It's a submenu (← closes it).
    pub is_sub: bool,
    /// Opened from the keyboard: the first row highlighted.
    pub keyboard: bool,
    /// Its submenus tear off (and it, if it's one of them).
    pub tear_sub: Option<TearSub>,
}

impl MenuLook {
    /// The look of the submenu at `path` of this menu, opened beside
    /// `anchor` (its row, in this menu's coordinates): titled with its own
    /// name if this menu was titled or its submenus tear off.
    pub fn sub(&self, title: &str, path: &[usize], anchor: Rect) -> MenuLook {
        let tear_sub = self.tear_sub.as_ref().map(|t| TearSub {
            path: t.path.iter().chain(path).copied().collect(),
            offset: t.offset + Vec2::new(anchor.x1, anchor.y0),
            map: t.map.clone(),
        });
        let tear_off = tear_sub.clone().map(|t| MsgFn::from_fn(move || t.map.make((t.path.clone(), t.offset))));
        MenuLook {
            title: (self.title.is_some() || tear_sub.is_some()).then(|| title.to_owned()),
            tear_off,
            tear_sub,
            typed: self.typed.clone(),
            typed_stays: self.typed_stays,
            side: self.side.clone(),
            is_sub: true,
            ..MenuLook::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    Action,
    Submenu,
    Separator,
    Header,
}

#[derive(Clone, Debug)]
struct Row {
    y: f64,
    h: f64,
    /// The item's place: its index, and its submenus' when filtered.
    path: Vec<usize>,
    kind: RowKind,
    enabled: bool,
    /// The submenus it's in, when filtered ("Edit › Find").
    trail: String,
}

/// What a menu remembers while open.
#[derive(Default)]
pub(crate) struct MenuState {
    hover: Option<usize>,
    /// The submenu open, by its row's item path.
    open_sub: Option<Vec<usize>>,
    /// What's been typed to filter the rows.
    query: String,
    /// The pointer, last seen, and a row it's crossing toward a submenu.
    last: Option<Point>,
    aiming: Option<usize>,
    /// The strip being dragged from.
    strip_press: Option<Point>,
}

/// A menu, drawn and run.
pub(crate) struct MenuView {
    spec: MenuSpec,
    look: MenuLook,
    rows: Vec<Row>,
    gutter: f64,
    width: f64,
    /// The query the rows were laid out for.
    query: String,
}

/// Whether `query`'s characters come in order in `text` (case aside),
/// and how well: lower is better.
pub(crate) fn fuzzy(query: &str, text: &str) -> Option<u32> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let mut qi = 0;
    let mut score = 0u32;
    let mut last: Option<usize> = None;
    for (i, c) in t.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            let word_start = i == 0 || !t[i - 1].is_alphanumeric();
            score += match last {
                Some(l) if l + 1 == i => 0,
                _ if word_start => 1,
                _ => 3,
            };
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then(|| score + t.len() as u32 / 16)
}

fn label_of(item: &ItemSpec) -> &str {
    match item {
        ItemSpec::Action(a) => &a.label,
        ItemSpec::Submenu(m) => &m.title,
        ItemSpec::Header(h) => h,
        ItemSpec::Separator => "",
    }
}

impl MenuView {
    pub fn new(spec: MenuSpec, look: MenuLook) -> MenuView {
        MenuView { spec, look, rows: Vec::new(), gutter: 0.0, width: 0.0, query: String::new() }
    }

    pub fn element(spec: MenuSpec, look: MenuLook) -> RawElement {
        RawElement::new(MenuView::new(spec, look), Common::default(), Vec::new())
    }

    fn label_spec(theme: &Theme) -> TextSpec {
        TextSpec::new(FontSpec::ui(theme.metrics.body), theme.text())
    }

    fn shortcut_spec(theme: &Theme) -> TextSpec {
        TextSpec::new(FontSpec::mono(theme.metrics.shortcut), theme.text3())
    }

    fn strip_spec(theme: &Theme) -> TextSpec {
        let mut f = FontSpec::ui(12.5 * theme.metrics.factor);
        f.weight = aizome_theme::typeface::WEIGHT_BOLD;
        f.condensed = true;
        TextSpec::new(f, theme.on_menu_title())
    }

    /// The top of the rows (under the strip, if any).
    fn top(&self) -> f64 {
        if self.look.title.is_some() { 1.0 + STRIP } else { 1.0 }
    }

    fn item(&self, path: &[usize]) -> Option<&ItemSpec> {
        let (first, rest) = path.split_first()?;
        let mut item = self.spec.items.get(*first)?;
        for i in rest {
            match item {
                ItemSpec::Submenu(m) => item = m.items.get(*i)?,
                _ => return None,
            }
        }
        Some(item)
    }

    /// The rows for `query`: the menu's own, or every action matching
    /// it, submenus' included, best first.
    fn rows_for(&self, query: &str) -> Vec<(Vec<usize>, RowKind, bool, String)> {
        if query.is_empty() {
            return self
                .spec
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    let (kind, enabled) = match it {
                        ItemSpec::Action(a) => (RowKind::Action, a.enabled),
                        ItemSpec::Submenu(m) => (RowKind::Submenu, !m.items.is_empty()),
                        ItemSpec::Separator => (RowKind::Separator, false),
                        ItemSpec::Header(_) => (RowKind::Header, false),
                    };
                    (vec![i], kind, enabled, String::new())
                })
                .collect();
        }
        let mut found: Vec<(u32, Vec<usize>, bool, String)> = Vec::new();
        fn walk(
            spec: &MenuSpec,
            path: &mut Vec<usize>,
            trail: &str,
            query: &str,
            found: &mut Vec<(u32, Vec<usize>, bool, String)>,
        ) {
            for (i, it) in spec.items.iter().enumerate() {
                path.push(i);
                match it {
                    ItemSpec::Action(a) => {
                        if let Some(s) = fuzzy(query, &a.label) {
                            found.push((s, path.clone(), a.enabled, trail.to_owned()));
                        }
                    }
                    ItemSpec::Submenu(m) => {
                        let t = if trail.is_empty() { m.title.clone() } else { format!("{trail} › {}", m.title) };
                        walk(m, path, &t, query, found);
                    }
                    _ => {}
                }
                path.pop();
            }
        }
        walk(&self.spec, &mut Vec::new(), "", query, &mut found);
        found.sort_by_key(|(s, p, _, _)| (*s, p.len()));
        found.into_iter().map(|(_, p, e, t)| (p, RowKind::Action, e, t)).collect()
    }

    fn row_at(&self, p: Point) -> Option<usize> {
        self.rows.iter().position(|r| p.y >= r.y && p.y < r.y + r.h && p.x >= 0.0 && p.x < self.width)
    }

    fn row_rect(&self, i: usize) -> Rect {
        let r = &self.rows[i];
        Rect::new(1.0, r.y, self.width - 1.0, r.y + r.h)
    }

    fn strip_rect(&self) -> Rect {
        Rect::new(1.0, 1.0, self.width - 1.0, 1.0 + STRIP)
    }

    fn close_box(&self) -> Rect {
        Rect::new(7.0, 1.0 + (STRIP - 10.0) / 2.0, 17.0, 1.0 + (STRIP + 10.0) / 2.0)
    }

    /// A palette, torn off into a window, closes by its close box; menus
    /// at the pointer close as menus do.
    fn has_close(&self) -> bool {
        self.look.on_close.is_some() || self.look.pin.is_some()
    }

    /// The pin: a palette's (kept or not), and a submenu's that tears off.
    fn has_pin(&self) -> bool {
        self.look.pin.is_some() || (self.look.tear_off.is_some() && self.look.is_sub)
    }

    fn pin_box(&self) -> Rect {
        Rect::new(self.width - 22.0, 1.0, self.width - 2.0, 1.0 + STRIP)
    }

    fn selectable(&self, i: usize) -> bool {
        let r = &self.rows[i];
        r.enabled && matches!(r.kind, RowKind::Action | RowKind::Submenu)
    }

    fn set_hover(&self, cx: &mut EventCx, row: Option<usize>, open_submenus: bool) {
        let old = cx.state::<MenuState>().hover;
        if old != row {
            cx.state::<MenuState>().hover = row;
            for r in [old, row].into_iter().flatten() {
                let rect = self.row_rect(r);
                cx.invalidate_rect(rect);
            }
        }
        if !open_submenus {
            return;
        }
        // A submenu opens under the pointer; another item closes it.
        match row.map(|r| (r, self.rows[r].clone())) {
            Some((i, r)) if r.kind == RowKind::Submenu && r.enabled => {
                if cx.state::<MenuState>().open_sub.as_ref() != Some(&r.path) {
                    self.open_sub(cx, i);
                }
            }
            Some(_) if cx.state::<MenuState>().open_sub.take().is_some() => {
                let window = cx.window_id();
                cx.emit_any(Box::new(KitMsg::MenuCloseSubs { window }));
                cx.invalidate();
            }
            _ => {}
        }
    }

    fn open_sub(&self, cx: &mut EventCx, i: usize) {
        let path = self.rows[i].path.clone();
        cx.state::<MenuState>().open_sub = Some(path.clone());
        let anchor = self.row_rect(i) + cx.window_rect().origin().to_vec2();
        let (window, node) = (cx.window_id(), cx.id());
        cx.emit_any(Box::new(KitMsg::MenuSub { window, node, path, anchor }));
        cx.invalidate();
    }

    fn activate_row(&self, cx: &mut EventCx, i: usize) {
        let row = self.rows[i].clone();
        if !row.enabled {
            return;
        }
        match row.kind {
            RowKind::Action => {
                let (window, node) = (cx.window_id(), cx.id());
                cx.emit_any(Box::new(KitMsg::MenuChoose { window, node, path: row.path }));
            }
            RowKind::Submenu => self.open_sub(cx, i),
            _ => {}
        }
    }

    fn step(&self, cx: &mut EventCx, down: bool) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let start = cx.state::<MenuState>().hover;
        let mut i = start.unwrap_or(if down { n - 1 } else { 0 });
        for _ in 0..n {
            i = if down { (i + 1) % n } else { (i + n - 1) % n };
            if self.selectable(i) {
                self.set_hover(cx, Some(i), false);
                return;
            }
        }
    }

    fn close(&self, cx: &mut EventCx) {
        if let Some(m) = &self.look.on_close {
            cx.emit(m);
        }
        let window = cx.window_id();
        cx.emit_any(Box::new(KitMsg::CloseMenus { window }));
    }

    fn set_query(&self, cx: &mut EventCx, q: String) {
        let st = cx.state::<MenuState>();
        st.query = q;
        st.hover = None;
        if st.open_sub.take().is_some() {
            let window = cx.window_id();
            cx.emit_any(Box::new(KitMsg::MenuCloseSubs { window }));
        }
        cx.rebuild();
        cx.invalidate();
    }
}

impl Widget for MenuView {
    fn name(&self) -> &'static str {
        "menu"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let theme = cx.env.theme;
        let m = &theme.metrics;
        self.query = cx.state::<MenuState>().query.clone();
        let label = MenuView::label_spec(theme);
        let shortcut = MenuView::shortcut_spec(theme);
        let caption = TextSpec::new(FontSpec::mono(m.caption), theme.text3());
        let filtered = !self.query.is_empty();
        let rows = self.rows_for(&self.query.clone());
        self.gutter = if rows.iter().any(|(p, ..)| match self.item(p) {
            Some(ItemSpec::Action(a)) => a.checked || a.icon.is_some(),
            Some(ItemSpec::Submenu(m)) => m.icon.is_some(),
            _ => false,
        }) {
            GUTTER
        } else {
            0.0
        };
        let mut out = Vec::new();
        let mut y = self.top() + 4.0;
        let mark = if self.look.mark { 18.0 } else { 0.0 };
        let mut widest: f64 =
            self.look.title.as_ref().map_or(0.0, |t| cx.measure(t, &MenuView::strip_spec(theme)).width + 56.0 + mark);
        if filtered {
            widest = widest.max(cx.measure(&self.query, &label).width + 40.0);
            y += m.menu_row;
        }
        for (path, kind, enabled, trail) in rows {
            let item = self.item(&path).cloned();
            let (h, w) = match (&item, kind) {
                (Some(ItemSpec::Action(a)), _) => {
                    let lw = cx.measure(&a.label, &label).width;
                    let tw = if trail.is_empty() { 0.0 } else { cx.measure(&trail, &shortcut).width + 12.0 };
                    let sw = a.shortcut.map_or(0.0, |s| cx.measure(&s.label(), &shortcut).width + 24.0);
                    (m.menu_row, lw + tw + sw)
                }
                (Some(ItemSpec::Submenu(s)), _) => (m.menu_row, cx.measure(&s.title, &label).width + ARROW + 12.0),
                (_, RowKind::Separator) => (9.0, 0.0),
                (Some(ItemSpec::Header(t)), _) => (22.0, cx.measure(&t.to_uppercase(), &caption).width),
                _ => (m.menu_row, 0.0),
            };
            widest = widest.max(w);
            out.push(Row { y, h, path, kind, enabled, trail });
            y += h;
        }
        if filtered && out.is_empty() {
            y += m.menu_row;
        }
        y += 4.0 + 1.0;
        self.rows = out;
        let shadow = m.shadow;
        let max_w = (bc.max.width - shadow).max(120.0);
        let width = (widest + 2.0 * PAD + self.gutter + 2.0).ceil().max(180.0).max(self.look.min_width).min(max_w);
        // Laid out in a popup that keeps its size (filtered), fill it.
        let width = width.max(bc.min.width - shadow);
        self.width = width;
        Size::new(width + shadow, (y + shadow).max(bc.min.height))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let panel = Rect::new(0.0, 0.0, self.width, cx.size().height - t.metrics.shadow);
        draw::menu_face(cx.canvas, t, panel);
        let state = cx.state::<MenuState>();
        let hover = state.and_then(|s| {
            s.hover.or_else(|| s.open_sub.as_ref().and_then(|p| self.rows.iter().position(|r| &r.path == p)))
        });
        let label_spec = MenuView::label_spec(t);
        let sc_spec = MenuView::shortcut_spec(t);
        if let Some(title) = &self.look.title {
            let strip = self.strip_rect();
            cx.canvas.fill_rect(strip, t.menu_title());
            draw::bottom(cx.canvas, strip.inflate(0.0, 1.0), t.bevel_lo(), 1.0);
            let fg = t.on_menu_title();
            let close = self.close_box();
            if self.has_close() {
                draw::border(cx.canvas, close, fg, 1.5);
            }
            let spec = MenuView::strip_spec(t);
            let right = if self.has_pin() { self.pin_box().x0 } else { strip.x1 - 6.0 };
            let mut x = if self.has_close() { close.x1 + 7.0 } else { PAD };
            if self.look.mark {
                // The mark at 12pt: its tab and three stripes, on whole points.
                let brand = aizome_theme::Brand::for_theme(t.palette.name);
                let m = 3.0;
                let y = (strip.y0 + (strip.height() - 12.0) / 2.0).round();
                for ([rx, ry, rw, rh], c) in brand.mark_rects() {
                    let r = Rect::new(
                        x + rx as f64 * m,
                        y + ry as f64 * m,
                        x + (rx + rw) as f64 * m,
                        y + (ry + rh) as f64 * m,
                    );
                    cx.canvas.fill_rect(r, crate::theme::color(c));
                }
                x += 18.0;
            }
            cx.draw_text_in(title, &spec, Rect::new(x, strip.y0, right, strip.y1), Align::Start);
            if self.has_pin() {
                let pinned = self.look.pin.as_ref().is_some_and(|(p, _)| *p);
                let pb = self.pin_box();
                let c = if pinned { t.focus() } else { t.text3() };
                // The boards draw the same pin either way; its colour says
                // whether the palette is kept.
                Icon::Pin.draw(cx.canvas, Point::new(pb.x0 + 4.0, pb.y0 + (pb.height() - 12.0) / 2.0), 12.0, c);
            }
        }
        let query = state.map(|s| s.query.as_str()).unwrap_or("");
        if !query.is_empty() {
            let r = Rect::new(1.0, self.top() + 4.0, self.width - 1.0, self.top() + 4.0 + t.metrics.menu_row);
            Icon::Search.draw(
                cx.canvas,
                Point::new(r.x0 + PAD - 2.0, r.y0 + (r.height() - 13.0) / 2.0),
                13.0,
                t.text3(),
            );
            let spec = TextSpec { color: t.text(), ..label_spec.clone() };
            let w = cx.draw_text_in(query, &spec, Rect::new(r.x0 + PAD + 16.0, r.y0, r.x1 - PAD, r.y1), Align::Start);
            // The caret after what's typed.
            let x = (r.x0 + PAD + 16.0 + w + 1.0).round();
            cx.canvas.fill_rect(Rect::new(x, r.y0 + 5.0, x + 2.0, r.y1 - 5.0), t.focus());
            draw::separator(cx.canvas, t, 1.0, self.width - 1.0, r.y1 - 1.0);
            if self.rows.is_empty() {
                let none = Rect::new(r.x0 + PAD, r.y1, r.x1 - PAD, r.y1 + t.metrics.menu_row);
                cx.draw_text_in(
                    "No items match",
                    &TextSpec { color: t.faint(), ..label_spec.clone() },
                    none,
                    Align::Start,
                );
            }
        }
        for (i, row) in self.rows.iter().enumerate() {
            let r = self.row_rect(i);
            if !cx.canvas.needs(r) {
                continue;
            }
            let item = self.item(&row.path);
            match row.kind {
                RowKind::Separator => draw::separator(cx.canvas, t, 1.0, self.width - 1.0, row.y + 4.0),
                RowKind::Header => {
                    let spec =
                        TextSpec { letter_spacing: 1.2, ..TextSpec::new(FontSpec::mono(t.metrics.caption), t.text3()) };
                    let text = item.map(label_of).unwrap_or("").to_uppercase();
                    cx.draw_text_in(&text, &spec, Rect::new(r.x0 + PAD, r.y0 + 6.0, r.x1 - PAD, r.y1), Align::Start);
                }
                RowKind::Action | RowKind::Submenu => {
                    let hot = hover == Some(i) && row.enabled;
                    // NeXT's menus (titled ones) engrave every row.
                    if self.look.title.is_some() && !hot {
                        cx.canvas.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + 1.0), t.bevel_hi());
                        if self.rows.get(i + 1).is_some_and(|n| matches!(n.kind, RowKind::Action | RowKind::Submenu)) {
                            cx.canvas.fill_rect(Rect::new(r.x0, r.y1 - 1.0, r.x1, r.y1), t.line());
                        }
                    }
                    if hot {
                        cx.canvas.fill_rect(r, t.selection());
                    }
                    let fg = if !row.enabled {
                        t.faint()
                    } else if hot {
                        t.on_selection()
                    } else {
                        t.text()
                    };
                    let x = r.x0 + PAD + self.gutter;
                    let mut right = r.x1 - PAD;
                    if row.kind == RowKind::Submenu {
                        let c = if hot { t.on_selection() } else { t.text3() };
                        Icon::ChevronRight.draw(
                            cx.canvas,
                            Point::new(right - 12.0, r.y0 + (r.height() - 12.0) / 2.0),
                            12.0,
                            c,
                        );
                        right -= ARROW;
                    }
                    if let Some(ItemSpec::Submenu(m)) = item
                        && let Some(icon) = m.icon
                    {
                        let gy = r.y0 + (r.height() - 14.0) / 2.0;
                        icon.draw(cx.canvas, Point::new(r.x0 + PAD - 3.0, gy), 14.0, if hot { fg } else { t.text3() });
                    }
                    if let Some(ItemSpec::Action(a)) = item {
                        let gy = r.y0 + (r.height() - 14.0) / 2.0;
                        if a.checked {
                            Icon::Check.draw(cx.canvas, Point::new(r.x0 + PAD - 3.0, gy), 14.0, fg);
                        } else if let Some(icon) = a.icon {
                            icon.draw(
                                cx.canvas,
                                Point::new(r.x0 + PAD - 3.0, gy),
                                14.0,
                                if hot { fg } else { t.text3() },
                            );
                        }
                        if let Some(s) = a.shortcut {
                            let c = if hot {
                                t.on_selection()
                            } else if row.enabled {
                                t.text3()
                            } else {
                                t.faint()
                            };
                            let spec = TextSpec { color: c, ..sc_spec.clone() };
                            let w = cx.draw_text_in(&s.label(), &spec, Rect::new(x, r.y0, right, r.y1), Align::End);
                            right -= w + 16.0;
                        }
                    }
                    let spec = TextSpec { color: fg, ..label_spec.clone() };
                    let w = cx.draw_text_in(
                        item.map(label_of).unwrap_or(""),
                        &spec,
                        Rect::new(x, r.y0, right.max(x), r.y1),
                        Align::Start,
                    );
                    if !row.trail.is_empty() {
                        let c = if hot { t.on_selection() } else { t.text3() };
                        let spec = TextSpec { color: c, ..sc_spec.clone() };
                        cx.draw_text_in(
                            &row.trail,
                            &spec,
                            Rect::new(x + w + 10.0, r.y0, right.max(x + w + 10.0), r.y1),
                            Align::Start,
                        );
                    }
                }
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.spec.title).f64(self.width).str(&self.query);
        if let Some(t) = &self.look.title {
            h.str(t);
        }
        h.add(&self.look.pin.as_ref().map(|(p, _)| *p));
        for i in &self.spec.items {
            match i {
                ItemSpec::Action(a) => {
                    h.str(&a.label).add(&(a.enabled, a.checked, a.shortcut, a.icon));
                }
                ItemSpec::Submenu(m) => {
                    h.str(&m.title).add(&(m.items.len(), m.icon));
                }
                ItemSpec::Header(t) => {
                    h.str(t);
                }
                ItemSpec::Separator => {
                    h.add(&0u8);
                }
            }
        }
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn overflow(&self) -> f64 {
        3.0
    }

    fn menu(&self) -> Option<&MenuSpec> {
        Some(&self.spec)
    }

    fn menu_title(&self) -> Option<&str> {
        self.look.title.as_deref()
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerMove { pos, .. } => {
                if let Some(from) = cx.state::<MenuState>().strip_press {
                    // Dragging the strip tears the menu off.
                    if (*pos - from).hypot() > 4.0
                        && let Some(m) = &self.look.tear_off
                    {
                        cx.state::<MenuState>().strip_press = None;
                        cx.emit(m);
                        let window = cx.window_id();
                        cx.emit_any(Box::new(KitMsg::CloseMenus { window }));
                    }
                    return Status::Handled;
                }
                let row = self.row_at(*pos).filter(|&i| self.selectable(i));
                let last = cx.state::<MenuState>().last.replace(*pos);
                // Heading right toward an open submenu: let the pointer
                // cross rows on the way.
                let toward_sub = cx.state::<MenuState>().open_sub.is_some()
                    && last.is_some_and(|l| {
                        let d: Vec2 = *pos - l;
                        d.x > 0.5 && d.y.abs() < d.x * 2.0
                    });
                if toward_sub && row != cx.state::<MenuState>().hover {
                    cx.state::<MenuState>().aiming = row;
                    cx.request_tick(AIM);
                } else {
                    cx.state::<MenuState>().aiming = None;
                    self.set_hover(cx, row, true);
                }
                Status::Handled
            }
            Event::Tick => {
                // The pointer stopped on its way: follow it after all.
                if let Some(row) = cx.state::<MenuState>().aiming.take() {
                    self.set_hover(cx, Some(row), true);
                }
                Status::Handled
            }
            Event::PointerLeave => {
                let st = cx.state::<MenuState>();
                st.last = None;
                st.aiming = None;
                if st.open_sub.is_none() {
                    self.set_hover(cx, None, false);
                }
                Status::Handled
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. }
                if self.look.title.is_some() && self.strip_rect().contains(*pos) =>
            {
                if self.has_close() && self.close_box().inflate(4.0, 4.0).contains(*pos) {
                    self.close(cx);
                } else if self.has_pin() && self.pin_box().contains(*pos) {
                    if let Some((on, f)) = &self.look.pin {
                        cx.emit_any(f.make(!on));
                    } else if let Some(m) = &self.look.tear_off {
                        cx.emit(m);
                        let window = cx.window_id();
                        cx.emit_any(Box::new(KitMsg::CloseMenus { window }));
                    }
                } else if self.look.tear_off.is_some() {
                    cx.state::<MenuState>().strip_press = Some(*pos);
                } else {
                    // A palette's strip moves its window.
                    cx.begin_window_move();
                }
                Status::Handled
            }
            Event::PointerDown { .. } => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left | PointerButton::Right, .. } => {
                if cx.state::<MenuState>().strip_press.take().is_some() {
                    return Status::Handled;
                }
                if let Some(i) = self.row_at(*pos).filter(|&i| self.selectable(i)) {
                    self.activate_row(cx, i);
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed => {
                let m = k.modifiers;
                if m.super_key() || m.control() {
                    return Status::Ignored;
                }
                match &k.key {
                    Key::Named(NamedKey::ArrowDown) => self.step(cx, true),
                    Key::Named(NamedKey::ArrowUp) => self.step(cx, false),
                    Key::Named(NamedKey::Home) => {
                        let first = (0..self.rows.len()).find(|&i| self.selectable(i));
                        self.set_hover(cx, first, false);
                    }
                    Key::Named(NamedKey::End) => {
                        let last = (0..self.rows.len()).rev().find(|&i| self.selectable(i));
                        self.set_hover(cx, last, false);
                    }
                    Key::Named(NamedKey::Enter) => {
                        let hover = cx.state::<MenuState>().hover;
                        let filtered = !cx.state::<MenuState>().query.is_empty();
                        let target = hover
                            .or_else(|| filtered.then(|| (0..self.rows.len()).find(|&i| self.selectable(i))).flatten());
                        if let Some(i) = target {
                            self.activate_row(cx, i);
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        if let Some(i) = cx.state::<MenuState>().hover
                            && self.rows[i].kind == RowKind::Submenu
                        {
                            self.open_sub(cx, i);
                            let window = cx.window_id();
                            cx.emit_any(Box::new(KitMsg::MenuFocusSub { window }));
                        } else if let Some(side) = &self.look.side {
                            cx.emit_any(side.make(1));
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) => match &self.look.side {
                        Some(side) if !self.look.is_sub => cx.emit_any(side.make(-1)),
                        _ => {
                            let window = cx.window_id();
                            cx.emit_any(Box::new(KitMsg::MenuBack { window }));
                        }
                    },
                    Key::Named(NamedKey::Escape) => {
                        if cx.state::<MenuState>().query.is_empty() {
                            self.close(cx);
                        } else {
                            self.set_query(cx, String::new());
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        let mut q = cx.state::<MenuState>().query.clone();
                        if q.pop().is_none() {
                            return Status::Handled;
                        }
                        self.set_query(cx, q);
                    }
                    Key::Character(s) if s == " " && cx.state::<MenuState>().query.is_empty() => {
                        if let Some(i) = cx.state::<MenuState>().hover {
                            self.activate_row(cx, i);
                        }
                    }
                    Key::Character(_) => {
                        let Some(typed) = k.text.clone() else { return Status::Ignored };
                        if let Some(f) = &self.look.typed {
                            // Handed over: the menus make way (now, or
                            // once whatever took the typing shows).
                            cx.emit_any(f.make(typed));
                            if !self.look.typed_stays {
                                let window = cx.window_id();
                                cx.emit_any(Box::new(KitMsg::CloseMenus { window }));
                            }
                            return Status::Handled;
                        }
                        let mut q = cx.state::<MenuState>().query.clone();
                        q.push_str(&typed);
                        self.set_query(cx, q);
                    }
                    _ => return Status::Ignored,
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Menu,
            label: Some(self.look.title.clone().unwrap_or_else(|| self.spec.title.clone())),
            ..Default::default()
        })
    }
}

/// A menu drawn in a window (a torn-off palette, a panel's list of
/// verbs): `menu_view(menu).title("Format").on_close(Msg::ClosePalette)`.
/// It behaves as the kit's popup menus do; its submenus open as popups.
pub struct MenuViewEl<Msg> {
    pub(crate) common: Common,
    menu: MenuSpec,
    look: MenuLook,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(MenuViewEl);

pub fn menu_view<Msg>(menu: Menu<Msg>) -> MenuViewEl<Msg> {
    MenuViewEl { common: Common::default(), menu: menu.spec, look: MenuLook::default(), _m: PhantomData }
}

impl<Msg> MenuViewEl<Msg> {
    /// A `menu_title` strip with `title`, a close box and (with
    /// [`pin`](MenuViewEl::pin)) a pin.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.look.title = Some(title.into());
        self
    }

    pub fn min_width(mut self, w: f64) -> Self {
        self.look.min_width = w;
        self
    }

    /// Aizome's mark in the title strip, before the title (the Toolchest).
    pub fn mark(mut self) -> Self {
        self.look.mark = true;
        self
    }
}

impl<Msg: Clone + Send + 'static> MenuViewEl<Msg> {
    /// Sent by the close box.
    pub fn on_close(mut self, msg: Msg) -> Self {
        self.look.on_close = Some(MsgFn::new(msg));
        self
    }

    /// A pin in the strip, `pinned` or not; pressing it sends `f(!pinned)`.
    pub fn pin(mut self, pinned: bool, f: impl Fn(bool) -> Msg + 'static) -> Self {
        self.look.pin = Some((pinned, MsgMap::new(f)));
        self
    }
}

impl<Msg> From<MenuViewEl<Msg>> for Element<Msg> {
    fn from(m: MenuViewEl<Msg>) -> Element<Msg> {
        let mut common = m.common;
        if common.key.is_none() {
            common.key = Some(crate::hash::hash_of(&m.menu.title));
        }
        Element::from_raw(RawElement::new(MenuView::new(m.menu, m.look), common, Vec::new()))
    }
}

/// Which title of the in-window bar is open or under the pointer.
#[derive(Default)]
pub(crate) struct BarState {
    pub open: Option<usize>,
    pub hover: Option<usize>,
}

/// The menu bar along a window's top where there's no Aizome shell: the
/// app's name in bold, then its menus.
pub(crate) struct MenuBarStrip {
    titles: Vec<String>,
    win: WinId,
    spans: Vec<(f64, f64)>,
}

impl MenuBarStrip {
    pub fn element(info: &AppInfo, menus: &[MenuSpec], win: WinId) -> RawElement {
        let mut titles: Vec<String> = menus.iter().map(|m| m.title.clone()).collect();
        if let Some(first) = titles.first_mut()
            && first.is_empty()
        {
            first.clone_from(&info.name);
        }
        let mut c = Common::sized(Length::Fill, Length::Shrink);
        c.key = Some(2);
        RawElement::new(MenuBarStrip { titles, win, spans: Vec::new() }, c, Vec::new())
    }

    fn spec(theme: &Theme, i: usize) -> TextSpec {
        let mut f = FontSpec::ui(theme.metrics.body);
        if i == 0 {
            f.weight = aizome_theme::typeface::WEIGHT_BOLD;
        }
        TextSpec::new(f, theme.text())
    }

    fn at(&self, x: f64) -> Option<usize> {
        self.spans.iter().position(|(a, b)| x >= *a && x < *b)
    }

    fn open(&self, cx: &mut EventCx, i: usize) {
        let (a, b) = self.spans[i];
        let h = cx.size().height;
        let anchor = Rect::new(a, 0.0, b, h - 1.0) + cx.window_rect().origin().to_vec2();
        cx.state::<BarState>().open = Some(i);
        cx.invalidate();
        cx.emit_any(Box::new(KitMsg::OpenBarMenu { window: self.win, index: i, anchor }));
    }
}

impl Widget for MenuBarStrip {
    fn name(&self) -> &'static str {
        "menu-bar"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let theme = cx.env.theme;
        let mut x = 3.0;
        self.spans.clear();
        for i in 0..self.titles.len() {
            let w = cx.measure(&self.titles[i], &MenuBarStrip::spec(theme, i)).width.ceil();
            self.spans.push((x, x + w + 18.0));
            x += w + 18.0;
        }
        bc.constrain(Size::new(x, theme.metrics.target))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        cx.canvas.fill_rect(r, t.bar());
        draw::bottom(cx.canvas, r, t.bevel_lo(), 1.0);
        draw::bottom(cx.canvas, Rect::new(r.x0, r.y0, r.x1, r.y1 - 1.0), t.bevel_hi(), 1.0);
        let state = cx.state::<BarState>();
        let (open, hover) = state.map_or((None, None), |s| (s.open, s.hover));
        for (i, (a, b)) in self.spans.iter().enumerate() {
            let cell = Rect::new(*a, 0.0, *b, r.y1 - 2.0);
            let fg = if open == Some(i) {
                cx.canvas.fill_rect(cell, t.selection());
                t.on_selection()
            } else {
                if hover == Some(i) {
                    cx.canvas.fill_rect(cell, t.hover(t.bar()));
                }
                t.text()
            };
            let spec = TextSpec { color: fg, ..MenuBarStrip::spec(t, i) };
            cx.draw_text_in(&self.titles[i], &spec, cell, Align::Center);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for t in &self.titles {
            h.str(t);
        }
    }

    fn hit(&self) -> bool {
        true
    }

    fn solid(&self) -> bool {
        true
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                match self.at(pos.x) {
                    Some(i) if cx.state::<BarState>().open == Some(i) => {
                        cx.state::<BarState>().open = None;
                        cx.invalidate();
                        cx.emit_any(Box::new(KitMsg::CloseMenus { window: self.win }));
                    }
                    Some(i) => self.open(cx, i),
                    None => {}
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let at = self.at(pos.x);
                let st = cx.state::<BarState>();
                if st.hover != at {
                    st.hover = at;
                    let open = st.open;
                    cx.invalidate();
                    if let (Some(o), Some(i)) = (open, at)
                        && o != i
                    {
                        self.open(cx, i);
                    }
                }
                Status::Handled
            }
            Event::PointerLeave => {
                cx.state::<BarState>().hover = None;
                cx.invalidate();
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::MenuBar, actions: vec![SemanticAction::ShowMenu], ..Default::default() })
    }
}

/// A tooltip: a dark label, as the boards' drag hint.
pub(crate) struct TooltipView {
    text: String,
}

impl TooltipView {
    pub fn element(text: &str) -> RawElement {
        RawElement::new(TooltipView { text: text.to_owned() }, Common::default(), Vec::new())
    }

    fn spec(theme: &Theme) -> TextSpec {
        let mut f = FontSpec::ui(theme.metrics.small + 1.0);
        f.weight = aizome_theme::typeface::WEIGHT_SEMIBOLD;
        TextSpec::new(f, theme.on_menu_title())
    }
}

impl Widget for TooltipView {
    fn name(&self) -> &'static str {
        "tooltip"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let spec = TooltipView::spec(cx.env.theme);
        let s = cx.measure(&self.text, &spec);
        bc.constrain(Size::new((s.width + 16.0).ceil().min(420.0), (s.height + 8.0).ceil().max(22.0)))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        cx.canvas.fill_rect(r, t.menu_title());
        draw::border(cx.canvas, r, t.bevel_lo(), 1.0);
        let spec = TooltipView::spec(t);
        cx.draw_text_in(&self.text, &spec, r.inset((-8.0, 0.0)), Align::Start);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.text);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Tooltip, label: Some(self.text.clone()), ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(look: MenuLook) -> MenuView {
        MenuView::new(MenuSpec { title: "Editor".into(), ..MenuSpec::default() }, look)
    }

    #[test]
    fn title_strips_are_drawn_as_the_menus_board_draws_them() {
        let tear = || Some(MsgFn::new(()));
        // The menu at the pointer: its name alone (its strip still tears it off).
        let at_pointer = view(MenuLook { title: Some("Editor".into()), tear_off: tear(), ..MenuLook::default() });
        assert!(!at_pointer.has_close() && !at_pointer.has_pin());
        // One of its submenus: a pin to tear it off, no close box.
        let sub = view(MenuLook { title: Some("Edit".into()), tear_off: tear(), is_sub: true, ..MenuLook::default() });
        assert!(!sub.has_close() && sub.has_pin());
        // A palette: a close box and its pin.
        let palette = view(MenuLook {
            title: Some("Toolchest".into()),
            on_close: Some(MsgFn::new(())),
            pin: Some((true, MsgMap::new(|_: bool| ()))),
            ..MenuLook::default()
        });
        assert!(palette.has_close() && palette.has_pin());
    }
}
