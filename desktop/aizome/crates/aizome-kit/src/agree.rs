//! A person's agreement to a destructive step.
//!
//! A button made with [`Button::on_agree`](crate::widgets::controls::Button::on_agree)
//! (or an alert's [`agree_button`](crate::Alert::agree_button), drawn
//! destructive) carries a target: what the alert, sheet or page shows
//! the person it will act on. When a person activates it, with the
//! pointer (a press and a release over it) or the keyboard (Space, or
//! Return while it has the focus), the kit makes an [`Agreed`] of that
//! target and sends it, in a [`Pressed`], with the button's message. A
//! destructive step takes the `Agreed` by value and acts on its target, so:
//!
//! - only the kit makes one, and only from input: program code can't write
//!   one, send a message carrying one, or press the button for itself (the
//!   testing [`Harness`](crate::testing::Harness) presses make none unless
//!   a test asks, which only a test build can);
//! - it holds the target the button showed, which nothing can change: a
//!   step acts on what the person agreed to, not on a target passed beside
//!   it;
//! - it's spent by the step that takes it, and can't be copied.
//!
//! What it doesn't show:
//!
//! - **That a person, rather than a program, moved the pointer.** The kit
//!   takes input from the compositor; whatever the compositor accepts as
//!   input (a virtual pointer or keyboard, `uinput`) presses buttons as a
//!   person does. A privileged helper can't tell either: an agreement is a
//!   value in the window's process, and what the window sends a helper is
//!   only bytes, which the helper must check for itself as it does now.
//! - **That the target is what the view drew.** The kit binds the agreement
//!   to the value the program gave the button, not to the pixels around it.
//!   Build the view from the target (as Tansu's Apply sheet lists the plan
//!   it agrees to) so they can't drift apart.
//!
//! ```
//! use aizome_kit::agree::{Agreed, Pressed};
//! # #[derive(Clone, Debug)] struct Disk { serial: String }
//! #[derive(Clone, Debug)]
//! enum Msg {
//!     /// The person pressed Erase on the disk the sheet showed.
//!     Erase(Pressed<Disk>),
//! }
//! fn erase(disk: Agreed<Disk>) { /* acts on disk.target() */ let _ = disk; }
//! fn update(msg: Msg) {
//!     match msg {
//!         // `None` only for a clone of the message whose agreement another
//!         // clone took first.
//!         Msg::Erase(pressed) => if let Some(agreed) = pressed.take() { erase(agreed) },
//!     }
//! }
//! ```

use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::element::AnyMsg;

/// Proof that a person agreed to `target`, by activating a destructive
/// button that showed it. Only the kit makes one, as a person presses the
/// button; take it from the button's message with [`Pressed::take`].
///
/// Program code can't write one:
///
/// ```compile_fail,E0451
/// use aizome_kit::agree::Agreed;
/// let _ = Agreed { target: "/dev/sdb", made: todo!() };
/// ```
///
/// nor copy one to use again:
///
/// ```compile_fail,E0599
/// fn twice(agreed: aizome_kit::agree::Agreed<String>) {
///     let kept = agreed.clone();
///     drop((agreed, kept));
/// }
/// ```
///
/// A step that takes it spends it; it can't be used again after:
///
/// ```compile_fail,E0382
/// use aizome_kit::agree::Agreed;
/// fn erase(disk: Agreed<String>) { drop(disk) }
/// fn erase_twice(disk: Agreed<String>) {
///     erase(disk);
///     erase(disk);
/// }
/// ```
///
/// ```
/// use aizome_kit::agree::Agreed;
/// fn erase(disk: Agreed<String>) { drop(disk) }
/// fn erase_once(disk: Agreed<String>) {
///     erase(disk);
/// }
/// ```
///
/// and its target can't be changed for another:
///
/// ```compile_fail,E0616
/// use aizome_kit::agree::Agreed;
/// fn retarget(mut disk: Agreed<String>, other: String) -> Agreed<String> {
///     disk.target = other;
///     disk
/// }
/// ```
///
/// ```
/// use aizome_kit::agree::Agreed;
/// fn target(disk: &Agreed<String>) -> &str {
///     disk.target()
/// }
/// ```
///
/// Tests get agreements from `agree::testing`, or from the testing
/// harness's presses once they count (`Harness::allow_agreements`). A
/// shipped program's build has neither: only the kit's own tests and builds
/// with its `testing` feature, which only dev-dependencies turn on, do.
#[cfg_attr(not(feature = "testing"), doc = "```compile_fail,E0433")]
#[cfg_attr(feature = "testing", doc = "```ignore")]
/// let _ = aizome_kit::agree::testing::agreed("/dev/sdb");
/// ```
///
#[cfg_attr(not(feature = "testing"), doc = "```compile_fail,E0599")]
#[cfg_attr(feature = "testing", doc = "```ignore")]
/// # use aizome_kit::prelude::*;
/// # #[derive(Default)] struct App; #[derive(Clone)] enum Msg {}
/// # impl Program for App { type Msg = Msg; fn info(&self) -> AppInfo { AppInfo::new("a", "A", "a") }
/// # fn launched(&mut self, cx: &mut Cx<Msg>) { cx.open_window(WindowSpec::new("A")); }
/// # fn update(&mut self, _: Msg, _: &mut Cx<Msg>) {} fn view(&self, _: WindowKey) -> Element<Msg> { text("Erase").into() } }
/// let mut h = aizome_kit::testing::Harness::new(App::default());
/// h.allow_agreements();
/// ```
///
/// The harness itself is there, in every build:
///
/// ```no_run
/// # use aizome_kit::prelude::*;
/// # #[derive(Default)] struct App; #[derive(Clone)] enum Msg {}
/// # impl Program for App { type Msg = Msg; fn info(&self) -> AppInfo { AppInfo::new("a", "A", "a") }
/// # fn launched(&mut self, cx: &mut Cx<Msg>) { cx.open_window(WindowSpec::new("A")); }
/// # fn update(&mut self, _: Msg, _: &mut Cx<Msg>) {} fn view(&self, _: WindowKey) -> Element<Msg> { text("Erase").into() } }
/// let mut h = aizome_kit::testing::Harness::new(App::default());
/// let w = h.window();
/// h.click_text(w, "Erase");
/// ```
pub struct Agreed<T> {
    target: T,
    made: Made,
}

