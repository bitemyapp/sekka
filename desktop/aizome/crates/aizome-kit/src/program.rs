//! The program: an app's state, how messages change it, and how it looks.

use std::path::PathBuf;

use crate::cx::Cx;
use crate::element::Element;
use crate::icons::Icon;
use crate::layout::{Rect, Size};
use crate::menu::MenuBar;
pub use sidestep_ui::Output;
pub use sidestep_ui::layer_shell::{Anchors, KeyboardInteractivity, Layer, LayerOptions, Margins};

/// A request for an activation token ([`Cx::request_activation_token`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActivationRequest(pub(crate) u64);

/// A drag out of one of the program's windows: [`Cx::start_drag`]'s, or
/// a list's, table's or mouse area's `drag_out`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DragId(pub(crate) u64);

/// An Aizome app, Elm's way: state that [`update`](Program::update)
/// changes with messages, and a [`view`](Program::view) of each window
/// drawn from it.
///
/// ```no_run
/// use aizome_kit::prelude::*;
/// use aizome_kit::column; // std has a `column!` too: name ours
///
/// #[derive(Default)]
/// struct Counter { count: i64 }
///
/// #[derive(Clone, Debug)]
/// enum Msg { Add }
///
/// impl Program for Counter {
///     type Msg = Msg;
///     fn info(&self) -> AppInfo {
///         AppInfo::new("org.aizome.Counter", "Counter", "Counter").icon(Icon::Calculator)
///     }
///     fn launched(&mut self, cx: &mut Cx<Msg>) {
///         cx.open_window(WindowSpec::new("Counter").size(320.0, 200.0));
///     }
///     fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
///         match msg { Msg::Add => self.count += 1 }
///     }
///     fn view(&self, _window: WindowKey) -> Element<Msg> {
///         column![
///             text(format!("{}", self.count)).size(28.0).tabular(),
///             button("Add").primary().on_press(Msg::Add),
///         ].spacing(12.0).padding(16.0).into()
///     }
/// }
///
/// fn main() { aizome_kit::run(Counter::default()) }
/// ```
pub trait Program: Sized + 'static {
    type Msg: Clone + Send + 'static;

    /// Who the app is: its id, names and icon.
    fn info(&self) -> AppInfo;

    /// The app started: open the first window(s).
    fn launched(&mut self, cx: &mut Cx<Self::Msg>);

    fn update(&mut self, msg: Self::Msg, cx: &mut Cx<Self::Msg>);

    /// What `window` shows now.
    fn view(&self, window: WindowKey) -> Element<Self::Msg>;

    /// The app's menus, in the global menu bar.
    fn menus(&self) -> MenuBar<Self::Msg> {
        MenuBar::standard(&self.info())
    }

    /// The user asked to close `window`: whether it closes. (The kit
    /// closes it; ask first with a sheet and close it later if not.)
    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Self::Msg>) -> bool {
        let _ = (window, cx);
        true
    }

    /// Files to open, from the command line (or the desktop).
    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Self::Msg>) {
        let _ = (paths, cx);
    }

    /// The outputs (screens) came, went or changed, or were described for
    /// the first time: [`Cx::outputs`] has them. (A shell opens a menu bar
    /// and a desktop on each.)
    fn outputs_changed(&mut self, cx: &mut Cx<Self::Msg>) {
        let _ = cx;
    }

    /// The pointer entered or left the window, independently of its widgets.
    /// Useful for transient shell surfaces whose children handle pointer events.
    fn pointer_presence(&mut self, window: WindowKey, inside: bool, cx: &mut Cx<Self::Msg>) {
        let _ = (window, inside, cx);
    }

    /// The compositor locked the session, as [`Cx::lock_session`] asked:
    /// only the lock's windows show until [`Cx::unlock_session`].
    fn session_locked(&mut self, cx: &mut Cx<Self::Msg>) {
        let _ = cx;
    }

    /// The lock ended without [`Cx::unlock_session`] (the compositor
    /// refused it, or another program holds one); its windows are closed.
    fn session_lock_finished(&mut self, cx: &mut Cx<Self::Msg>) {
        let _ = cx;
    }

    /// The activation token asked for with
    /// [`Cx::request_activation_token`], or none (no xdg-activation).
    fn activation_token(&mut self, request: ActivationRequest, token: Option<String>, cx: &mut Cx<Self::Msg>) {
        let _ = (request, token, cx);
    }

    /// A window's state changed: full screen (the program's asking, or the
    /// desktop's ⌃❖F), maximised. The kit views the window again after.
    fn window_state(&mut self, window: WindowKey, state: WindowState, cx: &mut Cx<Self::Msg>) {
        let _ = (window, state, cx);
    }

    /// A native drag successfully started from `window` (or one of its
    /// popups). Its matching [`Program::drag_ended`] reports completion or
    /// cancellation; a rejected start does not call this hook.
    fn drag_started(&mut self, window: WindowKey, drag: DragId, cx: &mut Cx<Self::Msg>) {
        let _ = (window, drag, cx);
    }

    /// A destination asks for the data of drag `drag` promised as `mime`
    /// ([`DragSource::promise`](sidestep_ui::DragSource::promise)): it, or
    /// none (the default).
    fn provide_drag_data(&mut self, drag: DragId, mime: &str, cx: &mut Cx<Self::Msg>) -> Option<Vec<u8>> {
        let _ = (drag, mime, cx);
        None
    }

    /// Drag `drag` out of a window is over: dropped to copy or to move
    /// what it carries (a move is the program's to finish, by removing
    /// what moved), or cancelled ([`DropAction::None`](sidestep_ui::DropAction::None)).
    fn drag_ended(&mut self, drag: DragId, action: sidestep_ui::DropAction, cx: &mut Cx<Self::Msg>) {
        let _ = (drag, action, cx);
    }

    /// Whether the app stops when its last window closes: apps do; a
    /// shell, whose surfaces come and go with the outputs, doesn't.
    fn quit_when_last_window_closes(&self) -> bool {
        true
    }

    /// Whether the app's menus go to the Aizome shell's bar (when there's
    /// one, at `$AIZOME_SHELL_SOCKET`): apps' do; the shell's own process
    /// says no, or it would publish to itself.
    fn menus_to_shell(&self) -> bool {
        true
    }
}

