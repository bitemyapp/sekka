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
//! [`Ticket`]; a transition that applies one takes the [`Current`] proof
//! that it's the latest review's.
//!
//! [`session::Ready`]: calamares_nixos::session::Ready
use crate::app::Review;
use aizome_typestate::{Current, Generation, Ticket};
use calamares_nixos::{
    install::Summary,
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
    /// The settings passed and the helper started: the Review page, while
    /// it prepares.
    pub fn reviewed(
        self,
        _: Current<ReviewRequest>,
        review: Box<Review>,
        session: Session,
    ) -> Preparing<'s> {
        let pending = Pending {
            link: Link::Running(session),
            review,
        };
        Preparing::enter(self.ctx, pending)
    }

    /// They didn't pass, or the helper didn't start: back to the pages.
    pub fn refused(self, _: Current<ReviewRequest>) -> Editing<'s> {
        Editing::enter(self.ctx, ())
    }
}

impl<'s> Preparing<'s> {
    pub fn review(&self) -> &Review {
        &self.data.review
    }

    /// The helper has prepared: its proof lets the confirmation be sent.
    pub fn prepared(self, _: Current<ReviewRequest>, ready: Prepared) -> Ready<'s> {
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
        Ready::enter(self.ctx, Box::new(primed))
    }

    /// A preview's sample preparation finished. A running helper's comes
    /// only with its proof.
    pub(crate) fn sampled(
        self,
        _: Current<ReviewRequest>,
        summary: Summary,
    ) -> Result<Ready<'s>, Preparing<'s>> {
        match self.data.link {
            Link::Sample => Ok(Ready::enter(
                self.ctx,
                Box::new(Primed {
                    sender: Sender::Sample,
                    review: self.data.review,
                    summary,
                }),
            )),
            link => Err(Preparing::enter(
                self.ctx,
                Pending {
                    link,
                    review: self.data.review,
                },
            )),
        }
    }

    /// Preparation failed or was cancelled, or the helper stopped: nothing
    /// was written.
    pub fn stopped(self, _: Current<ReviewRequest>) -> Unprepared<'s> {
        Unprepared::enter(self.ctx, self.data.review)
    }

    /// Confirmed before preparation finished: the confirmation waits for
    /// it, and nothing is sent yet. Refused unless `typed` is the review's
    /// phrase. Confirming early isn't installing:
    ///
    /// ```compile_fail,E0308
    /// use genkan::flow::{Installing, Preparing};
    /// fn confirm_early<'s>(preparing: Preparing<'s>, typed: &str) -> Installing<'s> {
    ///     preparing.confirm(typed).ok().unwrap()
    /// }
    /// ```
    ///
    /// ```
    /// use genkan::flow::{Preparing, Queued};
    /// fn confirm_early<'s>(preparing: Preparing<'s>, typed: &str) -> Queued<'s> {
    ///     preparing.confirm(typed).ok().unwrap()
    /// }
    /// ```
    pub fn confirm(self, typed: &str) -> Result<Queued<'s>, Preparing<'s>> {
        if typed == self.data.review.phrase() {
            Ok(Queued::enter(self.ctx, self.data))
        } else {
            Err(self)
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

    /// Sends the confirmation, if `typed` is the review's phrase: from now
    /// on the helper erases the disk. Refused, staying ready, when it isn't
    /// or the helper is gone.
    pub fn confirm(self, typed: &str) -> Result<Installing<'s>, Ready<'s>> {
        let phrase = self.data.review.phrase();
        if typed != phrase || self.data.sender.send(&phrase).is_err() {
            return Err(self);
        }
        let running = Running {
            _link: self.data.sender.into_link(),
            step: None,
        };
        Ok(Installing::enter(self.ctx, running))
    }

    /// The helper stopped while waiting for the confirmation.
    pub fn stopped(self, _: Current<ReviewRequest>) -> Unprepared<'s> {
        Unprepared::enter(self.ctx, self.data.review)
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
    /// Preparation finished: the queued confirmation is sent now. If the
    /// helper is gone instead, back to the review.
    pub fn prepared(
        self,
        _: Current<ReviewRequest>,
        ready: Prepared,
    ) -> Result<Installing<'s>, Unprepared<'s>> {
        let Pending { link, review } = self.data;
        let link = match link {
            Link::Running(session) => match session.confirm(&ready, &review.phrase()) {
                Ok(()) => Link::Running(session),
                Err(_) => return Err(Unprepared::enter(self.ctx, review)),
            },
            Link::Sample => Link::Sample,
        };
        Ok(Installing::enter(
            self.ctx,
            Running {
                _link: link,
                step: None,
            },
        ))
    }

    /// A preview's sample preparation finished. A running helper's comes
    /// only with its proof.
    pub(crate) fn sampled(self, _: Current<ReviewRequest>) -> Result<Installing<'s>, Queued<'s>> {
        match self.data.link {
            Link::Sample => Ok(Installing::enter(
                self.ctx,
                Running {
                    _link: Link::Sample,
                    step: None,
                },
            )),
            link => Err(Queued::enter(
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
    /// use aizome_typestate::Current;
    /// use genkan::flow::{Failed, Queued, ReviewRequest};
    /// fn failed<'s>(queued: Queued<'s>, current: Current<ReviewRequest>) -> Failed<'s> {
    ///     queued.stopped(current)
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_typestate::Current;
    /// use genkan::flow::{Queued, ReviewRequest, Unprepared};
    /// fn failed<'s>(queued: Queued<'s>, current: Current<ReviewRequest>) -> Unprepared<'s> {
    ///     queued.stopped(current)
    /// }
    /// ```
    pub fn stopped(self, _: Current<ReviewRequest>) -> Unprepared<'s> {
        Unprepared::enter(self.ctx, self.data.review)
    }
}

impl<'s> Installing<'s> {
    /// The last step the helper reported: none before the first disk write.
    pub fn step(&self) -> Option<u8> {
        self.data.step
    }

    /// The helper started step `step`.
    pub fn progress(mut self, _: Current<ReviewRequest>, step: u8) -> Installing<'s> {
        self.data.step = Some(step);
        self
    }

    pub fn complete(self, _: Current<ReviewRequest>) -> Installed<'s> {
        Installed::enter(self.ctx, ())
    }

    /// The installation failed, at the step the helper last reported. The
    /// helper may still be cleaning up.
    pub fn failed(self, _: Current<ReviewRequest>) -> Failed<'s> {
        let Running { _link: link, step } = self.data;
        Failed::enter(
            self.ctx,
            Failure {
                step,
                link: Some(link),
            },
        )
    }

    /// The helper exited without completing: a failure at the step it last
    /// reported.
    pub fn exited(self, _: Current<ReviewRequest>) -> Failed<'s> {
        let step = self.data.step;
        Failed::enter(self.ctx, Failure { step, link: None })
    }
}

impl<'s> Failed<'s> {
    /// The helper exited.
    pub fn exited(self, _: Current<ReviewRequest>) -> Failed<'s> {
        let step = self.data.step;
        Failed::enter(self.ctx, Failure { step, link: None })
    }
}
