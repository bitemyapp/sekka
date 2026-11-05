//! Confirmed targets: a destructive step acts on exactly the target the
//! person reviewed, re-identified at the moment it acts.

use core::fmt;

/// Whether two descriptions name the same physical target. Compare what
/// stays with the target (a serial, a WWN, a partition table's id, an
/// inode), never only what another target can take over (a device path such
/// as `/dev/sdb`, a drive's name, a row's index).
pub trait SameTarget {
    fn same_target(&self, fresh: &Self) -> bool;
}

/// A target as the person saw it when they agreed to act on it. Make one
/// only where the person reviews the target (the confirmation sheet), and
/// wrap it in a type of the machine's own when other code must not be able
/// to make one.
pub struct Reviewed<T> {
    target: T,
}

impl<T> Reviewed<T> {
    #[inline]
    pub fn new(target: T) -> Self {
        Reviewed { target }
    }

    /// What the person saw.
    #[inline]
    pub fn target(&self) -> &T {
        &self.target
    }
}

impl<T: SameTarget> Reviewed<T> {
    /// Checks a fresh reading of the target against what the person saw. The
    /// proof carries the fresh reading, so the step that takes it acts on the
    /// target as it is now (its current path), and only if it's the same one.
    ///
    /// ```
    /// use aizome_typestate::{Reviewed, SameTarget};
    /// #[derive(Debug, PartialEq)]
    /// struct Drive { path: &'static str, serial: &'static str }
    /// impl SameTarget for Drive {
    ///     fn same_target(&self, fresh: &Self) -> bool { self.serial == fresh.serial }
    /// }
    /// let seen = Reviewed::new(Drive { path: "/dev/sdb", serial: "A" });
    /// // The stick was swapped for another that took the same path.
    /// let changed = seen.confirm(Drive { path: "/dev/sdb", serial: "B" }).unwrap_err();
    /// assert_eq!(changed.found.serial, "B");
    /// ```
    #[inline]
    pub fn confirm(self, fresh: T) -> Result<Confirmed<T>, Changed<T>> {
        if self.target.same_target(&fresh) {
            Ok(Confirmed { target: fresh })
        } else {
            Err(Changed { reviewed: self.target, found: fresh })
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Reviewed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Reviewed").field(&self.target).finish()
    }
}

/// Proof that the target is still the one the person reviewed. Only
/// [`Reviewed::confirm`] makes one, and the destructive step consumes it and
/// acts on [`target`](Self::target), so it can't act on a target the caller
/// picked separately:
///
/// ```compile_fail,E0451
/// use aizome_typestate::Confirmed;
/// let _ = Confirmed { target: "/dev/sdb" };
/// ```
#[must_use = "a confirmation is spent by the step it allows"]
pub struct Confirmed<T> {
    target: T,
}

impl<T> Confirmed<T> {
    /// The target as it is now.
    #[inline]
    pub fn target(&self) -> &T {
        &self.target
    }
}

impl<T: fmt::Debug> fmt::Debug for Confirmed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Confirmed").field(&self.target).finish()
    }
}

/// The target changed since the person reviewed it.
#[derive(Debug)]
pub struct Changed<T> {
    pub reviewed: T,
    pub found: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct D(&'static str, u32);
    impl SameTarget for D {
        fn same_target(&self, fresh: &Self) -> bool {
            self.1 == fresh.1
        }
    }

    #[test]
    fn the_proof_carries_the_fresh_reading() {
        let c = Reviewed::new(D("/dev/sdb", 7)).confirm(D("/dev/sdc", 7)).unwrap();
        assert_eq!(c.target(), &D("/dev/sdc", 7));
    }
}
