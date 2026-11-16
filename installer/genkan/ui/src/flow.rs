// SPDX-License-Identifier: GPL-3.0-or-later
//! The review, the preparation going on behind it, the confirmation and the
//! installation it starts: the GUI's side of the helper session (GEN-34).
//!
//! One state per stage, each holding only its own data: the helper's
//! session, the review's display copy, what preparation found, the last step
//! the installation reported. The program reaches them only through
//! [`Flow::with_typed`], so the transitions below are the only ways from one
//! to another. Two things follow:
//!
//! - The confirmation is sent to the helper only with the helper's proof
//!   that it has prepared ([`session::Ready`]). Confirming earlier queues it
//!   ([`Queued`]); it's sent when preparation finishes.
//! - Only an installation the confirmation started ([`Installing`]) can fail
//!   as an installation ([`Failed`]), and how it failed is what the helper
//!   last reported: no step yet means no disk write. A queued confirmation
//!   whose preparation fails goes back to the review ([`Unprepared`]).
//!
//! The review worker's answer and the helper's events carry the review's
//! [`Ticket`], and reach their transitions bound to the proof that they're
//! the latest review's ([`Fresh`]): a transition reads its payload from
//! that answer, so it can't be handed another answer's proof.
//!
//! [`session::Ready`]: calamares_nixos::session::Ready
// Every stage and answer is matched by name: a new one forces a decision.
#![deny(clippy::wildcard_enum_match_arm)]
use crate::app::{Helper, Review};
use aizome_kit::agree::Agreed;
use aizome_typestate::{Answer, Fresh, Generation, Ticket};
use calamares_nixos::{
    install::{Event, Summary},
    session::{Ready as Prepared, Session},
};

/// Names a review: the worker's answer, and then the helper's events, carry
/// its ticket.
pub enum ReviewRequest {}

/// The helper behind a review, or a preview's sample with none.
pub enum Link {
    Running(Session),
    Sample,
}

/// What sends the confirmation: a prepared helper, with its proof, or a
/// preview's sample.
enum Sender {
    Helper(Session, Prepared),
    Sample,
}

impl Sender {
    fn send(&self, phrase: &str) -> anyhow::Result<()> {
        match self {
            Self::Helper(session, ready) => session.confirm(ready, phrase),
            Self::Sample => Ok(()),
        }
    }

    fn into_link(self) -> Link {
        match self {
            Self::Helper(session, _) => Link::Running(session),
            Self::Sample => Link::Sample,
        }
    }
}

/// A review whose helper is preparing.
pub struct Pending {
    link: Link,
    review: Box<Review>,
}

impl Pending {
    pub fn review(&self) -> &Review {
        &self.review
    }
}

/// A review whose helper has prepared.
pub struct Primed {
    sender: Sender,
    review: Box<Review>,
    summary: Summary,
}

impl Primed {
    pub fn review(&self) -> &Review {
        &self.review
    }
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
}

/// An installation under way: the helper, and the last step it reported.
pub struct Running {
    _link: Link,
    step: Option<u8>,
}

impl Running {
    /// The last step the helper reported: none before the first disk write.
    pub fn step(&self) -> Option<u8> {
        self.step
    }
}

/// A failed installation: the step it failed at, and the helper while it
/// still runs (cleaning up).
pub struct Failure {
    step: Option<u8>,
    link: Option<Link>,
}

impl Failure {
    /// The step it failed at: none when it failed before the first disk
    /// write.
    pub fn step(&self) -> Option<u8> {
        self.step
    }
    pub fn helper_running(&self) -> bool {
        self.link.is_some()
    }
}

