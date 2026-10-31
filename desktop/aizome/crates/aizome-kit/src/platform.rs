//! What the engine asks of the system: windows, popups, damage, the
//! clipboard, timers. Sidestep answers in a running app
//! ([`crate::app`]); tests answer with a recording stand-in
//! ([`crate::testing`]).

use std::sync::Arc;
use std::time::Duration;

use sidestep_ui::{Color, Cursor};

use crate::layout::{Rect, Size, Vec2};
use crate::program::Output;
use sidestep_ui::layer_shell::{Anchors, KeyboardInteractivity, Layer, LayerOptions, Margins};

/// The engine's name for a window (an app's, or a popup).
pub type WinId = u32;

/// Wakes the loop from another thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Where a popup opens against its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Below, left edges lined up (a pop-up button's menu).
    Below,
    /// Its top left at a point (a context menu).
    AtPoint,
    /// To the right, tops lined up (a submenu).
    Beside,
    /// Above, centred (a tooltip, a popover over its anchor).
    Above,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OpenKind {
    /// A window of its own; a dialog is kept over `parent`.
    Toplevel { parent: Option<WinId>, utility: bool },
    /// A popup over `parent`, at `anchor` (the parent's coordinates).
    /// With `grab` it takes the keyboard and pointer while open.
    Popup { parent: WinId, anchor: Rect, placement: Placement, grab: bool },
    /// A desktop surface (wlr-layer-shell).
    Layer(LayerOptions),
    /// The session lock's surface on an output.
    Lock { output: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenSpec {
    pub title: String,
    pub size: Size,
    pub min_size: Option<Size>,
    pub max_size: Option<Size>,
    pub resizable: bool,
    pub kind: OpenKind,
    /// What shows where nothing draws; none for see-through (popups with
    /// their shadow).
    pub background: Option<Color>,
    /// The app id the compositor groups it by.
    pub app_id: String,
    /// Shown when it opens (else hidden, until `show_window`).
    pub visible: bool,
    /// Another program's window it's kept over, by its exported handle.
    pub foreign_parent: Option<String>,
}

pub trait Platform {
    /// Queue a desktop opener. A rejected request never invokes `completion`.
    /// Accepted requests complete after the opener exits; all process work is off the UI thread.
    fn open_url(
        &mut self,
        _url: &str,
        _completion: crate::urls::OpenUrlCompletion,
    ) -> Result<(), crate::urls::OpenUrlError> {
        Err(crate::urls::OpenUrlError::Unavailable)
    }
    fn open_window(&mut self, id: WinId, spec: &OpenSpec);
    fn close_window(&mut self, id: WinId);
    fn set_title(&mut self, id: WinId, title: &str);
    fn invalidate(&mut self, id: WinId, rect: Rect);
    fn invalidate_all(&mut self, id: WinId);
    fn set_cursor(&mut self, id: WinId, cursor: Cursor);
    fn set_text_input(&mut self, id: WinId, wanted: bool, caret: Option<Rect>);
    fn request_size(&mut self, id: WinId, size: Size);
    fn minimize(&mut self, id: WinId);
    fn set_maximized(&mut self, id: WinId, on: bool);
    /// Ask for the window full screen, or not (the compositor decides,
    /// and says so with the window's state).
    fn set_fullscreen(&mut self, id: WinId, on: bool) {
        let _ = (id, on);
    }
    fn activate(&mut self, id: WinId);
    /// Consume a token supplied by the application that initiated a launch.
    fn activate_with_token(&mut self, id: WinId, token: &str);
    fn begin_move(&mut self, id: WinId);
    fn clipboard_text(&mut self) -> Option<String>;
    fn set_clipboard_text(&mut self, text: &str);
    /// Capture a transferable clipboard read without waiting for another client.
    fn clipboard_read(&mut self) -> sidestep_ui::ClipboardRead;
    /// Publish text whose allocation and MIME preparation already ran on a worker.
    fn set_prepared_clipboard_text(&mut self, text: sidestep_ui::PreparedClipboardText);
    fn set_prepared_clipboard(&mut self, data: sidestep_ui::PreparedClipboard);
    /// The primary selection (a middle click pastes it), as the clipboard.
    fn primary_read(&mut self) -> sidestep_ui::ClipboardRead;
    fn set_prepared_primary(&mut self, data: sidestep_ui::PreparedClipboard);
    /// A timer: the engine hears its token when it comes due.
    fn set_timer(&mut self, after: Duration, repeat: bool) -> u64;
    fn cancel_timer(&mut self, token: u64);
    fn quit(&mut self);
    /// Whether the desktop is dark (for the Auto theme).
    fn desktop_dark(&self) -> bool;
    fn waker(&self) -> Waker;
    fn set_layer(&mut self, id: WinId, change: LayerChange);
    /// Where the window takes the pointer (none: everywhere).
    fn set_input_region(&mut self, id: WinId, region: Option<Vec<Rect>>);
    /// Where the window is opaque (none: nowhere).
    fn set_opaque_region(&mut self, id: WinId, region: Option<Vec<Rect>>);
    /// Place popup `id` against another rectangle of its parent (points).
    fn set_popup_anchor(&mut self, id: WinId, anchor: Rect);
    /// Move the pixels of `rect` (points) by `delta` at the next frame,
    /// before drawing the damage (which the move takes along): what a
    /// scroll does. Uncovered strips are damaged.
    fn scroll(&mut self, id: WinId, rect: Rect, delta: Vec2);
    /// Start dragging `source` out of window `id`: the drag's number.
    fn start_drag(&mut self, id: WinId, source: sidestep_ui::DragSource) -> Option<u64>;
    fn outputs(&self) -> Vec<Output>;
    fn lock_session(&mut self);
    fn unlock_session(&mut self);
    fn is_session_locked(&self) -> bool;
    /// Ask for an activation token: the engine hears it under the id
    /// returned.
    fn request_activation_token(&mut self, app_id: Option<&str>) -> u64;
    /// Have `bytes` of window memory free and faulted in now (see
    /// [`Cx::reserve_buffers`](crate::Cx::reserve_buffers)); nothing where
    /// there's no shared memory.
    fn reserve_buffers(&mut self, bytes: usize) {
        let _ = bytes;
    }
    /// Show window `id` (opened hidden, or hidden since), or hide it.
    fn show_window(&mut self, id: WinId, shown: bool);
    /// Keep `picture`, hidden window `id` drawn ahead, for its next
    /// showing's first frame to start from, with what changed since it
    /// shows drawn over it (see [`Cx::prepare_window`](crate::Cx::prepare_window));
    /// none: drop what's kept.
    fn keep_picture(&mut self, id: WinId, picture: Option<crate::ahead::Picture>);
    /// Show video `video` of window `id` at `place`, or take it away.
    fn place_video(
        &mut self,
        id: WinId,
        video: sidestep_ui::video::VideoId,
        place: Option<sidestep_ui::video::VideoPlace>,
    ) {
        let _ = (id, video, place);
    }
    /// Hide the pointer until it next moves (full-screen video).
    fn hide_cursor_until_moved(&mut self) {}
    /// Where video `video` of window `id` takes frames from other threads.
    fn video_sink(&mut self, id: WinId, video: sidestep_ui::video::VideoId) -> Option<sidestep_ui::video::VideoSink> {
        let _ = (id, video);
        None
    }
}

/// A layer surface's placement, changed while it shows.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerChange {
    Size(Size),
    Layer(Layer),
    Anchors(Anchors),
    ExclusiveZone(i32),
    Margins(Margins),
    Keyboard(KeyboardInteractivity),
}
