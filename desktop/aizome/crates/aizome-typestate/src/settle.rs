//! Values that must be settled: spent through a named step, or the fallback
//! runs when they're dropped.

use core::fmt;

/// A value that must be settled through a named step, or else its fallback
/// runs when it's dropped: a restore point that restores unless the change
/// was committed, deleted generations whose boot menu is rewritten unless
/// the step that does it ran.
///
/// Rust has no linear types, so this is a runtime fallback for the paths the
/// types can't close: an early `?`, a panic. Give the transaction a consuming
/// step (`commit(self)`) that calls [`settle`](Self::settle), so the normal
/// path is the typed one, and let the fallback cover the rest.
///
/// ```
/// use aizome_typestate::Unsettled;
/// use std::cell::Cell;
/// let restored = Cell::new(false);
/// {
///     let point = Unsettled::new("backup", |_| restored.set(true));
///     let _ = &point; // an early return here drops it
/// }
/// assert!(restored.get());
///
/// restored.set(false);
/// let point = Unsettled::new("backup", |_| restored.set(true));
/// assert_eq!(point.settle(), "backup");
/// assert!(!restored.get());
/// ```
#[must_use = "dropping it runs the fallback"]
pub struct Unsettled<T, F: FnOnce(T)> {
    value: Option<T>,
    fallback: Option<F>,
}

impl<T, F: FnOnce(T)> Unsettled<T, F> {
    #[inline]
    pub fn new(value: T, fallback: F) -> Self {
        Unsettled { value: Some(value), fallback: Some(fallback) }
    }

    /// Settles it: the fallback won't run, and the value is the caller's.
    #[inline]
    pub fn settle(mut self) -> T {
        self.fallback = None;
        self.value.take().expect("settled once")
    }

    #[inline]
    pub fn get(&self) -> &T {
        self.value.as_ref().expect("not settled")
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.value.as_mut().expect("not settled")
    }
}

impl<T, F: FnOnce(T)> Drop for Unsettled<T, F> {
    fn drop(&mut self) {
        if let (Some(value), Some(fallback)) = (self.value.take(), self.fallback.take()) {
            fallback(value);
        }
    }
}

impl<T: fmt::Debug, F: FnOnce(T)> fmt::Debug for Unsettled<T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Unsettled").field(&self.value).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    #[test]
    fn the_fallback_gets_the_value() {
        let seen = Cell::new(0);
        drop(Unsettled::new(7, |v| seen.set(v)));
        assert_eq!(seen.get(), 7);
    }

    #[test]
    fn settling_disarms_it() {
        let seen = Cell::new(0);
        let mut u = Unsettled::new(7, |v| seen.set(v));
        *u.get_mut() += 1;
        assert_eq!(u.settle(), 8);
        assert_eq!(seen.get(), 0);
    }
}