/// Why `Agreed` can't be written as a literal outside the kit.
struct Made;

/// What the kit's input path holds when it makes an agreement: a person
/// pressed a button (or a test asked for its presses to count).
pub(crate) struct Mint(());

impl Mint {
    /// For [`Core::apply`](crate::engine::Core), handling a button's press
    /// from the platform's input, while presses make agreements.
    pub(crate) fn by_input() -> Mint {
        Mint(())
    }
}

impl<T> Agreed<T> {
    fn made(target: T, _: Mint) -> Agreed<T> {
        Agreed { target, made: Made }
    }

    /// What the person agreed to.
    pub fn target(&self) -> &T {
        &self.target
    }

    /// What the person agreed to, spending the agreement (for the step it
    /// allows, which should own the target then).
    pub fn into_target(self) -> T {
        let Agreed { target, made: Made } = self;
        target
    }
}

impl<T> Agreed<Vec<T>> {
    /// One agreement for each item, in order: the person agreed to the whole
    /// list as shown, so to each item of it (Tansu's Apply sheet agrees to its
    /// operations together, which run one at a time).
    pub fn split(self) -> Vec<Agreed<T>> {
        let Agreed { target, made: Made } = self;
        target.into_iter().map(|target| Agreed { target, made: Made }).collect()
    }
}

impl<T: fmt::Debug> fmt::Debug for Agreed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Agreed").field(&self.target).finish()
    }
}

/// What an agreeing button sends with its message. Messages are `Clone`, so
/// this is; the agreement inside goes to the first [`take`](Self::take), and
/// a clone of the message taken from later has none. Only the kit makes one:
///
/// ```compile_fail,E0624
/// use aizome_kit::agree::Pressed;
/// let _ = Pressed::<String>::new;
/// ```
pub struct Pressed<T>(Arc<Mutex<Option<Agreed<T>>>>);

impl<T> Pressed<T> {
    fn new(agreed: Agreed<T>) -> Pressed<T> {
        Pressed(Arc::new(Mutex::new(Some(agreed))))
    }

    /// The agreement, unless a clone of this message took it first.
    pub fn take(self) -> Option<Agreed<T>> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
    }
}

impl<T> Clone for Pressed<T> {
    fn clone(&self) -> Self {
        Pressed(self.0.clone())
    }
}

impl<T> fmt::Debug for Pressed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pressed(..)")
    }
}

/// How an agreeing button makes its message when a person presses it.
#[derive(Clone)]
pub(crate) struct AgreeFn(Rc<dyn Fn(Mint) -> AnyMsg>);

impl AgreeFn {
    pub(crate) fn new<T, Msg>(target: T, msg: impl Fn(Pressed<T>) -> Msg + 'static) -> AgreeFn
    where
        T: Clone + Send + 'static,
        Msg: Send + 'static,
    {
        AgreeFn(Rc::new(move |mint| Box::new(msg(Pressed::new(Agreed::made(target.clone(), mint))))))
    }

    pub(crate) fn make(&self, mint: Mint) -> AnyMsg {
        (self.0)(mint)
    }
}

impl fmt::Debug for AgreeFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AgreeFn")
    }
}

