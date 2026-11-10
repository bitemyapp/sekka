//! Subtrees kept from one view to the next: [`lazy`].
//!
//! A message runs the program's whole `view`, and the kit mounts, lays out
//! and compares every element it returns. Most of a window is as it was. A
//! lazy element says so for a subtree: it holds the value the subtree is
//! built from and the function that builds it, and when the next view has
//! the same value and function in the same place, the kit keeps the nodes,
//! layout and hashes it has and doesn't call the function.
//!
//! What can go wrong with keeping is showing what a subtree was after what
//! it's built from has changed. The types rule out the usual way there: the
//! function is a `fn` item or a closure that captures nothing, so what it
//! builds can depend on its value alone, and the value it's given is the one
//! the kit compares. There is no separate key to forget a field in.

use std::any::Any;
use std::num::NonZeroU16;

use crate::element::{Common, Element, RawElement};
use crate::layout::{Constraints, Length, Point, Size};
use crate::widget::{LayoutCx, PopoverContent, Widget};

/// What `build` makes of `value`, kept for as long as the view gives the
/// same value: the function isn't called again, and the subtree's nodes,
/// layout and drawing stay as they are, with its widgets' state.
///
/// ```
/// use aizome_kit::prelude::*;
///
/// #[derive(Clone, PartialEq)]
/// struct Row { name: String, done: bool }
///
/// #[derive(Clone, Debug)]
/// enum Msg { Toggle(String, bool) }
///
/// fn row(r: &Row) -> Element<Msg> {
///     let name = r.name.clone();
///     aizome_kit::row![text(r.name.clone()), spacer(), toggle(r.done).on_toggle(move |on| Msg::Toggle(name.clone(), on))].into()
/// }
///
/// fn rows(all: &[Row]) -> Element<Msg> {
///     aizome_kit::widgets::layout::column(all.iter().map(|r| lazy(r.clone(), row).key(&r.name)).collect()).into()
/// }
/// ```
///
/// `build` is a function or a closure that captures nothing, so everything
/// the subtree shows comes from `value`: the kit compares the very value
/// the function reads. A closure that reads something else doesn't compile:
///
/// ```compile_fail,E0308
/// use aizome_kit::prelude::*;
///
/// fn stale(title: String, count: u32) -> Element<()> {
///     // `title` isn't part of the value compared: a change to it alone
///     // would go unseen.
///     lazy(count, move |count| text(format!("{title}: {count}")).into())
/// }
/// ```
///
/// ```
/// use aizome_kit::prelude::*;
///
/// fn fresh(title: String, count: u32) -> Element<()> {
///     lazy((title, count), |(title, count)| text(format!("{title}: {count}")).into())
/// }
/// ```
///
/// What that leaves to the app: `value`'s `==` must tell apart any two
/// values `build` would draw differently (one written by hand that skips a
/// field, or a handle to something that changes inside, would not), and
/// `build` must not read what isn't in its argument (a static, a clock, a
/// file). The theme, the window's size and scale, focus, hover and widgets'
/// own state are the kit's to follow: a kept subtree is laid out again when
/// the room it's given changes, and redrawn when its look does, without a
/// call to `build`.
///
/// Give a lazy element a [`key`](crate::Layoutable::key) where siblings come,
/// go or change places, as for any element: it's kept by its place among
/// its siblings otherwise. It takes the size its content asks for unless
/// given one of its own.
pub fn lazy<D, Msg>(value: D, build: fn(&D) -> Element<Msg>) -> Element<Msg>
where
    D: PartialEq + 'static,
    Msg: 'static,
{
    Element::from_raw(RawElement::new(LazyWidget { value, build, flex: None }, Common::default(), Vec::new()))
}

/// What a widget is to the tree as it's mounted: nearly always nothing in
/// particular.
///
/// Only the kit's own widgets say otherwise: the type is out of reach from
/// outside the crate, so [`Widget::at_mount`] can't be written there.
// As large as the popover it may hold, as the `Option` of one it stands in
// for was: it is made where it is read, once for each widget mounted.
#[allow(clippy::large_enum_variant)]
pub enum AtMount<'a> {
    Plain,
    /// A popover: its content shows in a popup of its own.
    Popover(PopoverContent),
    /// A lazy element: its content is built now, or kept from before.
    Lazy(&'a mut dyn Lazy),
}

/// A lazy element as the tree sees it, whatever its value's type.
pub trait Lazy {
    /// Whether `old`, the widget of the node this element was at the last
    /// view, was built from an equal value by the same function: then what's
    /// under that node is what this element would build.
    fn same(&self, old: &dyn Any) -> Option<Same>;

    /// What it shows, built from its value.
    fn build(&mut self) -> RawElement;
}

/// Proof that a lazy element's value and function are those its node was
/// built from: what [`Lazy::same`] found, and the one way the tree keeps a
/// subtree in place of building it.
pub struct Same(());

