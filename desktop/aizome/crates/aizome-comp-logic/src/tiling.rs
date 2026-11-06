//! Tiling (`[windows] layout = "tiled"`): each window fills its Desk
//! beside the Deskbar, as Fill (the zoom box) fills it, and so keeps to
//! that area as displays, bars and the Deskbar change. Dialogs float over
//! their windows; so do windows that can't grow to the area (of a fixed
//! size, or too small a largest one) and windows floated by hand. Full
//! screen still takes the display, and a split stays split.
//!
//! A tiled window neither moves nor resizes: a drag of its tab can stack
//! it or tear it out, and it fills again where it's dropped. A new one is
//! told the area's size before it first draws, so it opens filled.
//!
//! What tiling did is marked on each window ([`Tiling`]) and undone when
//! tiling goes off: a window it filled goes back to its free place (where
//! its app last left it, or a smaller one), one it floated comes down.

use aizome_comp_core::aizome_config::WindowLayout;

use crate::actions::after_placement;
use crate::prelude::*;
use crate::wm::{Placement, Tiling};

/// Whether windows are tiled.
pub fn on(st: &State) -> bool {
    st.az.settings.windows.layout == WindowLayout::Tiled
}

/// What tiling does with a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    Fill,
    Float,
    Leave,
}

fn fit(st: &State, id: WinId) -> Fit {
    let Some(w) = st.lg.r().wm.get(id) else { return Fit::Leave };
    let by_hand = w.float && w.tiling != Some(Tiling::Floated);
    let own_place = matches!(w.placement, Placement::Fullscreen | Placement::SplitLeft | Placement::SplitRight);
    if w.parent.is_some() || by_hand || own_place {
        return Fit::Leave;
    }
    let (area, insets) = st.usable_for(id);
    let (_, max) = crate::grabs::size_limits(st, id);
    if grows_to(max, area.inset(insets)) { Fit::Fill } else { Fit::Float }
}

/// Whether a window no larger than `max` can fill `target`.
pub fn grows_to(max: (i32, i32), target: Rect) -> bool {
    max.0 >= target.w && max.1 >= target.h
}

/// Whether tiling keeps a window filled: it doesn't move, resize or
/// leave Fill but by floating.
pub fn holds(st: &State, id: WinId) -> bool {
    on(st) && fit(st, id) == Fit::Fill
}

/// Every window as tiling has it (nothing when tiling is off).
pub fn retile(st: &mut State) {
    if !on(st) {
        return;
    }
    let l = st.lg.r();
    let ids: Vec<WinId> = l.wm.windows().filter(|w| l.wm.is_group_active(w.id)).map(|w| w.id).collect();
    for id in ids {
        match fit(st, id) {
            Fit::Fill => fill(st, id),
            Fit::Float => float(st, id),
            Fit::Leave => {}
        }
    }
}

/// A window opened (placed, and stacked if it joined a group): one that
/// tiles gets a free place worth going back to, as it was told the area's
/// size, then everything tiles.
pub fn new_window(st: &mut State, id: WinId) {
    if !on(st) {
        return;
    }
    let alone = st.lg.r().wm.get(id).is_some_and(|w| w.group.is_none() && w.placement == Placement::Free);
    if alone && fit(st, id) == Fit::Fill {
        let free = free_rect(st, id);
        st.lg.m().wm.set_rect(id, free);
    }
    retile(st);
}

/// Where a window opened told the area's size goes when it's free: where
/// its app last left it, or two thirds of the area across and three
/// quarters down, cascaded.
fn free_rect(st: &State, id: WinId) -> Rect {
    let l = st.lg.r();
    let (area, insets) = st.usable_for(id);
    let app = l.wm.get(id).map(|w| w.app_id.clone()).unwrap_or_default();
    let remembered = st.az.settings.windows.remember_geometry.then(|| l.remembered.get(&app)).flatten();
    if let Some(r) = remembered {
        return crate::wm::fit_into(r, area, insets);
    }
    let (min, max) = crate::grabs::size_limits(st, id);
    let size = ((area.w * 2 / 3).max(min.0).min(max.0), (area.h * 3 / 4).max(min.1).min(max.1));
    l.wm.place(size, area, &[], insets, None)
}

fn fill(st: &mut State, id: WinId) {
    let (area, insets) = st.usable_for(id);
    let target = area.inset(insets);
    let l = st.lg.m();
    let Some(w) = l.wm.get(id) else { return };
    let (old, placement, floated) = (w.rect, w.placement, w.tiling == Some(Tiling::Floated));
    if placement == Placement::Fill && old == target {
        return;
    }
    // Filled already, by hand or with a group it left: only its rectangle.
    if placement == Placement::Fill {
        l.wm.fit_rect(id, target);
    } else {
        l.wm.set_fill(id, Some(true), area, insets);
        for m in l.wm.group_members(id) {
            if let Some(w) = l.wm.get_mut(m) {
                w.tiling = Some(Tiling::Filled);
                if floated {
                    w.float = false;
                }
            }
        }
    }
    after_placement(st, id, Some(old), target);
}

fn float(st: &mut State, id: WinId) {
    let l = st.lg.m();
    if l.wm.get(id).is_none_or(|w| w.float) {
        return;
    }
    let filled = l.wm.get(id).is_some_and(|w| w.fill());
    let members = l.wm.group_members(id);
    for m in &members {
        if let Some(w) = l.wm.get_mut(*m) {
            w.float = true;
            w.tiling = Some(Tiling::Floated);
        }
        l.window_changed(*m);
    }
    if filled {
        unfill(st, id);
    }
    st.redraw_window(id);
}

/// A filled window goes back to its free place.
fn unfill(st: &mut State, id: WinId) {
    let (area, insets) = st.usable_for(id);
    let l = st.lg.m();
    let Some(old) = l.wm.get(id).filter(|w| w.fill()).map(|w| w.rect) else { return };
    if let Some(rect) = l.wm.set_fill(id, Some(false), area, insets) {
        after_placement(st, id, Some(old), rect);
    }
}

/// A tiled window dragged and dropped fills again: its free place is the
/// one it had before the drag, not where it was dropped.
pub fn keep_free_place(st: &mut State, id: WinId, free: Rect) {
    let l = st.lg.m();
    if !l.wm.get(id).is_some_and(|w| w.fill()) {
        return;
    }
    for m in l.wm.group_members(id) {
        if let Some(w) = l.wm.get_mut(m) {
            w.restore = Some(free);
        }
    }
}

/// A window is floated by hand: a tiled one leaves for its free place,
/// and is the person's to place from now on.
pub fn release(st: &mut State, id: WinId) {
    if holds(st, id) {
        unfill(st, id);
    }
    let l = st.lg.m();
    for m in l.wm.group_members(id) {
        if let Some(w) = l.wm.get_mut(m) {
            w.tiling = None;
        }
    }
}

/// Tiling went off: what it did is undone.
pub fn untile(st: &mut State) {
    let l = st.lg.r();
    let marked: Vec<(WinId, Tiling)> =
        l.wm.windows().filter(|w| l.wm.is_group_active(w.id)).filter_map(|w| w.tiling.map(|t| (w.id, t))).collect();
    for (id, tiling) in marked {
        match tiling {
            Tiling::Filled => unfill(st, id),
            Tiling::Floated => {}
        }
        let l = st.lg.m();
        for m in l.wm.group_members(id) {
            if let Some(w) = l.wm.get_mut(m) {
                w.tiling = None;
                if tiling == Tiling::Floated {
                    w.float = false;
                }
            }
            l.window_changed(m);
        }
        st.redraw_window(id);
    }
}
