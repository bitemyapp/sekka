//! Conservative logical ownership charges, deliberately separate from RSS.
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use triomphe::Arc;

pub(crate) const ALLOCATION: usize = 32;
pub(crate) const MAX_DECODE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_LOGICAL_BLOCK_BYTES: usize = MAX_DECODE_BYTES / 2;

pub(crate) fn allocation(bytes: usize) -> usize {
    if bytes == 0 {
        0
    } else {
        bytes.saturating_add(ALLOCATION)
    }
}
pub(crate) fn vector<T>(capacity: usize) -> usize {
    allocation(capacity.saturating_mul(size_of::<T>()))
}
pub(crate) fn clone_vec<T: Clone>(source: &Vec<T>) -> Vec<T> {
    let mut result = Vec::with_capacity(source.capacity());
    result.extend(source.iter().cloned());
    result
}
/// The terminal owner is the only writer. Immutable readers may fill a row's
/// charge cache but never adjust this signal. Pausing groups resize/recycle
/// into one atomic ownership change, excluding temporary construction steps.
#[derive(Debug, Default)]
pub(crate) struct BudgetSignal {
    bytes: AtomicUsize,
    peak: AtomicUsize,
    dirty: AtomicBool,
    paused: AtomicBool,
}
impl BudgetSignal {
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }
    pub fn rebase(&self, bytes: usize) {
        self.bytes.store(bytes, Ordering::Relaxed);
        self.peak.store(bytes, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Relaxed);
        self.resume();
    }
    pub fn adjust(&self, old: usize, new: usize) {
        if old == new || self.paused.load(Ordering::Relaxed) {
            return;
        }
        // Only the terminal owner updates this signal; immutable consumers
        // may fill row charge caches but never write the active accounting.
        let current = self.bytes.load(Ordering::Relaxed);
        let total = if new > old {
            current + (new - old)
        } else {
            current - (old - new)
        };
        self.bytes.store(total, Ordering::Relaxed);
        self.peak.store(
            self.peak.load(Ordering::Relaxed).max(total),
            Ordering::Relaxed,
        );
        self.dirty.store(true, Ordering::Relaxed);
    }
    pub fn current(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }
    pub fn take_change(&self) -> Option<(usize, usize)> {
        if !self.dirty.load(Ordering::Relaxed) {
            return None;
        }
        self.dirty.store(false, Ordering::Relaxed);
        let current = self.current();
        let peak = self.peak.load(Ordering::Relaxed);
        self.peak.store(current, Ordering::Relaxed);
        Some((current, peak.max(current)))
    }
}
#[derive(Debug, Default)]
pub(crate) struct Charge {
    value: AtomicUsize,
    signal: Option<Arc<BudgetSignal>>,
}
impl Clone for Charge {
    fn clone(&self) -> Self {
        Self {
            value: AtomicUsize::new(self.value.load(Ordering::Relaxed)),
            signal: self.signal.clone(),
        }
    }
}
impl Charge {
    pub fn attached_to(&self, signal: &Arc<BudgetSignal>) -> bool {
        self.signal
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, signal))
    }
    pub fn attach(&mut self, signal: &Arc<BudgetSignal>) {
        self.signal = Some(signal.clone());
    }
    pub fn update(&mut self, bytes: usize) {
        let old = *self.value.get_mut();
        *self.value.get_mut() = bytes;
        if let Some(signal) = &self.signal {
            signal.adjust(old, bytes);
        }
    }
    pub fn get(&self, compute: impl FnOnce() -> usize) -> usize {
        let old = self.value.load(Ordering::Relaxed);
        if old != 0 {
            return old;
        }
        let bytes = compute();
        self.value.store(bytes, Ordering::Relaxed);
        bytes
    }
}
