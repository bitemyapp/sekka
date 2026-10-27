//! Elements: what a program's `view` returns, a tree of widgets described
//! anew each time, with keys for identity.
//!
//! An [`Element<Msg>`] is a widget with its layout settings ([`Common`])
//! and its children. Messages are typed where an element is built
//! (`button("OK").on_press(Msg::Ok)`) and travel untyped inside the kit,
//! so [`Element::map`] can put one program's elements inside another's.

use std::any::Any;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use crate::layout::{Length, Size};
use crate::widget::Widget;

/// A message on its way through the kit, typed again when it reaches
/// the program.
pub type AnyMsg = Box<dyn Any + Send>;

/// Makes a message each time it's asked: what `on_press(msg)` keeps.
#[derive(Clone)]
pub struct MsgFn(Rc<dyn Fn() -> AnyMsg>);

impl MsgFn {
    pub fn new<Msg: Clone + Send + 'static>(msg: Msg) -> MsgFn {
        MsgFn(Rc::new(move || Box::new(msg.clone())))
    }

    /// A message made by `f` each time.
    pub fn from_fn(f: impl Fn() -> AnyMsg + 'static) -> MsgFn {
        MsgFn(Rc::new(f))
    }

    pub fn make(&self) -> AnyMsg {
        (self.0)()
    }
}

impl std::fmt::Debug for MsgFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MsgFn")
    }
}

/// Makes a message from a value: what `on_change(Msg::Name)` keeps.
pub struct MsgMap<T>(Rc<dyn Fn(T) -> AnyMsg>);

impl<T> Clone for MsgMap<T> {
    fn clone(&self) -> Self {
        MsgMap(self.0.clone())
    }
}

impl<T: 'static> MsgMap<T> {
    pub fn new<Msg: Send + 'static>(f: impl Fn(T) -> Msg + 'static) -> MsgMap<T> {
        MsgMap(Rc::new(move |t| Box::new(f(t))))
    }

    pub fn make(&self, value: T) -> AnyMsg {
        (self.0)(value)
    }
}

/// Turns a subtree's messages into its parent's ([`Element::map`]).
pub(crate) type Mapper = Rc<dyn Fn(AnyMsg) -> AnyMsg>;

/// What every element has besides its widget: identity, size and the
/// things the kit does for any element (tooltips, names for focus).
#[derive(Clone, Default)]
pub struct Common {
    pub(crate) key: Option<u64>,
    /// A name to find the element by ([`Cx::focus`](crate::Cx::focus)).
    pub(crate) name: Option<Arc<str>>,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) min_width: Option<f64>,
    pub(crate) max_width: Option<f64>,
    pub(crate) min_height: Option<f64>,
    pub(crate) max_height: Option<f64>,
    pub(crate) mapper: Option<Mapper>,
    pub(crate) tooltip: Option<Arc<str>>,
    /// What assistive technology calls it, over what the widget says.
    pub(crate) label: Option<Arc<str>>,
}

impl Common {
    pub(crate) fn sized(width: Length, height: Length) -> Common {
        Common { width, height, ..Common::default() }
    }

    /// A size kept within the element's minimum and maximum.
    pub(crate) fn clamp(&self, s: Size) -> Size {
        let w = s.width.max(self.min_width.unwrap_or(0.0)).min(self.max_width.unwrap_or(f64::INFINITY));
        let h = s.height.max(self.min_height.unwrap_or(0.0)).min(self.max_height.unwrap_or(f64::INFINITY));
        Size::new(w, h)
    }
}

/// An element without its message type: what the kit keeps.
pub struct RawElement {
    pub(crate) widget: Box<dyn Widget>,
    pub(crate) common: Common,
    pub(crate) children: Vec<RawElement>,
}

impl RawElement {
    pub fn new(widget: impl Widget, common: Common, children: Vec<RawElement>) -> RawElement {
        RawElement { widget: Box::new(widget), common, children }
    }
}

