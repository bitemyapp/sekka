//! The window manager's model: windows, tab groups, Desks, the stacking
//! order and the focus history, with no Wayland types in sight, so the
//! rules can be tested on their own.
//!
//! Rectangles are in points in the global layout. A window's `rect` is its
//! content (what the client draws, its xdg window geometry); the frame, the
//! tab and the shadow sit outside it, as [`Insets`] describe.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use aizome_comp_core::WinId;
pub use aizome_comp_core::geom::{Insets, Rect};
pub type DeskId = u64;
pub type GroupId = u64;

/// How a window's rectangle was chosen; anything but `Free` remembers the
/// free rectangle to go back to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    #[default]
    Free,
    Fill,
    SplitLeft,
    SplitRight,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Win {
    pub id: WinId,
    pub app_id: String,
    pub title: String,
    pub pid: Option<i32>,
    pub desk: DeskId,
    /// On every Desk.
    pub sticky: bool,
    /// The content rectangle.
    pub rect: Rect,
    pub placement: Placement,
    /// The free rectangle to return to from fill, split or full screen.
    pub restore: Option<Rect>,
    pub minimised: bool,
    /// Its app is hidden (❖H).
    pub hidden: bool,
    /// Asked for attention without a valid activation token.
    pub urgent: bool,
    /// Kept above other windows.
    pub float: bool,
    pub group: Option<GroupId>,
    /// How far the tab has been slid along the top edge (⇧-drag), in points.
    pub tab_offset: i32,
    /// The window this one is a dialog of.
    pub parent: Option<WinId>,
    /// Aizome draws the frame (server-side decorations); otherwise only
    /// the tab.
    pub ssd: bool,
}

impl Win {
    pub fn fullscreen(&self) -> bool {
        self.placement == Placement::Fullscreen
    }
    pub fn fill(&self) -> bool {
        self.placement == Placement::Fill
    }
}

/// Windows stacked into one frame, their tabs side by side.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub id: GroupId,
    /// In tab order.
    pub members: Vec<WinId>,
    /// The member that shows.
    pub active: WinId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Desk {
    pub id: DeskId,
    pub name: String,
    pub folder: Option<String>,
    /// When it was parked (seconds since the epoch): its windows are told
    /// they're suspended, so they draw nothing until it's visited again.
    pub parked: Option<u64>,
    /// When it was last left (seconds since the epoch).
    pub left: Option<u64>,
}

/// What a change did to focus: the window that should now have it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refocus {
    /// Focus didn't move.
    Same,
    /// Focus moves to this window, or to nothing.
    To(Option<WinId>),
}

/// Saved across reloads (as JSON): fields added later need defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Model {
    wins: HashMap<WinId, Win>,
    /// Bottom to top.
    stacking: Vec<WinId>,
    /// Most recently focused first.
    mru: Vec<WinId>,
    groups: HashMap<GroupId, Group>,
    desks: Vec<Desk>,
    current: DeskId,
    focused: Option<WinId>,
    next_win: WinId,
    next_group: GroupId,
    next_desk: DeskId,
    /// Bumped by every change, so the logic tells the core only when
    /// something did change.
    #[serde(skip)]
    generation: u64,
}

impl Default for Model {
    fn default() -> Model {
        Model::new(Vec::new())
    }
}

/// How far a cascaded window steps from the one it would cover.
pub const CASCADE_STEP: i32 = 24;

impl Model {
    /// A model with these Desks (name, folder); at least one is made.
    pub fn new(desks: impl IntoIterator<Item = (String, Option<String>)>) -> Model {
        let mut m = Model {
            wins: HashMap::new(),
            stacking: Vec::new(),
            mru: Vec::new(),
            groups: HashMap::new(),
            desks: Vec::new(),
            current: 0,
            focused: None,
            next_win: 1,
            next_group: 1,
            next_desk: 1,
            generation: 0,
        };
        for (name, folder) in desks {
            m.push_desk(name, folder);
        }
        if m.desks.is_empty() {
            m.push_desk("1".into(), None);
        }
        m.current = m.desks[0].id;
        m
    }

    fn push_desk(&mut self, name: String, folder: Option<String>) -> DeskId {
        self.generation += 1;
        let id = self.next_desk;
        self.next_desk += 1;
        self.desks.push(Desk { id, name, folder, ..Desk::default() });
        id
    }