/// Agreements for tests: only in the kit's own tests, and in builds with
/// the kit's `testing` feature, which only tests' dev-dependencies turn on
/// (the probes on [`Agreed`] show shipped builds lack it). It adds these and
/// nothing else: nothing that exists without it behaves differently with
/// it, so a program built with it (as a test build unifies features) runs
/// the same code as without.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::{Agreed, Mint, Pressed};

    /// An agreement to `target`, as if a person had pressed its button.
    pub fn agreed<T>(target: T) -> Agreed<T> {
        Agreed::made(target, Mint(()))
    }

    /// A button's message carrier holding an agreement to `target`, as if a
    /// person had pressed it.
    pub fn pressed<T>(target: T) -> Pressed<T> {
        Pressed::new(agreed(target))
    }
}

#[cfg(test)]
mod tests {
    use super::{Agreed, Pressed};
    use crate::column;
    use crate::prelude::*;
    use crate::testing::Harness;
    use sidestep_ui::{Key, Modifiers, NamedKey, PointerButton};

    /// A window with a field, an Erase button agreeing to the disk it names,
    /// and an alert asking the same.
    #[derive(Default)]
    struct Eraser {
        main: Option<WindowKey>,
        disk: String,
        name: String,
        /// The agreements the program got, by target.
        erased: Vec<String>,
        /// Erase messages that came without one.
        empty: usize,
    }

    #[derive(Clone, Debug)]
    enum Msg {
        Name(String),
        Erase(Pressed<String>),
        Ask,
    }

