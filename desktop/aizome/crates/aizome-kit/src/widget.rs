//! The widget trait, and what a widget is given to lay out, draw and
//! handle input with.
//!
//! A widget is the part of an element that does something: it sizes
//! itself and places its children ([`Widget::layout`]), draws in its own
//! coordinates ([`Widget::paint`]), and answers input
//! ([`Widget::event`]). Widgets are rebuilt with every view; what must
//! outlive a rebuild (a caret, a scroll offset, a pressed state) lives in
//! the window's state map under the widget's key path, reached through
//! `cx.state::<T>()`.

use std::any::{Any, TypeId};
use std::time::{Duration, Instant};

use sidestep_ui::{Canvas, Cursor, Ime, KeyEvent, Modifiers, PointerButton, ScrollPhase, TextLayout};

use crate::element::{AnyMsg, Common, MsgFn, RawElement};
use crate::hash::{FxHashMap, PaintHasher};
use crate::layout::{Align, Constraints, Point, Rect, Size, Vec2};
use crate::menu::MenuSpec;
use crate::platform::Platform;
use crate::text::{TextCache, TextSpec};
use crate::theme::Theme;
use crate::tree::{NodeId, Tree};

/// Something that happened, as a widget hears it: positions are in the
/// widget's own coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    PointerMove {
        pos: Point,
        mods: Modifiers,
    },
    PointerDown {
        pos: Point,
        button: PointerButton,
        clicks: u32,
        mods: Modifiers,
    },
    PointerUp {
        pos: Point,
        button: PointerButton,
        mods: Modifiers,
    },
    /// The pointer left the widget (it was the one under the pointer).
    PointerLeave,
    /// Scrolling over the widget, in points (wheel lines already turned
    /// into points), positive toward the content's end.
    Scroll {
        pos: Point,
        delta: Vec2,
        phase: ScrollPhase,
        momentum: ScrollPhase,
        mods: Modifiers,
    },
    /// A two-finger pinch over the widget: `scale` is the change since
    /// the last one, as a factor (1.1 is ten per cent larger).
    Pinch {
        pos: Point,
        scale: f64,
        phase: ScrollPhase,
        mods: Modifiers,
    },
    Key(KeyEvent),
    Ime(Ime),
    /// The widget got (true) or lost the keyboard focus.
    Focus(bool),
    /// The window got or lost the keyboard.
    WindowFocus(bool),
    /// A tick the widget asked for came due ([`EventCx::request_tick`]).
    Tick,
    /// A keyed, one-shot timer set by this widget. Keys are scoped to its node.
    Timer(u32),
    /// Files or text dropped on the widget.
    Drop {
        pos: Point,
        urls: Vec<String>,
        text: Option<String>,
    },
    /// A drag hovers over the widget: whether it would take it.
    DragOver {
        pos: Point,
        urls: Vec<String>,
        has_text: bool,
    },
}

/// Whether a widget used an event; unused ones go on to its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ignored,
    Handled,
}

/// How a widget takes the keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focusable {
    No,
    /// By Tab (buttons, toggles: macOS's full keyboard access).
    Tab,
    /// By Tab and by a click (text, lists, sliders).
    Click,
    /// By a click only, not Tab (a scroll view, for its keys).
    Pointer,
}

/// What the Edit menu asks of the focused widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteAsPlainText,
    Delete,
    SelectAll,
    Find,
    FindNext,
    FindPrevious,
    UseSelectionForFind,
    Replace,
}