aizome_typestate::machine! {
    /// The review and the installation, from the wizard's pages to the
    /// installation's outcome.
    pub struct Flow over Generation<ReviewRequest> {
        /// Where the review is, with that stage's data.
        stored FlowState;
        /// The typed stage, holding the reviews' tickets.
        typed FlowAny;
        states {
            /// The wizard's pages: nothing is under review.
            Editing,
            /// The review worker checks the settings and starts the helper.
            Checking,
            /// The Review page, while the helper prepares.
            Preparing(Pending),
            /// The Review page; the helper has prepared and waits for the
            /// confirmation.
            Ready(Box<Primed>),
            /// The Review page after preparation failed or was cancelled, or
            /// the helper stopped: nothing was written.
            Unprepared(Box<Review>),
            /// The person confirmed before preparation finished. The Install
            /// page waits; the confirmation is sent when preparation does.
            Queued(Pending),
            /// The confirmation was sent: the helper erases the disk and
            /// installs.
            Installing(Running),
            /// Installed.
            Installed,
            /// The installation failed.
            Failed(Failure),
        }
        branches {
            Dequeued = Queued | Installing | Unprepared;
        }
        new(pub(crate));
        with_typed(pub(crate));
    }
}

/// Back from the review: dropping its helper withdraws the request, and its
/// later events are stale.
fn withdraw(ctx: &mut Generation<ReviewRequest>) -> Editing<'_> {
    ctx.retire();
    Editing::enter(ctx, ())
}

impl<'s> Editing<'s> {
    /// Starts reviewing: the worker's answer and the helper's events carry
    /// the ticket.
    pub fn review(self) -> (Checking<'s>, Ticket<ReviewRequest>) {
        let ticket = self.ctx.issue();
        (Checking::enter(self.ctx, ()), ticket)
    }

    /// A preview's sample review, with no helper behind it.
    pub(crate) fn sample(self, review: Box<Review>) -> (Preparing<'s>, Ticket<ReviewRequest>) {
        let ticket = self.ctx.issue();
        let pending = Pending {
            link: Link::Sample,
            review,
        };
        (Preparing::enter(self.ctx, pending), ticket)
    }
}

impl<'s> Checking<'s> {
    /// The review worker answered: the Review page while the helper
    /// prepares, or back to the pages with why not. The answer comes with
    /// the proof that it's this review's, bound to it.
    pub fn answered(
        self,
        answer: Fresh<ReviewRequest, ReviewAnswer>,
    ) -> Result<Preparing<'s>, (Editing<'s>, String)> {
        match answer.into_answer().result {
            Ok((review, session)) => {
                let pending = Pending {
                    link: Link::Running(session),
                    review,
                };
                Ok(Preparing::enter(self.ctx, pending))
            }
            Err(error) => Err((Editing::enter(self.ctx, ()), error)),
        }
    }

    /// The helper stopped (or got ahead of the review) before the review
    /// opened: back to the pages, and the worker's answer is stale. Any
    /// other answer leaves it checking.
    pub fn stopped(self, said: &Said) -> Result<Editing<'s>, Checking<'s>> {
        if !(stops(said) || matches!(said.answer().said, Helper::Prepared(_))) {
            return Err(self);
        }
        Ok(withdraw(self.ctx))
    }
}

/// What the Review page's Erase Disk and Install button agrees to: the
/// review the page shows (its ticket) and the phrase typed for it. The
/// person's press of that button makes the [`Agreed`] the confirmation takes
/// ([`Ready::confirm`], [`Preparing::confirm`]); the page builds one for each
/// view, so it names the review as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Erasing {
    review: Ticket<ReviewRequest>,
    typed: String,
}

impl Erasing {
    pub fn new(review: Ticket<ReviewRequest>, typed: impl Into<String>) -> Self {
        Self {
            review,
            typed: typed.into(),
        }
    }

    /// Whether it's an agreement to `review`, the latest one, with its
    /// phrase typed: one kept from an earlier review, or typed for another
    /// disk, isn't.
    fn to(&self, reviews: &Generation<ReviewRequest>, review: &Review) -> bool {
        reviews.is_current(self.review) && self.typed == review.phrase()
    }
}

/// The review worker's answer: the plan's display copy and its running
/// helper, or why there's no review. It names the review it answers.
pub struct ReviewAnswer {
    ticket: Ticket<ReviewRequest>,
    result: Result<(Box<Review>, Session), String>,
}

