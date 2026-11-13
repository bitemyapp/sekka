//! The main thread's messages, handled a frame at a time.
//!
//! The render thread's loop waits on two things: these messages and the
//! compositor's events. calloop hands over a channel's messages for as long
//! as there are any, and a main thread that sends frames as fast as the
//! render thread draws them never lets the channel empty. The compositor's
//! events then wait for as long as that lasts: input, configures, and the
//! buffer releases without which a frame at a new size takes new memory
//! from the pool where it could have had a slot just freed. A window given
//! a new size on every frame grew the pool to 128 MiB that way, where it
//! holds 16 when the releases are read.
//!
//! So messages are only queued as they arrive, and handled in turns. A turn
//! ends with the first message that puts a frame on a surface, and the loop
//! looks for the compositor's events before the next turn.

use std::collections::VecDeque;
use std::collections::vec_deque::Drain;

/// Messages waiting for their turn, oldest first.
pub(super) struct Inbox<T> {
    waiting: VecDeque<T>,
    closed: bool,
}

impl<T> Default for Inbox<T> {
    fn default() -> Self {
        Inbox { waiting: VecDeque::new(), closed: false }
    }
}

impl<T> Inbox<T> {
    /// A message arrived.
    pub(super) fn push(&mut self, msg: T) {
        self.waiting.push_back(msg);
    }

    /// Whoever sent the messages is gone: no more will arrive.
    pub(super) fn close(&mut self) {
        self.closed = true;
    }

    /// Whether no message waits.
    pub(super) fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// Whether every message sent has had its turn and no more will come.
    pub(super) fn finished(&self) -> bool {
        self.closed && self.waiting.is_empty()
    }

    /// One turn's messages, oldest first: those waiting, up to and
    /// including the first that `ends` a turn, and all of them if none
    /// does. They leave the inbox with the turn, taken from it or not.
    pub(super) fn turn(&mut self, ends: impl Fn(&T) -> bool) -> Drain<'_, T> {
        let through = self.waiting.iter().position(ends).map_or(self.waiting.len(), |i| i + 1);
        self.waiting.drain(..through)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-ins for messages: a number, where 10 and up ends a turn as a
    /// present does.
    fn ends(n: &u32) -> bool {
        *n >= 10
    }

    fn turn(inbox: &mut Inbox<u32>) -> Vec<u32> {
        inbox.turn(ends).collect()
    }

    #[test]
    fn a_turn_ends_with_the_first_message_that_ends_one() {
        let mut inbox = Inbox::default();
        for n in [1, 2, 10, 3, 20, 30, 4] {
            inbox.push(n);
        }
        assert_eq!(turn(&mut inbox), [1, 2, 10]);
        assert_eq!(turn(&mut inbox), [3, 20]);
        assert_eq!(turn(&mut inbox), [30], "one that ends a turn is a turn of its own after another");
        assert_eq!(turn(&mut inbox), [4], "what's left when none ends a turn is one turn");
        assert!(inbox.is_empty());
        assert!(turn(&mut inbox).is_empty(), "and a turn with nothing waiting is empty");
    }

    #[test]
    fn messages_keep_their_order_whenever_they_arrive() {
        let mut inbox = Inbox::default();
        inbox.push(1);
        inbox.push(10);
        inbox.push(2);
        assert_eq!(turn(&mut inbox), [1, 10]);
        // More arrive between turns: they go behind what was waiting.
        inbox.push(3);
        inbox.push(11);
        inbox.push(4);
        assert_eq!(turn(&mut inbox), [2, 3, 11]);
        assert_eq!(turn(&mut inbox), [4]);
    }

    #[test]
    fn a_turn_takes_its_messages_out_even_when_none_is_looked_at() {
        let mut inbox = Inbox::default();
        for n in [1, 10, 2] {
            inbox.push(n);
        }
        drop(inbox.turn(ends));
        assert_eq!(turn(&mut inbox), [2]);
    }

    #[test]
    fn it_is_finished_once_closed_and_empty_and_not_before() {
        let mut inbox = Inbox::default();
        assert!(!inbox.finished(), "open, though empty");
        inbox.push(1);
        inbox.push(10);
        inbox.push(2);
        inbox.close();
        assert!(!inbox.finished(), "closed, with messages still to handle");
        assert_eq!(turn(&mut inbox), [1, 10]);
        assert!(!inbox.finished());
        assert_eq!(turn(&mut inbox), [2]);
        assert!(inbox.finished());
    }
}