/// What assistive technology is told about a widget (AccessKit's model:
/// role, label, value, actions).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Semantics {
    pub role: SemanticRole,
    pub label: Option<String>,
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub selected: Option<bool>,
    pub disabled: bool,
    pub expanded: Option<bool>,
    /// For sliders, progress and meters.
    pub range: Option<(f64, f64, f64)>,
    pub actions: Vec<SemanticAction>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SemanticRole {
    #[default]
    Generic,
    Window,
    Label,
    Button,
    CheckBox,
    Switch,
    RadioButton,
    Slider,
    SpinButton,
    ComboBox,
    TextInput,
    MultilineTextInput,
    SearchInput,
    PasswordInput,
    List,
    ListItem,
    Table,
    Row,
    Cell,
    ColumnHeader,
    Tree,
    TreeItem,
    TabList,
    Tab,
    ProgressIndicator,
    Meter,
    Image,
    Menu,
    MenuBar,
    MenuItem,
    Toolbar,
    StatusBar,
    ScrollView,
    Group,
    Dialog,
    Tooltip,
    Splitter,
    Canvas,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticAction {
    Click,
    Focus,
    Increment,
    Decrement,
    SetValue,
    Expand,
    Collapse,
    ShowMenu,
    ScrollIntoView,
}

/// A piece of interface that lays out, draws and answers input. See the
/// module's documentation.
pub trait Widget: std::any::Any {
    /// Its kind, for identity among unkeyed siblings and for debugging.
    fn name(&self) -> &'static str;

    /// Choose a size within `bc` and place the children. The default
    /// stacks the children at the top left and takes the largest of them.
    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let mut size = Size::ZERO;
        for i in 0..cx.child_count() {
            let s = cx.layout_child(i, bc.loosen());
            cx.place_child(i, Point::ZERO);
            size = Size::new(size.width.max(s.width), size.height.max(s.height));
        }
        bc.constrain(size)
    }

    /// Draw itself (under its children), from its top left.
    fn paint(&self, cx: &mut PaintCx) {
        let _ = cx;
    }

    /// Draw over its children (scroll bars, overlays).
    fn paint_over(&self, cx: &mut PaintCx) {
        let _ = cx;
    }

    /// Feed what its drawing depends on (besides its size, hover, press
    /// and focus) into `h`: a widget whose hash and rectangle didn't change
    /// isn't redrawn.
    fn hash(&self, h: &mut PaintHasher) {
        let _ = h;
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let _ = (cx, event);
        Status::Ignored
    }

    /// Whether it takes the pointer: hover, presses, its cursor.
    fn hit(&self) -> bool {
        false
    }

    /// Whether the pointer stops at it even if it doesn't take it (a
    /// filled panel over what's behind it).
    fn solid(&self) -> bool {
        self.hit()
    }

    /// Whether it takes only drags with the left button (an edge to resize
    /// by, over rows that take clicks). A left press that ends without a
    /// drag reaches what's under it as a click, once released. Other
    /// buttons, the wheel and drops go straight there. It still shows its
    /// cursor and hover, and what's under it shows its hover and tooltip
    /// too. Its press ends if the pointer leaves the window meanwhile (the
    /// compositor took it: the release won't come).
    fn drags_only(&self) -> bool {
        false
    }

    /// Whether hovering it changes how it draws.
    fn hover_paints(&self) -> bool {
        self.hit()
    }

    fn focusable(&self) -> Focusable {
        Focusable::No
    }

    /// Whether its children are clipped to it.
    fn clips(&self) -> bool {
        false
    }

    /// How far outside its rectangle it draws (a shadow).
    fn overflow(&self) -> f64 {
        0.0
    }

    /// The pointer's shape over it.
    fn cursor(&self) -> Option<Cursor> {
        None
    }

    /// Whether it blocks what's under it (a sheet): the pointer and the
    /// focus stay inside it.
    fn modal(&self) -> bool {
        false
    }

    fn semantics(&self) -> Option<Semantics> {
        None
    }

    /// The video it shows, if it's a video's place (see
    /// [`widgets::video`](crate::widgets::video)), as last laid out: the
    /// kit places the window's video there.
    fn video(&self) -> Option<crate::widgets::video::VideoSpec> {
        None
    }

    /// Whether it's drawn over the window's videos ([`over_video`](crate::widgets::video::over_video)):
    /// a video it overlaps goes under the window's drawing.
    fn over_video(&self) -> bool {
        false
    }

    /// Carry out an Edit menu command (it has the focus): whether it did.
    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        let _ = (cx, command);
        false
    }

    /// Whether it can carry out `command` now (for the menu's state).
    fn can(&self, state: StateView, command: EditCommand) -> bool {
        let _ = (state, command);
        false
    }

    /// Activate it as Space or Return would, or as a sheet's default
    /// button answers Return.
    fn activate(&mut self, cx: &mut EventCx) -> bool {
        let _ = cx;
        false
    }

    /// Answers Return in a sheet.
    fn is_default(&self) -> bool {
        false
    }

    /// Answers Escape in a sheet.
    fn is_cancel(&self) -> bool {
        false
    }

    /// Scroll so `rect` (in its content's coordinates, through its
    /// children) shows: scroll views do.
    fn reveal(&mut self, cx: &mut EventCx, rect: Rect) -> bool {
        let _ = (cx, rect);
        false
    }

    /// A popover's content, taken when it's mounted: the kit shows it in a
    /// popup window while open.
    fn take_popover(&mut self) -> Option<PopoverContent> {
        None
    }

    /// The text it holds, for type-select and tests.
    fn text(&self) -> Option<&str> {
        None
    }

    /// Parts of it that aren't elements, each by its name, in its own
    /// coordinates (a segmented control's segments, the shapes on a
    /// canvas), for tests that drive the app from outside
    /// ([`crate::inspect`]).
    fn parts(&self) -> Vec<(String, Rect)> {
        Vec::new()
    }

    /// When the kit draws the focus ring around it.
    fn focus_ring(&self) -> FocusRing {
        FocusRing::Keyboard
    }

    /// The menu it shows (a menu view): the kit opens its submenus and
    /// carries out its items.
    fn menu(&self) -> Option<&MenuSpec> {
        None
    }

    /// The title strip of the menu it shows, if it has one.
    fn menu_title(&self) -> Option<&str> {
        None
    }

    /// The text selected in it (for Services and tests).
    fn selection(&self, state: StateView) -> Option<String> {
        let _ = state;
        None
    }

    /// Shares of a row's or column's leftover room along its axis,
    /// whatever its own lengths say (a spacer).
    fn flex(&self) -> Option<u16> {
        None
    }
}

