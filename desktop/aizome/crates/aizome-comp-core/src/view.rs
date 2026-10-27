//! What the logic tells the core about windows: where each one is, whether
//! it shows, which one has focus and what is stacked over what.
//!
//! The logic owns the window-management model; the core needs a little of
//! it for its own work (frame callbacks, which outputs a surface is on,
//! popups' places, keyboard focus, foreign-toplevel handles, screenshots)
//! and to keep the desktop usable when no logic is running (the recovery
//! path draws windows where they last were). The logic publishes this view
//! whenever its model changes ([`crate::LogicVTable::publish`]); the core
//! never writes it while a logic is loaded.

use std::path::PathBuf;

use crate::{Rect, WinId};

/// One window, as the logic last described it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WinView {
    /// The content rectangle in the global layout (points).
    pub rect: Rect,
    /// It shows on the current Desk (not minimised or hidden, the showing
    /// member of its group).
    pub shown: bool,
    pub minimised: bool,
    pub hidden: bool,
    pub fill: bool,
    pub fullscreen: bool,
    pub urgent: bool,
    /// The window it's a dialog of.
    pub parent: Option<WinId>,
    /// Its Desk, 1-based; `None` when it's on every Desk.
    pub desk: Option<u32>,
}

/// The whole desktop, as the logic last described it.
#[derive(Clone, Debug)]
pub struct View {
    pub focused: Option<WinId>,
    /// The windows that show, bottom to top.
    pub stacking: Vec<WinId>,
    /// Every window, most recently focused first.
    pub mru: Vec<WinId>,
    /// An overlay (the switcher, the overview, the area picker) has the
    /// keyboard, so no surface does.
    pub grabs_keyboard: bool,
    /// Something is moving: keep drawing frames.
    pub animating: bool,
    /// How far decorations reach outside a window's content, for redraws.
    pub outset: i32,
    /// The current Desk's folder, for programs started without one.
    pub desk_folder: Option<PathBuf>,
    /// Layer surfaces of these namespaces leave their last frame behind
    /// when they go ([`crate::ghosts`]), for the logic to move away.
    pub keep_on_unmap: Vec<String>,
}

impl Default for View {
    fn default() -> Self {
        View {
            focused: None,
            stacking: Vec::new(),
            mru: Vec::new(),
            grabs_keyboard: false,
            animating: false,
            outset: 48,
            desk_folder: None,
            keep_on_unmap: Vec::new(),
        }
    }
}