    impl Program for Eraser {
        type Msg = Msg;
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.Test", "Test", "Test")
        }
        fn launched(&mut self, cx: &mut Cx<Msg>) {
            self.main = Some(cx.open_window(WindowSpec::new("Test").size(480.0, 240.0)));
        }
        fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
            match msg {
                Msg::Name(n) => self.name = n,
                Msg::Erase(pressed) => match pressed.take() {
                    Some(agreed) => self.erased.push(agreed.into_target()),
                    None => self.empty += 1,
                },
                Msg::Ask => cx.alert(
                    self.main.unwrap(),
                    Alert::new(format!("Erase {}?", self.disk), "Everything on it will be lost.")
                        .agree_button("Erase Disk", self.disk.clone(), Msg::Erase)
                        .cancel_button("Cancel"),
                ),
            }
        }
        fn view(&self, _: WindowKey) -> Element<Msg> {
            column![
                Element::new(Grip, Vec::new()).id("grip"),
                text_field(&self.name).on_change(Msg::Name).on_submit(Msg::Ask).id("name"),
                button("Ask").on_press(Msg::Ask),
                button(format!("Erase {}", self.disk))
                    .destructive()
                    .on_agree(self.disk.clone(), Msg::Erase)
                    .id("erase"),
            ]
            .spacing(8.0)
            .padding(16.0)
            .into()
        }
    }

    /// An edge that takes only drags, as aizome-face's `drawn(…).drags_only()`
    /// does: a press that ends without a drag reaches what's under it.
    struct Grip;

    impl crate::widget::Widget for Grip {
        fn name(&self) -> &'static str {
            "grip"
        }
        fn layout(&mut self, _: &mut crate::widget::LayoutCx, bc: crate::layout::Constraints) -> crate::layout::Size {
            bc.constrain(crate::layout::Size::new(120.0, 24.0))
        }
        fn hit(&self) -> bool {
            true
        }
        fn drags_only(&self) -> bool {
            true
        }
        fn event(&mut self, cx: &mut crate::widget::EventCx, event: &crate::widget::Event) -> crate::widget::Status {
            match event {
                crate::widget::Event::PointerDown { button: PointerButton::Left, .. } => {
                    cx.capture_pointer();
                    crate::widget::Status::Handled
                }
                _ => crate::widget::Status::Ignored,
            }
        }
    }

    fn harness(agreeing: bool) -> (Harness<Eraser>, WindowKey) {
        let mut h = Harness::new(Eraser { disk: "sdb".into(), ..Eraser::default() });
        if agreeing {
            h.allow_agreements();
        }
        let w = h.window();
        (h, w)
    }

    #[test]
    fn a_click_agrees_to_what_the_button_showed() {
        let (mut h, w) = harness(true);
        assert!(h.click_text(w, "Erase sdb"));
        assert_eq!(h.program().erased, ["sdb"]);
        // The view moved on to another disk: the next press agrees to that.
        h.program_mut().disk = "sdc".into();
        h.send(Msg::Name(String::new()));
        assert!(h.click_text(w, "Erase sdc"));
        assert_eq!(h.program().erased, ["sdb", "sdc"]);
    }

    #[test]
    fn a_harness_press_makes_no_agreement_unless_the_test_asks() {
        let (mut h, w) = harness(false);
        assert!(h.click_text(w, "Erase sdb"));
        assert!(h.program().erased.is_empty(), "a programmatic press agrees to nothing");
        assert_eq!(h.program().empty, 0, "and sends nothing");
    }

    #[test]
    fn the_keyboard_agrees_only_through_the_focused_button() {
        let (mut h, w) = harness(true);
        // Return in the field submits the field, not the destructive button.
        h.send(Msg::Name("x".into()));
        assert!(h.click_text(w, "x"));
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert!(h.program().erased.is_empty());
        h.named(w, NamedKey::Escape, Modifiers::NONE);
        // Tab to Erase (past Ask), then Space.
        h.named(w, NamedKey::Tab, Modifiers::NONE);
        h.named(w, NamedKey::Tab, Modifiers::NONE);
        assert_eq!(h.focused_name(w).as_deref(), Some("erase"));
        h.key(w, Key::Character(" ".into()), Modifiers::NONE);
        assert_eq!(h.program().erased, ["sdb"]);
        // Return while it has the focus presses it too.
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert_eq!(h.program().erased, ["sdb", "sdb"]);
    }

    #[test]
    fn an_alert_agrees_to_its_target_and_closes() {
        let (mut h, w) = harness(true);
        h.send(Msg::Ask);
        assert!(h.texts(w).iter().any(|t| t == "Erase sdb?"));
        // Return doesn't press a destructive button.
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        assert!(h.program().erased.is_empty());
        assert!(h.click_text(w, "Erase Disk"));
        assert_eq!(h.program().erased, ["sdb"]);
        assert!(!h.texts(w).iter().any(|t| t == "Erase sdb?"), "the alert closed");
        // Cancel agrees to nothing.
        h.send(Msg::Ask);
        assert!(h.click_text(w, "Cancel"));
        assert_eq!(h.program().erased, ["sdb"]);
    }

    #[test]
    fn an_alert_in_a_harness_that_doesnt_agree_closes_and_sends_nothing() {
        let (mut h, w) = harness(false);
        h.send(Msg::Ask);
        assert!(h.click_text(w, "Erase Disk"));
        assert!(h.program().erased.is_empty() && h.program().empty == 0);
        assert!(!h.texts(w).iter().any(|t| t == "Erase sdb?"));
    }

    /// The pointer must press the button as well as release over it: a press
    /// that began on nothing and was released over it agrees to nothing.
    #[test]
    fn a_release_over_the_button_after_a_press_elsewhere_agrees_to_nothing() {
        let (mut h, w) = harness(true);
        let on = h.find(w, "Erase sdb").expect("the button").center();
        let size = h.size(w);
        let empty = crate::layout::Point::new(size.width - 4.0, size.height - 4.0);
        h.press(w, empty, PointerButton::Left, 1);
        h.move_to(w, on);
        h.release(w, on, PointerButton::Left);
        assert!(h.program().erased.is_empty(), "the press wasn't the button's");
        assert!(h.click_text(w, "Erase sdb"));
        assert_eq!(h.program().erased, ["sdb"]);
    }

    /// Nor does a press on a drags-only edge, released over the button: the
    /// press reaches what's under the edge, which isn't the button.
    #[test]
    fn a_release_over_the_button_after_a_press_on_a_drag_edge_agrees_to_nothing() {
        let (mut h, w) = harness(true);
        let grip = h.find_id(w, "grip").expect("the edge").center();
        let on = h.find(w, "Erase sdb").expect("the button").center();
        h.press(w, grip, PointerButton::Left, 1);
        h.move_to(w, on);
        h.release(w, on, PointerButton::Left);
        assert!(h.program().erased.is_empty(), "the press was the edge's");
        assert!(h.click_text(w, "Erase sdb"));
        assert_eq!(h.program().erased, ["sdb"]);
    }

    #[test]
    fn a_cloned_message_carries_the_agreement_once() {
        let pressed = super::testing::pressed("sdb".to_owned());
        let copy = pressed.clone();
        assert_eq!(pressed.take().map(Agreed::into_target).as_deref(), Some("sdb"));
        assert!(copy.take().is_none());
    }

    #[test]
    fn a_list_agreed_together_splits_into_its_items() {
        let parts = super::testing::agreed(vec!["format sdb1", "delete sdb2"]).split();
        let targets: Vec<_> = parts.iter().map(|a| *a.target()).collect();
        assert_eq!(targets, ["format sdb1", "delete sdb2"]);
    }
}
