//! Sizes, constraints, padding and alignment: what layout is made of.
//!
//! Layout runs top-down with constraints and returns sizes bottom-up, as
//! Flutter's does: a parent hands each child the smallest and largest size
//! it may take, the child picks its size, and the parent places it. An
//! element's [`Length`]s narrow what its parent offers before it chooses.

pub use sidestep_ui::kurbo::{Insets, Point, Rect, Size, Vec2};

/// How much room an element takes along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length {
    /// As little as its content needs (the default).
    #[default]
    Shrink,
    /// All the room its parent offers; in a row or column, an equal share
    /// of what the other children leave.
    Fill,
    /// In a row or column, this many shares of what the other children
    /// leave; elsewhere, like [`Length::Fill`].
    FillPortion(u16),
    /// Exactly this many points (as far as the parent allows).
    Fixed(f64),
}

impl Length {
    /// Its shares of the leftover space, if it fills.
    pub fn portion(self) -> Option<u16> {
        match self {
            Length::Fill => Some(1),
            Length::FillPortion(n) => Some(n.max(1)),
            _ => None,
        }
    }

    pub fn is_fill(self) -> bool {
        self.portion().is_some()
    }
}

impl From<f64> for Length {
    fn from(v: f64) -> Length {
        Length::Fixed(v)
    }
}

impl From<f32> for Length {
    fn from(v: f32) -> Length {
        Length::Fixed(f64::from(v))
    }
}

impl From<i32> for Length {
    fn from(v: i32) -> Length {
        Length::Fixed(f64::from(v))
    }
}

impl From<u32> for Length {
    fn from(v: u32) -> Length {
        Length::Fixed(f64::from(v))
    }
}

/// Shorthands: `width(Fill)`.
pub use Length::{Fill, Shrink};

/// Space inside an element's edges, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Padding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Padding {
    pub const ZERO: Padding = Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 };

    pub const fn all(v: f64) -> Padding {
        Padding { top: v, right: v, bottom: v, left: v }
    }

    /// `vertical` above and below, `horizontal` left and right.
    pub const fn symmetric(vertical: f64, horizontal: f64) -> Padding {
        Padding { top: vertical, right: horizontal, bottom: vertical, left: horizontal }
    }

    pub fn horizontal(&self) -> f64 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
    }

    pub fn size(&self) -> Size {
        Size::new(self.horizontal(), self.vertical())
    }
}

impl From<f64> for Padding {
    fn from(v: f64) -> Padding {
        Padding::all(v)
    }
}

impl From<i32> for Padding {
    fn from(v: i32) -> Padding {
        Padding::all(f64::from(v))
    }
}

/// `[vertical, horizontal]`, as CSS's two-value padding.
impl From<[f64; 2]> for Padding {
    fn from([v, h]: [f64; 2]) -> Padding {
        Padding::symmetric(v, h)
    }
}

/// `[top, right, bottom, left]`, as CSS's four-value padding.
impl From<[f64; 4]> for Padding {
    fn from([top, right, bottom, left]: [f64; 4]) -> Padding {
        Padding { top, right, bottom, left }
    }
}

/// Where a child sits along an axis with room to spare.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

impl Align {
    /// The offset of something `size` long in `room`.
    pub fn offset(self, size: f64, room: f64) -> f64 {
        let spare = (room - size).max(0.0);
        match self {
            Align::Start => 0.0,
            Align::Center => (spare / 2.0).round(),
            Align::End => spare,
        }
    }
}

/// The sizes a child may take: from `min` to `max` (which may be
/// infinite: a scroll view's content may be as tall as it likes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    pub min: Size,
    pub max: Size,
}

impl Constraints {
    /// Anything up to `max`.
    pub fn loose(max: Size) -> Constraints {
        Constraints { min: Size::ZERO, max }
    }

    /// Exactly `size`.
    pub fn tight(size: Size) -> Constraints {
        Constraints { min: size, max: size }
    }

    pub const UNBOUNDED: Constraints = Constraints { min: Size::ZERO, max: Size::new(f64::INFINITY, f64::INFINITY) };

    /// `size` brought within the constraints.
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min.width, self.max.width.max(self.min.width)),
            size.height.clamp(self.min.height, self.max.height.max(self.min.height)),
        )
    }

    /// The constraints for what's inside `padding`.
    pub fn shrink(&self, padding: Padding) -> Constraints {
        let (h, v) = (padding.horizontal(), padding.vertical());
        Constraints {
            min: Size::new((self.min.width - h).max(0.0), (self.min.height - v).max(0.0)),
            max: Size::new((self.max.width - h).max(0.0), (self.max.height - v).max(0.0)),
        }
    }

    /// The same, with no minimum.
    pub fn loosen(&self) -> Constraints {
        Constraints { min: Size::ZERO, max: self.max }
    }

    pub fn with_max_width(mut self, w: f64) -> Constraints {
        self.max.width = w.max(0.0);
        self.min.width = self.min.width.min(self.max.width);
        self
    }

    pub fn with_max_height(mut self, h: f64) -> Constraints {
        self.max.height = h.max(0.0);
        self.min.height = self.min.height.min(self.max.height);
        self
    }

    /// Exactly `w` wide (within the maximum).
    pub fn with_width(mut self, w: f64) -> Constraints {
        let w = w.min(self.max.width).max(0.0);
        self.min.width = w;
        self.max.width = w;
        self
    }

    pub fn with_height(mut self, h: f64) -> Constraints {
        let h = h.min(self.max.height).max(0.0);
        self.min.height = h;
        self.max.height = h;
        self
    }

    pub fn is_tight(&self) -> bool {
        self.min == self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_clamp_and_shrink() {
        let c = Constraints { min: Size::new(10.0, 10.0), max: Size::new(100.0, 50.0) };
        assert_eq!(c.constrain(Size::new(5.0, 80.0)), Size::new(10.0, 50.0));
        let inner = c.shrink(Padding::symmetric(4.0, 8.0));
        assert_eq!(inner.max, Size::new(84.0, 42.0));
        assert_eq!(inner.min, Size::new(0.0, 2.0));
        assert_eq!(Constraints::UNBOUNDED.with_width(30.0).max.width, 30.0);
        assert_eq!(c.with_width(300.0).min.width, 100.0);
    }

    #[test]
    fn alignment_rounds_to_points() {
        assert_eq!(Align::Center.offset(10.0, 21.0), 6.0);
        assert_eq!(Align::End.offset(10.0, 21.0), 11.0);
        assert_eq!(Align::Start.offset(30.0, 21.0), 0.0);
    }
}
