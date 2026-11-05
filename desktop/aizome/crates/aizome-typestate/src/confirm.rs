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

/// A target as the person saw it when they agreed to act on it, or as a
/// record kept it (a saved configuration's generation): anything a fresh
/// reading must still match before a step acts on it.
///
/// `B` is a brand: a type only the code that shows the person the target can
/// make (a unit struct with a private field). Making a `Reviewed` takes one,
/// so other code can't claim a review that didn't happen, and the
/// destructive step names the brand it accepts.
///
/// ```
/// mod sheet {
///     use aizome_typestate::{Reviewed, SameTarget};
///     #[derive(Debug, Clone, PartialEq)]
///     pub struct Drive { pub path: &'static str, pub serial: &'static str }
///     impl SameTarget for Drive {
///         fn same_target(&self, fresh: &Self) -> bool { self.serial == fresh.serial }
///     }
///     /// Only this module reviews drives.
///     pub struct Review(());
///     pub fn review(drive: Drive) -> Reviewed<Drive, Review> {
///         Reviewed::new(drive, Review(()))
///     }
/// }
/// let seen = sheet::review(sheet::Drive { path: "/dev/sdb", serial: "A" });
/// // The stick was swapped for another that took the same path.
/// let changed = seen.confirm(sheet::Drive { path: "/dev/sdb", serial: "B" }).unwrap_err();
/// assert_eq!(changed.found.serial, "B");
/// ```
///
/// Other code can't make the brand, so it can't make a review:
///
/// ```compile_fail,E0603
/// mod sheet {
///     pub struct Review(());
/// }
/// let _ = aizome_typestate::Reviewed::new("/dev/sdb", sheet::Review(()));
/// ```
pub struct Reviewed<T, B> {
    target: T,
    brand: B,
}

impl<T, B> Reviewed<T, B> {
    /// The review of `target`, made where the person saw it.
    #[inline]
    pub fn new(target: T, brand: B) -> Self {
        Reviewed { target, brand }
    }

    /// What the person saw.
    #[inline]
    pub fn target(&self) -> &T {
        &self.target
    }

    /// What the person saw, giving up the review.
    #[inline]
    pub fn into_target(self) -> T {
        self.target
    }
}

impl<T: SameTarget, B> Reviewed<T, B> {
    /// Checks a fresh reading of the target against what the person saw. The
    /// proof carries the fresh reading, so the step that takes it acts on the
    /// target as it is now (its current path), and only if it's the same one.
    #[inline]
    pub fn confirm(self, fresh: T) -> Result<Confirmed<T, B>, Changed<T>> {
        if self.target.same_target(&fresh) {
            Ok(Confirmed { target: fresh, brand: self.brand })
        } else {
            Err(Changed { reviewed: self.target, found: fresh })
        }
    }
}

impl<T: fmt::Debug, B> fmt::Debug for Reviewed<T, B> {
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
/// let _ = Confirmed { target: "/dev/sdb", brand: () };
/// ```
#[must_use = "a confirmation is spent by the step it allows"]
pub struct Confirmed<T, B> {
    target: T,
    brand: B,
}

impl<T, B> Confirmed<T, B> {
    /// The target as it is now.
    #[inline]
    pub fn target(&self) -> &T {
        &self.target
    }

    /// The target as it is now, spending the confirmation (for the step it
    /// allows, which should then own the target).
    #[inline]
    pub fn into_target(self) -> T {
        self.target
    }

    /// The brand of the review it came from.
    #[inline]
    pub fn brand(&self) -> &B {
        &self.brand
    }
}

impl<T: fmt::Debug, B> fmt::Debug for Confirmed<T, B> {
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
    struct Brand;

    #[test]
    fn the_proof_carries_the_fresh_reading() {
        let c = Reviewed::new(D("/dev/sdb", 7), Brand).confirm(D("/dev/sdc", 7)).unwrap();
        assert_eq!(c.target(), &D("/dev/sdc", 7));
        assert_eq!(c.into_target(), D("/dev/sdc", 7));
    }

    #[test]
    fn another_target_is_refused_with_both_readings() {
        let e = Reviewed::new(D("/dev/sdb", 7), Brand).confirm(D("/dev/sdb", 8)).unwrap_err();
        assert_eq!((e.reviewed, e.found), (D("/dev/sdb", 7), D("/dev/sdb", 8)));
    }
}