/// What the desktop has done with a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WindowState {
    pub fullscreen: bool,
    pub maximized: bool,
}

/// Who an app is.
#[derive(Clone, Debug, PartialEq)]
pub struct AppInfo {
    /// Reverse-DNS, as its desktop file is named: `org.aizome.Soroban`.
    pub app_id: String,
    /// Its own name: `Soroban`.
    pub name: String,
    /// What it is: `Calculator`.
    pub generic_name: String,
    pub icon: Option<Icon>,
    pub version: Option<String>,
}

impl AppInfo {
    pub fn new(app_id: impl Into<String>, name: impl Into<String>, generic_name: impl Into<String>) -> AppInfo {
        AppInfo {
            app_id: app_id.into(),
            name: name.into(),
            generic_name: generic_name.into(),
            icon: None,
            version: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> AppInfo {
        self.icon = Some(icon);
        self
    }

    pub fn version(mut self, version: impl Into<String>) -> AppInfo {
        self.version = Some(version.into());
        self
    }
}

/// A window of the app (or a sheet), as long as it's open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowKey(pub(crate) u64);

impl WindowKey {
    /// A key that names no window (before the first opens).
    pub const NONE: WindowKey = WindowKey(0);
}

/// What kind of window.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum WindowKind {
    /// A document or main window.
    #[default]
    Normal,
    /// Kept over `parent` (a dialog of its own).
    Dialog { parent: WindowKey },
    /// A small floating window (a palette, an inspector).
    Utility,
    /// A desktop surface (wlr-layer-shell): a panel, a menu bar, a
    /// wallpaper, an on-screen display. Its size is the spec's, a side of 0
    /// where it's anchored to both edges that way being the compositor's
    /// to choose. Move or resize it with `Cx::set_layer_…`.
    Layer(LayerOptions),
    /// The session lock's surface on an output (an [`Output`]'s id), while
    /// [`Cx::lock_session`](crate::Cx::lock_session) holds the lock.
    Lock { output: u32 },
    /// A popup of the program's own over `parent` (a panel under a
    /// menu bar's button), `anchor` a rectangle of `parent` it opens
    /// against as `placement` says; with `grab` it takes the keyboard and
    /// pointer while open and closes when clicked outside, which the
    /// program hears as [`Program::close_requested`](crate::Program::close_requested)
    /// (it closes whatever that answers). It is the spec's size and draws
    /// its own ground, shadow and all.
    Popup { parent: WindowKey, anchor: Rect, placement: crate::widgets::menus::MenuPlacement, grab: bool },
}

/// What a window is asked to be: `WindowSpec::new("Soroban").size(320.0, 480.0)`.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSpec {
    pub title: String,
    /// The content's size (and its limits): where the kit draws the menu
    /// bar in the window (no Aizome shell), the bar comes on top of it.
    pub size: Size,
    pub min_size: Option<Size>,
    pub max_size: Option<Size>,
    pub resizable: bool,
    pub kind: WindowKind,
    /// Opens hidden, until [`Cx::show_window`](crate::Cx::show_window).
    pub hidden: bool,
}

impl WindowSpec {
    pub fn new(title: impl Into<String>) -> WindowSpec {
        WindowSpec {
            title: title.into(),
            size: Size::new(640.0, 480.0),
            min_size: None,
            max_size: None,
            resizable: true,
            kind: WindowKind::Normal,
            hidden: false,
        }
    }

    /// Open hidden: nothing shows, and its view isn't asked for, until
    /// [`Cx::show_window`](crate::Cx::show_window) (or while it's drawn
    /// ahead: [`Cx::prepare_window`](crate::Cx::prepare_window)).
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.size = Size::new(width, height);
        self
    }

    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_size = Some(Size::new(width, height));
        self
    }

    pub fn max_size(mut self, width: f64, height: f64) -> Self {
        self.max_size = Some(Size::new(width, height));
        self
    }

    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    pub fn kind(mut self, kind: WindowKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn dialog(self, parent: WindowKey) -> Self {
        self.kind(WindowKind::Dialog { parent })
    }

    pub fn utility(self) -> Self {
        self.kind(WindowKind::Utility)
    }
}

impl WindowSpec {
    /// A desktop surface placed as `options` say, `width` by `height`
    /// points (0 along an axis it's anchored at both ends of: the
    /// compositor's choice).
    pub fn layer(options: LayerOptions, width: f64, height: f64) -> WindowSpec {
        WindowSpec {
            size: Size::new(width, height),
            resizable: false,
            kind: WindowKind::Layer(options),
            ..WindowSpec::new("")
        }
    }

    /// The lock screen's surface on output `output`.
    pub fn lock(output: u32) -> WindowSpec {
        WindowSpec { size: Size::ZERO, resizable: false, kind: WindowKind::Lock { output }, ..WindowSpec::new("") }
    }

    /// Whether the kit draws the in-window menu bar along its top (where
    /// there's no Aizome shell): normal windows only.
    pub fn has_menu_bar(&self) -> bool {
        self.kind == WindowKind::Normal
    }
}