/// When the focus ring shows around a focused widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusRing {
    /// Whenever it has the focus (text fields).
    Always,
    /// When the focus came from the keyboard (buttons, lists).
    Keyboard,
    /// Never (a text editor's whole area).
    Never,
}

/// A popover's content and how it shows.
pub struct PopoverContent {
    pub content: RawElement,
    pub open: bool,
    pub on_dismiss: Option<MsgFn>,
}

/// What a window keeps for its widgets between rebuilds.
pub(crate) type StateMap = FxHashMap<(NodeId, TypeId), Box<dyn Any>>;

/// A widget's states, to read: [`StateView::get`].
#[derive(Clone, Copy)]
pub struct StateView<'a> {
    pub(crate) map: &'a StateMap,
    pub(crate) id: NodeId,
}

impl<'a> StateView<'a> {
    pub(crate) fn new(map: &'a StateMap, id: NodeId) -> StateView<'a> {
        StateView { map, id }
    }

    pub fn get<T: 'static>(&self) -> Option<&'a T> {
        self.map.get(&(self.id, TypeId::of::<T>())).and_then(|b| b.downcast_ref::<T>())
    }
}

/// What layout, painting and events share.
pub(crate) struct Env<'a> {
    pub scale: f64,
    pub theme: &'a Theme,
    pub text: &'a mut TextCache,
    pub states: &'a mut StateMap,
    pub now: Instant,
    /// Laying out a selected row: text and icons take `on_selection`.
    pub on_selection: bool,
    /// The focused widget, while its window has the keyboard: a list
    /// with it draws its selection strong.
    pub focus: Option<NodeId>,
}

pub(crate) fn state_of<T: Default + 'static>(states: &mut StateMap, id: NodeId) -> &mut T {
    let entry = states.entry((id, TypeId::of::<T>())).or_insert_with(|| Box::new(T::default()));
    entry.downcast_mut::<T>().expect("state of its type")
}

