//! The "Erase …?" dialog, and the person's press of its OK.
//!
//! Kakehashi's window is on GPUI, not Aizome's kit, so it has a press of its
//! own: [`Pressed`], which only [`erase`]'s OK handler makes, as the person
//! clicks OK (or presses Return in the dialog). It holds the dialog's
//! review and the drive and image the dialog described, and the write takes
//! it ([`Plan::new`](kakehashi_core::write::Plan::new), then
//! [`elevate`](crate::elevate)), so a drive list of the code's own, run
//! through the same steps, starts no write. Code can't make one:
//!
//! ```compile_fail,E0451
//! use kakehashi_core::choice::{ReviewTicket, Target};
//! use kakehashi_window::dialog::Pressed;
//! fn forge(review: ReviewTicket, shown: Target) -> Pressed {
//!     Pressed { review, shown }
//! }
//! ```
//!
//! nor keep one for a second write:
//!
//! ```compile_fail,E0599
//! use kakehashi_window::dialog::Pressed;
//! fn keep(pressed: Pressed) -> (Pressed, Pressed) {
//!     (pressed.clone(), pressed)
//! }
//! ```
//!
//! ```
//! use kakehashi_window::dialog::Pressed;
//! fn review(pressed: &Pressed) -> kakehashi_core::choice::ReviewTicket {
//!     pressed.review()
//! }
//! ```
//!
//! What stays a run-time check: that the press is of the dialog still open
//! (its review ticket, which [`DriveList::confirm`] checks), and that the
//! drive found again at OK is the one described ([`Plan::new`] compares
//! them). Whatever the system takes for a click (an accessibility tool,
//! synthetic input) presses OK as a person does.
//!
//! [`DriveList::confirm`]: kakehashi_core::choice::DriveList::confirm
//! [`Plan::new`]: kakehashi_core::write::Plan::new

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{Context, SharedString, Window};
use kakehashi_core::choice::{ReviewTicket, Target};
use std::rc::Rc;

/// The person pressed OK in the "Erase …?" dialog that review `review`
/// opened, which described `shown`.
#[derive(Debug)]
pub struct Pressed {
    review: ReviewTicket,
    shown: Target,
}

impl Pressed {
    /// The review of the dialog pressed.
    pub fn review(&self) -> ReviewTicket {
        self.review
    }

    /// A press for this crate's tests and its window's (the `test-util`
    /// feature, which only the package's own dev-dependency turns on).
    #[cfg(any(test, feature = "test-util"))]
    pub fn for_tests(review: ReviewTicket, shown: Target) -> Pressed {
        Pressed { review, shown }
    }
}

/// What the dialog described: the drive and image a press agrees to.
impl AsRef<Target> for Pressed {
    fn as_ref(&self) -> &Target {
        &self.shown
    }
}

/// Opens the "Erase …?" dialog for `review`, describing `shown` with `title`
/// and `description`. OK calls `ok` with the person's [`Pressed`]; Cancel
/// calls `cancel`.
#[allow(clippy::too_many_arguments)]
pub fn erase<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    review: ReviewTicket,
    shown: Target,
    ok: impl Fn(&mut V, Pressed, &mut Context<V>) + 'static,
    cancel: impl Fn(&mut V, ReviewTicket, &mut Context<V>) + 'static,
) {
    let view = cx.entity();
    let (title, description) = (title.into(), description.into());
    let (ok, cancel) = (Rc::new(ok), Rc::new(cancel));
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (on_ok, on_cancel) = (view.clone(), view.clone());
        let (ok, cancel, shown) = (ok.clone(), cancel.clone(), shown.clone());
        alert
            .title(title.clone())
            .description(description.clone())
            .ok_text("Erase and write")
            .ok_variant(ButtonVariant::Danger)
            .cancel_text("Cancel")
            .show_cancel(true)
            .on_ok(move |_, _, cx| {
                // The person's press: the only place a `Pressed` is made.
                let pressed = Pressed { review, shown: shown.clone() };
                on_ok.update(cx, |this, cx| ok(this, pressed, cx));
                true
            })
            .on_cancel(move |_, _, cx| {
                on_cancel.update(cx, |this, cx| cancel(this, review, cx));
                true
            })
    });
}