/// A piece of interface: a widget, how it's sized, and its children.
pub struct Element<Msg> {
    pub(crate) raw: RawElement,
    _msg: PhantomData<fn() -> Msg>,
}

impl<Msg> Element<Msg> {
    /// An element of a widget written outside the kit.
    pub fn new(widget: impl Widget, children: Vec<Element<Msg>>) -> Element<Msg> {
        Element::from_raw(RawElement::new(widget, Common::default(), children.into_iter().map(|c| c.raw).collect()))
    }

    pub(crate) fn from_raw(raw: RawElement) -> Element<Msg> {
        Element { raw, _msg: PhantomData }
    }

    pub fn into_raw(self) -> RawElement {
        self.raw
    }

    /// The same element, its messages (and its children's) turned into
    /// another type: a component's elements inside its parent's view.
    pub fn map<B: Send + 'static>(mut self, f: impl Fn(Msg) -> B + 'static) -> Element<B>
    where
        Msg: 'static,
    {
        let map: Mapper = Rc::new(move |m: AnyMsg| match m.downcast::<Msg>() {
            Ok(m) => Box::new(f(*m)),
            // The kit's own messages pass through.
            Err(other) => other,
        });
        self.raw.common.mapper = Some(match self.raw.common.mapper.take() {
            None => map,
            Some(inner) => Rc::new(move |m| map(inner(m))),
        });
        Element { raw: self.raw, _msg: PhantomData }
    }
}

/// What every element builder can be given: a key, a name, a size, a
/// tooltip. Builders keep their own type, so these go in any order with
/// their own settings.
pub trait Layoutable: Sized {
    fn common_mut(&mut self) -> &mut Common;

    /// Identity among siblings, so state (a caret, a scroll offset, an
    /// open menu) follows the element when siblings come and go.
    fn key(mut self, key: impl Hash) -> Self {
        let mut h = crate::hash::Fx::default();
        key.hash(&mut h);
        self.common_mut().key = Some(h.finish());
        self
    }

    /// A name to find it by from the program: [`Cx::focus`](crate::Cx::focus).
    fn id(mut self, name: impl Into<Arc<str>>) -> Self {
        self.common_mut().name = Some(name.into());
        self
    }

    fn width(mut self, width: impl Into<Length>) -> Self {
        self.common_mut().width = width.into();
        self
    }

    fn height(mut self, height: impl Into<Length>) -> Self {
        self.common_mut().height = height.into();
        self
    }

    fn min_width(mut self, w: f64) -> Self {
        self.common_mut().min_width = Some(w);
        self
    }

    fn max_width(mut self, w: f64) -> Self {
        self.common_mut().max_width = Some(w);
        self
    }

    fn min_height(mut self, h: f64) -> Self {
        self.common_mut().min_height = Some(h);
        self
    }

    fn max_height(mut self, h: f64) -> Self {
        self.common_mut().max_height = Some(h);
        self
    }

    /// Text shown after the pointer rests on it a moment.
    fn tooltip(mut self, text: impl Into<Arc<str>>) -> Self {
        self.common_mut().tooltip = Some(text.into());
        self
    }

    /// What assistive technology announces it as.
    fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.common_mut().label = Some(label.into());
        self
    }
}

impl<Msg> Layoutable for Element<Msg> {
    fn common_mut(&mut self) -> &mut Common {
        &mut self.raw.common
    }
}

/// Builders become elements: implemented for each widget's builder.
#[macro_export]
#[doc(hidden)]
macro_rules! builder {
    ($ty:ident) => {
        impl<Msg> $crate::element::Layoutable for $ty<Msg> {
            fn common_mut(&mut self) -> &mut $crate::element::Common {
                &mut self.common
            }
        }
    };
}

/// A list of elements from anything that becomes them: what the
/// `column!`, `row!` and `stack!` macros build.
pub fn children<Msg>(items: impl IntoIterator<Item = impl Into<Element<Msg>>>) -> Vec<Element<Msg>> {
    items.into_iter().map(Into::into).collect()
}