/// What a widget lays out with: its children, its state, the theme and
/// the text cache.
pub struct LayoutCx<'a, 'e> {
    pub(crate) tree: &'a mut Tree,
    pub(crate) node: usize,
    pub(crate) env: &'a mut Env<'e>,
}

impl LayoutCx<'_, '_> {
    /// Device pixels per logical layout unit for this window.
    pub fn scale(&self) -> f64 {
        self.env.scale
    }
    pub fn child_count(&self) -> usize {
        self.tree.nodes[self.node].children.len()
    }

    /// Lay out child `i` within `bc` (narrowed by its own width and
    /// height): its size.
    pub fn layout_child(&mut self, i: usize, bc: Constraints) -> Size {
        let child = self.tree.nodes[self.node].children[i];
        self.tree.layout(child, bc, self.env)
    }

    /// Put child `i`'s top left at `at`, in this widget's coordinates.
    pub fn place_child(&mut self, i: usize, at: Point) {
        let child = self.tree.nodes[self.node].children[i];
        self.tree.nodes[child].local = at;
    }

    /// Child `i`'s size, as last laid out.
    pub fn child_size(&self, i: usize) -> Size {
        self.tree.nodes[self.tree.nodes[self.node].children[i]].size
    }

    /// Child `i`'s width and height settings.
    pub fn child_common(&self, i: usize) -> &Common {
        &self.tree.nodes[self.tree.nodes[self.node].children[i]].common
    }

    /// Add a child while laying out (a virtual list's rows): its index.
    pub fn add_child(&mut self, element: RawElement) -> usize {
        let i = self.child_count();
        let child = self.tree.mount(element, Some(self.node), i);
        self.tree.nodes[self.node].children.push(child);
        i
    }

    /// Lay child `i` out as a selected row: its text and icons take the
    /// colour on the selection.
    pub fn layout_child_selected(&mut self, i: usize, bc: Constraints, selected: bool) -> Size {
        let before = std::mem::replace(&mut self.env.on_selection, selected);
        let child = self.tree.nodes[self.node].children[i];
        if before != selected {
            self.tree.uncache(child);
        }
        let s = self.tree.layout(child, bc, self.env);
        self.env.on_selection = before;
        s
    }

    /// Whether this widget has the focus in the window that has the
    /// keyboard: lists then lay their selected rows out on `selection`
    /// (and otherwise on `line2`, in the ordinary text colours).
    pub fn selection_active(&self) -> bool {
        self.env.focus.is_some_and(|f| f == self.tree.nodes[self.node].id)
    }

    /// This widget's content in `rect` (its coordinates) moved by `delta`
    /// since it was last laid out: it scrolled. The kit moves those pixels
    /// on screen rather than drawing them again, where it can, and draws
    /// `fixed` (what stays put over the content: bars, a header) again.
    /// Lay the content out where it now is; what changed in it is damaged
    /// as always. A widget that calls this leaves its scroll offset out of
    /// its hash.
    pub fn scrolled(&mut self, rect: Rect, delta: Vec2, fixed: &[Rect]) {
        self.tree.scrolls.push(crate::tree::Scrolled { node: self.node, rect, delta, fixed: fixed.to_vec() });
    }

    /// Whether this is laid out in a selected row.
    pub fn on_selection(&self) -> bool {
        self.env.on_selection
    }

    /// Drop the children added while laying out before.
    pub fn clear_children(&mut self) {
        let kids = std::mem::take(&mut self.tree.nodes[self.node].children);
        for k in kids {
            self.tree.kill(k);
        }
    }