struct LazyWidget<D, Msg> {
    value: D,
    build: fn(&D) -> Element<Msg>,
    /// Its content's share of a row's or column's leftover room, as of when
    /// it was built: a lazy element stands where its content would.
    flex: Option<NonZeroU16>,
}

impl<D: PartialEq + 'static, Msg: 'static> Lazy for LazyWidget<D, Msg> {
    fn same(&self, old: &dyn Any) -> Option<Same> {
        // Another value type or message type is another widget type.
        let old = old.downcast_ref::<Self>()?;
        // Two functions at one address do the same thing; one function at
        // two addresses (as code generation may leave it) builds again,
        // which costs time and nothing else.
        (std::ptr::fn_addr_eq(self.build, old.build) && self.value == old.value).then_some(Same(()))
    }

    fn build(&mut self) -> RawElement {
        let content = (self.build)(&self.value).into_raw();
        self.flex = content.widget.flex();
        content
    }
}

impl<D: PartialEq + 'static, Msg: 'static> Widget for LazyWidget<D, Msg> {
    fn name(&self) -> &'static str {
        "lazy"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        // Its content's node has the same lengths (`take_after`), so the
        // room it's given is the room its content would have been given.
        if cx.child_count() == 0 {
            return bc.min;
        }
        let size = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        size
    }

    fn flex(&self) -> Option<NonZeroU16> {
        self.flex
    }

    fn at_mount(&mut self) -> AtMount<'_> {
        AtMount::Lazy(self)
    }
}

/// The lengths that say how an element is sized.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Lengths {
    width: Length,
    height: Length,
    min_width: Option<f64>,
    max_width: Option<f64>,
    min_height: Option<f64>,
    max_height: Option<f64>,
}

impl Lengths {
    pub(crate) fn of(c: &Common) -> Lengths {
        Lengths {
            width: c.width,
            height: c.height,
            min_width: c.min_width,
            max_width: c.max_width,
            min_height: c.min_height,
            max_height: c.max_height,
        }
    }
}

/// Size a lazy element as its content is sized, wherever it was given no
/// length of its own: its parent then lays it out as it would the content.
pub(crate) fn take_after(lazy: &mut Common, content: Lengths) {
    if lazy.width == Length::Shrink {
        lazy.width = content.width;
    }
    if lazy.height == Length::Shrink {
        lazy.height = content.height;
    }
    lazy.min_width = lazy.min_width.or(content.min_width);
    lazy.max_width = lazy.max_width.or(content.max_width);
    lazy.min_height = lazy.min_height.or(content.min_height);
    lazy.max_height = lazy.max_height.or(content.max_height);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::text;

    fn label(n: &u32) -> Element<()> {
        text(format!("{n}")).into()
    }

    fn other(n: &u32) -> Element<()> {
        text(format!("#{n}")).into()
    }

    fn widget(el: Element<()>) -> Box<dyn Widget> {
        el.into_raw().widget
    }

    fn same(new: Element<()>, old: Element<()>) -> bool {
        let (mut new, old) = (widget(new), widget(old));
        let old: &dyn Any = old.as_ref();
        match new.at_mount() {
            AtMount::Lazy(l) => l.same(old).is_some(),
            _ => panic!("a lazy element"),
        }
    }

    #[test]
    fn a_lazy_element_is_the_same_only_for_its_value_function_and_types() {
        assert!(same(lazy(7u32, label), lazy(7u32, label)));
        assert!(!same(lazy(8u32, label), lazy(7u32, label)), "another value");
        assert!(!same(lazy(7u32, other), lazy(7u32, label)), "another function");
        // The same bits as another type are another widget.
        fn wide(n: &u64) -> Element<()> {
            text(format!("{n}")).into()
        }
        assert!(!same(lazy(7u64, wide), lazy(7u32, label)), "another type");
        assert!(!same(lazy(7u32, label), text("7").into()), "not a lazy element");
        // A value that isn't equal to itself is never kept.
        fn real(n: &f64) -> Element<()> {
            text(format!("{n}")).into()
        }
        assert!(!same(lazy(f64::NAN, real), lazy(f64::NAN, real)));
    }

    #[test]
    fn a_lazy_element_takes_its_content_s_lengths_but_keeps_its_own() {
        let content = Lengths::of(&Common { width: Length::Fill, min_height: Some(16.0), ..Common::default() });
        let mut plain = Common::default();
        take_after(&mut plain, content);
        assert_eq!((plain.width, plain.height, plain.min_height), (Length::Fill, Length::Shrink, Some(16.0)));
        let mut sized = Common { width: Length::Fixed(40.0), min_height: Some(4.0), ..Common::default() };
        take_after(&mut sized, content);
        assert_eq!((sized.width, sized.min_height), (Length::Fixed(40.0), Some(4.0)));
    }
}