    // ---- windows ------------------------------------------------------

    /// Adds a window on the current Desk (or its parent's), on top of the
    /// stack, without focusing it, with the next free id.
    pub fn insert(&mut self, app_id: &str, title: &str, pid: Option<i32>, rect: Rect, parent: Option<WinId>) -> WinId {
        let id = self.next_win;
        self.insert_as(id, app_id, title, pid, rect, parent);
        id
    }

    /// Adds a window with the id the core gave it.
    pub fn insert_as(
        &mut self,
        id: WinId,
        app_id: &str,
        title: &str,
        pid: Option<i32>,
        rect: Rect,
        parent: Option<WinId>,
    ) {
        self.generation += 1;
        self.next_win = self.next_win.max(id + 1);
        let (desk, sticky) =
            parent.and_then(|p| self.wins.get(&p)).map(|p| (p.desk, p.sticky)).unwrap_or((self.current, false));
        self.wins.insert(
            id,
            Win {
                id,
                app_id: app_id.to_string(),
                title: title.to_string(),
                pid,
                desk,
                sticky,
                rect,
                placement: Placement::Free,
                restore: None,
                minimised: false,
                hidden: false,
                urgent: false,
                float: false,
                group: None,
                tab_offset: 0,
                parent,
                ssd: true,
            },
        );
        self.stacking.push(id);
        self.mru.push(id);
    }

    /// Bumped by every change.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Removes a window. Returns where focus goes if it had it.
    pub fn remove(&mut self, id: WinId) -> Refocus {
        self.generation += 1;
        if self.wins.contains_key(&id) {
            self.leave_group(id);
        }
        if self.wins.remove(&id).is_none() {
            return Refocus::Same;
        }
        self.stacking.retain(|w| *w != id);
        self.mru.retain(|w| *w != id);
        for w in self.wins.values_mut() {
            if w.parent == Some(id) {
                w.parent = None;
            }
        }
        if self.focused == Some(id) {
            self.focused = None;
            Refocus::To(self.next_focus(None))
        } else {
            Refocus::Same
        }
    }

    pub fn get(&self, id: WinId) -> Option<&Win> {
        self.wins.get(&id)
    }

    pub fn get_mut(&mut self, id: WinId) -> Option<&mut Win> {
        self.generation += 1;
        self.wins.get_mut(&id)
    }

    pub fn contains(&self, id: WinId) -> bool {
        self.wins.contains_key(&id)
    }

    pub fn windows(&self) -> impl Iterator<Item = &Win> {
        self.stacking.iter().filter_map(|id| self.wins.get(id))
    }

    pub fn len(&self) -> usize {
        self.wins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.wins.is_empty()
    }

    /// Bottom to top, every window.
    pub fn stacking(&self) -> &[WinId] {
        &self.stacking
    }

    /// Most recently focused first.
    pub fn mru(&self) -> &[WinId] {
        &self.mru
    }

    pub fn focused(&self) -> Option<WinId> {
        self.focused
    }

    pub fn on_current_desk(&self, id: WinId) -> bool {
        self.wins.get(&id).is_some_and(|w| w.sticky || w.desk == self.current)
    }

    #[allow(dead_code)]
    pub fn on_desk(&self, id: WinId, desk: DeskId) -> bool {
        self.wins.get(&id).is_some_and(|w| w.sticky || w.desk == desk)
    }

    /// Whether a window is the member of its group that shows (true when
    /// it has no group).
    pub fn is_group_active(&self, id: WinId) -> bool {
        match self.wins.get(&id).and_then(|w| w.group) {
            Some(g) => self.groups.get(&g).is_some_and(|g| g.active == id),
            None => true,
        }
    }

    /// Whether a window would show on a Desk: not minimised, not hidden,
    /// the active member of its group.
    pub fn is_shown_on(&self, id: WinId, desk: DeskId) -> bool {
        let Some(w) = self.wins.get(&id) else { return false };
        !w.minimised && !w.hidden && (w.sticky || w.desk == desk) && self.is_group_active(id)
    }

    /// Whether a window shows now.
    pub fn is_shown(&self, id: WinId) -> bool {
        self.is_shown_on(id, self.current)
    }

