//! Integer rectangles in points, free of smithay's typed geometry so the
//! model stays plain. Shared by the core (what windows show where) and the
//! logic (the model), and saved across reloads.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64 && y >= self.y as f64 && x < self.right() as f64 && y < self.bottom() as f64
    }

    pub fn intersects(&self, o: Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    pub fn intersection(&self, o: Rect) -> Option<Rect> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        (r > x && b > y).then(|| Rect::new(x, y, r - x, b - y))
    }

    pub fn area(&self) -> i64 {
        if self.is_empty() { 0 } else { self.w as i64 * self.h as i64 }
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Shrinks by insets.
    pub fn inset(&self, i: Insets) -> Rect {
        Rect::new(
            self.x + i.left,
            self.y + i.top,
            (self.w - i.left - i.right).max(1),
            (self.h - i.top - i.bottom).max(1),
        )
    }

    /// Grows by insets.
    pub fn outset(&self, i: Insets) -> Rect {
        Rect::new(self.x - i.left, self.y - i.top, self.w + i.left + i.right, self.h + i.top + i.bottom)
    }

    pub fn grow(&self, d: i32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2 * d, self.h + 2 * d)
    }

    pub fn union(&self, o: Rect) -> Rect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return *self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }
}

/// Space around a rectangle, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Insets {
    pub top: i32,
    pub left: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Insets {
    pub const fn new(top: i32, left: i32, right: i32, bottom: i32) -> Insets {
        Insets { top, left, right, bottom }
    }
}
