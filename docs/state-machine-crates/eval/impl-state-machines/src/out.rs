//! Effects: each context owns a `Vec<Effect>` the callbacks push to, and
//! each facade drains it into the caller's sink after the event.
//!
//! A `state_machine!` owns its context and generates no public accessor
//! for it (on the typed machine or on the `Dynamic*` wrapper), and its
//! context type has no lifetime parameter, so it can't borrow the sink for
//! one `handle`. The facades reach the buffer through the wrapper's private
//! `inner` field instead, which module privacy lets them see because the
//! macro expands in their module. One allocation per context, when the
//! machine is built; the `Vec` keeps its capacity, so none per event.

use spec::Sink;

/// Capacity each context's buffer starts with.
pub const CAPACITY: usize = 16;

#[inline]
pub fn drain<E>(buffer: &mut Vec<E>, sink: &mut impl Sink<E>) {
    for effect in buffer.drain(..) {
        sink.push(effect);
    }
}
