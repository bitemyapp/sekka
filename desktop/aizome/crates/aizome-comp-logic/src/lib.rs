//! Aizome's compositor logic: what happens on the desktop, as opposed to
//! the plumbing that keeps clients connected (the core,
//! `aizome-comp-core`).
//!
//! It holds window management and placement (`wm/`, `actions.rs`), tab
//! groups and Desks, key and pointer bindings and the command key
//! (`bindings.rs`, `input.rs`), focus policy, decorations and how they're
//! drawn (`decor.rs`; text and icons come from the core), the overlays (switcher,
//! overview, marking menu, area picker), motion (`anim.rs`), and each
//! frame's scene (`render.rs`).
//!
//! The core loads this library at run time and swaps it for a new build
//! (`aizomectl reload`, ⌃❖R) between two frames, without disconnecting a
//! client: the old logic saves its state ([`Saved`], JSON), the new one
//! starts from it. Rules for code here (docs/reload.md):
//!
//! - Keep state in [`Logic`] (reached through `state.lg.m()`), never in
//!   smithay's user data or anywhere else the core keeps: a rebuilt
//!   library's types keep their TypeIds but may change their layout.
//! - No statics that must survive a reload; they start afresh.
//! - No closures of your own on the event loop: ask the core for timers
//!   ([`State::schedule`]); pointer grabs go through
//!   [`State::set_logic_pointer_grab`].
//! - New fields of saved state need `#[serde(default)]`.
//! - A panic is caught; the core rolls back to the logic that ran before
//!   and tells the user.

pub mod actions;
pub mod anim;
pub mod bindings;
pub mod cmdkey;
pub mod config;
pub mod decor;
pub mod frost;
pub mod grabs;
pub mod hooks;
pub mod input;
pub mod memory;
pub mod overlays;
pub mod render;
pub mod requests;
pub mod scales;
pub mod slides;
pub mod wm;

use std::collections::{HashMap, HashSet};

use aizome_comp_core::logic::LogicHost;
use serde::{Deserialize, Serialize};

pub use aizome_comp_core::{Rect, State, WinId};

/// A variant of this logic built for tests (`AIZOME_LOGIC_VARIANT` at
/// build time): `red` draws the focus colour pure red and takes the
/// `test-panic` action, which panics.
pub const VARIANT: Option<&str> = option_env!("AIZOME_LOGIC_VARIANT");

/// What logic modules use everywhere.
pub mod prelude {
    pub use crate::actions::Actions;
    pub use crate::input::InputExt;
    pub use crate::requests::RequestsExt;
    pub use crate::{Lg, Logic};
    pub use aizome_comp_core::{Aizome, Rect, State, WinId};
}

/// The logic's state.
pub struct Logic {
    /// The window-management model: windows, tab groups, Desks, stacking,
    /// focus history.
    pub wm: wm::Model,
    /// What the logic keeps per window beside the model.
    pub wins: HashMap<WinId, WinData>,
    /// Windows whose IPC description changed since the last broadcast.
    pub dirty_windows: HashSet<WinId>,
    /// Last size and position per app_id.
    pub remembered: memory::Remembered,
    pub bindings: bindings::Bindings,
    pub anims: anim::Animations,
    pub overlays: overlays::Overlays,
    /// The shell's notification cards sliding in and out.
    pub slides: slides::Slides,
    pub interactive: input::Interactive,
    /// Slow keys and sticky keys.
    pub access: input::Access,
    /// The action a key decided on, run once the key is through.
    pub pending_action: Option<bindings::Action>,
    /// What was last published to the core ([`hooks::publish`]).
    published: Option<(u64, bool, bool, bool)>,
    /// The surfaces' scales as last told ([`scales::refresh`]).
    pub scales_seen: Option<u64>,
    /// Processes playing sound, with their parents (a browser plays from a
    /// child of the process its windows belong to), as the shell says.
    pub audible: HashSet<i32>,
}

/// Per window, beside the model.
#[derive(Default)]
pub struct WinData {
    pub decor: decor::DecorCache,
    /// While an interactive resize runs from a left or top edge, the
    /// opposite edges stay put as the client resizes.
    pub resize_anchor: Option<ResizeAnchor>,
    /// The size last asked of the client.
    pub requested_size: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Debug)]
pub struct ResizeAnchor {
    pub left: bool,
    pub top: bool,
    /// The rect when the resize began.
    pub start: Rect,
}