    /// The windows that show on a Desk, bottom to top: floating ones above
    /// the rest, and full-screen ones above those.
    pub fn shown_stacking_on(&self, desk: DeskId) -> Vec<WinId> {
        let shown: Vec<WinId> = self.stacking.iter().copied().filter(|id| self.is_shown_on(*id, desk)).collect();
        let rank = |id: &WinId| {
            let w = &self.wins[id];
            if w.fullscreen() && self.focused == Some(*id) {
                2
            } else if w.float {
                1
            } else {
                0
            }
        };
        let mut out = shown;
        out.sort_by_key(rank); // stable: keeps the stacking order within a rank
        out
    }

    pub fn shown_stacking(&self) -> Vec<WinId> {
        self.shown_stacking_on(self.current)
    }

    /// Focuses a window: first in the focus history, raised, its group
    /// showing it. It must be shown (see [`Model::reveal`]).
    pub fn focus(&mut self, id: Option<WinId>) {
        self.generation += 1;
        let Some(id) = id else {
            self.focused = None;
            return;
        };
        if !self.wins.contains_key(&id) {
            return;
        }
        if let Some(g) = self.wins[&id].group
            && let Some(g) = self.groups.get_mut(&g)
        {
            g.active = id;
        }
        self.focused = Some(id);
        self.mru.retain(|w| *w != id);
        self.mru.insert(0, id);
        if let Some(w) = self.wins.get_mut(&id) {
            w.urgent = false;
        }
        self.raise(id);
    }

    /// Makes a window show: unminimised, its app unhidden, its group
    /// showing it, its Desk current. Returns whether the Desk changed.
    pub fn reveal(&mut self, id: WinId) -> bool {
        self.generation += 1;
        let Some(w) = self.wins.get(&id) else { return false };
        let (desk, sticky, app) = (w.desk, w.sticky, w.app_id.clone());
        if let Some(w) = self.wins.get_mut(&id) {
            w.minimised = false;
        }
        if self.wins[&id].hidden {
            self.show_app(&app);
        }
        if let Some(g) = self.wins[&id].group
            && let Some(g) = self.groups.get_mut(&g)
        {
            g.active = id;
        }
        if !sticky && desk != self.current {
            self.current = desk;
            true
        } else {
            false
        }
    }

    /// Puts a window (with its group and its dialogs) on top.
    pub fn raise(&mut self, id: WinId) {
        self.generation += 1;
        let members = self.group_members(id);
        let mut lift: Vec<WinId> = self.stacking.iter().copied().filter(|w| members.contains(w)).collect();
        // Dialogs ride above their parents.
        let mut i = 0;
        while i < lift.len() {
            let p = lift[i];
            let kids: Vec<WinId> =
                self.stacking.iter().copied().filter(|w| self.wins[w].parent == Some(p) && !lift.contains(w)).collect();
            lift.extend(kids);
            i += 1;
        }
        self.stacking.retain(|w| !lift.contains(w));
        self.stacking.extend(lift);
    }

    /// Sends a window (and its group) to the bottom of the stack.
    pub fn lower(&mut self, id: WinId) -> Refocus {
        self.generation += 1;
        let members = self.group_members(id);
        let lowered: Vec<WinId> = self.stacking.iter().copied().filter(|w| members.contains(w)).collect();
        self.stacking.retain(|w| !members.contains(w));
        let mut s = lowered;
        s.append(&mut self.stacking);
        self.stacking = s;
        if self.focused.is_some_and(|f| members.contains(&f)) {
            // Focus the window that is now on top.
            let top = self.shown_stacking().into_iter().rev().find(|w| !members.contains(w));
            return Refocus::To(top.or(self.focused));
        }
        Refocus::Same
    }

    /// The most recently focused window that shows, other than `except`.
    pub fn next_focus(&self, except: Option<WinId>) -> Option<WinId> {
        self.mru.iter().copied().find(|id| Some(*id) != except && self.is_shown(*id))
    }

    /// The topmost shown window under the given one in the stacking
    /// order, not in its group.
    pub fn window_behind(&self, id: WinId) -> Option<WinId> {
        let members = self.group_members(id);
        let shown = self.shown_stacking();
        let pos = shown.iter().position(|w| *w == id)?;
        shown[..pos].iter().rev().copied().find(|w| !members.contains(w))
    }

