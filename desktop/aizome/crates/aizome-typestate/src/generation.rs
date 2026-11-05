//! Request generations: answers to requests that a newer request superseded
//! are dropped, and the code that applies an answer can't skip the check.

use core::fmt;
use core::marker::PhantomData;

/// The latest request of one kind. `K` is a marker type naming the kind, so
/// a ticket for one kind of request can't be checked against another's
/// generation.
///
/// ```
/// use aizome_typestate::Generation;
/// enum Scan {}
/// let mut scans = Generation::<Scan>::new();
/// let first = scans.issue();
/// let second = scans.issue();
/// assert!(scans.check(first).is_none());
/// assert_eq!(scans.check(second).map(|c| c.ticket()), Some(second));
/// ```
///
/// A ticket of another kind doesn't type-check:
///
/// ```compile_fail,E0308
/// use aizome_typestate::Generation;
/// enum Scan {}
/// enum Save {}
/// let mut scans = Generation::<Scan>::new();
/// let mut saves = Generation::<Save>::new();
/// let save = saves.issue();
/// let _ = scans.check(save);
/// ```
pub struct Generation<K> {
    latest: u64,
    kind: PhantomData<fn() -> K>,
}

impl<K> Generation<K> {
    /// No request yet: every ticket checks stale until the first
    /// [`issue`](Self::issue).
    #[inline]
    pub const fn new() -> Self {
        Generation { latest: 0, kind: PhantomData }
    }

    /// Starts a request. Every ticket issued before it is now stale.
    #[inline]
    pub fn issue(&mut self) -> Ticket<K> {
        self.latest += 1;
        Ticket { n: self.latest, kind: PhantomData }
    }

    /// Makes every outstanding ticket stale without starting a request (the
    /// request was abandoned).
    #[inline]
    pub fn retire(&mut self) {
        self.latest += 1;
    }

    /// The proof that `ticket` is the latest request's, or `None` when a
    /// newer request (or a [`retire`](Self::retire)) superseded it.
    #[inline]
    pub fn check(&self, ticket: Ticket<K>) -> Option<Current<K>> {
        (ticket.n == self.latest && ticket.n != 0).then_some(Current { ticket })
    }

    /// Whether `ticket` is the latest request's, for code that only reads.
    #[inline]
    pub fn is_current(&self, ticket: Ticket<K>) -> bool {
        self.check(ticket).is_some()
    }

    /// The answer, proven fresh, or the answer back when a newer request
    /// superseded it. Prefer this to [`check`](Self::check): the proof and
    /// the answer travel together, so a transition can't be handed the proof
    /// of one answer with another answer.
    ///
    /// ```
    /// use aizome_typestate::{Answer, Generation, Ticket};
    /// enum Scan {}
    /// struct Scanned { ticket: Ticket<Scan>, disks: u32 }
    /// impl Answer<Scan> for Scanned {
    ///     fn ticket(&self) -> Ticket<Scan> { self.ticket }
    /// }
    /// let mut scans = Generation::new();
    /// let old = scans.issue();
    /// let new = scans.issue();
    /// assert!(scans.check_answer(Scanned { ticket: old, disks: 1 }).is_err());
    /// let fresh = scans.check_answer(Scanned { ticket: new, disks: 2 }).ok().unwrap();
    /// assert_eq!(fresh.answer().disks, 2);
    /// ```
    #[inline]
    pub fn check_answer<A: Answer<K>>(&self, answer: A) -> Result<Fresh<K, A>, A> {
        match self.check(answer.ticket()) {
            Some(current) => Ok(Fresh { current, answer }),
            None => Err(answer),
        }
    }
}

/// An answer that names the request it answers.
pub trait Answer<K> {
    fn ticket(&self) -> Ticket<K>;
}

/// An answer proven to belong to the latest request: only
/// [`Generation::check_answer`] makes one.
///
/// ```compile_fail,E0451
/// use aizome_typestate::{Current, Fresh};
/// enum Scan {}
/// fn forge(current: Current<Scan>, answer: u32) -> Fresh<Scan, u32> {
///     Fresh { current, answer }
/// }
/// ```
#[must_use = "a Fresh answer is for the transition that applies it"]
pub struct Fresh<K, A> {
    current: Current<K>,
    answer: A,
}

impl<K, A> Fresh<K, A> {
    #[inline]
    pub fn answer(&self) -> &A {
        &self.answer
    }

    /// The answer, spending the proof.
    #[inline]
    pub fn into_answer(self) -> A {
        self.answer
    }

    /// The proof that came with it.
    #[inline]
    pub fn current(&self) -> &Current<K> {
        &self.current
    }
}

impl<K, A: fmt::Debug> fmt::Debug for Fresh<K, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fresh").field("ticket", &self.current.ticket.n).field("answer", &self.answer).finish()
    }
}

impl<K> Default for Generation<K> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<K> fmt::Debug for Generation<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Generation").field(&self.latest).finish()
    }
}

/// Names one request. Send it with the request and back with the answer.
pub struct Ticket<K> {
    n: u64,
    kind: PhantomData<fn() -> K>,
}

impl<K> Clone for Ticket<K> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<K> Copy for Ticket<K> {}
impl<K> PartialEq for Ticket<K> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.n == other.n
    }
}
impl<K> Eq for Ticket<K> {}
impl<K> core::hash::Hash for Ticket<K> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.n.hash(state);
    }
}
impl<K> fmt::Debug for Ticket<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Ticket").field(&self.n).finish()
    }
}

/// Proof that an answer belongs to the latest request: only
/// [`Generation::check`] makes one. A transition that applies an answer takes
/// it by value, so applying an answer without the check doesn't compile:
///
/// ```compile_fail,E0451
/// use aizome_typestate::{Current, Ticket};
/// enum Scan {}
/// fn forge(ticket: Ticket<Scan>) -> Current<Scan> {
///     Current { ticket }
/// }
/// ```
#[must_use = "a Current proves an answer is fresh only for the transition that takes it"]
pub struct Current<K> {
    ticket: Ticket<K>,
}

impl<K> Current<K> {
    /// The ticket that checked current.
    #[inline]
    pub fn ticket(&self) -> Ticket<K> {
        self.ticket
    }
}

impl<K> fmt::Debug for Current<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Current").field(&self.ticket.n).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    enum A {}

    #[test]
    fn a_fresh_generation_has_no_current_ticket() {
        let mut g = Generation::<A>::new();
        let forged_zero = Ticket::<A> { n: 0, kind: PhantomData };
        assert!(g.check(forged_zero).is_none());
        let t = g.issue();
        assert!(g.is_current(t));
    }

    #[test]
    fn retire_makes_the_latest_stale() {
        let mut g = Generation::<A>::new();
        let t = g.issue();
        g.retire();
        assert!(g.check(t).is_none());
        let u = g.issue();
        assert!(g.check(u).is_some());
        assert!(g.check(t).is_none());
    }
}