/// What a logic hands to the next across a reload: the model and what
/// else the user would miss (and cards in motion). Caches, overlays and
/// grabs start afresh.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub model: Option<wm::Model>,
    pub requested_sizes: HashMap<WinId, (i32, i32)>,
    /// Notification cards in motion.
    pub slides: Vec<slides::SavedSlide>,
    pub audible: Vec<i32>,
}

impl Logic {
    pub fn new(settings: &aizome_comp_core::aizome_config::Settings) -> Logic {
        let desks = settings.desks_or_default().into_iter().map(|d| (d.name, d.folder));
        Logic {
            wm: wm::Model::new(desks),
            wins: HashMap::new(),
            dirty_windows: HashSet::new(),
            remembered: memory::Remembered::load(),
            bindings: bindings::Bindings::new(&settings.keys.bindings),
            anims: anim::Animations::default(),
            overlays: overlays::Overlays::default(),
            slides: slides::Slides::default(),
            interactive: input::Interactive::default(),
            access: input::Access::default(),
            pending_action: None,
            published: None,
            scales_seen: None,
            audible: HashSet::new(),
        }
    }

    /// Whether any of a Desk's windows belongs to a process playing sound.
    pub fn desk_audible(&self, desk: wm::DeskId) -> bool {
        !self.audible.is_empty()
            && self
                .wm
                .desk_windows(desk)
                .iter()
                .any(|id| self.wm.get(*id).and_then(|w| w.pid).is_some_and(|pid| self.audible.contains(&pid)))
    }

    /// Notes that a window's IPC description changed.
    pub fn window_changed(&mut self, id: WinId) {
        self.dirty_windows.insert(id);
    }

    pub fn data(&mut self, id: WinId) -> &mut WinData {
        self.wins.entry(id).or_default()
    }
}

/// This library's mark: the address of a static, different for every
/// copy loaded.
static MARK: u8 = 0;

fn mark() -> usize {
    &MARK as *const u8 as usize
}

/// The logic's state, from the core's slot.
pub trait Lg {
    fn m(&mut self) -> &mut Logic;
    fn r(&self) -> &Logic;
}

impl Lg for LogicHost {
    fn m(&mut self) -> &mut Logic {
        let (p, owner) = self.raw();
        assert!(!p.is_null() && owner == mark(), "compositor logic code ran against another logic's state");
        // Safety: this library made the box (the mark says so) and the
        // borrow is tied to the core's slot, which the borrow checker
        // keeps from being taken twice.
        unsafe { &mut *(p as *mut Logic) }
    }

    fn r(&self) -> &Logic {
        let (p, owner) = self.raw();
        assert!(!p.is_null() && owner == mark(), "compositor logic code ran against another logic's state");
        // Safety: as above.
        unsafe { &*(p as *const Logic) }
    }
}

/// Gives the core a fresh state box (from `init`).
fn install(host: &mut LogicHost, logic: Logic) {
    let p = Box::into_raw(Box::new(logic)) as *mut ();
    host.set_raw(p, mark());
}

/// Takes the state box back (from `release`).
fn uninstall(host: &mut LogicHost) -> Option<Box<Logic>> {
    let (p, owner) = host.raw();
    if p.is_null() || owner != mark() {
        return None;
    }
    host.take_raw();
    // Safety: made by `install` in this library.
    Some(unsafe { Box::from_raw(p as *mut Logic) })
}

/// The ABI this library was built against, readable from its file before
/// it is loaded.
#[used]
static ABI_MARK: [u8; 81] = aizome_comp_core::logic::abi_marker();

/// The entry point the core looks up.
#[unsafe(no_mangle)]
pub fn aizome_logic_entry() -> &'static aizome_comp_core::LogicVTable {
    // Touch the mark so it's kept.
    let _ = std::hint::black_box(&ABI_MARK);
    &hooks::VTABLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desk_plays_sound_when_a_window_of_a_playing_process_is_on_it() {
        let mut l = Logic::new(&aizome_comp_core::aizome_config::Settings::default());
        let d2 = l.wm.desks()[1].id;
        let rect = crate::wm::Rect::new(0, 0, 100, 100);
        let a = l.wm.insert("firefox", "Music", Some(4242), rect, None);
        l.wm.move_to_desk(a, d2);
        assert!(!l.desk_audible(d2));
        l.audible = [4242].into_iter().collect();
        assert!(l.desk_audible(d2));
        assert!(!l.desk_audible(l.wm.desks()[0].id));
    }
}