    pub fn minimise(&mut self, id: WinId) -> Refocus {
        self.generation += 1;
        let Some(w) = self.wins.get_mut(&id) else { return Refocus::Same };
        w.minimised = true;
        self.refocus_if_hidden(id)
    }

    pub fn unminimise(&mut self, id: WinId) {
        self.generation += 1;
        if let Some(w) = self.wins.get_mut(&id) {
            w.minimised = false;
        }
    }

    fn refocus_if_hidden(&mut self, id: WinId) -> Refocus {
        self.generation += 1;
        if self.focused == Some(id) && !self.is_shown(id) {
            self.focused = None;
            Refocus::To(self.next_focus(Some(id)))
        } else if self.focused.is_some_and(|f| !self.is_shown(f)) {
            self.focused = None;
            Refocus::To(self.next_focus(None))
        } else {
            Refocus::Same
        }
    }

    /// Hides every window of an app.
    pub fn hide_app(&mut self, app_id: &str) -> Refocus {
        self.generation += 1;
        for w in self.wins.values_mut() {
            if w.app_id == app_id {
                w.hidden = true;
            }
        }
        if self.focused.is_some_and(|f| !self.is_shown(f)) {
            self.focused = None;
            return Refocus::To(self.next_focus(None));
        }
        Refocus::Same
    }

    pub fn show_app(&mut self, app_id: &str) {
        self.generation += 1;
        for w in self.wins.values_mut() {
            if w.app_id == app_id {
                w.hidden = false;
            }
        }
    }

    /// Hides every app but this one.
    pub fn hide_others(&mut self, app_id: &str) -> Refocus {
        self.generation += 1;
        for w in self.wins.values_mut() {
            if w.app_id != app_id {
                w.hidden = true;
            }
        }
        if self.focused.is_some_and(|f| !self.is_shown(f)) {
            self.focused = None;
            return Refocus::To(self.next_focus(None));
        }
        Refocus::Same
    }

    /// Apps with windows, most recently used first, each with its windows
    /// in focus order (on every Desk, minimised and hidden ones too).
    pub fn apps_mru(&self) -> Vec<(String, Vec<WinId>)> {
        let mut apps: Vec<(String, Vec<WinId>)> = Vec::new();
        let order = self.mru.iter().chain(self.stacking.iter().rev());
        for id in order {
            let w = &self.wins[id];
            match apps.iter_mut().find(|(a, _)| *a == w.app_id) {
                Some((_, ws)) => {
                    if !ws.contains(id) {
                        ws.push(*id)
                    }
                }
                None => apps.push((w.app_id.clone(), vec![*id])),
            }
        }
        apps
    }

    // ---- geometry -----------------------------------------------------

    /// Sets a window's content rectangle, and its group's.
    pub fn set_rect(&mut self, id: WinId, rect: Rect) -> Vec<WinId> {
        self.generation += 1;
        let members = self.group_members(id);
        for m in &members {
            if let Some(w) = self.wins.get_mut(m) {
                w.rect = rect;
            }
        }
        members
    }

    /// Fill on or off (`None` toggles). Returns the window's new rect.
    pub fn set_fill(&mut self, id: WinId, on: Option<bool>, usable: Rect, insets: Insets) -> Option<Rect> {
        self.generation += 1;
        let w = self.wins.get(&id)?;
        let on = on.unwrap_or(w.placement != Placement::Fill);
        self.set_placement(id, if on { Placement::Fill } else { Placement::Free }, usable.inset(insets))
    }

    /// Split to one side (again on the same side goes back).
    pub fn set_split(&mut self, id: WinId, side: Side, usable: Rect, insets: Insets) -> Option<Rect> {
        self.generation += 1;
        let w = self.wins.get(&id)?;
        let want = match side {
            Side::Left => Placement::SplitLeft,
            Side::Right => Placement::SplitRight,
        };
        let half = usable.w / 2;
        let area = match side {
            Side::Left => Rect::new(usable.x, usable.y, half, usable.h),
            Side::Right => Rect::new(usable.x + half, usable.y, usable.w - half, usable.h),
        };
        if w.placement == want {
            return self.set_placement(id, Placement::Free, Rect::default());
        }
        self.set_placement(id, want, area.inset(insets))
    }