    /// This widget's persistent state.
    pub fn state<T: Default + 'static>(&mut self) -> &mut T {
        let id = self.tree.nodes[self.node].id;
        state_of(self.env.states, id)
    }

    pub fn id(&self) -> NodeId {
        self.tree.nodes[self.node].id
    }

    pub fn theme(&self) -> &Theme {
        self.env.theme
    }

    pub fn text_cache(&mut self) -> &mut TextCache {
        self.env.text
    }

    pub fn now(&self) -> Instant {
        self.env.now
    }

    /// Hear [`Event::Tick`] after `after` (an animation starting).
    pub fn request_tick(&mut self, after: Duration) {
        let id = self.id();
        self.tree.ticks.push((id, after));
    }

    pub fn is_focused(&self) -> bool {
        self.env.focus == Some(self.id())
    }

    /// Replace this widget's timer at `key`. Independent of layout ticks.
    pub fn set_timer(&mut self, key: u32, after: Duration) {
        self.tree.widget_timers.push((self.id(), key, Some(after)));
    }
    pub fn cancel_timer(&mut self, key: u32) {
        self.tree.widget_timers.push((self.id(), key, None));
    }

    /// Redraw `rect` (its own coordinates): what a widget whose drawing
    /// it diffs itself (an editor's lines) found changed.
    pub fn invalidate_rect(&mut self, rect: Rect) {
        self.tree.local_damage.push((self.node, rect));
    }

    /// How much room one line of `text` set as `spec` takes.
    pub fn measure(&mut self, text: &str, spec: &TextSpec) -> Size {
        let theme = self.env.theme;
        self.env.text.layout(theme, text, spec, None).size()
    }
}

/// What a widget draws with: the canvas (moved to its top left), its
/// size, whether it's hovered, pressed or focused, its state, the theme.
pub struct PaintCx<'a> {
    pub canvas: &'a mut Canvas,
    pub(crate) theme: &'a Theme,
    pub(crate) text: &'a mut TextCache,
    pub(crate) state: StateView<'a>,
    pub(crate) size: Size,
    pub(crate) hovered: bool,
    pub(crate) pressed: bool,
    pub(crate) focused: bool,
    pub(crate) focus_visible: bool,
    pub(crate) window_active: bool,
    pub(crate) now: Instant,
}

impl<'a> PaintCx<'a> {
    pub fn theme(&self) -> &'a Theme {
        self.theme
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Its rectangle, in its own coordinates.
    pub fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// Held down by the pointer (and the pointer still over it).
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Focused, and the focus came from the keyboard: show it.
    pub fn focus_visible(&self) -> bool {
        self.focused && self.focus_visible
    }

    /// Whether its window has the keyboard.
    pub fn window_active(&self) -> bool {
        self.window_active
    }

    pub fn state<T: 'static>(&self) -> Option<&'a T> {
        self.state.get::<T>()
    }

    pub fn text_cache(&mut self) -> &mut TextCache {
        self.text
    }

    pub fn now(&self) -> Instant {
        self.now
    }

    /// `text` set as `spec` (through the cache).
    pub fn layout_text(&mut self, text: &str, spec: &TextSpec, width: Option<f64>) -> TextLayout {
        self.text.layout(self.theme, text, spec, width)
    }

    /// Draw one line of `text` in `rect`, centred down it and placed
    /// across it by `align`, ending in an ellipsis if it doesn't fit: the
    /// width it took.
    pub fn draw_text_in(&mut self, text: &str, spec: &TextSpec, rect: Rect, align: Align) -> f64 {
        if text.is_empty() || rect.width() <= 0.0 {
            return 0.0;
        }
        let mut l = self.text.layout(self.theme, text, spec, None);
        if l.size().width > rect.width() + 0.5 {
            l = self.text.layout(self.theme, text, spec, Some(rect.width().max(1.0)));
        }
        let s = l.size();
        let x = rect.x0 + align.offset(s.width.min(rect.width()), rect.width());
        let y = rect.y0 + ((rect.height() - s.height) / 2.0).round();
        self.canvas.draw_text(&l, Point::new(x.round(), y));
        s.width.min(rect.width())
    }
}