impl ReviewAnswer {
    pub fn new(
        ticket: Ticket<ReviewRequest>,
        result: Result<(Box<Review>, Session), String>,
    ) -> Self {
        Self { ticket, result }
    }
}

impl Answer<ReviewRequest> for ReviewAnswer {
    fn ticket(&self) -> Ticket<ReviewRequest> {
        self.ticket
    }
}

/// What the helper said about a review: it names the review it answers.
#[derive(Clone, Debug)]
pub struct HelperAnswer {
    ticket: Ticket<ReviewRequest>,
    said: Helper,
}

impl HelperAnswer {
    pub fn new(ticket: Ticket<ReviewRequest>, said: Helper) -> Self {
        Self { ticket, said }
    }
}

impl Answer<ReviewRequest> for HelperAnswer {
    fn ticket(&self) -> Ticket<ReviewRequest> {
        self.ticket
    }
}

/// What the helper said, proven to be about the latest review: the
/// transitions it drives take it whole and read their payload from it, so
/// a payload can't be applied with another answer's proof.
pub type Said = Fresh<ReviewRequest, HelperAnswer>;

/// What the helper said, read only from a proven answer: a stale answer's
/// payload can't be re-wrapped under the latest ticket.
pub fn heard(said: &Said) -> &Helper {
    &said.answer().said
}

/// Whether the helper said it stopped: cancelled, failed or exited.
fn stops(said: &Said) -> bool {
    matches!(
        said.answer().said,
        Helper::Event(Event::Cancelled | Event::Failed { .. }) | Helper::Finished(_)
    )
}

impl<'s> Preparing<'s> {
    pub fn review(&self) -> &Review {
        &self.data.review
    }

    /// The helper has prepared: its proof, which the answer carries, lets the
    /// confirmation be sent. Any other answer leaves it preparing.
    pub fn prepared(self, said: &Said) -> Result<Ready<'s>, Preparing<'s>> {
        let Helper::Prepared(ready) = &said.answer().said else {
            return Err(self);
        };
        let ready = ready.clone();
        let summary = ready.summary().clone();
        let Pending { link, review } = self.data;
        let sender = match link {
            Link::Running(session) => Sender::Helper(session, ready),
            Link::Sample => Sender::Sample,
        };
        let primed = Primed {
            sender,
            review,
            summary,
        };
        Ok(Ready::enter(self.ctx, Box::new(primed)))
    }

    /// A preview's sample preparation finished. A running helper's comes
    /// only with its proof.
    pub(crate) fn sampled(self, said: &Said) -> Result<Ready<'s>, Preparing<'s>> {
        let Helper::Event(Event::Prepared { summary }) = &said.answer().said else {
            return Err(self);
        };
        let summary = summary.clone();
        match self.data.link {
            Link::Sample => Ok(Ready::enter(
                self.ctx,
                Box::new(Primed {
                    sender: Sender::Sample,
                    review: self.data.review,
                    summary,
                }),
            )),
            link @ Link::Running(_) => Err(Preparing::enter(
                self.ctx,
                Pending {
                    link,
                    review: self.data.review,
                },
            )),
        }
    }

    /// Preparation failed or was cancelled, or the helper stopped: nothing
    /// was written. Any other answer leaves it preparing.
    pub fn stopped(self, said: &Said) -> Result<Unprepared<'s>, Preparing<'s>> {
        if !stops(said) {
            return Err(self);
        }
        Ok(Unprepared::enter(self.ctx, self.data.review))
    }