    /// Full screen on or off (`None` toggles), covering `output`.
    pub fn set_fullscreen(&mut self, id: WinId, on: Option<bool>, output: Rect) -> Option<Rect> {
        self.generation += 1;
        let w = self.wins.get(&id)?;
        let on = on.unwrap_or(!w.fullscreen());
        self.set_placement(id, if on { Placement::Fullscreen } else { Placement::Free }, output)
    }

    /// Moves a window (with its group) into a placement, remembering the
    /// free rect; `Free` goes back to it.
    pub fn set_placement(&mut self, id: WinId, placement: Placement, rect: Rect) -> Option<Rect> {
        self.generation += 1;
        let members = self.group_members(id);
        let w = self.wins.get(&id)?;
        if w.placement == placement && placement != Placement::Free {
            return Some(w.rect);
        }
        let new_rect = if placement == Placement::Free { w.restore.unwrap_or(w.rect) } else { rect };
        let restore = if w.placement == Placement::Free { Some(w.rect) } else { w.restore };
        for m in members {
            if let Some(w) = self.wins.get_mut(&m) {
                w.restore = if placement == Placement::Free { None } else { restore };
                w.placement = placement;
                w.rect = new_rect;
            }
        }
        Some(new_rect)
    }

    /// Where a new window of `size` (content) goes: centred on the usable
    /// area, beside anything to avoid (the Deskbar), cascading off windows
    /// already there; dialogs centre on their parent.
    pub fn place(&self, size: (i32, i32), usable: Rect, avoid: &[Rect], insets: Insets, parent: Option<WinId>) -> Rect {
        let area = avoid.iter().fold(usable, |area, a| beside(area, Some(*a)));
        let bounds = area.inset(insets);
        let (w, h) = (size.0.clamp(1, bounds.w), size.1.clamp(1, bounds.h));
        if let Some(p) = parent.and_then(|p| self.wins.get(&p)) {
            let c = p.rect.center();
            return clamp_into(Rect::new(c.0 - w / 2, c.1 - h / 2, w, h), area, insets);
        }
        let fw = w + insets.left + insets.right;
        let fh = h + insets.top + insets.bottom;
        let mut x = area.x + (area.w - fw) / 2 + insets.left;
        let mut y = area.y + (area.h - fh) / 2 + insets.top;
        x = x.max(area.x + insets.left);
        y = y.max(area.y + insets.top);
        let taken: Vec<(i32, i32)> =
            self.shown_stacking().iter().map(|id| (self.wins[id].rect.x, self.wins[id].rect.y)).collect();
        let near = |x: i32, y: i32| taken.iter().any(|(tx, ty)| (tx - x).abs() < 8 && (ty - y).abs() < 8);
        let mut tries = 0;
        while near(x, y) && tries < 64 {
            x += CASCADE_STEP;
            y += CASCADE_STEP;
            if x + w + insets.right > area.right() || y + h + insets.bottom > area.bottom() {
                x = area.x + insets.left + CASCADE_STEP * (tries % 4);
                y = area.y + insets.top;
            }
            tries += 1;
        }
        // Even a crowded cascade, or a window as wide as the available
        // area, must keep its entire frame out of the shell's space.
        clamp_into(Rect::new(x, y, w, h), area, insets)
    }

    // ---- tab groups ----------------------------------------------------