/// What an event handler does: emit messages, redraw or rebuild, move the
/// focus, take the pointer, ask for ticks, open menus.
pub struct EventCx<'a, 'e> {
    pub(crate) tree: &'a Tree,
    pub(crate) node: usize,
    pub(crate) win: crate::platform::WinId,
    pub(crate) env: &'a mut Env<'e>,
    pub(crate) out: &'a mut Out,
    pub(crate) platform: &'a mut dyn Platform,
    pub(crate) hovered: bool,
    pub(crate) pressed: bool,
    pub(crate) focused: bool,
    /// Whether anything in the window has the focus.
    pub(crate) any_focus: bool,
    pub(crate) mods: Modifiers,
}

/// What handling an event asked for, done by the engine once it returns.
#[derive(Default)]
pub(crate) struct Out {
    pub msgs: Vec<(usize, AnyMsg)>,
    pub damage: Vec<Rect>,
    pub rebuild: bool,
    pub relayout: bool,
    pub focus: Option<FocusRequest>,
    pub capture: Option<bool>,
    pub cursor: Option<Cursor>,
    pub ime: Option<(bool, Option<Rect>)>,
    pub ticks: Vec<(NodeId, Duration)>,
    pub widget_timers: Vec<(NodeId, u32, Option<Duration>)>,
    pub menus: Vec<MenuRequest>,
    pub close_popup: bool,
    pub reveal: Option<(usize, Rect)>,
    pub begin_move: bool,
    pub drag: Option<sidestep_ui::DragSource>,
    pub handled_by: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FocusRequest {
    Node(NodeId),
    Clear,
    Next,
    Previous,
}

/// A menu a widget opens (a pop-up button's, a context menu).
pub(crate) struct MenuRequest {
    pub menu: MenuSpec,
    /// Where it opens from, in window coordinates.
    pub anchor: Rect,
    pub at_point: bool,
    /// The node that opened it.
    pub opener: usize,
    /// For a pop-up button: the minimum width (its own).
    pub min_width: f64,
}

impl EventCx<'_, '_> {
    /// Send `msg` to the program (through the mappers of the elements
    /// around this one).
    pub fn emit(&mut self, msg: &MsgFn) {
        self.out.msgs.push((self.node, msg.make()));
    }

    pub(crate) fn emit_any(&mut self, msg: AnyMsg) {
        self.out.msgs.push((self.node, msg));
    }

    pub fn id(&self) -> NodeId {
        self.tree.nodes[self.node].id
    }

    /// The window (or popup) it's in, as the engine names it.
    pub fn window_id(&self) -> crate::platform::WinId {
        self.win
    }

    /// Its rectangle in the window.
    pub fn window_rect(&self) -> Rect {
        self.tree.nodes[self.node].rect
    }

    pub fn size(&self) -> Size {
        self.tree.nodes[self.node].size
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size())
    }

    /// Redraw it.
    pub fn invalidate(&mut self) {
        let n = &self.tree.nodes[self.node];
        self.out.damage.push(n.rect.inflate(3.0, 3.0).intersect(n.clip.inflate(3.0, 3.0)));
    }

    /// Redraw part of it (its own coordinates).
    pub fn invalidate_rect(&mut self, r: Rect) {
        let n = &self.tree.nodes[self.node];
        self.out.damage.push((r + n.rect.origin().to_vec2()).intersect(n.clip));
    }

    /// View and lay the window out again (its state changed its layout).
    pub fn rebuild(&mut self) {
        self.out.rebuild = true;
    }

    /// Lay it out again (and what's around it) without viewing the window
    /// again: its own state moved its children (a scroll).
    pub fn relayout(&mut self) {
        self.out.relayout = true;
    }