    /// Confirmed before preparation finished: the confirmation waits for
    /// it, and nothing is sent yet. It takes the person's agreement, which
    /// only their press of the Review page's Erase Disk and Install button
    /// makes, and is refused unless that agreement is to this review, with
    /// its phrase typed. Confirming early isn't installing:
    ///
    /// ```compile_fail,E0308
    /// use aizome_kit::agree::Agreed;
    /// use genkan::flow::{Erasing, Installing, Preparing};
    /// fn confirm_early<'s>(preparing: Preparing<'s>, agreed: Agreed<Erasing>) -> Installing<'s> {
    ///     preparing.confirm(agreed).ok().unwrap()
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_kit::agree::Agreed;
    /// use genkan::flow::{Erasing, Preparing, Queued};
    /// fn confirm_early<'s>(preparing: Preparing<'s>, agreed: Agreed<Erasing>) -> Queued<'s> {
    ///     preparing.confirm(agreed).ok().unwrap()
    /// }
    /// ```
    ///
    /// The phrase typed is no confirmation by itself, as it once was: code
    /// that has it can't confirm for the person.
    ///
    /// ```compile_fail,E0308
    /// use genkan::flow::{Preparing, Queued};
    /// fn confirm_unasked<'s>(preparing: Preparing<'s>, typed: &str) -> Option<Queued<'s>> {
    ///     preparing.confirm(typed).ok()
    /// }
    /// ```
    ///
    /// A refused agreement is spent: trying again takes another press.
    ///
    /// ```compile_fail,E0382
    /// use aizome_kit::agree::Agreed;
    /// use genkan::flow::{Erasing, Preparing, Queued};
    /// fn confirm_twice<'s>(preparing: Preparing<'s>, agreed: Agreed<Erasing>) -> Option<Queued<'s>> {
    ///     match preparing.confirm(agreed) {
    ///         Ok(queued) => Some(queued),
    ///         Err(preparing) => preparing.confirm(agreed).ok(),
    ///     }
    /// }
    /// ```
    pub fn confirm(self, agreed: Agreed<Erasing>) -> Result<Queued<'s>, Preparing<'s>> {
        if agreed.target().to(self.ctx, &self.data.review) {
            Ok(Queued::enter(self.ctx, self.data))
        } else {
            Err(self)
        }
    }

    /// A preview's sample review confirmed without a press: it has no helper
    /// to send a confirmation to. Refused for a running helper's review.
    pub(crate) fn confirm_sample(self) -> Result<Queued<'s>, Preparing<'s>> {
        match self.data.link {
            Link::Sample => Ok(Queued::enter(self.ctx, self.data)),
            Link::Running(_) => Err(self),
        }
    }

    pub fn withdraw(self) -> Editing<'s> {
        withdraw(self.ctx)
    }
}

impl<'s> Ready<'s> {
    pub fn review(&self) -> &Review {
        &self.data.review
    }
    pub fn summary(&self) -> &Summary {
        &self.data.summary
    }

    /// Sends the confirmation, if the person agreed to this review, with
    /// its phrase typed ([`Erasing`]): from now on the helper erases the
    /// disk. Refused, staying ready, when they didn't or the helper is gone.
    /// The agreement names the review; nothing beside it can:
    ///
    /// ```compile_fail,E0061
    /// use aizome_kit::agree::Agreed;
    /// use genkan::flow::{Erasing, Installing, Ready};
    /// fn confirm_other<'s>(ready: Ready<'s>, agreed: Agreed<Erasing>, phrase: &str) -> Option<Installing<'s>> {
    ///     ready.confirm(agreed, phrase).ok()
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_kit::agree::Agreed;
    /// use genkan::flow::{Erasing, Installing, Ready};
    /// fn confirm<'s>(ready: Ready<'s>, agreed: Agreed<Erasing>) -> Option<Installing<'s>> {
    ///     ready.confirm(agreed).ok()
    /// }
    /// ```
    ///
    /// The preview's way past the press is the crate's alone:
    ///
    /// ```compile_fail,E0624
    /// use genkan::flow::{Installing, Ready};
    /// fn confirm_unasked<'s>(ready: Ready<'s>) -> Option<Installing<'s>> {
    ///     ready.confirm_sample().ok()
    /// }
    /// ```
    pub fn confirm(self, agreed: Agreed<Erasing>) -> Result<Installing<'s>, Ready<'s>> {
        if !agreed.target().to(self.ctx, &self.data.review) {
            return Err(self);
        }
        self.send()
    }