    #[allow(dead_code)]
    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.get(&id)
    }

    pub fn group_of(&self, win: WinId) -> Option<&Group> {
        self.wins.get(&win).and_then(|w| w.group).and_then(|g| self.groups.get(&g))
    }

    /// The window and the others in its group, in tab order (just the
    /// window when it has no group).
    pub fn group_members(&self, win: WinId) -> Vec<WinId> {
        match self.group_of(win) {
            Some(g) => g.members.clone(),
            None => vec![win],
        }
    }

    /// Stacks `b` (and its group, if any) into `a`'s group, after `a`'s
    /// tabs, and shows `b`. Returns the members whose rect changed.
    pub fn stack(&mut self, a: WinId, b: WinId) -> Option<Vec<WinId>> {
        self.generation += 1;
        if a == b || !self.wins.contains_key(&a) || !self.wins.contains_key(&b) {
            return None;
        }
        if self.group_of(a).is_some_and(|g| g.members.contains(&b)) {
            return None;
        }
        let incoming = self.group_members(b);
        for m in &incoming {
            self.leave_group(*m);
        }
        let gid = match self.wins[&a].group {
            Some(g) => g,
            None => {
                let g = self.next_group;
                self.next_group += 1;
                self.groups.insert(g, Group { id: g, members: vec![a], active: a });
                self.wins.get_mut(&a).unwrap().group = Some(g);
                g
            }
        };
        let (rect, desk, sticky, placement, restore, offset) = {
            let w = &self.wins[&a];
            (w.rect, w.desk, w.sticky, w.placement, w.restore, w.tab_offset)
        };
        for m in &incoming {
            let w = self.wins.get_mut(m).unwrap();
            w.group = Some(gid);
            w.rect = rect;
            w.desk = desk;
            w.sticky = sticky;
            w.placement = placement;
            w.restore = restore;
            w.tab_offset = offset;
            w.minimised = false;
        }
        let g = self.groups.get_mut(&gid).unwrap();
        g.members.extend(incoming.iter().copied());
        g.active = b;
        // The group sits together in the stack, where `a` was.
        let members = g.members.clone();
        let a_pos = self.stacking.iter().position(|w| *w == a).unwrap_or(self.stacking.len());
        let ordered: Vec<WinId> = self.stacking.iter().copied().filter(|w| members.contains(w)).collect();
        let insert_at = self.stacking[..a_pos].iter().filter(|w| !members.contains(w)).count();
        self.stacking.retain(|w| !members.contains(w));
        for (i, w) in ordered.into_iter().enumerate() {
            self.stacking.insert(insert_at + i, w);
        }
        if self.focused.is_some_and(|f| members.contains(&f)) {
            self.focus(Some(b));
        }
        Some(incoming)
    }

    /// Takes a window out of its group. It keeps the group's rect; the
    /// caller moves it.
    pub fn unstack(&mut self, id: WinId) -> bool {
        self.generation += 1;
        if self.wins.get(&id).and_then(|w| w.group).is_none() {
            return false;
        }
        self.leave_group(id);
        true
    }

    fn leave_group(&mut self, id: WinId) {
        self.generation += 1;
        let Some(gid) = self.wins.get(&id).and_then(|w| w.group) else { return };
        self.wins.get_mut(&id).unwrap().group = None;
        let Some(g) = self.groups.get_mut(&gid) else { return };
        let pos = g.members.iter().position(|m| *m == id);
        g.members.retain(|m| *m != id);
        if g.active == id && !g.members.is_empty() {
            // The neighbour tab shows: the one to the left, or the first.
            let i = pos.unwrap_or(0).saturating_sub(1).min(g.members.len() - 1);
            g.active = g.members[i];
        }
        if g.members.len() <= 1 {
            let rest = g.members.clone();
            self.groups.remove(&gid);
            for m in rest {
                if let Some(w) = self.wins.get_mut(&m) {
                    w.group = None;
                }
            }
        }
    }

    /// Shows a member of a group.
    pub fn set_group_active(&mut self, id: WinId) {
        self.generation += 1;
        if let Some(g) = self.wins.get(&id).and_then(|w| w.group)
            && let Some(g) = self.groups.get_mut(&g)
        {
            g.active = id;
        }
    }

    /// Moves a tab within its group to position `index`.
    #[allow(dead_code)]
    pub fn reorder_tab(&mut self, id: WinId, index: usize) {
        self.generation += 1;
        if let Some(g) = self.wins.get(&id).and_then(|w| w.group).and_then(|g| self.groups.get_mut(&g)) {
            g.members.retain(|m| *m != id);
            let i = index.min(g.members.len());
            g.members.insert(i, id);
        }
    }

    #[allow(dead_code)]
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    // ---- Desks ---------------------------------------------------------

    pub fn desks(&self) -> &[Desk] {
        &self.desks
    }

    pub fn current_desk(&self) -> DeskId {
        self.current
    }

    pub fn desk(&self, id: DeskId) -> Option<&Desk> {
        self.desks.iter().find(|d| d.id == id)
    }

    /// 1-based position.
    pub fn desk_index(&self, id: DeskId) -> Option<u32> {
        self.desks.iter().position(|d| d.id == id).map(|i| i as u32 + 1)
    }

    pub fn desk_at(&self, index: u32) -> Option<DeskId> {
        self.desks.get((index as usize).checked_sub(1)?).map(|d| d.id)
    }

    pub fn desk_named(&self, name: &str) -> Option<DeskId> {
        self.desks.iter().find(|d| d.name == name).map(|d| d.id)
    }

    /// Windows on a Desk (not counting sticky ones).
    pub fn desk_window_count(&self, desk: DeskId) -> usize {
        self.wins.values().filter(|w| w.desk == desk && !w.sticky).count()
    }

    /// Switches Desks, unparking the one arrived at. Returns where focus
    /// goes, or `None` if nothing changed.
    pub fn switch_desk(&mut self, desk: DeskId) -> Option<Refocus> {
        self.generation += 1;
        if desk == self.current || self.desk(desk).is_none() {
            return None;
        }
        self.current = desk;
        if let Some(d) = self.desks.iter_mut().find(|d| d.id == desk) {
            d.parked = None;
        }
        let keep = self.focused.filter(|f| self.is_shown(*f));
        Some(Refocus::To(keep.or_else(|| self.next_focus(None))))
    }

    /// Parks a Desk (`at`, when) or unparks it (`None`). The current Desk
    /// isn't parked. True if that changed it.
    pub fn set_parked(&mut self, id: DeskId, at: Option<u64>) -> bool {
        if at.is_some() && id == self.current {
            return false;
        }
        let Some(d) = self.desks.iter_mut().find(|d| d.id == id) else { return false };
        if d.parked.is_some() == at.is_some() {
            return false;
        }
        d.parked = at;
        self.generation += 1;
        true
    }

    /// Whether a window's Desk is parked (sticky windows show on every
    /// Desk, so aren't).
    pub fn is_parked_window(&self, id: WinId) -> bool {
        self.get(id).is_some_and(|w| !w.sticky && self.desk(w.desk).is_some_and(|d| d.parked.is_some()))
    }

    /// Notes when a Desk was left.
    pub fn mark_left(&mut self, id: DeskId, at: u64) {
        if let Some(d) = self.desks.iter_mut().find(|d| d.id == id) {
            d.left = Some(at);
        }
    }

    /// A Desk's windows (not counting sticky ones).
    pub fn desk_windows(&self, desk: DeskId) -> Vec<WinId> {
        let mut ids: Vec<WinId> = self.wins.values().filter(|w| w.desk == desk && !w.sticky).map(|w| w.id).collect();
        ids.sort();
        ids
    }

    /// How many of a Desk's windows ask for attention.
    pub fn desk_urgent_count(&self, desk: DeskId) -> usize {
        self.wins.values().filter(|w| w.desk == desk && !w.sticky && w.urgent).count()
    }

    /// The Desk before or after the current one (wrapping).
    pub fn neighbour_desk(&self, forward: bool) -> DeskId {
        let n = self.desks.len();
        let i = self.desks.iter().position(|d| d.id == self.current).unwrap_or(0);
        let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
        self.desks[j].id
    }

    /// Moves a window (with its group and dialogs) to a Desk.
    pub fn move_to_desk(&mut self, id: WinId, desk: DeskId) -> Refocus {
        self.generation += 1;
        if self.desk(desk).is_none() {
            return Refocus::Same;
        }
        let mut members = self.group_members(id);
        let kids: Vec<WinId> =
            self.wins.values().filter(|w| w.parent.is_some_and(|p| members.contains(&p))).map(|w| w.id).collect();
        members.extend(kids);
        for m in &members {
            if let Some(w) = self.wins.get_mut(m) {
                w.desk = desk;
                w.sticky = false;
            }
        }
        self.refocus_if_hidden(id)
    }

    pub fn set_sticky(&mut self, id: WinId, sticky: bool) {
        self.generation += 1;
        let current = self.current;
        for m in self.group_members(id) {
            if let Some(w) = self.wins.get_mut(&m) {
                w.sticky = sticky;
                if !sticky {
                    w.desk = current;
                }
            }
        }
    }

    /// Adds a Desk at a 1-based position (the end by default).
    pub fn add_desk(&mut self, name: String, folder: Option<String>, index: Option<u32>) -> DeskId {
        self.generation += 1;
        let id = self.push_desk(name, folder);
        if let Some(i) = index {
            let d = self.desks.pop().unwrap();
            let at = (i as usize).saturating_sub(1).min(self.desks.len());
            self.desks.insert(at, d);
        }
        id
    }

    /// Removes a Desk; its windows go to the Desk before it (or after, for
    /// the first). The last Desk stays.
    pub fn remove_desk(&mut self, id: DeskId) -> Option<Refocus> {
        self.generation += 1;
        if self.desks.len() <= 1 {
            return None;
        }
        let i = self.desks.iter().position(|d| d.id == id)?;
        self.desks.remove(i);
        let heir = self.desks[i.saturating_sub(1).min(self.desks.len() - 1)].id;
        for w in self.wins.values_mut() {
            if w.desk == id {
                w.desk = heir;
            }
        }
        if self.current == id {
            self.current = heir;
            let keep = self.focused.filter(|f| self.is_shown(*f));
            return Some(Refocus::To(keep.or_else(|| self.next_focus(None))));
        }
        Some(Refocus::Same)
    }

    pub fn rename_desk(&mut self, id: DeskId, name: String) -> bool {
        self.generation += 1;
        match self.desks.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.name = name;
                true
            }
            None => false,
        }
    }

    pub fn set_desk_folder(&mut self, id: DeskId, folder: Option<String>) -> bool {
        self.generation += 1;
        match self.desks.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.folder = folder;
                true
            }
            None => false,
        }
    }

    /// Moves a Desk to a 1-based position.
    pub fn move_desk(&mut self, id: DeskId, index: u32) -> bool {
        self.generation += 1;
        let Some(i) = self.desks.iter().position(|d| d.id == id) else { return false };
        let d = self.desks.remove(i);
        let at = (index as usize).saturating_sub(1).min(self.desks.len());
        self.desks.insert(at, d);
        true
    }

    /// Replaces the Desks' names and folders from the settings, keeping
    /// identities by position; extra Desks are added, missing ones removed
    /// (their windows moving to the last one kept).
    pub fn sync_desks(&mut self, wanted: &[(String, Option<String>)]) -> bool {
        self.generation += 1;
        let mut changed = false;
        for (i, (name, folder)) in wanted.iter().enumerate() {
            if let Some(d) = self.desks.get_mut(i) {
                if d.name != *name || d.folder != *folder {
                    d.name = name.clone();
                    d.folder = folder.clone();
                    changed = true;
                }
            } else {
                self.push_desk(name.clone(), folder.clone());
                changed = true;
            }
        }
        while self.desks.len() > wanted.len().max(1) {
            let id = self.desks.last().unwrap().id;
            self.remove_desk(id);
            changed = true;
        }
        changed
    }
}

