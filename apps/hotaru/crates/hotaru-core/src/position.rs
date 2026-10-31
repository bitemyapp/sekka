//! Stable ownership identity and coordinates for asynchronous content readers.
use triomphe::Arc;

/// Opaque screen-layout identity. Equality compares owned identity, not values
/// or an address whose allocation could be reused while a capture still lives.
#[derive(Clone, Debug)]
pub struct ContentEpoch(Arc<()>);

impl Default for ContentEpoch {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}
impl PartialEq for ContentEpoch {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ContentEpoch {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentPosition {
    /// Changes on reflow, history clear, reset or screen replacement. Returning
    /// from the alternate screen preserves the primary screen's identity.
    pub epoch: ContentEpoch,
    /// Number of historical rows removed since this epoch began.
    pub origin: u64,
    /// First captured/displayed row, relative to the current history origin.
    pub first_row: usize,
    /// Current historical row count, excluding the active screen.
    pub history_rows: usize,
    pub active_rows: usize,
}

impl ContentPosition {
    pub(crate) fn absolute_row(&self, relative_row: usize) -> Option<u64> {
        self.origin
            .checked_add(u64::try_from(self.first_row).ok()?)?
            .checked_add(u64::try_from(relative_row).ok()?)
    }
}