    /// A preview's sample review confirmed without a press: it has no helper
    /// to send a confirmation to. Refused for a running helper's review.
    pub(crate) fn confirm_sample(self) -> Result<Installing<'s>, Ready<'s>> {
        match self.data.sender {
            Sender::Sample => self.send(),
            Sender::Helper(..) => Err(self),
        }
    }

    fn send(self) -> Result<Installing<'s>, Ready<'s>> {
        if self.data.sender.send(&self.data.review.phrase()).is_err() {
            return Err(self);
        }
        let running = Running {
            _link: self.data.sender.into_link(),
            step: None,
        };
        Ok(Installing::enter(self.ctx, running))
    }

    /// The helper stopped while waiting for the confirmation.
    pub fn stopped(self, said: &Said) -> Result<Unprepared<'s>, Ready<'s>> {
        if !stops(said) {
            return Err(self);
        }
        Ok(Unprepared::enter(self.ctx, self.data.review))
    }

    pub fn withdraw(self) -> Editing<'s> {
        withdraw(self.ctx)
    }
}

impl<'s> Unprepared<'s> {
    pub fn review(&self) -> &Review {
        &self.data
    }

    pub fn withdraw(self) -> Editing<'s> {
        withdraw(self.ctx)
    }
}

impl<'s> Queued<'s> {
    /// Preparation finished: the queued confirmation is sent now, with the
    /// helper's proof the answer carries. If the helper is gone instead,
    /// back to the review; any other answer leaves it queued.
    pub fn prepared(self, said: &Said) -> Dequeued<'s> {
        let Helper::Prepared(ready) = &said.answer().said else {
            return Dequeued::Queued(self);
        };
        let Pending { link, review } = self.data;
        let link = match link {
            Link::Running(session) => match session.confirm(ready, &review.phrase()) {
                Ok(()) => Link::Running(session),
                Err(_) => return Dequeued::Unprepared(Unprepared::enter(self.ctx, review)),
            },
            Link::Sample => Link::Sample,
        };
        Dequeued::Installing(Installing::enter(
            self.ctx,
            Running {
                _link: link,
                step: None,
            },
        ))
    }

    /// A preview's sample preparation finished. A running helper's comes
    /// only with its proof.
    pub(crate) fn sampled(self, said: &Said) -> Result<Installing<'s>, Queued<'s>> {
        if !matches!(said.answer().said, Helper::Event(Event::Prepared { .. })) {
            return Err(self);
        }
        match self.data.link {
            Link::Sample => Ok(Installing::enter(
                self.ctx,
                Running {
                    _link: Link::Sample,
                    step: None,
                },
            )),
            link @ Link::Running(_) => Err(Queued::enter(
                self.ctx,
                Pending {
                    link,
                    review: self.data.review,
                },
            )),
        }
    }

    /// Preparation failed or was cancelled, or the helper stopped, before
    /// the confirmation could be sent. Nothing was written: it's the
    /// review's failed preparation, not a failed installation (GEN-34).
    ///
    /// ```compile_fail,E0308
    /// use genkan::flow::{Failed, Queued, Said};
    /// fn failed<'s>(queued: Queued<'s>, said: &Said) -> Result<Failed<'s>, Queued<'s>> {
    ///     queued.stopped(said)
    /// }
    /// ```
    ///
    /// ```
    /// use genkan::flow::{Queued, Said, Unprepared};
    /// fn failed<'s>(queued: Queued<'s>, said: &Said) -> Result<Unprepared<'s>, Queued<'s>> {
    ///     queued.stopped(said)
    /// }
    /// ```
    pub fn stopped(self, said: &Said) -> Result<Unprepared<'s>, Queued<'s>> {
        if !stops(said) {
            return Err(self);
        }
        Ok(Unprepared::enter(self.ctx, self.data.review))
    }
}

