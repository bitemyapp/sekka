//! Dragging inside a window: where things can be dropped.
//!
//! Sidestep has no drag source of its own yet, so a drag that starts in
//! Hako is a gesture: the view that took the press keeps the pointer and
//! asks the window's [`Spots`] what's under it as it moves. Every place
//! that takes a drop (each view, each sidebar row, each crumb of the
//! path bar) says where it is, in window points, whenever it's drawn.
//! Drags from other programs come through Sidestep's drag and drop, as
//! `file://` URIs, and are dropped through the same places.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use aizome_kit::{Point, Rect};

/// Where a drop goes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Dest {
    /// Into a folder.
    Folder(PathBuf),
    /// To the Trash.
    Trash,
}

/// What's under the pointer during a drag: where a drop would go, and
/// what shows it (a spot, and a row of it).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub dest: Dest,
    pub spot: u64,
    pub row: Option<usize>,
}

/// Finds the target at a window point, within a spot.
pub type Resolve = Rc<dyn Fn(Point) -> Option<Target>>;

/// A place that takes drops.
#[derive(Clone)]
pub struct Spot {
    pub rect: Rect,
    /// Spots of the sidebar and the path bar come before the views', which
    /// can be scrolled under them.
    pub priority: u8,
    pub resolve: Resolve,
}

/// Spot IDs: views are their column's number; the rest are offset.
pub const SIDEBAR: u64 = 1 << 20;
pub const PATHBAR: u64 = 2 << 20;

/// A window's spots, by ID, and where the window itself is.
#[derive(Clone, Default)]
pub struct Spots {
    map: Rc<RefCell<BTreeMap<u64, Spot>>>,
    window: Rc<Cell<Rect>>,
}

impl Spots {
    pub fn set(&self, id: u64, spot: Spot) {
        self.map.borrow_mut().insert(id, spot);
    }

    /// Forget spots no longer shown.
    pub fn retain(&self, keep: impl Fn(u64) -> bool) {
        self.map.borrow_mut().retain(|id, _| keep(*id));
    }

    /// Where the window's content is, as last drawn (a drag that leaves
    /// it goes on as the desktop's).
    pub fn set_window(&self, r: Rect) {
        self.window.set(r);
    }

    pub fn window_bounds(&self) -> Option<Rect> {
        let r = self.window.get();
        (r.area() > 0.0).then_some(r)
    }

    /// The target at `p` (window points).
    pub fn at(&self, p: Point) -> Option<Target> {
        let spots = self.map.borrow();
        let mut found: Vec<&Spot> = spots.values().filter(|s| s.rect.contains(p)).collect();
        found.sort_by_key(|s| std::cmp::Reverse(s.priority));
        found.into_iter().find_map(|s| (s.resolve)(p))
    }

    pub fn len(&self) -> usize {
        self.map.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A drag's course, as a view tells it.
#[derive(Clone, Debug, PartialEq)]
pub enum DragEvent {
    /// It started, from the selection of the view.
    Start,
    /// What's under it now.
    Over(Option<Target>),
    /// Let go: onto a target (if any), copying (⌥ or ⌃ held) or not.
    Drop { target: Option<Target>, copy: bool },
    /// Escape, or the pointer lost.
    Cancel,
    /// It left the window (at this window point): it goes on as the
    /// desktop's drag, to other programs.
    Leave(Point),
}

/// Points the pointer must move with the button down before it drags.
pub const SLOP: f64 = 5.0;

/// `child`, telling `spots` where the window's content is whenever it's
/// drawn: the window's whole view goes in one.
pub fn window_bounds<Msg: 'static>(child: aizome_kit::Element<Msg>, spots: Spots) -> aizome_kit::Element<Msg> {
    use aizome_kit::Layoutable;
    aizome_kit::Element::new(Bounds { spots }, vec![child])
        .width(aizome_kit::Length::Fill)
        .height(aizome_kit::Length::Fill)
}

struct Bounds {
    spots: Spots,
}

impl aizome_kit::widget::Widget for Bounds {
    fn name(&self) -> &'static str {
        "hako-window-bounds"
    }

    fn layout(&mut self, cx: &mut aizome_kit::widget::LayoutCx, bc: aizome_kit::Constraints) -> aizome_kit::Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn paint(&self, cx: &mut aizome_kit::widget::PaintCx) {
        let origin = cx.canvas.transform().translation();
        self.spots.set_window(cx.bounds() + origin);
    }
}