    pub fn state<T: Default + 'static>(&mut self) -> &mut T {
        let id = self.id();
        state_of(self.env.states, id)
    }

    pub fn theme(&self) -> &Theme {
        self.env.theme
    }

    pub fn text_cache(&mut self) -> &mut TextCache {
        self.env.text
    }

    pub fn now(&self) -> Instant {
        self.env.now
    }

    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// This widget owns keyboard focus in the active window.
    pub fn has_keyboard_focus(&self) -> bool {
        self.env.focus == Some(self.id())
    }

    /// Whether nothing in the window has the focus (keys then go to what's
    /// under the pointer).
    pub fn nothing_focused(&self) -> bool {
        !self.any_focus
    }

    /// The modifier keys held.
    pub fn modifiers(&self) -> Modifiers {
        self.mods
    }

    pub fn request_focus(&mut self) {
        self.out.focus = Some(FocusRequest::Node(self.id()));
    }

    pub fn resign_focus(&mut self) {
        self.out.focus = Some(FocusRequest::Clear);
    }

    pub fn focus_next(&mut self) {
        self.out.focus = Some(FocusRequest::Next);
    }

    pub fn focus_previous(&mut self) {
        self.out.focus = Some(FocusRequest::Previous);
    }

    /// Keep getting the pointer's moves and its release, wherever it goes.
    pub fn capture_pointer(&mut self) {
        self.out.capture = Some(true);
    }

    pub fn release_pointer(&mut self) {
        self.out.capture = Some(false);
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.out.cursor = Some(cursor);
    }

    /// Take text from input methods, with the caret at `caret` (its own
    /// coordinates), or stop.
    pub fn set_text_input(&mut self, wanted: bool, caret: Option<Rect>) {
        let origin = self.tree.nodes[self.node].rect.origin().to_vec2();
        self.out.ime = Some((wanted, caret.map(|r| r + origin)));
    }

    /// Hear [`Event::Tick`] after `after`.
    pub fn request_tick(&mut self, after: Duration) {
        let id = self.id();
        self.out.ticks.push((id, after));
    }

    /// Replace this widget's one-shot timer at `key`. Only one is pending per
    /// key; it is suspended when hidden and cancelled if the widget disappears.
    pub fn set_timer(&mut self, key: u32, after: Duration) {
        self.out.widget_timers.push((self.id(), key, Some(after)));
    }
    pub fn cancel_timer(&mut self, key: u32) {
        self.out.widget_timers.push((self.id(), key, None));
    }

    pub fn clipboard_text(&mut self) -> Option<String> {
        self.platform.clipboard_text()
    }

    pub fn set_clipboard_text(&mut self, text: &str) {
        self.platform.set_clipboard_text(text);
    }

    /// Open `menu` below `anchor` (its own coordinates), at least
    /// `min_width` wide.
    pub fn open_menu(&mut self, menu: MenuSpec, anchor: Rect, min_width: f64) {
        let origin = self.tree.nodes[self.node].rect.origin().to_vec2();
        self.out.menus.push(MenuRequest {
            menu,
            anchor: anchor + origin,
            at_point: false,
            opener: self.node,
            min_width,
        });
    }

    /// Open `menu` at `at` (its own coordinates), as a context menu.
    pub fn open_context_menu(&mut self, menu: MenuSpec, at: Point) {
        let p = at + self.tree.nodes[self.node].rect.origin().to_vec2();
        let anchor = Rect::from_origin_size(p, (1.0, 1.0));
        self.out.menus.push(MenuRequest { menu, anchor, at_point: true, opener: self.node, min_width: 0.0 });
    }

    /// Close the popup this widget is in.
    pub fn close_popup(&mut self) {
        self.out.close_popup = true;
    }

    /// Have the scroll views around it show `rect` (its own coordinates).
    pub fn reveal(&mut self, rect: Rect) {
        let origin = self.tree.nodes[self.node].rect.origin().to_vec2();
        self.out.reveal = Some((self.node, rect + origin));
    }

    /// Move the window with the pointer, from the press under way.
    pub fn begin_window_move(&mut self) {
        self.out.begin_move = true;
    }

    /// Start dragging `source` out of the window, from the press under
    /// way (see [`Cx::start_drag`](crate::Cx::start_drag)): the program
    /// hears how it ended in `drag_ended`.
    pub fn start_drag(&mut self, source: sidestep_ui::DragSource) {
        self.out.drag = Some(source);
    }
}

/// A placeholder while a widget is out of the tree being laid out.
pub(crate) struct Vacant;

impl Widget for Vacant {
    fn name(&self) -> &'static str {
        "vacant"
    }
}