impl<'s> Installing<'s> {
    /// The last step the helper reported: none before the first disk write.
    pub fn step(&self) -> Option<u8> {
        self.data.step
    }

    /// The helper started a step, as the answer says.
    pub fn progress(mut self, said: &Said) -> Installing<'s> {
        if let Helper::Event(Event::Progress { step, .. }) = said.answer().said {
            self.data.step = Some(step);
        }
        self
    }

    /// The helper completed the installation; any other answer leaves it
    /// installing.
    pub fn complete(self, said: &Said) -> Result<Installed<'s>, Installing<'s>> {
        if !matches!(said.answer().said, Helper::Event(Event::Complete)) {
            return Err(self);
        }
        Ok(Installed::enter(self.ctx, ()))
    }

    /// The installation failed, at the step the helper last reported. The
    /// helper may still be cleaning up. Only the helper's `Failed` says so.
    pub fn failed(self, said: &Said) -> Result<Failed<'s>, Installing<'s>> {
        if !matches!(said.answer().said, Helper::Event(Event::Failed { .. })) {
            return Err(self);
        }
        let Running { _link: link, step } = self.data;
        Ok(Failed::enter(
            self.ctx,
            Failure {
                step,
                link: Some(link),
            },
        ))
    }

    /// The helper exited without completing: a failure at the step it last
    /// reported. Only the helper's exit says so.
    pub fn exited(self, said: &Said) -> Result<Failed<'s>, Installing<'s>> {
        if !matches!(said.answer().said, Helper::Finished(_)) {
            return Err(self);
        }
        let step = self.data.step;
        Ok(Failed::enter(self.ctx, Failure { step, link: None }))
    }
}

impl<'s> Failed<'s> {
    /// The helper exited; any other answer leaves it as it is.
    pub fn exited(self, said: &Said) -> Result<Failed<'s>, Failed<'s>> {
        if !matches!(said.answer().said, Helper::Finished(_)) {
            return Err(self);
        }
        let step = self.data.step;
        Ok(Failed::enter(self.ctx, Failure { step, link: None }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The helper can exit (pkexec failing at once) before the worker's
    /// answer opens the review: the review is abandoned, and the answer that
    /// follows, with its helper, is stale. Otherwise a dead helper sat behind
    /// the Review page, and a confirmation queued for it waited for ever.
    #[test]
    fn a_helper_that_stops_before_the_review_opens_leaves_no_review() {
        let mut flow = Flow::new(Generation::new());
        let ticket = flow.with_typed(|state| match state {
            FlowAny::Editing(editing) => {
                let (checking, ticket) = editing.review();
                (checking.into(), ticket)
            }
            FlowAny::Checking(_)
            | FlowAny::Preparing(_)
            | FlowAny::Ready(_)
            | FlowAny::Unprepared(_)
            | FlowAny::Queued(_)
            | FlowAny::Installing(_)
            | FlowAny::Installed(_)
            | FlowAny::Failed(_) => unreachable!("a new flow starts on the pages"),
        });
        let exited = HelperAnswer::new(ticket, Helper::Finished(Err("pkexec failed".into())));
        let said = flow.ctx().check_answer(exited).ok().unwrap();
        flow.with_typed(|state| match state {
            FlowAny::Checking(checking) => (checking.stopped(&said).ok().unwrap().into(), ()),
            other @ (FlowAny::Editing(_)
            | FlowAny::Preparing(_)
            | FlowAny::Ready(_)
            | FlowAny::Unprepared(_)
            | FlowAny::Queued(_)
            | FlowAny::Installing(_)
            | FlowAny::Installed(_)
            | FlowAny::Failed(_)) => (other, ()),
        });
        assert!(matches!(flow.stored(), FlowState::Editing(())));
        let late = ReviewAnswer::new(ticket, Err("late".into()));
        assert!(flow.ctx().check_answer(late).is_err());
    }
}