/// The part of a usable area beside the Deskbar, for filling and splitting
/// (as Haiku's zoom leaves its Deskbar clear): the Deskbar stands against
/// one side, so the area stops at its edge. A Deskbar that stands against
/// neither side, or isn't there (collapsed), leaves the area whole.
pub fn beside(usable: Rect, deskbar: Option<Rect>) -> Rect {
    let Some(d) = deskbar.filter(|d| d.intersects(usable)) else { return usable };
    let mut r = usable;
    if d.right() >= usable.right() && d.x > usable.x + usable.w / 2 {
        r.w = d.x - usable.x;
    } else if d.x <= usable.x && d.right() < usable.x + usable.w / 2 {
        r.w = usable.right() - d.right();
        r.x = d.right();
    }
    r
}

/// Moves a content rect so its frame and tab lie inside `area`, as far as
/// they fit.
pub fn clamp_into(mut r: Rect, area: Rect, insets: Insets) -> Rect {
    let min_x = area.x + insets.left;
    let min_y = area.y + insets.top;
    let max_x = area.right() - insets.right - r.w;
    let max_y = area.bottom() - insets.bottom - r.h;
    r.x = r.x.min(max_x).max(min_x);
    r.y = r.y.min(max_y).max(min_y);
    r
}

/// Automatic placement may shrink an oversized client as well as move it.
/// Explicit user moves keep using `clamp_into` where only position is wanted.
pub fn fit_into(mut r: Rect, area: Rect, insets: Insets) -> Rect {
    let bounds = area.inset(insets);
    r.w = r.w.clamp(1, bounds.w);
    r.h = r.h.clamp(1, bounds.h);
    clamp_into(r, area, insets)
}
